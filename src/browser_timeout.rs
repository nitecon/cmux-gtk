//! Browser wait budgets shared by the CLI and daemon transport.

use std::time::Duration;

/// Return transport and client budgets, allowing five seconds for each response boundary.
/// Convert milliseconds before adding margins so even the largest accepted u64 input cannot wrap.
pub(crate) fn wait_budgets(timeout_ms: u64) -> (Duration, Duration) {
    let requested = Duration::from_millis(timeout_ms);
    let transport = requested.saturating_add(Duration::from_secs(5));
    let client = transport.saturating_add(Duration::from_secs(5));
    (transport, client)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Preserve zero-time polling and full response margins without overflowing accepted CLI input.
    #[test]
    fn wait_margins_cover_extreme_arguments() {
        for milliseconds in [0, 8_000, u64::MAX] {
            let (transport, client) = wait_budgets(milliseconds);
            assert_eq!(transport.as_millis(), u128::from(milliseconds) + 5_000);
            assert_eq!(client.as_millis(), u128::from(milliseconds) + 10_000);
        }
    }
}
