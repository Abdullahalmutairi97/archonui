//! The Terminal tab: the focused session's shell, run in tmux when `use_tmux` is on, and drawn from
//! `crate::terminal`'s screen snapshots. Spec: docs/design/tools-and-behaviour.md §A5, in theme colours.

use crate::{
    app::workspace::RightTab,
    terminal::{tmux_cancel, tmux_kill, tmux_scroll, tmux_session_name, tmux_socket, Screen, Span, TermEvent, Terminal},
    ui::{
        shell::Shell,
        theme::Theme,
        widgets::{Button, ButtonStyle},
    },
};
use gpui::{
    canvas, div, font, prelude::*, rgb, AnyElement, Bounds, ClickEvent, ClipboardItem, Context, DispatchPhase,
    FocusHandle, Font, FontStyle, FontWeight, Hsla, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, Pixels, Point, ScrollWheelEvent, StyledText, TextRun, UnderlineStyle, Window,
};
use crate::ui::scale::{px, real};
use std::{cell::Cell, collections::HashMap, rc::Rc};

/// Horizontal and vertical space between the text and the terminal's edge, the prototype's `padding: 14px 16px`.
const PAD_X: f32 = 16.;
const PAD_Y: f32 = 14.;
/// Size of a new shell before the panel has been laid out.
const DEFAULT_SIZE: (u16, u16) = (80, 24);
/// Lines scrolled per mouse wheel notch.
const WHEEL_LINES: f32 = 3.;
/// Row height as a multiple of the text size, the prototype's `line-height: 1.6`.
const LINE_HEIGHT: f32 = 1.6;

/// The tab's state, not persisted: each chat's shell, and the terminal area.
#[derive(Default)]
pub struct TerminalUi {
    /// Each session's shell, or why it couldn't start. Started when the tab first shows for it.
    sessions: HashMap<String, Result<TermState, String>>,
    /// Bumped for every shell started, so events from a replaced shell are ignored.
    generations: u64,
    /// Keystrokes go to the shell while this has focus.
    focus: Option<FocusHandle>,
    /// Where the terminal area was last laid out, for sizing the shell.
    bounds: Rc<Cell<Bounds<Pixels>>>,
    /// Width of a terminal cell, as of the last render.
    cell_w: Pixels,
}

struct TermState {
    term: Terminal,
    generation: u64,
    screen: Screen,
    /// Set once the shell has exited, with its exit code if it reported one.
    exited: Option<Option<u32>>,
    /// Size last sent to the shell, as (cols, rows).
    size: (u16, u16),
    selection: Option<Selection>,
    /// Set from a left press in the terminal until the button comes up.
    selecting: bool,
    /// Set by a wheel scroll in a tmux session, which leaves tmux in copy mode.
    copy_mode: bool,
}

/// The text selected on the visible screen: from `anchor` to `head`, as `(row, col)`.
/// Either end may come first.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Selection {
    anchor: (usize, usize),
    head: (usize, usize),
}

impl Selection {
    /// The selected columns of `row`, inclusive, or `None` if the row isn't selected.
    fn cols_in_row(self, row: usize, cols: usize) -> Option<(usize, usize)> {
        let (start, end) = if self.anchor <= self.head { (self.anchor, self.head) } else { (self.head, self.anchor) };
        if row < start.0 || row > end.0 {
            return None;
        }
        let from = if row == start.0 { start.1 } else { 0 };
        let to = if row == end.0 { end.1 } else { cols.saturating_sub(1) };
        Some((from, to))
    }
}

/// The terminal's colours, taken from the theme. The ANSI colours a shell sets stay as the backend gives them.
#[derive(Clone, Copy)]
struct Colours {
    fg: Hsla,
    bg: Hsla,
    selection: Hsla,
    cursor: Hsla,
}

impl Colours {
    fn of(theme: &Theme) -> Self {
        Self {
            fg: theme.fg.to_hsla(),
            bg: theme.bg.to_hsla(),
            selection: theme.soft.to_hsla(),
            cursor: theme.acc2.to_hsla(),
        }
    }
}

impl TerminalUi {
    /// Columns and rows that fit the laid-out area, or `None` before the first layout.
    fn grid(&self, cell_w: Pixels, line_h: f32) -> Option<(u16, u16)> {
        let b = self.bounds.get();
        let (w, h) = (f32::from(b.size.width), f32::from(b.size.height));
        if w <= 0. || h <= 0. {
            return None;
        }
        // Bounds and cell width are window px; the paddings and line height are design px.
        let (pad_x, pad_y, line_h) = (real(PAD_X), real(PAD_Y), real(line_h));
        let cols = ((w - 2. * pad_x) / f32::from(cell_w)).floor().max(2.) as u16;
        let rows = ((h - 2. * pad_y) / line_h).floor().max(2.) as u16;
        Some((cols, rows))
    }

    /// The `(row, col)` of the screen cell under `pos`, not yet clamped to the screen.
    fn cell_at(&self, pos: Point<Pixels>, line_h: f32) -> (usize, usize) {
        let origin = self.bounds.get().origin;
        let x = f32::from(pos.x - origin.x) - real(PAD_X);
        let y = f32::from(pos.y - origin.y) - real(PAD_Y);
        let col = (x / f32::from(self.cell_w)).floor().max(0.) as usize;
        let row = (y / real(line_h)).floor().max(0.) as usize;
        (row, col)
    }

