//! The Models page: defaults for new sessions, and which discovered models appear in the composer. Spec: settings.md §6.

use super::{
    controls::{act_button, chips, edit, empty_box, listen, more_row, search_box, section, segmented_row},
    flag_row, HARNESS_IDS,
};
use crate::ui::widgets::section::{card, section_head};
use crate::{
    engine,
    store::AgentDefaults,
    ui::{
        shell::Shell,
        widgets::toggle::Toggle,
    },
};
use gpui::{div, prelude::*, AnyElement, Context, SharedString, WeakEntity};
use crate::ui::scale::px;

/// The effort levels a session can start with, in segmented-control order: the id the
/// harness's `efforts` list spells (stored and passed to the CLI) and the label shown.
const EFFORTS: [(&str, &str); 4] = [("low", "Low"), ("medium", "Medium"), ("high", "High"), ("max", "Max")];

impl Shell {
    pub(super) fn models_page(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = self.theme.clone();
        let weak = cx.entity().downgrade();
        let default_id = self.default_harness_id();
        let effort = default_id
            .and_then(|id| self.state.settings.harness_defaults.get(id))
            .and_then(|d| d.effort.clone())
            .unwrap_or_else(|| "medium".to_string());
        let effort_index = EFFORTS.iter().position(|(id, _)| *id == effort).unwrap_or(1);
        let labels: Vec<&str> = EFFORTS.iter().map(|(_, label)| *label).collect();
        let effort_row = segmented_row(
            "default-effort",
            "Default effort",
            "",
            &labels,
            effort_index,
            &weak,
            &theme,
            |this, i, cx| {
                let Some(id) = this.default_harness_id() else { return };
                let id = id.to_string();
                let defaults: &mut AgentDefaults = this.state.settings.harness_defaults.entry(id).or_default();
                defaults.effort = EFFORTS.get(i).map(|(level, _)| (*level).to_string());
                this.save(cx);
            },
            default_id.is_none().then_some("Enable a harness first."),
        );

        let fast_titles = flag_row(
            "fast-titles",
            "Use a fast model for titles and summaries",
            "",
            self.state.settings.fast_titles,
            &weak,
            &theme,
            |s, on| s.fast_titles = on,
        );
        let long_context = flag_row(
            "long-context",
            "Prefer long-context variants",
            "",
            self.state.settings.long_context,
            &weak,
            &theme,
            |s, on| s.long_context = on,
        );

        vec![section("Defaults", vec![effort_row, fast_titles, long_context], &theme), self.settings_model_list(&weak, cx)]
    }

    /// Whether the harness `id` is installed and switched on.
    fn harness_enabled(&self, id: &str) -> bool {
        self.agents.iter().any(|a| a.harness.id() == id && a.enabled)
    }

