//! Bounded ownership adapter for native terminal TTY metadata.
use super::ffi;

/// Read one live native TTY allocation on GTK and release it before returning owned metadata.
///
/// # Safety
/// The caller must keep the surface alive on its owning GTK thread throughout this non-callback getter.
pub(crate) unsafe fn name(surface: ffi::ghostty_surface_t) -> Option<String> {
    // SAFETY: caller guarantees native surface lifetime and GTK ownership.
    let value = unsafe { ffi::ghostty_surface_tty_name(surface) };
    let result = if value.ptr.is_null() || value.len == 0 || value.len > 256 {
        None
    } else {
        // SAFETY: native getter owns a readable allocation of the returned length until string_free.
        std::str::from_utf8(unsafe {
            std::slice::from_raw_parts(value.ptr.cast::<u8>(), value.len)
        })
        .ok()
        .map(str::to_owned)
    };
    // SAFETY: this is the sole release of the allocation returned by the native getter (empty is valid).
    unsafe { ffi::ghostty_string_free(value) };
    result
}
