//! The built-in editor in the right-hand panel's IDE tab: a tab per open
//! file, a virtualized text view, and the keys and mouse that edit it.

mod changes;
mod tree;

use super::{button, terminal_panel::{cell_width, FONT_FAMILY}, ChatView};
use crate::{
    editor::{
        buffer::{Buffer, Pos},
        diff::{first_change, line_marks, Mark},
        highlight::{Highlighter, Span, Style},
    },
    store::{IdeKind, PanelTab},
    workspace::{self, ChangeKind, Watcher},
};
use gpui::{
    canvas, div, font, prelude::*, px, rgb, uniform_list, Bounds, ClickEvent, ClipboardItem, Context, Div, Font,
    FontStyle, FontWeight, KeyDownEvent, MouseButton, MouseDownEvent, PathPromptOptions, Pixels, Point, ScrollStrategy,
    ScrollWheelEvent, StyledText, Task, TextRun, UniformListScrollHandle, Window,
};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    io,
    ops::Range,
    path::{Path, PathBuf},
    rc::Rc,
    time::{Duration, Instant},
};
use tree::Tree;

const FONT_SIZE: f32 = 13.;
/// Height of a line, which is also the height of its row.
const LINE_H: f32 = 19.;
/// Width of the change-marker column between the gutter and the text.
const BAR_W: f32 = 3.;
/// Columns between tab stops.
const TAB_WIDTH: usize = 4;
/// Time the highlighter may spend on the visible lines in one frame.
const FRAME_BUDGET: Duration = Duration::from_millis(6);
const BG: u32 = 0x1e1e2e;
const FG: u32 = 0xcdd6f4;
const PLAIN: Style = Style { fg: FG, bold: false, italic: false };
const LINE_NUMBER: u32 = 0x6c7086;
const CURRENT_LINE: u32 = 0x262838;
const SELECTION: u32 = 0x45475a;
const CURSOR: u32 = 0xf5e0dc;
const ADDED: u32 = 0xa6e3a1;
const MODIFIED: u32 = 0x89b4fa;
const REMOVED: u32 = 0xf38ba8;
/// How long typing must pause before the change markers are recomputed.
const MARKS_IDLE: Duration = Duration::from_millis(300);

/// The files open in the built-in editor, per chat.
pub struct Editors {
    sets: HashMap<String, EditorSet>,
    /// Keystrokes go to the editor (after clicking into it).
    focused: bool,
    /// Set from a left press in the editor until the button comes up.
    selecting: bool,
    /// Where the editor's text area was last laid out, for mapping mouse positions.
    bounds: Rc<Cell<Bounds<Pixels>>>,
    /// Width of a cell, as of the last render.
    cell_w: Pixels,
    /// Whether the file tree pane is shown.
    tree_shown: bool,
    /// Whether the editor follows the files the agent changes.
    follow: bool,
}

impl Default for Editors {
    fn default() -> Self {
        Self {
            sets: HashMap::new(),
            focused: false,
            selecting: false,
            bounds: Rc::default(),
            cell_w: Pixels::default(),
            tree_shown: true,
            follow: true,
        }
    }
}

/// One chat's open files, file tree, and the agent's changes to its folder.
#[derive(Default)]
struct EditorSet {
    /// The chat's folder, canonicalized: the root of the tree and of the watch.
    root: PathBuf,
    files: Vec<OpenFile>,
    active: usize,
    /// The file whose "Save changes?" bar is showing.
    closing: Option<usize>,
    /// Why the last file could not be opened.
    error: Option<String>,
    tree: Tree,
    /// Watches `root`, from when the IDE tab shows or a turn starts. Holds why it failed, if it did.
    watch: Option<Result<Watcher, String>>,
    /// Files changed since the last Clear, each with its latest kind and when it last changed.
    changes: Vec<(PathBuf, ChangeKind, Instant)>,
    /// What each changed file held before its first change since the last Clear.
    baselines: HashMap<PathBuf, String>,
    /// Counts the Clears, so a baseline read before one is dropped.
    generation: u64,
    /// A file the agent changed while the editor had the keyboard, offered in the toolbar.
    notice: Option<PathBuf>,
}

/// An open file: its text, how it is highlighted, and how it differs from its saved text.
struct OpenFile {
    buffer: Buffer,
    /// In a `RefCell` so rows can be highlighted while the view is only borrowed.
    highlighter: RefCell<Highlighter>,
    /// The text as last saved, which the change markers compare against.
    saved: String,
    /// Changed lines against `saved`, sorted by line.
    marks: Vec<(usize, Mark)>,
    scroll: UniformListScrollHandle,
    /// How far the text is scrolled sideways, in pixels.
    h_offset: f32,
    /// Why the last save failed.
    error: Option<String>,
    /// Counts the edits, so markers computed from an older text are dropped.
    version: u64,
    /// Recomputes `marks`. Replacing or dropping it cancels the recomputation.
    marks_task: Option<Task<()>>,
    /// Scroll to the first change once `marks` have one, for a change the user is being shown.
    reveal_change: bool,
    /// Changed on disk while the buffer had unsaved edits, so it was not reloaded.
    changed_on_disk: bool,
    /// Removed on disk.
    deleted: bool,
}

impl Editors {
    /// Width of the editor's area, or 0 before its first layout.
    fn editor_width(&self) -> f32 {
        f32::from(self.bounds.get().size.width)
    }

