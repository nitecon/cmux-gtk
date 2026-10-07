//! Windows terminal identity for command-line callers.

/// ConPTY provides no POSIX TTY pathname; callers use CMUX_SURFACE_ID for routing.
pub fn caller_tty() -> Option<String> {
    None
}
