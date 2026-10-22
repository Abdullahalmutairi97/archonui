//! The Ide tab: open files as tabs, a code editor with syntax colours and changed-line bands, a breadcrumb,
//! a status bar and the Follow agent toggle. Spec: docs/design/tools-and-behaviour.md §A2.

use super::{
    browser::WebView,
    files::{file_match_row, no_project, search, FolderWatch, SEARCH},
};
use crate::{
    app::workspace::RightTab,
    editor::{
        buffer::{Buffer, Pos},
        diff::{line_marks, Mark},
        highlight::Highlighter,
    },
    ide::{find_code_server, open_in_window, IdeServer, COOKIE, NOT_INSTALLED},
    store::IdeKind,
    ui::{
        composer::icons::stroke_icon,
        shell::Shell,
        stream::{
            code::{tokenize, Tok},
            token_colour,
        },
        theme::Theme,
        widgets::{
            button::{Button, ButtonStyle},
            popover::{tooltip, TooltipColors},
            text_input::{text_input, TextInput, TextInputConfig, TextInputEvent},
            toggle::Toggle,
        },
    },
    workspace::{self, Change, ChangeKind},
};
use gpui::{
    anchored, deferred, div, font, hsla, point, prelude::*, AnyElement, App, BoxShadow, ClickEvent, ClipboardItem,
    Context, Corner, Div, Entity, FocusHandle, FontWeight, Hsla, KeyDownEvent, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Pixels, Point, ScrollHandle, ScrollWheelEvent, SharedString, StyledText,
    Subscription, Task, TextRun, Window,
};
use crate::ui::scale::{design, px};
use std::{
    collections::HashMap,
    io,
    ops::Range,
    path::{Path, PathBuf},
};

/// Height of one code row: 1.5 × the code size, tighter than the design's 21.25px.
const LINE_H: f32 = 17.5;
/// Padding above and below the code rows.
const PAD_Y: f32 = 6.;
/// Width of the line-number gutter.
const GUTTER_W: f32 = 34.;
/// Space between the gutter and the code.
const GAP: f32 = 14.;
/// Where the code starts, from the code area's left edge.
const TEXT_X: f32 = GUTTER_W + GAP;
const CODE_SIZE: f32 = 11.5;
/// Columns a tab is drawn as.
const TAB_WIDTH: usize = 4;
/// Rows the tab keeps in its AGENT CHANGES list.
const MAX_CHANGES: usize = 200;
/// Rows and columns assumed before the code area has been laid out.
const FALLBACK_LINES: usize = 40;
const FALLBACK_COLS: usize = 120;

/// View state of the Ide tab, not persisted. The open files are its tabs.
#[derive(Default)]
pub struct IdeUi {
    docs: Vec<Doc>,
    active: usize,
    /// Focus of the code area. Keys reach the editor only while it has focus.
    focus: Option<FocusHandle>,
    /// Focuses the code area on the next frame, for changes made outside render.
    want_focus: bool,
    /// Opens the files the agent changes in the tool folder.
    follow: bool,
    watch: Option<FolderWatch>,
    /// The code area's layout from the last frame: its bounds give the visible rows and columns.
    viewport: ScrollHandle,
    /// Width of one code column, from the code font.
    cell: f32,
    /// A mouse drag that started in the code area.
    drag: bool,
    /// code-server for VS Code mode.
    vscode: VsCode,
    /// The tab whose "Save changes?" bar is showing.
    closing: Option<usize>,
    /// The files the agent changed in the tool folder this session, in the order they were first seen.
    pub(super) changes: Vec<(PathBuf, ChangeKind)>,
    /// Whether the AGENT CHANGES section of the Files tab is folded away.
    pub(super) changes_collapsed: bool,
    /// The Open file… picker.
    picker: Option<Picker>,
}

/// The Open file… picker: a filter field and the files under the tool folder that match it.
struct Picker {
    input: Entity<TextInput>,
    _subs: Vec<Subscription>,
    root: PathBuf,
    matches: Vec<PathBuf>,
}

/// The code-server behind VS Code mode. It starts on first use and is stopped when the mode is left or it exits.
#[derive(Default)]
pub struct VsCode {
    server: Option<IdeServer>,
    /// The session cookie code-server issued at login.
    cookie: String,
    starting: bool,
    /// Why code-server isn't running. `NOT_INSTALLED` offers the built-in editor instead.
    error: Option<String>,
    task: Option<Task<()>>,
    /// The page code-server shows, and the folder it was last loaded for.
    view: WebView,
    folder: Option<PathBuf>,
    /// The file to open in the window once code-server runs. Taken when it is sent.
    open: Option<PathBuf>,
    /// Sends `open`. Kept apart from `task`, which runs code-server itself.
    open_task: Option<Task<()>>,
}

impl IdeUi {
    /// The whole rows that fit the code area.
    fn visible_lines(&self) -> usize {
        let height = design(self.viewport.bounds().size.height);
        if height <= 0. {
            FALLBACK_LINES
        } else {
            ((height - 2. * PAD_Y) / LINE_H).floor().max(1.) as usize
        }
    }

    /// The whole columns of code that fit beside the gutter.
    fn visible_cols(&self) -> usize {
        let width = design(self.viewport.bounds().size.width);
        if width <= 0. || self.cell <= 0. {
            FALLBACK_COLS
        } else {
            ((width - TEXT_X) / self.cell).floor().max(1.) as usize
        }
    }
}

/// An open file. Its baseline is the text it had when opened or last saved; changed lines are measured
/// against it.
struct Doc {
    path: PathBuf,
    buffer: Buffer,
    highlighter: Highlighter,
    saved: String,
    /// Edits made so far. `marks` is recomputed when this has moved past `marks_at`.
    edits: u64,
    marks: Vec<(usize, Mark)>,
    marks_at: u64,
    /// The first line and column shown.
    top: usize,
    left: usize,
    /// Wheel distance not yet applied, in lines and in columns.
    wheel_y: f32,
    wheel_x: f32,
    /// The file was removed on disk.
    deleted: bool,
    /// The file changed on disk while the buffer had unsaved edits, so it was not reloaded.
    changed_on_disk: bool,
}

impl Doc {
    fn open(path: &Path) -> io::Result<Self> {
        let buffer = Buffer::open(path)?;
        let saved = buffer.text();
        let highlighter = Highlighter::new(Some(path), &buffer.line(0));
        Ok(Self {
            path: path.to_path_buf(),
            buffer,
            highlighter,
            saved,
            edits: 0,
            marks: Vec::new(),
            marks_at: 0,
            top: 0,
            left: 0,
            wheel_y: 0.,
            wheel_x: 0.,
            deleted: false,
            changed_on_disk: false,
        })
    }

    fn name(&self) -> String {
        self.path.file_name().map_or_else(|| self.path.display().to_string(), |n| n.to_string_lossy().into_owned())
    }

    /// Passes the last buffer edit to the highlighter and counts it.
    fn sync(&mut self) {
        if let Some((first, removed, inserted)) = self.buffer.take_edit() {
            self.highlighter.edited(first, removed, inserted);
            self.edits += 1;
        }
    }

    /// The changed lines against the baseline, recomputed after edits.
    fn marks(&mut self) -> &[(usize, Mark)] {
        if self.marks_at != self.edits {
            self.marks = line_marks(&self.saved, &self.buffer.text());
            self.marks_at = self.edits;
        }
        &self.marks
    }

    /// Replaces the buffer with the file's text on disk. The baseline stays as it was.
    fn reload(&mut self, text: &str) {
        self.buffer.reload_from(text);
        self.highlighter.invalidate(0, self.buffer.line_count());
        self.edits += 1;
        self.deleted = false;
        self.changed_on_disk = false;
    }
}

/// The active tab and the tab asking "Save changes?", by path, so they still point at the same files
/// after other tabs are dropped.
struct TabMarks {
    active: Option<PathBuf>,
    closing: Option<PathBuf>,
}