    fn focused(&self, window: &Window) -> bool {
        self.focus.as_ref().is_some_and(|f| f.is_focused(window))
    }

    /// Hangs up a session's shell. Its tmux session is killed too, so the shell ends.
    fn hang_up(&mut self, id: &str) {
        if let Some(Ok(state)) = self.sessions.remove(id) {
            if let Some(name) = state.term.tmux() {
                tmux_kill(name);
            }
        }
    }
}

impl Shell {
    /// Terminal text size and row height, from the settings.
    fn term_metrics(&self) -> (f32, f32) {
        let size = self.state.settings.terminal_font_size;
        (size, line_height(size))
    }

    fn terminal_visible(&self) -> bool {
        let ws = &self.state.workspace;
        ws.right_open && ws.right_tab == RightTab::Terminal
    }

    /// The focused session, which the terminal is for. `None` for a chat on another device: a shell
    /// here would open in this device's copy of that folder, if there is one.
    fn terminal_session(&self) -> Option<String> {
        let id = self.state.workspace.layout.focus.as_ref()?;
        self.state.session(id).filter(|s| s.origin.is_none()).map(|s| s.id.clone())
    }

    pub(in crate::ui) fn render_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = self.theme.clone();
        let focus = self.tabs.terminal.focus.get_or_insert_with(|| cx.focus_handle()).clone();
        let focused = focus.is_focused(window);
        let Some(id) = self.terminal_session() else {
            return div()
                .flex_1()
                .min_h_0()
                .px(px(16.))
                .py(px(10.))
                .text_xs()
                .text_color(theme.muted.to_hsla())
                .child("Open a chat to use its terminal.")
                .into_any_element();
        };
        let cwd = self.state.session(&id).map(|s| s.cwd.display().to_string()).unwrap_or_default();
        let (font_size, line_h) = self.term_metrics();
        let cell_w = cell_width(window, &theme.code_font, font_size);
        self.tabs.terminal.cell_w = cell_w;

        let state = self.tabs.terminal.sessions.get(&id);
        let (title, title_color): (String, Hsla) = match state {
            Some(Ok(s)) if !s.screen.title.is_empty() => (s.screen.title.clone(), theme.muted.to_hsla()),
            _ => (cwd, theme.muted.to_hsla()),
        };
        // A shell that has exited says so on its own wrapping line, so the "press Enter" hint fits any panel width.
        let notice: Option<String> = match state {
            Some(Ok(TermState { exited: Some(code), .. })) => Some(exit_notice(*code)),
            _ => None,
        };
        let error: Option<String> = match state {
            Some(Err(e)) => Some(e.clone()),
            _ => None,
        };
        let colours = Colours::of(&theme);
        let (rows, cursor) = match state {
            Some(Ok(s)) => {
                let cols = usize::from(s.size.0);
                let rows = s
                    .screen
                    .rows
                    .iter()
                    .enumerate()
                    .map(|(row, spans)| {
                        term_row(spans, s.selection.and_then(|sel| sel.cols_in_row(row, cols)), line_h, colours, &theme.code_font)
                    })
                    .collect::<Vec<_>>();
                let cursor = s.screen.cursor.filter(|_| focused).map(|(row, col)| cursor_block(row, col, cell_w, line_h, colours));
                (rows, cursor)
            }
            _ => (Vec::new(), None),
        };

