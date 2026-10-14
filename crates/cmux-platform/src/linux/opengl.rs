//! Linux GL dispatcher used by the shared GTK context callbacks.

use std::ffi::{c_char, c_void};

#[link(name = "GL")]
extern "C" {
    /// Resolve a desktop GL entry point through the Linux GL dispatcher.
    fn glXGetProcAddressARB(name: *const u8) -> *mut c_void;
    /// Read a driver-owned NUL-terminated label while a GL context is current.
    pub(super) fn glGetString(name: u32) -> *const u8;
}

/// Linux retains the existing GTK caller contract without an additional thread registry.
pub(super) fn register_gtk_thread() {}

/// Leave GTK thread validation to the callback's existing safety contract on Linux.
pub(super) fn on_gtk_thread() -> bool {
    true
}

/// GDK's current context is sufficient on Linux; no extra native-context check is needed.
pub(super) fn has_native_context() -> bool {
    true
}

/// Resolve a borrowed NUL-terminated name; GL does not retain it.
/// # Safety
/// The name must be readable for this call; the caller owns compatible context selection.
pub(super) unsafe fn get_proc_address(name: *const c_char) -> *mut c_void {
    // SAFETY: the caller guarantees the readable C name throughout dispatcher lookup.
    unsafe { glXGetProcAddressARB(name.cast()) }
}
