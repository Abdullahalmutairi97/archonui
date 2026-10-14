//! The left pane of the built-in editor: the changes list, then the chat's folder as a tree.

use super::{button, ChatView, ClickEvent, EditorSet};
use crate::workspace::{self, ChangeKind, Entry};
use gpui::{div, prelude::*, px, rgb, Context, Div, SharedString};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

/// Width of the file tree pane.
pub(super) const TREE_WIDTH: f32 = 200.;

/// The folder listings behind the file tree, cached per directory.
#[derive(Default)]
pub struct Tree {
    root: PathBuf,
    /// The listings that have been read. A directory is read when first expanded.
    listings: HashMap<PathBuf, Vec<Entry>>,
    /// The directories shown expanded.
    expanded: HashSet<PathBuf>,
}

/// One visible row of the tree.
pub struct Row<'a> {
    pub entry: &'a Entry,
    pub depth: usize,
    /// Whether this row is a directory shown expanded.
    pub expanded: bool,
}

impl Tree {
    /// A tree of `root` with its listing read and the root expanded.
    pub fn new(root: &Path) -> Self {
        let mut tree = Self { root: root.to_path_buf(), ..Self::default() };
        tree.expanded.insert(root.to_path_buf());
        tree.list(root);
        tree
    }

    /// Reads the listing of `dir` into the cache. A directory that can't be read has none.
    fn list(&mut self, dir: &Path) {
        match workspace::list_dir(&self.root, dir) {
            Ok(entries) => self.listings.insert(dir.to_path_buf(), entries),
            Err(_) => self.listings.remove(dir),
        };
    }

    /// Expands `dir` or collapses it. Expanding reads its listing unless it is cached.
    pub fn toggle(&mut self, dir: &Path) {
        if self.expanded.remove(dir) {
            return;
        }
        self.expanded.insert(dir.to_path_buf());
        if !self.listings.contains_key(dir) {
            self.list(dir);
        }
    }

    /// Updates the cached listings after the watcher reports `path` created or removed. Removing a
    /// directory drops the listings under it.
    pub fn changed(&mut self, path: &Path, kind: ChangeKind) {
        if kind == ChangeKind::Modified {
            return;
        }
        if kind == ChangeKind::Removed {
            self.listings.retain(|dir, _| !dir.starts_with(path));
            self.expanded.retain(|dir| !dir.starts_with(path));
        }
        if let Some(parent) = path.parent().filter(|parent| self.listings.contains_key(*parent)) {
            self.list(parent);
        }
    }

    /// The visible rows, top to bottom: each expanded directory's entries follow it.
    pub fn rows(&self) -> Vec<Row<'_>> {
        let mut rows = Vec::new();
        self.push_rows(&self.root, 0, &mut rows);
        rows
    }

    fn push_rows<'a>(&'a self, dir: &Path, depth: usize, rows: &mut Vec<Row<'a>>) {
        let Some(entries) = self.listings.get(dir) else { return };
        for entry in entries {
            let expanded = entry.is_dir && self.expanded.contains(&entry.path);
            rows.push(Row { entry, depth, expanded });
            if expanded {
                self.push_rows(&entry.path, depth + 1, rows);
            }
        }
    }
}

/// The folders under `root` that hold one of `changed`, or hold a folder that does. They get a dot
/// too, so a change deep in the tree is visible from the top.
pub fn changed_dirs<'a>(root: &Path, changed: impl IntoIterator<Item = &'a Path>) -> HashSet<PathBuf> {
    let mut dirs = HashSet::new();
    for path in changed {
        let mut dir = path.parent();
        while let Some(current) = dir.filter(|d| *d != root && d.starts_with(root)) {
            // Every folder added also added its ancestors up to `root`, so stop at the first known one.
            if !dirs.insert(current.to_path_buf()) {
                break;
            }
            dir = current.parent();
        }
    }
    dirs
}