        let tmux = match state {
            Some(Ok(s)) => s.term.tmux().map(str::to_owned),
            _ => None,
        };
        let shell = cx.entity();
        let bounds = self.tabs.terminal.bounds.clone();
        let click_focus = focus.clone();
        let page = div()
            .id("term-page")
            .track_focus(&focus)
            .flex_1()
            .min_h_0()
            .relative()
            .overflow_hidden()
            .px(px(PAD_X))
            .py(px(PAD_Y))
            .bg(colours.bg)
            .text_size(px(font_size))
            .line_height(px(line_h))
            .cursor_text()
            // Records where the terminal is, so the shell can be sized to it. Window-level listeners
            // extend a selection that leaves the terminal; they are registered on every paint.
            .child(
                canvas(
                    move |b, window, _| {
                        if bounds.get() != b {
                            bounds.set(b);
                            // The shell was sized from the old bounds; draw once more to size it to these.
                            window.request_animation_frame();
                        }
                    },
                    move |_, _, window, _| {
                        let moving = shell.clone();
                        window.on_mouse_event(move |ev: &MouseMoveEvent, phase, _, cx| {
                            if phase == DispatchPhase::Bubble && ev.pressed_button == Some(MouseButton::Left) {
                                moving.update(cx, |this, cx| this.term_drag_move(ev.position, cx));
                            }
                        });
                        let releasing = shell.clone();
                        window.on_mouse_event(move |_: &MouseUpEvent, phase, _, cx| {
                            if phase == DispatchPhase::Bubble {
                                releasing.update(cx, |this, cx| this.term_drag_end(cx));
                            }
                        });
                    },
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
            )
            .on_mouse_down(MouseButton::Left, cx.listener(move |this, ev: &MouseDownEvent, window, cx| {
                cx.stop_propagation();
                window.focus(&click_focus);
                this.term_press(ev);
                cx.notify();
            }))
            .on_scroll_wheel(cx.listener(|this, ev: &ScrollWheelEvent, _, cx| this.terminal_scroll(ev, cx)))
            .children(rows)
            .children(cursor);

        let mut toolbar = div()
            .px(px(12.))
            .py(px(10.))
            .flex()
            .flex_col()
            .gap(px(4.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .children(tmux.as_ref().map(|name| {
                        div().text_xs().text_color(theme.faint.to_hsla()).child(format!("tmux · {name}"))
                    }))
                    .child(div().flex_1().min_w_0().text_xs().text_color(title_color).truncate().child(title))
                    .child(Button::new("term-clear", "Clear", &theme).style(ButtonStyle::Ghost).on_click({
                        let shell = cx.entity();
                        // Ctrl+L clears the shell's screen.
                        move |_, cx| shell.update(cx, |this, _| this.write_terminal(b"\x0c"))
                    }))
                    .child(Button::new("term-restart", "Restart", &theme).style(ButtonStyle::Ghost).on_click({
                        let shell = cx.entity();
                        move |_, cx| shell.update(cx, |this, cx| this.restart_terminal(cx))
                    })),
            );
        // The command that attaches to this session from another terminal. Click to copy it.
        if let Some(text) = notice {
            toolbar = toolbar.child(div().text_xs().text_color(theme.acc.to_hsla()).child(text));
        }
        if let Some(name) = tmux {
            let attach = format!("tmux -L {} attach -t {name}", tmux_socket());
            toolbar = toolbar.child(
                div()
                    .id("term-attach")
                    .text_xs()
                    .text_color(theme.faint.to_hsla())
                    .truncate()
                    .child(format!("attach: {attach}"))
                    .on_click(move |_: &ClickEvent, _, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(attach.clone()));
                    }),
            );
        }

