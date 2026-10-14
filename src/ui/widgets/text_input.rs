//! A text field for single-line and soft-wrapped multi-line input, with IME through
//! `EntityInputHandler`. Editing rules are in [`edit`]; this file does layout, hit-testing and events.
//! Call [`init`] once at startup to bind the keys. Spec: `docs/design/chat.md` §4.

mod edit;

use std::ops::Range;

use gpui::{
    actions, black, div, fill, point, prelude::*, relative, size, App, AvailableSpace,
    Bounds, ClipboardItem, Context, CursorStyle, Div, DispatchPhase, Element, ElementInputHandler,
    Entity, EntityInputHandler, EventEmitter, Font, FocusHandle, GlobalElementId, Hsla,
    InspectorElementId, IntoElement, KeyBinding, LayoutId, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Pixels, Point, RenderOnce, ScrollWheelEvent, SharedString,
    Style, Subscription, TextAlign, TextRun, UTF16Selection, Window, WrappedLine,
};
use crate::ui::fonts;
use crate::ui::scale::px;

use crate::editor::buffer::Pos;
use crate::ui::theme::Theme;
use edit::{char_to_utf16, enter_action, utf16_to_char, Edit, EnterAction};

/// Key context the bindings from [`init`] are scoped to.
const CONTEXT: &str = "TextInput";

actions!(
    text_input,
    [
        Backspace,
        Delete,
        DeleteWordLeft,
        DeleteWordRight,
        Left,
        Right,
        Up,
        Down,
        WordLeft,
        WordRight,
        SelectLeft,
        SelectRight,
        SelectUp,
        SelectDown,
        SelectWordLeft,
        SelectWordRight,
        Home,
        End,
        SelectHome,
        SelectEnd,
        SelectAll,
        CopySelection,
        CutSelection,
        PasteText,
        Undo,
        Redo,
        Enter,
        ShiftEnter,
        SecondaryEnter,
    ]
);

/// Binds the text input keys. Ctrl is the word and clipboard modifier on Linux and Windows, Cmd on macOS
/// (word moves use Alt there, as the native fields do).
pub fn init(cx: &mut App) {
    let word = if cfg!(target_os = "macos") { "alt" } else { "ctrl" };
    cx.bind_keys([
        KeyBinding::new("backspace", Backspace, Some(CONTEXT)),
        KeyBinding::new("delete", Delete, Some(CONTEXT)),
        KeyBinding::new(&format!("{word}-backspace"), DeleteWordLeft, Some(CONTEXT)),
        KeyBinding::new(&format!("{word}-delete"), DeleteWordRight, Some(CONTEXT)),
        KeyBinding::new("left", Left, Some(CONTEXT)),
        KeyBinding::new("right", Right, Some(CONTEXT)),
        KeyBinding::new("up", Up, Some(CONTEXT)),
        KeyBinding::new("down", Down, Some(CONTEXT)),
        KeyBinding::new(&format!("{word}-left"), WordLeft, Some(CONTEXT)),
        KeyBinding::new(&format!("{word}-right"), WordRight, Some(CONTEXT)),
        KeyBinding::new("shift-left", SelectLeft, Some(CONTEXT)),
        KeyBinding::new("shift-right", SelectRight, Some(CONTEXT)),
        KeyBinding::new("shift-up", SelectUp, Some(CONTEXT)),
        KeyBinding::new("shift-down", SelectDown, Some(CONTEXT)),
        KeyBinding::new(&format!("shift-{word}-left"), SelectWordLeft, Some(CONTEXT)),
        KeyBinding::new(&format!("shift-{word}-right"), SelectWordRight, Some(CONTEXT)),
        KeyBinding::new("home", Home, Some(CONTEXT)),
        KeyBinding::new("end", End, Some(CONTEXT)),
        KeyBinding::new("shift-home", SelectHome, Some(CONTEXT)),
        KeyBinding::new("shift-end", SelectEnd, Some(CONTEXT)),
        KeyBinding::new("secondary-a", SelectAll, Some(CONTEXT)),
        KeyBinding::new("secondary-c", CopySelection, Some(CONTEXT)),
        KeyBinding::new("secondary-x", CutSelection, Some(CONTEXT)),
        KeyBinding::new("secondary-v", PasteText, Some(CONTEXT)),
        KeyBinding::new("secondary-z", Undo, Some(CONTEXT)),
        KeyBinding::new("secondary-shift-z", Redo, Some(CONTEXT)),
        KeyBinding::new("enter", Enter, Some(CONTEXT)),
        KeyBinding::new("shift-enter", ShiftEnter, Some(CONTEXT)),
        KeyBinding::new("secondary-enter", SecondaryEnter, Some(CONTEXT)),
    ]);
}

