//! Ghostty callback boundary: coalesced wakeups, validated action targets and deferred GTK mutation.

use gtk4::ffi;
use gtk4::prelude::{GLAreaExt, WidgetExt};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, Mutex};

/// Coalesces burst wakeup calls into a single GLib idle dispatch.
/// GLib's idle_add does not deduplicate — this flag prevents queueing N
/// idle tasks when Ghostty fires N wakeups in a single frame burst.
pub static WAKEUP_PENDING: AtomicBool = AtomicBool::new(false);

/// The GhosttyApp handle — stored as usize to be Send across the idle closure.
/// Safety: ghostty_app_t is opaque void* and only called from the GLib main thread.
pub static APP_PTR: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Wakeup count for diagnostic logging (only logs occasionally to avoid spam)
static WAKEUP_COUNT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Coalesce renderer-thread wakeups into a GTK-thread Ghostty tick without inline native calls.
/// Userdata is ignored; no native pointer is retained by the idle closure.
///
/// # Safety
/// Install only while the application's GTK main context is available. APP_PTR must
/// identify a live Ghostty application or be cleared before native teardown.
pub unsafe extern "C" fn wakeup_cb(_userdata: *mut std::ffi::c_void) {
    // Swap: if already pending, another idle task is queued — skip.
    if WAKEUP_PENDING.swap(true, Ordering::SeqCst) {
        return;
    }
    glib::idle_add_once(|| {
        WAKEUP_PENDING.store(false, Ordering::SeqCst);

        // Log wakeup occasionally to verify it's firing
        let count = WAKEUP_COUNT.fetch_add(1, Ordering::SeqCst) + 1;
        if count % 60 == 1 {
            eprintln!("cmux: wakeup_cb #{}", count);
        }

        let app_ptr = APP_PTR.load(Ordering::SeqCst);
        if app_ptr != 0 {
            unsafe {
                let app = app_ptr as crate::ghostty::ffi::ghostty_app_t;
                crate::ghostty::ffi::ghostty_app_tick(app);
            }
        }
        // app_tick dispatches targeted RENDER actions. A mailbox wakeup can
        // also be title/clipboard/scrollbar work; it does not itself dirty every
        // terminal in every workspace.
    });
}

/// Maps GLArea raw pointer (as usize) → surface pointer (as usize).
/// GTK-thread owners remove entries before freeing widgets or native surfaces.
pub static GL_TO_SURFACE: LazyLock<Mutex<HashMap<usize, usize>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Record a native close request on GTK without closing its tab or freeing its surface.
/// Explicit pane operations currently own teardown; shell exit alone leaves the tab open.
/// Userdata and the process-alive hint are ignored.
///
/// # Safety
/// Register with Ghostty's close callback ABI. No userdata pointer is dereferenced.
pub unsafe extern "C" fn close_surface_cb(_userdata: *mut std::ffi::c_void, _process_alive: bool) {
    eprintln!("cmux: native close requested; tab awaits explicit pane teardown");
}

/// Handle directory, bell, terminal-tab and render actions from Ghostty on the GTK thread.
/// Directory payloads are copied before returning. Model mutations use the bounded
/// native event queue; redraws only schedule painting. Return false for unsupported
/// actions or queue rejection, and true for handled or stale owned-action targets.
///
/// # Safety
/// Ghostty must supply valid tagged unions. A PWD payload must point to a live
/// NUL-terminated string for this call. The native application and widget registry
/// must remain live on the GTK thread; action payload pointers are never retained.
pub unsafe extern "C" fn action_cb(
    _app: crate::ghostty::ffi::ghostty_app_t,
    target: crate::ghostty::ffi::ghostty_target_s,
    action: crate::ghostty::ffi::ghostty_action_s,
) -> bool {
    use crate::ghostty::ffi;

    let surface_target = if target.tag == ffi::ghostty_target_tag_e_GHOSTTY_TARGET_SURFACE {
        // SAFETY: the target discriminant selects the surface member. Treat the
        // opaque value only as a registry identity; do not dereference it.
        Some(unsafe { target.target.surface } as usize)
    } else {
        None
    };

    // Raw output already queues bounded OSC messages before native desktop throttling/truncation.
    // Acknowledge this presentation callback without creating a duplicate inbox record.
    if action.tag == ffi::ghostty_action_tag_e_GHOSTTY_ACTION_DESKTOP_NOTIFICATION {
        return true;
    }

    if action.tag == ffi::ghostty_action_tag_e_GHOSTTY_ACTION_PWD {
        if let Some(surface) = surface_target {
            // SAFETY: The discriminants above select these union members. Ghostty
            // owns the NUL-terminated directory for this callback; copy it before returning.
            unsafe {
                let directory = action.action.pwd.pwd;
                if !directory.is_null() {
                    let directory = std::ffi::CStr::from_ptr(directory).to_string_lossy();
                    crate::ghostty::registry::set_working_directory(surface, &directory);
                }
            }
        }
        return true;
    }

    if action.tag == ffi::ghostty_action_tag_e_GHOSTTY_ACTION_RING_BELL
        || action.tag == ffi::ghostty_action_tag_e_GHOSTTY_ACTION_NEW_TAB
    {
        if let Some(pane_id) = surface_target.and_then(crate::ghostty::registry::pane_id) {
            let event = if action.tag == ffi::ghostty_action_tag_e_GHOSTTY_ACTION_RING_BELL {
                super::events::Event::Bell(pane_id)
            } else {
                super::events::Event::NewTerminalTab(pane_id)
            };
            return super::events::push(event);
        }
        return true;
    }

    if action.tag == ffi::ghostty_action_tag_e_GHOSTTY_ACTION_RENDER {
        queue_render_target(surface_target);
        return true;
    }
    // Leave unsupported actions to the native caller.
    false
}

/// Route native redraws on the GTK thread without retaining registry locks during GTK calls.
/// Surface requests avoid allocation; application requests snapshot the currently owned widgets.
fn queue_render_target(target: Option<usize>) {
    if let Some(target) = target {
        let area = GL_TO_SURFACE.lock().ok().and_then(|mappings| {
            mappings
                .iter()
                .find(|(_, surface)| **surface == target)
                .map(|(area, _)| *area)
        });
        if let Some(area) = area {
            queue_mapped_area(area);
        }
    } else {
        let areas: Vec<usize> = GL_TO_SURFACE
            .lock()
            .ok()
            .map(|mappings| mappings.keys().copied().collect())
            .unwrap_or_default();
        for area in areas {
            queue_mapped_area(area);
        }
    }
}

/// Schedule painting for a live registered widget on GTK; hidden tabs need no frame.
fn queue_mapped_area(pointer: usize) {
    // SAFETY: the caller obtained this pointer from the live registry on the GTK
    // thread. No event-loop iteration or native callback occurs between lookup and
    // use; widget teardown also runs on this thread. queue_render only schedules work.
    let area: glib::translate::Borrowed<gtk4::GLArea> =
        unsafe { glib::translate::from_glib_borrow(pointer as *mut ffi::GtkGLArea) };
    if area.is_mapped() {
        area.queue_render();
    }
}
