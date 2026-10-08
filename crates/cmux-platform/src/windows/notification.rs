//! Windows preview desktop notifications; in-app inbox remains available.

/// Native desktop bells are unavailable in this preview; workspace attention remains active.
pub fn terminal_bell(_workspace_name: &str) -> Option<std::process::Command> {
    None
}
/// Native toasts are unavailable in this preview; notifications remain in CMUX's inbox.
pub fn message(_title: &str, _subtitle: &str, _body: &str) -> Option<std::process::Command> {
    None
}