/// Emitted by a [`TextInput`] entity. Programmatic [`TextInput::set_text`] does not emit `Changed`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextInputEvent {
    /// The text changed by typing, pasting, deleting or IME.
    Changed,
    /// Enter (or Ctrl/Cmd+Enter) was pressed where it sends. The owner decides what submitting means.
    Submit,
    /// Focus left the field.
    Blur,
}

/// Layout and editing options. Sizes are design px, which are logical px here.
#[derive(Clone)]
pub struct TextInputConfig {
    pub multiline: bool,
    /// Rows the field has when empty. Single-line fields are always one row.
    pub min_lines: usize,
    /// Rows before the field scrolls. `None` grows with the content.
    pub max_lines: Option<usize>,
    /// Longest text in chars.
    pub max_len: Option<usize>,
    pub placeholder: SharedString,
    /// Multi-line only. `true`: Enter sends and Shift+Enter breaks the line. `false`: the keys swap.
    /// Ctrl/Cmd+Enter always sends.
    pub enter_submits: bool,
    pub font_size: Pixels,
    pub line_height: Pixels,
    /// Padding inside the field.
    pub pad_top: Pixels,
    pub pad_x: Pixels,
    pub pad_bottom: Pixels,
}

impl Default for TextInputConfig {
    fn default() -> Self {
        Self {
            multiline: false,
            min_lines: 1,
            max_lines: None,
            max_len: None,
            placeholder: SharedString::default(),
            enter_submits: true,
            font_size: px(14.),
            line_height: px(20.),
            pad_top: px(0.),
            pad_x: px(0.),
            pad_bottom: px(0.),
        }
    }
}

/// The text field's state: text, selection, scroll and the last layout. Create it with `cx.new`.
pub struct TextInput {
    focus: FocusHandle,
    edit: Edit,
    config: TextInputConfig,
    /// Char range of the IME composition, if one is open.
    marked: Option<Range<usize>>,
    /// Set while the mouse drags a selection.
    selecting: bool,
    /// Horizontal position that Up and Down keep.
    goal_x: Option<Pixels>,
    scroll_y: Pixels,
    /// Scroll to the caret on the next paint. Set by edits and moves, cleared by the wheel.
    follow_caret: bool,
    layout: Option<Layout>,
    _blur: Subscription,
}

impl EventEmitter<TextInputEvent> for TextInput {}

/// What the last paint laid out. Mouse and IME queries read it.
#[derive(Clone)]
struct Layout {
    /// The visible content area, in window coordinates.
    content: Bounds<Pixels>,
    scroll: Pixels,
    lines: Vec<WrappedLine>,
    /// Top of each hard line, in content coordinates before scrolling.
    tops: Vec<Pixels>,
    line_height: Pixels,
}

impl Layout {
    /// Content-space top-left of the caret at `pos`, before scrolling.
    fn caret(&self, pos: Pos) -> Point<Pixels> {
        let Some(line) = self.lines.get(pos.line) else {
            return point(px(0.), px(0.));
        };
        let at = line.text.char_indices().nth(pos.col).map_or(line.text.len(), |(byte, _)| byte);
        let offset = if line.text.is_empty() {
            Point::default()
        } else {
            line.position_for_index(at, self.line_height).unwrap_or_default()
        };
        point(offset.x, self.tops[pos.line] + offset.y)
    }