    /// Whole lines that fit the editor's area, for paging.
    fn page_lines(&self) -> usize {
        ((f32::from(self.bounds.get().size.height) / LINE_H).floor() as usize).max(1)
    }
}

impl EditorSet {
    /// A chat's editor state, for a chat whose folder is `cwd`.
    fn new(cwd: &Path) -> Self {
        let root = std::fs::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf());
        Self { tree: Tree::new(&root), root, ..Self::default() }
    }

    fn active_file(&mut self) -> Option<&mut OpenFile> {
        self.files.get_mut(self.active)
    }

    /// Saves file `index`, and tells the watcher to ignore the write. A failure is kept on the file for
    /// the status line.
    fn save(&mut self, index: usize) -> bool {
        let Some(file) = self.files.get_mut(index) else { return false };
        if let Some(path) = file.buffer.path() {
            if let Err(e) = workspace::resolve(&self.root, path) {
                file.error = Some(format!("Could not save {}: {e}", path.display()));
                return false;
            }
        }
        if let (Some(Ok(watcher)), Some(path)) = (&self.watch, file.buffer.path()) {
            watcher.ignore_own_write(path);
        }
        match file.buffer.save() {
            Ok(()) => {
                file.saved = file.buffer.text();
                file.error = None;
                file.deleted = false;
                file.changed_on_disk = false;
                true
            }
            Err(e) => {
                file.error = Some(format!("Could not save: {e}"));
                false
            }
        }
    }

    /// Drops file `index`, keeping the active file where it was when it can.
    fn remove(&mut self, index: usize) {
        self.files.remove(index);
        self.closing = None;
        if index < self.active || self.active >= self.files.len() {
            self.active = self.active.saturating_sub(1);
        }
    }
}

impl OpenFile {
    fn new(buffer: Buffer) -> Self {
        let first = buffer.line(0);
        Self {
            highlighter: RefCell::new(Highlighter::new(buffer.path(), &first)),
            saved: buffer.text(),
            buffer,
            marks: Vec::new(),
            scroll: UniformListScrollHandle::new(),
            h_offset: 0.,
            error: None,
            version: 0,
            marks_task: None,
            reveal_change: false,
            changed_on_disk: false,
            deleted: false,
        }
    }

    /// Brings highlighting up to date after an edit, and counts the edit. Returns whether there was one.
    fn sync_edit(&mut self) -> bool {
        let Some((first, removed, inserted)) = self.buffer.take_edit() else { return false };
        self.highlighter.get_mut().edited(first, removed, inserted);
        self.version += 1;
        true
    }

    /// Takes a change to the file on disk: a clean buffer reloads, an edited one is flagged, and a
    /// file that is gone is marked deleted.
    fn take_disk_change(&mut self, kind: ChangeKind) {
        let Some(path) = self.buffer.path().map(Path::to_path_buf) else { return };
        if kind == ChangeKind::Removed {
            self.deleted = true;
            return;
        }
        match Buffer::open(&path) {
            Ok(disk) => {
                self.deleted = false;
                let text = disk.text();
                if text == self.saved {
                    // Back to the saved text, so the disk no longer differs from what the user sees.
                    self.changed_on_disk = false;
                    return;
                }
                if self.buffer.is_dirty() {
                    self.changed_on_disk = true;
                } else {
                    self.reload(&text);
                }
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => self.deleted = true,
            Err(e) => self.error = Some(format!("Could not read {}: {e}", path.display())),
        }
    }

    /// Replaces the buffer with `text` from disk, which becomes its saved text.
    fn reload(&mut self, text: &str) {
        self.buffer.reload_from(text);
        self.sync_edit();
        self.saved = self.buffer.text();
        self.changed_on_disk = false;
        self.deleted = false;
    }

    /// Scrolls the list and the text so the cursor shows. `editor_w` is 0 before the first layout.
    fn reveal(&mut self, cell_w: f32, editor_w: f32) {
        let cursor = self.buffer.cursor();
        self.scroll.scroll_to_item(cursor.line, ScrollStrategy::Center);
        let text_w = text_width(editor_w, cell_w, self.buffer.line_count());
        if text_w <= 0. {
            return;
        }
        let x = visual_col(&self.buffer.line(cursor.line), cursor.col) as f32 * cell_w;
        if x < self.h_offset {
            self.h_offset = x;
        } else if x + cell_w > self.h_offset + text_w {
            self.h_offset = x + cell_w - text_w;
        }
    }
}

impl ChatView {
    /// Whether the IDE tab shows the built-in editor.
    fn editor_visible(&self) -> bool {
        !self.settings.hide_panel
            && !self.show_settings
            && self.settings.panel_tab == PanelTab::Ide
            && self.settings.ide == IdeKind::Builtin
    }

    /// Shows the IDE tab with the built-in editor, and gives it the keyboard.
    pub(super) fn show_editor(&mut self, cx: &mut Context<Self>) {
        self.settings.hide_panel = false;
        self.settings.panel_tab = PanelTab::Ide;
        self.settings.ide = IdeKind::Builtin;
        self.save_settings();
        self.editors.focused = true;
        cx.notify();
    }

    /// Measures a cell, which clicks and the caret need. Starts watching the chat's folder while the
    /// built-in editor shows.
    pub(super) fn editor_before_render(&mut self, window: &Window, cx: &mut Context<Self>) {
        self.editors.cell_w = cell_width(window, FONT_SIZE);
        if let Some(id) = self.current.clone().filter(|_| self.editor_visible()) {
            self.watch_chat(&id, cx);
        }
    }

