//! Rebuild saved workspace layouts on the GTK main thread.
//!
//! Restored terminals are newly launched surfaces. This module preserves saved
//! identities and selection; it does not resume operating-system processes.

use super::{
    append_pane_surface, attach_terminal_context_menu, collect_leaves_in_order, create_pane,
    PaneSurface, PaneSurfaceData, SplitEngine, SplitNode, SplitNodeData,
};
use crate::ghostty::ffi;
use gtk4::prelude::*;
use std::sync::atomic::Ordering;
use uuid::Uuid;

/// Reject an over-deep tree before constructing any GTK objects or scheduling layout callbacks.
fn valid_depth(data: &SplitNodeData, depth: usize) -> bool {
    if depth > crate::project_config::project_action::MAX_LAYOUT_DEPTH {
        return false;
    }
    match data {
        SplitNodeData::Split { start, end, .. } => {
            valid_depth(start, depth + 1) && valid_depth(end, depth + 1)
        }
        _ => true,
    }
}

/// Borrow immutable launch dependencies while rebuilding one saved pane tree on GTK.
struct RestoreContext<'a> {
    ghostty_app: ffi::ghostty_app_t,
    resume_policy: &'a crate::resume_policy::ResumePolicy,
    working_directory: Option<&'a std::path::Path>,
    launch_command: Option<&'a str>,
    launch_environment: &'a std::collections::BTreeMap<String, String>,
    remote_launch: Option<&'a crate::ghostty::surface::SurfaceIoMode>,
    remote_workspace: bool,
}

impl RestoreContext<'_> {
    /// Create a restored terminal with shared launch precedence, UUID and context-menu wiring.
    /// Plain terminals resume their last CWD; explicit startup/remote launches retain workspace context.
    #[allow(clippy::too_many_arguments)] // Explicit per-surface launch data avoids ambient mutable state.
    fn terminal(
        &self,
        pane_id: u64,
        uuid: Uuid,
        saved_cwd: &str,
        resume: Option<&crate::resume::ResumeBinding>,
        scrollback: Option<&std::sync::Arc<str>>,
        environment: Option<&std::collections::BTreeMap<String, String>>,
        initial_input: Option<&str>,
    ) -> PaneSurface {
        let saved_directory = (!saved_cwd.is_empty()).then(|| std::path::PathBuf::from(saved_cwd));
        let workspace_directory = self.working_directory.map(std::path::Path::to_path_buf);
        let directory = if self.launch_command.is_some() || self.remote_launch.is_some() {
            workspace_directory.or(saved_directory)
        } else {
            saved_directory.or(workspace_directory)
        };
        let launch = if let Some(mut remote) = self.remote_launch.cloned() {
            if let crate::ghostty::surface::SurfaceIoMode::Remote { initial_input, .. } =
                &mut remote
            {
                *initial_input =
                    resume.and_then(|binding| self.resume_policy.remote_shell_input(binding));
            }
            remote
        } else {
            let mut launch_environment = self.launch_environment.clone();
            launch_environment.extend(
                environment
                    .into_iter()
                    .flatten()
                    .map(|(key, value)| (key.clone(), value.clone())),
            );
            crate::ghostty::surface::SurfaceIoMode::Configured {
                initial_input: initial_input.map(str::to_owned),
                command: if self.remote_workspace {
                    self.launch_command.map(str::to_owned)
                } else {
                    resume
                        .and_then(|binding| self.resume_policy.launch_command(binding))
                        .or_else(|| self.launch_command.map(str::to_owned))
                },
                environment: launch_environment,
            }
        };
        let (gl_area, _) = crate::ghostty::surface::create_surface(
            self.ghostty_app,
            None,
            directory,
            pane_id,
            launch,
        );
        crate::scrollback::prepare(&gl_area, scrollback);
        attach_terminal_context_menu(&gl_area);
        PaneSurface::Terminal {
            gl_area,
            uuid,
            resume: resume
                .filter(|binding| binding.validate().is_ok())
                .cloned()
                .map(|mut binding| {
                    binding.sanitize_environment();
                    binding
                }),
        }
    }
}

