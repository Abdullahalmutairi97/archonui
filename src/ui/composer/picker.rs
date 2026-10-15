//! The model picker: the mode cards, the model list and the effort moons, opened from the model
//! trigger in the composer's toolbar. Spec: chat.md §5. Choosing a mode, model or effort closes it.

use super::{
    float_above,
    icons::{self, moon, stroke_icon},
    rules::{effort_description, effort_fraction, nearest_effort},
    Shell,
};
use crate::{
    engine::{Mode, ModelInfo},
    store::{Session, MAX_PINS},
    ui::{
        anim::{self, Lerp, EFFORT_FILL, EFFORT_NODE},
        theme::{Rgba, Theme},
        widgets::popover::{tooltip, TooltipColors},
    },
};
use gpui::{
    black, div, linear_color_stop, linear_gradient, point, prelude::*, relative, AnyElement, App, BoxShadow,
    ClickEvent, Context, Corner, Hsla, SharedString, Window,
};
use crate::ui::scale::px;
use std::rc::Rc;

/// The suffix of a long-context variant's model id.
const LONG_CONTEXT: &str = "[1m]";

/// A pin glyph for a model row's hover affordance (net-new; drawn in the composer icons' 16-unit box).
const PIN: &str = "M8 3.5v6M5.5 6l2.5-2.5 2.5 2.5M4.5 12.5h7";

/// A popover open above a composer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Pop {
    Model,
    Attach,
    Context,
}

/// Tokens in the session's context, the model's window, and whether the agent reported the window.
fn context_state(s: &Session) -> (u64, u64, bool) {
    let window = s.context_window.filter(|w| *w > 0);
    let fallback = crate::engine::context::context_window(&s.agent, s.model.as_deref());
    (s.context_used.unwrap_or(0), window.unwrap_or(fallback), window.is_some())
}

/// The meter's colour: the accent, then the highlight past 70%, then red past 90%, when the agent is
/// close to compacting or refusing.
fn context_colour(share: f32, theme: &Theme) -> crate::ui::theme::Rgba {
    match share {
        s if s >= 0.9 => crate::ui::theme::Rgba::hex(0xf85149),
        s if s >= 0.7 => theme.acc2,
        _ => theme.acc,
    }
}

/// `950`, `12k`, `1.2M` tokens.
fn tokens(n: u64) -> String {
    match n {
        0..=999 => n.to_string(),
        1_000..=999_999 => format!("{}k", (n as f64 / 1e3).round()),
        _ => format!("{:.1}M", n as f64 / 1e6),
    }
}

/// The effort scale of a session's model.
pub(in crate::ui) struct EffortState {
    pub levels: Vec<String>,
    /// The level in use; `None` when the session's effort is not one of the model's levels.
    pub index: Option<usize>,
    /// The effort in use, explicit or the agent's default. `None` when the agent's default is unknown.
    pub name: Option<String>,
}

impl EffortState {
    /// Where the level in use sits on the 0–1 scale; 0 when none is in use.
    fn fraction(&self) -> f32 {
        self.index.map_or(0.0, |i| effort_fraction(&self.levels, i))
    }
}

/// The mode's icon path, colour-coded by [`mode_tint`].
fn mode_glyph(mode: Mode) -> &'static str {
    match mode {
        Mode::Auto => icons::MODE_AUTO,
        Mode::Plan => icons::MODE_PLAN,
        Mode::Ask => icons::MODE_ASK,
    }
}

/// The card's subtitle (chat.md §5.2).
fn mode_sub(mode: Mode) -> &'static str {
    match mode {
        Mode::Auto => "Edits and runs",
        Mode::Plan => "Steps first",
        Mode::Ask => "Read-only",
    }
}

/// The colour of a mode's icon and title when selected (chat.md §5.2, §5.6).
fn mode_tint(mode: Mode, theme: &Theme) -> Rgba {
    match mode {
        Mode::Auto => theme.acc2,
        Mode::Plan => theme.alt,
        Mode::Ask => theme.fg,
    }
}

