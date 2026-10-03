use gtk4::prelude::*;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::LazyLock;

#[derive(serde::Serialize, serde::Deserialize)]
struct Preferences {
    font_size: f32,
    #[serde(default)]
    invert_scroll: bool,
}

/// Scroll inversion read by every terminal's scroll handler; loaded once, updated on Apply.
static INVERT_SCROLL: LazyLock<AtomicBool> =
    LazyLock::new(|| AtomicBool::new(read(&path()).is_some_and(|prefs| prefs.invert_scroll)));

/// Whether mouse-wheel and touchpad scrolling are inverted.
pub fn invert_scroll() -> bool {
    INVERT_SCROLL.load(Ordering::Relaxed)
}

/// Locate terminal preferences beside the application configuration.
fn path() -> PathBuf {
    crate::config::config_path().with_file_name("preferences.json")
}

/// Accept finite font sizes within the supported point-size range.
fn valid(size: f32) -> bool {
    size.is_finite() && (6.0..=72.0).contains(&size)
}

/// Load stored preferences, ignoring missing or malformed files.
fn read(path: &Path) -> Option<Preferences> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

/// Load a valid stored font size, ignoring missing or malformed preferences.
fn read_size(path: &Path) -> Option<f32> {
    read(path).map(|prefs| prefs.font_size).filter(|size| valid(*size))
}

/// Return the optional user-selected terminal size without changing native configuration.
pub fn saved_font_size() -> Option<f32> {
    read_size(&path())
}

/// Validate and atomically persist the preferences, returning user-readable errors.
fn save(path: &Path, size: f32, invert_scroll: bool) -> Result<(), String> {
    if !valid(size) {
        return Err("Font size must be between 6 and 72 points.".into());
    }
    let contents = serde_json::to_vec_pretty(&Preferences { font_size: size, invert_scroll })
        .map_err(|error| error.to_string())?;
    cmux_platform::filesystem::atomic_write(path, &contents).map_err(|error| error.to_string())
}

/// Snapshot registered native terminal handles for use on the GTK thread.
fn surfaces() -> Vec<usize> {
    crate::ghostty::callbacks::GL_TO_SURFACE
        .lock()
        .map(|registry| registry.values().copied().collect())
        .unwrap_or_default()
}

/// Display the font-size editor and apply successful changes to live terminal surfaces.
pub fn show(parent: &gtk4::ApplicationWindow, state: &crate::app_state::AppStateRef) {
    let dialog = gtk4::Dialog::builder()
        .title("Preferences")
        .transient_for(parent)
        .modal(true)
        .default_width(380)
        .build();
    dialog.add_button("Cancel", gtk4::ResponseType::Cancel);
    dialog.add_button("Apply", gtk4::ResponseType::Apply);
    let content = dialog.content_area();
    content.set_spacing(12);
    content.set_margin_top(20);
    content.set_margin_bottom(20);
    content.set_margin_start(20);
    content.set_margin_end(20);
    let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 12);
    let label = gtk4::Label::new(Some("Terminal font size (pt)"));
    label.set_hexpand(true);
    label.set_xalign(0.0);
    let size = gtk4::SpinButton::with_range(6.0, 72.0, 0.5);
    size.set_digits(1);
    let current = saved_font_size()
        .or_else(|| {
            surfaces().first().map(|surface| unsafe {
                crate::ghostty::ffi::ghostty_surface_font_size(*surface as _)
            })
        })
        .unwrap_or(12.0);
    size.set_value(current as f64);
    row.append(&label);
    row.append(&size);
    content.append(&row);
    let help = gtk4::Label::new(Some(
        "Applies to all terminal tabs, including new tabs.\nSaved for future launches.",
    ));
    help.set_xalign(0.0);
    help.set_wrap(true);
    content.append(&help);
    let invert = gtk4::CheckButton::with_label("Invert scrolling (mouse wheel and touchpad)");
    invert.set_active(invert_scroll());
    content.append(&invert);
    crate::resume_review::append(&content, state);
    let error_label = gtk4::Label::new(None);
    error_label.set_wrap(true);
    content.append(&error_label);
    dialog.connect_response(move |dialog, response| {
        if response != gtk4::ResponseType::Apply {
            dialog.close();
            return;
        }
        size.update();
        let value = size.value() as f32;
        if let Err(error) = save(&path(), value, invert.is_active()) {
            error_label.set_text(&format!("Could not save preferences: {error}"));
            return;
        }
        INVERT_SCROLL.store(invert.is_active(), Ordering::Relaxed);
        let action = format!("set_font_size:{value}");
        let mut failed = false;
        for surface in surfaces() {
            let applied = unsafe {
                crate::ghostty::ffi::ghostty_surface_binding_action(
                    surface as _,
                    action.as_ptr().cast(),
                    action.len(),
                )
            };
            failed |= !applied;
        }
        crate::diagnostics::event(format_args!(
            "terminal font size saved points={value} live_apply_failed={failed}"
        ));
        if failed {
            error_label.set_text(
                "Saved. Some terminals could not update; reopen those tabs to apply the size.",
            );
        } else {
            dialog.close();
        }
    });
    dialog.present();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// Verify stored sizes round-trip and unsupported values are rejected.
    fn font_size_roundtrip_and_invalid_values() {
        let dir = std::env::temp_dir().join(format!("cmux-font-{}", uuid::Uuid::new_v4()));
        let path = dir.join("preferences.json");
        assert_eq!(read_size(&path), None);
        save(&path, 15.5, true).unwrap();
        assert_eq!(read_size(&path), Some(15.5));
        assert!(read(&path).unwrap().invert_scroll);
        std::fs::write(&path, r#"{"font_size": 14.0}"#).unwrap();
        assert!(!read(&path).unwrap().invert_scroll);
        save(&path, 15.5, false).unwrap();
        for invalid in [0.0, 73.0, f32::NAN, f32::INFINITY] {
            assert!(save(&path, invalid, true).is_err());
            assert_eq!(read_size(&path), Some(15.5));
        }
        std::fs::write(&path, "broken json").unwrap();
        assert_eq!(read_size(&path), None);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