        div()
            .size_full()
            .min_h_0()
            .flex()
            .flex_col()
            .child(toolbar)
            .children(error.map(|text| {
                div()
                    .mx(px(12.))
                    .mb(px(8.))
                    .px(px(10.))
                    .py(px(6.))
                    .rounded(px(6.))
                    .bg(theme.deep.to_hsla())
                    .text_xs()
                    // `on` is for text on the bright accent; on `deep` it is dark on dark, so the body colour reads.
                    .text_color(theme.fg.to_hsla())
                    .child(text)
            }))
            .child(page)
            .into_any_element()
    }

    /// Starts the focused session's shell once the tab shows, and keeps it sized to the panel.
    pub(in crate::ui) fn terminal_before_render(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.terminal_visible() {
            return;
        }
        let Some(id) = self.terminal_session() else { return };
        let Some(cwd) = self.state.session(&id).map(|s| s.cwd.clone()) else { return };
        let (font_size, line_h) = self.term_metrics();
        let cell_w = cell_width(window, &self.theme.code_font, font_size);
        self.tabs.terminal.cell_w = cell_w;
        let grid = self.tabs.terminal.grid(cell_w, line_h);
        if !self.tabs.terminal.sessions.contains_key(&id) {
            let size = grid.unwrap_or(DEFAULT_SIZE);
            self.tabs.terminal.generations += 1;
            let generation = self.tabs.terminal.generations;
            // Without tmux the shell ends when ArchonUI does.
            let tmux = self.state.settings.use_tmux.then(|| tmux_session_name(&id));
            let state = match Terminal::spawn(&cwd, size.0, size.1, tmux.as_deref()) {
                Ok((term, events)) => {
                    self.state.mark_terminal(&id);
                    self.flush_sessions(cx);
                    let loop_id = id.clone();
                    cx.spawn(async move |shell, cx| {
                        while let Ok(ev) = events.recv().await {
                            let live = shell.update(cx, |this, cx| this.terminal_event(&loop_id, generation, ev, cx));
                            if !live.unwrap_or(false) {
                                break;
                            }
                        }
                    })
                    .detach();
                    Ok(TermState {
                        screen: term.screen(),
                        term,
                        generation,
                        exited: None,
                        size,
                        selection: None,
                        selecting: false,
                        copy_mode: false,
                    })
                }
                Err(e) => Err(e),
            };
            self.tabs.terminal.sessions.insert(id.clone(), state);
        }
        if let (Some(size), Some(Ok(state))) = (grid, self.tabs.terminal.sessions.get_mut(&id)) {
            if state.size != size {
                state.term.resize(size.0, size.1);
                state.size = size;
                state.selection = None;
                state.screen = state.term.screen();
            }
        }
    }

    /// Applies an event from a shell. Returns false once that shell has been replaced, which ends its event loop.
    fn terminal_event(&mut self, id: &str, generation: u64, ev: TermEvent, cx: &mut Context<Self>) -> bool {
        let Some(Ok(state)) = self.tabs.terminal.sessions.get_mut(id) else { return false };
        if state.generation != generation {
            return false;
        }
        match ev {
            TermEvent::Changed => state.screen = state.term.screen(),
            TermEvent::Exited(code) => state.exited = Some(code),
        }
        cx.notify();
        true
    }

    /// Hangs up the focused session's shell, killing its tmux session. The next render starts a new one.
    fn restart_terminal(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = self.terminal_session() {
            self.tabs.terminal.hang_up(&id);
        }
        cx.notify();
    }

    /// After the client exited: drops the entry, so the next render reattaches if the tmux session still runs
    /// (`-A`), else starts fresh.
    fn reattach_terminal(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = self.terminal_session() {
            self.tabs.terminal.sessions.remove(&id);
        }
        cx.notify();
    }

    /// Hangs up a deleted session's shell and kills its tmux session, which may be from an earlier run and so
    /// have no shell in the tab. Call this from the sidebar's delete.
    pub(in crate::ui) fn terminal_forget(&mut self, id: &str) {
        self.tabs.terminal.hang_up(id);
        tmux_kill(&tmux_session_name(id));
    }

    /// Ends a drag-selection when the keyboard leaves the Terminal tab.
    pub(in crate::ui) fn terminal_blur(&mut self, cx: &mut Context<Self>) {
        self.term_drag_end(cx);
    }

    fn write_terminal(&self, bytes: &[u8]) {
        let Some(id) = self.terminal_session() else { return };
        if let Some(Ok(state)) = self.tabs.terminal.sessions.get(&id) {
            state.term.write(bytes);
        }
    }

    /// The focused session's visible screen as text, one line per row, or `None` without a running shell.
    pub(in crate::ui) fn terminal_capture(&self) -> Option<String> {
        let id = self.terminal_session()?;
        let Some(Ok(state)) = self.tabs.terminal.sessions.get(&id) else { return None };
        let lines: Vec<String> = state
            .screen
            .rows
            .iter()
            .map(|spans| spans.iter().map(|s| s.text.as_str()).collect::<String>().trim_end().to_string())
            .collect();
        Some(lines.join("\n"))
    }

    pub(in crate::ui) fn terminal_key(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let ks = &ev.keystroke;
        let m = ks.modifiers;
        if !self.terminal_visible() || m.platform || !self.tabs.terminal.focused(window) {
            return false;
        }
        if self.app_shortcut(ks) {
            return false;
        }
        let Some(id) = self.terminal_session() else { return false };
        let Some(Ok(state)) = self.tabs.terminal.sessions.get_mut(&id) else { return false };
        // Ctrl+Shift+C copies; plain Ctrl+C copies only when something is selected, else it is the interrupt.
        if m.control && ks.key == "c" && (m.shift || state.selection.is_some()) {
            if let Some(sel) = state.selection {
                let text = selected_text(&state.screen, sel, usize::from(state.size.0));
                cx.write_to_clipboard(ClipboardItem::new_string(text));
            }
        } else if m.control && m.shift && ks.key == "v" {
            state.selection = None;
            leave_copy_mode(state);
            if let Some(text) = cx.read_from_clipboard().and_then(|c| c.text()) {
                let bytes = if state.term.bracketed_paste() {
                    // Pasted text can't end the paste early: an end marker inside it is dropped.
                    format!("\x1b[200~{}\x1b[201~", strip_paste_end(&text))
                } else {
                    text.replace("\r\n", "\r").replace('\n', "\r")
                };
                state.term.write(bytes.as_bytes());
            }
        } else if state.exited.is_some() {
            if ks.key == "enter" {
                self.reattach_terminal(cx);
            }
        } else {
            state.selection = None;
            // Typing returns to the live screen.
            if state.term.scrollback() > 0 {
                state.term.set_scrollback(0);
                state.screen = state.term.screen();
            }
            leave_copy_mode(state);
            if let Some(mut bytes) = key_bytes(&ks.key, ks.key_char.as_deref(), m.control, m.alt, m.shift) {
                if state.term.application_cursor() {
                    to_application_cursor(&mut bytes);
                }
                state.term.write(&bytes);
            }
        }
        cx.notify();
        true
    }

    /// Mouse wheel: scrolls tmux's copy mode in a tmux session. Otherwise it scrolls the shell's history,
    /// or sends arrow keys when the shell is on a full-screen app's alternate screen.
    fn terminal_scroll(&mut self, ev: &ScrollWheelEvent, cx: &mut Context<Self>) {
        let (_, line_h) = self.term_metrics();
        let Some(id) = self.terminal_session() else { return };
        let Some(Ok(state)) = self.tabs.terminal.sessions.get_mut(&id) else { return };
        let notches = f32::from(ev.delta.pixel_delta(px(line_h)).y) / real(line_h);
        let lines = (notches * WHEEL_LINES).round() as i32;
        if lines == 0 {
            return;
        }
        state.selection = None;
        if let Some(name) = state.term.tmux() {
            tmux_scroll(name, lines > 0, lines.unsigned_abs());
            state.copy_mode = true;
        } else if state.screen.alternate {
            let mut arrow = if lines > 0 { b"\x1b[A".to_vec() } else { b"\x1b[B".to_vec() };
            if state.term.application_cursor() {
                to_application_cursor(&mut arrow);
            }
            state.term.write(&arrow.repeat(lines.unsigned_abs() as usize));
        } else {
            let back = state.term.scrollback() as i64 + i64::from(lines);
            state.term.set_scrollback(back.max(0) as usize);
            state.screen = state.term.screen();
        }
        cx.notify();
    }

    /// A left press in the terminal: starts a selection, or selects the word or row on a double or triple click.
    fn term_press(&mut self, ev: &MouseDownEvent) {
        let (_, line_h) = self.term_metrics();
        let cell = self.tabs.terminal.cell_at(ev.position, line_h);
        let Some(id) = self.terminal_session() else { return };
        let Some(Ok(state)) = self.tabs.terminal.sessions.get_mut(&id) else { return };
        let (row, col) = clamp_cell(cell, state.size);
        let cols = usize::from(state.size.0);
        state.selecting = ev.click_count == 1;
        state.selection = match ev.click_count {
            1 => Some(Selection { anchor: (row, col), head: (row, col) }),
            2 => word_selection(&state.screen, (row, col)),
            _ => Some(Selection { anchor: (row, 0), head: (row, cols.saturating_sub(1)) }),
        };
    }

    /// Extends the drag-selection to the cell under `pos`, even when the pointer has left the terminal.
    fn term_drag_move(&mut self, pos: Point<Pixels>, cx: &mut Context<Self>) {
        let (_, line_h) = self.term_metrics();
        let cell = self.tabs.terminal.cell_at(pos, line_h);
        let Some(id) = self.terminal_session() else { return };
        let Some(Ok(state)) = self.tabs.terminal.sessions.get_mut(&id) else { return };
        if !state.selecting {
            return;
        }
        let cell = clamp_cell(cell, state.size);
        if let Some(selection) = &mut state.selection {
            selection.head = cell;
        }
        cx.notify();
    }

    /// Ends a drag-selection. A press that never moved leaves no selection.
    fn term_drag_end(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.terminal_session() else { return };
        let Some(Ok(state)) = self.tabs.terminal.sessions.get_mut(&id) else { return };
        if !state.selecting {
            return;
        }
        state.selecting = false;
        if state.selection.is_some_and(|sel| sel.anchor == sel.head) {
            state.selection = None;
        }
        cx.notify();
    }
}

