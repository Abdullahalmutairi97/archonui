//! The composer at the bottom of each tile: multiline input, attachments, send/stop, and the model /
//! mode / effort picker. Spec: docs/design/chat.md §3–§5.

mod attach;
pub(crate) mod icons;
mod palette;
mod picker;
mod rules;

use self::{
    icons::{stroke_icon, PAPERCLIP, SEND},
    palette::{Action, PaletteEntry},
    picker::Pop,
    rules::{next_mode, placeholder},
};
use super::{
    shell::Shell,
    widgets::{text_input, AttachmentCard, CardVariant, TextInput, TextInputConfig, TextInputEvent},
};
use crate::{
    app::workspace::RightTab,
    engine::{attach::human_size, Capabilities, Event, Mode},
};
use gpui::{
    anchored, canvas, deferred, div, point, prelude::*, AnyElement, App, ClickEvent, Context, Corner, Entity,
    KeyDownEvent, Pixels, SharedString, Subscription, Window,
};
use crate::ui::scale::px;
use std::{cell::Cell, collections::HashMap, path::PathBuf, rc::Rc};

/// Per-session composer state, keyed by session id. An entry is made the first time the composer
/// renders and dropped when its session is deleted.
#[derive(Default)]
pub struct ComposerUi {
    entries: HashMap<String, ComposerEntry>,
}

struct ComposerEntry {
    input: Entity<TextInput>,
    /// The popover open above the composer, if any.
    pop: Option<Pop>,
    /// The palette row Up and Down select.
    row: usize,
    /// Files the attach popover offers from the session's folder, scanned when it opens.
    recent: Vec<PathBuf>,
    /// The composer card's width from its last paint; the palette matches it.
    card_width: Rc<Cell<Pixels>>,
    /// Messages typed while the agent was working, oldest first. The first is sent when the turn ends.
    queued: Vec<String>,
    /// Steer now stopped the turn: send the queued message even though the turn was stopped.
    steer: bool,
    _subs: Vec<Subscription>,
}

impl ComposerUi {
    fn input(&self, id: &str) -> Option<Entity<TextInput>> {
        self.entries.get(id).map(|e| e.input.clone())
    }

    fn pop(&self, id: &str) -> Option<Pop> {
        self.entries.get(id).and_then(|e| e.pop)
    }

    fn set_pop(&mut self, id: &str, pop: Option<Pop>) {
        if let Some(e) = self.entries.get_mut(id) {
            e.pop = pop;
        }
    }

    fn row(&self, id: &str) -> usize {
        self.entries.get(id).map_or(0, |e| e.row)
    }

    fn set_row(&mut self, id: &str, row: usize) {
        if let Some(e) = self.entries.get_mut(id) {
            e.row = row;
        }
    }

    fn recent(&self, id: &str) -> &[PathBuf] {
        self.entries.get(id).map_or(&[], |e| e.recent.as_slice())
    }

    fn set_recent(&mut self, id: &str, recent: Vec<PathBuf>) {
        if let Some(e) = self.entries.get_mut(id) {
            e.recent = recent;
        }
    }

    fn queued(&self, id: &str) -> &[String] {
        self.entries.get(id).map_or(&[], |e| e.queued.as_slice())
    }

    fn queue(&mut self, id: &str, text: String) {
        if let Some(e) = self.entries.get_mut(id) {
            e.queued.push(text);
        }
    }

    fn unqueue(&mut self, id: &str, index: usize) -> Option<String> {
        let e = self.entries.get_mut(id)?;
        (index < e.queued.len()).then(|| e.queued.remove(index))
    }

    fn card_width(&self, id: &str) -> Rc<Cell<Pixels>> {
        self.entries.get(id).map_or_else(|| Rc::new(Cell::new(px(0.))), |e| e.card_width.clone())
    }
}