impl Shell {
    /// The effort scale for `s`, or `None` when its model has no effort levels or no caps are known.
    pub(in crate::ui) fn effort_state(&self, s: &Session) -> Option<EffortState> {
        let caps = self.caps_for(&s.agent)?;
        let levels = caps.efforts(s.model.as_deref());
        if levels.is_empty() {
            return None;
        }
        // The agent's default effort is only known for its default model.
        let on_default_model = s.model.is_none() || s.model == caps.default_model;
        let effective = s.effort.clone().or_else(|| if on_default_model { caps.default_effort.clone() } else { None });
        let index = effective.as_deref().and_then(|e| levels.iter().position(|l| l == e));
        Some(EffortState { levels: levels.to_vec(), index, name: effective })
    }

    /// The model's short label for the trigger. A session on the agent's default shows that model's label.
    pub(in crate::ui) fn trigger_label(&self, s: &Session) -> String {
        let caps = self.caps_for(&s.agent);
        let id = s.model.as_deref().or_else(|| caps.and_then(|c| c.default_model.as_deref()));
        match id {
            Some(id) => caps
                .and_then(|c| c.models.iter().find(|m| m.id == id))
                .map_or_else(|| id.to_string(), |m| m.label.clone()),
            None => "Default model".into(),
        }
    }

    /// Picks `model` for session `id`, keeping the effort's relative position on the new model's scale.
    pub(super) fn set_model(&mut self, id: &str, model: Option<String>, cx: &mut Context<Self>) {
        let Some(s) = self.state.session(id) else { return };
        let (agent, mode) = (s.agent.clone(), s.mode);
        let fraction = self.effort_state(s).and_then(|e| e.index.map(|i| effort_fraction(&e.levels, i))).unwrap_or(0.5);
        let effort = self
            .caps_for(&agent)
            .map(|c| c.efforts(model.as_deref()).to_vec())
            .and_then(|levels| nearest_effort(&levels, fraction).map(|i| levels[i].clone()));
        self.state.set_options(id, model, effort, mode);
        self.close_popover(id, cx);
    }

    /// Sets the effort of session `id`; `None` is the agent's default.
    pub(super) fn set_effort(&mut self, id: &str, level: Option<String>, cx: &mut Context<Self>) {
        let Some(s) = self.state.session(id) else { return };
        let (model, mode) = (s.model.clone(), s.mode);
        self.state.set_options(id, model, level, mode);
        self.close_popover(id, cx);
    }

    /// Sets the mode of session `id`. It applies to the next message.
    pub(super) fn set_mode(&mut self, id: &str, mode: Mode, cx: &mut Context<Self>) {
        let Some(s) = self.state.session(id) else { return };
        let (model, effort) = (s.model.clone(), s.effort.clone());
        self.state.set_options(id, model, effort, mode);
        self.refresh_placeholder(id, cx);
        self.close_popover(id, cx);
    }

    /// Hands session `id` to `agent`, with that agent's default model and effort. The mode is kept
    /// when the new agent enforces it. The new agent gets the chat so far with the next message.
    pub(in crate::ui) fn switch_agent(&mut self, id: &str, agent: &str, cx: &mut Context<Self>) {
        let Some(s) = self.state.session(id) else { return };
        if s.agent == agent {
            self.close_popover(id, cx);
            return;
        }
        if self.state.busy(id) {
            self.toast("Stop the running reply before switching agents.", cx);
            return;
        }
        let Some(name) = self.agents.iter().find(|a| a.harness.id() == agent && a.enabled).map(|a| a.harness.name()) else {
            self.toast(format!("{} is not installed or is turned off.", crate::engine::agent_name(agent)), cx);
            return;
        };
        let (mode, started) = (s.mode, !s.messages.is_empty());
        let modes = self.supported_modes(agent);
        let mode = if modes.contains(&mode) { mode } else { modes.first().copied().unwrap_or_default() };
        let (model, effort) = self.agent_defaults(agent);
        self.state.switch_agent(id, agent, model, effort, mode);
        self.refresh_placeholder(id, cx);
        self.close_popover(id, cx);
        if started {
            self.toast(format!("Switched to {name}. It gets this chat so far with your next message."), cx);
        }
    }

    /// Opens `pop` above session `id`'s composer, or closes it if it is already open.
    pub(super) fn toggle_popover(&mut self, id: &str, pop: Pop, cx: &mut Context<Self>) {
        let open = self.composers.pop(id) == Some(pop);
        self.composers.set_pop(id, if open { None } else { Some(pop) });
        cx.notify();
    }

