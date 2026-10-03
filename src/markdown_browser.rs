//! Native Files surface: bounded directory discovery, expandable folders and structured Markdown.

use gtk4::{gio, glib, prelude::*};
use pulldown_cmark::{Alignment, Event, Options, Parser, Tag, TagEnd};
use std::cell::Cell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

/// Directory levels searched below the workspace directory.
const MAX_DEPTH: usize = 4;
/// Listed documents; later matches are omitted so huge trees stay responsive.
const MAX_FILES: usize = 500;
/// Largest document read for rendering.
const MAX_BYTES: u64 = 2 * 1024 * 1024;

/// Worker-produced document blocks. Tables retain column alignment and individual cell markup.
#[derive(Debug)]
enum Block {
    Markup(String),
    Code(String),
    Table {
        alignments: Vec<Alignment>,
        rows: Vec<Vec<String>>,
    },
}

/// Relative directory hierarchy, ordered by name with folders displayed before documents.
#[derive(Default, Debug)]
struct Folder {
    folders: BTreeMap<String, Folder>,
    files: Vec<PathBuf>,
}

impl Folder {
    /// Build the displayed hierarchy from discovered paths without further filesystem access.
    fn from_paths(root: &Path, paths: &[PathBuf]) -> Self {
        let mut tree = Self::default();
        for path in paths {
            let Ok(relative) = path.strip_prefix(root) else {
                continue;
            };
            let mut folder = &mut tree;
            if let Some(parent) = relative.parent() {
                for component in parent.components() {
                    folder = folder
                        .folders
                        .entry(component.as_os_str().to_string_lossy().into_owned())
                        .or_default();
                }
            }
            folder.files.push(path.clone());
        }
        tree
    }
}

/// Construct a Files tab on GTK. Workers return owned data and weak callbacks never retain the tab.
/// Remote workspaces without a local directory display an explanation instead of scanning local files.
pub fn create(directory: Option<PathBuf>) -> gtk4::Widget {
    let status = gtk4::Label::new(Some("Searching for Markdown files…"));
    status.set_xalign(0.0);
    status.set_wrap(true);
    status.add_css_class("dim-label");
    let tree = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
    tree.add_css_class("files-tree");
    let sidebar = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
    sidebar.set_margin_start(8);
    sidebar.set_margin_end(8);
    sidebar.set_margin_top(8);
    sidebar.append(&status);
    sidebar.append(
        &gtk4::ScrolledWindow::builder()
            .vexpand(true)
            .hscrollbar_policy(gtk4::PolicyType::Never)
            .child(&tree)
            .build(),
    );
    let title = gtk4::Label::new(Some("Select a document"));
    title.set_xalign(0.0);
    title.set_ellipsize(gtk4::pango::EllipsizeMode::Middle);
    title.add_css_class("heading");
    let document = gtk4::Box::new(gtk4::Orientation::Vertical, 16);
    document.set_margin_start(20);
    document.set_margin_end(20);
    document.set_margin_top(16);
    document.set_margin_bottom(24);
    document.append(&text_label("Select a document from the file tree.", false));
    let reader = gtk4::ScrolledWindow::builder()
        .hexpand(true)
        .vexpand(true)
        .hscrollbar_policy(gtk4::PolicyType::Automatic)
        .child(&document)
        .build();
    let preview = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
    preview.set_margin_top(8);
    preview.append(&title);
    preview.append(&reader);
    let paned = gtk4::Paned::new(gtk4::Orientation::Vertical);
    paned.set_start_child(Some(&sidebar));
    paned.set_end_child(Some(&preview));
    paned.set_resize_start_child(false);
    paned.set_shrink_start_child(true);
    paned.set_shrink_end_child(true);
    paned.set_position(200);
    paned.set_wide_handle(true);
    paned.set_hexpand(true);
    paned.set_vexpand(true);
    let Some(directory) = directory else {
        status.set_text("This workspace has no local directory.");
        return paned.upcast();
    };
    let generation = Rc::new(Cell::new(0u64));
    let tree = tree.downgrade();
    let status = status.downgrade();
    let document = document.downgrade();
    let reader = reader.downgrade();
    let title = title.downgrade();
    glib::MainContext::default().spawn_local(async move {
        let root = directory.clone();
        let files = gio::spawn_blocking(move || markdown_files(&root))
            .await
            .unwrap_or_default();
        let (Some(tree), Some(status), Some(document), Some(reader), Some(title)) = (
            tree.upgrade(),
            status.upgrade(),
            document.upgrade(),
            reader.upgrade(),
            title.upgrade(),
        ) else {
            return;
        };
        let summary = if files.is_empty() {
            "No Markdown files found".to_owned()
        } else {
            format!("{} Markdown files", files.len())
        };
        status.set_text(&format!(
            "{summary} · {}",
            directory
                .file_name()
                .unwrap_or(directory.as_os_str())
                .to_string_lossy()
        ));
        status.set_tooltip_text(Some(&format!(
            "{}\nSearch depth: {MAX_DEPTH}; at most {MAX_FILES} files",
            directory.display()
        )));
        append_folder(
            &tree,
            &Folder::from_paths(&directory, &files),
            &directory,
            &document,
            &reader,
            &title,
            &generation,
        );
    });
    paned.upcast()
}

