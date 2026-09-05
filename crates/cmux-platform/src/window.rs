//! Safe GTK-facing window placement; Wayland placement remains compositor-owned.

use gtk4::prelude::*;

extern "C" {
    /// Read or restore coordinates for a live GDK X11 surface on the GTK thread.
    fn cmux_window_position(
        surface: *mut gtk4::gdk::ffi::GdkSurface,
        x: *mut i32,
        y: *mut i32,
        restore: i32,
    ) -> i32;
}

/// Read root-window coordinates, returning None for Wayland or native failure.
/// Call on the GTK main thread with a live surface.
pub fn position(surface: &gtk4::gdk::Surface) -> Option<(i32, i32)> {
    let (mut x, mut y) = (0, 0);
    // SAFETY: the borrowed GDK surface outlives the call; output pointers refer
    // to exclusive initialized stack integers. The bridge checks the backend.
    let success = unsafe { cmux_window_position(surface.as_ptr(), &mut x, &mut y, 0) };
    (success != 0).then_some((x, y))
}

/// Request root-window coordinates; return false when placement is unsupported.
/// Call on the GTK main thread. The window manager may override the request.
pub fn restore_position(surface: &gtk4::gdk::Surface, x: i32, y: i32) -> bool {
    let (mut x, mut y) = (x, y);
    // SAFETY: the borrowed GDK surface and exclusive coordinate values remain
    // valid for the call. The bridge checks X11 before accessing native handles.
    unsafe { cmux_window_position(surface.as_ptr(), &mut x, &mut y, 1) != 0 }
}
