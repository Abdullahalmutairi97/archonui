//! The Files tab: the tool folder as a tree, with create, rename and delete, a right-click menu, a filter,
//! git change marks and a footer for the selected file. Spec: docs/design/tools-and-behaviour.md §A3.

use super::ide::styled_line;
use crate::{
    ui::{
        composer::icons::{stroke_icon, PAPERCLIP},
        shell::Shell,
        stream::code::{tokenize, Tok},
        theme::{Rgba, Theme},
        widgets::{
            button::{Button, ButtonStyle},
            text_input::{text_input, TextInput, TextInputConfig, TextInputEvent},
        },
    },
    workspace::{self, Change, ChangeKind, Entry, Watcher},
};
use gpui::{
    anchored, deferred, div, prelude::*, AnyElement, App, ClickEvent, ClipboardItem, Context, Div, Entity,
    FontWeight, KeyDownEvent, MouseButton, MouseDownEvent, Pixels, Point, SharedString, Stateful, Subscription,
    Task, Window,
};
use crate::ui::scale::px;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    fs,
    ops::Range,
    path::{Path, PathBuf},
    process::Command,
    time::SystemTime,
};

/// Most matches a filter lists.
const MAX_MATCHES: usize = 500;
/// Most folders a filter reads, so a miss in a huge tree doesn't freeze the UI.
const MAX_SEARCH_DIRS: usize = 5000;
/// Lines of the selected file shown in the footer.
const PREVIEW_LINES: usize = 6;
/// Largest file the footer describes. Bigger files get no footer.
const MAX_INFO_BYTES: u64 = 2 * 1024 * 1024;

const FOLDER: &str = "M2 4h4l1.5 1.5H14v7.5H2z";
const FILE: &str = "M4 2h5l3.5 3.5V14H4z";
const COLLAPSE: &str = "M3 6.5l5-4 5 4M3 11.5l5 4 5-4";
pub(super) const SEARCH: &str = "M2.5 2.5h7v7h-7zM8.5 8.5l5 5";
const OPEN: &str = "M4 12l8-8M6 4h6v6";
const COPY: &str = "M5 5h8v8H5zM3 11V3h8";

/// Watches a folder until dropped. Each change is passed to `on_change` on the UI thread.
pub(super) struct FolderWatch {
    pub(super) root: PathBuf,
    /// `None` when the folder can't be watched, so no change arrives.
    watcher: Option<Watcher>,
    _task: Task<()>,
}

impl FolderWatch {
    pub(super) fn start(root: &Path, cx: &mut Context<Shell>, on_change: fn(&mut Shell, Change, &mut Context<Shell>)) -> Self {
        let (watcher, rx) = Watcher::new_background(root);
        let task = cx.spawn(async move |this, cx| {
            while let Ok(change) = rx.recv().await {
                if this.update(cx, |this, cx| on_change(this, change, cx)).is_err() {
                    break;
                }
            }
        });
        Self { root: root.to_path_buf(), watcher: Some(watcher), _task: task }
    }

    /// Tells the watcher that the app wrote `path` itself, so the write isn't reported as a change.
    pub(super) fn ignore_own_write(&self, path: &Path) {
        if let Some(watcher) = &self.watcher {
            watcher.ignore_own_write(path);
        }
    }
}

/// View state of the Files tab, not persisted.
#[derive(Default)]
pub struct FilesUi {
    /// The folder shown, and its tree. The tree is rebuilt when the folder changes.
    root: Option<PathBuf>,
    tree: Option<Tree>,
    selected: Option<PathBuf>,
    /// The selected file's details for the footer. `None` when a folder or nothing is selected.
    info: Option<Info>,
    filter: Option<Filter>,
    editing: Option<Editing>,
    menu: Option<Menu>,
    pub(super) watch: Option<FolderWatch>,
    /// Gives the keyboard back to the shell after a name field went away.
    refocus: bool,
}

/// The filter field and the files it matches.
struct Filter {
    input: Entity<TextInput>,
    _subs: Vec<Subscription>,
    query: String,
    matches: Vec<PathBuf>,
}

/// An inline name field: a new file or folder in a folder, or a rename.
struct Editing {
    edit: Edit,
    input: Entity<TextInput>,
    _subs: Vec<Subscription>,
}

enum Edit {
    Create { dir: PathBuf, folder: bool },
    Rename(PathBuf),
}

/// The right-click menu on a row. `confirm` shows the delete confirmation instead of the choices. `error` is
/// why moving to the trash failed, which offers deleting for good instead.
struct Menu {
    path: PathBuf,
    at: Point<Pixels>,
    confirm: bool,
    error: Option<String>,
}

/// What the footer shows about the selected file.
struct Info {
    path: PathBuf,
    size: u64,
    lines: usize,
    modified: Option<SystemTime>,
    /// The first lines, each with its highlight spans.
    preview: Vec<(String, Vec<(Range<usize>, Tok)>)>,
}

/// The folder listings behind the tree, cached per directory. A directory is read when first expanded.
#[derive(Default)]
struct Tree {
    root: PathBuf,
    listings: HashMap<PathBuf, Vec<Entry>>,
    /// Entry counts of the shown directories, including the collapsed ones, which have no listing.
    counts: HashMap<PathBuf, usize>,
    expanded: HashSet<PathBuf>,
}

/// One visible row of the tree.
struct Row<'a> {
    entry: &'a Entry,
    depth: usize,
    expanded: bool,
}

impl Tree {
    fn new(root: &Path) -> Self {
        let mut tree = Self { root: root.to_path_buf(), ..Self::default() };
        tree.expanded.insert(root.to_path_buf());
        tree.list(root);
        tree
    }

    /// Reads the listing of `dir` into the cache, and the counts of its subdirectories. A directory that
    /// can't be read has none.
    fn list(&mut self, dir: &Path) {
        match workspace::list_dir(&self.root, dir) {
            Ok(entries) => {
                for child in entries.iter().filter(|entry| entry.is_dir) {
                    let count = self.read_count(&child.path);
                    self.counts.insert(child.path.clone(), count);
                }
                self.counts.insert(dir.to_path_buf(), entries.len());
                self.listings.insert(dir.to_path_buf(), entries);
            }
            Err(_) => {
                self.listings.remove(dir);
                self.counts.remove(dir);
            }
        }
    }

