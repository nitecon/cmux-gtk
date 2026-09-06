//! Bounded per-terminal resume metadata. Registering a binding never executes its command.
use std::collections::BTreeMap;

/// Persisted manual resume intent; agent provenance and automatic launch require separate policy.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct ResumeBinding {
    pub command: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub checkpoint_id: Option<String>,
    #[serde(default, deserialize_with = "environment_or_empty")]
    pub environment: BTreeMap<String, String>,
}

/// Treat an omitted or explicit null environment as no overrides, matching upstream payloads.
fn environment_or_empty<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<BTreeMap<String, String>, D::Error> {
    use serde::Deserialize;
    Ok(Option::<BTreeMap<String, String>>::deserialize(deserializer)?.unwrap_or_default())
}

impl ResumeBinding {
    /// Drop secret-like overrides and application routing identities before persistence or launch.
    /// Ambient process credentials remain managed by the user's shell, not by saved metadata.
    pub fn sanitize_environment(&mut self) {
        self.environment.retain(|key, _| {
            let key = key.to_ascii_uppercase();
            !key.starts_with("CMUX_")
                && ![
                    "TOKEN",
                    "PASSWORD",
                    "SECRET",
                    "API_KEY",
                    "APIKEY",
                    "ACCESS_KEY",
                    "PRIVATE_KEY",
                    "CREDENTIAL",
                ]
                .iter()
                .any(|part| key.contains(part))
        });
    }
    /// Validate both incoming and persisted metadata without logging commands or environment values.
    /// Commands are shell text, not parsed argv; NUL, invalid environment names and oversize data fail.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.command.trim().is_empty() || self.command.len() > 16 * 1024 {
            return Err("command must contain 1 to 16384 bytes");
        }
        let strings = std::iter::once(self.command.as_str())
            .chain(self.name.as_deref())
            .chain(self.kind.as_deref())
            .chain(self.cwd.as_deref())
            .chain(self.checkpoint_id.as_deref())
            .chain(
                self.environment
                    .iter()
                    .flat_map(|(key, value)| [key.as_str(), value.as_str()]),
            );
        let mut bytes = 0usize;
        for value in strings {
            if value.contains('\0') || value.len() > 16 * 1024 {
                return Err("resume values must be NUL-free and at most 16384 bytes");
            }
            bytes += value.len();
        }
        if bytes > 64 * 1024 || self.environment.len() > 64 {
            return Err("resume binding exceeds 65536 bytes or 64 environment entries");
        }
        for key in self.environment.keys() {
            let mut characters = key.chars();
            if !characters
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
                || !characters.all(|c| c.is_ascii_alphanumeric() || c == '_')
            {
                return Err("environment keys must be shell variable names");
            }
        }
        Ok(())
    }
}

/// A validated metadata operation; clearing may require the current checkpoint identity.
pub enum ResumeAction {
    Set(ResumeBinding),
    Show,
    Clear { checkpoint_id: Option<String> },
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Real serialized metadata preserves literal command/environment data and rejects invalid launch inputs.
    #[test]
    fn binding_roundtrip_and_limits() {
        let mut binding: ResumeBinding = serde_json::from_value(serde_json::json!({
            "command": "tmux attach -t 'a b'", "checkpoint_id": "a b",
            "environment": {"PROJECT": "value with $ and quotes '"}
        }))
        .unwrap();
        binding.validate().unwrap();
        let encoded = serde_json::to_vec(&binding).unwrap();
        assert_eq!(
            serde_json::from_slice::<ResumeBinding>(&encoded).unwrap(),
            binding
        );
        binding.environment.insert("BAD=KEY".into(), "value".into());
        assert!(binding.validate().is_err());
        binding.environment.clear();
        binding.command = "x".repeat(16385);
        assert!(binding.validate().is_err());
        binding.command = "bad\0command".into();
        assert!(binding.validate().is_err());
    }

    /// Null maps are compatible and persisted overrides omit secrets and routing identities.
    #[test]
    fn environment_sanitization() {
        let mut binding: ResumeBinding = serde_json::from_value(serde_json::json!({
            "command": "true", "environment": null
        }))
        .unwrap();
        assert!(binding.environment.is_empty());
        binding.environment.extend([
            ("SERVICE_API_KEY".into(), "secret".into()),
            ("CMUX_SOCKET".into(), "/wrong".into()),
            ("CLAUDE_SECURESTORAGE_CONFIG_DIR".into(), "/settings".into()),
        ]);
        binding.sanitize_environment();
        assert_eq!(
            binding.environment,
            BTreeMap::from([("CLAUDE_SECURESTORAGE_CONFIG_DIR".into(), "/settings".into())])
        );
    }
}