/// Append native expanders and selectable file buttons; selection reads asynchronously with stale-result protection.
fn append_folder(
    parent: &gtk4::Box,
    folder: &Folder,
    root: &Path,
    document: &gtk4::Box,
    reader: &gtk4::ScrolledWindow,
    title: &gtk4::Label,
    generation: &Rc<Cell<u64>>,
) {
    for (name, folder) in &folder.folders {
        let expander = gtk4::Expander::new(Some(name));
        let children = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
        children.set_margin_start(16);
        append_folder(&children, folder, root, document, reader, title, generation);
        expander.set_child(Some(&children));
        parent.append(&expander);
    }
    for path in &folder.files {
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        let button = gtk4::Button::new();
        button.add_css_class("flat");
        let label = gtk4::Label::new(Some(&name));
        label.set_xalign(0.0);
        label.set_ellipsize(gtk4::pango::EllipsizeMode::Middle);
        button.set_child(Some(&label));
        button.set_tooltip_text(Some(&path.to_string_lossy()));
        button.connect_clicked({
            let path = path.clone();
            let relative = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .into_owned();
            let document = document.downgrade();
            let reader = reader.downgrade();
            let title = title.downgrade();
            let generation = generation.clone();
            move |_| {
                let current = generation.get().wrapping_add(1);
                generation.set(current);
                if let Some(title) = title.upgrade() {
                    title.set_text(&relative);
                }
                if let Some(document) = document.upgrade() {
                    display_blocks(&document, vec![Block::Markup("Loading…".into())]);
                }
                let path = path.clone();
                let document = document.clone();
                let reader = reader.clone();
                let generation = generation.clone();
                glib::MainContext::default().spawn_local(async move {
                    let blocks = gio::spawn_blocking(move || read_document(&path))
                        .await
                        .unwrap_or_else(|_| {
                            vec![Block::Markup("Document rendering failed.".into())]
                        });
                    if generation.get() != current {
                        return;
                    }
                    let (Some(document), Some(reader)) = (document.upgrade(), reader.upgrade())
                    else {
                        return;
                    };
                    display_blocks(&document, blocks);
                    reader.vadjustment().set_value(0.0);
                    reader.hadjustment().set_value(0.0);
                });
            }
        });
        parent.append(&button);
    }
}

/// Create a selectable, wrapping label; markup is generated only by the escaping renderer.
fn text_label(text: &str, markup: bool) -> gtk4::Label {
    let label = gtk4::Label::new(None);
    if markup {
        label.set_markup(text);
    } else {
        label.set_text(text);
    }
    label.set_xalign(0.0);
    label.set_yalign(0.0);
    label.set_wrap(true);
    label.set_wrap_mode(gtk4::pango::WrapMode::WordChar);
    label.set_selectable(true);
    label
}