    /// Opens `pop` above session `id`'s composer.
    pub(super) fn open_popover(&mut self, id: &str, pop: Pop, cx: &mut Context<Self>) {
        self.composers.set_pop(id, Some(pop));
        cx.notify();
    }

    pub(super) fn close_popover(&mut self, id: &str, cx: &mut Context<Self>) {
        self.composers.set_pop(id, None);
        self.flush_sessions(cx);
        cx.notify();
    }

    /// The model trigger: mode icon, model label, the effort moon and level, and a caret.
    pub(super) fn render_model_trigger(&self, s: &Session, open: bool, cx: &mut Context<Self>) -> AnyElement {
        let theme = &self.theme;
        let id = s.id.clone();
        let effort = self.effort_state(s);
        let tint = mode_tint(s.mode, theme).to_hsla();
        // Shrinks before the toolbar wraps: the model name truncates, the moon and caret keep their size.
        let mut row = div()
            .id(gpui::SharedString::from(format!("composer-model-{id}")))
            .flex()
            .flex_shrink()
            .min_w_0()
            .items_center()
            .gap(px(8.))
            .h(px(32.))
            .px(px(12.))
            .rounded(px(7.))
            .text_size(px(13.))
            .text_color(theme.fg.to_hsla())
            .bg(if open { theme.line2 } else { theme.line }.to_hsla())
            .cursor_pointer()
            .hover(|h| h.bg(theme.line2.to_hsla()))
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.toggle_popover(&id, Pop::Model, cx)))
            .child(stroke_icon(mode_glyph(s.mode), 13., 1.6, tint))
            .child(
                div()
                    .min_w_0()
                    .flex_shrink()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .truncate()
                    .child(self.trigger_label(s)),
            );
        if let Some(e) = effort {
            // The level name truncates in narrow tiles; the moon keeps its size.
            let mut effort_text =
                div().flex().flex_shrink().min_w_0().items_center().gap(px(5.)).text_color(theme.muted.to_hsla());
            effort_text = effort_text.child(
                div().flex_none().child(moon(e.fraction(), 14., theme.acc2.to_hsla(), theme.acc2.alpha(0.45).to_hsla(), 1.0)),
            );
            if let Some(name) = e.name {
                effort_text = effort_text.child(div().min_w_0().truncate().child(name));
            }
            row = row.child(effort_text);
        }
        let row = row.child(div().flex_none().text_size(px(10.)).text_color(theme.muted.to_hsla()).child("▾"));
        // The panel opens from the trigger's top-right corner, so it grows leftwards.
        let panel = open.then(|| float_above(self.render_model_popover(s, cx), Corner::BottomRight));
        div().flex().flex_row().items_start().flex_shrink().min_w_0().child(row).children(panel).into_any_element()
    }

    /// How full the agent's context is: a bar with no numbers. Clicking it opens the details.
    pub(super) fn render_context_meter(&self, s: &Session, open: bool, cx: &mut Context<Self>) -> AnyElement {
        let theme = &self.theme;
        let id = s.id.clone();
        let (used, window, _) = context_state(s);
        let share = (used as f32 / window as f32).clamp(0., 1.);
        let fill = context_colour(share, theme);
        let meter = div()
            .id(gpui::SharedString::from(format!("composer-context-{id}")))
            .flex_none()
            .h(px(32.))
            .px(px(7.))
            .flex()
            .items_center()
            .rounded(px(7.))
            .cursor_pointer()
            .when(open, |d| d.bg(theme.line2.to_hsla()))
            .hover(|h| h.bg(theme.line.to_hsla()))
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.toggle_popover(&id, Pop::Context, cx)))
            .child(
                div()
                    .w(px(34.))
                    .h(px(5.))
                    .rounded(px(3.))
                    .bg(theme.line2.to_hsla())
                    .overflow_hidden()
                    .child(div().h_full().w(gpui::relative(share)).rounded(px(3.)).bg(fill.to_hsla())),
            );
        let panel = open.then(|| float_above(self.render_context_popover(s), Corner::BottomRight));
        div().flex().flex_row().items_start().flex_none().child(meter).children(panel).into_any_element()
    }

    /// The context details: the share used, the tokens used of the window, and where the window comes from.
    fn render_context_popover(&self, s: &Session) -> AnyElement {
        let theme = &self.theme;
        let (used, window, reported) = context_state(s);
        let share = (used as f32 / window as f32).clamp(0., 1.);
        let percent = (share * 100.).round() as u32;
        let model = s.model.clone().unwrap_or_else(|| format!("{} default model", crate::engine::agent_name(&s.agent)));
        div()
            .w(px(240.))
            .p(px(14.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .rounded(px(10.))
            .bg(theme.pop.to_hsla())
            .border_1()
            .border_color(theme.line2.to_hsla())
            .occlude()
            .child(div().text_size(px(10.5)).text_color(theme.muted.to_hsla()).child("CONTEXT"))
            .child(div().text_size(px(20.)).font_weight(gpui::FontWeight::SEMIBOLD).child(format!("{percent}% full")))
            .child(
                div()
                    .h(px(6.))
                    .rounded(px(3.))
                    .bg(theme.line2.to_hsla())
                    .overflow_hidden()
                    .child(div().h_full().w(gpui::relative(share)).bg(context_colour(share, theme).to_hsla())),
            )
            .child(div().text_size(px(12.5)).child(format!("{} of {} tokens used", tokens(used), tokens(window))))
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(theme.muted.to_hsla())
                    .child(if reported { format!("{model} · window reported by the agent") } else { format!("{model} · usual window for this model") }),
            )
            .when(used == 0, |d| {
                d.child(div().text_size(px(11.)).text_color(theme.muted.to_hsla()).child("Fills in after the agent's first reply."))
            })
            .into_any_element()
    }

    /// The model popover: MODE cards, the MODEL list and the EFFORT moons.
    pub(super) fn render_model_popover(&self, s: &Session, cx: &mut Context<Self>) -> AnyElement {
        let theme = &self.theme;
        let id = s.id.clone();
        let modes = self.supported_modes(&s.agent);

        let cards = modes.iter().map(|&mode| {
            let on = mode == s.mode;
            let (fg, bd, bg) = if on {
                (mode_tint(mode, theme), theme.acc2, Some(theme.line2))
            } else {
                (theme.muted, theme.line, None)
            };
            let id = id.clone();
            let mut card = div()
                .id(gpui::SharedString::from(format!("composer-mode-{}-{id}", mode.name())))
                .flex()
                .flex_col()
                .gap(px(5.))
                .px(px(if on { 10. } else { 7. }))
                .py(px(9.))
                .rounded(px(7.))
                .border_1()
                .border_color(bd.to_hsla())
                .cursor_pointer()
                .hover(|h| h.bg(theme.line2.to_hsla()))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.set_mode(&id, mode, cx)))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(7.))
                        .text_size(px(12.5))
                        .text_color(fg.to_hsla())
                        .child(stroke_icon(mode_glyph(mode), 13., 1.6, fg.to_hsla()))
                        .child(mode.name()),
                )
                .child(div().text_size(px(10.5)).text_color(theme.muted.to_hsla()).child(mode_sub(mode)));
            if let Some(bg) = bg {
                card = card.bg(bg.to_hsla());
            }
            card
        });
        let modes_grid = div().grid().grid_cols(3).gap(px(4.)).px(px(4.)).pb(px(8.)).children(cards);

        let panel = div()
            .id(gpui::SharedString::from(format!("composer-model-popover-{id}")))
            .w(px(374.))
            .p(px(6.))
            .rounded(px(10.))
            .bg(theme.pop.to_hsla())
            .border_1()
            .border_color(theme.line2.to_hsla())
            .shadow(popover_shadow())
            .flex()
            .flex_col()
            .gap(px(2.))
            // Short tiles scroll the panel rather than clip its top edge.
            .max_h(px(545.))
            .overflow_y_scroll()
            .occlude()
            .children(self.agent_row(s, cx).map(|row| div().flex().flex_col().child(section_label("AGENT", theme)).child(row)))
            .child(section_label("MODE", theme).when(self.agent_row_shown(), |l| l.border_t_1().border_color(theme.line.to_hsla())))
            .child(modes_grid)
            .child(section_label("MODEL", theme).border_t_1().border_color(theme.line.to_hsla()))
            .child(self.model_list(s, cx));

        let panel = match self.effort_state(s) {
            Some(e) => panel.child(self.effort_block(s, &e, cx)),
            // Without caps the model list says it is still looking; no effort block is shown.
            None if self.caps_for(&s.agent).is_some() => panel.child(
                div()
                    .mt(px(4.))
                    .pt(px(12.))
                    .pb(px(10.))
                    .px(px(10.))
                    .border_t_1()
                    .border_color(theme.line.to_hsla())
                    .text_size(px(12.))
                    .text_color(theme.muted.to_hsla())
                    .child(format!("{} has no effort levels.", self.trigger_label(s))),
            ),
            None => panel,
        };
        panel.into_any_element()
    }

    /// Whether the popover offers other agents: more than one is installed and turned on.
    fn agent_row_shown(&self) -> bool {
        self.agents.iter().filter(|a| a.enabled).count() > 1
    }

    /// The AGENT chips: every enabled agent, the session's own highlighted. Choosing another hands it the chat.
    fn agent_row(&self, s: &Session, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.agent_row_shown() {
            return None;
        }
        let theme = &self.theme;
        let chips = self.agents.iter().filter(|a| a.enabled).map(|a| {
            let agent = a.harness.id();
            let on = agent == s.agent;
            let fg = if on { theme.fg } else { theme.muted };
            let id = s.id.clone();
            let mut chip = div()
                .id(gpui::SharedString::from(format!("composer-agent-{agent}-{id}")))
                .flex()
                .items_center()
                .gap(px(6.))
                .px(px(9.))
                .py(px(6.))
                .rounded(px(7.))
                .border_1()
                .border_color(if on { theme.acc2 } else { theme.line }.to_hsla())
                .text_size(px(12.))
                .text_color(fg.to_hsla())
                .cursor_pointer()
                .hover(|h| h.bg(theme.line2.to_hsla()))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.switch_agent(&id, agent, cx)))
                .child(icons::harness_icon(agent, 13., 1.5, fg.to_hsla()))
                .child(a.harness.name());
            if on {
                chip = chip.bg(theme.line2.to_hsla());
            } else if self.state.busy(&s.id) {
                chip = chip.opacity(0.5).tooltip(tooltip("Stop the reply to switch", TooltipColors::of(theme)));
            }
            chip
        });
        Some(div().flex().flex_wrap().gap(px(4.)).px(px(4.)).pb(px(8.)).children(chips).into_any_element())
    }

    /// The model rows: enabled models (plus the session's own), pinned first, then the agent default.
    fn model_list(&self, s: &Session, cx: &mut Context<Self>) -> AnyElement {
        let theme = &self.theme;
        let id = s.id.clone();
        let Some(caps) = self.caps_for(&s.agent) else {
            return div()
                .px(px(10.))
                .py(px(8.))
                .text_size(px(12.))
                .text_color(theme.muted.to_hsla())
                .child("Looking for models…")
                .into_any_element();
        };
        let current = s.model.as_deref();
        let settings = &self.state.settings;
        let shown = |m: &ModelInfo| settings.model_enabled(&s.agent, &m.id) || current == Some(m.id.as_str());
        let models = picker_models(&caps.models, settings.long_context, shown);
        let pins = settings.pins(&s.agent);
        // With long context on, a pin on a model also pins its `[1m]` row, which stands in for it.
        let is_pin = |m: &ModelInfo, pin: &str| m.id == pin || (settings.long_context && m.id == format!("{pin}{LONG_CONTEXT}"));
        let pinned = pins.iter().filter_map(|pin| models.iter().find(|m| is_pin(m, pin)));
        let rest = models.iter().filter(|m| !pins.iter().any(|pin| is_pin(m, pin)));

        // With long context on, a `[1m]` row pins its base model, so the pin also matches the ordering rule.
        let pin_target = |m: &ModelInfo| match m.id.strip_suffix(LONG_CONTEXT) {
            Some(base) if settings.long_context && models.iter().any(|x| x.id == base) => base.to_string(),
            _ => m.id.clone(),
        };

        let row = |label: String, desc: String, select: Option<String>, on: bool, pin: Option<(String, bool)>| {
            let id = id.clone();
            let agent = s.agent.clone();
            let group = format!("composer-model-row-{id}-{}", select.as_deref().unwrap_or("default"));
            let select_id = id.clone();
            let mut r = div()
                .id(gpui::SharedString::from(group.clone()))
                .group(group.clone())
                .flex()
                .items_center()
                .gap(px(10.))
                .px(px(10.))
                .py(px(8.))
                .rounded(px(6.))
                .cursor_pointer()
                .hover(|h| h.bg(theme.line2.to_hsla()))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.set_model(&select_id, select.clone(), cx)))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(1.))
                        .child(div().text_size(px(13.)).text_color(theme.fg.to_hsla()).truncate().child(label))
                        .when(!desc.is_empty(), |d| {
                            d.child(div().text_size(px(11.)).text_color(theme.muted.to_hsla()).truncate().child(desc))
                        }),
                );
            // Model rows carry a pin toggle that fades in on row hover; the agent-default row has no model.
            if let Some((target, pinned)) = pin {
                let (id, agent) = (id.clone(), agent.clone());
                let colour = if pinned { theme.acc2 } else { theme.muted };
                r = r.child(
                    div()
                        .id(gpui::SharedString::from(format!("composer-model-pin-{id}-{target}")))
                        .flex_none()
                        .p(px(2.))
                        .rounded(px(4.))
                        .cursor_pointer()
                        .opacity(if pinned { 1. } else { 0. })
                        .group_hover(group, |s| s.opacity(1.))
                        .hover(|h| h.bg(theme.line2.to_hsla()))
                        .tooltip(tooltip(if pinned { "Unpin" } else { "Pin" }, TooltipColors::of(theme)))
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            cx.stop_propagation();
                            this.toggle_model_pin(&agent, &target, cx);
                        }))
                        .child(stroke_icon(PIN, 12., 1.4, colour.to_hsla())),
                );
            }
            r = r.child(div().text_color(theme.acc2.to_hsla()).child(if on { "✓" } else { "" }));
            if on {
                r = r.bg(theme.soft.to_hsla());
            }
            r
        };

        let default_desc = caps.default_model.clone().unwrap_or_else(|| "Default model".into());
        let mut list = div()
            .id(gpui::SharedString::from(format!("composer-model-list-{id}")))
            // Keeps its rows when the panel is short; the panel scrolls instead.
            .flex_none()
            .flex()
            .flex_col()
            .gap(px(2.))
            .max_h(px(232.))
            .overflow_y_scroll()
            .child(row("Agent default".into(), default_desc, None, current.is_none(), None));
        for m in pinned.chain(rest) {
            let on = current == Some(m.id.as_str())
                || (settings.long_context && current.is_some_and(|c| m.id == format!("{c}{LONG_CONTEXT}")));
            let target = pin_target(m);
            let is_pinned = pins.iter().any(|pin| is_pin(m, pin));
            let desc = if !m.description.is_empty() {
                m.description.clone()
            } else {
                crate::engine::agent_name(&s.agent).to_string()
            };
            list = list.child(row(m.label.clone(), desc, Some(m.id.clone()), on, Some((target, is_pinned))));
        }
        list.into_any_element()
    }

    /// Pins or unpins `model` for `agent`, saving the settings; a toast when all `MAX_PINS` slots are taken.
    pub(super) fn toggle_model_pin(&mut self, agent: &str, model: &str, cx: &mut Context<Self>) {
        if !self.state.settings.toggle_pin(agent, model) {
            self.toast(format!("You can pin up to {MAX_PINS} models. Unpin one first."), cx);
            return;
        }
        self.save(cx);
    }

    /// The EFFORT block: the scale's name, the moon track and its labels, and the description.
    fn effort_block(&self, s: &Session, e: &EffortState, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = &self.theme;
        let n = e.levels.len();
        let fraction = e.fraction();
        let inset = 0.5 / n as f32;
        // The lit segment in fractions of the rail; the rail is `1 - 1/n` of the track.
        let fill = match (n, e.index) {
            (n, Some(i)) if n > 1 => i as f32 / (n - 1) as f32 * (1.0 - 1.0 / n as f32),
            _ => 0.0,
        };
        let rail_top = relative(0.5);
        let (acc, acc2) = (theme.acc, theme.acc2);

        let mut track = div().relative().h(px(44.)).grid().grid_cols(n as u16).child(
            div()
                .absolute()
                .left(relative(inset))
                .right(relative(inset))
                .top(rail_top)
                .mt(px(-1.))
                .h(px(2.))
                .rounded(px(1.))
                .bg(theme.line2.to_hsla()),
        );
        if fill > 0.0 {
            // The lit part's width follows an effort change over 0.25 s (layout.md §6 #6).
            track = track.child(anim::animated(
                (SharedString::from(format!("composer-effort-fill-{}", s.id)), 0usize),
                EFFORT_FILL,
                fill,
                move |fill| {
                    div()
                        .absolute()
                        .left(relative(inset))
                        .top(rail_top)
                        .mt(px(-1.))
                        .h(px(2.))
                        .w(relative(fill))
                        .rounded(px(1.))
                        .bg(linear_gradient(90., linear_color_stop(acc.to_hsla(), 0.), linear_color_stop(acc2.to_hsla(), 1.)))
                        .shadow(vec![BoxShadow {
                            color: acc.to_hsla(),
                            offset: point(px(0.), px(0.)),
                            blur_radius: px(10.),
                            spread_radius: px(0.),
                        }])
                        .into_any_element()
                },
            ));
        }
        let moons = e.levels.iter().enumerate().map(|(k, level)| {
            let reached = e.index.is_some_and(|ei| k <= ei);
            let selected = e.index == Some(k);
            let border = match (selected, reached) {
                (true, _) => theme.acc2,
                (false, true) => theme.line2,
                (false, false) => theme.line,
            };
            let id = s.id.clone();
            let level = level.clone();
            let pick: Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)> = Rc::new(cx.listener(move |this, _, _, cx| {
                this.set_effort(&id, Some(level.clone()), cx)
            }));
            let button_id = SharedString::from(format!("composer-moon-{}-{k}", s.id));
            let (faint, pop) = (theme.faint, theme.pop);
            let levels = e.levels.clone();
            // The moon's colour and its glow follow an effort change over 0.2 s (layout.md §6 #7).
            let button = anim::animated(
                (button_id.clone(), 0usize),
                EFFORT_NODE,
                (if reached { 1.0 } else { 0.0 }, if selected { 1.0 } else { 0.0 }),
                move |(lit, glow): (f32, f32)| {
                    let colour: Hsla = Hsla::lerp(&faint.to_hsla(), &acc2.to_hsla(), lit);
                    let pick = pick.clone();
                    div()
                        .id(button_id.clone())
                        .w(px(38.))
                        .h(px(38.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_full()
                        .bg(pop.to_hsla())
                        .border_1()
                        .border_color(border.to_hsla())
                        .cursor_pointer()
                        .shadow(vec![BoxShadow {
                            color: Hsla { a: glow, ..acc.to_hsla() },
                            offset: point(px(0.), px(0.)),
                            blur_radius: px(8.),
                            spread_radius: px(0.),
                        }])
                        .on_click(move |event, window, cx| pick(event, window, cx))
                        .child(moon(effort_fraction(&levels, k), 24., colour, colour.alpha(0.5), 0.8))
                        .into_any_element()
                },
            );
            div().flex().justify_center().child(button)
        });
        track = track.children(moons);

        let labels = e.levels.iter().enumerate().map(|(k, level)| {
            let colour = if e.index == Some(k) { theme.fg } else { theme.muted };
            div()
                .text_center()
                .text_size(px(11.5))
                .whitespace_nowrap()
                .text_color(colour.to_hsla())
                .child(level.clone())
        });
        let labels = div().grid().grid_cols(n as u16).children(labels);

        // A level outside the model's list has no description; the header shows the agent's name for it.
        let name = e.index.map(|i| e.levels[i].clone()).or_else(|| e.name.clone());
        let desc = match &name {
            Some(name) if e.index.is_some() => format!("{} {} has {n} levels.", effort_description(name, fraction), self.trigger_label(s)),
            _ => format!("{} has {n} levels.", self.trigger_label(s)),
        };
        div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .px(px(10.))
            .pt(px(12.))
            .pb(px(14.))
            .mt(px(4.))
            .border_t_1()
            .border_color(theme.line.to_hsla())
            .child(
                div()
                    .flex()
                    .items_baseline()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(theme.muted.to_hsla())
                            .child(format!("EFFORT · {n} LEVELS")),
                    )
                    .child(
                        div()
                            .font_family(theme.display_font.clone())
                            .text_size(px(22.))
                            .line_height(px(22.))
                            .text_color(theme.acc2.to_hsla())
                            .child(name.unwrap_or_else(|| "—".into())),
                    ),
            )
            .child(track)
            .child(labels)
            .child(div().text_size(px(12.)).text_center().text_color(theme.muted.to_hsla()).child(desc))
    }
}