    /// The entry count of `dir`, read without caching the listing. A directory that can't be read has none.
    fn read_count(&self, dir: &Path) -> usize {
        workspace::list_dir(&self.root, dir).map_or(0, |entries| entries.len())
    }

    fn expand(&mut self, dir: &Path) {
        self.expanded.insert(dir.to_path_buf());
        if !self.listings.contains_key(dir) {
            self.list(dir);
        }
    }

    /// Expands `dir` or collapses it.
    fn toggle(&mut self, dir: &Path) {
        if self.expanded.contains(dir) {
            self.expanded.remove(dir);
        } else {
            self.expand(dir);
        }
    }

    fn is_expanded(&self, dir: &Path) -> bool {
        self.expanded.contains(dir)
    }

    fn collapse_all(&mut self) {
        let root = self.root.clone();
        self.expanded.retain(|dir| *dir == root);
    }

    /// How many entries `dir` lists.
    fn count(&self, dir: &Path) -> usize {
        self.counts.get(dir).copied().unwrap_or(0)
    }

    /// Updates the cached listings after `path` was created or removed. Removing a directory drops the
    /// listings under it.
    fn changed(&mut self, path: &Path, kind: ChangeKind) {
        if kind == ChangeKind::Modified {
            return;
        }
        if kind == ChangeKind::Removed {
            self.listings.retain(|dir, _| !dir.starts_with(path));
            self.counts.retain(|dir, _| !dir.starts_with(path));
            self.expanded.retain(|dir| !dir.starts_with(path));
        }
        let Some(parent) = path.parent() else { return };
        if self.listings.contains_key(parent) {
            self.list(parent);
        } else if self.counts.contains_key(parent) {
            let count = self.read_count(parent);
            self.counts.insert(parent.to_path_buf(), count);
        }
    }

    /// The visible rows, top to bottom: each expanded directory's entries follow it.
    fn rows(&self) -> Vec<Row<'_>> {
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

impl Shell {
    /// The project folder the Files, IDE and Git tabs show, canonicalized so that paths match what git
    /// reports. `None` when no project folder is in focus.
    pub(super) fn tool_root(&self) -> Option<PathBuf> {
        workspace::project_root(&self.tool_folder()?).ok()
    }

    /// The Files tab's root, or an empty path when there is none (every operation on it is then refused).
    fn files_root(&self) -> PathBuf {
        self.tabs.files.root.clone().unwrap_or_default()
    }

    pub(in crate::ui) fn render_files(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = self.theme.clone();
        let Some(root) = self.tool_root() else {
            return no_project(&theme);
        };
        self.files_sync(&root, window, cx);
        if std::mem::take(&mut self.tabs.files.refocus) {
            window.focus(&self.focus);
        }
        let header = self.files_header(&theme, cx);
        let filter = self.files_filter_row(&theme);
        let changes = self.files_changes(&theme, cx);
        let body = self.files_body(&theme, cx);
        let footer = self.files_footer(&root, &theme, cx);
        let menu = self.files_menu(&theme, cx);
        div()
            .size_full()
            .min_h_0()
            .flex()
            .flex_col()
            .font_family(theme.ui_font.clone())
            .text_color(theme.fg.to_hsla())
            .child(header)
            .child(filter)
            .children(changes)
            .child(body)
            .children(footer)
            .children(menu)
            .into_any_element()
    }

    /// Escape closes the context menu.
    pub(in crate::ui) fn files_key(&mut self, ev: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) -> bool {
        if ev.keystroke.key != "escape" || self.tabs.files.menu.is_none() {
            return false;
        }
        self.tabs.files.menu = None;
        cx.notify();
        true
    }

    pub(in crate::ui) fn files_blur(&mut self, cx: &mut Context<Self>) {
        if self.tabs.files.menu.take().is_some() {
            cx.notify();
        }
    }

    /// Keeps the tree, the watcher, the filter field and the git state on the tool folder.
    fn files_sync(&mut self, root: &Path, window: &mut Window, cx: &mut Context<Self>) {
        if self.tabs.files.root.as_deref() != Some(root) {
            let files = &mut self.tabs.files;
            files.root = Some(root.to_path_buf());
            files.tree = Some(Tree::new(root));
            files.selected = None;
            files.info = None;
            files.editing = None;
            files.menu = None;
            // The filter's matches belong to the old folder.
            if files.filter.as_ref().is_some_and(|filter| !filter.query.is_empty()) {
                self.files_filter_changed(cx);
            }
        }
        if !self.tabs.files.watch.as_ref().is_some_and(|watch| watch.root == root) {
            self.tabs.files.watch = Some(FolderWatch::start(root, cx, Self::files_change));
        }
        if self.tabs.files.filter.is_none() {
            let input = cx.new(|cx| TextInput::new(filter_config(), window, cx));
            let observe = cx.observe(&input, |_, _, cx| cx.notify());
            let changed = cx.subscribe(&input, |this, _, event: &TextInputEvent, cx| {
                if let TextInputEvent::Changed = event {
                    this.files_filter_changed(cx);
                }
            });
            self.tabs.files.filter = Some(Filter { input, _subs: vec![observe, changed], query: String::new(), matches: Vec::new() });
        }
        if self.tabs.git.folder.as_deref() != Some(root) {
            self.refresh_git(cx);
        }
    }

    fn files_header(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let root = self.tabs.files.root.clone().unwrap_or_default();
        let name = root.file_name().map_or_else(|| root.display().to_string(), |n| n.to_string_lossy().into_owned());
        let view = cx.entity();
        let muted = theme.muted.to_hsla();
        let button = |id: &'static str, path: &'static str, action: fn(&mut Self, &mut Window, &mut Context<Self>)| {
            let view = view.clone();
            Button::icon(id, stroke_icon(path, 14., 1.3, muted), theme)
                .on_click(move |window, cx| view.update(cx, |this, cx| action(this, window, cx)))
                .into_any_element()
        };
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .pl(px(16.))
            .pr(px(10.))
            .pt(px(12.))
            .pb(px(8.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .font_family(theme.display_font.clone())
                            .text_size(px(20.))
                            .line_height(px(22.))
                            .truncate()
                            .child(name),
                    )
                    .child(div().text_size(px(11.5)).text_color(muted).truncate().child(crate::ui::settings::tilde(&root))),
            )
            .child(button("files-new-file", FILE, |this, window, cx| this.files_new(false, window, cx)))
            .child(button("files-new-folder", FOLDER, |this, window, cx| this.files_new(true, window, cx)))
            .child(button("files-collapse", COLLAPSE, |this, _, cx| this.files_collapse(cx)))
            .into_any_element()
    }

