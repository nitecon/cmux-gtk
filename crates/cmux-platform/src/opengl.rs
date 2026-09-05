//! Linux OpenGL callbacks for renderers hosted inside a GTK GLArea.
//!
//! GTK owns context lifetime and presentation. External renderers resolve desktop
//! GL entry points through libGL, but must leave GTK's context current after drawing.

use std::ffi::{c_char, c_void};

#[link(name = "GL")]
extern "C" {
    /// Resolve a desktop GL entry point through the Linux GL dispatcher.
    fn glXGetProcAddressARB(name: *const u8) -> *mut c_void;
}

/// Make the hosting GLArea's context current, retaining an already-current context.
///
/// # Safety
/// Non-null `userdata` must point to a live GtkGLArea on its owning GTK thread.
/// The caller must keep the widget alive throughout this callback.
pub unsafe extern "C" fn make_current(userdata: *mut c_void) -> bool {
    if userdata.is_null() {
        return false;
    }
    let area = userdata.cast();
    // SAFETY: The callback contract guarantees a live GLArea on its GTK thread.
    // Context pointers remain borrowed from GTK and are used only during this call.
    unsafe {
        let context = gtk4::ffi::gtk_gl_area_get_context(area);
        if !context.is_null() && gtk4::gdk::ffi::gdk_gl_context_get_current() == context {
            return true;
        }
        gtk4::ffi::gtk_gl_area_make_current(area);
        gtk4::ffi::gtk_gl_area_get_error(area).is_null()
    }
}

/// Leave the context current for GTK's post-render compositing and libepoxy dispatch.
pub extern "C" fn clear_current(_userdata: *mut c_void) {}

/// Look up a desktop GL function for the renderer; a null name returns null.
///
/// # Safety
/// A non-null `name` must reference a readable NUL-terminated C string for this call.
/// The returned address must only be called with the corresponding GL signature
/// and with a compatible current context.
pub unsafe extern "C" fn get_proc_address(
    _userdata: *mut c_void,
    name: *const c_char,
) -> *mut c_void {
    if name.is_null() {
        return std::ptr::null_mut();
    }
    // SAFETY: The caller supplies a readable C name; libGL does not retain it.
    unsafe { glXGetProcAddressARB(name.cast()) }
}

/// Leave presentation to GtkGLArea after the render signal returns.
pub extern "C" fn swap_buffers(_userdata: *mut c_void) {}