/// Replace the preview with structured GTK content, retaining whitespace in code and aligned table cells.
fn display_blocks(document: &gtk4::Box, blocks: Vec<Block>) {
    while let Some(child) = document.first_child() {
        document.remove(&child);
    }
    for block in blocks {
        match block {
            Block::Markup(markup) => document.append(&text_label(&markup, true)),
            Block::Code(code) => {
                let label = text_label(&code, false);
                label.set_wrap(false);
                label.add_css_class("monospace");
                let frame = gtk4::Frame::new(None);
                label.set_margin_start(12);
                label.set_margin_end(12);
                label.set_margin_top(12);
                label.set_margin_bottom(12);
                frame.set_child(Some(&label));
                document.append(&frame);
            }
            Block::Table { alignments, rows } => {
                let grid = gtk4::Grid::new();
                grid.set_hexpand(true);
                for (row, cells) in rows.into_iter().enumerate() {
                    for (column, markup) in cells.into_iter().enumerate() {
                        let label = text_label(&markup, true);
                        label.set_hexpand(true);
                        label.set_margin_start(10);
                        label.set_margin_end(10);
                        label.set_margin_top(8);
                        label.set_margin_bottom(8);
                        if row == 0 {
                            label.add_css_class("heading");
                        }
                        label.set_xalign(match alignments.get(column) {
                            Some(Alignment::Right) => 1.0,
                            Some(Alignment::Center) => 0.5,
                            _ => 0.0,
                        });
                        let frame = gtk4::Frame::new(None);
                        frame.set_child(Some(&label));
                        grid.attach(&frame, column as i32, row as i32, 1, 1);
                    }
                }
                document.append(&grid);
            }
        }
    }
}

/// Flush ordinary Markdown events around tables and code without losing inline formatting.
fn flush_markup<'a>(
    events: &mut Vec<Event<'a>>,
    blocks: &mut Vec<Block>,
    state: &mut crate::workspace_metadata::MarkupState,
) {
    if !events.is_empty() {
        let markup =
            crate::workspace_metadata::render_fragment(events.drain(..), true, true, true, state);
        if !markup.is_empty() {
            blocks.push(Block::Markup(markup));
        }
    }
}

/// Parse CommonMark plus tables, task lists and strikethrough into worker-safe render blocks.
fn parse_document(text: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut ordinary = Vec::new();
    let mut state = crate::workspace_metadata::MarkupState::default();
    let mut events = Parser::new_ext(
        text,
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS,
    );
    while let Some(event) = events.next() {
        match event {
            Event::Start(Tag::Table(alignments)) => {
                flush_markup(&mut ordinary, &mut blocks, &mut state);
                let mut rows = Vec::new();
                let mut row = Vec::new();
                let mut cell = Vec::new();
                for event in events.by_ref() {
                    match event {
                        Event::End(TagEnd::Table) => break,
                        Event::Start(Tag::TableHead | Tag::TableRow | Tag::TableCell) => {}
                        Event::End(TagEnd::TableHead | TagEnd::TableRow) => {
                            rows.push(std::mem::take(&mut row))
                        }
                        Event::End(TagEnd::TableCell) => {
                            row.push(crate::workspace_metadata::render_events(
                                cell.drain(..),
                                true,
                                true,
                                true,
                            ))
                        }
                        event => cell.push(event),
                    }
                }
                blocks.push(Block::Table { alignments, rows });
            }
            Event::Start(Tag::CodeBlock(_)) => {
                flush_markup(&mut ordinary, &mut blocks, &mut state);
                let mut code = String::new();
                for event in events.by_ref() {
                    match event {
                        Event::End(TagEnd::CodeBlock) => break,
                        Event::Text(text) => code.push_str(&text),
                        _ => {}
                    }
                }
                blocks.push(Block::Code(code));
            }
            event => ordinary.push(event),
        }
    }
    flush_markup(&mut ordinary, &mut blocks, &mut state);
    if blocks.is_empty() {
        blocks.push(Block::Markup("This document is empty.".into()));
    }
    blocks
}