/// Leaves tmux's copy mode, if a wheel scroll put the session in it, so keys reach the shell.
fn leave_copy_mode(state: &mut TermState) {
    if std::mem::take(&mut state.copy_mode) {
        if let Some(name) = state.term.tmux() {
            tmux_cancel(name);
        }
    }
}

/// Width of one terminal cell: the advance of an `m` in the terminal font.
fn cell_width(window: &Window, family: &str, font_size: f32) -> Pixels {
    let font_id = window.text_system().resolve_font(&font(family.to_string()));
    window.text_system().advance(font_id, px(font_size), 'm').map_or(px(8.), |s| s.width)
}

/// Height of a terminal row for a text size, in whole pixels.
fn line_height(font_size: f32) -> f32 {
    (font_size * LINE_HEIGHT).round()
}

/// A screen row as one line of styled text, with the selected columns highlighted.
fn term_row(spans: &[Span], selection: Option<(usize, usize)>, line_h: f32, colours: Colours, family: &str) -> impl IntoElement {
    let text: String = spans.iter().map(|s| s.text.as_str()).collect();
    let runs = term_runs(spans, selection, colours, family);
    div().h(px(line_h)).whitespace_nowrap().child(StyledText::new(text).with_runs(runs))
}

/// The text runs of a row. Spans are split where the selection starts and ends.
fn term_runs(spans: &[Span], selection: Option<(usize, usize)>, colours: Colours, family: &str) -> Vec<TextRun> {
    let mut runs = Vec::new();
    let mut col = 0;
    for span in spans {
        let run = term_run(span, colours, family);
        let len = span.text.chars().count();
        let cells: usize = span.text.chars().map(char_width).sum();
        // The selected columns that fall in this span, as char offsets into it.
        let (from, to) = selection.map_or((0, 0), |(first, last)| {
            char_range(&span.text, first.saturating_sub(col), (last + 1).saturating_sub(col))
        });
        for (start, end, selected) in [(0, from, false), (from, to, true), (to, len, false)] {
            if start == end {
                continue;
            }
            let bytes: usize = span.text.chars().skip(start).take(end - start).map(char::len_utf8).sum();
            runs.push(TextRun {
                len: bytes,
                background_color: if selected { Some(colours.selection) } else { run.background_color },
                ..run.clone()
            });
        }
        col += cells;
    }
    runs
}

/// The chars of `text` that cover cells `first..end` (relative to its start), as an offset range.
/// A wide character is in the range if either of its cells is.
fn char_range(text: &str, first: usize, end: usize) -> (usize, usize) {
    let (mut from, mut to, mut cell) = (0, 0, 0);
    for (i, c) in text.chars().enumerate() {
        let w = char_width(c);
        if cell + w <= first {
            from = i + 1;
        }
        if cell < end {
            to = i + 1;
        }
        cell += w;
    }
    (from.min(to), to)
}

