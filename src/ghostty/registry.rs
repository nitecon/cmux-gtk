//! Metadata owned for exactly the lifetime of each registered Ghostty surface.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

/// Routing identity and the latest directory known for one terminal.
struct Surface {
    pane_id: u64,
    working_directory: String,
    input_revision: u64,
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
                working_directory: directory
                    .map(|path| path.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                input_revision: 0,
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

/// Invalidate a prompt snapshot before human input or programmatic paste; GTK serializes submission.
pub(crate) fn record_input(surface: usize) {
    if let Ok(mut surfaces) = SURFACES.lock() {
        if let Some(surface) = surfaces.get_mut(&surface) {
            surface.input_revision = surface.input_revision.wrapping_add(1);
        }
    }
}

/// Read the input generation without accessing the native handle.
pub(crate) fn input_revision(surface: usize) -> u64 {
    SURFACES
        .lock()
        .ok()
        .and_then(|surfaces| surfaces.get(&surface).map(|surface| surface.input_revision))
        .unwrap_or(0)
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
        assert_eq!(working_directory(second), "");
        record_input(first);
        record_input(first);
        assert_eq!(input_revision(first), 2);
        assert_eq!(input_revision(second), 0);
        set_working_directory(first, "/first");
        set_working_directory(second, "/second");
        assert_eq!(working_directory(first), "/first");
        assert_eq!(working_directory(second), "/second");
        assert_eq!(pane_id(first), Some(u64::MAX));
        unregister(first);
        set_working_directory(first, "/late");
        assert_eq!(working_directory(first), "");
        assert_eq!(input_revision(first), 0);
        assert_eq!(pane_id(first), None);
        register(first, u64::MAX, None);
        assert_eq!(working_directory(first), "");
        unregister(first);
        unregister(second);
    }
}