    /// Opens `path` in the current chat's editor, or activates it when it is already open.
    pub(super) fn open_in_editor(&mut self, path: &Path, cx: &mut Context<Self>) {
        let Some(id) = self.session().map(|s| s.id.clone()) else { return };
        self.open_in(&id, path, false, cx);
    }

    /// Opens `path` in chat `id`'s editor, or activates it when it is already open. With `reveal`, the
    /// view scrolls to the file's first change once its markers are computed.
    pub(super) fn open_in(&mut self, id: &str, path: &Path, reveal: bool, cx: &mut Context<Self>) {
        let Some(set) = self.set_of(id) else { return };
        set.error = None;
        // Only files inside the chat's project folder open.
        let path = match workspace::resolve(&set.root, path) {
            Ok(real) => real,
            Err(e) => {
                set.error = Some(format!("Could not open {}: {e}", path.display()));
                cx.notify();
                return;
            }
        };
        let index = match set.files.iter().position(|f| f.buffer.path() == Some(path.as_path())) {
            Some(index) => index,
            None => match Buffer::open(&path) {
                Ok(buffer) => {
                    set.files.push(OpenFile::new(buffer));
                    set.files.len() - 1
                }
                Err(e) => {
                    set.error = Some(format!("Could not open {}: {e}", path.display()));
                    cx.notify();
                    return;
                }
            },
        };
        set.active = index;
        set.files[index].reveal_change = reveal;
        self.refresh_marks(id, index, Duration::ZERO, cx);
        cx.notify();
    }

    /// Chat `id`'s editor state, created on first use with the chat's folder as the tree's root.
    fn set_of(&mut self, id: &str) -> Option<&mut EditorSet> {
        if !self.editors.sets.contains_key(id) {
            let cwd = self.sessions.iter().find(|s| s.id == id)?.cwd.clone();
            self.editors.sets.insert(id.to_string(), EditorSet::new(&cwd));
        }
        self.editors.sets.get_mut(id)
    }

    /// Starts watching chat `id`'s folder for the agent's edits, once. Called while the IDE tab shows
    /// and when a turn starts, so changes are tracked even while the IDE tab is hidden.
    pub(super) fn watch_chat(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(set) = self.set_of(id) else { return };
        if set.watch.is_some() {
            return;
        }
        match Watcher::new(&set.root) {
            Ok((watcher, rx)) => {
                set.watch = Some(Ok(watcher));
                let id = id.to_string();
                cx.spawn(async move |this, cx| {
                    while let Ok(change) = rx.recv().await {
                        if this.update(cx, |this, cx| this.agent_change(&id, change, cx)).is_err() {
                            break;
                        }
                    }
                })
                .detach();
            }
            Err(e) => {
                eprintln!("could not watch {}: {e}", set.root.display());
                set.watch = Some(Err(e));
            }
        }
    }

    /// Drops a deleted chat's editor state, which stops its watcher.
    pub(super) fn forget_editor(&mut self, id: &str) {
        self.editors.sets.remove(id);
    }

    /// The index of the open file at `path` in chat `id`'s editor.
    fn file_index(&self, id: &str, path: &Path) -> Option<usize> {
        self.editors.sets.get(id)?.files.iter().position(|f| f.buffer.path() == Some(path))
    }