/// The text style of a span. Inverse video swaps its colours.
fn term_run(span: &Span, colours: Colours, family: &str) -> TextRun {
    let colour = |c: Option<u32>, default: Hsla| c.map_or(default, |c| rgb(c).into());
    let (fg, bg): (Hsla, Option<Hsla>) = if span.inverse {
        (colour(span.bg, colours.bg), Some(colour(span.fg, colours.fg)))
    } else {
        (colour(span.fg, colours.fg), span.bg.map(|c| rgb(c).into()))
    };
    TextRun {
        len: span.text.len(),
        font: Font {
            weight: if span.bold { FontWeight::BOLD } else { FontWeight::NORMAL },
            style: if span.italic { FontStyle::Italic } else { FontStyle::Normal },
            ..font(family.to_string())
        },
        color: fg,
        background_color: bg,
        underline: span.underline.then(|| UnderlineStyle { thickness: px(1.), color: None, wavy: false }),
        strikethrough: None,
    }
}

/// The cursor as a translucent block over its cell.
fn cursor_block(row: u16, col: u16, cell_w: Pixels, line_h: f32, colours: Colours) -> impl IntoElement {
    div()
        .absolute()
        .left(px(PAD_X) + cell_w * f32::from(col))
        .top(px(PAD_Y + line_h * f32::from(row)))
        .w(cell_w)
        .h(px(line_h))
        .bg(colours.cursor)
        .opacity(0.7)
}

/// The toolbar's text once the shell has exited.
fn exit_notice(code: Option<u32>) -> String {
    match code {
        Some(code) => format!("Process exited (code {code}). Press Enter or Restart."),
        None => "Process exited. Press Enter or Restart.".to_string(),
    }
}

/// Removes every bracketed-paste end marker, including ones that removing another would form.
fn strip_paste_end(text: &str) -> String {
    let mut text = text.to_string();
    while text.contains("\x1b[201~") {
        text = text.replace("\x1b[201~", "");
    }
    text
}

/// How many terminal cells a character covers: 0 for combining marks, 2 for wide (CJK, emoji) ones.
fn char_width(c: char) -> usize {
    match u32::from(c) {
        0x0300..=0x036F | 0x200B..=0x200F | 0x20D0..=0x20FF | 0xFE00..=0xFE0F => 0,
        0x1100..=0x115F
        | 0x2E80..=0xA4CF
        | 0xAC00..=0xD7A3
        | 0xF900..=0xFAFF
        | 0xFE30..=0xFE6F
        | 0xFF00..=0xFF60
        | 0xFFE0..=0xFFE6
        | 0x1F300..=0x1F64F
        | 0x1F900..=0x1F9FF
        | 0x20000..=0x3FFFD => 2,
        _ => 1,
    }
}

/// The text of each cell of a row. The second cell of a wide character is empty, and a combining
/// mark joins the cell before it, so indexes are screen columns.
fn row_cells(spans: &[Span]) -> Vec<String> {
    let mut cells: Vec<String> = Vec::new();
    for c in spans.iter().flat_map(|span| span.text.chars()) {
        match char_width(c) {
            0 => match cells.last_mut() {
                Some(last) => last.push(c),
                None => cells.push(c.to_string()),
            },
            2 => {
                cells.push(c.to_string());
                cells.push(String::new());
            }
            _ => cells.push(c.to_string()),
        }
    }
    cells
}