    fn files_filter_row(&self, theme: &Theme) -> AnyElement {
        let Some(filter) = &self.tabs.files.filter else { return div().into_any_element() };
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .mx(px(12.))
            .mb(px(8.))
            .px(px(10.))
            .py(px(7.))
            .rounded(px(7.))
            .border_1()
            .border_color(theme.line.to_hsla())
            .bg(theme.line.to_hsla())
            .child(stroke_icon(SEARCH, 13., 1.3, theme.muted.to_hsla()))
            .child(div().flex_1().min_w_0().text_size(px(12.5)).child(text_input(&filter.input, theme)))
            .into_any_element()
    }

    fn files_body(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let filtering = self.tabs.files.filter.as_ref().is_some_and(|f| !f.query.is_empty());
        let rows: Vec<AnyElement> = if filtering { self.files_match_rows(theme, cx) } else { self.files_tree_rows(theme, cx) };
        div()
            .id("files-tree")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px(px(8.))
            .pb(px(8.))
            .flex()
            .flex_col()
            .children(rows)
            .into_any_element()
    }

    /// The tree's rows. A new file or folder field goes at the top of the root, or under its folder.
    fn files_tree_rows(&self, theme: &Theme, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let files = &self.tabs.files;
        let Some(tree) = &files.tree else { return Vec::new() };
        let root = files.root.clone().unwrap_or_default();
        let creating = match &files.editing {
            Some(Editing { edit: Edit::Create { dir, folder }, input, .. }) => Some((dir.clone(), *folder, input.clone())),
            _ => None,
        };
        let mut rows = Vec::new();
        if let Some((dir, folder, input)) = &creating {
            if *dir == root {
                rows.push(files_create_row(0, *folder, input, theme));
            }
        }
        for row in tree.rows() {
            rows.push(self.files_row(&row, tree, theme, cx));
            if let Some((dir, folder, input)) = &creating {
                if row.entry.is_dir && row.expanded && row.entry.path == *dir {
                    rows.push(files_create_row(row.depth + 1, *folder, input, theme));
                }
            }
        }
        rows
    }

    fn files_row(&self, row: &Row, tree: &Tree, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let entry = row.entry;
        let path = entry.path.clone();
        let is_dir = entry.is_dir;
        let selected = self.tabs.files.selected.as_deref() == Some(path.as_path());
        let renaming = match &self.tabs.files.editing {
            Some(Editing { edit: Edit::Rename(target), input, .. }) if *target == path => Some(input.clone()),
            _ => None,
        };
        let mark = self.git_mark(&path);
        let (tag, tag_colour) = file_tag(&entry.name, theme);
        let folder_colour = if row.expanded { theme.acc2 } else { theme.alt };
        let muted = theme.muted.to_hsla();

        let mut line = div()
            .id(SharedString::from(format!("files-row-{}", path.display())))
            .h(px(30.))
            .flex()
            .items_center()
            .gap(px(8.))
            .pl(px(10. + 16. * row.depth as f32))
            .pr(px(10.))
            .rounded(px(6.))
            .text_size(px(13.))
            .cursor_pointer()
            .when(selected, |d| d.bg(theme.soft.to_hsla()))
            .hover(|d| d.bg(theme.line2.to_hsla()))
            .on_click(cx.listener({
                let path = path.clone();
                move |this, ev: &ClickEvent, _, cx| this.files_click(path.clone(), is_dir, ev.click_count(), cx)
            }))
            .on_mouse_down(MouseButton::Right, cx.listener({
                let path = path.clone();
                move |this, ev: &MouseDownEvent, _, cx| this.files_open_menu(path.clone(), ev.position, cx)
            }));
        for depth in 0..row.depth {
            line = line.child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(px(15. + 16. * depth as f32))
                    .w(px(1.))
                    .bg(theme.line2.to_hsla()),
            );
        }
        line = line.child(
            div()
                .w(px(10.))
                .flex_none()
                .text_size(px(9.))
                .text_color(muted)
                .child(match (is_dir, row.expanded) {
                    (true, true) => "▼",
                    (true, false) => "▶",
                    _ => "",
                }),
        );
        line = line.child(if is_dir {
            stroke_icon(FOLDER, 15., 1.2, folder_colour.to_hsla()).into_any_element()
        } else {
            div()
                .w(px(30.))
                .flex_none()
                .py(px(2.))
                .rounded(px(3.))
                .text_center()
                .text_size(px(9.5))
                .font_weight(FontWeight::BOLD)
                .text_color(tag_colour.to_hsla())
                .bg(theme.line.to_hsla())
                .child(tag)
                .into_any_element()
        });
        let name = match renaming {
            Some(input) => div().flex_1().min_w_0().child(text_input(&input, theme)).into_any_element(),
            None => div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_color(if selected { theme.acc2.to_hsla() } else { theme.fg.to_hsla() })
                .child(entry.name.clone())
                .into_any_element(),
        };
        line = line.child(name);
        let (indicator, indicator_colour) = match (is_dir, &mark) {
            (true, _) if self.git_dir_changed(&path) => ("•".to_string(), theme.acc2),
            (false, Some(mark)) => (mark.letter.to_string(), mark.colour(theme)),
            _ => (String::new(), theme.muted),
        };
        line = line.child(
            div()
                .w(px(12.))
                .flex_none()
                .text_center()
                .text_size(px(11.))
                .font_weight(FontWeight::BOLD)
                .text_color(indicator_colour.to_hsla())
                .child(indicator),
        );
        let meta = if is_dir {
            item_count(tree.count(&path))
        } else {
            fs::metadata(&path).map(|m| human_size(m.len())).unwrap_or_default()
        };
        // At least the old 48px so short sizes still line up, but wide enough that `14 items` or `183.6 KB`
        // stay on one line.
        line.child(div().min_w(px(48.)).flex_none().whitespace_nowrap().text_right().text_size(px(11.)).text_color(muted).child(meta))
            .into_any_element()
    }

