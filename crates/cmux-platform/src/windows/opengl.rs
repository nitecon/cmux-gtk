//! Windows WGL lookup and GTK owner-thread checks for shared rendering callbacks.

use std::ffi::{c_char, c_void};
use std::sync::OnceLock;
use windows_sys::Win32::{
    Graphics::OpenGL::{wglGetCurrentContext, wglGetProcAddress},
    System::LibraryLoader::{GetModuleHandleA, GetProcAddress},
};

static GTK_THREAD: OnceLock<std::thread::ThreadId> = OnceLock::new();

#[link(name = "opengl32")]
extern "system" {
    /// Read a borrowed driver label while a desktop GL context is current.
    pub(super) fn glGetString(name: u32) -> *const u8;
}

/// Record the first GTK owner thread before native renderer workers start.
pub(super) fn register_gtk_thread() {
    GTK_THREAD.get_or_init(|| std::thread::current().id());
}

/// Reject renderer callbacks on any thread other than GTK's registered owner.
pub(super) fn on_gtk_thread() -> bool {
    GTK_THREAD.get() == Some(&std::thread::current().id())
}

/// Require WGL to agree that the calling thread has a current native context.
pub(super) fn has_native_context() -> bool {
    // SAFETY: this only queries the calling thread's context without retaining it.
    unsafe { !wglGetCurrentContext().is_null() }
}

/// Resolve WGL extension functions, then fall back to the desktop OpenGL DLL exports.
/// # Safety
/// The name must be a readable NUL-terminated string and a desktop GL context must be current.
pub(super) unsafe fn get_proc_address(name: *const c_char) -> *mut c_void {
    // SAFETY: the caller keeps its GL context and borrowed C name live for both lookups.
    unsafe {
        let pointer = wglGetProcAddress(name.cast())
            .map(|function| function as *mut c_void)
            .unwrap_or(std::ptr::null_mut());
        if !matches!(pointer as isize, 0 | 1 | 2 | 3 | -1) {
            return pointer;
        }
        let module = GetModuleHandleA(c"opengl32.dll".as_ptr().cast());
        if module.is_null() {
            return std::ptr::null_mut();
        }
        GetProcAddress(module, name.cast())
            .map(|function| function as *mut c_void)
            .unwrap_or(std::ptr::null_mut())
    }
}
