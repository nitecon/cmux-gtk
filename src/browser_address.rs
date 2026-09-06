//! Shared syntactic browser address normalization; no filesystem reads or shell expansion.

/// Preserve browser schemes, encode absolute Unix file paths, and default bare hosts to HTTPS.
/// Relative paths require an explicit file URL; normalization does not change browser access policy.
pub fn normalize(value: &str) -> String {
    let value = value.trim();
    if value.starts_with('/') {
        return reqwest::Url::from_file_path(value)
            .map(String::from)
            .unwrap_or_else(|_| value.to_owned());
    }
    let lower = value.to_ascii_lowercase();
    if value.contains("://")
        || ["about:", "data:", "blob:"]
            .iter()
            .any(|scheme| lower.starts_with(scheme))
        || value.is_empty()
    {
        value.to_owned()
    } else {
        format!("https://{value}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Browser pseudo-schemes stay intact and literal filename delimiters are percent-encoded.
    #[test]
    fn browser_and_file_addresses() {
        for address in [
            "about:blank",
            "data:text/html,<h1>Hello</h1>",
            "blob:https://example.com/id",
            "https://example.com",
            "file:///tmp/a.html",
        ] {
            assert_eq!(normalize(address), address);
        }
        assert_eq!(normalize(" /tmp/a #?.html "), "file:///tmp/a%20%23%3F.html");
        assert_eq!(normalize("example.com/path"), "https://example.com/path");
        assert_eq!(normalize("localhost:3000"), "https://localhost:3000");
    }
}