    /// The filter's matches as a flat list, each with its folder in grey.
    fn files_match_rows(&self, theme: &Theme, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let Some(filter) = &self.tabs.files.filter else { return Vec::new() };
        if filter.matches.is_empty() {
            return vec![div()
                .px(px(10.))
                .py(px(14.))
                .text_size(px(12.5))
                .text_color(theme.muted.to_hsla())
                .child("No files match.")
                .into_any_element()];
        }
        let root = self.tabs.files.root.clone().unwrap_or_default();
        filter
            .matches
            .iter()
            .map(|path| {
                let selected = self.tabs.files.selected.as_deref() == Some(path.as_path());
                let id = SharedString::from(format!("files-match-{}", path.display()));
                let path = path.clone();
                file_match_row(id, &root, &path, selected, theme)
                    .on_click(cx.listener(move |this, ev: &ClickEvent, _, cx| {
                        this.files_click(path.clone(), false, ev.click_count(), cx)
                    }))
                    .into_any_element()
            })
            .collect()
    }

    /// The files the agent changed in the tool folder this session: a collapsible label with Clear, and a
    /// row per file that opens it in the Ide tab.
    fn files_changes(&self, theme: &Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
        let ide = &self.tabs.ide;
        if ide.changes.is_empty() {
            return None;
        }
        let muted = theme.muted.to_hsla();
        let root = self.tabs.files.root.clone().unwrap_or_default();
        let chevron = if ide.changes_collapsed { "▸" } else { "▾" };
        let header = div()
            .flex()
            .items_center()
            .justify_between()
            .px(px(16.))
            .pt(px(10.))
            .pb(px(6.))
            .text_size(px(10.5))
            .text_color(muted)
            .child(
                div()
                    .id("files-changes-toggle")
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .cursor_pointer()
                    .hover(|d| d.text_color(theme.fg.to_hsla()))
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.tabs.ide.changes_collapsed = !this.tabs.ide.changes_collapsed;
                        cx.notify();
                    }))
                    .child(format!("{chevron} AGENT CHANGES · {}", ide.changes.len())),
            )
            .child(
                div()
                    .id("files-changes-clear")
                    .px(px(4.))
                    .rounded(px(4.))
                    .cursor_pointer()
                    .hover(|d| d.text_color(theme.fg.to_hsla()))
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.tabs.ide.changes.clear();
                        cx.notify();
                    }))
                    .child("Clear"),
            );
        let rows: Vec<AnyElement> = if ide.changes_collapsed {
            Vec::new()
        } else {
            ide.changes
                .iter()
                .enumerate()
                .map(|(ix, (path, kind))| {
                    let letter = change_letter(*kind);
                    let colour = change_colour(letter, theme);
                    let rel = workspace::relative(&root, path);
                    let path = path.clone();
                    div()
                        .id(SharedString::from(format!("files-change-{ix}")))
                        .h(px(30.))
                        .px(px(10.))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .rounded(px(6.))
                        .text_size(px(13.))
                        .cursor_pointer()
                        .hover(|d| d.bg(theme.line2.to_hsla()))
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.ide_open(&path, cx)))
                        .child(
                            div()
                                .w(px(12.))
                                .flex_none()
                                .text_center()
                                .text_size(px(11.))
                                .font_weight(FontWeight::BOLD)
                                .text_color(colour.to_hsla())
                                .child(letter.to_string()),
                        )
                        .child(div().flex_1().min_w_0().truncate().child(rel))
                        .into_any_element()
                })
                .collect()
        };
        Some(div().flex().flex_col().child(header).child(div().flex().flex_col().px(px(8.)).children(rows)).into_any_element())
    }

    /// The selected file's footer: its name and git status, a details line, a preview and actions.
    fn files_footer(&self, root: &Path, theme: &Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
        let info = self.tabs.files.info.as_ref()?;
        let muted = theme.muted.to_hsla();
        let name = info.path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        let (status, status_colour) = match self.git_mark(&info.path) {
            Some(mark) => {
                let word = match mark.letter {
                    'A' => "added",
                    'D' => "deleted",
                    _ => "modified",
                };
                let counts = if mark.removed > 0 {
                    format!("+{} −{}", mark.added, mark.removed)
                } else {
                    format!("+{}", mark.added)
                };
                (format!("{word} · {counts}"), mark.colour(theme))
            }
            None => ("unchanged".to_string(), theme.muted),
        };
        let details = format!(
            "{} · {} · {} lines · {}",
            workspace::relative(root, &info.path),
            human_size(info.size),
            info.lines,
            info.modified.map_or_else(String::new, ago)
        );
        let preview: Vec<AnyElement> = info
            .preview
            .iter()
            .enumerate()
            .map(|(i, (text, spans))| {
                div()
                    .flex()
                    .gap(px(10.))
                    .child(
                        div()
                            .w(px(22.))
                            .flex_none()
                            .text_right()
                            .text_color(theme.faint.to_hsla())
                            .child((i + 1).to_string()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .whitespace_nowrap()
                            .overflow_hidden()
                            .child(styled_line(text, spans, theme)),
                    )
                    .into_any_element()
            })
            .collect();
        let open = footer_action("files-open", stroke_icon(OPEN, 13., 1.3, muted), "Open", theme).on_click(cx.listener({
            let path = info.path.clone();
            move |this, _: &ClickEvent, _, cx| this.ide_open(&path, cx)
        }));
        let attach = footer_action("files-attach", stroke_icon(PAPERCLIP, 13., 1.3, muted), "Attach", theme).on_click(cx.listener({
            let path = info.path.clone();
            move |this, _: &ClickEvent, _, cx| this.files_attach(path.clone(), cx)
        }));
        let copy = footer_action("files-copy", stroke_icon(COPY, 13., 1.3, muted), "Copy path", theme).on_click(cx.listener({
            let path = info.path.clone();
            move |this, _: &ClickEvent, _, cx| this.files_copy(path.clone(), cx)
        }));
        Some(
            div()
                .flex()
                .flex_col()
                .gap(px(10.))
                .px(px(14.))
                .pt(px(12.))
                .pb(px(14.))
                .border_t_1()
                .border_color(theme.line2.to_hsla())
                .bg(theme.line.to_hsla())
                .child(
                    div()
                        .flex()
                        .items_baseline()
                        .gap(px(8.))
                        .min_w_0()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_size(px(14.))
                                .font_weight(FontWeight::BOLD)
                                .child(name),
                        )
                        .child(
                            div()
                                .text_size(px(11.))
                                .text_color(status_colour.to_hsla())
                                .whitespace_nowrap()
                                .child(status),
                        ),
                )
                .child(div().text_size(px(11.5)).text_color(muted).truncate().child(details))
                .child(
                    div()
                        .h(px(118.))
                        .rounded(px(6.))
                        .border_1()
                        .border_color(theme.line.to_hsla())
                        .bg(theme.bg.to_hsla())
                        .py(px(8.))
                        .overflow_hidden()
                        .font_family(theme.code_font.clone())
                        .text_size(px(11.5))
                        .line_height(px(18.4))
                        .flex()
                        .flex_col()
                        .children(preview),
                )
                .child(div().flex().gap(px(6.)).child(open).child(attach).child(copy))
                .into_any_element(),
        )
    }

    /// The right-click menu as an overlay at the pointer.
    fn files_menu(&self, theme: &Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
        let menu = self.tabs.files.menu.as_ref()?;
        let (path, at, confirm) = (menu.path.clone(), menu.at, menu.confirm);
        let error = menu.error.clone();
        let name = path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        let items: Vec<AnyElement> = if confirm {
            let delete_path = path.clone();
            let (title, delete_id, delete_label, permanently) = match &error {
                Some(_) => (format!("Couldn’t move “{name}” to the trash."), "files-menu-delete-permanent", "Delete permanently", true),
                None => (format!("Delete “{name}”?"), "files-menu-delete-confirm", "Delete", false),
            };
            let mut items = vec![
                div()
                    .px(px(10.))
                    .py(px(6.))
                    .text_size(px(12.))
                    .text_color(theme.muted.to_hsla())
                    .child(title)
                    .into_any_element(),
            ];
            if let Some(error) = error {
                items.push(
                    div()
                        .px(px(10.))
                        .pb(px(6.))
                        .max_w(px(260.))
                        .text_size(px(11.5))
                        .text_color(theme.on.to_hsla())
                        .child(error)
                        .into_any_element(),
                );
            }
            items.push(
                Button::new(delete_id, delete_label, theme)
                    .style(ButtonStyle::Danger)
                    .on_click({
                        let view = cx.entity();
                        move |_, cx| {
                            let path = delete_path.clone();
                            view.update(cx, |this, cx| this.files_delete(path, permanently, cx))
                        }
                    })
                    .into_any_element(),
            );
            items.push(
                Button::new("files-menu-delete-cancel", "Cancel", theme)
                    .on_click({
                        let view = cx.entity();
                        move |_, cx| {
                            view.update(cx, |this, cx| {
                                this.tabs.files.menu = None;
                                cx.notify();
                            })
                        }
                    })
                    .into_any_element(),
            );
            items
        } else {
            let dir = if path.is_dir() { path.clone() } else { path.parent().map(Path::to_path_buf).unwrap_or_default() };
            let mut items = vec![
                menu_item("files-menu-new-file", "New file", theme, cx.listener({
                    let dir = dir.clone();
                    move |this, _: &ClickEvent, window, cx| this.files_new_in(dir.clone(), false, window, cx)
                })),
                menu_item("files-menu-new-folder", "New folder", theme, cx.listener({
                    let dir = dir.clone();
                    move |this, _: &ClickEvent, window, cx| this.files_new_in(dir.clone(), true, window, cx)
                })),
                menu_item("files-menu-rename", "Rename", theme, cx.listener({
                    let path = path.clone();
                    move |this, _: &ClickEvent, window, cx| this.files_rename(path.clone(), window, cx)
                })),
                menu_item("files-menu-copy", "Copy path", theme, cx.listener({
                    let path = path.clone();
                    move |this, _: &ClickEvent, _, cx| this.files_copy(path.clone(), cx)
                })),
                menu_item("files-menu-reveal", "Reveal in folder", theme, cx.listener({
                    let path = path.clone();
                    move |this, _: &ClickEvent, _, cx| this.files_reveal(path.clone(), cx)
                })),
            ];
            if !path.is_dir() {
                items.push(menu_item("files-menu-attach", "Attach to message", theme, cx.listener({
                    let path = path.clone();
                    move |this, _: &ClickEvent, _, cx| this.files_attach(path.clone(), cx)
                })));
            }
            items.push(menu_item("files-menu-delete", "Delete…", theme, cx.listener(|this, _: &ClickEvent, _, cx| {
                if let Some(menu) = this.tabs.files.menu.as_mut() {
                    menu.confirm = true;
                }
                cx.notify();
            })));
            items
        };
        let panel = div()
            .id("files-menu")
            .occlude()
            .w(px(220.))
            .p(px(4.))
            .flex()
            .flex_col()
            .gap(px(1.))
            .rounded(px(9.))
            .border_1()
            .border_color(theme.line2.to_hsla())
            .bg(theme.pop.to_hsla())
            .text_color(theme.fg.to_hsla())
            .font_family(theme.ui_font.clone())
            .on_mouse_down_out(cx.listener(|this, _: &MouseDownEvent, _, cx| {
                this.tabs.files.menu = None;
                cx.notify();
            }))
            .children(items);
        Some(deferred(anchored().position(at).snap_to_window_with_margin(px(8.)).child(panel)).with_priority(1).into_any_element())
    }

    fn files_click(&mut self, path: PathBuf, is_dir: bool, clicks: usize, cx: &mut Context<Self>) {
        self.tabs.files.selected = Some(path.clone());
        self.tabs.files.menu = None;
        if is_dir {
            self.tabs.files.info = None;
            if clicks == 1 {
                if let Some(tree) = &mut self.tabs.files.tree {
                    tree.toggle(&path);
                }
            }
        } else {
            self.tabs.files.info = read_info(&self.files_root(), &path);
            if clicks >= 2 {
                self.ide_open(&path, cx);
            }
        }
        cx.notify();
    }

    /// Applies one debounced change from the tool folder's watcher.
    fn files_change(&mut self, change: Change, cx: &mut Context<Self>) {
        if !super::ide::is_scratch(&change.path) {
            super::ide::record_change(&mut self.tabs.ide.changes, &change.path, change.kind);
        }
        if let Some(tree) = &mut self.tabs.files.tree {
            tree.changed(&change.path, change.kind);
        }
        if self.tabs.files.info.as_ref().is_some_and(|info| info.path == change.path) {
            self.tabs.files.info =
                if change.kind == ChangeKind::Removed { None } else { read_info(&self.files_root(), &change.path) };
        }
        if self.tabs.files.filter.as_ref().is_some_and(|filter| !filter.query.is_empty()) {
            self.files_filter_changed(cx);
        }
        self.refresh_git(cx);
        cx.notify();
    }

    fn files_filter_changed(&mut self, cx: &mut Context<Self>) {
        let root = self.tabs.files.root.clone().unwrap_or_default();
        let Some(filter) = self.tabs.files.filter.as_mut() else { return };
        let query = filter.input.read(cx).text().trim().to_string();
        filter.matches = if query.is_empty() { Vec::new() } else { search(&root, &query) };
        filter.query = query;
        cx.notify();
    }

    fn files_collapse(&mut self, cx: &mut Context<Self>) {
        if let Some(tree) = &mut self.tabs.files.tree {
            tree.collapse_all();
        }
        cx.notify();
    }

    fn files_open_menu(&mut self, path: PathBuf, at: Point<Pixels>, cx: &mut Context<Self>) {
        self.tabs.files.selected = Some(path.clone());
        self.tabs.files.menu = Some(Menu { path, at, confirm: false, error: None });
        cx.notify();
    }

    /// Starts a new file or folder in the selected folder, or beside the selected file, or at the root.
    fn files_new(&mut self, folder: bool, window: &mut Window, cx: &mut Context<Self>) {
        let root = self.tabs.files.root.clone().unwrap_or_default();
        let dir = match self.tabs.files.selected.clone() {
            Some(path) if path.is_dir() => path,
            Some(path) => path.parent().map_or(root, Path::to_path_buf),
            None => root,
        };
        self.files_new_in(dir, folder, window, cx);
    }

    fn files_new_in(&mut self, dir: PathBuf, folder: bool, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(tree) = &mut self.tabs.files.tree {
            tree.expand(&dir);
        }
        self.files_begin(Edit::Create { dir, folder }, "", window, cx);
    }

    fn files_rename(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let name = path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        self.files_begin(Edit::Rename(path), &name, window, cx);
    }

    fn files_begin(&mut self, edit: Edit, initial: &str, window: &mut Window, cx: &mut Context<Self>) {
        let input = cx.new(|cx| TextInput::new(name_config(), window, cx));
        input.update(cx, |input, cx| input.set_text(initial, cx));
        input.read(cx).focus(window);
        let submit = cx.subscribe(&input, |this, _, event: &TextInputEvent, cx| match event {
            TextInputEvent::Changed => {}
            TextInputEvent::Submit => this.files_finish(true, cx),
            TextInputEvent::Blur => this.files_finish(false, cx),
        });
        let observe = cx.observe(&input, |_, _, cx| cx.notify());
        self.tabs.files.menu = None;
        self.tabs.files.editing = Some(Editing { edit, input, _subs: vec![submit, observe] });
        cx.notify();
    }

    /// Ends the inline name field. A submitted name creates or renames the file; leaving the field without
    /// submitting cancels.
    fn files_finish(&mut self, submit: bool, cx: &mut Context<Self>) {
        let Some(editing) = self.tabs.files.editing.take() else { return };
        self.tabs.files.refocus = true;
        if !submit {
            cx.notify();
            return;
        }
        let name = editing.input.read(cx).text().trim().to_string();
        if !valid_name(&name) {
            self.toast("A name can't be empty, “.”, “..”, or contain a slash.", cx);
            cx.notify();
            return;
        }
        match editing.edit {
            Edit::Create { dir, folder } => {
                let root = self.files_root();
                let path = dir.join(&name);
                let made = if folder { workspace::create_dir(&root, &path) } else { workspace::create_file(&root, &path) };
                match made {
                    Ok(path) => {
                        if let Some(tree) = &mut self.tabs.files.tree {
                            tree.changed(&path, ChangeKind::Created);
                        }
                        self.tabs.files.selected = Some(path.clone());
                        self.tabs.files.info = if folder { None } else { read_info(&root, &path) };
                    }
                    Err(e) => self.toast(format!("Could not create {name}: {e}"), cx),
                }
            }
            Edit::Rename(old) => {
                let new = old.parent().map_or_else(|| PathBuf::from(&name), |parent| parent.join(&name));
                if new == old {
                    cx.notify();
                    return;
                }
                if self.ide_dirty_under(&old) {
                    self.toast("Save or close the open file first.", cx);
                    cx.notify();
                    return;
                }
                if new.exists() {
                    self.toast(format!("{name} already exists here."), cx);
                    cx.notify();
                    return;
                }
                let root = self.files_root();
                match workspace::rename(&root, &old, &new) {
                    Ok((old, new)) => {
                        if let Some(tree) = &mut self.tabs.files.tree {
                            tree.changed(&old, ChangeKind::Removed);
                            tree.changed(&new, ChangeKind::Created);
                        }
                        if self.tabs.files.selected.as_deref() == Some(old.as_path()) {
                            self.tabs.files.selected = Some(new.clone());
                            self.tabs.files.info = read_info(&root, &new);
                        }
                        self.ide_moved(&old, &new);
                    }
                    Err(e) => self.toast(format!("Could not rename {name}: {e}"), cx),
                }
            }
        }
        self.refresh_git(cx);
        cx.notify();
    }

    /// Deletes `path` after the menu's confirmation: to the trash, or for good when `permanently`. A file with
    /// unsaved edits in the IDE blocks it. A failed trash keeps the confirmation open with the reason.
    fn files_delete(&mut self, path: PathBuf, permanently: bool, cx: &mut Context<Self>) {
        let menu = self.tabs.files.menu.take();
        if self.ide_dirty_under(&path) {
            self.toast("Save or close the open file first.", cx);
            cx.notify();
            return;
        }
        let removed = workspace::delete(&self.files_root(), &path, permanently);
        match removed {
            Ok(()) => {
                if let Some(tree) = &mut self.tabs.files.tree {
                    tree.changed(&path, ChangeKind::Removed);
                }
                if self.tabs.files.selected.as_ref().is_some_and(|selected| selected.starts_with(&path)) {
                    self.tabs.files.selected = None;
                    self.tabs.files.info = None;
                }
                self.ide_closed_under(&path);
                self.refresh_git(cx);
            }
            Err(e) if !permanently => {
                if let Some(menu) = menu {
                    self.tabs.files.menu = Some(Menu { path, at: menu.at, confirm: true, error: Some(e.to_string()) });
                }
            }
            Err(e) => {
                let name = path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
                self.toast(format!("Could not delete {name}: {e}"), cx);
            }
        }
        cx.notify();
    }

    /// Attaches the file to the focused session's next message.
    fn files_attach(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.tabs.files.menu = None;
        let Some(id) = self.state.workspace.layout.focus.clone() else {
            self.toast("Focus a session to attach the file to.", cx);
            return;
        };
        match workspace::resolve(&self.files_root(), &path) {
            Ok(real) => self.add_files(&id, &[real], cx),
            Err(e) => self.toast(format!("Could not attach {}: {e}", workspace::relative(&self.files_root(), &path)), cx),
        }
    }

    fn files_copy(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.tabs.files.menu = None;
        cx.write_to_clipboard(ClipboardItem::new_string(path.display().to_string()));
    }

    /// Opens the folder that holds `path` in the file manager.
    fn files_reveal(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.tabs.files.menu = None;
        let folder = match workspace::resolve(&self.files_root(), &path) {
            Ok(real) => real.parent().unwrap_or(real.as_path()).to_path_buf(),
            Err(e) => {
                self.toast(format!("Could not open {}: {e}", path.display()), cx);
                return;
            }
        };
        match Command::new("xdg-open").arg(&folder).spawn() {
            // Reap it, so it doesn't stay a zombie.
            Ok(mut child) => {
                std::thread::spawn(move || {
                    let _ = child.wait();
                });
            }
            Err(e) => self.toast(format!("Could not open {}: {e}", folder.display()), cx),
        }
        cx.notify();
    }
}