impl TabMarks {
    fn take(ide: &IdeUi) -> Self {
        let path = |ix: usize| ide.docs.get(ix).map(|doc| doc.path.clone());
        Self { active: path(ide.active), closing: ide.closing.and_then(path) }
    }

    /// Points `active` and `closing` back at their files, whose paths are now `moved(path)`. A closed
    /// active tab leaves the one at its place active; a closed asking tab ends the question.
    fn restore(self, ide: &mut IdeUi, moved: impl Fn(&Path) -> PathBuf) {
        let find = |path: Option<PathBuf>| {
            let path = moved(&path?);
            ide.docs.iter().position(|doc| doc.path == path)
        };
        let active = find(self.active);
        ide.closing = find(self.closing);
        ide.active = active.unwrap_or(ide.active).min(ide.docs.len().saturating_sub(1));
    }
}

/// How code rows are drawn: the theme, one column's width, and the first column shown.
struct RowStyle<'a> {
    theme: &'a Theme,
    cell: f32,
    left: usize,
}

impl Shell {
    pub(in crate::ui) fn render_ide(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = self.theme.clone();
        if self.state.settings.ide == IdeKind::VsCode {
            return self.ide_vscode_view(&theme, cx);
        }
        let Some(root) = self.tool_root() else {
            return no_project(&theme);
        };
        let focus = self.tabs.ide.focus.get_or_insert_with(|| cx.focus_handle()).clone();
        if std::mem::take(&mut self.tabs.ide.want_focus) {
            window.focus(&focus);
        }
        self.tabs.ide.cell = cell_width(window, &theme.code_font);
        if self.tabs.ide.docs.is_empty() {
            let tabs = self.ide_tab_strip(&theme, cx);
            let picker = self.ide_picker_view(&theme, cx);
            return div()
                .relative()
                .size_full()
                .min_h_0()
                .flex()
                .flex_col()
                .font_family(theme.ui_font.clone())
                .text_color(theme.fg.to_hsla())
                .child(tabs)
                .child(placeholder("Open a file from Files or click Open file… to edit it here.", &theme))
                .children(picker)
                .into_any_element();
        }
        self.tabs.ide.active = self.tabs.ide.active.min(self.tabs.ide.docs.len() - 1);
        let branch = self.tabs.git.snapshot.as_ref().and_then(|snap| snap.status.branch.clone());
        let focused = focus.is_focused(window);
        let tabs = self.ide_tab_strip(&theme, cx);
        let prompt = self.ide_prompt(&theme, cx);
        let picker = self.ide_picker_view(&theme, cx);
        let crumb = self.ide_breadcrumb(&root, &theme, cx);
        let code = self.ide_code(focused, &focus, &theme, cx);
        let status = self.ide_status(branch.as_deref(), &theme);
        div()
            .relative()
            .size_full()
            .min_h_0()
            .flex()
            .flex_col()
            .font_family(theme.ui_font.clone())
            .text_color(theme.fg.to_hsla())
            .child(tabs)
            .children(prompt)
            .child(crumb)
            .child(code)
            .child(status)
            .children(picker)
            .into_any_element()
    }

    pub(in crate::ui) fn ide_key(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if ev.keystroke.key == "escape" {
            if self.tabs.ide.picker.is_some() {
                self.ide_picker_close(cx);
                return true;
            }
            if self.tabs.ide.closing.is_some() {
                self.ide_cancel_close(cx);
                return true;
            }
        }
        if self.state.settings.ide == IdeKind::VsCode {
            return self.web_key(ide_view, ev, window, cx);
        }
        let key = ev.keystroke.key.as_str();
        let mods = ev.keystroke.modifiers;
        let cmd = mods.control || mods.platform;
        if cmd && key == "p" && !mods.shift {
            self.ide_picker_open(window, cx);
            return true;
        }
        let focused = self.tabs.ide.focus.as_ref().is_some_and(|focus| focus.is_focused(window));
        if !focused || self.tabs.ide.docs.is_empty() {
            return false;
        }
        let select = mods.shift;
        if cmd {
            return match (key, select) {
                ("s", false) => {
                    self.ide_save(cx);
                    true
                }
                ("w", false) => {
                    let ix = self.tabs.ide.active;
                    self.ide_close(ix, cx);
                    true
                }
                ("a", false) => self.ide_act(cx, |doc| doc.buffer.select_all()),
                ("c", false) => {
                    self.ide_copy(cx);
                    true
                }
                ("x", false) => {
                    self.ide_cut(cx);
                    true
                }
                ("v", false) => match cx.read_from_clipboard().and_then(|item| item.text()) {
                    Some(text) => self.ide_act(cx, |doc| doc.buffer.paste(&text)),
                    None => false,
                },
                ("z", false) => self.ide_act(cx, |doc| {
                    doc.buffer.undo();
                }),
                ("z", true) | ("y", false) => self.ide_act(cx, |doc| {
                    doc.buffer.redo();
                }),
                ("left", _) => self.ide_act(cx, |doc| doc.buffer.move_word_left(select)),
                ("right", _) => self.ide_act(cx, |doc| doc.buffer.move_word_right(select)),
                ("home", _) => self.ide_act(cx, |doc| doc.buffer.move_doc_start(select)),
                ("end", _) => self.ide_act(cx, |doc| doc.buffer.move_doc_end(select)),
                _ => false,
            };
        }
        let visible = self.tabs.ide.visible_lines();
        match key {
            "escape" => {
                window.focus(&self.focus);
                true
            }
            "left" => self.ide_act(cx, |doc| doc.buffer.move_left(select)),
            "right" => self.ide_act(cx, |doc| doc.buffer.move_right(select)),
            "up" => self.ide_act(cx, |doc| doc.buffer.move_up(select)),
            "down" => self.ide_act(cx, |doc| doc.buffer.move_down(select)),
            "home" => self.ide_act(cx, |doc| doc.buffer.move_home(select)),
            "end" => self.ide_act(cx, |doc| doc.buffer.move_end(select)),
            "pageup" => self.ide_act(cx, |doc| doc.buffer.page_up(visible, select)),
            "pagedown" => self.ide_act(cx, |doc| doc.buffer.page_down(visible, select)),
            "enter" => self.ide_act(cx, |doc| doc.buffer.newline()),
            "backspace" => self.ide_act(cx, |doc| doc.buffer.backspace()),
            "delete" => self.ide_act(cx, |doc| doc.buffer.delete()),
            "tab" if select => self.ide_act(cx, |doc| doc.buffer.outdent()),
            "tab" => self.ide_act(cx, |doc| doc.buffer.indent()),
            _ => match ev.keystroke.key_char.clone() {
                Some(text) if !text.is_empty() => self.ide_act(cx, |doc| doc.buffer.insert(&text)),
                _ => false,
            },
        }
    }

    pub(in crate::ui) fn ide_before_render(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let visible = self.ide_visible();
        if self.state.settings.ide == IdeKind::VsCode {
            if visible || self.state.settings.ide_autostart {
                self.ide_vscode_start(cx);
            }
            let folder = self.tool_root();
            if self.tabs.ide.vscode.server.is_some() && folder.is_some() && self.tabs.ide.vscode.folder != folder {
                self.ide_vscode_load(cx);
            }
        }
        self.tabs.ide.vscode.view.sync(visible, window);
        // No project folder in focus: nothing to watch until one is.
        let Some(root) = self.tool_root() else { return };
        if self.tabs.ide.watch.as_ref().is_some_and(|watch| watch.root == root) {
            return;
        }
        // A new tool folder starts a new change list and drops a picker that listed the old one.
        self.tabs.ide.changes.clear();
        self.tabs.ide.picker = None;
        self.tabs.ide.watch = Some(FolderWatch::start(&root, cx, Self::ide_change));
    }