/// The models the picker lists, in the agent's order. With `long_context`, a model whose `[1m]`
/// variant is listed shows as that variant, labelled "· 1M"; without it, a `[1m]` variant is hidden
/// when its base model is listed. `shown` says which models the settings and the session allow.
pub(super) fn picker_models(models: &[ModelInfo], long_context: bool, shown: impl Fn(&ModelInfo) -> bool) -> Vec<ModelInfo> {
    let mut rows = Vec::new();
    for m in models {
        match m.id.strip_suffix(LONG_CONTEXT) {
            // A variant of a listed model is reached through that model's row.
            Some(base) if models.iter().any(|x| x.id == base) => {}
            Some(_) => {
                if shown(m) {
                    rows.push(m.clone());
                }
            }
            None => {
                let variant = models.iter().find(|v| v.id == format!("{}{LONG_CONTEXT}", m.id));
                match variant {
                    Some(v) if long_context && shown(m) && shown(v) => {
                        rows.push(ModelInfo { label: format!("{} · 1M", m.label), ..v.clone() })
                    }
                    _ if shown(m) => rows.push(m.clone()),
                    _ => {}
                }
            }
        }
    }
    rows
}

/// The popover's shadow (chat.md §5.1): `0 14px 40px rgba(0,0,0,.45)`.
pub(super) fn popover_shadow() -> Vec<BoxShadow> {
    vec![BoxShadow {
        color: black().alpha(0.45),
        offset: point(px(0.), px(14.)),
        blur_radius: px(40.),
        spread_radius: px(0.),
    }]
}