    /// Recomputes the change markers of file `index` in chat `id` after `delay`, off the UI thread.
    /// They compare against the file's baseline, else its saved text. A new request replaces this one,
    /// and an edit made while it runs discards its result.
    fn refresh_marks(&mut self, id: &str, index: usize, delay: Duration, cx: &mut Context<Self>) {
        let Some(set) = self.editors.sets.get_mut(id) else { return };
        let Some(path) = set.files.get(index).and_then(|f| f.buffer.path()).map(Path::to_path_buf) else { return };
        let id = id.to_string();
        let task = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(delay).await;
            // The texts are copied after the wait, so typing copies them once per pause.
            let Ok(Some((version, old, new))) = this.update(cx, |this, _| this.marks_input(&id, &path)) else {
                return;
            };
            let marks = cx.background_executor().spawn(async move { line_marks(&old, &new) }).await;
            this.update(cx, |this, cx| this.apply_marks(&id, &path, version, marks, cx)).ok();
        });
        set.files[index].marks_task = Some(task);
    }

    /// The version of file `path` in chat `id`, and the texts its markers compare: its baseline or saved
    /// text, and its current text.
    fn marks_input(&self, id: &str, path: &Path) -> Option<(u64, String, String)> {
        let set = self.editors.sets.get(id)?;
        let file = set.files.iter().find(|f| f.buffer.path() == Some(path))?;
        let old = set.baselines.get(path).cloned().unwrap_or_else(|| file.saved.clone());
        Some((file.version, old, file.buffer.text()))
    }

    /// Stores markers computed for `version` of file `path`, unless the file has changed since.
    fn apply_marks(&mut self, id: &str, path: &Path, version: u64, marks: Vec<(usize, Mark)>, cx: &mut Context<Self>) {
        let Some(set) = self.editors.sets.get_mut(id) else { return };
        let Some(file) = set.files.iter_mut().find(|f| f.buffer.path() == Some(path)) else { return };
        if file.version != version {
            return;
        }
        if file.reveal_change {
            if let Some(line) = first_change(&marks) {
                file.reveal_change = false;
                file.scroll.scroll_to_item(line, ScrollStrategy::Center);
            }
        }
        file.marks = marks;
        cx.notify();
    }

    /// Asks for a file to open in the editor.
    fn pick_file(&mut self, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Open".into()),
        });
        cx.spawn(async move |this, cx| {
            let result = picked.await;
            this.update(cx, |this, cx| {
                if let Ok(Ok(Some(paths))) = result {
                    if let Some(path) = paths.first() {
                        this.open_in_editor(path, cx);
                    }
                }
            })
            .ok();
        })
        .detach();
    }

    fn editor_set(&self) -> Option<&EditorSet> {
        self.editors.sets.get(self.current.as_deref()?)
    }

    fn editor_set_mut(&mut self) -> Option<&mut EditorSet> {
        let id = self.current.as_deref()?;
        self.editors.sets.get_mut(id)
    }

    fn active_file(&self) -> Option<&OpenFile> {
        let set = self.editor_set()?;
        set.files.get(set.active)
    }

    fn activate_file(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(set) = self.editor_set_mut() {
            if index < set.files.len() {
                set.active = index;
            }
        }
        cx.notify();
    }

    /// Asks before closing a file with unsaved changes; closes it otherwise.
    fn close_file(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(set) = self.editor_set_mut() {
            if set.files.get(index).is_some_and(|f| f.buffer.is_dirty()) {
                set.closing = Some(index);
            } else {
                set.remove(index);
            }
        }
        cx.notify();
    }

    /// Closes file `index` after its "Save changes?" bar. A save that fails keeps it open.
    fn finish_close(&mut self, index: usize, save: bool, cx: &mut Context<Self>) {
        if let Some(set) = self.editor_set_mut() {
            if !save || set.save(index) {
                set.remove(index);
            }
        }
        cx.notify();
    }

    fn cancel_close(&mut self, cx: &mut Context<Self>) {
        if let Some(set) = self.editor_set_mut() {
            set.closing = None;
        }
        cx.notify();
    }

    /// Saves the active file, and recomputes its markers, which a baseline keeps.
    fn save_active(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.current.clone() else { return };
        let Some(set) = self.editors.sets.get_mut(&id) else { return };
        let index = set.active;
        if set.save(index) {
            self.refresh_marks(&id, index, Duration::ZERO, cx);
        }
        cx.notify();
    }

    /// Gives the keyboard back to the chat when a click lands outside the editor.
    pub(super) fn editor_blur(&mut self, cx: &mut Context<Self>) {
        if self.editors.focused {
            self.editors.focused = false;
            cx.notify();
        }
    }

    /// Keys for the built-in editor while it has the keyboard. Returns true if consumed.
    pub(super) fn editor_key(&mut self, ev: &KeyDownEvent, cx: &mut Context<Self>) -> bool {
        let ks = &ev.keystroke;
        let m = ks.modifiers;
        if !self.editors.focused || !self.editor_visible() || m.platform {
            return false;
        }
        // Chat shortcuts stay with the chat.
        if m.control && matches!(ks.key.as_str(), "n" | "i" | ",") {
            return false;
        }
        if ks.key == "escape" {
            self.editor_blur(cx);
            return true;
        }
        let (editor_w, cell_w, page) =
            (self.editors.editor_width(), f32::from(self.editors.cell_w), self.editors.page_lines());
        let clipboard = if m.control && ks.key == "v" { cx.read_from_clipboard().and_then(|c| c.text()) } else { None };
        let Some(set) = self.editor_set_mut() else { return false };
        if set.files.is_empty() {
            return false;
        }
        if m.control && ks.key == "s" {
            self.save_active(cx);
            return true;
        }
        let index = set.active;
        let file = &mut set.files[index];
        let buffer = &mut file.buffer;
        let select = m.shift;
        let handled = if m.alt {
            false
        } else if m.control {
            match ks.key.as_str() {
                "a" => {
                    buffer.select_all();
                    true
                }
                "c" => {
                    if let Some(text) = buffer.copy() {
                        cx.write_to_clipboard(ClipboardItem::new_string(text));
                    }
                    true
                }
                "x" => {
                    if let Some(text) = buffer.cut() {
                        cx.write_to_clipboard(ClipboardItem::new_string(text));
                    }
                    true
                }
                "v" => {
                    if let Some(text) = &clipboard {
                        buffer.paste(text);
                    }
                    true
                }
                "z" if m.shift => {
                    buffer.redo();
                    true
                }
                "z" => {
                    buffer.undo();
                    true
                }
                "y" => {
                    buffer.redo();
                    true
                }
                "backspace" => {
                    buffer.delete_word_left();
                    true
                }
                "left" => {
                    buffer.move_word_left(select);
                    true
                }
                "right" => {
                    buffer.move_word_right(select);
                    true
                }
                "home" => {
                    buffer.move_doc_start(select);
                    true
                }
                "end" => {
                    buffer.move_doc_end(select);
                    true
                }
                _ => false,
            }
        } else {
            match ks.key.as_str() {
                "enter" => {
                    buffer.newline();
                    true
                }
                "backspace" => {
                    buffer.backspace();
                    true
                }
                "delete" => {
                    buffer.delete();
                    true
                }
                "tab" if m.shift => {
                    buffer.outdent();
                    true
                }
                "tab" => {
                    buffer.indent();
                    true
                }
                "left" => {
                    buffer.move_left(select);
                    true
                }
                "right" => {
                    buffer.move_right(select);
                    true
                }
                "up" => {
                    buffer.move_up(select);
                    true
                }
                "down" => {
                    buffer.move_down(select);
                    true
                }
                "home" => {
                    buffer.move_home(select);
                    true
                }
                "end" => {
                    buffer.move_end(select);
                    true
                }
                "pageup" => {
                    buffer.page_up(page, select);
                    true
                }
                "pagedown" => {
                    buffer.page_down(page, select);
                    true
                }
                _ => match ks.key_char.as_deref() {
                    Some(text) => {
                        buffer.insert(text);
                        true
                    }
                    None => false,
                },
            }
        };
        if !handled {
            return false;
        }
        let edited = file.sync_edit();
        if edited {
            file.reveal_change = false;
        }
        file.reveal(cell_w, editor_w);
        cx.notify();
        if edited {
            if let Some(id) = self.current.clone() {
                self.refresh_marks(&id, index, MARKS_IDLE, cx);
            }
        }
        true
    }

    /// A left press places the cursor, extends the selection with shift, and on a second or third
    /// click selects the word or the line.
    fn editor_press(&mut self, ev: &MouseDownEvent) {
        let origin = self.editors.bounds.get().origin;
        let (editor_w, cell_w) = (self.editors.editor_width(), f32::from(self.editors.cell_w));
        self.editors.selecting = ev.click_count == 1;
        let Some(file) = self.editor_set_mut().and_then(EditorSet::active_file) else { return };
        let at = pos_at(file, ev.position, origin, cell_w);
        match ev.click_count {
            1 => file.buffer.set_cursor(at, ev.modifiers.shift),
            2 => file.buffer.select_word_at(at),
            _ => file.buffer.select_line(at.line),
        }
        file.reveal(cell_w, editor_w);
    }

    /// Extends a drag selection to `pos`, which may lie outside the editor.
    pub(super) fn editor_drag_move(&mut self, pos: Point<Pixels>, cx: &mut Context<Self>) {
        if !self.editors.selecting {
            return;
        }
        let origin = self.editors.bounds.get().origin;
        let (editor_w, cell_w) = (self.editors.editor_width(), f32::from(self.editors.cell_w));
        let Some(file) = self.editor_set_mut().and_then(EditorSet::active_file) else { return };
        let at = pos_at(file, pos, origin, cell_w);
        file.buffer.set_cursor(at, true);
        file.reveal(cell_w, editor_w);
        cx.notify();
    }

    pub(super) fn editor_drag_end(&mut self, cx: &mut Context<Self>) {
        if std::mem::take(&mut self.editors.selecting) {
            cx.notify();
        }
    }

    /// Shift+wheel scrolls the text sideways. A plain wheel scrolls the list.
    fn editor_scroll(&mut self, ev: &ScrollWheelEvent, cx: &mut Context<Self>) {
        let delta = ev.delta.pixel_delta(px(LINE_H));
        let by = f32::from(delta.x) + f32::from(delta.y);
        let (editor_w, cell_w) = (self.editors.editor_width(), f32::from(self.editors.cell_w));
        let Some(file) = self.editor_set_mut().and_then(EditorSet::active_file) else { return };
        let max = max_h_offset(file, cell_w, editor_w);
        file.h_offset = (file.h_offset - by).clamp(0., max);
        cx.notify();
    }

    /// The rows for `lines` of the active file. Lines the highlighter cannot reach before `deadline`
    /// are drawn unstyled, and the next frame tries again.
    fn editor_rows(&self, lines: Range<usize>, deadline: Instant, window: &mut Window, cx: &mut Context<Self>) -> Vec<Div> {
        let Some(file) = self.active_file() else { return Vec::new() };
        let buffer = &file.buffer;
        let line_count = buffer.line_count();
        let cell_w = f32::from(self.editors.cell_w);
        let cursor = buffer.cursor();
        let focused = self.editors.focused;
        let selection = buffer.selection_range();
        let gutter = gutter_width(line_count, cell_w);
        let mut rows = Vec::with_capacity(lines.len());
        for line in lines {
            let text = buffer.line(line);
            let spans = file.highlighter.borrow_mut().spans_by(|i| buffer.line(i), line_count, line, deadline);
            if spans.is_none() {
                window.request_animation_frame();
            }
            let (selected, continues) = selected_cols(selection, line, text.chars().count());
            let (display, runs) = row_text(&text, spans.as_deref().unwrap_or_default(), selected, continues);
            let current = line == cursor.line;
            let mark = file.marks.binary_search_by_key(&line, |&(at, _)| at).ok().map(|i| file.marks[i].1);
            let caret = (focused && current).then(|| visual_col(&text, cursor.col) as f32 * cell_w);
            let bar = div().w(px(BAR_W)).h_full().flex_none();
            let bar = match mark {
                Some(Mark::Added) => bar.bg(rgb(ADDED)),
                Some(Mark::Modified) => bar.bg(rgb(MODIFIED)),
                Some(Mark::Removed) => bar.child(div().w_full().h(px(4.)).bg(rgb(REMOVED))),
                None => bar,
            };
            let number_color = if current { FG } else { LINE_NUMBER };
            // The text scrolls sideways inside a clipped box, so it never paints over the gutter.
            let shifted = div()
                .absolute()
                .top_0()
                .left(px(-file.h_offset))
                .whitespace_nowrap()
                .child(StyledText::new(display).with_runs(runs))
                .children(caret.map(|x| div().absolute().top_0().left(px(x)).w(px(2.)).h(px(LINE_H)).bg(rgb(CURSOR))));
            let text_part = div().flex_1().min_w_0().h_full().relative().overflow_hidden().child(shifted);
            rows.push(
                div()
                    .h(px(LINE_H))
                    .w_full()
                    .flex()
                    .when(current && focused, |d| d.bg(rgb(CURRENT_LINE)))
                    .on_scroll_wheel(cx.listener(|this, ev: &ScrollWheelEvent, _, cx| {
                        // Only shift+wheel is taken here; the list scrolls on a plain wheel.
                        if ev.modifiers.shift {
                            this.editor_scroll(ev, cx);
                            cx.stop_propagation();
                        }
                    }))
                    .child(
                        div()
                            .w(px(gutter))
                            .flex_none()
                            .pr(px(8.))
                            .text_right()
                            .text_color(rgb(number_color))
                            .child((line + 1).to_string()),
                    )
                    .child(bar)
                    .child(text_part),
            );
        }
        rows
    }

    /// The IDE tab's built-in editor: open files, the active file's text and a status line.
    pub(super) fn render_editor(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let deadline = Instant::now() + FRAME_BUDGET;
        let set = self.editor_set();
        let active = set.map_or(0, |s| s.active);
        let files = set.map_or(&[][..], |s| s.files.as_slice());

        let tabs = div().px_2().pt_2().flex().gap_1().overflow_hidden().children(files.iter().enumerate().map(
            |(i, file)| {
                let dirty = if file.buffer.is_dirty() { " ●" } else { "" };
                let deleted = if file.deleted { " (deleted)" } else { "" };
                div()
                    .id(("editor-tab", i))
                    .flex()
                    .gap_2()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .text_xs()
                    .cursor_pointer()
                    .when(i == active, |d| d.bg(rgb(0x313244)))
                    .hover(|d| d.bg(rgb(0x2a2b3c)))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.activate_file(i, cx)))
                    .child(format!("{}{dirty}{deleted}", file_name(file)))
                    .child(
                        div()
                            .id(("editor-close", i))
                            .text_color(rgb(0x585b70))
                            .hover(|d| d.text_color(rgb(0xf38ba8)))
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                cx.stop_propagation();
                                this.close_file(i, cx);
                            }))
                            .child("×"),
                    )
            },
        ));

        let closing = set.and_then(|s| s.closing).and_then(|i| files.get(i).map(|file| (i, file_name(file))));
        let closing_bar = closing.map(|(i, name)| {
            div()
                .mx_2()
                .mb_1()
                .px_2()
                .py_1()
                .rounded_md()
                .bg(rgb(0x313244))
                .flex()
                .items_center()
                .gap_1()
                .text_xs()
                .child(div().flex_1().min_w_0().truncate().child(format!("Save changes to {name}?")))
                .child(button("editor-close-save", "Save").on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.finish_close(i, true, cx)
                })))
                .child(button("editor-close-discard", "Discard").on_click(cx.listener(
                    move |this, _: &ClickEvent, _, cx| this.finish_close(i, false, cx),
                )))
                .child(button("editor-close-cancel", "Cancel").on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.cancel_close(cx)
                })))
        });

        let active_file = set.and_then(|s| s.files.get(s.active));
        let path = active_file
            .and_then(|f| f.buffer.path())
            .map_or_else(String::new, |p| p.display().to_string());
        let toolbar = div()
            .px_2()
            .py_1()
            .flex()
            .items_center()
            .gap_1()
            .child(button("editor-open", "Open file…").on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                this.pick_file(cx)
            })))
            .child(button("editor-save", "Save").on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                this.save_active(cx)
            })))
            .child(
                button("editor-files", "☰ Files")
                    .when(self.editors.tree_shown, |b| b.bg(rgb(0x45475a)))
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.editors.tree_shown = !this.editors.tree_shown;
                        cx.notify();
                    })),
            )
            .child(
                button("editor-follow", "Follow agent")
                    .when(self.editors.follow, |b| b.bg(rgb(0x45475a)))
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.editors.follow = !this.editors.follow;
                        cx.notify();
                    })),
            )
            .child(div().flex_1().min_w_0().px_2().text_xs().text_color(rgb(0x7f849c)).truncate().child(path));

        let error = set.and_then(|s| s.error.clone());
        let error_bar = error.map(|text| {
            div().mx_2().mb_1().px_2().py_1().rounded_md().bg(rgb(0x4a2530)).text_xs().child(text)
        });

        let changed_bar = active_file.filter(|file| file.changed_on_disk).map(|file| {
            div()
                .mx_2()
                .mb_1()
                .px_2()
                .py_1()
                .rounded_md()
                .bg(rgb(0x313244))
                .flex()
                .items_center()
                .gap_1()
                .text_xs()
                .child(div().flex_1().min_w_0().truncate().child(format!("{} was changed on disk.", file_name(file))))
                .child(button("editor-disk-reload", "Reload").on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.reload_from_disk(cx)
                })))
                .child(button("editor-disk-keep", "Keep mine").on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.keep_mine(cx)
                })))
        });

        let notice = set.and_then(|s| s.notice.as_ref().map(|path| workspace::relative(&s.root, path)));
        let notice_bar = notice.map(|name| {
            div()
                .mx_2()
                .mb_1()
                .px_2()
                .py_1()
                .rounded_md()
                .bg(rgb(0x313244))
                .flex()
                .items_center()
                .gap_1()
                .text_xs()
                .child(div().flex_1().min_w_0().truncate().child(format!("Agent changed {name}")))
                .child(button("editor-notice-view", "View").on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.view_notice(cx)
                })))
                .child(button("editor-notice-dismiss", "×").on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.dismiss_notice(cx)
                })))
        });

        let body = match active_file {
            Some(file) => {
                let bounds = self.editors.bounds.clone();
                let list = uniform_list(
                    "editor-lines",
                    file.buffer.line_count(),
                    cx.processor(move |this: &mut Self, lines: Range<usize>, window: &mut Window, cx: &mut Context<Self>| {
                        this.editor_rows(lines, deadline, window, cx)
                    }),
                )
                .track_scroll(file.scroll.clone())
                .size_full();
                div()
                    .flex_1()
                    .min_h_0()
                    .relative()
                    .overflow_hidden()
                    .bg(rgb(BG))
                    .font_family(FONT_FAMILY)
                    .text_size(px(FONT_SIZE))
                    .line_height(px(LINE_H))
                    // Records where the text area is, so clicks can be mapped to positions.
                    .child(canvas(move |b, _, _| bounds.set(b), |_, _, _, _| {}).absolute().top_0().left_0().size_full())
                    .child(list)
                    .on_mouse_down(MouseButton::Left, cx.listener(|this, ev: &MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        window.focus(&this.focus);
                        this.preview_blur(cx);
                        this.term_blur(cx);
                        this.git_blur(cx);
                        this.editors.focused = true;
                        this.editor_press(ev);
                        cx.notify();
                    }))
                    .into_any_element()
            }
            None => div()
                .flex_1()
                .min_h_0()
                .flex()
                .items_center()
                .justify_center()
                .text_xs()
                .text_color(rgb(0x7f849c))
                .child("Open a file with Open file…, or /edit <path>")
                .into_any_element(),
        };

        let status = active_file.map(|file| {
            let cursor = file.buffer.cursor();
            let line = file.buffer.line(cursor.line);
            let language = match file.highlighter.borrow().language() {
                Some(name) => name.to_string(),
                None => file
                    .buffer
                    .path()
                    .and_then(Path::extension)
                    .and_then(|ext| ext.to_str())
                    .map_or_else(|| "Plain Text".to_string(), str::to_string),
            };
            let indent = match file.buffer.indent_unit() {
                "\t" => "Tabs".to_string(),
                unit => format!("Spaces: {}", unit.len()),
            };
            let saved = if file.buffer.is_dirty() { "Unsaved" } else { "Saved" };
            div()
                .h(px(22.))
                .px_2()
                .flex()
                .items_center()
                .gap_3()
                .text_xs()
                .text_color(rgb(0x9399b2))
                .border_t_1()
                .border_color(rgb(0x313244))
                .child(format!("Ln {}, Col {}", cursor.line + 1, visual_col(&line, cursor.col) + 1))
                .child(language)
                .child(indent)
                .child(saved)
                .children(file.error.clone().map(|e| div().text_color(rgb(0xf38ba8)).truncate().child(e)))
        });

        let editor = div()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .flex()
            .flex_col()
            .child(tabs)
            .children(closing_bar)
            .child(toolbar)
            .children(error_bar)
            .children(changed_bar)
            .children(notice_bar)
            .child(body)
            .children(status);
        let tree = set.filter(|_| self.editors.tree_shown).map(|set| self.render_tree(set, cx));
        div().flex_1().min_h_0().flex().children(tree).child(editor)
    }
}

