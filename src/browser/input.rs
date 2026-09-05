//! Browser input translation independent of application state and command transport.
use gtk4::prelude::*;

/// Resolve positive viewport dimensions, falling back to widget allocation before a frame arrives.
pub(super) fn viewport_size(picture: &gtk4::Picture) -> Option<(f64, f64)> {
    let widget = (picture.width() as f64, picture.height() as f64);
    if widget.0 <= 0.0 || widget.1 <= 0.0 {
        return None;
    }
    Some(
        picture
            .paintable()
            .map(|p| (p.intrinsic_width() as f64, p.intrinsic_height() as f64))
            .filter(|(width, height)| *width > 0.0 && *height > 0.0)
            .unwrap_or(widget),
    )
}

/// Map a GTK picture event into its contained, centered preview image; ignore padding clicks.
/// Browser preview construction explicitly selects ContentFit::Contain.
pub(super) fn picture_point(picture: &gtk4::Picture, x: f64, y: f64) -> Option<(i64, i64)> {
    contained_point(
        (picture.width() as f64, picture.height() as f64),
        viewport_size(picture)?,
        (x, y),
    )
}

/// Convert coordinates through aspect-preserving scaling; reject padding and invalid geometry.
fn contained_point(
    widget: (f64, f64),
    viewport: (f64, f64),
    point: (f64, f64),
) -> Option<(i64, i64)> {
    if ![widget.0, widget.1, viewport.0, viewport.1]
        .iter()
        .all(|v| v.is_finite() && *v > 0.0)
        || !point.0.is_finite()
        || !point.1.is_finite()
    {
        return None;
    }
    let scale = (widget.0 / viewport.0).min(widget.1 / viewport.1);
    let x = (point.0 - (widget.0 - viewport.0 * scale) / 2.0) / scale;
    let y = (point.1 - (widget.1 - viewport.1 * scale) / 2.0) / scale;
    if x < 0.0 || y < 0.0 || x >= viewport.0 || y >= viewport.1 {
        return None;
    }
    Some((x as i64, y as i64))
}

/// Map GDK keyval to (CDP key name, CDP code name).
/// Returns empty strings for unmapped keys.
pub(super) fn gdk_keyval_to_cdp(keyval: gtk4::gdk::Key) -> (String, String) {
    use gtk4::gdk::Key;
    match keyval {
        Key::Return | Key::KP_Enter => ("Enter".into(), "Enter".into()),
        Key::Tab => ("Tab".into(), "Tab".into()),
        Key::Escape => ("Escape".into(), "Escape".into()),
        Key::BackSpace => ("Backspace".into(), "Backspace".into()),
        Key::Delete => ("Delete".into(), "Delete".into()),
        Key::Home => ("Home".into(), "Home".into()),
        Key::End => ("End".into(), "End".into()),
        Key::Page_Up => ("PageUp".into(), "PageUp".into()),
        Key::Page_Down => ("PageDown".into(), "PageDown".into()),
        Key::Left => ("ArrowLeft".into(), "ArrowLeft".into()),
        Key::Right => ("ArrowRight".into(), "ArrowRight".into()),
        Key::Up => ("ArrowUp".into(), "ArrowUp".into()),
        Key::Down => ("ArrowDown".into(), "ArrowDown".into()),
        Key::space => (" ".into(), "Space".into()),
        Key::F1 => ("F1".into(), "F1".into()),
        Key::F2 => ("F2".into(), "F2".into()),
        Key::F3 => ("F3".into(), "F3".into()),
        Key::F4 => ("F4".into(), "F4".into()),
        Key::F5 => ("F5".into(), "F5".into()),
        Key::F6 => ("F6".into(), "F6".into()),
        Key::F7 => ("F7".into(), "F7".into()),
        Key::F8 => ("F8".into(), "F8".into()),
        Key::F9 => ("F9".into(), "F9".into()),
        Key::F10 => ("F10".into(), "F10".into()),
        Key::F11 => ("F11".into(), "F11".into()),
        Key::F12 => ("F12".into(), "F12".into()),
        other => {
            // For printable characters, use the unicode value
            if let Some(ch) = other.to_unicode() {
                let s = ch.to_string();
                let code = if ch.is_ascii_alphabetic() {
                    format!("Key{}", ch.to_ascii_uppercase())
                } else if ch.is_ascii_digit() {
                    format!("Digit{}", ch)
                } else {
                    s.clone()
                };
                (s, code)
            } else {
                (String::new(), String::new())
            }
        }
    }
}

/// Convert GDK modifier flags to CDP modifier bitmask.
/// CDP: Alt=1, Ctrl=2, Meta=4, Shift=8
pub(super) fn cdp_modifiers(mods: gtk4::gdk::ModifierType) -> i32 {
    let mut m = 0;
    if mods.contains(gtk4::gdk::ModifierType::ALT_MASK) {
        m |= 1;
    }
    if mods.contains(gtk4::gdk::ModifierType::CONTROL_MASK) {
        m |= 2;
    }
    if mods.contains(gtk4::gdk::ModifierType::SHIFT_MASK) {
        m |= 8;
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Wide and tall previews map image pixels consistently while their padding receives no input.
    #[test]
    fn coordinates_follow_contained_image() {
        assert_eq!(
            contained_point((100.0, 100.0), (200.0, 100.0), (50.0, 50.0)),
            Some((100, 50))
        );
        assert_eq!(
            contained_point((100.0, 100.0), (200.0, 100.0), (0.0, 25.0)),
            Some((0, 0))
        );
        assert_eq!(
            contained_point((100.0, 100.0), (200.0, 100.0), (50.0, 24.0)),
            None
        );
        assert_eq!(
            contained_point((100.0, 100.0), (100.0, 200.0), (25.0, 0.0)),
            Some((0, 0))
        );
        assert_eq!(
            contained_point((100.0, 100.0), (100.0, 200.0), (24.0, 50.0)),
            None
        );
        assert_eq!(
            contained_point((100.0, 100.0), (100.0, 100.0), (100.0, 50.0)),
            None
        );
        assert_eq!(
            contained_point((0.0, 100.0), (100.0, 100.0), (0.0, 0.0)),
            None
        );
        assert_eq!(
            contained_point((100.0, 100.0), (100.0, 100.0), (f64::NAN, 0.0)),
            None
        );
    }
}
