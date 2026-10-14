//! The right-hand panel's Terminal tab: one shell per chat, started in the
//! chat's folder and drawn from `crate::terminal`'s screen snapshots.

use super::{button, save, ChatView};
use crate::{
    store::PanelTab,
    terminal::{
        tmux_cancel, tmux_kill, tmux_scroll, tmux_session_name, tmux_socket, Screen, Span, TermEvent, Terminal,
    },
};
use gpui::{
    canvas, div, font, prelude::*, px, rgb, Bounds, ClickEvent, ClipboardItem, Context, Font, FontStyle, FontWeight,
    KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, Pixels, Point, ScrollWheelEvent, SharedString,
    StyledText, TextRun, UnderlineStyle, Window,
};
use std::{cell::Cell, collections::HashMap, rc::Rc};

pub(super) const FONT_FAMILY: &str = "DejaVu Sans Mono";
/// Space between the text and the terminal's border.
const PADDING: f32 = 8.;
/// Size of a new shell before the panel has been laid out.
const DEFAULT_SIZE: (u16, u16) = (80, 24);
/// Lines scrolled per mouse wheel notch.
const WHEEL_LINES: f32 = 3.;
const BG: u32 = 0x11111b;
const FG: u32 = 0xcdd6f4;
const CURSOR: u32 = 0xf5e0dc;
const SELECTION: u32 = 0x45475a;

/// The chat sessions' shells and what the panel knows about them.
#[derive(Default)]
pub struct Terms {
    /// Each session's shell, or why it couldn't start. Started when its tab first shows.
    sessions: HashMap<String, Result<TermState, String>>,
    /// Bumped for every shell started, so events from a replaced shell are ignored.
    generations: u64,
    /// Keystrokes go to the shell (after clicking into it).
    focused: bool,
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

impl Terms {
    /// Hangs up the shells of sessions for which `live` is false.
    pub(super) fn retain(&mut self, live: impl Fn(&str) -> bool) {
        let gone: Vec<String> = self.sessions.keys().filter(|id| !live(id.as_str())).cloned().collect();
        for id in gone {
            self.hang_up(&id);
        }
    }

    /// Hangs up a session's shell. Its tmux session is killed too, so the shell ends.
    fn hang_up(&mut self, id: &str) {
        if let Some(Ok(state)) = self.sessions.remove(id) {
            if let Some(name) = state.term.tmux() {
                tmux_kill(name);
            }
        }
    }

    /// Columns and rows that fit the laid-out area, or `None` before the first layout.
    fn grid(&self, cell_w: Pixels, line_h: f32) -> Option<(u16, u16)> {
        let b = self.bounds.get();
        let (w, h) = (f32::from(b.size.width), f32::from(b.size.height));
        if w <= 0. || h <= 0. {
            return None;
        }
        let cols = ((w - 2. * PADDING) / f32::from(cell_w)).floor().max(2.) as u16;
        let rows = ((h - 2. * PADDING) / line_h).floor().max(2.) as u16;
        Some((cols, rows))
    }

    /// The `(row, col)` of the screen cell under `pos`, not yet clamped to the screen.
    fn cell_at(&self, pos: Point<Pixels>, line_h: f32) -> (usize, usize) {
        let origin = self.bounds.get().origin;
        let x = f32::from(pos.x - origin.x) - PADDING;
        let y = f32::from(pos.y - origin.y) - PADDING;
        let col = (x / f32::from(self.cell_w)).floor().max(0.) as usize;
        let row = (y / line_h).floor().max(0.) as usize;
        (row, col)
    }
}

impl ChatView {
    /// Terminal text size and row height, from the settings.
    fn term_metrics(&self) -> (f32, f32) {
        let size = self.settings.terminal_font_size;
        (size, line_height(size))
    }