    /// Starts code-server in the background, unless it runs, is starting, or its last start failed.
    fn ide_vscode_start(&mut self, cx: &mut Context<Self>) {
        let vscode = &self.tabs.ide.vscode;
        if vscode.server.is_some() || vscode.starting || vscode.error.is_some() {
            return;
        }
        let configured = self.state.settings.code_server_path.clone();
        let Some(program) = find_code_server(configured.as_deref()) else {
            self.tabs.ide.vscode.error = Some(NOT_INSTALLED.into());
            return;
        };
        let vscode = &mut self.tabs.ide.vscode;
        vscode.starting = true;
        vscode.task = Some(cx.spawn(async move |this, cx| {
            let started = cx
                .background_executor()
                .spawn(async move {
                    let server = IdeServer::start(&program)?;
                    let cookie = server.login_cookie()?;
                    Ok::<_, String>((server, cookie))
                })
                .await;
            let exited = this
                .update(cx, |this, cx| {
                    let vscode = &mut this.tabs.ide.vscode;
                    vscode.starting = false;
                    let exited = match started {
                        Ok((server, cookie)) => {
                            let exited = server.exited();
                            vscode.server = Some(server);
                            vscode.cookie = cookie;
                            Some(exited)
                        }
                        Err(e) => {
                            vscode.error = Some(e);
                            None
                        }
                    };
                    this.ide_vscode_load(cx);
                    cx.notify();
                    exited
                })
                .ok()
                .flatten();
            let Some(exited) = exited else { return };
            if let Ok(code) = exited.recv().await {
                this.update(cx, |this, cx| this.ide_vscode_exited(code, cx)).ok();
            }
        }));
    }

    /// Forgets code-server after it exited on its own, and shows why, with Retry.
    fn ide_vscode_exited(&mut self, code: Option<i32>, cx: &mut Context<Self>) {
        self.ide_vscode_stop(cx);
        self.tabs.ide.vscode.error = Some(match code {
            Some(code) => format!("VS Code stopped (exit code {code})."),
            None => "VS Code stopped (killed by a signal).".into(),
        });
        cx.notify();
    }

    /// Stops code-server and its page, and forgets the folder and file they showed.
    fn ide_vscode_stop(&mut self, cx: &mut Context<Self>) {
        let vscode = &mut self.tabs.ide.vscode;
        vscode.task = None;
        vscode.starting = false;
        vscode.server = None;
        vscode.cookie.clear();
        vscode.folder = None;
        vscode.open = None;
        vscode.open_task = None;
        vscode.error = None;
        vscode.view.stop(cx);
    }

    /// Loads the tool folder in code-server's page unless it shows it, then opens the file waiting for the window.
    /// Does nothing until code-server runs.
    fn ide_vscode_load(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.tool_root() else { return };
        let vscode = &mut self.tabs.ide.vscode;
        let Some(server) = &vscode.server else { return };
        if vscode.folder.as_ref() != Some(&root) {
            let url = server.url_for(&root);
            let cookie = (server.base_url(), COOKIE.to_string(), vscode.cookie.clone());
            vscode.folder = Some(root);
            vscode.view.navigate_with_cookie(url, cookie, ide_view, cx);
        }
        if let Some(file) = vscode.open.take() {
            let program = server.program().to_path_buf();
            vscode.open_task = Some(cx.spawn(async move |this, cx| {
                let opened = cx.background_executor().spawn(async move { open_in_window(&program, &file) }).await;
                if let Err(error) = opened {
                    this.update(cx, |this, cx| this.toast(error, cx)).ok();
                }
            }));
        }
    }

    /// Switches the IDE setting to the built-in editor and saves it.
    fn ide_use_builtin(&mut self, cx: &mut Context<Self>) {
        self.ide_vscode_stop(cx);
        self.state.settings.ide = IdeKind::Builtin;
        self.save(cx);
    }

    /// Whether the IDE tab is shown in VS Code mode, so its page should stream frames.
    fn ide_visible(&self) -> bool {
        let ws = &self.state.workspace;
        ws.right_open && ws.right_tab == RightTab::Ide && self.state.settings.ide == IdeKind::VsCode && self.focused_remote().is_none()
    }

    /// VS Code mode: the page once code-server runs, else its starting or failed state.
    fn ide_vscode_view(&mut self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let vscode = &self.tabs.ide.vscode;
        if let Some(error) = vscode.error.clone() {
            return self.ide_vscode_error(&error, theme, cx);
        }
        if vscode.server.is_none() {
            return placeholder("Starting VS Code…", theme);
        }
        let page_error = vscode.view.error.clone();
        let page = self.web_page("ide-vscode-page", ide_view, "Opening VS Code…", cx);
        match page_error {
            Some(error) => div()
                .size_full()
                .min_h_0()
                .flex()
                .flex_col()
                .child(
                    div()
                        .mx(px(12.))
                        .mt(px(8.))
                        .px(px(10.))
                        .py(px(6.))
                        .rounded(px(6.))
                        .bg(theme.deep.to_hsla())
                        .text_xs()
                        // `on` is for text on the bright accent; on `deep` it is dark on dark, so the body colour reads.
                        .text_color(theme.fg.to_hsla())
                        .child(error),
                )
                .child(page)
                .into_any_element(),
            None => page,
        }
    }

    /// code-server isn't running: the message, with Use built-in editor when it isn't installed, else Retry.
    fn ide_vscode_error(&mut self, error: &str, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let shell = cx.entity();
        let mut body = div()
            .flex()
            .flex_col()
            .items_start()
            .gap(px(10.))
            .p(px(14.))
            .text_size(px(12.5))
            .text_color(theme.muted.to_hsla())
            .child(error.to_string());
        if error == NOT_INSTALLED {
            body = body.child(Button::new("ide-use-builtin", "Use built-in editor", theme).style(ButtonStyle::Filled).on_click({
                let shell = shell.clone();
                move |_, cx| shell.update(cx, |this, cx| this.ide_use_builtin(cx))
            }));
        } else {
            body = body.child(Button::new("ide-vscode-retry", "Retry", theme).style(ButtonStyle::Outline).on_click({
                let shell = shell.clone();
                move |_, cx| {
                    shell.update(cx, |this, cx| {
                        this.tabs.ide.vscode.error = None;
                        cx.notify();
                    })
                }
            }));
        }
        body.into_any_element()
    }

    pub(in crate::ui) fn ide_blur(&mut self, _cx: &mut Context<Self>) {
        self.tabs.ide.drag = false;
        self.tabs.ide.vscode.view.cancel_press();
    }

    /// Opens `path` in the Ide tab, shows the tab and focuses the code area. Opening a file that is
    /// already open activates its tab.
    pub(in crate::ui) fn ide_open(&mut self, path: &Path, cx: &mut Context<Self>) {
        let Some(root) = self.tool_root() else {
            self.toast("Pick a project to open its files.", cx);
            return;
        };
        // Only files inside the project folder open; a symlink out of it is followed and refused here.
        let path = match workspace::resolve(&root, path) {
            Ok(real) => real,
            Err(e) => {
                self.toast(format!("Could not open {}: {e}", path.display()), cx);
                return;
            }
        };
        let path = path.as_path();
        if self.state.settings.ide == IdeKind::VsCode {
            self.show_tool_tab(RightTab::Ide, cx);
            self.tabs.ide.vscode.open = Some(path.to_path_buf());
            self.ide_vscode_start(cx);
            self.ide_vscode_load(cx);
            cx.notify();
            return;
        }
        let existing = self.tabs.ide.docs.iter().position(|doc| doc.path == path);
        let ix = match existing {
            Some(ix) => ix,
            None => match Doc::open(path) {
                Ok(doc) => {
                    self.tabs.ide.docs.push(doc);
                    self.tabs.ide.docs.len() - 1
                }
                Err(e) => {
                    let name = path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
                    self.toast(format!("Could not open {name}: {e}"), cx);
                    return;
                }
            },
        };
        self.show_tool_tab(RightTab::Ide, cx);
        self.tabs.ide.active = ix;
        self.tabs.ide.want_focus = true;
        cx.notify();
    }