/// The name shown for a file in its tab.
fn file_name(file: &OpenFile) -> String {
    file.buffer
        .path()
        .and_then(Path::file_name)
        .map_or_else(|| "untitled".to_string(), |name| name.to_string_lossy().into_owned())
}

/// Width of the line-number gutter: a cell per digit of `lines`, with room around them.
fn gutter_width(lines: usize, cell_w: f32) -> f32 {
    lines.to_string().len() as f32 * cell_w + 16.
}

/// Width of the text area inside an editor `editor_w` wide, past the gutter and change bar.
fn text_width(editor_w: f32, cell_w: f32, lines: usize) -> f32 {
    editor_w - gutter_width(lines, cell_w) - BAR_W
}

/// The furthest the text can scroll sideways, in pixels: the widest line less the text area.
fn max_h_offset(file: &OpenFile, cell_w: f32, editor_w: f32) -> f32 {
    let widest = (0..file.buffer.line_count())
        .map(|i| {
            let line = file.buffer.line(i);
            visual_col(&line, line.chars().count())
        })
        .max()
        .unwrap_or(0);
    (widest as f32 * cell_w - text_width(editor_w, cell_w, file.buffer.line_count())).max(0.)
}

/// The position in `file` under `point`, where `origin` is the top-left corner of the editor.
fn pos_at(file: &OpenFile, point: Point<Pixels>, origin: Point<Pixels>, cell_w: f32) -> Pos {
    let scrolled = -f32::from(file.scroll.0.borrow().base_handle.offset().y);
    let y = f32::from(point.y - origin.y) + scrolled;
    let line = ((y / LINE_H).floor().max(0.) as usize).min(file.buffer.line_count() - 1);
    let gutter = gutter_width(file.buffer.line_count(), cell_w) + BAR_W;
    let x = f32::from(point.x - origin.x) - gutter + file.h_offset;
    let vcol = (x / cell_w).round().max(0.) as usize;
    Pos { line, col: char_col(&file.buffer.line(line), vcol) }
}

