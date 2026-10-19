//! The Harnesses page: one card per agent CLI, with its path, sign-in, status and enable toggle. Spec: settings.md §5.

use super::{controls::listen, HARNESS_IDS};
use crate::{
    engine,
    ui::{
        composer::icons::harness_icon,
        shell::Shell,
        widgets::button::{Button, ButtonStyle},
    },
};
use gpui::{div, prelude::*, AnyElement, Context, SharedString};
use crate::ui::scale::px;

/// The one-line description for each harness the page lists.
fn harness_blurb(id: &str) -> &'static str {
    match id {
        "claude" => "Anthropic · Claude models",
        "codex" => "OpenAI · GPT models",
        "gemini" => "Google · Gemini models",
        "opencode" => "Any provider, including local Ollama",
        _ => "Minimal terminal agent, any provider",
    }
}

impl Shell {
    pub(super) fn harnesses_page(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = self.theme.clone();
        // Two cards a row while each gets room for its name, blurb and status; one below that.
        let cols = super::columns(self.settings.body_w, 340., 14., 2);
        let mut grid = div().grid().grid_cols(cols).gap(px(14.)).pt(px(18.));
        for id in HARNESS_IDS {
            let Some(harness) = engine::harness(id) else { continue };
            let agent = self.agents.iter().find(|a| a.harness.id() == id);
            let installed = agent.is_some();
            let on = agent.is_some_and(|a| a.enabled);
            let blurb = harness_blurb(id);
            let (status, status_colour) = match (installed, on) {
                (true, true) => ("Ready", theme.alt),
                (true, false) => ("Disabled", theme.faint),
                (false, _) => ("Not installed", theme.faint),
            };
            let path = match agent {
                Some(a) => format!("{} · {}", a.path.display(), a.version),
                None => "Not found on PATH".to_string(),
            };
            let auth = agent.map_or("—".to_string(), |a| a.auth.label().to_string());
            let summary = self.model_summary(id, installed);

            let mut footer = div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(8.))
                .child(div().flex_1().min_w(px(0.)).text_size(px(12.)).text_color(theme.muted.to_hsla()).child(summary));
            if on {
                footer = footer.child(
                    div()
                        .id(SharedString::from(format!("test-{id}")))
                        .px(px(12.))
                        .py(px(7.))
                        .rounded(px(6.))
                        .text_size(px(12.))
                        .text_color(theme.muted.to_hsla())
                        .cursor_pointer()
                        .hover(|s| s.bg(theme.line2.to_hsla()).text_color(theme.fg.to_hsla()))
                        .on_click(cx.listener(move |this, _, _, cx| this.test_harness(id, cx)))
                        .child(if self.settings.testing == Some(id) { "Testing…" } else { "Test" }),
                );
            }
            let (label, style) = match (installed, on) {
                (true, true) => ("Disable", ButtonStyle::Outline),
                (true, false) => ("Enable", ButtonStyle::Filled),
                (false, _) => ("Install", ButtonStyle::Filled),
            };
            let action = Button::new(SharedString::from(format!("harness-action-{id}")), label, &theme)
                .style(style)
                .on_click(listen(cx, move |this, cx| this.harness_action(id, installed, on, cx)));
            footer = footer.child(action);

            grid = grid.child(
                div()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .gap(px(14.))
                    .p(px(18.))
                    .rounded(px(12.))
                    .border_1()
                    .border_color(if on { theme.line2.to_hsla() } else { theme.line.to_hsla() })
                    .when(on, |d| d.bg(theme.line.to_hsla()))
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap(px(14.))
                            .child(
                                div()
                                    .flex_none()
                                    .w(px(42.))
                                    .h(px(42.))
                                    .rounded(px(10.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .bg(theme.line2.to_hsla())
                                    .child(harness_icon(id, 20., 1.5, if on { theme.acc2.to_hsla() } else { theme.muted.to_hsla() })),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .flex_basis(px(140.))
                                    .min_w(px(0.))
                                    .flex()
                                    .flex_col()
                                    .child(div().text_size(px(16.)).child(harness.name()))
                                    .child(div().text_size(px(12.)).text_color(theme.muted.to_hsla()).child(blurb)),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .flex()
                                    .items_center()
                                    .gap(px(7.))
                                    .text_size(px(12.))
                                    .text_color(status_colour.to_hsla())
                                    .child(div().w(px(7.)).h(px(7.)).rounded_full().bg(status_colour.to_hsla()))
                                    .child(status),
                            ),
                    )
                    .child(info_row("Path", &path, installed, &theme))
                    .child(info_row("Auth", &auth, installed, &theme))
                    .child(footer),
            );
        }
        grid = grid.child(
            div()
                .col_span_full()
                .p(px(16.))
                .rounded(px(12.))
                .border_1()
                .border_dashed()
                .border_color(theme.line2.to_hsla())
                .text_size(px(13.))
                .text_color(theme.muted.to_hsla())
                .cursor_pointer()
                .id("add-harness")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.toast("Add any ACP-compatible CLI", cx);
                }))
                .child("+ Add harness"),
        );
        vec![grid.into_any_element()]
    }

    /// "N models · K enabled" once discovery has run, or what the harness uses instead.
    fn model_summary(&self, id: &str, installed: bool) -> String {
        if !installed {
            return "Uses its own model config".to_string();
        }
        match self.caps.get(id) {
            None => "Discovering models…".to_string(),
            Some(None) => "Uses its own model config".to_string(),
            Some(Some(caps)) if caps.models.is_empty() => "Uses its own model config".to_string(),
            Some(Some(caps)) => {
                let enabled = caps.models.iter().filter(|m| self.state.settings.model_enabled(id, &m.id)).count();
                format!("{} models · {} enabled", caps.models.len(), enabled)
            }
        }
    }

    /// Runs the harness's connection check off the UI thread, then reports the result in a toast.
    fn test_harness(&mut self, id: &'static str, cx: &mut Context<Self>) {
        // One check at a time: a second click would start another CLI while the first still runs.
        if self.settings.testing.is_some() {
            return;
        }
        let Some(agent) = self.agents.iter().find(|a| a.harness.id() == id) else { return };
        let (path, name, version) = (agent.path.clone(), agent.harness.name(), agent.version.clone());
        let cwd = self.default_folder();
        self.settings.testing = Some(id);
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let harness = engine::harness(id).ok_or_else(|| "unknown harness".to_string())?;
                    harness.test_connection(&path, &cwd)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.settings.testing = None;
                match result {
                    Ok(_) => this.toast(format!("{name} {version} · handshake OK"), cx),
                    Err(e) => this.toast(format!("{name} failed: {e}"), cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Enable or disable a harness, or explain how to install one that isn't on PATH. Enabling rescans.
    fn harness_action(&mut self, id: &'static str, installed: bool, on: bool, cx: &mut Context<Self>) {
        let name = engine::harness(id).map_or(id, |h| h.name());
        if !installed {
            let binary = engine::harness(id).map_or("", |h| h.binary());
            self.toast(format!("Install {name}, then put `{binary}` on PATH"), cx);
            return;
        }
        self.state.settings.harnesses.entry(id.to_string()).or_default().enabled = !on;
        self.toast(if on { format!("Disabled {name}") } else { format!("Enabled {name}") }, cx);
        self.save(cx);
        self.rescan(cx);
    }
}

/// A read-only label and value line on a harness card.
fn info_row(label: &str, value: &str, installed: bool, theme: &crate::ui::theme::Theme) -> AnyElement {
    div()
        .flex()
        .items_center()
        .gap(px(12.))
        .px(px(12.))
        .py(px(9.))
        .rounded(px(8.))
        .border_1()
        .border_color(theme.line.to_hsla())
        .bg(theme.line.to_hsla())
        .child(div().w(px(62.)).flex_none().text_size(px(11.)).text_color(theme.muted.to_hsla()).child(label.to_string()))
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .overflow_hidden()
                .text_ellipsis()
                .whitespace_nowrap()
                .text_size(px(12.5))
                .text_color(if installed { theme.alt.to_hsla() } else { theme.faint.to_hsla() })
                .child(value.to_string()),
        )
        .into_any_element()
}
