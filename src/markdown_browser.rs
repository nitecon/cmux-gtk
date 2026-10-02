//! Header file browser listing the active workspace's Markdown documents with a rendered reader.

use crate::app_state::AppStateRef;
use gtk4::{gio, glib, prelude::*};
use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

/// Directory levels searched below the workspace directory.
const MAX_DEPTH: usize = 4;
/// Listed documents; later matches are omitted so huge trees stay responsive.
const MAX_FILES: usize = 500;
/// Largest document read for rendering.
const MAX_BYTES: u64 = 2 * 1024 * 1024;

/// Show the Markdown browser for the active local workspace on the GTK thread.
/// Listing, reading and rendering run on blocking workers; only markup returns to GTK.
pub fn show(parent: &gtk4::ApplicationWindow, state: &AppStateRef) {
    let directory = {
        let state = state.borrow();
        state.local_workspace_directory(state.active_index)
    };
    let dialog = gtk4::Window::builder()
        .title("Markdown Files")
        .transient_for(parent)
        .default_width(1000)
        .default_height(700)
        .build();
    let list = gtk4::ListBox::new();
    list.set_selection_mode(gtk4::SelectionMode::Single);
    let status = gtk4::Label::new(Some("Searching for Markdown files…"));
    status.set_xalign(0.0);
    status.set_wrap(true);
    status.add_css_class("dim-label");
    status.set_margin_start(8);
    status.set_margin_end(8);
    status.set_margin_top(8);
    let sidebar = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
    sidebar.append(&status);
    sidebar.append(
        &gtk4::ScrolledWindow::builder()
            .vexpand(true)
            .hscrollbar_policy(gtk4::PolicyType::Never)
            .child(&list)
            .build(),
    );
    let document = gtk4::Label::new(Some("Select a document to read it."));
    document.set_xalign(0.0);
    document.set_yalign(0.0);
    document.set_wrap(true);
    document.set_wrap_mode(gtk4::pango::WrapMode::WordChar);
    document.set_selectable(true);
    document.set_margin_start(24);
    document.set_margin_end(24);
    document.set_margin_top(16);
    document.set_margin_bottom(24);
    let reader = gtk4::ScrolledWindow::builder()
        .hscrollbar_policy(gtk4::PolicyType::Never)
        .hexpand(true)
        .child(&document)
        .build();
    let paned = gtk4::Paned::new(gtk4::Orientation::Horizontal);
    paned.set_start_child(Some(&sidebar));
    paned.set_end_child(Some(&reader));
    paned.set_resize_start_child(false);
    paned.set_shrink_start_child(false);
    paned.set_position(280);
    dialog.set_child(Some(&paned));
    dialog.present();

    let Some(directory) = directory else {
        status.set_text("The active workspace has no local directory.");
        return;
    };
    // Monotonic selection generation; a finished read renders only if still current.
    let generation = Rc::new(Cell::new(0u64));
    list.connect_row_selected({
        let document = document.downgrade();
        let reader = reader.downgrade();
        let generation = generation.clone();
        move |_, row| {
            let Some(path) = row.and_then(|row| row.tooltip_text()) else {
                return;
            };
            let current = generation.get() + 1;
            generation.set(current);
            let document = document.clone();
            let reader = reader.clone();
            let generation = generation.clone();
            glib::MainContext::default().spawn_local(async move {
                let path = PathBuf::from(path.as_str());
                let markup = gio::spawn_blocking(move || render_file(&path))
                    .await
                    .unwrap_or_else(|_| "Document rendering failed.".to_owned());
                let (Some(document), Some(reader)) = (document.upgrade(), reader.upgrade()) else {
                    return;
                };
                if generation.get() == current {
                    document.set_markup(&markup);
                    reader.vadjustment().set_value(0.0);
                }
            });
        }
    });
    let list = list.downgrade();
    let status = status.downgrade();
    glib::MainContext::default().spawn_local(async move {
        let root = directory.clone();
        let files = gio::spawn_blocking(move || markdown_files(&root))
            .await
            .unwrap_or_default();
        let (Some(list), Some(status)) = (list.upgrade(), status.upgrade()) else {
            return;
        };
        status.set_text(&format!(
            "{} Markdown files in {}",
            files.len(),
            directory.display()
        ));
        for path in &files {
            let relative = path.strip_prefix(&directory).unwrap_or(path);
            let label = gtk4::Label::new(Some(&relative.to_string_lossy()));
            label.set_xalign(0.0);
            label.set_ellipsize(gtk4::pango::EllipsizeMode::Start);
            label.set_margin_start(8);
            label.set_margin_end(8);
            label.set_margin_top(4);
            label.set_margin_bottom(4);
            let row = gtk4::ListBoxRow::new();
            row.set_child(Some(&label));
            row.set_tooltip_text(Some(&path.to_string_lossy()));
            list.append(&row);
        }
    });
}

/// Collect sorted Markdown files below `root`, skipping hidden directories.
/// Bounded by `MAX_DEPTH` and `MAX_FILES`; unreadable directories are ignored. Blocking.
fn markdown_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut pending = vec![(root.to_path_buf(), 0)];
    while let Some((directory, depth)) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let hidden = entry.file_name().to_string_lossy().starts_with('.');
            if kind.is_dir() {
                if !hidden && depth < MAX_DEPTH {
                    pending.push((path, depth + 1));
                }
            } else if is_markdown(&path) && files.len() < MAX_FILES {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

/// Recognize Markdown documents by extension, ignoring case.
fn is_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            ["md", "markdown"]
                .iter()
                .any(|known| extension.eq_ignore_ascii_case(known))
        })
}

/// Read a bounded UTF-8 document and return label markup, or an escaped error message. Blocking.
fn render_file(path: &Path) -> String {
    let size = std::fs::metadata(path).map(|metadata| metadata.len());
    match size {
        Ok(size) if size > MAX_BYTES => "Document is too large to display.".to_owned(),
        Ok(_) => match std::fs::read_to_string(path) {
            Ok(text) => crate::workspace_metadata::markdown_document_markup(&text),
            Err(error) => {
                glib::markup_escape_text(&format!("Cannot read document: {error}")).into()
            }
        },
        Err(error) => glib::markup_escape_text(&format!("Cannot read document: {error}")).into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Listing finds nested Markdown by extension and skips hidden directories and other files.
    #[test]
    fn lists_visible_markdown_files() {
        let root = std::env::temp_dir().join(format!("cmux-md-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("Docs")).unwrap();
        std::fs::create_dir_all(root.join(".git")).unwrap();
        for file in [
            "README.md",
            "Docs/Guide.MARKDOWN",
            ".git/HEAD.md",
            "main.rs",
        ] {
            std::fs::write(root.join(file), "# Title").unwrap();
        }
        let files = markdown_files(&root);
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(
            files,
            vec![root.join("Docs/Guide.MARKDOWN"), root.join("README.md")]
        );
    }

    /// Document markup sizes headings, numbers ordered lists and escapes raw HTML.
    #[test]
    fn document_markup_is_readable_and_escaped() {
        let markup = crate::workspace_metadata::markdown_document_markup(
            "# Title\n\nSome\ntext <b>x</b>\n\n1. one\n2. two\n   - nested",
        );
        assert!(markup.starts_with("<span size='xx-large' weight='bold'>Title</span>\n\n"));
        assert!(markup.contains("Some text &lt;b&gt;x&lt;/b&gt;"));
        assert!(markup.contains("1. one\n2. two\n    • nested"));
    }
}
