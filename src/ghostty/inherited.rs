//! Ownership of configuration returned by Ghostty's inherited-surface API.
use super::ffi;
use std::sync::atomic::{AtomicUsize, Ordering};

static LIVE_DIRECTORIES: AtomicUsize = AtomicUsize::new(0);

/// Own the inherited working-directory allocation until deferred initialization is retired.
/// Never clone this owner: the native configuration is a shallow pointer-bearing value.
pub(crate) struct InheritedConfig(ffi::ghostty_surface_config_s);

impl InheritedConfig {
    /// Capture settings for a new split or tab and assume ownership of its allocated directory.
    ///
    /// # Safety
    /// Call on GTK with a live Ghostty surface. The vendored newSurfaceOptions returns
    /// a fresh dupeZ directory from the global allocator; other fields are borrowed or scalar.
    pub(crate) unsafe fn from_surface(
        surface: ffi::ghostty_surface_t,
        context: ffi::ghostty_surface_context_e,
    ) -> Self {
        // SAFETY: caller guarantees a live surface and serialized native access.
        let config = unsafe { ffi::ghostty_surface_inherited_config(surface, context) };
        if !config.working_directory.is_null() {
            LIVE_DIRECTORIES.fetch_add(1, Ordering::Relaxed);
        }
        Self(config)
    }

    /// Copy scalar settings and borrowed pointers for native creation while this owner stays alive.
    pub(super) fn config(&self) -> ffi::ghostty_surface_config_s {
        self.0
    }
}

impl Drop for InheritedConfig {
    /// Release only the owned sentinel-terminated directory through Ghostty's global allocator.
    fn drop(&mut self) {
        if !self.0.working_directory.is_null() {
            // SAFETY: from_surface owns this unique dupeZ allocation. String length
            // excludes its NUL, and sentinel=true matches Ghostty's dupeZ allocation.
            unsafe {
                let directory = std::ffi::CStr::from_ptr(self.0.working_directory);
                ffi::ghostty_string_free(ffi::ghostty_string_s {
                    ptr: self.0.working_directory.cast(),
                    len: directory.to_bytes().len(),
                    sentinel: true,
                });
            }
            LIVE_DIRECTORIES.fetch_sub(1, Ordering::Relaxed);
        }
    }
}

/// Count native directory allocations still owned by pending or realized terminal widgets.
pub(crate) fn live_directories() -> usize {
    LIVE_DIRECTORIES.load(Ordering::Relaxed)
}