    /// The position under `local`, a content-space point before scrolling.
    fn pos_at(&self, local: Point<Pixels>) -> Pos {
        if self.lines.is_empty() {
            return Pos::default();
        }
        let line = self.tops.partition_point(|top| *top <= local.y).saturating_sub(1);
        let wrapped = &self.lines[line];
        let byte = if wrapped.text.is_empty() {
            0
        } else {
            let within = point(local.x, local.y - self.tops[line]);
            wrapped
                .closest_index_for_position(within, self.line_height)
                .unwrap_or_else(|index| index)
        };
        let col = wrapped.text[..byte.min(wrapped.text.len())].chars().count();
        Pos { line, col }
    }

    /// Maps a window point into content space before scrolling.
    fn to_local(&self, window_point: Point<Pixels>) -> Point<Pixels> {
        point(
            window_point.x - self.content.left(),
            window_point.y - self.content.top() + self.scroll,
        )
    }

    fn total_height(&self) -> Pixels {
        match (self.tops.last(), self.lines.last()) {
            (Some(top), Some(line)) => *top + self.line_height * (line.wrap_boundaries().len() + 1) as f32,
            _ => px(0.),
        }
    }
}

/// The caret's motion, from the arrow, word, home and end keys.
#[derive(Clone, Copy)]
enum Cmd {
    Left(bool),
    Right(bool),
    Up(bool),
    Down(bool),
    WordLeft(bool),
    WordRight(bool),
    Home(bool),
    End(bool),
    SelectAll,
    Backspace,
    Delete,
    DeleteWordLeft,
    DeleteWordRight,
    Copy,
    Cut,
    Paste,
    Undo,
    Redo,
    Enter { shift: bool, secondary: bool },
}

impl TextInput {
    /// An empty field. `window` is needed to watch focus for [`TextInputEvent::Blur`].
    pub fn new(config: TextInputConfig, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        let weak = cx.weak_entity();
        let blur = window.on_focus_out(&focus, cx, move |_, _, cx| {
            weak.update(cx, |_, cx| cx.emit(TextInputEvent::Blur)).ok();
        });
        Self {
            focus,
            edit: Edit::new("", config.multiline, config.max_len),
            config,
            marked: None,
            selecting: false,
            goal_x: None,
            scroll_y: px(0.),
            follow_caret: true,
            layout: None,
            _blur: blur,
        }
    }

    pub fn text(&self) -> String {
        self.edit.text()
    }

    /// Replaces the text and puts the caret at its end. Doesn't emit `Changed`.
    pub fn set_text(&mut self, text: &str, cx: &mut Context<Self>) {
        self.edit.select_all();
        self.edit.insert_text(text);
        self.edit.buffer_mut().move_doc_end(false);
        self.follow_caret = true;
        cx.notify();
    }

    /// Replaces the text shown while the field is empty.
    pub fn set_placeholder(&mut self, placeholder: SharedString, cx: &mut Context<Self>) {
        self.config.placeholder = placeholder;
        cx.notify();
    }

    pub fn focus_handle(&self) -> FocusHandle {
        self.focus.clone()
    }

    pub fn focus(&self, window: &mut Window) {
        window.focus(&self.focus);
    }