/// Collect sorted Markdown files below `root`, skipping hidden directories and symlinks. Blocking.
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
            } else if kind.is_file() && is_markdown(&path) {
                files.push(path);
                if files.len() == MAX_FILES {
                    files.sort();
                    return files;
                }
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

/// Read at most MAX_BYTES plus one byte, rejecting oversized/non-UTF-8 documents. Blocking.
fn read_document(path: &Path) -> Vec<Block> {
    use std::io::Read;
    let result = (|| -> std::io::Result<String> {
        let file = std::fs::File::open(path)?;
        let mut text = String::new();
        file.take(MAX_BYTES + 1).read_to_string(&mut text)?;
        Ok(text)
    })();
    match result {
        Ok(text) if text.len() as u64 > MAX_BYTES => {
            vec![Block::Markup("Document is too large to display.".into())]
        }
        Ok(text) => parse_document(&text),
        Err(error) => vec![Block::Markup(
            glib::markup_escape_text(&format!("Cannot read document: {error}")).into(),
        )],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Find real GTK descendants by type for behavior assertions without CSS/source matching.
    fn descendants<T: IsA<gtk4::Widget> + glib::object::IsClass>(widget: &gtk4::Widget) -> Vec<T> {
        let mut found = Vec::new();
        if let Ok(value) = widget.clone().downcast::<T>() {
            found.push(value);
        }
        let mut child = widget.first_child();
        while let Some(widget) = child {
            found.extend(descendants::<T>(&widget));
            child = widget.next_sibling();
        }
        found
    }

    /// Drive asynchronous reads and GTK layout with a deadline on the main thread.
    fn wait_until(mut ready: impl FnMut() -> bool) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !ready() {
            assert!(
                std::time::Instant::now() < deadline,
                "Files UI did not converge"
            );
            for _ in 0..8 {
                glib::MainContext::default().iteration(false);
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    /// Folder expansion reveals documents; selecting one renders actual table and code widgets.
    #[test]
    #[ignore = "requires GTK display; run in GitHub Actions under Xvfb"]
    fn files_viewer_tree_and_rendering() {
        gtk4::init().unwrap();
        let root = std::env::temp_dir().join(format!("cmux-files-ui-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("Docs")).unwrap();
        std::fs::write(root.join("Docs/Guide.md"), "# Guide\n\nFirst\nsecond\n\n| Key | Value |\n| --- | ---: |\n| **Alpha** | 42 |\n\n```\n  indented\n\n    deeper\n```\n").unwrap();
        let widget = create(Some(root.clone()));
        let window = gtk4::Window::new();
        window.set_default_size(600, 700);
        window.set_child(Some(&widget));
        window.present();
        wait_until(|| !descendants::<gtk4::Expander>(&widget).is_empty());
        let expander = descendants::<gtk4::Expander>(&widget).remove(0);
        assert!(!expander.is_expanded());
        // GtkExpander retains its collapsed child outside the visible widget hierarchy.
        let folder = expander.child().expect("folder content");
        let button = descendants::<gtk4::Button>(&folder)
            .into_iter()
            .find(|button| {
                button
                    .tooltip_text()
                    .is_some_and(|path| path.ends_with("Guide.md"))
            })
            .unwrap();
        assert!(!button.is_mapped());
        expander.set_expanded(true);
        wait_until(|| button.is_mapped());
        button.emit_clicked();
        wait_until(|| !descendants::<gtk4::Grid>(&widget).is_empty());
        let grid = descendants::<gtk4::Grid>(&widget).remove(0);
        for (column, row, expected) in [
            (0, 0, "Key"),
            (1, 0, "Value"),
            (0, 1, "Alpha"),
            (1, 1, "42"),
        ] {
            let cell = grid.child_at(column, row).unwrap();
            let labels = descendants::<gtk4::Label>(&cell);
            assert_eq!(labels[0].text(), expected);
            if column == 1 {
                assert_eq!(labels[0].xalign(), 1.0);
            }
        }
        let labels = descendants::<gtk4::Label>(&widget);
        assert!(labels
            .iter()
            .any(|label| label.text().contains("First\nsecond")));
        assert!(labels
            .iter()
            .any(|label| label.text() == "  indented\n\n    deeper\n"));
        expander.set_expanded(false);
        wait_until(|| !button.is_mapped());
        window.close();
        std::fs::remove_dir_all(root).unwrap();
    }

    /// Discovery excludes hidden directories and non-Markdown files, and builds nested folders.
    #[test]
    fn lists_visible_markdown_files() {
        let root = std::env::temp_dir().join(format!("cmux-md-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("Docs/Nested")).unwrap();
        std::fs::create_dir_all(root.join(".git")).unwrap();
        for file in [
            "README.md",
            "Docs/Guide.MARKDOWN",
            "Docs/Nested/more.md",
            ".git/HEAD.md",
            "main.rs",
        ] {
            std::fs::write(root.join(file), "# Title").unwrap();
        }
        let files = markdown_files(&root);
        let tree = Folder::from_paths(&root, &files);
        assert_eq!(tree.files, vec![root.join("README.md")]);
        assert_eq!(
            tree.folders["Docs"].files,
            vec![root.join("Docs/Guide.MARKDOWN")]
        );
        assert_eq!(
            tree.folders["Docs"].folders["Nested"].files,
            vec![root.join("Docs/Nested/more.md")]
        );
        assert_eq!(files.len(), 3);
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// A native code block must not break surrounding quote markup or ordered-list numbering.
    #[test]
    fn structured_markdown_keeps_surrounding_context() {
        let blocks =
            parse_document("> 3. Before\n>\n>    ```\n>    code\n>    ```\n>\n> 4. After **bold**");
        let markup: Vec<_> = blocks
            .iter()
            .filter_map(|block| match block {
                Block::Markup(text) => Some(text),
                _ => None,
            })
            .collect();
        assert!(markup[0].contains("3. Before"));
        assert!(markup.last().unwrap().contains("4. After <b>bold</b>"));
        for text in markup {
            gtk4::pango::parse_markup(text, '\0').expect("balanced Pango markup");
        }
    }

    /// Tables keep formatted cells/alignment, and prose/code preserve line breaks and indentation.
    #[test]
    fn structured_markdown_preserves_tables_and_whitespace() {
        let blocks = parse_document("# Title\n\nSome\ntext  \nbreak\n\n```rust\n  a < b\n\n    c\n```\n\n| Name | Value |\n| :--- | ---: |\n| **bold** | `a<b` |\n\nAfter <b>raw</b>");
        let Block::Markup(prose) = &blocks[0] else {
            panic!("missing prose")
        };
        assert!(prose.contains("Some\ntext\nbreak"));
        assert!(prose.contains("weight='bold'>Title"));
        let Block::Code(code) = &blocks[1] else {
            panic!("missing code")
        };
        assert_eq!(code, "  a < b\n\n    c\n");
        let Block::Table { alignments, rows } = &blocks[2] else {
            panic!("missing table")
        };
        assert_eq!(alignments, &[Alignment::Left, Alignment::Right]);
        assert_eq!(
            rows,
            &[
                vec!["Name", "Value"],
                vec!["<b>bold</b>", "<tt>a&lt;b</tt>"]
            ]
        );
        let Block::Markup(after) = &blocks[3] else {
            panic!("missing final prose")
        };
        assert!(after.contains("After &lt;b&gt;raw&lt;/b&gt;"));
    }
}
