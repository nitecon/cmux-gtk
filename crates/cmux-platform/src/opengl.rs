//! Linux OpenGL callbacks for renderers hosted inside a GTK GLArea.
//!
//! GTK owns context lifetime and presentation. External renderers resolve desktop
//! GL entry points through libGL, but must leave GTK's context current after drawing.

use std::ffi::{c_char, c_void};
use std::sync::OnceLock;

/// Bounded driver labels from the first current terminal OpenGL context in this process.
pub struct RendererInfo {
    /// GL implementation vendor, or None for missing/invalid/oversized labels.
    pub vendor: Option<String>,
    /// Driver-reported renderer label; not a unique hardware identifier.
    pub renderer: Option<String>,
    /// GL version including driver suffix when provided.
    pub version: Option<String>,
}

static RENDERER: OnceLock<RendererInfo> = OnceLock::new();

/// Read captured labels without querying GTK or GL; None means no context has been observed yet.
pub fn renderer_info() -> Option<&'static RendererInfo> {
    RENDERER.get()
}

#[link(name = "GL")]
extern "C" {
    /// Resolve a desktop GL entry point through the Linux GL dispatcher.
    fn glXGetProcAddressARB(name: *const u8) -> *mut c_void;
    /// Read a driver-owned NUL-terminated label while a GL context is current.
    fn glGetString(name: u32) -> *const u8;
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
            capture_renderer();
            return true;
        }
        gtk4::ffi::gtk_gl_area_make_current(area);
        let ready = gtk4::ffi::gtk_gl_area_get_error(area).is_null();
        if ready && !gtk4::gdk::ffi::gdk_gl_context_get_current().is_null() {
            capture_renderer();
        }
        ready
    }
}

/// Query labels only once, copying borrowed driver storage while the context remains current.
/// # Safety
/// The calling thread must own a current desktop OpenGL context throughout this call.
unsafe fn capture_renderer() {
    RENDERER.get_or_init(|| {
        // SAFETY: the caller keeps a current GL context; valid enum values return static C strings or null.
        unsafe {
            RendererInfo {
                vendor: read_label(glGetString(0x1F00)),
                renderer: read_label(glGetString(0x1F01)),
                version: read_label(glGetString(0x1F02)),
            }
        }
    });
}

/// Copy at most 256 UTF-8 bytes, rejecting empty, oversized or control-containing labels.
/// # Safety
/// A non-null pointer must reference a readable NUL-terminated byte string for this call.
unsafe fn read_label(value: *const u8) -> Option<String> {
    if value.is_null() {
        return None;
    }
    let mut bytes = Vec::with_capacity(256);
    for offset in 0..=256 {
        // SAFETY: sequential reads stop at the first NUL within the caller-owned C string.
        let byte = unsafe { *value.add(offset) };
        if byte == 0 {
            let label = String::from_utf8(bytes).ok()?;
            return (!label.trim().is_empty() && !label.chars().any(char::is_control))
                .then_some(label);
        }
        if offset == 256 {
            return None;
        }
        bytes.push(byte);
    }
    None
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    /// Copy bounded owned C strings and reject malformed identity labels without reading past NUL.
    #[test]
    fn bounds_driver_labels() {
        for (bytes, expected) in [
            ("Mesa λ".as_bytes().to_vec(), Some("Mesa λ".to_owned())),
            (vec![b'x'; 256], Some("x".repeat(256))),
            (vec![b'x'; 257], None),
            (vec![0xff], None),
            (b"line\nlabel".to_vec(), None),
            (b"   ".to_vec(), None),
            (vec![], None),
        ] {
            let value = CString::new(bytes).unwrap();
            // SAFETY: owned CString remains live for the complete bounded read.
            assert_eq!(unsafe { read_label(value.as_ptr().cast()) }, expected);
        }
        assert!(unsafe { read_label(std::ptr::null()) }.is_none());
    }
}
