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
    if journal.runs.len() > 128 || journal.config.mappings.len() > MAX_SESSIONS {
        return Err("Gateway journal exceeds retention limits".into());
    }
    if !journal.config.url.is_empty() {
        endpoint(&journal.config.url)?;
    }
    for mapping in &journal.config.mappings {
        uuid::Uuid::parse_str(&mapping.workspace_id)
            .map_err(|_| "Invalid gateway workspace mapping")?;
        identity(&mapping.project_ident)?;
    }
    for run in &mut journal.runs {
        run.assignment.validate()?;
        if !run.session.matches(&run.assignment) || run.reports.len() > 16 || run.last_sequence < 0
        {
            return Err("Invalid gateway delivery journal".into());
        }
        // A crash between native input and durable completion must never replay the prompt.
        if matches!(run.phase.as_str(), "submitting" | "accepting") {
            run.phase = "uncertain".into();
        }
    }
    Ok(journal)
}

/// Replace and fsync private journal state before any external acceptance, report or input.
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
}