/// The chars of `line` that a selection covers, as a char range, and whether the selection goes on
/// past the line's end, so its line break is selected too.
fn selected_cols(selection: Option<(Pos, Pos)>, line: usize, len: usize) -> (Option<Range<usize>>, bool) {
    let Some((start, end)) = selection else { return (None, false) };
    if line < start.line || line > end.line {
        return (None, false);
    }
    let from = if line == start.line { start.col } else { 0 };
    let to = if line == end.line { end.col } else { len };
    (Some(from..to), line < end.line)
}

/// A row's text, with tabs expanded to spaces, and its runs. Each char takes the style of the span
/// covering it, and selected chars get the selection colour. `continues` selects one extra cell for
/// the line break.
fn row_text(text: &str, spans: &[Span], selected: Option<Range<usize>>, continues: bool) -> (String, Vec<TextRun>) {
    let mut cells: Vec<(String, Style, bool)> = Vec::new();
    let mut span = 0;
    let mut vcol = 0;
    for (col, (byte, c)) in text.char_indices().enumerate() {
        while span < spans.len() && spans[span].0.end <= byte {
            span += 1;
        }
        let style = match spans.get(span) {
            Some((range, style)) if range.start <= byte => *style,
            _ => PLAIN,
        };
        let cell = if c == '\t' { " ".repeat(TAB_WIDTH - vcol % TAB_WIDTH) } else { c.to_string() };
        vcol += cell.chars().count();
        let is_selected = selected.as_ref().is_some_and(|range| range.contains(&col));
        cells.push((cell, style, is_selected));
    }
    if continues {
        cells.push((" ".to_string(), PLAIN, true));
    }
    let mut display = String::new();
    let mut runs: Vec<TextRun> = Vec::new();
    let mut last = None;
    for (cell, style, is_selected) in cells {
        display.push_str(&cell);
        if last != Some((style, is_selected)) {
            runs.push(text_run(style, is_selected));
            last = Some((style, is_selected));
        }
        if let Some(run) = runs.last_mut() {
            run.len += cell.len();
        }
    }
    (display, runs)
}

