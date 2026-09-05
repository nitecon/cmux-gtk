//! Tests for GDK keycode → ghostty input mapping (GHOST-03, GHOST-06/Pitfall 6)

/// Hardware A must resolve to a recognized Ghostty key.
#[test]
fn keycode_a_maps_to_ghostty_a() {
    let result = crate::ghostty::input::map_keycode_to_ghostty(38);
    // Must not be GHOSTTY_KEY_INVALID
    assert_ne!(
        result,
        crate::ghostty::ffi::ghostty_input_key_e_GHOSTTY_KEY_UNIDENTIFIED,
        "keycode 38 (hardware 'a') must map to a valid key"
    );
}

/// Hardware Return must map to the Enter key.
#[test]
fn keycode_return_maps_to_enter() {
    let result = crate::ghostty::input::map_keycode_to_ghostty(36);
    assert_eq!(
        result,
        crate::ghostty::ffi::ghostty_input_key_e_GHOSTTY_KEY_ENTER,
        "keycode 36 (Return) must map to GHOSTTY_KEY_ENTER"
    );
}

/// Hardware Escape must map to the Escape key.
#[test]
fn keycode_escape_maps_correctly() {
    let result = crate::ghostty::input::map_keycode_to_ghostty(9);
    assert_eq!(
        result,
        crate::ghostty::ffi::ghostty_input_key_e_GHOSTTY_KEY_ESCAPE,
        "keycode 9 (Escape) must map to GHOSTTY_KEY_ESCAPE"
    );
}

/// An unknown hardware key must fail gracefully.
#[test]
fn unknown_keycode_returns_invalid() {
    let result = crate::ghostty::input::map_keycode_to_ghostty(0);
    assert_eq!(
        result,
        crate::ghostty::ffi::ghostty_input_key_e_GHOSTTY_KEY_UNIDENTIFIED,
        "unknown keycode must map to GHOSTTY_KEY_INVALID without panic"
    );
}

/// Modifier combinations preserve each contributing modifier.
#[test]
fn test_map_mods() {
    use gtk4::gdk::ModifierType;

    // Test shift
    let shift = crate::ghostty::input::map_mods(ModifierType::SHIFT_MASK);
    assert_ne!(shift, 0, "SHIFT modifier must map to non-zero");

    // Test control
    let ctrl = crate::ghostty::input::map_mods(ModifierType::CONTROL_MASK);
    assert_ne!(ctrl, 0, "CONTROL modifier must map to non-zero");

    // Test combined
    let combined =
        crate::ghostty::input::map_mods(ModifierType::SHIFT_MASK | ModifierType::CONTROL_MASK);
    assert_ne!(combined, 0, "Combined modifiers must map to non-zero");
    assert_ne!(combined, shift, "Combined must differ from shift alone");
    assert_ne!(combined, ctrl, "Combined must differ from ctrl alone");
}
