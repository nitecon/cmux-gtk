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

/// Called by Ghostty when a surface wants to close (e.g. shell exits).
/// Runs on the GLib main thread (called during ghostty_app_tick).
/// Records the notification; pane lifecycle code owns the actual surface teardown.
pub unsafe extern "C" fn close_surface_cb(_userdata: *mut std::ffi::c_void, _process_alive: bool) {
    eprintln!("cmux: close_surface_cb fired — per-pane close will be handled by AppState");
}

/// Action callback — Ghostty fires actions (e.g. new tab, font size changes).
/// Handles the `.render` action to trigger a GtkGLArea redraw on the main thread.
/// This is required because must_draw_from_app_thread=true in embedded.zig means
/// the renderer thread sends redraw_surface → App.redrawSurface → action_cb(.render).
/// Returns true if handled, false otherwise.
pub unsafe extern "C" fn action_cb(
    _app: crate::ghostty::ffi::ghostty_app_t,
    _target: crate::ghostty::ffi::ghostty_target_s,
    action: crate::ghostty::ffi::ghostty_action_s,
) -> bool {
    use crate::ghostty::ffi;

    if action.tag == ffi::ghostty_action_tag_e_GHOSTTY_ACTION_PWD {
        if _target.tag == ffi::ghostty_target_tag_e_GHOSTTY_TARGET_SURFACE {
            // SAFETY: The discriminants above select these union members. Ghostty
            // owns the NUL-terminated directory for this callback; copy it before returning.
            unsafe {
                let directory = action.action.pwd.pwd;
                if !directory.is_null() {
                    let directory = std::ffi::CStr::from_ptr(directory).to_string_lossy();
                    crate::ghostty::registry::set_working_directory(
                        _target.target.surface as usize,
                        &directory,
                    );
                }
            }
        }
        return true;
    }

    // Defer attention mutation until GTK is outside the native callback.
    if action.tag == ffi::ghostty_action_tag_e_GHOSTTY_ACTION_RING_BELL {
        if _target.tag == ffi::ghostty_target_tag_e_GHOSTTY_TARGET_SURFACE {
            let surface_ptr = unsafe { _target.target.surface } as usize;
            let pane_id = crate::ghostty::registry::pane_id(surface_ptr);
            if let Some(pane_id) = pane_id {
                return super::events::push(super::events::Event::Bell(pane_id));
            }
        }
        return true;
    }

    if action.tag == ffi::ghostty_action_tag_e_GHOSTTY_ACTION_NEW_TAB {
        if _target.tag == ffi::ghostty_target_tag_e_GHOSTTY_TARGET_SURFACE {
            let surface_ptr = unsafe { _target.target.surface } as usize;
            if let Some(pane_id) = crate::ghostty::registry::pane_id(surface_ptr) {
                return super::events::push(super::events::Event::NewTerminalTab(pane_id));
            }
        }
        return true;
    }

    if action.tag == ffi::ghostty_action_tag_e_GHOSTTY_ACTION_RENDER {
        // Trigger a render on the GLArea — will call ghostty_surface_draw on main thread.
        let target = if _target.tag == ffi::ghostty_target_tag_e_GHOSTTY_TARGET_SURFACE {
            Some(unsafe { _target.target.surface } as usize)
        } else {
            None
        };
        queue_render_target(target);
        return true;
    }
    // Phase 1 ignores all other actions — return false (unhandled)
    false
}


/// Route native redraws on the GTK thread without retaining registry locks during GTK calls.
/// Surface requests avoid allocation; application requests snapshot the currently owned widgets.
fn queue_render_target(target: Option<usize>) {
    if let Some(target) = target {
        let area = GL_TO_SURFACE.lock().ok().and_then(|mappings| {
            mappings.iter().find(|(_, surface)| **surface == target).map(|(area, _)| *area)
        });
        if let Some(area) = area {
            queue_mapped_area(area);
        }
    } else {
        let areas: Vec<usize> = GL_TO_SURFACE.lock().ok()
            .map(|mappings| mappings.keys().copied().collect()).unwrap_or_default();
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