    /// Runs one command. Up and Down keep the goal column; every other command clears it.
    fn run(&mut self, cmd: Cmd, cx: &mut Context<Self>) {
        if let Cmd::Up(select) = cmd {
            return self.vertical(-1.0, select, cx);
        }
        if let Cmd::Down(select) = cmd {
            return self.vertical(1.0, select, cx);
        }
        self.goal_x = None;
        if let Cmd::Enter { shift, secondary } = cmd {
            let action = enter_action(self.config.multiline, self.config.enter_submits, shift, secondary);
            match action {
                EnterAction::Submit => cx.emit(TextInputEvent::Submit),
                EnterAction::Newline => return self.insert_changed("\n", cx),
                EnterAction::Ignored => {}
            }
            return self.moved(cx);
        }
        let before = self.edit.text();
        match cmd {
            Cmd::Left(select) => self.edit.buffer_mut().move_left(select),
            Cmd::Right(select) => self.edit.buffer_mut().move_right(select),
            Cmd::WordLeft(select) => self.edit.buffer_mut().move_word_left(select),
            Cmd::WordRight(select) => self.edit.buffer_mut().move_word_right(select),
            Cmd::Home(select) => self.edit.buffer_mut().move_home(select),
            Cmd::End(select) => self.edit.buffer_mut().move_end(select),
            Cmd::SelectAll => self.edit.select_all(),
            Cmd::Backspace => self.edit.backspace(),
            Cmd::Delete => self.edit.delete(),
            Cmd::DeleteWordLeft => self.edit.delete_word_left(),
            Cmd::DeleteWordRight => self.edit.delete_word_right(),
            Cmd::Copy => {
                if let Some(text) = self.edit.copy() {
                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                }
            }
            Cmd::Cut => {
                if let Some(text) = self.edit.cut() {
                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                }
            }
            Cmd::Paste => {
                if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                    self.edit.insert_text(&text);
                }
            }
            Cmd::Undo => {
                self.edit.undo();
            }
            Cmd::Redo => {
                self.edit.redo();
            }
            Cmd::Up(_) | Cmd::Down(_) | Cmd::Enter { .. } => unreachable!("handled above"),
        }
        if self.edit.text() != before {
            cx.emit(TextInputEvent::Changed);
        }
        self.moved(cx);
    }

    /// Inserts `text` at the caret and reports the change.
    fn insert_changed(&mut self, text: &str, cx: &mut Context<Self>) {
        self.edit.insert_text(text);
        cx.emit(TextInputEvent::Changed);
        self.moved(cx);
    }

    /// Up and Down move by visual row, keeping the horizontal position.
    fn vertical(&mut self, rows: f32, select: bool, cx: &mut Context<Self>) {
        let Some(layout) = self.layout.as_ref() else {
            return;
        };
        let caret = layout.caret(self.edit.buffer().cursor());
        let x = *self.goal_x.get_or_insert(caret.x);
        let y = caret.y + layout.line_height * rows;
        let target = if y < px(0.) {
            Pos::default()
        } else if y >= layout.total_height() {
            self.edit.buffer().pos_of(self.edit.char_len())
        } else {
            layout.pos_at(point(x, y))
        };
        self.edit.buffer_mut().set_cursor(target, select);
        self.moved(cx);
    }

    /// Asks for the caret to be scrolled into view on the next paint, and redraws.
    fn moved(&mut self, cx: &mut Context<Self>) {
        self.follow_caret = true;
        cx.notify();
    }

    fn pointer_down(&mut self, position: Point<Pixels>, click_count: usize, shift: bool, cx: &mut Context<Self>) {
        let Some(pos) = self.layout.as_ref().map(|layout| layout.pos_at(layout.to_local(position))) else {
            return;
        };
        if click_count >= 2 {
            self.edit.buffer_mut().select_word_at(pos);
        } else {
            self.edit.buffer_mut().set_cursor(pos, shift);
        }
        self.selecting = true;
        self.goal_x = None;
        self.moved(cx);
    }

    fn drag_to(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(pos) = self.layout.as_ref().map(|layout| layout.pos_at(layout.to_local(position))) else {
            return;
        };
        self.edit.buffer_mut().set_cursor(pos, true);
        self.moved(cx);
    }

    /// Scrolls the field by `dy` px. Returns whether it moved, so the wheel can pass through at the ends.
    fn scroll_by(&mut self, dy: Pixels, cx: &mut Context<Self>) -> bool {
        let Some(layout) = self.layout.as_ref() else {
            return false;
        };
        let max = (layout.total_height() - layout.content.size.height).max(px(0.));
        let next = (self.scroll_y - dy).max(px(0.)).min(max);
        if next == self.scroll_y {
            return false;
        }
        self.scroll_y = next;
        self.follow_caret = false;
        cx.notify();
        true
    }

    /// Chars at `range` (UTF-16 offsets from the platform) in the current text.
    fn chars_of(&self, range: Range<usize>) -> Range<usize> {
        let text = self.edit.text();
        utf16_to_char(&text, range.start)..utf16_to_char(&text, range.end)
    }

    fn utf16_of(&self, chars: Range<usize>) -> Range<usize> {
        let text = self.edit.text();
        char_to_utf16(&text, chars.start)..char_to_utf16(&text, chars.end)
    }

    /// Notifies after an IME change and reports it as an edit.
    fn ime_changed(&mut self, cx: &mut Context<Self>) {
        self.follow_caret = true;
        self.goal_x = None;
        cx.emit(TextInputEvent::Changed);
        cx.notify();
    }
}