impl SplitEngine {
    /// Rebuild a saved tree with fresh pane IDs and the supplied launch context.
    /// Preserve surface UUIDs; reject excessive nesting and fall back to the first pane for focus.
    #[allow(clippy::too_many_arguments)] // Explicit immutable restore dependencies; no global launch state.
    pub fn from_data_with_command(
        ghostty_app: ffi::ghostty_app_t,
        data: &SplitNodeData,
        active_pane_uuid: Option<&str>,
        working_directory: Option<std::path::PathBuf>,
        launch_command: Option<String>,
        remote_launch: Option<crate::ghostty::surface::SurfaceIoMode>,
        remote_workspace: bool,
        resume_policy: &crate::resume_policy::ResumePolicy,
        launch_environment: std::collections::BTreeMap<String, String>,
    ) -> Option<Self> {
        if !valid_depth(data, 0) {
            eprintln!(
                "cmux: session restore tree exceeds project layout depth limit, falling back"
            );
            return None;
        }
        static NEXT_RESTORE_BASE: std::sync::atomic::AtomicU64 =
            std::sync::atomic::AtomicU64::new(1 << 24);
        let mut next_pane_id = NEXT_RESTORE_BASE.fetch_add(1 << 18, Ordering::Relaxed);
        let context = RestoreContext {
            ghostty_app,
            resume_policy,
            working_directory: working_directory.as_deref(),
            launch_command: launch_command.as_deref(),
            launch_environment: &launch_environment,
            remote_launch: remote_launch.as_ref(),
            remote_workspace,
        };
        let root = Self::node_from_data(&context, data, &mut next_pane_id, 0)?;
        // Find active pane by saved UUID, or fall back to first leaf
        let active_id = active_pane_uuid
            .and_then(|uuid_str| root.find_pane_id_by_uuid(uuid_str))
            .unwrap_or_else(|| {
                let mut leaves = Vec::new();
                collect_leaves_in_order(&root, &mut leaves);
                leaves.first().copied().unwrap_or(1)
            });
        root.update_focus_css(active_id);
        Some(SplitEngine {
            root,
            active_pane_id: active_id,
            next_pane_id,
            ghostty_app,
            working_directory,
            launch_command,
            launch_environment,
            remote_launch,
        })
    }

