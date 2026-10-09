//! Private atomic gateway journal and credential storage, used only on blocking workers.
use super::model::*;
use std::path::{Path, PathBuf};

/// Keep integration state outside terminal snapshots, under the existing cmux configuration root.
pub fn path() -> PathBuf {
    crate::config::config_path().with_file_name("gateway.json")
}

/// Load bounded state, preserving a malformed journal rather than replacing its delivery fences.
pub fn load(path: &Path) -> Result<Journal, String> {
    let text = match cmux_platform::filesystem::read_text_bounded(path, 8 * 1024 * 1024) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Journal::default()),
        Err(_) => return Err("Cannot read gateway journal; integration paused".into()),
    };
    let mut journal: Journal =
        serde_json::from_str(&text).map_err(|_| "Invalid gateway journal; integration paused")?;
    uuid::Uuid::parse_str(&journal.instance_id).map_err(|_| "Invalid gateway instance identity")?;
    if journal.receipts.len() > MAX_RECEIPTS {
        return Err("Gateway journal exceeds retention limits".into());
    }
    if !journal.config.url.is_empty() {
        endpoint(&journal.config.url)?;
    }
    if journal.cursor.is_some_and(|c| c < 0) {
        return Err("Invalid gateway cursor".into());
    }
    let mut ids = std::collections::HashSet::new();
    for receipt in &mut journal.receipts {
        identity(&receipt.event_id)?;
        if let Some(session_id) = &receipt.recipient_session_id {
            uuid::Uuid::parse_str(session_id).map_err(|_| "Invalid gateway receipt session")?;
            if receipt
                .target
                .as_ref()
                .is_some_and(|target| target.session_id != *session_id)
            {
                return Err("Gateway recipient fence identity changed".into());
            }
        }
        if receipt.reason.len() > 4096 {
            return Err("Invalid gateway receipt reason".into());
        }
        if let Some(payload) = &receipt.payload {
            payload.validate()?;
        }
        if receipt.candidates.len() > MAX_PENDING
            || receipt
                .event
                .as_ref()
                .is_some_and(|e| e.to_string().len() > 65536)
            || receipt.outcome == "received" && receipt.event.is_none()
        {
            return Err("Invalid received gateway event".into());
        }
        if receipt.outcome == "queued" && (receipt.payload.is_none() || receipt.target.is_none()) {
            return Err("Incomplete queued gateway event".into());
        }
        if let Some(target) = &receipt.target {
            uuid::Uuid::parse_str(&target.terminal.workspace_id)
                .map_err(|_| "Invalid gateway target workspace")?;
            uuid::Uuid::parse_str(&target.terminal.surface_id)
                .map_err(|_| "Invalid gateway target surface")?;
            if !target.terminal.directory.is_absolute()
                || target.process.client.is_empty()
                || target.process.client.len() > 256
                || target.process.client.contains('\0')
            {
                return Err("Invalid gateway terminal target".into());
            }
            if !target.session_id.is_empty() {
                uuid::Uuid::parse_str(&target.session_id)
                    .map_err(|_| "Invalid gateway agent session")?;
            }
        }
        if !ids.insert((
            receipt.event_id.clone(),
            receipt.recipient().map(str::to_owned),
        )) || !matches!(
            receipt.outcome.as_str(),
            "received" | "queued" | "submitting" | "injected" | "skipped" | "failed" | "uncertain"
        ) {
            return Err("Invalid gateway delivery receipt".into());
        }
        if receipt.outcome == "queued" && receipt.recipient() == Some("") {
            receipt.outcome = "skipped".into();
            receipt.reason =
                "Legacy queued recipient has no exact agent session; replay disabled".into();
            receipt.confirmed = false;
            receipt.payload = None;
        }
        if receipt.outcome == "submitting" {
            receipt.outcome = "uncertain".into();
            receipt.reason = "Interrupted terminal submission; automatic replay is disabled".into();
            receipt.confirmed = false;
            receipt.payload = None;
            receipt.event = None;
            receipt.candidates.clear();
        }
    }
    Ok(journal)
}

/// Replace and fsync private journal state before any terminal input.
pub fn save(path: &Path, journal: &Journal) -> Result<(), String> {
    let data = serde_json::to_vec(journal).map_err(|_| "Cannot encode gateway journal")?;
    if data.len() > 8 * 1024 * 1024 {
        return Err("Gateway journal exceeds storage limit".into());
    }
    cmux_platform::filesystem::atomic_write(path, &data)
        .and_then(|_| cmux_platform::filesystem::sync_file_and_parent(path))
        .map_err(|_| "Cannot persist gateway journal; delivery paused".into())
}

/// Read the same bearer environment used by native gateway clients, or a separately saved private key.
pub fn key(path: &Path) -> Result<String, String> {
    let value = std::env::var("CMUX_GATEWAY_API_KEY")
        .or_else(|_| std::env::var("GATEWAY_API_KEY"))
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            cmux_platform::filesystem::read_text_bounded(&path.with_file_name("gateway-key"), 4096)
                .ok()
        })
        .unwrap_or_default();
    validate_key(&value)?;
    Ok(value)
}

/// Reject header control bytes without including secret data in any error message.
pub fn validate_key(key: &str) -> Result<(), String> {
    if key.len() > 4096 || key.chars().any(char::is_control) {
        return Err("Invalid gateway API key".into());
    }
    Ok(())
}

/// Save an explicitly entered key using the existing owner-only atomic filesystem helper.
pub fn save_key(path: &Path, key: &str) -> Result<(), String> {
    validate_key(key)?;
    let path = path.with_file_name("gateway-key");
    cmux_platform::filesystem::atomic_write(&path, key.as_bytes())
        .and_then(|_| cmux_platform::filesystem::sync_file_and_parent(&path))
        .map_err(|_| "Cannot save gateway credential".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A damaged journal cannot silently reset instance identity or replay accepted work.
    #[test]
    fn damaged_journal_is_not_replaced() {
        let root =
            std::env::temp_dir().join(format!("cmux-gateway-storage-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("gateway.json");
        let journal = Journal::default();
        save(&path, &journal).unwrap();
        assert_eq!(load(&path).unwrap().instance_id, journal.instance_id);
        std::fs::write(&path, b"{damaged").unwrap();
        assert!(load(&path).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"{damaged");
        assert!(validate_key("header\r\ninjection").is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
    /// Legacy manual mappings/runs never grant global consent; interrupted submissions remain nonreplayable.
    #[test]
    fn legacy_preferences_and_delivery_fences() {
        let root =
            std::env::temp_dir().join(format!("cmux-gateway-migration-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("gateway.json");
        let legacy = serde_json::json!({"instance_id": uuid::Uuid::new_v4().to_string(),
            "config":{"enabled":true,"url":"https://gateway.example", "mappings":[{"workspace_id":"old","project_ident":"old"}]}, "runs":[]});
        std::fs::write(&path, legacy.to_string()).unwrap();
        let mut journal = load(&path).unwrap();
        assert!(journal.config.enabled);
        assert!(!journal.config.injection_approved);
        journal.receipts.push_back(Receipt {
            event_id: "event".into(),
            outcome: "submitting".into(),
            ..Default::default()
        });
        save(&path, &journal).unwrap();
        let restored = load(&path).unwrap();
        assert_eq!(restored.receipts[0].outcome, "uncertain");
        let encoded = serde_json::to_value(&restored).unwrap();
        assert!(encoded["config"].get("mappings").is_none());
        assert!(encoded.get("runs").is_none());
        std::fs::remove_dir_all(root).unwrap();
    }
}
