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

/// Borrow immutable launch dependencies while rebuilding one saved pane tree on GTK.
struct RestoreContext<'a> {
    ghostty_app: ffi::ghostty_app_t,
    working_directory: Option<&'a std::path::Path>,
    launch_command: Option<&'a str>,
    remote_launch: Option<&'a crate::ghostty::surface::SurfaceIoMode>,
}

impl RestoreContext<'_> {
    /// Create a restored terminal with shared launch precedence, UUID and context-menu wiring.
    /// Workspace directory overrides saved CWD; remote launch overrides a startup command.
    fn terminal(
        &self,
        pane_id: u64,
        uuid: Uuid,
        saved_cwd: &str,
        resume: Option<&crate::resume::ResumeBinding>,
    ) -> PaneSurface {
        let directory = self
            .working_directory
            .map(std::path::Path::to_path_buf)
            .or_else(|| (!saved_cwd.is_empty()).then(|| std::path::PathBuf::from(saved_cwd)));
        let launch = self.remote_launch.cloned().unwrap_or_else(|| {
            self.launch_command
                .map(|command| crate::ghostty::surface::SurfaceIoMode::Command(command.to_owned()))
                .unwrap_or(crate::ghostty::surface::SurfaceIoMode::Exec)
        });
        let (gl_area, _) = crate::ghostty::surface::create_surface(
            self.ghostty_app,
            None,
            directory,
            pane_id,
            launch,
        );
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
    pub fn from_data_with_command(
        ghostty_app: ffi::ghostty_app_t,
        data: &SplitNodeData,
        active_pane_uuid: Option<&str>,
        working_directory: Option<std::path::PathBuf>,
        launch_command: Option<String>,
        remote_launch: Option<crate::ghostty::surface::SurfaceIoMode>,
    ) -> Option<Self> {
        static NEXT_RESTORE_BASE: std::sync::atomic::AtomicU64 =
            std::sync::atomic::AtomicU64::new(1 << 24);
        let mut next_pane_id = NEXT_RESTORE_BASE.fetch_add(1 << 18, Ordering::Relaxed);
        let context = RestoreContext {
            ghostty_app,
            working_directory: working_directory.as_deref(),
            launch_command: launch_command.as_deref(),
            remote_launch: remote_launch.as_ref(),
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
            remote_launch,
        })
    }

    /// Recursively construct GTK panes from legacy or tabbed snapshots, limiting depth to 16.
    /// Native terminals initialize on realization; caller launch settings override saved defaults.
    fn node_from_data(
        context: &RestoreContext<'_>,
        data: &SplitNodeData,
        next_pane_id: &mut u64,
        depth: u32,
    ) -> Option<SplitNode> {
        if depth > 16 {
            eprintln!("cmux: session restore tree depth > 16, falling back (D-14)");
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
                    context.terminal(pane_id, *surface_uuid, cwd, None),
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
                        ..
                    } => context.terminal(pane_id, *surface_uuid, cwd, resume.as_ref()),
                    PaneSurfaceData::Browser { surface_uuid, url } => {
                        let mut widgets = crate::browser::create_preview_pane(pane_id);
                        widgets.uuid = *surface_uuid;
                        widgets.url_entry.set_text(url);
                        PaneSurface::Browser {
                            widgets,
                            uuid: *surface_uuid,
                        }
                    }
                });
                let initial = restored
                    .next()
                    .unwrap_or_else(|| context.terminal(pane_id, Uuid::new_v4(), "", None));
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
                paned.set_shrink_start_child(false);
                paned.set_shrink_end_child(false);
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
