use gtk4::prelude::*;
use std::{cell::RefCell, path::PathBuf, rc::Rc};

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
struct WindowState {
    width: i32,
    height: i32,
    maximized: bool,
    position: Option<(i32, i32)>,
}

impl Default for WindowState {
    /// Use a normal 800×600 window when no valid saved geometry exists.
    fn default() -> Self {
        Self {
            width: 800,
            height: 600,
            maximized: false,
            position: None,
        }
    }
}

/// Locate persisted geometry beside the workspace session.
fn path() -> PathBuf {
    crate::session::session_path().with_file_name("window-state.json")
}

/// Decode geometry with defaults and clamp invalid dimensions to usable bounds.
fn read(contents: &str) -> WindowState {
    let mut state: WindowState = serde_json::from_str(contents).unwrap_or_default();
    state.width = state.width.clamp(320, 16384);
    state.height = state.height.clamp(240, 16384);
    state
}

/// Persist changed normal geometry on the GTK thread, retaining prior placement while maximized.
fn capture(window: &gtk4::ApplicationWindow, previous: &Rc<RefCell<WindowState>>) {
    if !window.is_mapped() {
        return;
    }
    let mut state = previous.borrow().clone();
    state.maximized = window.is_maximized();
    if !state.maximized && !window.is_fullscreen() {
        // GTK retains the normal dimensions independently of maximized size.
        let (width, height) = window.default_size();
        if width > 0 && height > 0 {
            state.width = width;
            state.height = height;
        }
        if let Some(surface) = window.surface() {
            if let Some(position) = cmux_platform::window::position(&surface) {
                state.position = Some(position);
            }
        }
    }
    if state == *previous.borrow() {
        return;
    }
    let save = || -> Result<(), Box<dyn std::error::Error>> {
        cmux_platform::filesystem::atomic_write(&path(), &serde_json::to_vec_pretty(&state)?)?;
        Ok(())
    };
    match save() {
        Ok(()) => *previous.borrow_mut() = state,
        Err(error) => crate::diagnostics::event(format_args!("window state save failed: {error}")),
    }
}

/// Restore saved geometry and register weak periodic and close-time capture callbacks.
pub fn install(window: &gtk4::ApplicationWindow) {
    let saved = read(&std::fs::read_to_string(path()).unwrap_or_default());
    window.set_default_size(saved.width, saved.height);
    if saved.maximized {
        window.maximize();
    }
    if let Some((x, y)) = saved.position {
        window.connect_realize(move |window| {
            let Some(surface) = window.surface() else {
                return;
            };
            // Ignore stale coordinates if the monitor layout has changed.
            let monitors = gtk4::prelude::WidgetExt::display(window).monitors();
            let visible = (0..monitors.n_items()).any(|i| {
                monitors
                    .item(i)
                    .and_downcast::<gtk4::gdk::Monitor>()
                    .map(|monitor| {
                        let r = monitor.geometry();
                        x >= r.x() && x < r.x() + r.width() && y >= r.y() && y < r.y() + r.height()
                    })
                    .unwrap_or(false)
            });
            if visible {
                cmux_platform::window::restore_position(&surface, x, y);
            }
        });
    }
    let previous = Rc::new(RefCell::new(saved));
    window.connect_close_request({
        let previous = previous.clone();
        move |window| {
            capture(window, &previous);
            glib::Propagation::Proceed
        }
    });
    let weak = window.downgrade();
    glib::timeout_add_local(std::time::Duration::from_millis(500), move || {
        let Some(window) = weak.upgrade() else {
            return glib::ControlFlow::Break;
        };
        capture(&window, &previous);
        glib::ControlFlow::Continue
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    /// Verify saved geometry round-trips and malformed dimensions use safe bounds.
    fn roundtrip_and_invalid_geometry() {
        let state = WindowState {
            width: 1200,
            height: 900,
            maximized: true,
            position: Some((-900, 80)),
        };
        assert_eq!(read(&serde_json::to_string(&state).unwrap()), state);
        assert_eq!(read("invalid"), WindowState::default());
        let clamped = read(r#"{"width":-1,"height":999999}"#);
        assert_eq!((clamped.width, clamped.height), (320, 16384));
    }
}