impl EntityInputHandler for TextInput {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let chars = self.chars_of(range_utf16);
        actual_range.replace(self.utf16_of(chars.clone()));
        Some(self.edit.text().chars().skip(chars.start).take(chars.end - chars.start).collect())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let chars = self.edit.char_selection();
        let cursor = self.edit.buffer().cursor();
        let cursor_at = self.edit.buffer().char_index(cursor);
        Some(UTF16Selection {
            range: self.utf16_of(chars.clone()),
            reversed: chars.start < chars.end && cursor_at == chars.start,
        })
    }

    fn marked_text_range(&self, _window: &mut Window, _cx: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked.clone().map(|chars| self.utf16_of(chars))
    }

    fn unmark_text(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {
        self.marked = None;
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = match range_utf16 {
            Some(range) => self.chars_of(range),
            None => self.marked.clone().unwrap_or_else(|| self.edit.char_selection()),
        };
        self.edit.replace_range(range, new_text);
        self.marked = None;
        self.ime_changed(cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_utf16: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = match range_utf16 {
            Some(range) => self.chars_of(range),
            None => self.marked.clone().unwrap_or_else(|| self.edit.char_selection()),
        };
        let inserted = self.edit.replace_range(range, new_text);
        self.marked = (!inserted.is_empty()).then(|| inserted.clone());
        if let Some(selected) = new_selected_utf16 {
            let within = utf16_to_char(new_text, selected.start)..utf16_to_char(new_text, selected.end);
            self.edit.select_chars(inserted.start + within.start..inserted.start + within.end);
        }
        self.ime_changed(cx);
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        _bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let layout = self.layout.as_ref()?;
        let chars = self.chars_of(range_utf16);
        let caret = layout.caret(self.edit.buffer().pos_of(chars.start));
        let origin = point(
            layout.content.left() + caret.x,
            layout.content.top() + caret.y - layout.scroll,
        );
        Some(Bounds::new(origin, size(px(2.), layout.line_height)))
    }

    fn character_index_for_point(
        &mut self,
        position: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        let layout = self.layout.as_ref()?;
        let pos = layout.pos_at(layout.to_local(position));
        let chars = self.edit.buffer().char_index(pos);
        Some(self.utf16_of(chars..chars).start)
    }
}

/// Routes an action to a command on `input`.
fn route<A: gpui::Action>(input: &Entity<TextInput>, cmd: Cmd) -> impl Fn(&A, &mut Window, &mut App) + 'static {
    let input = input.clone();
    move |_, _window, cx| input.update(cx, |text, cx| text.run(cmd, cx))
}

/// The text field as an element, with the theme's text, placeholder and selection colours and font.
/// Build it each frame: `div().child(text_input(&self.input, theme))`.
#[derive(IntoElement)]
pub struct TextInputView {
    input: Entity<TextInput>,
    colors: Colors,
    font: Font,
}

#[derive(Clone, Copy)]
struct Colors {
    text: Hsla,
    placeholder: Hsla,
    selection: Hsla,
}

pub fn text_input(input: &Entity<TextInput>, theme: &Theme) -> TextInputView {
    TextInputView {
        input: input.clone(),
        colors: Colors {
            text: theme.fg.to_hsla(),
            placeholder: theme.muted.to_hsla(),
            selection: theme.soft.to_hsla(),
        },
        font: fonts::themed(theme.ui_font.clone()),
    }
}

