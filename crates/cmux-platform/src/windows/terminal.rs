//! Windows terminal identity for command-line callers.

/// ConPTY provides no POSIX TTY pathname; callers use CMUX_SURFACE_ID for routing.
pub fn caller_tty() -> Option<String> {
    None
}

/// Convert GDK's Windows virtual key to the renderer's native scan code, retaining extended keys.
pub fn physical_keycode(hardware: u32, keypad_enter: bool) -> u32 {
    if keypad_enter {
        return 0xe01c;
    }
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{MapVirtualKeyW, MAPVK_VK_TO_VSC_EX};
    // SAFETY: this queries the current keyboard mapping without retaining pointers.
    unsafe { MapVirtualKeyW(hardware, MAPVK_VK_TO_VSC_EX) }
}