    /// Show the Terminal tab with its shell focused.
    pub(super) fn show_terminal(&mut self, cx: &mut Context<Self>) {
        self.settings.hide_panel = false;
        self.settings.panel_tab = PanelTab::Terminal;
        self.save_settings();
        self.terms.focused = true;
        cx.notify();
    }

    /// Starts the chat's shell once its tab shows, and keeps it sized to the panel.
    pub(super) fn terminal_before_render(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.settings.hide_panel || self.settings.panel_tab != PanelTab::Terminal {
            return;
        }
        let Some((id, cwd)) = self.session().map(|s| (s.id.clone(), s.cwd.clone())) else { return };
        let (font_size, line_h) = self.term_metrics();
        let cell_w = cell_width(window, font_size);
        self.terms.cell_w = cell_w;
        let grid = self.terms.grid(cell_w, line_h);
        if !self.terms.sessions.contains_key(&id) {
            let size = grid.unwrap_or(DEFAULT_SIZE);
            self.terms.generations += 1;
            let generation = self.terms.generations;
            // Without tmux the shell ends when ArchonUI does.
            let tmux = self.settings.use_tmux.then(|| tmux_session_name(&id));
            let state = match Terminal::spawn(&cwd, size.0, size.1, tmux.as_deref()) {
                Ok((term, events)) => {
                    self.mark_terminal(&id);
                    let loop_id = id.clone();
                    cx.spawn(async move |this, cx| {
                        while let Ok(ev) = events.recv().await {
                            let live = this.update(cx, |this, cx| this.terminal_event(&loop_id, generation, ev, cx));
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
            self.terms.sessions.insert(id.clone(), state);
        }
        if let (Some(size), Some(Ok(state))) = (grid, self.terms.sessions.get_mut(&id)) {
            if state.size != size {
                state.term.resize(size.0, size.1);
                state.size = size;
                state.selection = None;
                state.screen = state.term.screen();
            }
        }
    }

    /// Applies an event from a shell. Returns false once that shell has been
    /// replaced, which ends its event loop.
    fn terminal_event(&mut self, id: &str, generation: u64, ev: TermEvent, cx: &mut Context<Self>) -> bool {
        let Some(Ok(state)) = self.terms.sessions.get_mut(id) else { return false };
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

    /// Records that the chat has a terminal, so it is kept without messages. A chat with
    /// no title yet is titled "Terminal" until its first message.
    fn mark_terminal(&mut self, id: &str) {
        let Some(s) = self.sessions.iter_mut().find(|s| s.id == id) else { return };
        if s.terminal {
            return;
        }
        s.terminal = true;
        if s.title.is_empty() {
            s.title = "Terminal".into();
        }
        save(s);
    }

    /// Hangs up the chat's shell, killing its tmux session; the next render starts a new one.
    pub(super) fn restart_terminal(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = self.session().map(|s| s.id.clone()) {
            self.terms.hang_up(&id);
        }
        cx.notify();
    }

    /// After the client exited: reattach if the tmux session still runs (it was only
    /// detached elsewhere), else start fresh.
    pub(super) fn reattach_terminal(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = self.session().map(|s| s.id.clone()) {
            // Dropping the exited client detaches only; the next render spawns with `-A`.
            self.terms.sessions.remove(&id);
        }
        cx.notify();
    }

    /// Hangs up a deleted session's shell and kills its tmux session, which may be from an
    /// earlier run and so have no shell in `terms`.
    pub(super) fn forget_terminal(&mut self, id: &str, cx: &mut Context<Self>) {
        self.terms.hang_up(id);
        tmux_kill(&tmux_session_name(id));
        self.term_blur(cx);
    }

    /// A click outside the terminal gives the keyboard back to the chat.
    pub(super) fn term_blur(&mut self, cx: &mut Context<Self>) {
        if self.terms.focused {
            self.terms.focused = false;
            cx.notify();
        }
    }

    fn write_terminal(&self, bytes: &[u8]) {
        let Some(id) = self.session().map(|s| &s.id) else { return };
        if let Some(Ok(state)) = self.terms.sessions.get(id) {
            state.term.write(bytes);
        }
    }

    /// Keys for the shell while the terminal is focused. Returns true if consumed.
    pub(super) fn terminal_key(&mut self, ev: &KeyDownEvent, cx: &mut Context<Self>) -> bool {
        let ks = &ev.keystroke;
        let m = ks.modifiers;
        if !self.terms.focused || self.settings.hide_panel || self.settings.panel_tab != PanelTab::Terminal || m.platform {
            return false;
        }
        // Chat shortcuts stay with the chat.
        if m.control && matches!(ks.key.as_str(), "n" | "i" | ",") {
            return false;
        }
        let Some(id) = self.session().map(|s| s.id.clone()) else { return false };
        let Some(Ok(state)) = self.terms.sessions.get_mut(&id) else { return false };
        if m.control && m.shift && ks.key == "c" {
            if let Some(sel) = state.selection {
                let text = selected_text(&state.screen, sel, usize::from(state.size.0));
                cx.write_to_clipboard(ClipboardItem::new_string(text));
            }
        } else if m.control && m.shift && ks.key == "v" {
            state.selection = None;
            leave_copy_mode(state);
            if let Some(text) = cx.read_from_clipboard().and_then(|c| c.text()) {
                let bytes = if state.term.bracketed_paste() {
                    format!("\x1b[200~{text}\x1b[201~")
                } else {
                    text.replace('\n', "\r")
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

    /// Mouse wheel: scrolls tmux's copy mode in a tmux session. Otherwise it scrolls the
    /// shell's history, or sends arrow keys when the shell is on a full-screen app's alternate screen.
    fn terminal_scroll(&mut self, ev: &ScrollWheelEvent, cx: &mut Context<Self>) {
        let (_, line_h) = self.term_metrics();
        let Some(id) = self.session().map(|s| s.id.clone()) else { return };
        let Some(Ok(state)) = self.terms.sessions.get_mut(&id) else { return };
        let notches = f32::from(ev.delta.pixel_delta(px(line_h)).y) / line_h;
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
        let cell = self.terms.cell_at(ev.position, line_h);
        let Some(id) = self.session().map(|s| s.id.clone()) else { return };
        let Some(Ok(state)) = self.terms.sessions.get_mut(&id) else { return };
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
    pub(super) fn term_drag_move(&mut self, pos: Point<Pixels>, cx: &mut Context<Self>) {
        let (_, line_h) = self.term_metrics();
        let cell = self.terms.cell_at(pos, line_h);
        let Some(id) = self.session().map(|s| s.id.clone()) else { return };
        let Some(Ok(state)) = self.terms.sessions.get_mut(&id) else { return };
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
    pub(super) fn term_drag_end(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.session().map(|s| s.id.clone()) else { return };
        let Some(Ok(state)) = self.terms.sessions.get_mut(&id) else { return };
        if !state.selecting {
            return;
        }
        state.selecting = false;
        if state.selection.is_some_and(|sel| sel.anchor == sel.head) {
            state.selection = None;
        }
        cx.notify();
    }

    /// The Terminal tab: the chat's shell, drawn from its latest screen snapshot.
    pub(super) fn render_terminal(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(session) = self.session() else {
            return div().flex_1().min_h_0().child(
                div().px_4().py_2().text_xs().text_color(rgb(0x7f849c)).child("No chat selected."),
            );
        };
        let (font_size, line_h) = self.term_metrics();
        let cell_w = cell_width(window, font_size);
        let focused = self.terms.focused;
        let state = self.terms.sessions.get(&session.id);
        // A shell that has exited says so in the toolbar, so the terminal keeps its size.
        let (title, title_color): (SharedString, u32) = match state {
            Some(Ok(TermState { exited: Some(code), .. })) => (exit_notice(*code).into(), 0xf38ba8),
            Some(Ok(s)) if !s.screen.title.is_empty() => (s.screen.title.clone().into(), 0x9399b2),
            _ => (session.cwd.display().to_string().into(), 0x9399b2),
        };
        let error: Option<SharedString> = match state {
            Some(Err(e)) => Some(e.clone().into()),
            _ => None,
        };
        let (rows, cursor) = match state {
            Some(Ok(s)) => {
                let cols = usize::from(s.size.0);
                (
                    s.screen
                        .rows
                        .iter()
                        .enumerate()
                        .map(|(row, spans)| {
                            term_row(spans, s.selection.and_then(|sel| sel.cols_in_row(row, cols)), line_h)
                        })
                        .collect::<Vec<_>>(),
                    s.screen.cursor.filter(|_| focused).map(|(row, col)| cursor_block(row, col, cell_w, line_h)),
                )
            }
            _ => (Vec::new(), None),
        };

        let bounds = self.terms.bounds.clone();
        let page = div()
            .id("term-page")
            .flex_1()
            .min_h_0()
            .relative()
            .overflow_hidden()
            .p(px(PADDING))
            .bg(rgb(BG))
            .text_size(px(font_size))
            .line_height(px(line_h))
            .border_2()
            .border_color(if focused { rgb(0x89b4fa) } else { rgb(0x181825) })
            // Records where the terminal is, so the shell can be sized to it.
            .child(canvas(move |b, _, _| bounds.set(b), |_, _, _, _| {}).absolute().top_0().left_0().size_full())
            .on_mouse_down(MouseButton::Left, cx.listener(|this, ev: &MouseDownEvent, window, cx| {
                cx.stop_propagation();
                window.focus(&this.focus);
                this.preview_blur(cx);
                this.terms.focused = true;
                this.term_press(ev);
                cx.notify();
            }))
            .on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, _, cx| {
                if ev.pressed_button == Some(MouseButton::Left) {
                    this.term_drag_move(ev.position, cx);
                }
            }))
            .on_scroll_wheel(cx.listener(|this, ev: &ScrollWheelEvent, _, cx| this.terminal_scroll(ev, cx)))
            .children(rows)
            .children(cursor);

        let tmux = match state {
            Some(Ok(s)) => s.term.tmux().map(str::to_owned),
            _ => None,
        };
        let row = div()
            .flex()
            .items_center()
            .gap_1()
            .children(tmux.as_ref().map(|name| {
                div().text_xs().text_color(rgb(0x7f849c)).child(format!("tmux · {name}"))
            }))
            .child(div().flex_1().min_w_0().text_xs().text_color(rgb(title_color)).truncate().child(title))
            .child(button("term-clear", "Clear").on_click(cx.listener(|this, _: &ClickEvent, _, _| {
                // Ctrl+L clears the shell's screen.
                this.write_terminal(b"\x0c");
            })))
            .child(button("term-restart", "Restart").on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                this.restart_terminal(cx);
            })));
        let toolbar = div()
            .px_2()
            .py_2()
            .flex()
            .flex_col()
            .gap_1()
            .child(row)
            // The command that attaches to this session from another terminal. Click to copy it.
            .children(tmux.map(|name| {
                let attach = format!("tmux -L {} attach -t {name}", tmux_socket());
                div()
                    .id("term-attach")
                    .text_xs()
                    .text_color(rgb(0x7f849c))
                    .truncate()
                    .child(format!("attach: {attach}"))
                    .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(attach.clone()));
                    }))
            }));

        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(toolbar)
            .children(error.map(|text| {
                div().mx_2().mb_1().px_2().py_1().rounded_md().bg(rgb(0x4a2530)).text_xs().child(text)
            }))
            .child(page)
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
pub(super) fn cell_width(window: &Window, font_size: f32) -> Pixels {
    let font_id = window.text_system().resolve_font(&font(FONT_FAMILY));
    window.text_system().advance(font_id, px(font_size), 'm').map_or(px(8.), |s| s.width)
}

/// Height of a terminal row for a text size: 1.4 times the size, in whole pixels.
fn line_height(font_size: f32) -> f32 {
    (font_size * 1.4).round()
}

/// A screen row as one line of styled text, with the selected columns highlighted.
fn term_row(spans: &[Span], selection: Option<(usize, usize)>, line_h: f32) -> impl IntoElement {
    let text: String = spans.iter().map(|s| s.text.as_str()).collect();
    let runs = term_runs(spans, selection);
    div().h(px(line_h)).whitespace_nowrap().child(StyledText::new(text).with_runs(runs))
}

/// The text runs of a row. Spans are split where the selection starts and ends.
fn term_runs(spans: &[Span], selection: Option<(usize, usize)>) -> Vec<TextRun> {
    let mut runs = Vec::new();
    let mut col = 0;
    for span in spans {
        let run = term_run(span);
        let len = span.text.chars().count();
        // The selected columns that fall in this span, as char offsets into it.
        let (from, to) = selection.map_or((0, 0), |(first, last)| {
            (first.saturating_sub(col).min(len), (last + 1).saturating_sub(col).min(len))
        });
        for (start, end, selected) in [(0, from, false), (from, to, true), (to, len, false)] {
            if start == end {
                continue;
            }
            let bytes: usize = span.text.chars().skip(start).take(end - start).map(char::len_utf8).sum();
            runs.push(TextRun {
                len: bytes,
                background_color: if selected { Some(rgb(SELECTION).into()) } else { run.background_color },
                ..run.clone()
            });
        }
        col += len;
    }
    runs
}

/// The text style of a span. Inverse video swaps its colors.
fn term_run(span: &Span) -> TextRun {
    let (fg, bg) = if span.inverse {
        (span.bg.unwrap_or(BG), Some(span.fg.unwrap_or(FG)))
    } else {
        (span.fg.unwrap_or(FG), span.bg)
    };
    TextRun {
        len: span.text.len(),
        font: Font {
            weight: if span.bold { FontWeight::BOLD } else { FontWeight::NORMAL },
            style: if span.italic { FontStyle::Italic } else { FontStyle::Normal },
            ..font(FONT_FAMILY)
        },
        color: rgb(fg).into(),
        background_color: bg.map(|c| rgb(c).into()),
        underline: span.underline.then(|| UnderlineStyle { thickness: px(1.), color: None, wavy: false }),
        strikethrough: None,
    }
}

/// The cursor as a translucent block over its cell.
fn cursor_block(row: u16, col: u16, cell_w: Pixels, line_h: f32) -> impl IntoElement {
    div()
        .absolute()
        .left(px(PADDING) + cell_w * f32::from(col))
        .top(px(PADDING + line_h * f32::from(row)))
        .w(cell_w)
        .h(px(line_h))
        .bg(rgb(CURSOR))
        .opacity(0.7)
}

/// The toolbar's text once the shell has exited.
fn exit_notice(code: Option<u32>) -> String {
    match code {
        Some(code) => format!("Process exited (code {code}). Press Enter or Restart."),
        None => "Process exited. Press Enter or Restart.".to_string(),
    }
}

/// The characters of a row, one per cell.
fn row_chars(spans: &[Span]) -> impl Iterator<Item = char> + '_ {
    spans.iter().flat_map(|span| span.text.chars())
}

/// The selected text, one line per row, each without trailing spaces.
fn selected_text(screen: &Screen, selection: Selection, cols: usize) -> String {
    screen
        .rows
        .iter()
        .enumerate()
        .filter_map(|(row, spans)| {
            let (from, to) = selection.cols_in_row(row, cols)?;
            let text: String = row_chars(spans).skip(from).take((to + 1).saturating_sub(from)).collect();
            Some(text.trim_end().to_string())
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The word under `cell`: the run of non-space cells around it on its row. `None` on a space.
fn word_selection(screen: &Screen, (row, col): (usize, usize)) -> Option<Selection> {
    let chars: Vec<char> = row_chars(screen.rows.get(row)?).collect();
    if chars.get(col)?.is_whitespace() {
        return None;
    }
    let start = chars[..col].iter().rposition(|c| c.is_whitespace()).map_or(0, |i| i + 1);
    let end = col + chars[col..].iter().position(|c| c.is_whitespace()).unwrap_or(chars.len() - col);
    Some(Selection { anchor: (row, start), head: (row, end - 1) })
}

/// `cell` moved onto a screen of `size` (cols, rows).
fn clamp_cell((row, col): (usize, usize), size: (u16, u16)) -> (usize, usize) {
    (row.min(usize::from(size.1).saturating_sub(1)), col.min(usize::from(size.0).saturating_sub(1)))
}

/// The bytes a keystroke sends to the shell, or `None` if it sends nothing.
/// Arrow, Home and End keys as `ESC O x` instead of `ESC [ x`.
fn to_application_cursor(bytes: &mut [u8]) {
    if let [0x1b, b'[', b'A' | b'B' | b'C' | b'D' | b'H' | b'F'] = bytes {
        bytes[1] = b'O';
    }
}

fn key_bytes(key: &str, key_char: Option<&str>, ctrl: bool, alt: bool, shift: bool) -> Option<Vec<u8>> {
    if let Some(seq) = special_key(key, shift) {
        return Some(seq.as_bytes().to_vec());
    }
    if ctrl {
        let byte = match key {
            "space" => 0x00,
            "[" => 0x1b,
            _ if key.len() == 1 && key.as_bytes()[0].is_ascii_alphabetic() => key.as_bytes()[0].to_ascii_lowercase() & 0x1f,
            _ => return None,
        };
        return Some(vec![byte]);
    }
    let text = key_char.or((key == "space").then_some(" "))?;
    let bytes = if alt { format!("\x1b{text}") } else { text.to_string() };
    Some(bytes.into_bytes())
}

/// The escape sequence for a key that isn't text.
fn special_key(key: &str, shift: bool) -> Option<&'static str> {
    Some(match key {
        "enter" => "\r",
        "backspace" => "\x7f",
        "tab" if shift => "\x1b[Z",
        "tab" => "\t",
        "escape" => "\x1b",
        "up" => "\x1b[A",
        "down" => "\x1b[B",
        "right" => "\x1b[C",
        "left" => "\x1b[D",
        "home" => "\x1b[H",
        "end" => "\x1b[F",
        "pageup" => "\x1b[5~",
        "pagedown" => "\x1b[6~",
        "delete" => "\x1b[3~",
        "insert" => "\x1b[2~",
        "f1" => "\x1bOP",
        "f2" => "\x1bOQ",
        "f3" => "\x1bOR",
        "f4" => "\x1bOS",
        "f5" => "\x1b[15~",
        "f6" => "\x1b[17~",
        "f7" => "\x1b[18~",
        "f8" => "\x1b[19~",
        "f9" => "\x1b[20~",
        "f10" => "\x1b[21~",
        "f11" => "\x1b[23~",
        "f12" => "\x1b[24~",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::{key_bytes, line_height, selected_text, term_runs, to_application_cursor, word_selection, Selection, SELECTION};
    use crate::terminal::{Screen, Span};
    use gpui::rgb;

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
        let runs = term_runs(&[span("abcd")], Some((1, 2)));
        assert_eq!(runs.iter().map(|r| r.len).collect::<Vec<_>>(), vec![1, 2, 1]);
        assert_eq!(runs[0].background_color, None);
        assert_eq!(runs[1].background_color, Some(rgb(SELECTION).into()));
    }

    #[test]
    fn line_height_follows_font_size() {
        assert_eq!(line_height(13.), 18.);
        assert_eq!(line_height(10.), 14.);
        assert_eq!(line_height(24.), 34.);
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
    }
}