/// A row of the right-click menu.
fn menu_item(id: &'static str, label: &'static str, theme: &Theme, on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> AnyElement {
    div()
        .id(id)
        .px(px(10.))
        .py(px(6.))
        .rounded(px(6.))
        .text_size(px(12.5))
        .cursor_pointer()
        .hover(|d| d.bg(theme.line2.to_hsla()))
        .on_click(on_click)
        .child(label)
        .into_any_element()
}

/// A footer button: an icon beside its label, in an outlined box.
fn footer_action(id: &'static str, icon: impl IntoElement, label: &'static str, theme: &Theme) -> Stateful<Div> {
    div()
        .id(id)
        .flex_1()
        .flex()
        .items_center()
        .justify_center()
        .gap(px(6.))
        .py(px(8.))
        .rounded(px(6.))
        .border_1()
        .border_color(theme.line2.to_hsla())
        .text_size(px(12.))
        .cursor_pointer()
        .hover(|d| d.bg(theme.line2.to_hsla()))
        .child(icon)
        .child(label)
}

/// The inline name field for a new file or folder as a tree row, indented to `depth`.
fn files_create_row(depth: usize, folder: bool, input: &Entity<TextInput>, theme: &Theme) -> AnyElement {
    div()
        .h(px(30.))
        .pl(px(10. + 16. * depth as f32))
        .pr(px(10.))
        .flex()
        .items_center()
        .gap(px(8.))
        .child(if folder {
            stroke_icon(FOLDER, 15., 1.2, theme.alt.to_hsla()).into_any_element()
        } else {
            div().w(px(15.)).into_any_element()
        })
        .child(div().flex_1().min_w_0().text_size(px(13.)).child(text_input(input, theme)))
        .into_any_element()
}

fn filter_config() -> TextInputConfig {
    TextInputConfig {
        placeholder: "Filter files".into(),
        font_size: px(12.5),
        line_height: px(18.),
        ..Default::default()
    }
}

fn name_config() -> TextInputConfig {
    TextInputConfig {
        placeholder: "Name".into(),
        font_size: px(13.),
        line_height: px(18.),
        max_len: Some(255),
        ..Default::default()
    }
}

/// A file name the tree can hold: not empty, not `.` or `..`, and no path separator.
fn valid_name(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains('/') && !name.contains('\\')
}

/// One file row as the Files filter and the Ide tab's Open file… list draw it: extension badge, name, and
/// folder in grey. The caller adds the click.
pub(super) fn file_match_row(id: SharedString, root: &Path, path: &Path, selected: bool, theme: &Theme) -> Stateful<Div> {
    let name = path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let folder = path.parent().map(|p| workspace::relative(root, p)).unwrap_or_default();
    let (tag, tag_colour) = file_tag(&name, theme);
    div()
        .id(id)
        .h(px(30.))
        .px(px(10.))
        .flex()
        .items_center()
        .gap(px(8.))
        .rounded(px(6.))
        .text_size(px(13.))
        .cursor_pointer()
        .when(selected, |d| d.bg(theme.soft.to_hsla()))
        .hover(|d| d.bg(theme.line2.to_hsla()))
        .child(
            div()
                .w(px(30.))
                .flex_none()
                .text_center()
                .text_size(px(9.5))
                .font_weight(FontWeight::BOLD)
                .text_color(tag_colour.to_hsla())
                .child(tag),
        )
        .child(div().flex_1().min_w_0().truncate().child(name))
        .child(div().text_size(px(11.)).text_color(theme.muted.to_hsla()).truncate().child(folder))
}

/// The letter a change shows in the AGENT CHANGES list.
fn change_letter(kind: ChangeKind) -> char {
    match kind {
        ChangeKind::Created => 'A',
        ChangeKind::Modified => 'M',
        ChangeKind::Removed => 'D',
    }
}

/// The letter's colour in the design: `A` alt, `M` acc2, anything else muted.
fn change_colour(letter: char, theme: &Theme) -> Rgba {
    match letter {
        'A' => theme.alt,
        'M' => theme.acc2,
        _ => theme.muted,
    }
}

/// The badge text and colour for a file, by extension.
fn file_tag(name: &str, theme: &Theme) -> (&'static str, Rgba) {
    let lower = name.to_lowercase();
    if lower.ends_with(".gitignore") {
        ("GIT", theme.muted)
    } else if lower.ends_with(".ts") || lower.ends_with(".tsx") {
        ("TS", theme.acc2)
    } else if lower.ends_with(".json") {
        ("{ }", theme.alt)
    } else if lower.ends_with(".md") {
        ("MD", theme.muted)
    } else {
        ("TXT", theme.muted)
    }
}

/// The empty state of the tool tabs when no project folder is in focus.
pub(super) fn no_project(theme: &Theme) -> AnyElement {
    super::ide::placeholder("No project folder in focus. Pick a project to see its files here.", theme)
}

/// The files under `root` whose names contain `query`, ignoring case. Stops at `MAX_MATCHES`.
pub(super) fn search(root: &Path, query: &str) -> Vec<PathBuf> {
    let needle = query.to_lowercase();
    let mut found = Vec::new();
    let mut queue = VecDeque::from([root.to_path_buf()]);
    let mut read = 0;
    while let Some(dir) = queue.pop_front() {
        read += 1;
        if read > MAX_SEARCH_DIRS {
            break;
        }
        let Ok(entries) = workspace::list_dir(root, &dir) else { continue };
        for entry in entries {
            if entry.is_dir {
                queue.push_back(entry.path);
            } else if entry.name.to_lowercase().contains(&needle) {
                found.push(entry.path);
                if found.len() >= MAX_MATCHES {
                    found.sort();
                    return found;
                }
            }
        }
    }
    found.sort();
    found
}

/// The footer's details of a text file in `root`. `None` for folders, files over `MAX_INFO_BYTES`, non-UTF-8
/// files, and paths outside `root`.
fn read_info(root: &Path, path: &Path) -> Option<Info> {
    let real = workspace::resolve(root, path).ok()?;
    let meta = real.metadata().ok()?;
    if !meta.is_file() || meta.len() > MAX_INFO_BYTES {
        return None;
    }
    let text = workspace::read_to_string(root, path).ok()?;
    let lines: Vec<&str> = text.lines().collect();
    let head: Vec<String> = lines.iter().take(PREVIEW_LINES).map(|line| line.to_string()).collect();
    let preview = head.iter().map(|text| (text.clone(), tokenize(text))).collect();
    Some(Info { path: path.to_path_buf(), size: meta.len(), lines: lines.len(), modified: meta.modified().ok(), preview })
}

/// A folder's entry count as its row shows it: `1 item`, `14 items`.
fn item_count(n: usize) -> String {
    format!("{n} item{}", if n == 1 { "" } else { "s" })
}

/// A byte count as the footer shows it: `402 B`, `1.2 KB`, `3.4 MB`.
fn human_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024. * 1024.))
    }
}

