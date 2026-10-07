//! Private Windows handle, user and access-control helpers.

use std::{io, os::windows::ffi::OsStrExt, ptr};
use windows_sys::Win32::{
    Foundation::*,
    Security::{Authorization::*, *},
    System::Threading::*,
};

pub(crate) struct Handle(pub HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: this wrapper exclusively owns a valid kernel handle.
        unsafe {
            CloseHandle(self.0);
        }
    }
}

pub(crate) fn wide(value: &std::ffi::OsStr) -> Vec<u16> {
    value.encode_wide().chain(Some(0)).collect()
}

pub(crate) fn open_process(pid: u32) -> io::Result<Handle> {
    // SAFETY: only queries a process; no borrowed pointers or inherited handle.
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle.is_null() {
        Err(io::Error::last_os_error())
    } else {
        Ok(Handle(handle))
    }
}

pub(crate) fn process_sid(process: HANDLE) -> io::Result<String> {
    // SAFETY: handles are live; output buffers are owned and correctly aligned.
    unsafe {
        let mut token = ptr::null_mut();
        if OpenProcessToken(process, TOKEN_QUERY, &mut token) == 0 {
            return Err(io::Error::last_os_error());
        }
        let token = Handle(token);
        let mut size = 0;
        GetTokenInformation(token.0, TokenUser, ptr::null_mut(), 0, &mut size);
        if size == 0 || size > 65536 {
            return Err(io::Error::last_os_error());
        }
        let mut storage = vec![0usize; (size as usize).div_ceil(std::mem::size_of::<usize>())];
        if GetTokenInformation(
            token.0,
            TokenUser,
            storage.as_mut_ptr().cast(),
            size,
            &mut size,
        ) == 0
        {
            return Err(io::Error::last_os_error());
        }
        let user = &*storage.as_ptr().cast::<TOKEN_USER>();
        sid_string(user.User.Sid)
    }
}

/// Convert a borrowed valid SID; the Windows allocation is released after copying.
unsafe fn sid_string(sid: PSID) -> io::Result<String> {
    // SAFETY: the caller owns a valid SID; conversion returns a LocalAlloc string.
    unsafe {
        let mut string = ptr::null_mut();
        if ConvertSidToStringSidW(sid, &mut string) == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut length = 0;
        while *string.add(length) != 0 {
            length += 1;
        }
        let result = String::from_utf16_lossy(std::slice::from_raw_parts(string, length));
        LocalFree(string.cast());
        Ok(result)
    }
}

pub(crate) fn current_sid() -> io::Result<String> {
    // SAFETY: the pseudo-handle is borrowed, never closed.
    process_sid(unsafe { GetCurrentProcess() })
}

pub(crate) struct PrivateSecurity(PSECURITY_DESCRIPTOR);
impl PrivateSecurity {
    pub(crate) fn new() -> io::Result<Self> {
        let text = wide(std::ffi::OsStr::new(&format!(
            "D:P(A;OICI;FA;;;{})",
            current_sid()?
        )));
        let mut descriptor = ptr::null_mut();
        // SAFETY: text is NUL terminated; output points to an owned LocalAlloc descriptor.
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                text.as_ptr(),
                1,
                &mut descriptor,
                ptr::null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(descriptor))
    }

    pub(crate) fn attributes(&self) -> SECURITY_ATTRIBUTES {
        SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: self.0,
            bInheritHandle: 0,
        }
    }

    pub(crate) fn restrict(&self, path: &std::path::Path) -> io::Result<()> {
        let name = wide(path.as_os_str());
        let (mut present, mut defaulted, mut acl) = (0, 0, ptr::null_mut());
        // SAFETY: descriptor and path remain live, output pointers are exclusive locals.
        let result = unsafe {
            if GetSecurityDescriptorDacl(self.0, &mut present, &mut acl, &mut defaulted) == 0 {
                return Err(io::Error::last_os_error());
            }
            SetNamedSecurityInfoW(
                name.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                ptr::null_mut(),
                ptr::null_mut(),
                acl,
                ptr::null(),
            )
        };
        if result == 0 {
            Ok(())
        } else {
            Err(io::Error::from_raw_os_error(result as i32))
        }
    }
}
impl Drop for PrivateSecurity {
    fn drop(&mut self) {
        // SAFETY: descriptor was allocated by the Windows SDDL converter.
        unsafe {
            LocalFree(self.0);
        }
    }
}

/// Require an owner-only DACL on the opened file, without trusting its current pathname.
pub(crate) fn private_file(handle: HANDLE) -> io::Result<bool> {
    let (mut owner, mut acl, mut descriptor) = (ptr::null_mut(), ptr::null_mut(), ptr::null_mut());
    // SAFETY: handle is borrowed and output pointers reference exclusive initialized locals.
    unsafe {
        let status = GetSecurityInfo(
            handle,
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &mut owner,
            ptr::null_mut(),
            &mut acl,
            ptr::null_mut(),
            &mut descriptor,
        );
        if status != 0 {
            return Err(io::Error::from_raw_os_error(status as i32));
        }
        let expected = current_sid();
        let valid = (|| -> io::Result<bool> {
            let expected = expected?;
            if owner.is_null()
                || acl.is_null()
                || sid_string(owner)? != expected
                || (*acl).AceCount != 1
            {
                return Ok(false);
            }
            let mut ace = ptr::null_mut();
            if GetAce(acl, 0, &mut ace) == 0 {
                return Err(io::Error::last_os_error());
            }
            let ace = &*ace.cast::<ACCESS_ALLOWED_ACE>();
            Ok(u32::from(ace.Header.AceType)
                == windows_sys::Win32::System::SystemServices::ACCESS_ALLOWED_ACE_TYPE
                && sid_string((&ace.SidStart as *const u32).cast_mut().cast())? == expected)
        })();
        LocalFree(descriptor);
        valid
    }
}

pub(crate) fn filetime(value: FILETIME) -> u64 {
    (u64::from(value.dwHighDateTime) << 32) | u64::from(value.dwLowDateTime)
}

pub(crate) fn process_start(handle: HANDLE) -> io::Result<u64> {
    let (mut creation, mut exit, mut kernel, mut user) =
        unsafe { std::mem::zeroed::<(FILETIME, FILETIME, FILETIME, FILETIME)>() };
    // SAFETY: handle is borrowed; all outputs are properly sized stack structures.
    if unsafe { GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let mut code = 0;
    // SAFETY: the same live handle is used to check whether the process still runs.
    if unsafe { GetExitCodeProcess(handle, &mut code) } == 0 {
        return Err(io::Error::last_os_error());
    }
    if code != STILL_ACTIVE as u32 {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "process has exited",
        ));
    }
    Ok(filetime(creation))
}