impl ChatView {
    /// The left pane of the editor: the changes list above the folder's tree.
    pub(super) fn render_tree(&self, set: &EditorSet, cx: &mut Context<Self>) -> Div {
        let active = set.files.get(set.active).and_then(|f| f.buffer.path());
        let changed_files: HashSet<&Path> = set.changes.iter().map(|(p, _, _)| p.as_path()).collect();
        let dirs = changed_dirs(&set.root, changed_files.iter().copied());

        let header = div()
            .px_2()
            .py_1()
            .flex()
            .items_center()
            .justify_between()
            .text_xs()
            .text_color(rgb(0x7f849c))
            .child(format!("Changes ({})", set.changes.len()))
            .child(button("editor-changes-clear", "Clear").on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                this.clear_changes(cx)
            })));

        let changes: Vec<_> = set.changes.iter().enumerate().map(|(i, (path, kind, _))| {
            let color = match kind {
                ChangeKind::Created => 0xa6e3a1,
                ChangeKind::Modified => 0xf9e2af,
                ChangeKind::Removed => 0xf38ba8,
            };
            let open = path.clone();
            div()
                .id(SharedString::from(format!("change-{i}")))
                .px_2()
                .py(px(2.))
                .flex()
                .items_center()
                .gap_1()
                .text_xs()
                .cursor_pointer()
                .hover(|d| d.bg(rgb(0x2a2b3c)))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.open_change(&open, cx)))
                .child(div().w(px(6.)).h(px(6.)).rounded_md().bg(rgb(color)))
                .child(div().flex_1().min_w_0().truncate().child(workspace::relative(&set.root, path)))
        }).collect();

        let rows: Vec<_> = set.tree.rows().into_iter().enumerate().map(|(i, row)| {
            let path = row.entry.path.clone();
            let is_dir = row.entry.is_dir;
            let dot = if is_dir {
                dirs.contains(&path)
            } else {
                changed_files.contains(path.as_path())
            };
            let is_active = active == Some(path.as_path());
            let marker = match (is_dir, row.expanded) {
                (true, true) => "▾ ",
                (true, false) => "▸ ",
                _ => "",
            };
            div()
                .id(SharedString::from(format!("tree-{i}")))
                .pl(px(6. + 12. * row.depth as f32))
                .pr_2()
                .py(px(1.))
                .flex()
                .items_center()
                .gap_1()
                .text_xs()
                .cursor_pointer()
                .when(is_active, |d| d.bg(rgb(0x313244)))
                .hover(|d| d.bg(rgb(0x2a2b3c)))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.tree_click(&path, is_dir, cx)))
                .child(div().flex_1().min_w_0().truncate().child(format!("{marker}{}", row.entry.name)))
                .children(dot.then(|| div().w(px(6.)).h(px(6.)).rounded_md().bg(rgb(0xf9e2af)).flex_shrink_0()))
        }).collect();

        div()
            .w(px(TREE_WIDTH))
            .flex_shrink_0()
            .h_full()
            .min_h_0()
            .flex()
            .flex_col()
            .border_r_1()
            .border_color(rgb(0x313244))
            .child(header)
            .child(div().flex().flex_col().children(changes).pb_1().border_b_1().border_color(rgb(0x313244)))
            .child(div().id("editor-tree").flex_1().min_h_0().overflow_y_scroll().flex().flex_col().children(rows))
    }

    /// Opens a file from the tree, or expands and collapses a folder in it.
    fn tree_click(&mut self, path: &Path, is_dir: bool, cx: &mut Context<Self>) {
        if is_dir {
            if let Some(set) = self.editor_set_mut() {
                set.tree.toggle(path);
            }
            cx.notify();
        } else {
            self.open_in_editor(path, cx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parent_dirs_of_changes_get_dots() {
        let root = Path::new("/work");
        let changed = [Path::new("/work/src/chat/tree.rs"), Path::new("/work/README.md")];
        let dirs = changed_dirs(root, changed);
        let expected: HashSet<PathBuf> = ["/work/src", "/work/src/chat"].iter().map(PathBuf::from).collect();
        assert_eq!(dirs, expected);
        assert!(!dirs.contains(Path::new("/work")), "the root itself gets no dot");
        assert!(changed_dirs(root, std::iter::empty()).is_empty());
    }

    /// The visible rows as `(name, depth)`.
    fn rows_of(tree: &Tree) -> Vec<(String, usize)> {
        tree.rows().iter().map(|row| (row.entry.name.clone(), row.depth)).collect()
    }

    fn named(rows: &[(&str, usize)]) -> Vec<(String, usize)> {
        rows.iter().map(|(name, depth)| (name.to_string(), *depth)).collect()
    }

    #[test]
    fn listings_follow_expansion_and_creation() {
        let dir = std::env::temp_dir().join(format!("archonui-tree-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/lib.rs"), "").unwrap();
        std::fs::write(dir.join("Cargo.toml"), "").unwrap();

        let mut tree = Tree::new(&dir);
        assert_eq!(rows_of(&tree), named(&[("src", 0), ("Cargo.toml", 0)]));

        tree.toggle(&dir.join("src"));
        assert_eq!(rows_of(&tree), named(&[("src", 0), ("lib.rs", 1), ("Cargo.toml", 0)]));

        std::fs::write(dir.join("src/main.rs"), "").unwrap();
        tree.changed(&dir.join("src/main.rs"), ChangeKind::Created);
        assert_eq!(rows_of(&tree).len(), 4);

        std::fs::remove_dir_all(dir.join("src")).unwrap();
        tree.changed(&dir.join("src"), ChangeKind::Removed);
        assert_eq!(rows_of(&tree), named(&[("Cargo.toml", 0)]));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