    /// The harness that new sessions use: the chosen default if it's installed and enabled, else the
    /// first enabled one.
    pub(super) fn default_harness_id(&self) -> Option<&'static str> {
        crate::ui::shell::pick_harness(self.state.settings.default_harness.as_deref(), &self.agents)
    }

    /// Every discovered model in one list: searchable, filtered by agent, ten at a time.
    fn settings_model_list(&mut self, weak: &WeakEntity<Shell>, cx: &mut Context<Self>) -> AnyElement {
        let theme = self.theme.clone();
        let all = self.all_models();
        let mut agents: Vec<(&'static str, &'static str)> = Vec::new();
        for m in &all {
            if !agents.iter().any(|(id, _)| *id == m.harness) {
                agents.push((m.harness, m.agent));
            }
        }
        let chip = self.settings.chip.get("models").copied().unwrap_or(0).min(agents.len());
        let only = chip.checked_sub(1).and_then(|i| agents.get(i)).map(|(id, _)| *id);
        let query = self.query("models", cx);
        let shown: Vec<&ModelRow> = all
            .iter()
            .filter(|m| only.is_none_or(|id| m.harness == id))
            .filter(|m| query.is_empty() || format!("{} {}", m.label, m.description).to_lowercase().contains(&query))
            .collect();
        let on = shown.iter().filter(|m| m.on).count();
        let keys: Vec<(&'static str, String)> = shown.iter().map(|m| (m.harness, m.id.clone())).collect();
        let (enable, disable) = (keys.clone(), keys);
        let acts = vec![
            act_button("models-enable-all", "Enable all", None, &theme, listen(cx, move |this, cx| this.bulk_models(&enable, true, cx))),
            act_button("models-disable-all", "Disable all", None, &theme, listen(cx, move |this, cx| this.bulk_models(&disable, false, cx))),
        ];
        let head = section_head(format!("Models · {}", shown.len()), Some(format!("{on} on").into()), acts, &theme);
        let search = self.settings.searches.get("models").map(|(input, _)| search_box(input, &theme));
        let mut names: Vec<String> = vec!["All".into()];
        names.extend(agents.iter().map(|(_, n)| n.to_string()));
        let chips = chips("model-chip", &names, chip, &theme, edit(weak, |this: &mut Shell, i: usize, cx| this.pick_chip("models", i, cx)));

        let limit = self.limit("models", 10);
        let left = shown.len().saturating_sub(limit);
        let muted = theme.muted.to_hsla();
        let mut rows = vec![model_line(
            div().text_size(px(12.)).text_color(muted).py(px(10.)),
            div().child("Model"),
            div().child("Agent"),
            div().flex().justify_center().child("Default"),
            div().flex().justify_end().child("On"),
            &theme,
        )];
        for m in shown.iter().take(limit) {
            let (h, radio_id, toggle_id) = (m.harness, m.id.clone(), m.id.clone());
            let ring = if m.default { theme.acc } else { theme.fg.alpha(0.35) };
            let radio = div()
                .id(SharedString::from(format!("radio-{h}-{}", m.id)))
                .size(px(16.))
                .rounded_full()
                .border(px(1.5))
                .border_color(ring.to_hsla())
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| this.set_default_model(h, radio_id.clone(), cx)))
                .when(m.default, |d| d.child(div().size(px(8.)).rounded_full().bg(theme.acc.to_hsla())));
            let toggle = Toggle::new(SharedString::from(format!("model-on-{h}-{}", m.id)), m.on, &theme).on_change(edit(
                weak,
                move |this: &mut Shell, value: bool, cx| this.set_model_enabled(h, toggle_id.clone(), value, cx),
            ));
            rows.push(
                model_line(
                    div().py(px(12.)).when(!m.harness_on, |d| d.opacity(0.45)),
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .min_w(px(0.))
                        .child(m.label.clone())
                        .when(!m.description.is_empty(), |d| d.child(div().text_size(px(12.)).text_color(muted).child(m.description.clone()))),
                    div().text_size(px(12.)).text_color(muted).child(m.agent),
                    div().flex().justify_center().child(radio),
                    div().flex().justify_end().child(toggle),
                    &theme,
                ),
            );
        }
        if left > 0 {
            rows.push(more_row("models-more", 10, left, &theme, listen(cx, |this, cx| this.show_more("models", 10, 10, cx))));
        }
        let body = if all.is_empty() {
            empty_box(None, "Enable a harness to see its models", &theme)
        } else if shown.is_empty() {
            empty_box(None, "No matching models", &theme)
        } else {
            card(rows, &theme).into_any_element()
        };
        div().flex().flex_col().child(head).children(search).child(chips).child(body).into_any_element()
    }

    /// Every model of every harness that lists its models, in harness order.
    fn all_models(&self) -> Vec<ModelRow> {
        let default_id = self.default_harness_id();
        let default_model = default_id.and_then(|id| self.default_model_of(id));
        let mut out = Vec::new();
        for id in HARNESS_IDS {
            let Some(Some(caps)) = self.caps.get(id) else { continue };
            let harness_on = self.agents.iter().any(|a| a.harness.id() == id && a.enabled);
            let agent = engine::harness(id).map_or(id, |h| h.name());
            for model in &caps.models {
                out.push(ModelRow {
                    harness: id,
                    agent,
                    harness_on,
                    id: model.id.clone(),
                    label: model.label.clone(),
                    description: model.description.clone(),
                    on: self.state.settings.model_enabled(id, &model.id),
                    default: default_id == Some(id) && default_model.as_deref() == Some(model.id.as_str()),
                });
            }
        }
        out
    }

    /// The model new sessions on `harness` start with: the one picked here, else the harness's own default.
    fn default_model_of(&self, harness: &str) -> Option<String> {
        let chosen = self.state.settings.harness_defaults.get(harness).and_then(|d| d.model.clone());
        chosen.or_else(|| self.caps.get(harness).and_then(Option::as_ref).and_then(|c| c.default_model.clone()))
    }

    /// Turns every model in `models` on or off. Off keeps one per harness on: its default when that is
    /// among them, else its first, so the message box never runs out of models.
    fn bulk_models(&mut self, models: &[(&'static str, String)], on: bool, cx: &mut Context<Self>) {
        for (harness, model) in models {
            self.state.settings.model_enabled.entry(harness.to_string()).or_default().insert(model.clone(), on);
        }
        if !on {
            let mut seen: Vec<&'static str> = Vec::new();
            for (harness, _) in models {
                if seen.contains(harness) {
                    continue;
                }
                seen.push(harness);
                let Some(Some(caps)) = self.caps.get(*harness) else { continue };
                let keep = self
                    .default_model_of(harness)
                    .filter(|d| caps.models.iter().any(|m| &m.id == d))
                    .or_else(|| caps.models.first().map(|m| m.id.clone()));
                if let Some(keep) = keep {
                    self.state.settings.model_enabled.entry(harness.to_string()).or_default().insert(keep, true);
                }
            }
        }
        self.save(cx);
    }

    /// Makes `model` the default for new sessions, on its harness, and enables it.
    fn set_default_model(&mut self, harness: &'static str, model: String, cx: &mut Context<Self>) {
        if !self.harness_enabled(harness) {
            self.toast("Enable this harness first", cx);
            return;
        }
        let s = &mut self.state.settings;
        s.default_harness = Some(harness.to_string());
        s.harness_defaults.entry(harness.to_string()).or_default().model = Some(model.clone());
        s.model_enabled.entry(harness.to_string()).or_default().insert(model, true);
        self.save(cx);
    }

    /// Shows or hides one model in the composer. Guarded while its harness is disabled.
    fn set_model_enabled(&mut self, harness: &'static str, model: String, on: bool, cx: &mut Context<Self>) {
        if !self.harness_enabled(harness) {
            self.toast("Enable this harness first", cx);
            return;
        }
        if !on {
            let others_on = self.caps.get(harness).and_then(Option::as_ref).map_or(0, |caps| {
                caps.models.iter().filter(|m| m.id != model && self.state.settings.model_enabled(harness, &m.id)).count()
            });
            if others_on == 0 {
                let name = engine::harness(harness).map_or(harness, |h| h.name());
                self.toast(format!("Keep at least one {name} model on"), cx);
                return;
            }
        }
        self.state.settings.model_enabled.entry(harness.to_string()).or_default().insert(model, on);
        self.save(cx);
    }
}

/// One model as the list shows it.
struct ModelRow {
    harness: &'static str,
    agent: &'static str,
    harness_on: bool,
    id: String,
    label: String,
    description: String,
    on: bool,
    default: bool,
}

/// A line of the models list: model, agent, default and on, in the prototype's columns.
fn model_line(
    row: gpui::Div,
    model: gpui::Div,
    agent: gpui::Div,
    default: gpui::Div,
    on: gpui::Div,
    theme: &crate::ui::theme::Theme,
) -> AnyElement {
    row.flex()
        .items_center()
        .gap(px(12.))
        .px(px(18.))
        .bg(theme.tile.to_hsla())
        .child(model.flex_1().min_w(px(0.)))
        .child(agent.flex_none().w(px(100.)))
        .child(default.flex_none().w(px(60.)))
        .child(on.flex_none().w(px(48.)))
        .into_any_element()
}

/// The `[1m]` variant of `model` when `long_context` is on and `lists` says the harness offers it; otherwise `model`.
pub(crate) fn long_context_model(model: &str, long_context: bool, lists: impl Fn(&str) -> bool) -> String {
    let variant = format!("{model}[1m]");
    if long_context && !model.ends_with("[1m]") && lists(&variant) {
        variant
    } else {
        model.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::{long_context_model, EFFORTS};

    #[test]
    fn efforts_are_stored_as_harness_ids_and_shown_title_cased() {
        // Saved to settings and passed to the CLI: the harness's own lower-case id.
        assert_eq!(EFFORTS.map(|(id, _)| id), ["low", "medium", "high", "max"]);
        // The segmented row still reads as a title-cased label.
        assert_eq!(EFFORTS.map(|(_, label)| label), ["Low", "Medium", "High", "Max"]);
    }

    #[test]
    fn long_context_uses_the_1m_variant_only_when_on_and_listed() {
        let listed = |id: &str| id == "opus[1m]";
        assert_eq!(long_context_model("opus", true, listed), "opus[1m]");
        assert_eq!(long_context_model("opus", false, listed), "opus");
        assert_eq!(long_context_model("sonnet", true, listed), "sonnet");
        assert_eq!(long_context_model("opus[1m]", true, listed), "opus[1m]");
    }
}
