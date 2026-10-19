//! The Models page: defaults for new sessions, and which discovered models appear in the composer. Spec: settings.md §6.

use super::{
    controls::{edit, section, segmented_row, toggle_row},
    HARNESS_IDS,
};
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

        let fast_titles = toggle_row(
            "fast-titles",
            "Use a fast model for titles and summaries",
            "",
            self.state.settings.fast_titles,
            &weak,
            &theme,
            |this, on, cx| {
                this.state.settings.fast_titles = on;
                this.save(cx);
            },
            None,
        );

        let long_context = toggle_row(
            "long-context",
            "Prefer long-context variants",
            "",
            self.state.settings.long_context,
            &weak,
            &theme,
            |this, on, cx| {
                this.state.settings.long_context = on;
                this.save(cx);
            },
            None,
        );

        let rows = self.model_rows(&weak, cx);
        let table = if rows.is_empty() {
            div()
                .p(px(16.))
                .text_size(px(12.5))
                .text_color(theme.muted.to_hsla())
                .child("Enable a harness to see models")
                .into_any_element()
        } else {
            div()
                .flex()
                .flex_col()
                .rounded(px(10.))
                .border_1()
                .border_color(theme.line2.to_hsla())
                .overflow_hidden()
                .child(table_header(self.settings.body_w >= HARNESS_COLUMN_MIN, &theme))
                .children(rows)
                .into_any_element()
        };

        vec![
            section("DEFAULTS", vec![effort_row, fast_titles, long_context], &theme),
            div()
                .flex()
                .flex_col()
                .gap(px(12.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(16.))
                        .child(
                            div()
                                .flex_none()
                                .text_size(px(11.))
                                .text_color(theme.muted.to_hsla())
                                .child("MODELS"),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .text_right()
                                .text_size(px(12.))
                                .text_color(theme.muted.to_hsla())
                                .child("Only enabled appear in composer"),
                        ),
                )
                .child(table)
                .into_any_element(),
        ]
    }

    /// The harness that new sessions use: the chosen default if it's installed and enabled, else the
    /// first enabled one.
    pub(super) fn default_harness_id(&self) -> Option<&'static str> {
        crate::ui::shell::pick_harness(self.state.settings.default_harness.as_deref(), &self.agents)
    }

    /// One row per discovered model. Rows of a disabled harness are dimmed and their controls only explain why.
    fn model_rows(&mut self, weak: &WeakEntity<Shell>, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = self.theme.clone();
        let default_id = self.default_harness_id();
        let default_model = default_id.and_then(|id| {
            let chosen = self.state.settings.harness_defaults.get(id).and_then(|d| d.model.clone());
            chosen.or_else(|| self.caps.get(id).and_then(Option::as_ref).and_then(|c| c.default_model.clone()))
        });
        // A narrow page names the harness under each model instead of in a column of its own.
        let wide = self.settings.body_w >= HARNESS_COLUMN_MIN;
        let mut rows = Vec::new();
        for id in HARNESS_IDS {
            let Some(Some(caps)) = self.caps.get(id) else { continue };
            let agent = self.agents.iter().find(|a| a.harness.id() == id);
            let harness_name = engine::harness(id).map_or(id, |h| h.name());
            let harness_on = agent.is_some_and(|a| a.enabled);
            for model in caps.models.iter() {
                let model_id = model.id.clone();
                let is_default = default_id == Some(id) && default_model.as_deref() == Some(model.id.as_str());
                let on = self.state.settings.model_enabled(id, &model.id);
                let radio_id = model_id.clone();
                let toggle_id = model_id.clone();
                let radio = div()
                    .id(SharedString::from(format!("radio-{id}-{}", model.id)))
                    .w(px(14.))
                    .h(px(14.))
                    .rounded_full()
                    .border_2()
                    .border_color(if is_default { theme.acc.to_hsla() } else { theme.faint.to_hsla() })
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.set_default_model(id, radio_id.clone(), cx);
                    }))
                    .when(is_default, |d| d.child(div().w(px(6.)).h(px(6.)).rounded_full().bg(theme.acc.to_hsla())));
                let toggle = Toggle::new(SharedString::from(format!("model-on-{id}-{}", model.id)), on, &theme).on_change(edit(
                    weak,
                    move |this: &mut Shell, value: bool, cx| this.set_model_enabled(id, toggle_id.clone(), value, cx),
                ));
                rows.push(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(16.))
                        .pr(px(16.))
                        .py(px(11.))
                        .border_t_1()
                        .border_color(theme.line.to_hsla())
                        .when(!harness_on, |d| d.opacity(0.45))
                        .child(div().flex_none().w(px(48.)).flex().justify_center().child(radio))
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .flex()
                                .flex_col()
                                .child(div().text_size(px(13.5)).child(model.label.clone()))
                                .when(!wide, |col| {
                                    col.child(div().text_size(px(11.5)).text_color(theme.alt.to_hsla()).child(harness_name))
                                })
                                .child(
                                    div()
                                        .text_size(px(11.5))
                                        .text_color(theme.muted.to_hsla())
                                        .child(model.description.clone()),
                                ),
                        )
                        .when(wide, |row| {
                            row.child(
                                div()
                                    .flex_none()
                                    .w(px(200.))
                                    .text_size(px(12.5))
                                    .text_color(theme.alt.to_hsla())
                                    .child(harness_name),
                            )
                        })
                        .child(div().flex_none().w(px(64.)).flex().justify_end().child(toggle))
                        .into_any_element(),
                );
            }
        }
        rows
    }

    /// Makes `model` the default for new sessions, on its harness, and enables it.
    fn set_default_model(&mut self, harness: &'static str, model: String, cx: &mut Context<Self>) {
        if !self.agents.iter().any(|a| a.harness.id() == harness && a.enabled) {
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
        if !self.agents.iter().any(|a| a.harness.id() == harness && a.enabled) {
            self.toast("Enable this harness first", cx);
            return;
        }
        self.state.settings.model_enabled.entry(harness.to_string()).or_default().insert(model, on);
        self.save(cx);
    }
}

/// The page width from which the models table has a HARNESS column, in design px.
const HARNESS_COLUMN_MIN: f32 = 600.;

/// The MODEL / HARNESS / ON header line, above the rows. Without `harness_column` the harness is named
/// in each row instead.
fn table_header(harness_column: bool, theme: &crate::ui::theme::Theme) -> AnyElement {
    let label = |text: &str| div().text_size(px(10.5)).text_color(theme.muted.to_hsla()).child(text.to_string());
    div()
        .flex()
        .items_center()
        .gap(px(16.))
        .py(px(10.))
        .pr(px(16.))
        .bg(theme.line.to_hsla())
        .child(div().flex_none().w(px(48.)))
        .child(div().flex_1().child(label("MODEL")))
        .when(harness_column, |row| row.child(div().flex_none().w(px(200.)).child(label("HARNESS"))))
        .child(div().flex_none().w(px(64.)).flex().justify_end().child(label("ON")))
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