    /// For screenshots (`ARCHONUI_OPEN=ide:<path>`): opens `path` in the built-in editor without a
    /// project and scrolls it so `top` is the first row.
    pub(in crate::ui) fn ide_dev_open(&mut self, path: &Path, top: usize, cx: &mut Context<Self>) {
        let Ok(mut doc) = Doc::open(path) else { return };
        doc.top = top;
        self.tabs.ide.docs.push(doc);
        self.tabs.ide.active = self.tabs.ide.docs.len() - 1;
        self.show_tool_tab(RightTab::Ide, cx);
        cx.notify();
    }

    /// Whether an open file under `path` has unsaved edits.
    pub(super) fn ide_dirty_under(&self, path: &Path) -> bool {
        self.tabs.ide.docs.iter().any(|doc| doc.path.starts_with(path) && doc.buffer.is_dirty())
    }

    /// Points the open files under `old` at `new` after a rename. A file that no longer opens is closed.
    pub(super) fn ide_moved(&mut self, old: &Path, new: &Path) {
        let ide = &mut self.tabs.ide;
        let marks = TabMarks::take(ide);
        let docs = std::mem::take(&mut ide.docs);
        ide.docs = docs
            .into_iter()
            .filter_map(|doc| match doc.path.strip_prefix(old) {
                Ok(rest) => Doc::open(&new.join(rest)).ok(),
                Err(_) => Some(doc),
            })
            .collect();
        marks.restore(ide, |path| path.strip_prefix(old).map_or_else(|_| path.to_path_buf(), |rest| new.join(rest)));
    }

    /// Closes the open files under `path`, after they were deleted or renamed.
    pub(super) fn ide_closed_under(&mut self, path: &Path) {
        let ide = &mut self.tabs.ide;
        let marks = TabMarks::take(ide);
        ide.docs.retain(|doc| !doc.path.starts_with(path));
        marks.restore(ide, Path::to_path_buf);
    }

    /// Applies one change from the tool folder's watcher: records it in the agent-changes list, reloads a
    /// clean open file, flags a dirty one, and with Follow agent on opens a file the agent creates or
    /// changes.
    fn ide_change(&mut self, change: Change, cx: &mut Context<Self>) {
        let root = self.tool_root().unwrap_or_default();
        let ide = &mut self.tabs.ide;
        if !is_scratch(&change.path) {
            record_change(&mut ide.changes, &change.path, change.kind);
        }
        match ide.docs.iter().position(|doc| doc.path == change.path) {
            Some(ix) => {
                let doc = &mut ide.docs[ix];
                match change.kind {
                    ChangeKind::Removed => doc.deleted = true,
                    _ if doc.buffer.is_dirty() => doc.changed_on_disk = true,
                    _ => {
                        if let Ok(text) = workspace::read_to_string(&root, &doc.path) {
                            doc.reload(&text);
                        }
                    }
                }
            }
            None if ide.follow && change.kind != ChangeKind::Removed => {
                if let Some(doc) = workspace::resolve(&root, &change.path).ok().and_then(|real| Doc::open(&real).ok()) {
                    ide.docs.push(doc);
                    ide.active = ide.docs.len() - 1;
                }
            }
            None => {}
        }
        cx.notify();
    }

    fn ide_select(&mut self, ix: usize, cx: &mut Context<Self>) {
        self.tabs.ide.active = ix;
        self.tabs.ide.want_focus = true;
        cx.notify();
    }

    /// Closes the tab at `ix`. A tab with unsaved edits asks Save / Don't save / Cancel first.
    fn ide_close(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(doc) = self.tabs.ide.docs.get(ix) else { return };
        if doc.buffer.is_dirty() {
            self.tabs.ide.closing = Some(ix);
            cx.notify();
            return;
        }
        self.ide_remove(ix, cx);
    }

    /// Drops the tab at `ix`, keeping the active tab in range.
    fn ide_remove(&mut self, ix: usize, cx: &mut Context<Self>) {
        let ide = &mut self.tabs.ide;
        if ix >= ide.docs.len() {
            return;
        }
        ide.docs.remove(ix);
        ide.closing = None;
        if ide.active > ix {
            ide.active -= 1;
        }
        ide.active = ide.active.min(ide.docs.len().saturating_sub(1));
        cx.notify();
    }

    /// Ends the "Save changes?" bar: closes the tab, saving it first when `save`. A save that fails keeps
    /// the tab open.
    fn ide_finish_close(&mut self, ix: usize, save: bool, cx: &mut Context<Self>) {
        if !save {
            self.ide_remove(ix, cx);
            return;
        }
        if self.ide_save_doc(ix, cx) {
            self.ide_remove(ix, cx);
        } else {
            self.tabs.ide.closing = None;
            cx.notify();
        }
    }

    fn ide_cancel_close(&mut self, cx: &mut Context<Self>) {
        self.tabs.ide.closing = None;
        cx.notify();
    }

    /// Replaces tab `ix` with the file on disk, dropping its unsaved edits.
    fn ide_reload(&mut self, ix: usize, cx: &mut Context<Self>) {
        let root = self.tool_root().unwrap_or_default();
        let Some(doc) = self.tabs.ide.docs.get_mut(ix) else { return };
        let failed = match workspace::read_to_string(&root, &doc.path) {
            Ok(text) => {
                doc.reload(&text);
                None
            }
            Err(e) => Some(format!("Could not read {}: {e}", doc.name())),
        };
        if let Some(msg) = failed {
            self.toast(msg, cx);
        }
        cx.notify();
    }

    /// Keeps tab `ix`'s unsaved edits over the file that changed on disk.
    fn ide_keep_mine(&mut self, ix: usize, cx: &mut Context<Self>) {
        if let Some(doc) = self.tabs.ide.docs.get_mut(ix) {
            doc.changed_on_disk = false;
        }
        cx.notify();
    }

    fn ide_save(&mut self, cx: &mut Context<Self>) {
        let ix = self.tabs.ide.active;
        self.ide_save_doc(ix, cx);
    }

    /// Saves tab `ix`, telling the watcher to ignore the write. False when the write failed, which is
    /// kept on screen as a toast.
    fn ide_save_doc(&mut self, ix: usize, cx: &mut Context<Self>) -> bool {
        let Some(path) = self.tabs.ide.docs.get(ix).map(|d| d.path.clone()) else { return false };
        let root = self.tool_root().unwrap_or_default();
        if let Err(e) = workspace::resolve(&root, &path) {
            self.toast(format!("Could not save {}: {e}", path.display()), cx);
            return false;
        }
        if let Some(watch) = &self.tabs.ide.watch {
            watch.ignore_own_write(&path);
        }
        if let Some(watch) = &self.tabs.files.watch {
            watch.ignore_own_write(&path);
        }
        let doc = &mut self.tabs.ide.docs[ix];
        let failed = match doc.buffer.save() {
            Ok(()) => {
                doc.saved = doc.buffer.text();
                doc.marks_at = u64::MAX;
                doc.deleted = false;
                doc.changed_on_disk = false;
                None
            }
            Err(e) => Some(format!("Could not save {}: {e}", doc.name())),
        };
        if let Some(msg) = failed {
            self.toast(msg, cx);
            return false;
        }
        cx.notify();
        true
    }

