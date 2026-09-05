//! Metadata owned for exactly the lifetime of each registered Ghostty surface.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

/// Routing identity and the latest directory known for one terminal.
struct Surface {
    pane_id: u64,
    working_directory: String,
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

/// Find any terminal in a pane when the caller has no selected-widget mapping.
pub(crate) fn first_surface(pane_id: u64) -> Option<usize> {
    SURFACES
        .lock()
        .ok()?
        .iter()
        .find(|(_, surface)| surface.pane_id == pane_id)
        .map(|(pointer, _)| *pointer)
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
        set_working_directory(first, "/first");
        set_working_directory(second, "/second");
        assert_eq!(working_directory(first), "/first");
        assert_eq!(working_directory(second), "/second");
        assert_eq!(pane_id(first), Some(u64::MAX));
        assert_eq!(first_surface(u64::MAX), Some(first));
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
