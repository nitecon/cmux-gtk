use crate::ghostty::ffi;
use gtk4::prelude::*;

/// Maps GDK modifier state (gdk4::ModifierType bits) to ghostty_input_mods_e.
/// Returns 0 if no modifiers.
pub fn map_mods(state: gtk4::gdk::ModifierType) -> ffi::ghostty_input_mods_e {
    let mut mods: ffi::ghostty_input_mods_e = 0;
    use gtk4::gdk::ModifierType;
    if state.contains(ModifierType::SHIFT_MASK) {
        mods |= ffi::ghostty_input_mods_e_GHOSTTY_MODS_SHIFT;
    }
    if state.contains(ModifierType::CONTROL_MASK) {
        mods |= ffi::ghostty_input_mods_e_GHOSTTY_MODS_CTRL;
    }
    if state.contains(ModifierType::ALT_MASK) {
        mods |= ffi::ghostty_input_mods_e_GHOSTTY_MODS_ALT;
    }
    if state.contains(ModifierType::SUPER_MASK) {
        mods |= ffi::ghostty_input_mods_e_GHOSTTY_MODS_SUPER;
    }
    mods
}

/// Codepoint of the key without any modifier (level 0 of the active layout),
/// as Ghostty's own GTK runtime computes it (`keyvalUnicodeUnshifted` in
/// ghostty/src/apprt/gtk/key.zig). The Kitty keyboard protocol needs it: when
/// it is 0, Ghostty sends the text as-is, so Ctrl+C reaches the app as `c`.
///
/// `entries` = (group, level, unicode codepoint) for every keyval the physical
/// key can produce (from `gdk::Display::map_keycode`); `layout` = the event's
/// active layout (group). Returns 0 when nothing matches.
pub fn unshifted_codepoint<I>(entries: I, layout: u32) -> u32
where
    I: IntoIterator<Item = (i32, i32, u32)>,
{
    entries
        .into_iter()
        .find(|&(group, level, _)| group as u32 == layout && level == 0)
        .map_or(0, |(_, _, cp)| cp)
}

/// Text to send with a key press. Control characters (< 0x20) are not text:
/// Ghostty's encoder handles them from the key itself (same rule as
/// ghostty/src/apprt/gtk/class/surface.zig, `keyEvent`).
pub fn key_text_char(unicode: Option<char>) -> Option<char> {
    unicode.filter(|&ch| ch as u32 >= 0x20)
}

/// Unshifted codepoint and consumed modifiers of a GDK key event.
/// `consumed_mods` = modifiers GDK used to pick the keyval (e.g. Shift for
/// 'C'); Ghostty ignores them when encoding the text, as in its GTK runtime.
pub fn key_event_details(
    ctrl: &gtk4::EventControllerKey,
    keycode: u32,
) -> (u32, ffi::ghostty_input_mods_e) {
    use gtk4::gdk::prelude::DisplayExtManual;
    let Some(event) = ctrl
        .current_event()
        .and_then(|e| e.downcast::<gtk4::gdk::KeyEvent>().ok())
    else {
        return (0, 0);
    };
    let entries = event.display().and_then(|d| d.map_keycode(keycode));
    let unshifted = entries.map_or(0, |entries| {
        unshifted_codepoint(
            entries
                .iter()
                .map(|(k, kv)| (k.group(), k.level(), kv.to_unicode().map_or(0, |c| c as u32))),
            event.layout(),
        )
    });
    (unshifted, map_mods(event.consumed_modifiers()))
}

#[cfg(test)]
mod tests {
    use super::*;

    // US layout: the C key produces 'c' (level 0) and 'C' (level 1).
    #[test]
    fn test_unshifted_codepoint_ctrl_c_us_layout() {
        let entries = [(0, 0, 'c' as u32), (0, 1, 'C' as u32)];
        assert_eq!(unshifted_codepoint(entries, 0), 'c' as u32);
    }

    // Two layouts (US + Russian): level 0 of the active layout wins.
    #[test]
    fn test_unshifted_codepoint_uses_active_layout() {
        let entries = [(0, 0, 'c' as u32), (0, 1, 'C' as u32), (1, 0, 0x0441), (1, 1, 0x0421)];
        assert_eq!(unshifted_codepoint(entries, 0), 'c' as u32);
        assert_eq!(unshifted_codepoint(entries, 1), 0x0441);
    }

    #[test]
    fn test_unshifted_codepoint_no_match_is_zero() {
        assert_eq!(unshifted_codepoint([(0, 1, 'C' as u32)], 0), 0);
        assert_eq!(unshifted_codepoint([], 0), 0);
    }

    #[test]
    fn test_key_text_char_skips_control_characters() {
        assert_eq!(key_text_char(Some('c')), Some('c'));
        assert_eq!(key_text_char(Some('é')), Some('é'));
        assert_eq!(key_text_char(Some('\r')), None);
        assert_eq!(key_text_char(Some('\u{1b}')), None);
        assert_eq!(key_text_char(None), None);
    }

    /// Modifier combinations preserve exact native flags without unrelated bits.
    #[test]
    fn test_map_mods() {
        use gtk4::gdk::ModifierType;

        assert_eq!(map_mods(ModifierType::empty()), 0);
        assert_eq!(
            map_mods(ModifierType::ALT_MASK),
            ffi::ghostty_input_mods_e_GHOSTTY_MODS_ALT
        );
        assert_eq!(
            map_mods(ModifierType::SUPER_MASK),
            ffi::ghostty_input_mods_e_GHOSTTY_MODS_SUPER
        );
        // Test shift
        let shift = map_mods(ModifierType::SHIFT_MASK);
        assert_eq!(shift, ffi::ghostty_input_mods_e_GHOSTTY_MODS_SHIFT);

        // Test control
        let ctrl = map_mods(ModifierType::CONTROL_MASK);
        assert_eq!(ctrl, ffi::ghostty_input_mods_e_GHOSTTY_MODS_CTRL);

        // Test combined
        let combined = map_mods(ModifierType::SHIFT_MASK | ModifierType::CONTROL_MASK);
        assert_eq!(
            combined,
            ffi::ghostty_input_mods_e_GHOSTTY_MODS_SHIFT
                | ffi::ghostty_input_mods_e_GHOSTTY_MODS_CTRL,
            "Combined modifiers must have both bits set"
        );
    }
}