    fn ide_copy(&mut self, cx: &mut Context<Self>) {
        let text = self.tabs.ide.docs.get(self.tabs.ide.active).and_then(|doc| doc.buffer.copy());
        if let Some(text) = text {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    fn ide_cut(&mut self, cx: &mut Context<Self>) {
        let mut cut = None;
        self.ide_act(cx, |doc| cut = doc.buffer.cut());
        if let Some(text) = cut {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    /// Runs `act` on the active file, then keeps the cursor in view. Returns true: the key was used.
    fn ide_act(&mut self, cx: &mut Context<Self>, act: impl FnOnce(&mut Doc)) -> bool {
        let Some(doc) = self.tabs.ide.docs.get_mut(self.tabs.ide.active) else { return false };
        act(doc);
        doc.sync();
        self.ide_reveal();
        cx.notify();
        true
    }

    /// Scrolls the active file so its cursor is inside the code area.
    fn ide_reveal(&mut self) {
        let visible_lines = self.tabs.ide.visible_lines();
        let visible_cols = self.tabs.ide.visible_cols();
        let Some(doc) = self.tabs.ide.docs.get_mut(self.tabs.ide.active) else { return };
        let cursor = doc.buffer.cursor();
        let col = visual_col(&doc.buffer.line(cursor.line), cursor.col);
        if cursor.line < doc.top {
            doc.top = cursor.line;
        } else if cursor.line >= doc.top + visible_lines {
            doc.top = cursor.line + 1 - visible_lines;
        }
        if col < doc.left {
            doc.left = col;
        } else if col >= doc.left + visible_cols {
            doc.left = col + 1 - visible_cols;
        }
    }

    /// Moves the caret or extends the selection to the pointer. A double click selects a word.
    fn ide_pointer(&mut self, at: Point<Pixels>, clicks: usize, select: bool, cx: &mut Context<Self>) {
        let ide = &mut self.tabs.ide;
        let origin = ide.viewport.bounds().origin;
        let cell = ide.cell.max(1.);
        let Some(doc) = ide.docs.get_mut(ide.active) else { return };
        let count = doc.buffer.line_count();
        // The first row drawn is `doc.top`, so a click's row is offset by the scroll.
        let line = doc.top + ((design(at.y - origin.y) - PAD_Y) / LINE_H).floor().max(0.) as usize;
        let line = line.min(count - 1);
        let raw = doc.buffer.line(line);
        let vcol = doc.left + ((design(at.x - origin.x) - TEXT_X) / cell).round().max(0.) as usize;
        let pos = Pos { line, col: char_col(&raw, vcol) };
        if clicks >= 2 {
            doc.buffer.select_word_at(pos);
        } else {
            doc.buffer.set_cursor(pos, select);
        }
        cx.notify();
    }

    /// Scrolls the active file by the wheel's distance, in whole rows and columns.
    fn ide_wheel(&mut self, ev: &ScrollWheelEvent, cx: &mut Context<Self>) {
        let delta = ev.delta.pixel_delta(px(LINE_H));
        let visible = self.tabs.ide.visible_lines();
        let cell = self.tabs.ide.cell.max(1.);
        let Some(doc) = self.tabs.ide.docs.get_mut(self.tabs.ide.active) else { return };
        let max_top = doc.buffer.line_count().saturating_sub(visible);
        doc.wheel_y -= design(delta.y) / LINE_H;
        let rows = doc.wheel_y.trunc();
        doc.wheel_y -= rows;
        doc.top = (doc.top as f32 + rows).clamp(0., max_top as f32) as usize;
        doc.wheel_x -= design(delta.x) / cell;
        let cols = doc.wheel_x.trunc();
        doc.wheel_x -= cols;
        doc.left = (doc.left as f32 + cols).max(0.) as usize;
        cx.notify();
    }

    fn ide_tab_strip(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let ide = &self.tabs.ide;
        let tabs: Vec<AnyElement> = ide
            .docs
            .iter()
            .enumerate()
            .map(|(ix, doc)| {
                let active = ix == ide.active;
                let muted = theme.muted.to_hsla();
                let close_popover = (ide.closing == Some(ix)).then(|| self.ide_close_popover(ix, theme, cx));
                div()
                    .id(("ide-tab", ix))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .pl(px(12.))
                    .pr(px(8.))
                    .py(px(7.))
                    .rounded_t(px(6.))
                    .text_size(px(12.))
                    .cursor_pointer()
                    .when(active, |d| d.bg(theme.bg.to_hsla()).text_color(theme.fg.to_hsla()))
                    .when(!active, |d| d.text_color(muted).hover(|d| d.text_color(theme.fg.to_hsla())))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.ide_select(ix, cx)))
                    .child(doc.name())
                    .when(doc.buffer.is_dirty(), |d| {
                        d.child(
                            div()
                                .id(("ide-save", ix))
                                .px(px(2.))
                                .rounded(px(4.))
                                .cursor_pointer()
                                .text_color(theme.acc2.to_hsla())
                                .hover(|d| d.text_color(theme.fg.to_hsla()))
                                .tooltip(tooltip("Save", TooltipColors::of(theme)))
                                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                    cx.stop_propagation();
                                    this.ide_save_doc(ix, cx);
                                }))
                                .child("•"),
                        )
                    })
                    .child(
                        div()
                            .id(("ide-close", ix))
                            .px(px(4.))
                            .rounded(px(4.))
                            .text_color(muted)
                            .hover(|d| d.text_color(theme.fg.to_hsla()))
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                cx.stop_propagation();
                                this.ide_close(ix, cx);
                            }))
                            .child("×"),
                    )
                    .children(close_popover)
                    .into_any_element()
            })
            .collect();
        let view = cx.entity();
        let right_controls = div()
            .flex()
            .items_center()
            .gap(px(8.))
            .pb(px(4.))
            .text_size(px(11.5))
            .text_color(theme.muted.to_hsla())
            .child(
                div()
                    .id("ide-open-file")
                    .px(px(6.))
                    .py(px(3.))
                    .rounded(px(4.))
                    .cursor_pointer()
                    .hover(|d| d.text_color(theme.fg.to_hsla()))
                    .on_click(cx.listener(|this, _: &ClickEvent, window, cx| this.ide_picker_open(window, cx)))
                    .child("Open file…"),
            )
            .child(div().flex_none().child("Follow agent"))
            .child(Toggle::new("ide-follow", self.tabs.ide.follow, theme).on_change(move |on, _, cx| {
                view.update(cx, |this, cx| {
                    this.tabs.ide.follow = on;
                    cx.notify();
                })
            }));
        div()
            .flex()
            .items_end()
            .justify_between()
            .gap(px(8.))
            .px(px(8.))
            .pt(px(6.))
            .border_b_1()
            .border_color(theme.line2.to_hsla())
            .child(
                div()
                    .flex()
                    .items_end()
                    .gap(px(2.))
                    .overflow_hidden()
                    .children(tabs),
            )
            .child(right_controls)
            .into_any_element()
    }

    /// The popover asking Save / Don't save / Cancel when closing a dirty tab.
    fn ide_close_popover(&self, ix: usize, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let name = self.tabs.ide.docs.get(ix).map(|d| d.name()).unwrap_or_default();
        let shell = cx.entity();
        let panel = div()
            .id("ide-close-popover")
            .occlude()
            .w(px(280.))
            .p(px(12.))
            .flex()
            .flex_col()
            .gap(px(10.))
            .rounded(px(10.))
            .border_1()
            .border_color(theme.line2.to_hsla())
            .bg(theme.pop.to_hsla())
            .text_color(theme.fg.to_hsla())
            .font_family(theme.ui_font.clone())
            .shadow(vec![BoxShadow {
                color: hsla(0., 0., 0., 0.45),
                offset: point(px(0.), px(14.)),
                blur_radius: px(40.),
                spread_radius: px(0.),
            }])
            .on_mouse_down_out(cx.listener(|this, _: &MouseDownEvent, _, cx| this.ide_cancel_close(cx)))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(
                        div()
                            .text_size(px(13.))
                            .font_weight(FontWeight::BOLD)
                            .child(format!("Save changes to {name}?")),
                    )
                    .child(
                        div()
                            .text_size(px(11.5))
                            .text_color(theme.muted.to_hsla())
                            .child("Your changes will be lost if you close without saving."),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap(px(6.))
                    .child(
                        Button::new("ide-close-cancel", "Cancel", theme)
                            .on_click({
                                let shell = shell.clone();
                                move |_, cx| shell.update(cx, |this, cx| this.ide_cancel_close(cx))
                            }),
                    )
                    .child(
                        Button::new("ide-close-discard", "Don't save", theme)
                            .style(ButtonStyle::Danger)
                            .on_click({
                                let shell = shell.clone();
                                move |_, cx| shell.update(cx, |this, cx| this.ide_finish_close(ix, false, cx))
                            }),
                    )
                    .child(
                        Button::new("ide-close-save", "Save", theme)
                            .style(ButtonStyle::Filled)
                            .on_click({
                                let shell = shell.clone();
                                move |_, cx| shell.update(cx, |this, cx| this.ide_finish_close(ix, true, cx))
                            }),
                    ),
            );

        deferred(
            anchored()
                .anchor(Corner::TopLeft)
                .offset(point(px(0.), px(32.)))
                .snap_to_window_with_margin(px(8.))
                .child(panel),
        )
        .with_priority(1)
        .into_any_element()
    }

    /// The thin bar under the tab strip for the changed-on-disk choice.
    fn ide_prompt(&self, theme: &Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
        let ide = &self.tabs.ide;
        if ide.closing.is_some() {
            return None;
        }
        let active = ide.active;
        let doc = ide.docs.get(active).filter(|doc| doc.changed_on_disk)?;
        let name = doc.name();
        Some(
            ide_bar(theme)
                .child(div().flex_1().min_w_0().truncate().child(format!("{name} was changed on disk.")))
                .child(bar_button("ide-disk-reload", "Reload", theme, cx.listener(move |this, _, _, cx| this.ide_reload(active, cx))))
                .child(bar_button("ide-disk-keep", "Keep mine", theme, cx.listener(move |this, _, _, cx| this.ide_keep_mine(active, cx))))
                .into_any_element(),
        )
    }

    /// Opens the Open file… picker on the tool folder.
    fn ide_picker_open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.tool_root() else {
            self.toast("Pick a project to open its files.", cx);
            return;
        };
        let input = cx.new(|cx| TextInput::new(picker_config(), window, cx));
        let changed = cx.subscribe(&input, |this, _, event: &TextInputEvent, cx| match event {
            TextInputEvent::Changed => this.ide_picker_filter(cx),
            TextInputEvent::Submit => this.ide_picker_first(cx),
            TextInputEvent::Blur => {}
        });
        let observe = cx.observe(&input, |_, _, cx| cx.notify());
        input.read(cx).focus(window);
        let matches = search(&root, "");
        self.tabs.ide.picker = Some(Picker { input, _subs: vec![changed, observe], root, matches });
        cx.notify();
    }

    /// Recomputes the picker's matches from its field.
    fn ide_picker_filter(&mut self, cx: &mut Context<Self>) {
        let Some(picker) = self.tabs.ide.picker.as_mut() else { return };
        let query = picker.input.read(cx).text().trim().to_string();
        let matches = search(&picker.root, &query);
        picker.matches = matches;
        cx.notify();
    }

    /// Opens the picker's first match.
    fn ide_picker_first(&mut self, cx: &mut Context<Self>) {
        let path = self.tabs.ide.picker.as_ref().and_then(|picker| picker.matches.first().cloned());
        if let Some(path) = path {
            self.ide_picker_pick(path, cx);
        }
    }

    /// Closes the picker and opens `path` in the tab.
    fn ide_picker_pick(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.tabs.ide.picker = None;
        self.ide_open(&path, cx);
    }

    fn ide_picker_close(&mut self, cx: &mut Context<Self>) {
        if self.tabs.ide.picker.take().is_some() {
            cx.notify();
        }
    }

    /// The Open file… picker: a filter field over the tool folder, above its matching file rows.
    fn ide_picker_view(&self, theme: &Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
        let picker = self.tabs.ide.picker.as_ref()?;
        let rows: Vec<AnyElement> = picker
            .matches
            .iter()
            .map(|path| {
                let id = SharedString::from(format!("ide-picker-{}", path.display()));
                let path = path.clone();
                file_match_row(id, &picker.root, &path, false, theme)
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.ide_picker_pick(path.clone(), cx)))
                    .into_any_element()
            })
            .collect();
        Some(
            div()
                .id("ide-picker-layer")
                .absolute()
                .top_0()
                .left_0()
                .right_0()
                .bottom_0()
                .occlude()
                .px(px(8.))
                .pt(px(8.))
                .flex()
                .flex_col()
                .child(
                    div()
                        .id("ide-picker")
                        .flex()
                        .flex_col()
                        .max_h(px(360.))
                        .rounded(px(9.))
                        .border_1()
                        .border_color(theme.line2.to_hsla())
                        .bg(theme.pop.to_hsla())
                        .text_color(theme.fg.to_hsla())
                        .font_family(theme.ui_font.clone())
                        .on_mouse_down_out(cx.listener(|this, _: &MouseDownEvent, _, cx| this.ide_picker_close(cx)))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(8.))
                                .px(px(10.))
                                .py(px(7.))
                                .border_b_1()
                                .border_color(theme.line.to_hsla())
                                .child(stroke_icon(SEARCH, 13., 1.3, theme.muted.to_hsla()))
                                .child(div().flex_1().min_w_0().text_size(px(12.5)).child(text_input(&picker.input, theme))),
                        )
                        .child(div().id("ide-picker-rows").flex().flex_col().p(px(4.)).min_h_0().overflow_y_scroll().children(rows)),
                )
                .into_any_element(),
        )
    }

    /// The active file's path under the tool folder.
    fn ide_breadcrumb(&self, root: &Path, theme: &Theme, _cx: &mut Context<Self>) -> AnyElement {
        let Some(doc) = self.tabs.ide.docs.get(self.tabs.ide.active) else { return div().into_any_element() };
        // A file outside the folder is named from home, as `~/…`.
        let rel = if doc.path.starts_with(root) { workspace::relative(root, &doc.path) } else { crate::ui::settings::tilde(&doc.path) };
        let parts: Vec<&str> = rel.split('/').filter(|p| !p.is_empty()).collect();
        let last = parts.len().saturating_sub(1);
        let mut path = div().flex().items_center().gap(px(5.)).min_w_0().overflow_hidden().whitespace_nowrap().flex_1();
        for (i, part) in parts.iter().enumerate() {
            if i > 0 {
                path = path.child(div().text_color(theme.faint.to_hsla()).child("/"));
            }
            let segment = div().child(part.to_string());
            path = path.child(if i == last { segment.text_color(theme.fg.to_hsla()) } else { segment });
        }
        div()
            .flex()
            .items_center()
            .px(px(14.))
            .py(px(8.))
            .bg(theme.line.to_hsla())
            .border_b_1()
            .border_color(theme.line.to_hsla())
            .text_size(px(11.5))
            .text_color(theme.muted.to_hsla())
            .child(path)
            .into_any_element()
    }

    /// The code area: the visible rows from `doc.top`, drawn from the top edge. The area doesn't scroll
    /// itself; the wheel moves `doc.top`, so rows scrolled past are simply not drawn.
    fn ide_code(&mut self, focused: bool, focus: &FocusHandle, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let ide = &mut self.tabs.ide;
        let (cell, visible) = (ide.cell, ide.visible_lines());
        let viewport = ide.viewport.clone();
        let Some(doc) = ide.docs.get_mut(ide.active) else { return div().into_any_element() };
        let count = doc.buffer.line_count();
        let top = doc.top.min(count - 1);
        doc.top = top;
        let end = (top + visible + 1).min(count);
        let cursor = doc.buffer.cursor();
        let selection = doc.buffer.selection_range();
        let marks: HashMap<usize, Mark> = doc.marks().iter().copied().collect();
        let left = doc.left;
        let Doc { buffer, .. } = doc;
        let style = RowStyle { theme, cell, left };
        // Only the top padding goes above the first drawn row: a spacer for the rows scrolled past pushed
        // the visible rows below the bottom edge of this unscrolled area, so scrolling showed nothing.
        let mut rows = vec![div().h(px(PAD_Y)).into_any_element()];
        for line in top..end {
            let raw = buffer.line(line);
            let text = shown(&raw);
            let spans = tokenize(&text);
            let selected = selected_cols(selection, line, &raw);
            let caret = (focused && cursor.line == line).then(|| visual_col(&raw, cursor.col));
            rows.push(code_row(&style, line, &text, &spans, marks.get(&line).copied(), selected, caret));
        }
        div()
            .id("ide-code")
            .relative()
            .flex_1()
            .min_h_0()
            .overflow_hidden()
            .track_focus(focus)
            .track_scroll(&viewport)
            .on_mouse_down(MouseButton::Left, cx.listener(|this, ev: &MouseDownEvent, window, cx| {
                if let Some(focus) = this.tabs.ide.focus.clone() {
                    window.focus(&focus);
                }
                this.ide_pointer(ev.position, ev.click_count, ev.modifiers.shift, cx);
                this.tabs.ide.drag = true;
            }))
            .on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, _, cx| {
                if this.tabs.ide.drag && ev.pressed_button == Some(MouseButton::Left) {
                    this.ide_pointer(ev.position, 1, true, cx);
                }
            }))
            .on_mouse_up(MouseButton::Left, cx.listener(|this, _: &MouseUpEvent, _, _| this.tabs.ide.drag = false))
            .on_scroll_wheel(cx.listener(|this, ev: &ScrollWheelEvent, _, cx| this.ide_wheel(ev, cx)))
            .children(rows)
            .into_any_element()
    }

    /// The status bar: the branch, the language, the line count and the encoding.
    fn ide_status(&self, branch: Option<&str>, theme: &Theme) -> AnyElement {
        let Some(doc) = self.tabs.ide.docs.get(self.tabs.ide.active) else { return div().into_any_element() };
        let mut bar = div()
            .h(px(24.))
            .flex()
            .items_center()
            .gap(px(14.))
            .px(px(14.))
            .text_size(px(11.))
            .text_color(theme.muted.to_hsla());
        if let Some(branch) = branch {
            bar = bar.child(div().text_color(theme.alt.to_hsla()).child(format!("⎇ {branch}")));
        }
        if let Some(language) = doc.highlighter.language() {
            bar = bar.child(language);
        }
        bar = bar.child(div().flex_1());
        if doc.deleted {
            bar = bar.child(div().text_color(theme.acc2.to_hsla()).child("deleted on disk"));
        }
        if doc.changed_on_disk {
            bar = bar.child(div().text_color(theme.acc2.to_hsla()).child("changed on disk"));
        }
        bar.child(format!("{} lines", doc.buffer.line_count())).child("UTF-8").into_any_element()
    }
}

