//! Windows terminal identity for command-line callers.

/// ConPTY provides no POSIX TTY pathname; callers use CMUX_SURFACE_ID for routing.
pub fn caller_tty() -> Option<String> {
    None
}

/// Native Return scan code for complete-message submission.
pub fn enter_keycode() -> u32 {
    physical_keycode(0x0d, false)
}

/// ConPTY turns bulk text into key records. End finishes that text at its final caret before Return.
pub fn submission_boundary_keycode() -> Option<u32> {
    Some(physical_keycode(0x23, false))
}

/// Convert GDK's Windows virtual key to the renderer's native scan code, retaining extended keys.
pub fn physical_keycode(hardware: u32, keypad_enter: bool) -> u32 {
    if keypad_enter {
        return 0xe01c;
    }
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{MapVirtualKeyW, MAPVK_VK_TO_VSC_EX};
    // SAFETY: this queries the current keyboard mapping without retaining pointers.
    let scan = unsafe { MapVirtualKeyW(hardware, MAPVK_VK_TO_VSC_EX) };
    // Navigation virtual keys identify the dedicated extended keys. Some Windows
    // mappings omit E0 even with MAPVK_VK_TO_VSC_EX; Ghostty then sees keypad keys.
    match hardware {
        0x21..=0x28 | 0x2d | 0x2e => scan | 0xe000,
        _ => scan,
    }
}

#[cfg(test)]
mod tests {
    use super::physical_keycode;

    #[test]
    fn navigation_keys_remain_distinct_from_keypad_keys() {
        for (virtual_key, scan) in [
            (0x21, 0xe049),
            (0x22, 0xe051),
            (0x23, 0xe04f),
            (0x24, 0xe047),
            (0x25, 0xe04b),
            (0x26, 0xe048),
            (0x27, 0xe04d),
            (0x28, 0xe050),
            (0x2d, 0xe052),
            (0x2e, 0xe053),
        ] {
            assert_eq!(physical_keycode(virtual_key, false), scan);
        }
        assert_eq!(physical_keycode(0x68, false), 0x48); // VK_NUMPAD8
        assert_eq!(physical_keycode(0x0d, true), 0xe01c);
    }
}