impl TextInputView {
    /// Sets the field's font family, for fields in the display font such as the app name.
    pub fn family(mut self, family: impl Into<SharedString>) -> Self {
        self.font = fonts::themed(family.into());
        self
    }
}

impl RenderOnce for TextInputView {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let input = self.input.read(cx);
        let focus = input.focus.clone();
        let line_height = input.config.line_height;
        let font_size = input.config.font_size;
        let family = self.font.family.clone();
        let element = TextElement { input: self.input.clone(), colors: self.colors, font: self.font };
        let wheel = self.input.clone();
        let down = self.input.clone();
        let down_focus = focus.clone();

        bind_actions(div(), &self.input)
            .key_context(CONTEXT)
            .track_focus(&focus)
            .cursor(CursorStyle::IBeam)
            .w_full()
            .text_color(self.colors.text)
            .font_family(family)
            .text_size(font_size)
            .line_height(line_height)
            .on_mouse_down(MouseButton::Left, move |event: &MouseDownEvent, window, cx| {
                window.focus(&down_focus);
                down.update(cx, |text, cx| {
                    text.pointer_down(event.position, event.click_count, event.modifiers.shift, cx)
                });
            })
            .on_scroll_wheel(move |event: &ScrollWheelEvent, _window, cx| {
                let dy = event.delta.pixel_delta(line_height).y;
                if wheel.update(cx, |text, cx| text.scroll_by(dy, cx)) {
                    cx.stop_propagation();
                }
            })
            .child(element)
    }
}

/// Binds every text input action on `div` to `input`.
fn bind_actions(div: Div, input: &Entity<TextInput>) -> Div {
    div.on_action(route::<Backspace>(input, Cmd::Backspace))
        .on_action(route::<Delete>(input, Cmd::Delete))
        .on_action(route::<DeleteWordLeft>(input, Cmd::DeleteWordLeft))
        .on_action(route::<DeleteWordRight>(input, Cmd::DeleteWordRight))
        .on_action(route::<Left>(input, Cmd::Left(false)))
        .on_action(route::<Right>(input, Cmd::Right(false)))
        .on_action(route::<Up>(input, Cmd::Up(false)))
        .on_action(route::<Down>(input, Cmd::Down(false)))
        .on_action(route::<WordLeft>(input, Cmd::WordLeft(false)))
        .on_action(route::<WordRight>(input, Cmd::WordRight(false)))
        .on_action(route::<SelectLeft>(input, Cmd::Left(true)))
        .on_action(route::<SelectRight>(input, Cmd::Right(true)))
        .on_action(route::<SelectUp>(input, Cmd::Up(true)))
        .on_action(route::<SelectDown>(input, Cmd::Down(true)))
        .on_action(route::<SelectWordLeft>(input, Cmd::WordLeft(true)))
        .on_action(route::<SelectWordRight>(input, Cmd::WordRight(true)))
        .on_action(route::<Home>(input, Cmd::Home(false)))
        .on_action(route::<End>(input, Cmd::End(false)))
        .on_action(route::<SelectHome>(input, Cmd::Home(true)))
        .on_action(route::<SelectEnd>(input, Cmd::End(true)))
        .on_action(route::<SelectAll>(input, Cmd::SelectAll))
        .on_action(route::<CopySelection>(input, Cmd::Copy))
        .on_action(route::<CutSelection>(input, Cmd::Cut))
        .on_action(route::<PasteText>(input, Cmd::Paste))
        .on_action(route::<Undo>(input, Cmd::Undo))
        .on_action(route::<Redo>(input, Cmd::Redo))
        .on_action(route::<Enter>(input, Cmd::Enter { shift: false, secondary: false }))
        .on_action(route::<ShiftEnter>(input, Cmd::Enter { shift: true, secondary: false }))
        .on_action(route::<SecondaryEnter>(input, Cmd::Enter { shift: false, secondary: true }))
}