/// A centred-left message in place of the code area.
/// The IDE's page in VS Code mode, for the shell's `WebView` calls.
fn ide_view(shell: &mut Shell) -> &mut WebView {
    &mut shell.tabs.ide.vscode.view
}

/// The thin bar under the Ide tab strip, in the header's style: the `line` fill and its border, and the
/// breadcrumb's text size.
fn ide_bar(theme: &Theme) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .px(px(14.))
        .py(px(6.))
        .bg(theme.line.to_hsla())
        .border_b_1()
        .border_color(theme.line.to_hsla())
        .text_size(px(11.5))
        .text_color(theme.fg.to_hsla())
}

/// A small outlined button for an Ide bar, like the tools' other header buttons.
fn bar_button(
    id: &'static str,
    label: &'static str,
    theme: &Theme,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    div()
        .id(id)
        .px(px(9.))
        .py(px(3.))
        .rounded(px(6.))
        .border_1()
        .border_color(theme.line2.to_hsla())
        .text_size(px(11.5))
        .cursor_pointer()
        .hover(|d| d.bg(theme.line2.to_hsla()))
        .on_click(on_click)
        .child(label)
        .into_any_element()
}

/// The Open file… picker's field, the Files filter's metrics.
fn picker_config() -> TextInputConfig {
    TextInputConfig {
        placeholder: "Open file…".into(),
        font_size: px(12.5),
        line_height: px(18.),
        ..Default::default()
    }
}

