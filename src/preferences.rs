use gtk4::prelude::*;
use std::path::{Path, PathBuf};

#[derive(serde::Serialize, serde::Deserialize)]
struct Preferences {
    font_size: f32,
}

fn path() -> PathBuf {
    crate::config::config_path().with_file_name("preferences.json")
}

fn valid(size: f32) -> bool {
    size.is_finite() && (6.0..=72.0).contains(&size)
}

fn read_size(path: &Path) -> Option<f32> {
    let prefs: Preferences = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
    valid(prefs.font_size).then_some(prefs.font_size)
}

pub fn saved_font_size() -> Option<f32> {
    read_size(&path())
}

fn save_size(path: &Path, size: f32) -> Result<(), String> {
    if !valid(size) {
        return Err("Font size must be between 6 and 72 points.".into());
    }
    let contents = serde_json::to_vec_pretty(&Preferences { font_size: size })
        .map_err(|error| error.to_string())?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let temporary = path.with_extension("json.tmp");
    std::fs::write(&temporary, contents).map_err(|error| error.to_string())?;
    std::fs::rename(temporary, path).map_err(|error| error.to_string())
}

fn surfaces() -> Vec<usize> {
    crate::ghostty::callbacks::GL_TO_SURFACE.lock()
        .map(|registry| registry.values().copied().collect())
        .unwrap_or_default()
}

pub fn show(parent: &gtk4::ApplicationWindow) {
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
    let current = saved_font_size().or_else(|| surfaces().first().map(|surface| unsafe {
        crate::ghostty::ffi::ghostty_surface_font_size(*surface as _)
    })).unwrap_or(12.0);
    size.set_value(current as f64);
    row.append(&label);
    row.append(&size);
    content.append(&row);
    let help = gtk4::Label::new(Some("Applies to all terminal tabs, including new tabs.\nSaved for future launches."));
    help.set_xalign(0.0);
    help.set_wrap(true);
    content.append(&help);
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
        if let Err(error) = save_size(&path(), value) {
            error_label.set_text(&format!("Could not save preferences: {error}"));
            return;
        }
        let action = format!("set_font_size:{value}");
        let mut failed = false;
        for surface in surfaces() {
            let applied = unsafe {
                crate::ghostty::ffi::ghostty_surface_binding_action(
                    surface as _, action.as_ptr().cast(), action.len(),
                )
            };
            failed |= !applied;
        }
        crate::diagnostics::event(format_args!("terminal font size saved points={value} live_apply_failed={failed}"));
        if failed {
            error_label.set_text("Saved. Some terminals could not update; reopen those tabs to apply the size.");
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
    fn font_size_roundtrip_and_invalid_values() {
        let dir = std::env::temp_dir().join(format!("cmux-font-{}", uuid::Uuid::new_v4()));
        let path = dir.join("preferences.json");
        assert_eq!(read_size(&path), None);
        save_size(&path, 15.5).unwrap();
        assert_eq!(read_size(&path), Some(15.5));
        for invalid in [0.0, 73.0, f32::NAN, f32::INFINITY] {
            assert!(save_size(&path, invalid).is_err());
            assert_eq!(read_size(&path), Some(15.5));
        }
        std::fs::write(&path, "broken json").unwrap();
        assert_eq!(read_size(&path), None);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