/// How long ago `time` was, in the footer's words: `just now`, `4 min ago`, `2 h ago`, `3 days ago`.
fn ago(time: SystemTime) -> String {
    let secs = SystemTime::now().duration_since(time).map_or(0, |d| d.as_secs());
    match secs {
        0..=59 => "just now".to_string(),
        60..=3599 => format!("{} min ago", secs / 60),
        3600..=86_399 => format!("{} h ago", secs / 3600),
        _ => format!("{} days ago", secs / 86_400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// The visible rows as `(name, depth)`.
    fn rows_of(tree: &Tree) -> Vec<(String, usize)> {
        tree.rows().iter().map(|row| (row.entry.name.clone(), row.depth)).collect()
    }

    fn named(rows: &[(&str, usize)]) -> Vec<(String, usize)> {
        rows.iter().map(|(name, depth)| (name.to_string(), *depth)).collect()
    }

    #[test]
    fn listings_follow_expansion_and_creation() {
        let dir = std::env::temp_dir().join(format!("archonui-files-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::write(dir.join("src/lib.rs"), "").unwrap();
        fs::write(dir.join("Cargo.toml"), "").unwrap();

        let mut tree = Tree::new(&dir);
        assert_eq!(rows_of(&tree), named(&[("src", 0), ("Cargo.toml", 0)]));

        tree.toggle(&dir.join("src"));
        assert_eq!(rows_of(&tree), named(&[("src", 0), ("lib.rs", 1), ("Cargo.toml", 0)]));

        fs::write(dir.join("src/main.rs"), "").unwrap();
        tree.changed(&dir.join("src/main.rs"), ChangeKind::Created);
        assert_eq!(rows_of(&tree).len(), 4);

        fs::remove_dir_all(dir.join("src")).unwrap();
        tree.changed(&dir.join("src"), ChangeKind::Removed);
        assert_eq!(rows_of(&tree), named(&[("Cargo.toml", 0)]));

        tree.expand(&dir.join("src"));
        assert!(tree.is_expanded(&dir.join("src")));
        tree.collapse_all();
        assert!(!tree.is_expanded(&dir.join("src")));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn change_letters_follow_the_kind() {
        assert_eq!(change_letter(ChangeKind::Created), 'A');
        assert_eq!(change_letter(ChangeKind::Modified), 'M');
        assert_eq!(change_letter(ChangeKind::Removed), 'D');
    }

    #[test]
    fn sizes_ages_and_names_read_as_the_footer_shows_them() {
        assert_eq!(item_count(1), "1 item");
        assert_eq!(item_count(0), "0 items");
        assert_eq!(item_count(14), "14 items");
        assert_eq!(human_size(402), "402 B");
        assert_eq!(human_size(1536), "1.5 KB");
        assert_eq!(human_size(3 * 1024 * 1024), "3.0 MB");
        let four_minutes = SystemTime::now() - Duration::from_secs(4 * 60 + 5);
        assert_eq!(ago(four_minutes), "4 min ago");
        assert!(valid_name("notes.md"));
        assert!(!valid_name(".."));
        assert!(!valid_name("a/b"));
    }

    #[test]
    fn search_filters_files_by_case_insensitive_name() {
        let dir = std::env::temp_dir().join(format!("archonui-search-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("sub")).unwrap();
        fs::write(dir.join("main.rs"), "").unwrap();
        fs::write(dir.join("sub/Session.ts"), "").unwrap();
        fs::write(dir.join("sub/other.txt"), "").unwrap();

        let all = search(&dir, "");
        assert_eq!(all.len(), 3);

        let ts = search(&dir, "session");
        assert_eq!(ts.len(), 1);
        assert_eq!(ts[0].file_name().unwrap(), "Session.ts");

        let none = search(&dir, "xyz");
        assert!(none.is_empty());

        let _ = fs::remove_dir_all(&dir);
    }
}