/// Records a change to `path`: a path already listed takes its newest kind and keeps its place. The
/// oldest entry is dropped once the list is full.
pub(super) fn record_change(changes: &mut Vec<(PathBuf, ChangeKind)>, path: &Path, kind: ChangeKind) {
    if let Some(entry) = changes.iter_mut().find(|(p, _)| p == path) {
        entry.1 = kind;
        return;
    }
    if changes.len() >= MAX_CHANGES {
        changes.remove(0);
    }
    changes.push((path.to_path_buf(), kind));
}

/// Whether `path` is the scratch file the editor's atomic save writes beside its target, which is the
/// app's own write and not an agent change.
pub(super) fn is_scratch(path: &Path) -> bool {
    path.file_name().and_then(|name| name.to_str()).is_some_and(|name| name.ends_with(".archonui-tmp"))
}

pub(super) fn placeholder(text: &str, theme: &Theme) -> AnyElement {
    div()
        .flex_1()
        .p(px(14.))
        .text_size(px(12.5))
        .text_color(theme.muted.to_hsla())
        .child(text.to_string())
        .into_any_element()
}

/// One code row: the gutter, the highlighted text, the changed band, the selection and the caret.
/// `selected` and `caret` are visual columns.
fn code_row(
    style: &RowStyle,
    line: usize,
    text: &str,
    spans: &[(Range<usize>, Tok)],
    mark: Option<Mark>,
    selected: Option<(usize, usize)>,
    caret: Option<usize>,
) -> AnyElement {
    let (theme, cell, left) = (style.theme, style.cell, style.left);
    let x = |col: usize| TEXT_X + (col as f32 - left as f32) * cell;
    let mut row = div()
        .relative()
        .h(px(LINE_H))
        .w_full()
        .overflow_hidden()
        .flex()
        .text_size(px(CODE_SIZE))
        .font_family(theme.code_font.clone());
    if matches!(mark, Some(Mark::Added | Mark::Modified)) {
        row = row.bg(theme.soft.to_hsla());
    }
    if let Some((from, to)) = selected.filter(|(from, to)| to > from) {
        row = row.child(
            div()
                .absolute()
                .top_0()
                .bottom_0()
                .left(px(x(from)))
                .w(px((to - from) as f32 * cell))
                .bg(theme.line2.to_hsla()),
        );
    }
    let gutter = if caret.is_some() { theme.muted } else { theme.faint };
    row = row.child(
        div()
            .w(px(GUTTER_W))
            .flex_none()
            .text_right()
            .text_color(gutter.to_hsla())
            .child((line + 1).to_string()),
    );
    row = row.child(
        div()
            .ml(px(GAP))
            .flex_1()
            .min_w_0()
            .whitespace_nowrap()
            .relative()
            .left(px(-(left as f32) * cell))
            .child(styled_line(text, spans, theme)),
    );
    if let Some(col) = caret {
        row = row.child(
            div()
                .absolute()
                .top(px(3.))
                .bottom(px(3.))
                .left(px(x(col)))
                .w(px(2.))
                .bg(theme.acc2.to_hsla()),
        );
    }
    if matches!(mark, Some(Mark::Removed)) {
        row = row.child(div().absolute().top_0().left_0().right_0().h(px(2.)).bg(theme.alt.to_hsla()));
    }
    row.into_any_element()
}

