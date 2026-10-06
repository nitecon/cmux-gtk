//! Metadata owned for exactly the lifetime of each registered Ghostty surface.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

/// Routing identity and the latest directory known for one terminal.
struct Surface {
    pane_id: u64,
    working_directory: String,
    input_revision: u64,
    pending_clipboard: u32,
}

static SURFACES: LazyLock<Mutex<HashMap<usize, Surface>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Count registered native terminals without accessing GTK or dereferencing handles.
pub(crate) fn live_count() -> Option<usize> {
    SURFACES.lock().ok().map(|surfaces| surfaces.len())
}

/// Register a newly created surface with its explicit launch directory, if known.
pub(crate) fn register(surface: usize, pane_id: u64, directory: Option<&std::path::Path>) {
    if let Ok(mut surfaces) = SURFACES.lock() {
        surfaces.insert(
            surface,
            Surface {
                pane_id,
                input_revision: 0,
                pending_clipboard: 0,
                working_directory: directory
                    .map(|path| path.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            },
        );
    }
}

/// Retire routing and directory metadata after the native surface is freed.
pub(crate) fn unregister(surface: usize) {
    if let Ok(mut surfaces) = SURFACES.lock() {
        surfaces.remove(&surface);
    }
}

/// Resolve an action's terminal pointer to its owning pane without dereferencing it.
pub(crate) fn pane_id(surface: usize) -> Option<u64> {
    SURFACES
        .lock()
        .ok()?
        .get(&surface)
        .map(|surface| surface.pane_id)
}

/// Copy the latest reported directory, or return empty when no directory is known.
pub(crate) fn working_directory(surface: usize) -> String {
    SURFACES
        .lock()
        .ok()
        .and_then(|surfaces| {
            surfaces
                .get(&surface)
                .map(|surface| surface.working_directory.clone())
        })
        .unwrap_or_default()
}

/// Apply a native directory report only to an existing surface; never recreate retired state.
pub(crate) fn set_working_directory(surface: usize, directory: &str) {
    if let Ok(mut surfaces) = SURFACES.lock() {
        if let Some(surface) = surfaces.get_mut(&surface) {
            surface.working_directory.clear();
            surface.working_directory.push_str(directory);
        }
    }
}

/// Invalidate readiness before keyboard, typed text or clipboard input reaches the process.
pub(crate) fn record_input(surface: usize) {
    if let Ok(mut surfaces) = SURFACES.lock() {
        if let Some(surface) = surfaces.get_mut(&surface) {
            surface.input_revision = surface.input_revision.saturating_add(1);
        }
    }
}

/// Read a monotonic input revision; an unavailable registry never permits a matching observation.
pub(crate) fn input_revision(surface: usize) -> u64 {
    SURFACES
        .lock()
        .ok()
        .and_then(|s| s.get(&surface).map(|s| s.input_revision))
        .unwrap_or(u64::MAX)
}

/// Mark asynchronous clipboard input pending, blocking injection through its completion.
pub(crate) fn clipboard_pending(surface: usize, pending: bool) {
    if let Ok(mut surfaces) = SURFACES.lock() {
        if let Some(surface) = surfaces.get_mut(&surface) {
            surface.input_revision = surface.input_revision.saturating_add(1);
            surface.pending_clipboard = if pending {
                surface.pending_clipboard.saturating_add(1)
            } else {
                surface.pending_clipboard.saturating_sub(1)
            };
        }
    }
}

/// Require all asynchronous clipboard deliveries to finish before accepting an empty-prompt observation.
pub(crate) fn input_pending(surface: usize) -> bool {
    SURFACES
        .lock()
        .ok()
        .and_then(|s| s.get(&surface).map(|s| s.pending_clipboard > 0))
        .unwrap_or(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Keep directory reports isolated between terminals and discard them on retirement.
    #[test]
    fn directory_lifetime_and_surface_isolation() {
        let first = usize::MAX;
        let second = usize::MAX - 1;
        register(first, u64::MAX, Some(std::path::Path::new("/launch")));
        register(second, u64::MAX - 1, None);
        assert_eq!(working_directory(first), "/launch");
        let revision = input_revision(first);
        record_input(first);
        assert_eq!(input_revision(first), revision + 1);
        assert!(!input_pending(first));
        clipboard_pending(first, true);
        clipboard_pending(first, true);
        assert!(input_pending(first));
        clipboard_pending(first, false);
        assert!(input_pending(first));
        clipboard_pending(first, false);
        assert!(!input_pending(first));
        assert_eq!(working_directory(second), "");
        set_working_directory(first, "/first");
        set_working_directory(second, "/second");
        assert_eq!(working_directory(first), "/first");
        assert_eq!(working_directory(second), "/second");
        assert_eq!(pane_id(first), Some(u64::MAX));
        unregister(first);
        set_working_directory(first, "/late");
        assert_eq!(working_directory(first), "");
        assert_eq!(pane_id(first), None);
        register(first, u64::MAX, None);
        assert_eq!(working_directory(first), "");
        unregister(first);
        unregister(second);
    }
}