/// Floats `panel` at window level, so a tile's clipping can't cut it. `slot` must be a zero-size
/// element placed at the trigger's corner in the flow; `corner` is the panel corner that meets it.
/// The panel shifts to stay inside the window.
fn float_above(panel: impl IntoElement, corner: Corner) -> AnyElement {
    deferred(
        anchored()
            .anchor(corner)
            .offset(point(px(0.), px(-6.)))
            .snap_to_window_with_margin(px(8.))
            .child(panel),
    )
    .into_any_element()
}

/// `/name arg` split into its parts. `None` when the text is not a slash command.
fn local_command(text: &str) -> Option<(&str, &str)> {
    let rest = text.strip_prefix('/')?;
    let (name, arg) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
    Some((name, arg.trim()))
}

impl Shell {
    /// The agent's capabilities, or `None` while discovery runs or the agent is missing.
    pub(super) fn caps_for(&self, agent: &str) -> Option<&Capabilities> {
        self.caps.get(agent).and_then(Option::as_ref)
    }

    /// The composer for `session`, `focused` when its tile is the focused one.
    pub(super) fn render_composer(&mut self, session: &str, focused: bool, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let state = &self.state;
        self.composers.entries.retain(|id, _| state.session(id).is_some());
        if self.state.session(session).is_none() {
            return div().into_any_element();
        }
        let input = self.composer_input(session, window, cx);
        // Screenshots: `ARCHONUI_OPEN=popover:context` (or `model`, `attach`) opens that popover once.
        static DEV_POPOVER: std::sync::Once = std::sync::Once::new();
        if focused {
            DEV_POPOVER.call_once(|| {
                let pop = match std::env::var("ARCHONUI_OPEN").ok().as_deref() {
                    Some("popover:context") => Some(Pop::Context),
                    Some("popover:model") => Some(Pop::Model),
                    Some("popover:attach") => Some(Pop::Attach),
                    _ => None,
                };
                if pop.is_some() {
                    self.composers.set_pop(session, pop);
                }
            });
        }
        let Some(s) = self.state.session(session) else { return div().into_any_element() };
        let theme = &self.theme;
        let draft = input.read(cx).text();
        let busy = self.state.busy(session);
        let pending = self.state.pending(session);
        let pop = self.composers.pop(session);
        let palette = self.palette_entries(session, &draft).unwrap_or_default();
        let row = self.composers.row(session);
        let can_send = !draft.trim().is_empty() || !pending.is_empty();
        let border = if focused { theme.line2 } else { theme.line };
        let id = session.to_string();
        let shell = cx.entity();

        let cards: Vec<AnyElement> = pending
            .iter()
            .enumerate()
            .map(|(index, a)| {
                let (shell, id) = (shell.clone(), id.clone());
                div()
                    .flex_none()
                    .child(
                        AttachmentCard::new(
                            SharedString::from(format!("composer-att-{id}-{index}")),
                            a.name.clone(),
                            human_size(a.size),
                            CardVariant::Composer,
                            theme,
                        )
                        .on_remove(move |_, cx| shell.update(cx, |this, cx| this.remove_attachment(&id, index, cx))),
                    )
                    .into_any_element()
            })
            .collect();

        let attach_open = pop == Some(Pop::Attach);
        let attach_id = id.clone();
        let attach = div()
            .id(SharedString::from(format!("composer-attach-{id}")))
            .flex_none()
            .w(px(30.))
            .h(px(30.))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(7.))
            .text_color(theme.fg.to_hsla())
            .bg(if attach_open { theme.line2 } else { theme.line }.to_hsla())
            .hover(|h| h.bg(theme.line2.to_hsla()))
            .cursor_pointer()
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.toggle_attach(&attach_id, cx)))
            .child(stroke_icon(PAPERCLIP, 16., 1.5, theme.fg.to_hsla()));
        // The slot sits before the button so the attach popover opens from its left edge.
        let attach_slot = div()
            .flex_none()
            .flex()
            .flex_row()
            .items_start()
            .children(attach_open.then(|| float_above(self.render_attach_popover(&id, cx), Corner::BottomLeft)))
            .child(attach);

        // Busy with a draft, the button queues the draft, so it shows send rather than stop.
        let stop = busy && draft.trim().is_empty();
        let (send_bg, send_fg) = if stop {
            (theme.fg, theme.bg)
        } else if can_send {
            (theme.acc, theme.on)
        } else {
            (theme.line2, theme.muted)
        };
        let send_id = id.clone();
        let send_glyph = if stop {
            div().w(px(11.)).h(px(11.)).rounded(px(2.)).bg(send_fg.to_hsla()).into_any_element()
        } else {
            stroke_icon(SEND, 16., 1.8, send_fg.to_hsla()).into_any_element()
        };
        let send = div()
            .id(SharedString::from(format!("composer-send-{id}")))
            .flex_none()
            .w(px(30.))
            .h(px(30.))
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .bg(send_bg.to_hsla())
            .cursor_pointer()
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.composer_action(&send_id, cx)))
            .child(send_glyph);

        // Everything on one line: attach, the message box, the model trigger and send. The box
        // grows with the text and the buttons stay on its last line.
        let row_line = div()
            .flex()
            .items_end()
            .gap(px(6.))
            .min_w_0()
            .px(px(6.))
            .py(px(5.))
            .child(attach_slot)
            .child(div().flex_1().min_w_0().child(text_input(&input, theme).family(theme.chat_font.clone())))
            .child(self.render_context_meter(s, pop == Some(Pop::Context), cx))
            .child(div().flex_none().max_w(gpui::relative(0.45)).min_w_0().child(self.render_model_trigger(s, pop == Some(Pop::Model), cx)))
            .child(send);

        let width = self.composers.card_width(&id);
        let measured = width.clone();
        let has_cards = !cards.is_empty();
        // One line of chips that scrolls sideways, so a short tile keeps its composer height.
        let chips = div()
            .id(SharedString::from(format!("composer-chips-{id}")))
            .flex()
            .flex_nowrap()
            .gap(px(6.))
            .px(px(8.))
            .pt(px(8.))
            .overflow_x_scroll()
            .children(cards);
        let card = div()
            .relative()
            .flex()
            .flex_col()
            .rounded(px(10.))
            .bg(theme.pop.to_hsla())
            .border_1()
            .border_color(border.to_hsla())
            .when(has_cards, |c| c.child(chips))
            .child(row_line)
            .child(
                canvas(|_, _, _| (), move |bounds, _, _, _| measured.set(bounds.size.width))
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full(),
            );
        // The palette sits above the card; the card's width is the one it measured last frame.
        let palette_slot = (!palette.is_empty()).then(|| {
            float_above(self.render_palette(&id, &palette, row, width.get(), cx), Corner::BottomLeft)
        });
        div()
            .flex_none()
            .px(px(12.))
            .pt(px(8.))
            .pb(px(12.))
            .children(self.render_queue(&id, cx))
            .child(div().children(palette_slot).child(card))
            .into_any_element()
    }

    /// The slash palette above the composer. Clicking a row runs it.
    fn render_palette(
        &self,
        id: &str,
        rows: &[PaletteEntry],
        row: usize,
        width: Pixels,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = &self.theme;
        let selected = row.min(rows.len().saturating_sub(1));
        div()
            .id(SharedString::from(format!("composer-palette-{id}")))
            .min_w(px(200.))
            .w(width)
            .p(px(4.))
            .max_h(px(260.))
            .overflow_y_scroll()
            .rounded(px(10.))
            .bg(theme.pop.to_hsla())
            .border_1()
            .border_color(theme.line2.to_hsla())
            .occlude()
            .children(rows.iter().enumerate().map(|(i, entry)| {
                let (id, entry) = (id.to_string(), entry.clone());
                let label = match entry.action {
                    Action::Command => format!("/{}", entry.name),
                    _ => entry.name.clone(),
                };
                let row_id = format!("composer-palette-row-{id}-{i}");
                let chosen = entry.clone();
                div()
                    .id(SharedString::from(row_id))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .px(px(10.))
                    .py(px(7.))
                    .rounded(px(6.))
                    .cursor_pointer()
                    .when(i == selected, |r| r.bg(theme.soft.to_hsla()))
                    .hover(|h| h.bg(theme.line2.to_hsla()))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.run_palette_entry(&id, &chosen, cx)))
                    .child(div().text_size(px(13.)).text_color(theme.fg.to_hsla()).whitespace_nowrap().child(label))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(12.))
                            .text_color(theme.muted.to_hsla())
                            .child(entry.description.clone()),
                    )
                    .children(entry.tags.iter().map(|tag| {
                        div().text_size(px(10.5)).text_color(theme.faint.to_hsla()).child(*tag)
                    }))
            }))
    }

    /// The input for session `id`, created with its subscriptions on first use.
    fn composer_input(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) -> Entity<TextInput> {
        if let Some(input) = self.composers.input(id) {
            return input;
        }
        let mode = self.state.session(id).map_or(Mode::Auto, |s| s.mode);
        let config = TextInputConfig {
            multiline: true,
            // One line like a chat app's box; it grows with the text, up to ten lines.
            min_lines: 1,
            max_lines: Some(10),
            placeholder: placeholder(mode, &self.state.settings.app_name).into(),
            font_size: px(14.),
            line_height: px(20.),
            pad_top: px(7.),
            pad_x: px(4.),
            pad_bottom: px(7.),
            ..Default::default()
        };
        let input = cx.new(|cx| TextInput::new(config, window, cx));
        // A draft left in this chat earlier, maybe before a restart.
        if let Some(draft) = self.state.session(id).map(|s| s.draft.clone()).filter(|d| !d.is_empty()) {
            input.update(cx, |input, cx| input.set_text(&draft, cx));
        }
        let key = id.to_string();
        let sub = cx.subscribe(&input, move |this, input, ev: &TextInputEvent, cx| match ev {
            TextInputEvent::Changed => {
                this.state.set_draft(&key, &input.read(cx).text());
                this.composers.set_row(&key, 0);
                cx.notify();
            }
            TextInputEvent::Submit => this.composer_enter(&key, cx),
            TextInputEvent::Blur => {}
        });
        self.composers.entries.insert(
            id.to_string(),
            ComposerEntry {
                input: input.clone(),
                pop: None,
                row: 0,
                recent: Vec::new(),
                queued: Vec::new(),
                steer: false,
                card_width: Rc::new(Cell::new(px(0.))),
                _subs: vec![sub],
            },
        );
        input
    }

    /// The draft in session `id`.
    fn draft(&self, id: &str, cx: &App) -> String {
        self.composers.input(id).map_or_else(String::new, |input| input.read(cx).text())
    }

    /// Replaces the draft in session `id` and puts the palette selection back on its first row.
    fn set_draft(&mut self, id: &str, text: &str, cx: &mut Context<Self>) {
        self.composers.set_row(id, 0);
        if let Some(input) = self.composers.input(id) {
            input.update(cx, |input, cx| input.set_text(text, cx));
        }
    }

    pub(super) fn clear_input(&mut self, id: &str, cx: &mut Context<Self>) {
        self.set_draft(id, "", cx);
    }

    /// Enter in the composer: allows a waiting approval, runs the selected palette row, or sends the draft.
    fn composer_enter(&mut self, id: &str, cx: &mut Context<Self>) {
        if self.allow_first_approval(id, cx) {
            return;
        }
        match self.selected_palette_entry(id, cx) {
            Some(entry) => self.run_palette_entry(id, &entry, cx),
            None => {
                let draft = self.draft(id, cx);
                self.submit_text(id, &draft, cx);
            }
        }
    }

    /// The send button: queues the draft while the agent works, stops a running turn when there is no
    /// draft, else sends the draft.
    fn composer_action(&mut self, id: &str, cx: &mut Context<Self>) {
        if self.state.busy(id) && self.draft(id, cx).trim().is_empty() {
            self.stop_chat(id);
            cx.notify();
            return;
        }
        let draft = self.draft(id, cx);
        self.submit_text(id, &draft, cx);
    }

    /// Sends `text` to the session's agent, or runs it when it is a local command. A local command
    /// or a sent message clears the draft. A busy session or a missing agent keeps it.
    fn submit_text(&mut self, id: &str, text: &str, cx: &mut Context<Self>) {
        let text = text.trim();
        if let Some((name, arg)) = local_command(text) {
            if self.run_local(id, name, arg, cx) {
                self.clear_input(id, cx);
                return;
            }
        }
        // Typed while the agent works: queued, and sent when the turn ends (or now, with Steer now).
        if self.state.busy(id) {
            if !text.is_empty() {
                self.composers.queue(id, text.to_string());
                self.clear_input(id, cx);
                cx.notify();
            }
            return;
        }
        if self.send_now(id, text, cx) {
            self.clear_input(id, cx);
        }
    }

    /// Starts a turn with `text` in session `id`. Returns whether it started.
    pub(super) fn send_now(&mut self, id: &str, text: &str, cx: &mut Context<Self>) -> bool {
        if (text.is_empty() && self.state.pending(id).is_empty()) || self.state.busy(id) {
            return false;
        }
        // A chat on another device runs there; its agent need not be installed here.
        if self.state.is_remote(id).is_some() {
            return self.send_remote(id, text, cx);
        }
        let Some(agent) = self.state.session(id).map(|s| s.agent.clone()) else { return false };
        if !self.agents.iter().any(|a| a.harness.id() == agent) {
            self.toast("The agent for this chat isn't installed", cx);
            return false;
        }
        // An effort the model does not take (kept from another model or a saved default) is dropped
        // for the agent's default rather than sent.
        if let Some(s) = self.state.session(id) {
            let accepted = s.effort.as_deref().and_then(|e| self.caps_for(&agent)?.accepts_effort(s.model.as_deref(), e));
            if accepted == Some(false) {
                let (model, mode) = (s.model.clone(), s.mode);
                self.state.set_options(id, model, None, mode);
            }
        }
        let Some(events) = self.state.send(id, text, &self.agents) else { return false };
        self.share_started(id, text);
        self.follow_turn(id, events, cx);
        cx.notify();
        true
    }

    /// Steer now on the queued message at `index`: stops the running turn so the message goes next,
    /// or sends it at once when nothing runs.
    fn steer(&mut self, id: &str, index: usize, cx: &mut Context<Self>) {
        let Some(text) = self.composers.unqueue(id, index) else { return };
        if self.state.busy(id) {
            if let Some(e) = self.composers.entries.get_mut(id) {
                e.queued.insert(0, text);
                e.steer = true;
            }
            self.stop_chat(id);
        } else {
            self.send_now(id, &text, cx);
        }
        cx.notify();
    }

    /// After a turn ends, sends the first queued message, unless the user stopped the turn without
    /// Steer now: then the queue waits for Steer now or Send.
    pub(super) fn send_queued(&mut self, id: &str, stopped: bool, cx: &mut Context<Self>) {
        let steer = self.composers.entries.get_mut(id).map_or(false, |e| std::mem::take(&mut e.steer));
        if stopped && !steer {
            return;
        }
        if let Some(text) = self.composers.unqueue(id, 0) {
            if !self.send_now(id, &text, cx) {
                self.composers.entries.get_mut(id).map(|e| e.queued.insert(0, text));
            }
        }
    }

    /// The messages waiting to be sent, above the composer card: each with Steer now (or Send, when the
    /// agent isn't working) and a remove button.
    fn render_queue(&self, id: &str, cx: &mut Context<Self>) -> Option<AnyElement> {
        let queued = self.composers.queued(id);
        if queued.is_empty() {
            return None;
        }
        let theme = &self.theme;
        let busy = self.state.busy(id);
        let rows = queued.iter().enumerate().map(|(i, text)| {
            let (steer_id, drop_id) = (id.to_string(), id.to_string());
            let button = |key: &str, label: &'static str| {
                div()
                    .id(SharedString::from(format!("queue-{key}-{id}-{i}")))
                    .flex_none()
                    .px(px(8.))
                    .py(px(2.))
                    .rounded(px(5.))
                    .border_1()
                    .border_color(theme.line2.to_hsla())
                    .text_size(px(11.))
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.line2.to_hsla()))
                    .child(label)
            };
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .px(px(10.))
                .py(px(5.))
                .rounded(px(8.))
                .bg(theme.line.to_hsla())
                .text_size(px(12.))
                .child(div().flex_none().text_color(theme.acc2.to_hsla()).child(if i == 0 && busy { "Next" } else { "Queued" }))
                .child(div().flex_1().min_w_0().truncate().text_color(theme.fg.to_hsla()).child(text.replace('\n', " ")))
                .child(
                    button("steer", if busy { "Steer now" } else { "Send" })
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.steer(&steer_id, i, cx))),
                )
                .child(button("drop", "×").on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.composers.unqueue(&drop_id, i);
                    cx.notify();
                })))
        });
        Some(div().flex().flex_col().gap(px(4.)).pb(px(6.)).children(rows).into_any_element())
    }

    /// Applies a turn's events to session `id` as they arrive, until the turn's stream closes.
    pub(super) fn follow_turn(&mut self, id: &str, events: smol::channel::Receiver<Event>, cx: &mut Context<Self>) {
        let id = id.to_string();
        cx.spawn(async move |this, cx| {
            while let Ok(ev) = events.recv().await {
                if this.update(cx, |this, cx| this.apply_turn_event(&id, ev, cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    fn apply_turn_event(&mut self, id: &str, ev: Event, cx: &mut Context<Self>) {
        self.share_event(id, &ev);
        let applied = self.state.apply(id, ev);
        if applied.finished {
            self.flush_sessions(cx);
            self.end_approvals(id);
            self.turn_ended(id, &applied, cx);
            // The reply may have changed files: copy them to the cloud (no-op unless sharing is on).
            self.backup(cx);
            let stopped = applied.ended.as_ref().is_some_and(|e| matches!(e.outcome, crate::app::notify::Outcome::Stopped));
            self.send_queued(id, stopped, cx);
        }
        if let Some(url) = applied.preview.filter(|_| self.state.settings.auto_open_preview) {
            self.open_url(&url, cx);
        }
        cx.notify();
    }

    /// Keys for the focused composer. Returns true when consumed. Enter is not handled here: the
    /// input's submit event does that. Alt+Enter is left to the global handler.
    pub(super) fn composer_key(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(id) = self.state.workspace.layout.focus.clone() else { return false };
        let Some(input) = self.composers.input(&id) else { return false };
        if !input.read(cx).focus_handle().is_focused(window) {
            return false;
        }
        let mods = ev.keystroke.modifiers;
        let plain = !(mods.shift || mods.alt || mods.control || mods.platform);
        // Escape closes any open popover or palette first; with nothing open it stops a running reply.
        if plain && ev.keystroke.key == "escape" {
            if self.composers.pop(&id).is_some() {
                self.close_popover(&id, cx);
                return true;
            }
            // A path such as `/usr/bin` matches no row, so no palette is showing and the draft stays.
            if self.palette_entries(&id, &self.draft(&id, cx)).is_some_and(|rows| !rows.is_empty()) {
                self.clear_input(&id, cx);
                return true;
            }
            if self.state.busy(&id) {
                self.stop_chat(&id);
                cx.notify();
                return true;
            }
        }
        match ev.keystroke.key.as_str() {
            "tab" if mods.shift && !mods.alt && !mods.control && !mods.platform => {
                self.cycle_mode(&id, cx);
                true
            }
            "tab" if plain => self.complete_palette(&id, cx),
            "up" if plain => self.move_palette(&id, -1, cx),
            "down" if plain => self.move_palette(&id, 1, cx),
            "v" if mods.control || mods.platform => self.paste_image(&id, cx),
            "p" if mods.control || mods.platform => self.pin_selected_model(&id, cx),
            _ => false,
        }
    }

    /// Ctrl+P: pin or unpin the model the picker would choose. With a `/model` palette open it acts on
    /// the highlighted row; otherwise it acts on the session's active model.
    fn pin_selected_model(&mut self, id: &str, cx: &mut Context<Self>) -> bool {
        if let Some(entry) = self.selected_palette_entry(id, cx) {
            if let Action::Model(Some(model)) = entry.action {
                let Some(agent) = self.state.session(id).map(|s| s.agent.clone()) else { return false };
                self.toggle_model_pin(&agent, &model, cx);
                return true;
            }
        }
        let Some(s) = self.state.session(id) else { return false };
        let (agent, model) = (
            s.agent.clone(),
            s.model.clone().or_else(|| self.caps_for(&s.agent).and_then(|c| c.default_model.clone())),
        );
        let Some(model) = model else { return false };
        self.toggle_model_pin(&agent, &model, cx);
        true
    }

    /// Shift+Tab: the next mode the agent enforces.
    fn cycle_mode(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(s) = self.state.session(id) else { return };
        let Some(next) = next_mode(s.mode, self.supported_modes(&s.agent)) else { return };
        self.set_mode(id, next, cx);
    }

    /// The palette row selected in session `id`, when the palette is open.
    fn selected_palette_entry(&self, id: &str, cx: &App) -> Option<PaletteEntry> {
        let rows = self.palette_entries(id, &self.draft(id, cx))?;
        let row = self.composers.row(id).min(rows.len().checked_sub(1)?);
        rows.into_iter().nth(row)
    }

    /// Tab: a command completes to `/name `; any other row runs. False when the palette is closed.
    fn complete_palette(&mut self, id: &str, cx: &mut Context<Self>) -> bool {
        let Some(entry) = self.selected_palette_entry(id, cx) else { return false };
        match entry.action {
            Action::Command => self.set_draft(id, &format!("/{} ", entry.name), cx),
            _ => self.run_palette_entry(id, &entry, cx),
        }
        true
    }

    /// Up and Down move the palette selection, wrapping. False when the palette is closed.
    fn move_palette(&mut self, id: &str, delta: isize, cx: &mut Context<Self>) -> bool {
        let Some(rows) = self.palette_entries(id, &self.draft(id, cx)) else { return false };
        if rows.is_empty() {
            return false;
        }
        // Step from the row shown, which is clamped when the list has shrunk since it was set.
        let cur = self.composers.row(id).min(rows.len() - 1);
        let row = (cur as isize + delta).rem_euclid(rows.len() as isize) as usize;
        self.composers.set_row(id, row);
        cx.notify();
        true
    }

    /// Updates the placeholder to the mode's text. Called when the mode changes.
    fn refresh_placeholder(&self, id: &str, cx: &mut Context<Self>) {
        let (Some(input), Some(s)) = (self.composers.input(id), self.state.session(id)) else { return };
        let text: SharedString = placeholder(s.mode, &self.state.settings.app_name).into();
        input.update(cx, |input, cx| input.set_placeholder(text, cx));
    }

    fn remove_attachment(&mut self, id: &str, index: usize, cx: &mut Context<Self>) {
        self.state.remove_pending(id, index);
        cx.notify();
    }

    /// Shows `tab` in the tools panel and opens the panel.
    pub(super) fn show_tool_tab(&mut self, tab: RightTab, cx: &mut Context<Self>) {
        self.state.workspace.right_tab = tab;
        self.state.workspace.right_open = true;
        self.widths_changed(false, cx);
    }

    /// Opens a file in the IDE tab.
    pub(super) fn open_in_editor(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.ide_open(&path, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_command_splits_name_and_argument() {
        assert_eq!(local_command("/model  gpt-5 "), Some(("model", "gpt-5")));
        assert_eq!(local_command("/stop"), Some(("stop", "")));
        assert_eq!(local_command("review the code"), None);
    }
}