/// `text` with its token `spans` coloured by the stream's token colours, in the code font. Span ranges are
/// byte ranges into `text`; text outside them uses the theme's text colour.
pub(super) fn styled_line(text: &str, spans: &[(Range<usize>, Tok)], theme: &Theme) -> StyledText {
    let mut runs = Vec::new();
    let mut at = 0;
    for (range, tok) in spans {
        let (start, end) = (range.start, range.end.min(text.len()));
        if start < at || end <= start || !text.is_char_boundary(start) || !text.is_char_boundary(end) {
            continue;
        }
        if start > at {
            runs.push(text_run(start - at, theme.fg.to_hsla(), theme));
        }
        runs.push(text_run(end - start, token_colour(*tok, theme), theme));
        at = end;
    }
    if at < text.len() {
        runs.push(text_run(text.len() - at, theme.fg.to_hsla(), theme));
    }
    StyledText::new(text.to_string()).with_runs(runs)
}

fn text_run(len: usize, color: Hsla, theme: &Theme) -> TextRun {
    TextRun {
        len,
        font: font(theme.code_font.clone()),
        color,
        background_color: None,
        underline: None,
        strikethrough: None,
    }
}

/// The width of one column of the code font, from the text system.
fn cell_width(window: &Window, family: &str) -> f32 {
    let text = window.text_system();
    let font_id = text.resolve_font(&font(family.to_string()));
    text.advance(font_id, px(CODE_SIZE), 'm').map_or(8., |size| design(size.width))
}

/// A line as it is drawn: each tab becomes `TAB_WIDTH` spaces.
fn shown(raw: &str) -> String {
    raw.replace('\t', &" ".repeat(TAB_WIDTH))
}

/// The visual column of char column `col` in `raw`.
fn visual_col(raw: &str, col: usize) -> usize {
    raw.chars().take(col).map(|c| if c == '\t' { TAB_WIDTH } else { 1 }).sum()
}

/// The char column at visual column `vcol` of `raw`, or the line's end when `vcol` is past it.
fn char_col(raw: &str, vcol: usize) -> usize {
    let mut width = 0;
    for (col, c) in raw.chars().enumerate() {
        if width >= vcol {
            return col;
        }
        width += if c == '\t' { TAB_WIDTH } else { 1 };
    }
    raw.chars().count()
}

/// The visual columns of `raw` that the selection covers on `line`. A selection that runs past the
/// line's end also covers the line break.
fn selected_cols(selection: Option<(Pos, Pos)>, line: usize, raw: &str) -> Option<(usize, usize)> {
    let (start, end) = selection?;
    if line < start.line || line > end.line {
        return None;
    }
    let len = raw.chars().count();
    let from = if line == start.line { start.col } else { 0 };
    let to = if line == end.line { end.col } else { len + 1 };
    let to_visual = visual_col(raw, to.min(len)) + usize::from(to > len);
    Some((visual_col(raw, from.min(len)), to_visual))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn changes_are_recorded_once_with_their_newest_kind() {
        let mut changes = Vec::new();
        record_change(&mut changes, Path::new("/w/a.rs"), ChangeKind::Created);
        record_change(&mut changes, Path::new("/w/b.rs"), ChangeKind::Modified);
        record_change(&mut changes, Path::new("/w/a.rs"), ChangeKind::Modified);
        assert_eq!(changes.len(), 2);
        assert_eq!(changes[0].0.as_path(), Path::new("/w/a.rs"), "a repeated path keeps its place");
        assert_eq!(changes[0].1, ChangeKind::Modified, "the newest kind wins");
        for ix in 0..MAX_CHANGES {
            record_change(&mut changes, Path::new(&format!("/w/f{ix}.rs")), ChangeKind::Created);
        }
        assert_eq!(changes.len(), MAX_CHANGES, "the list stops growing");
    }

    #[test]
    fn the_editors_atomic_save_scratch_file_is_not_a_change() {
        assert!(is_scratch(Path::new("/w/.cache.ts.archonui-tmp")));
        assert!(!is_scratch(Path::new("/w/cache.ts")));
        assert!(!is_scratch(Path::new("/w/cache.ts.tmp")));
    }

    #[test]
    fn columns_count_tabs_as_tab_width() {
        let raw = "\tab\tc";
        assert_eq!(visual_col(raw, 0), 0);
        assert_eq!(visual_col(raw, 1), TAB_WIDTH);
        assert_eq!(visual_col(raw, 3), TAB_WIDTH + 2);
        assert_eq!(char_col(raw, TAB_WIDTH), 1);
        assert_eq!(char_col(raw, TAB_WIDTH + 1), 2);
        assert_eq!(char_col(raw, 99), raw.chars().count());
        assert_eq!(shown(raw), "    ab    c");
    }

    #[test]
    fn selection_covers_each_line_it_touches() {
        let sel = Some((Pos { line: 1, col: 2 }, Pos { line: 3, col: 1 }));
        assert_eq!(selected_cols(sel, 0, "abc"), None);
        assert_eq!(selected_cols(sel, 1, "abcd"), Some((2, 5)));
        assert_eq!(selected_cols(sel, 2, "xy"), Some((0, 3)));
        assert_eq!(selected_cols(sel, 3, "pqr"), Some((0, 1)));
        assert_eq!(selected_cols(None, 1, "abc"), None);
    }

    #[test]
    fn reload_clears_changed_on_disk_and_deleted() {
        let dir = std::env::temp_dir().join(format!("archonui-test-doc-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.rs");
        fs::write(&path, "original content\n").unwrap();

        let mut doc = Doc::open(&path).unwrap();
        doc.changed_on_disk = true;
        doc.deleted = true;
        doc.reload("new content\n");

        assert_eq!(doc.buffer.text(), "new content\n");
        assert!(!doc.changed_on_disk);
        assert!(!doc.deleted);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn dropping_a_tab_keeps_the_active_and_asking_tabs_on_their_files() {
        let dir = std::env::temp_dir().join(format!("archonui-test-marks-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let paths: Vec<PathBuf> = ["a.rs", "b.rs", "c.rs"].iter().map(|name| dir.join(name)).collect();
        for path in &paths {
            fs::write(path, "x\n").unwrap();
        }
        let mut ide = IdeUi::default();
        ide.docs = paths.iter().map(|path| Doc::open(path).unwrap()).collect();
        ide.active = 2;
        ide.closing = Some(1);

        // The first tab goes, as when its file is deleted from the Files tab.
        let marks = TabMarks::take(&ide);
        ide.docs.retain(|doc| doc.path != paths[0]);
        marks.restore(&mut ide, Path::to_path_buf);
        assert_eq!(ide.docs[ide.active].path, paths[2]);
        assert_eq!(ide.closing.map(|ix| ide.docs[ix].path.clone()), Some(paths[1].clone()));

        // The asking tab goes: the question ends rather than moving to another file.
        let marks = TabMarks::take(&ide);
        ide.docs.retain(|doc| doc.path != paths[1]);
        marks.restore(&mut ide, Path::to_path_buf);
        assert_eq!(ide.closing, None);
        assert_eq!(ide.docs[ide.active].path, paths[2]);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn dirty_save_syncs_buffer_state() {
        let dir = std::env::temp_dir().join(format!("archonui-test-save-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("save_test.rs");
        fs::write(&path, "initial\n").unwrap();

        let mut doc = Doc::open(&path).unwrap();
        assert!(!doc.buffer.is_dirty());

        doc.buffer.insert("edit ");
        assert!(doc.buffer.is_dirty());

        doc.buffer.save().unwrap();
        assert!(!doc.buffer.is_dirty());
        assert_eq!(fs::read_to_string(&path).unwrap(), "edit initial\n");

        let _ = fs::remove_dir_all(&dir);
    }
}