/// A popover's uppercase label (`MODE`, `MODEL`).
pub(super) fn section_label(text: &str, theme: &Theme) -> gpui::Div {
    div()
        .px(px(8.))
        .py(px(6.))
        .text_size(px(11.))
        .text_color(theme.muted.to_hsla())
        .child(text.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(id: &str, label: &str) -> ModelInfo {
        ModelInfo { id: id.into(), label: label.into(), description: String::new(), efforts: Vec::new() }
    }

    fn ids(rows: &[ModelInfo]) -> Vec<(&str, &str)> {
        rows.iter().map(|m| (m.id.as_str(), m.label.as_str())).collect()
    }

    #[test]
    fn popover_is_a_known_pop_variant() {
        assert_ne!(Pop::Model, Pop::Attach);
    }

    #[test]
    fn without_long_context_the_1m_duplicate_is_hidden_when_its_base_is_listed() {
        let models = [model("opus", "Opus"), model("opus[1m]", "Opus 1M"), model("sonnet", "Sonnet")];
        let rows = picker_models(&models, false, |_| true);
        assert_eq!(ids(&rows), [("opus", "Opus"), ("sonnet", "Sonnet")]);

        let lone = [model("gpt[1m]", "GPT 1M")];
        assert_eq!(ids(&picker_models(&lone, false, |_| true)), [("gpt[1m]", "GPT 1M")], "no base, so it stays");
    }

    #[test]
    fn with_long_context_a_model_shows_as_its_1m_variant() {
        let models = [model("opus", "Opus"), model("opus[1m]", "Opus 1M"), model("sonnet", "Sonnet")];
        let rows = picker_models(&models, true, |_| true);
        assert_eq!(ids(&rows), [("opus[1m]", "Opus · 1M"), ("sonnet", "Sonnet")]);
    }

    #[test]
    fn a_disabled_variant_falls_back_to_its_base_and_a_disabled_base_hides_the_row() {
        let models = [model("opus", "Opus"), model("opus[1m]", "Opus 1M")];
        let no_variant = picker_models(&models, true, |m| m.id != "opus[1m]");
        assert_eq!(ids(&no_variant), [("opus", "Opus")]);
        let no_base = picker_models(&models, true, |m| m.id != "opus");
        assert!(no_base.is_empty(), "the variant is only offered through its base");
    }
}