/// The text style of a run: its colour, bold and italic, and the selection background when selected.
fn text_run(style: Style, selected: bool) -> TextRun {
    TextRun {
        len: 0,
        font: Font {
            weight: if style.bold { FontWeight::BOLD } else { FontWeight::NORMAL },
            style: if style.italic { FontStyle::Italic } else { FontStyle::Normal },
            ..font(FONT_FAMILY)
        },
        color: rgb(style.fg).into(),
        background_color: selected.then(|| rgb(SELECTION).into()),
        underline: None,
        strikethrough: None,
    }
}

/// The display column where char `col` of `line` starts. Tabs advance to the next tab stop.
fn visual_col(line: &str, col: usize) -> usize {
    line.chars().take(col).fold(0, |vcol, c| vcol + char_width(c, vcol))
}

/// The char of `line` shown at display column `vcol`, or the line's length past its end. A column
/// inside a tab gives that tab.
fn char_col(line: &str, vcol: usize) -> usize {
    let mut start = 0;
    for (col, c) in line.chars().enumerate() {
        let end = start + char_width(c, start);
        if vcol < end {
            return col;
        }
        start = end;
    }
    line.chars().count()
}

/// Columns that `c` takes when it starts at display column `vcol`.
fn char_width(c: char, vcol: usize) -> usize {
    if c == '\t' {
        TAB_WIDTH - vcol % TAB_WIDTH
    } else {
        1
    }
}

#[cfg(test)]
mod tests {
    use super::{char_col, visual_col};

    #[test]
    fn tabs_advance_to_the_next_stop() {
        assert_eq!(visual_col("\tx", 1), 4);
        assert_eq!(visual_col("ab\tx", 3), 4);
        assert_eq!(visual_col("abcd\tx", 5), 8);
        assert_eq!(visual_col("a\t\tb", 3), 8);
    }

    #[test]
    fn multibyte_chars_take_one_column() {
        assert_eq!(visual_col("héllo", 2), 2);
        assert_eq!(visual_col("ñ\tb", 2), 4);
        assert_eq!(char_col("ñ\tb", 4), 2);
    }

    #[test]
    fn char_col_inverts_visual_col() {
        for line in ["\tfoo\tbar", "héllo wörld", "a\t日b", ""] {
            for col in 0..=line.chars().count() {
                assert_eq!(char_col(line, visual_col(line, col)), col, "{line:?} at char {col}");
            }
        }
    }

    #[test]
    fn char_col_inside_a_tab_is_the_tab() {
        assert_eq!(char_col("a\tb", 2), 1);
        assert_eq!(char_col("a\tb", 4), 2);
        assert_eq!(char_col("ab", 99), 2);
    }
}