    /// Recursively construct GTK panes from legacy or tabbed snapshots, using the same depth bound as project layouts.
    /// Native terminals initialize on realization; caller launch settings override saved defaults.
    fn node_from_data(
        context: &RestoreContext<'_>,
        data: &SplitNodeData,
        next_pane_id: &mut u64,
        depth: u32,
    ) -> Option<SplitNode> {
        if depth as usize > crate::project_config::project_action::MAX_LAYOUT_DEPTH {
            eprintln!(
                "cmux: session restore tree exceeds project layout depth limit, falling back"
            );
            return None;
        }
        match data {
            SplitNodeData::Leaf {
                surface_uuid, cwd, ..
            } => {
                let pane_id = *next_pane_id;
                *next_pane_id += 1;
                Some(create_pane(
                    pane_id,
                    context.terminal(pane_id, *surface_uuid, cwd, None, None, None, None),
                ))
            }
            SplitNodeData::Pane {
                active_surface_uuid,
                surfaces,
            } => {
                let pane_id = *next_pane_id;
                *next_pane_id += 1;
                let mut restored = surfaces.iter().map(|surface| match surface {
                    PaneSurfaceData::Terminal {
                        surface_uuid,
                        cwd,
                        resume,
                        scrollback,
                        environment,
                        initial_input,
                        ..
                    } => context.terminal(
                        pane_id,
                        *surface_uuid,
                        cwd,
                        resume.as_ref(),
                        scrollback.as_ref(),
                        Some(environment),
                        initial_input.as_deref(),
                    ),
                    PaneSurfaceData::Browser {
                        surface_uuid,
                        url,
                        profile,
                    } => {
                        let mut widgets = crate::browser::create_preview_pane(pane_id);
                        widgets.uuid = *surface_uuid;
                        widgets.url_entry.set_text(url);
                        widgets.profile = profile
                            .as_deref()
                            .and_then(crate::browser::profile_selector);
                        PaneSurface::Browser {
                            widgets,
                            uuid: *surface_uuid,
                        }
                    }
                });
                let initial = restored.next().unwrap_or_else(|| {
                    context.terminal(pane_id, Uuid::new_v4(), "", None, None, None, None)
                });
                let node = create_pane(pane_id, initial);
                if let SplitNode::Leaf {
                    notebook,
                    surfaces: pane_surfaces,
                    ..
                } = &node
                {
                    for surface in restored {
                        append_pane_surface(notebook, pane_surfaces, surface, false);
                    }
                    if let Some(active_uuid) = active_surface_uuid {
                        if let Some(index) = pane_surfaces
                            .borrow()
                            .iter()
                            .position(|surface| surface.uuid() == *active_uuid)
                        {
                            notebook.set_current_page(Some(index as u32));
                        }
                    }
                }
                Some(node)
            }
            SplitNodeData::Split {
                orientation,
                ratio,
                start,
                end,
            } => {
                let start_node = Self::node_from_data(context, start, next_pane_id, depth + 1)?;
                let end_node = Self::node_from_data(context, end, next_pane_id, depth + 1)?;
                let gtk_orientation = match orientation.as_str() {
                    "vertical" => gtk4::Orientation::Vertical,
                    _ => gtk4::Orientation::Horizontal,
                };
                let paned = gtk4::Paned::new(gtk_orientation);
                paned.set_resize_start_child(true);
                paned.set_resize_end_child(true);
                // Allow constrained descendants below natural size so a bounded deep
                // session cannot monopolize GTK size negotiation before socket readiness.
                paned.set_shrink_start_child(true);
                paned.set_shrink_end_child(true);
                paned.set_wide_handle(true);
                paned.set_start_child(Some(&start_node.widget()));
                paned.set_end_child(Some(&end_node.widget()));
                // D-03: restore ratio after layout pass
                let saved_ratio = *ratio;
                let paned_ref = paned.clone();
                let orient = gtk_orientation;
                gtk4::glib::idle_add_local_once(move || {
                    let size = if orient == gtk4::Orientation::Horizontal {
                        paned_ref.width()
                    } else {
                        paned_ref.height()
                    };
                    if size > 0 {
                        paned_ref.set_position((size as f64 * saved_ratio) as i32);
                    }
                });
                Some(SplitNode::Split {
                    orientation: gtk_orientation,
                    paned,
                    start: Box::new(start_node),
                    end: Box::new(end_node),
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leaf() -> SplitNodeData {
        SplitNodeData::Leaf {
            pane_id: 1,
            surface_uuid: Uuid::new_v4(),
            shell: String::new(),
            cwd: String::new(),
        }
    }

    /// Validate both branches before GTK construction, accepting exactly the shared depth bound.
    #[test]
    fn depth_preflight_checks_late_branches() {
        let mut tree = leaf();
        for _ in 0..crate::project_config::project_action::MAX_LAYOUT_DEPTH {
            tree = SplitNodeData::Split {
                orientation: "horizontal".into(),
                ratio: 0.5,
                start: Box::new(leaf()),
                end: Box::new(tree),
            };
        }
        assert!(valid_depth(&tree, 0));
        let too_deep = SplitNodeData::Split {
            orientation: "vertical".into(),
            ratio: 0.5,
            start: Box::new(leaf()),
            end: Box::new(tree),
        };
        assert!(!valid_depth(&too_deep, 0));
    }
}