/// The selected text, one line per row, each without trailing spaces.
fn selected_text(screen: &Screen, selection: Selection, cols: usize) -> String {
    screen
        .rows
        .iter()
        .enumerate()
        .filter_map(|(row, spans)| {
            let (from, to) = selection.cols_in_row(row, cols)?;
            let cells = row_cells(spans);
            // A selection that starts on the second half of a wide character keeps the character.
            let from = if cells.get(from).is_some_and(String::is_empty) { from.saturating_sub(1) } else { from };
            let text: String = cells.iter().skip(from).take((to + 1).saturating_sub(from)).map(String::as_str).collect();
            Some(text.trim_end().to_string())
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The word under `cell`: the run of non-space cells around it on its row. `None` on a space.
fn word_selection(screen: &Screen, (row, col): (usize, usize)) -> Option<Selection> {
    let cells = row_cells(screen.rows.get(row)?);
    let blank = |cell: &String| cell.chars().next().is_some_and(char::is_whitespace);
    if blank(cells.get(col)?) {
        return None;
    }
    let start = cells[..col].iter().rposition(blank).map_or(0, |i| i + 1);
    let end = col + cells[col..].iter().position(blank).unwrap_or(cells.len() - col);
    Some(Selection { anchor: (row, start), head: (row, end - 1) })
}

/// `cell` moved onto a screen of `size` (cols, rows).
fn clamp_cell((row, col): (usize, usize), size: (u16, u16)) -> (usize, usize) {
    (row.min(usize::from(size.1).saturating_sub(1)), col.min(usize::from(size.0).saturating_sub(1)))
}

/// Arrow, Home and End keys as `ESC O x` instead of `ESC [ x`, which applications in cursor mode expect.
fn to_application_cursor(bytes: &mut [u8]) {
    if let [0x1b, b'[', b'A' | b'B' | b'C' | b'D' | b'H' | b'F'] = bytes {
        bytes[1] = b'O';
    }
}

/// The bytes a keystroke sends to the shell, or `None` if it sends nothing.
fn key_bytes(key: &str, key_char: Option<&str>, ctrl: bool, alt: bool, shift: bool) -> Option<Vec<u8>> {
    if let Some(seq) = special_key(key, shift, alt, ctrl) {
        return Some(seq.into_bytes());
    }
    if ctrl {
        let byte = match key {
            "space" => 0x00,
            "[" => 0x1b,
            "\\" => 0x1c,
            "]" => 0x1d,
            "^" => 0x1e,
            "/" | "_" => 0x1f,
            _ if key.len() == 1 && key.as_bytes()[0].is_ascii_alphabetic() => key.as_bytes()[0].to_ascii_lowercase() & 0x1f,
            _ => return None,
        };
        return Some(if alt { vec![0x1b, byte] } else { vec![byte] });
    }
    // Alt sends Esc before the character. Without text from the platform, the key name stands in for it.
    let text = match key_char {
        Some(text) => text.to_string(),
        None if key == "space" => " ".to_string(),
        None if key.chars().count() == 1 => if shift { key.to_uppercase() } else { key.to_string() },
        None => return None,
    };
    let bytes = if alt { format!("\x1b{text}") } else { text };
    Some(bytes.into_bytes())
}

/// The escape sequence for a key that isn't text. Modified cursor, editing and function keys carry
/// xterm's modifier number (Ctrl+Left is `ESC [ 1 ; 5 D`), which shells and editors read for word jumps.
fn special_key(key: &str, shift: bool, alt: bool, ctrl: bool) -> Option<String> {
    let modifier = 1 + u8::from(shift) + 2 * u8::from(alt) + 4 * u8::from(ctrl);
    // Keys whose meaning Alt changes by prefixing Esc.
    let esc = if alt { "\x1b" } else { "" };
    let csi = |letter: char| {
        if modifier > 1 { format!("\x1b[1;{modifier}{letter}") } else { format!("\x1b[{letter}") }
    };
    let ss3 = |letter: char| {
        if modifier > 1 { format!("\x1b[1;{modifier}{letter}") } else { format!("\x1bO{letter}") }
    };
    let tilde = |code: u8| {
        if modifier > 1 { format!("\x1b[{code};{modifier}~") } else { format!("\x1b[{code}~") }
    };
    Some(match key {
        "enter" => format!("{esc}\r"),
        "backspace" if ctrl => "\x08".to_string(),
        "backspace" => format!("{esc}\x7f"),
        "tab" if shift => "\x1b[Z".to_string(),
        "tab" => format!("{esc}\t"),
        "escape" => format!("{esc}\x1b"),
        "up" => csi('A'),
        "down" => csi('B'),
        "right" => csi('C'),
        "left" => csi('D'),
        "home" => csi('H'),
        "end" => csi('F'),
        "pageup" => tilde(5),
        "pagedown" => tilde(6),
        "delete" => tilde(3),
        "insert" => tilde(2),
        "f1" => ss3('P'),
        "f2" => ss3('Q'),
        "f3" => ss3('R'),
        "f4" => ss3('S'),
        "f5" => tilde(15),
        "f6" => tilde(17),
        "f7" => tilde(18),
        "f8" => tilde(19),
        "f9" => tilde(20),
        "f10" => tilde(21),
        "f11" => tilde(23),
        "f12" => tilde(24),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::{char_range, key_bytes, strip_paste_end, line_height, selected_text, term_runs, to_application_cursor, word_selection, Colours, Selection};
    use crate::terminal::{Screen, Span};
    use gpui::Hsla;

    fn span(text: &str) -> Span {
        Span { text: text.to_string(), fg: None, bg: None, bold: false, italic: false, underline: false, inverse: false }
    }

    /// A screen of unstyled rows.
    fn screen(rows: &[&str]) -> Screen {
        Screen {
            rows: rows.iter().map(|row| vec![span(row)]).collect(),
            cursor: None,
            title: String::new(),
            alternate: false,
        }
    }

    fn colours() -> Colours {
        Colours { fg: Hsla::default(), bg: Hsla::default(), selection: Hsla { h: 0.5, s: 0.5, l: 0.5, a: 0.5 }, cursor: Hsla::default() }
    }

    #[test]
    fn selected_text_spans_rows() {
        let screen = screen(&["hello world  ", "second line  ", "third"]);
        let forward = Selection { anchor: (0, 6), head: (1, 5) };
        assert_eq!(selected_text(&screen, forward, 13), "world\nsecond");
        let reversed = Selection { anchor: (1, 5), head: (0, 6) };
        assert_eq!(selected_text(&screen, reversed, 13), "world\nsecond");
    }

    #[test]
    fn selected_text_trims_each_row() {
        let screen = screen(&["ab   ", "cd   ", "ef   "]);
        let all = Selection { anchor: (0, 0), head: (2, 4) };
        assert_eq!(selected_text(&screen, all, 5), "ab\ncd\nef");
        let partial = Selection { anchor: (0, 3), head: (2, 0) };
        assert_eq!(selected_text(&screen, partial, 5), "\ncd\ne");
    }

    #[test]
    fn word_selection_covers_one_word() {
        let screen = screen(&["foo  bar-baz "]);
        assert_eq!(word_selection(&screen, (0, 7)), Some(Selection { anchor: (0, 5), head: (0, 11) }));
        assert_eq!(word_selection(&screen, (0, 0)), Some(Selection { anchor: (0, 0), head: (0, 2) }));
        assert_eq!(word_selection(&screen, (0, 3)), None);
    }

    #[test]
    fn term_runs_split_at_selection() {
        let colours = colours();
        let runs = term_runs(&[span("abcd")], Some((1, 2)), colours, "mono");
        assert_eq!(runs.iter().map(|r| r.len).collect::<Vec<_>>(), vec![1, 2, 1]);
        assert_eq!(runs[0].background_color, None);
        assert_eq!(runs[1].background_color, Some(colours.selection));
    }

    #[test]
    fn wide_chars_count_two_columns() {
        let screen = screen(&["a\u{4e2d}b cd"]);
        // Columns: a=0, wide=1..2, b=3, space=4, c=5, d=6.
        let sel = Selection { anchor: (0, 2), head: (0, 3) };
        assert_eq!(selected_text(&screen, sel, 8), "\u{4e2d}b");
        let sel = Selection { anchor: (0, 5), head: (0, 6) };
        assert_eq!(selected_text(&screen, sel, 8), "cd");
        assert_eq!(word_selection(&screen, (0, 3)), Some(Selection { anchor: (0, 0), head: (0, 3) }));
        assert_eq!(char_range("a\u{4e2d}b", 3, 4), (2, 3));
        assert_eq!(char_range("a\u{4e2d}b", 2, 3), (1, 2));
    }

    #[test]
    fn paste_end_marker_cannot_be_rebuilt() {
        assert_eq!(strip_paste_end("a\x1b[201~b"), "ab");
        assert_eq!(strip_paste_end("\x1b[2\x1b[201~01~"), "");
    }

    #[test]
    fn line_height_follows_font_size() {
        assert_eq!(line_height(13.), 21.);
        assert_eq!(line_height(10.), 16.);
        assert_eq!(line_height(24.), 38.);
    }

    #[test]
    fn application_cursor_keys() {
        let mut up = b"\x1b[A".to_vec();
        to_application_cursor(&mut up);
        assert_eq!(up, b"\x1bOA");
        let mut pgup = b"\x1b[5~".to_vec();
        to_application_cursor(&mut pgup);
        assert_eq!(pgup, b"\x1b[5~");
    }

    #[test]
    fn key_bytes_maps_keys() {
        assert_eq!(key_bytes("a", Some("a"), false, false, false), Some(b"a".to_vec()));
        assert_eq!(key_bytes("a", Some("A"), false, false, true), Some(b"A".to_vec()));
        assert_eq!(key_bytes("x", Some("x"), false, true, false), Some(b"\x1bx".to_vec()));
        assert_eq!(key_bytes("space", Some(" "), false, false, false), Some(b" ".to_vec()));
        assert_eq!(key_bytes("space", None, false, false, false), Some(b" ".to_vec()));
        assert_eq!(key_bytes("c", None, true, false, false), Some(vec![0x03]));
        assert_eq!(key_bytes("space", None, true, false, false), Some(vec![0x00]));
        assert_eq!(key_bytes("[", None, true, false, false), Some(vec![0x1b]));
        assert_eq!(key_bytes("enter", Some("\r"), false, false, false), Some(b"\r".to_vec()));
        assert_eq!(key_bytes("backspace", None, false, false, false), Some(vec![0x7f]));
        assert_eq!(key_bytes("escape", None, false, false, false), Some(vec![0x1b]));
        assert_eq!(key_bytes("tab", Some("\t"), false, false, false), Some(b"\t".to_vec()));
        assert_eq!(key_bytes("tab", None, false, false, true), Some(b"\x1b[Z".to_vec()));
        assert_eq!(key_bytes("up", None, false, false, false), Some(b"\x1b[A".to_vec()));
        assert_eq!(key_bytes("f1", None, false, false, false), Some(b"\x1bOP".to_vec()));
        assert_eq!(key_bytes("f5", None, false, false, false), Some(b"\x1b[15~".to_vec()));
        assert_eq!(key_bytes("shift", None, false, false, true), None);
        assert_eq!(key_bytes("1", None, true, false, false), None);
        assert_eq!(key_bytes("left", None, true, false, false), Some(b"\x1b[1;5D".to_vec()));
        assert_eq!(key_bytes("pageup", None, false, false, true), Some(b"\x1b[5;2~".to_vec()));
        assert_eq!(key_bytes("backspace", None, false, true, false), Some(b"\x1b\x7f".to_vec()));
        assert_eq!(key_bytes("c", None, true, true, false), Some(vec![0x1b, 0x03]));
        assert_eq!(key_bytes("f", None, false, true, false), Some(b"\x1bf".to_vec()));
        assert_eq!(key_bytes("f", None, false, true, true), Some(b"\x1bF".to_vec()));
    }
}