/// The element that shapes, paints and reports the layout of the text.
struct TextElement {
    input: Entity<TextInput>,
    colors: Colors,
    font: Font,
}

struct Prepaint {
    layout: Layout,
    /// The visible content area, in window coordinates.
    content: Bounds<Pixels>,
    /// Window point of content-space (0, 0), after scrolling.
    base: Point<Pixels>,
    selection: Vec<Bounds<Pixels>>,
    caret: Option<Bounds<Pixels>>,
    placeholder: Option<WrappedLine>,
}

impl IntoElement for TextElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for TextElement {
    type RequestLayoutState = ();
    type PrepaintState = Prepaint;

    fn id(&self) -> Option<gpui::ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let input = self.input.read(cx);
        let config = input.config.clone();
        let texts = line_texts(&input.edit);
        let font = self.font.clone();
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        let layout_id = window.request_measured_layout(style, move |known, available, window, _cx| {
            let width = known.width.or(match available.width {
                AvailableSpace::Definite(width) => Some(width),
                _ => None,
            });
            // Same wrap width as `prepaint`, which lays out inside the horizontal padding.
            let wrap = if config.multiline { width.map(|width| width - config.pad_x * 2.) } else { None };
            let rows: usize = texts
                .iter()
                .map(|text| {
                    let line = shape(window, text, &font, config.font_size, black(), wrap);
                    line.wrap_boundaries().len() + 1
                })
                .sum();
            let rows = rows.max(config.min_lines).min(config.max_lines.unwrap_or(usize::MAX));
            let padding = config.pad_top + config.pad_bottom;
            size(width.unwrap_or(px(0.)), config.line_height * rows as f32 + padding)
        });
        (layout_id, ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Prepaint {
        let input = self.input.read(cx);
        let config = input.config.clone();
        let content = Bounds::new(
            point(bounds.left() + config.pad_x, bounds.top() + config.pad_top),
            size(
                bounds.size.width - config.pad_x * 2.,
                bounds.size.height - config.pad_top - config.pad_bottom,
            ),
        );
        let wrap = if config.multiline { Some(content.size.width) } else { None };
        let text_color = self.colors.text;
        let lines: Vec<WrappedLine> = line_texts(&input.edit)
            .iter()
            .map(|text| shape(window, text, &self.font, config.font_size, text_color, wrap))
            .collect();
        let mut tops = Vec::with_capacity(lines.len());
        let mut top = px(0.);
        for line in &lines {
            tops.push(top);
            top += config.line_height * (line.wrap_boundaries().len() + 1) as f32;
        }
        let mut layout = Layout {
            content,
            scroll: input.scroll_y,
            lines,
            tops,
            line_height: config.line_height,
        };

        let cursor = input.edit.buffer().cursor();
        let selection = input.edit.buffer().selection_range();
        let follow = input.follow_caret;
        let is_empty = input.edit.char_len() == 0;
        let no_selection = selection.is_none();

        let max_scroll = (layout.total_height() - content.size.height).max(px(0.));
        let mut scroll = layout.scroll.min(max_scroll).max(px(0.));
        if follow {
            let caret = layout.caret(cursor);
            if caret.y < scroll {
                scroll = caret.y;
            }
            let bottom = caret.y + config.line_height;
            if bottom > scroll + content.size.height {
                scroll = bottom - content.size.height;
            }
            scroll = scroll.min(max_scroll).max(px(0.));
        }
        layout.scroll = scroll;

        let base = point(content.left(), content.top() - scroll);
        let selection_rects = match selection {
            Some((start, end)) => selection_bounds(&layout, start, end, base),
            None => Vec::new(),
        };
        let caret = no_selection.then(|| {
            let at = layout.caret(cursor);
            Bounds::new(base + at, size(px(2.), config.line_height))
        });
        let placeholder = (is_empty && !config.placeholder.is_empty()).then(|| {
            shape(window, &config.placeholder, &self.font, config.font_size, self.colors.placeholder, wrap)
        });

        let stored = layout.clone();
        self.input.update(cx, |input, _| {
            input.layout = Some(stored);
            input.scroll_y = scroll;
            input.follow_caret = false;
        });

        Prepaint { layout, content, base, selection: selection_rects, caret, placeholder }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus = self.input.read(cx).focus.clone();
        window.handle_input(&focus, ElementInputHandler::new(bounds, self.input.clone()), cx);

        // Drag selection can leave the field, so the mouse listeners are window-wide.
        let input = self.input.clone();
        window.on_mouse_event(move |event: &MouseMoveEvent, phase, _window, cx| {
            if phase == DispatchPhase::Bubble && input.read(cx).selecting {
                input.update(cx, |text, cx| text.drag_to(event.position, cx));
            }
        });
        let input = self.input.clone();
        window.on_mouse_event(move |_: &MouseUpEvent, phase, _window, cx| {
            if phase == DispatchPhase::Bubble && input.read(cx).selecting {
                input.update(cx, |text, _| text.selecting = false);
            }
        });

        let line_height = prepaint.layout.line_height;
        let selection_color = self.colors.selection;
        let caret_color = self.colors.text;
        let base = prepaint.base;
        window.with_content_mask(Some(gpui::ContentMask { bounds: prepaint.content }), |window| {
            for rect in &prepaint.selection {
                window.paint_quad(fill(*rect, selection_color));
            }
            if let Some(placeholder) = &prepaint.placeholder {
                let _ = placeholder.paint(prepaint.content.origin, line_height, TextAlign::Left, None, window, cx);
            }
            for (line, top) in prepaint.layout.lines.iter().zip(&prepaint.layout.tops) {
                let _ = line.paint(base + point(px(0.), *top), line_height, TextAlign::Left, None, window, cx);
            }
            if focus.is_focused(window) {
                if let Some(caret) = prepaint.caret {
                    window.paint_quad(fill(caret, caret_color));
                }
            }
        });
    }
}

/// Each hard line of the edit as a string, in buffer order.
fn line_texts(edit: &Edit) -> Vec<String> {
    let buffer = edit.buffer();
    (0..buffer.line_count()).map(|line| buffer.line(line)).collect()
}

/// Shapes one hard line, wrapped at `wrap` when given. Empty lines skip the shaper.
fn shape(
    window: &Window,
    text: &str,
    font: &Font,
    font_size: Pixels,
    color: Hsla,
    wrap: Option<Pixels>,
) -> WrappedLine {
    if text.is_empty() {
        return WrappedLine::default();
    }
    let run = TextRun {
        len: text.len(),
        font: font.clone(),
        color,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    window
        .text_system()
        .shape_text(SharedString::from(text.to_string()), font_size, &[run], wrap, None)
        .ok()
        .and_then(|lines| lines.into_iter().next())
        .unwrap_or_default()
}

/// Rectangles covering `start..end`, one per visual row, in window coordinates from `base`.
fn selection_bounds(layout: &Layout, start: Pos, end: Pos, base: Point<Pixels>) -> Vec<Bounds<Pixels>> {
    let lh = layout.line_height;
    let mut rects = Vec::new();
    for line in start.line..=end.line {
        let Some(wrapped) = layout.lines.get(line) else {
            continue;
        };
        let line_len = wrapped.text.chars().count();
        let from = if line == start.line { start.col } else { 0 };
        let to = if line == end.line { end.col } else { line_len };
        let a = layout.caret(Pos { line, col: from });
        let b = layout.caret(Pos { line, col: to });
        if (b.y - a.y).abs() < lh / 2. {
            rects.push(Bounds::new(base + a, size(b.x - a.x, lh)));
            continue;
        }
        let right = wrapped.wrap_width.unwrap_or(b.x).max(a.x);
        rects.push(Bounds::new(base + a, size(right - a.x, lh)));
        let mut y = a.y + lh;
        while y < b.y {
            rects.push(Bounds::new(base + point(px(0.), y), size(right, lh)));
            y += lh;
        }
        rects.push(Bounds::new(base + point(px(0.), b.y), size(b.x, lh)));
    }
    rects
}
