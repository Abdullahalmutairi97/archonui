//! The Harnesses page: one card per agent CLI, with its path, sign-in, status and enable toggle. Spec: settings.md §5.

use super::{
    controls::{link, listen, status},
    AgentJob, HARNESS_IDS,
};
use crate::{
    engine,
    ui::{
        composer::icons::harness_icon,
        shell::Shell,
        theme::{AMBER, GREEN},
    },
};
use gpui::{div, prelude::*, AnyElement, Context, SharedString, Stateful};
use crate::ui::scale::px;

/// Who makes each harness, under its name.
fn harness_blurb(id: &str) -> &'static str {
    match id {
        "claude" => "Anthropic",
        "codex" => "OpenAI",
        "gemini" => "Google",
        "opencode" => "Any provider",
        _ => "Minimal terminal harness",
    }
}

impl Shell {
    pub(super) fn harnesses_page(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = self.theme.clone();
        let (radius, border) = crate::ui::widgets::section::card_style();
        let cols = super::columns(self.settings.body_w, 310., 12., 4);
        let mut grid = div().grid().grid_cols(cols).gap(px(12.));
        for id in HARNESS_IDS {
            let Some(harness) = engine::harness(id) else { continue };
            let agent = self.agents.iter().find(|a| a.harness.id() == id);
            let installed = agent.is_some();
            let on = agent.is_some_and(|a| a.enabled);
            let (word, dot) = match (installed, on) {
                (true, true) => ("Ready", GREEN),
                (true, false) => ("Disabled", theme.fg.alpha(0.4)),
                (false, _) => ("Not installed", AMBER),
            };
            let mut card = div()
                .min_w(px(0.))
                .flex()
                .flex_col()
                .gap(px(14.))
                .p(px(16.))
                .rounded(px(radius))
                .bg(theme.tile.to_hsla())
                .when(border, |d| d.border_1().border_color(theme.line.to_hsla()))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(12.))
                        .child(
                            div()
                                .flex_none()
                                .size(px(36.))
                                .rounded(px(9.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .bg(theme.line.to_hsla())
                                .child(harness_icon(id, 18., 1.4, theme.fg.to_hsla())),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .flex()
                                .flex_col()
                                .gap(px(2.))
                                .child(harness.name())
                                .child(div().text_size(px(12.)).text_color(theme.muted.to_hsla()).child(harness_blurb(id))),
                        )
                        .child(status(dot, word, &theme)),
                );
            let job = self.settings.agent_job.filter(|(job_id, _)| *job_id == id).map(|(_, job)| job);
            // Not installed: one button installs it.
            if agent.is_none() {
                let word = if job == Some(AgentJob::Install) { "Installing…" } else { "Install" };
                card = card.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(14.))
                        .pt(px(12.))
                        .border_t_1()
                        .border_color(theme.line.to_hsla())
                        .child(div().flex_1().min_w(px(0.)).text_size(px(12.)).text_color(theme.muted.to_hsla()).child("Installs for your user only"))
                        .child(card_button(SharedString::from(format!("harness-install-{id}")), word, &theme).on_click(cx.listener(move |this, _, _, cx| this.agent_job(id, AgentJob::Install, cx)))),
                );
            }
            if let Some(agent) = agent {
                let path = format!("{} · {}", super::general::tilde(&agent.path), agent.version);
                let auth = agent.auth.label().to_string();
                let label = |t: &str| div().text_color(theme.muted.to_hsla()).child(t.to_string()).w(px(44.)).flex_none();
                let key_row = |key: &str, value: gpui::Div| div().flex().gap(px(12.)).child(label(key)).child(value);
                card = card.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .text_size(px(12.))
                        .child(key_row(
                            "Path",
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .font_family(theme.code_font.clone())
                                .overflow_hidden()
                                .text_ellipsis()
                                .whitespace_nowrap()
                                .child(path),
                        ))
                        .child(key_row(
                            "Auth",
                            div().flex_1().min_w(px(0.)).flex().gap(px(10.)).child(auth).when(agent.auth == engine::AuthStatus::Missing, |d| {
                                d.child(link(SharedString::from(format!("sign-in-{id}")), "Sign in", theme.acc2, listen(cx, move |this, cx| this.sign_in(id, cx))))
                            }),
                        )),
                );
                let testing = self.settings.testing == Some(id);
                let worked = self.settings.tested == Some(id);
                let (test_word, test_colour) = if testing {
                    ("Testing…", theme.acc2)
                } else if worked {
                    ("Works ✓", GREEN)
                } else {
                    ("Test", theme.acc2)
                };
                card = card.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(14.))
                        .pt(px(12.))
                        .border_t_1()
                        .border_color(theme.line.to_hsla())
                        .child(div().flex_1().min_w(px(0.)).text_size(px(12.)).text_color(theme.muted.to_hsla()).child(self.model_summary(id, true)))
                        .when(on, |d| d.child(link(SharedString::from(format!("test-{id}")), test_word, test_colour, listen(cx, move |this, cx| this.test_harness(id, cx)))))
                        .child(link(
                            SharedString::from(format!("update-{id}")),
                            if job == Some(AgentJob::Update) { "Updating…" } else { "Update" },
                            theme.acc2,
                            listen(cx, move |this, cx| this.agent_job(id, AgentJob::Update, cx)),
                        ))
                        .child(
                            card_button(SharedString::from(format!("harness-action-{id}")), if on { "Disable" } else { "Enable" }, &theme)
                                .on_click(cx.listener(move |this, _, _, cx| this.harness_action(id, true, on, cx))),
                        ),
                );
            }
            grid = grid.child(card);
        }
        vec![crate::ui::widgets::section::section_head("Agents", None, Vec::new(), &theme), grid.into_any_element()]
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
                    Ok(_) => {
                        // "Works ✓" shows for a few seconds, then the link reads Test again.
                        this.settings.tested = Some(id);
                        cx.spawn(async move |this, cx| {
                            cx.background_executor().timer(std::time::Duration::from_millis(2400)).await;
                            let _ = this.update(cx, |this, cx| {
                                if this.settings.tested == Some(id) {
                                    this.settings.tested = None;
                                    cx.notify();
                                }
                            });
                        })
                        .detach();
                    }
                    Err(e) => this.toast(format!("{name} {version} failed: {e}"), cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Installs or updates harness `id` off the UI thread, then says how it went and rescans. A step that
    /// needs root shows the desktop's password prompt (pkexec); everything else runs as this user.
    fn agent_job(&mut self, id: &'static str, job: AgentJob, cx: &mut Context<Self>) {
        let name = engine::agent_name(id);
        if let Some((busy, _)) = self.settings.agent_job {
            self.toast(format!("Wait for {} to finish", engine::agent_name(busy)), cx);
            return;
        }
        let steps = match job {
            AgentJob::Install => engine::install::install_steps(id, &engine::install::Host::here()),
            AgentJob::Update => match self.agents.iter().find(|a| a.harness.id() == id) {
                Some(agent) => engine::install::update_steps(id, &agent.path),
                None => return,
            },
        };
        let steps = match steps {
            Ok(steps) => steps,
            Err(e) => return self.toast(e, cx),
        };
        if steps.iter().any(|s| s.root) {
            self.toast("Your system will ask for your password", cx);
        }
        self.settings.agent_job = Some((id, job));
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { engine::install::run(&steps, &|line| eprintln!("archonui: {id}: {line}")) })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.settings.agent_job = None;
                match (result, job) {
                    (Ok(()), AgentJob::Install) => this.toast(format!("Installed {name}. Sign in from its card"), cx),
                    (Ok(()), AgentJob::Update) => this.toast(format!("Updated {name}"), cx),
                    (Err(e), _) => this.toast(e, cx),
                }
                this.rescan(cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// Runs harness `id`'s sign-in in the Terminal tab of the focused chat, and closes Settings so it shows.
    fn sign_in(&mut self, id: &'static str, cx: &mut Context<Self>) {
        let Some(command) = engine::install::sign_in_command(id) else { return };
        if self.run_in_terminal(command, cx) {
            self.settings_close(cx);
        } else {
            self.toast(format!("Open a chat, then Sign in again, or run `{command}` in a terminal"), cx);
        }
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

/// The small bordered button at the bottom right of a card.
fn card_button(id: SharedString, text: &'static str, theme: &crate::ui::theme::Theme) -> Stateful<gpui::Div> {
    let line = theme.line.to_hsla();
    div()
        .id(id)
        .px(px(12.))
        .py(px(5.))
        .rounded(px(7.))
        .text_size(px(12.))
        .border_1()
        .border_color(theme.line2.to_hsla())
        .cursor_pointer()
        .hover(move |s| s.bg(line))
        .child(text)
}
