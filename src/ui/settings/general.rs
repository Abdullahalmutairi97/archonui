//! The General page: startup, agent behaviour, notifications, updates and data. Spec: settings.md §2.

use super::{
    controls::{button_row, listen, section, segmented_row, toggle_row},
    Slide,
};
use crate::{
    app::settings::{AppSettings, McpTransport, UpdateChannel, MAX_STEPS},
    engine::Mode,
    ui::{
        shell::Shell,
        theme::Theme,
        widgets::{
            button::{Button, ButtonStyle},
            section::SettingRow,
            slider::SliderRange,
        },
    },
};
use gpui::{div, prelude::*, AnyElement, Context, PathPromptOptions};
use crate::ui::scale::px;

/// The channels in the order the segmented control lists them.
const CHANNELS: [UpdateChannel; 3] = [UpdateChannel::Stable, UpdateChannel::Beta, UpdateChannel::Nightly];

/// `settings` with every MCP secret blanked, so an export can be shared. Key and
/// header names are kept; only their values are replaced.
fn redact_secrets(mut settings: AppSettings) -> AppSettings {
    for server in &mut settings.mcp_servers {
        match &mut server.transport {
            McpTransport::Stdio { env, .. } => redact_values(env),
            McpTransport::Http { headers, .. } => redact_values(headers),
        }
    }
    settings
}

fn redact_values(values: &mut std::collections::BTreeMap<String, String>) {
    for value in values.values_mut() {
        *value = "<redacted>".into();
    }
}

/// A path shown muted and truncated in a settings row's control, as the file and folder rows do.
fn path_text(path: &str, theme: &Theme) -> impl IntoElement {
    div()
        .max_w(px(220.))
        .min_w(px(0.))
        .overflow_hidden()
        .text_ellipsis()
        .whitespace_nowrap()
        .text_size(px(12.))
        .text_color(theme.muted.to_hsla())
        .child(path.to_string())
}

impl Shell {
    pub(super) fn general_page(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = self.theme.clone();
        let weak = cx.entity().downgrade();
        let projects: Vec<(String, String)> =
            self.state.projects().iter().map(|p| (p.id.clone(), p.name.clone())).collect();
        let s = &self.state.settings;
        let restore = s.restore;
        let default_project = s.default_project.as_ref().and_then(|id| projects.iter().position(|(pid, _)| pid == id));
        let mode = Mode::ALL.iter().position(|m| *m == s.default_mode).unwrap_or(0);
        let (sandbox, ask_network, show_tools) = (s.sandbox, s.ask_network, s.show_tools);
        let max_steps = s.max_steps;
        let (notify, sound) = (s.notify, s.sound);
        let channel = CHANNELS.iter().position(|c| *c == s.update_channel).unwrap_or(0);
        let analytics = s.analytics;
        let steps = self.slide(
            Slide::MaxSteps,
            max_steps as f32,
            SliderRange { min: MAX_STEPS.0 as f32, max: MAX_STEPS.1 as f32, step: 5. },
            "",
            cx,
        );

        let project_names: Vec<&str> = projects.iter().map(|(_, name)| name.as_str()).collect();
        let project_row = if project_names.is_empty() {
            SettingRow::new("Default project", "Where new sessions start when no session is focused.", &theme)
                .control(div().text_size(px(12.)).text_color(theme.muted.to_hsla()).child("No projects yet"))
        } else {
            segmented_row(
                "default-project",
                "Default project",
                "Where new sessions start when no session is focused.",
                &project_names,
                default_project.unwrap_or(usize::MAX),
                &weak,
                &theme,
                |this, i, cx| {
                    this.state.settings.default_project = this.state.projects().get(i).map(|p| p.id.clone());
                    this.save(cx);
                },
                None,
            )
        };

        let folder = self.state.settings.default_folder.clone();
        let folder_text = folder.as_ref().map_or_else(|| "Not set".to_string(), |p| p.display().to_string());
        let folder_note = if folder.is_some() {
            "New sessions run here, if it still exists."
        } else {
            "New sessions start in the folder ArchonUI was started from."
        };
        let folder_row = SettingRow::new("Default folder for new sessions", folder_note, &theme).control(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(path_text(&folder_text, &theme))
                .child(
                    Button::new("folder-choose", "Choose…", &theme)
                        .style(ButtonStyle::Outline)
                        .on_click(listen(cx, |this, cx| this.pick_default_folder(cx))),
                )
                .when(folder.is_some(), |d| {
                    d.child(Button::new("folder-reset", "Reset", &theme).style(ButtonStyle::Outline).on_click(listen(cx, |this, cx| {
                        this.state.settings.default_folder = None;
                        this.save(cx);
                    })))
                }),
        );

        let data_dir = crate::store::dir();
        let data_text = data_dir.display().to_string();
        let data_folder_row = SettingRow::new("Data folder", "Sessions, attachments and settings are kept here.", &theme).control(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(path_text(&data_text, &theme))
                .child(
                    Button::new("data-open", "Open", &theme).style(ButtonStyle::Outline).on_click(listen(cx, move |_this, _cx| {
                        let _ = std::process::Command::new("xdg-open").arg(&data_dir).spawn();
                    })),
                ),
        );

        let startup = section(
            "STARTUP",
            vec![
                toggle_row(
                    "restore",
                    "Restore sessions on launch",
                    "Reopen the tiles you had open when archonui last quit.",
                    restore,
                    &weak,
                    &theme,
                    |this, on, cx| {
                        this.state.settings.restore = on;
                        this.save(cx);
                    },
                    None,
                ),
                project_row,
                folder_row,
            ],
            &theme,
        );

        let agent = section(
            "AGENT",
            vec![
                segmented_row(
                    "default-mode",
                    "Default mode",
                    "Auto edits and runs commands. Plan proposes steps first. Ask only answers. ⇧Tab cycles in the composer.",
                    &["Auto", "Plan", "Ask"],
                    mode,
                    &weak,
                    &theme,
                    |this, i, cx| {
                        if let Some(mode) = Mode::ALL.get(i).copied() {
                            this.state.settings.default_mode = mode;
                            this.save(cx);
                        }
                    },
                    None,
                ),
                toggle_row(
                    "sandbox",
                    "Run commands in sandbox",
                    "Shell commands run in an isolated container with no network by default.",
                    sandbox,
                    &weak,
                    &theme,
                    |this, on, cx| {
                        this.state.settings.sandbox = on;
                        this.save(cx);
                    },
                    None,
                ),
                toggle_row(
                    "ask-network",
                    "Ask before network access",
                    "Pause and confirm when a tool wants to reach the internet.",
                    ask_network,
                    &weak,
                    &theme,
                    |this, on, cx| {
                        this.state.settings.ask_network = on;
                        this.save(cx);
                    },
                    None,
                ),
                toggle_row(
                    "show-tools",
                    "Show tool calls in chat",
                    "Display reads, edits and commands above each reply.",
                    show_tools,
                    &weak,
                    &theme,
                    |this, on, cx| {
                        this.state.settings.show_tools = on;
                        this.save(cx);
                    },
                    None,
                ),
                SettingRow::new(
                    "Max steps per turn",
                    "The agent stops and asks for input after this many tool calls.",
                    &theme,
                )
                .control(steps),
            ],
            &theme,
        );

        let notifications = section(
            "NOTIFICATIONS",
            vec![
                toggle_row(
                    "notify",
                    "Notify when a session finishes",
                    "Only when archonui is not the active window.",
                    notify,
                    &weak,
                    &theme,
                    |this, on, cx| {
                        this.state.settings.notify = on;
                        this.save(cx);
                    },
                    None,
                ),
                toggle_row(
                    "sound",
                    "Play a sound",
                    "",
                    sound,
                    &weak,
                    &theme,
                    |this, on, cx| {
                        this.state.settings.sound = on;
                        this.save(cx);
                    },
                    None,
                ),
            ],
            &theme,
        );

        let updates = section(
            "UPDATES AND PRIVACY",
            vec![
                segmented_row(
                    "update-channel",
                    "Update channel",
                    "Beta and Nightly get features earlier and may be unstable.",
                    &["Stable", "Beta", "Nightly"],
                    channel,
                    &weak,
                    &theme,
                    |this, i, cx| {
                        if let Some(channel) = CHANNELS.get(i).copied() {
                            this.state.settings.update_channel = channel;
                            this.save(cx);
                        }
                    },
                    Some("Update channels are not available in this build."),
                ),
                toggle_row(
                    "analytics",
                    "Share anonymous usage data",
                    "Crash reports and feature counts. Never prompts or code.",
                    analytics,
                    &weak,
                    &theme,
                    |this, on, cx| {
                        this.state.settings.analytics = on;
                        this.save(cx);
                    },
                    Some("Anonymous usage data is not sent in this build."),
                ),
            ],
            &theme,
        );

        let data = section(
            "DATA",
            vec![
                button_row(
                    "export-settings",
                    "Export settings",
                    "Themes, providers, tools and keybindings as JSON. API keys and MCP credentials are left out.",
                    "Export",
                    ButtonStyle::Outline,
                    &weak,
                    &theme,
                    |this, cx| this.export_settings(cx),
                ),
                button_row(
                    "clear-history",
                    "Clear session history",
                    "Removes every session that is not currently tiled.",
                    "Clear history",
                    ButtonStyle::Danger,
                    &weak,
                    &theme,
                    |this, cx| {
                        let cleared = this.state.clear_history().len();
                        this.save(cx);
                        this.toast(format!("Cleared {cleared} sessions"), cx);
                    },
                ),
                data_folder_row,
            ],
            &theme,
        );

        vec![startup, agent, notifications, updates, data]
    }

    /// Opens a folder picker and stores it as the folder new sessions run in.
    fn pick_default_folder(&mut self, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions { files: false, directories: true, multiple: false, prompt: Some("Choose".into()) });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else { return };
            let Some(path) = paths.into_iter().next() else { return };
            let _ = this.update(cx, |this, cx| {
                this.state.settings.default_folder = Some(path);
                this.save(cx);
            });
        })
        .detach();
    }

    /// Writes the settings as JSON to a path the user picks. Nothing is written if the prompt is cancelled.
    fn export_settings(&mut self, cx: &mut Context<Self>) {
        let dir = self.default_folder();
        let settings = redact_secrets(self.state.settings.clone());
        let picked = cx.prompt_for_new_path(&dir, Some("archonui-settings.json"));
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(path))) = picked.await else { return };
            let written = cx
                .background_executor()
                .spawn(async move {
                    let json = serde_json::to_string_pretty(&settings).map_err(|e| e.to_string())?;
                    std::fs::write(&path, json).map_err(|e| e.to_string())?;
                    Ok::<_, String>(path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())
                })
                .await;
            let _ = this.update(cx, |this, cx| match written {
                Ok(name) => this.toast(format!("Exported {name}"), cx),
                Err(e) => this.toast(format!("Export failed: {e}"), cx),
            });
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::settings::{McpServer, McpTransport};
    use std::collections::BTreeMap;

    #[test]
    fn an_export_blanks_mcp_credentials() {
        let mut settings = AppSettings::default();
        settings.mcp_servers.push(McpServer {
            id: "docs".into(),
            name: "docs".into(),
            transport: McpTransport::Http {
                url: "https://d.test/mcp".into(),
                headers: BTreeMap::from([("Authorization".to_string(), "Bearer secret".to_string())]),
            },
            enabled: true,
        });
        settings.mcp_servers.push(McpServer {
            id: "gh".into(),
            name: "gh".into(),
            transport: McpTransport::Stdio {
                command: "gh".into(),
                args: vec![],
                env: BTreeMap::from([("GITHUB_TOKEN".to_string(), "ghp_secret".to_string())]),
            },
            enabled: true,
        });

        let exported = redact_secrets(settings);
        let json = serde_json::to_string(&exported).unwrap();
        assert!(!json.contains("Bearer secret") && !json.contains("ghp_secret"), "{json}");
        match &exported.mcp_servers[0].transport {
            McpTransport::Http { url, headers } => {
                assert_eq!(url, "https://d.test/mcp", "the address stays");
                assert_eq!(headers["Authorization"], "<redacted>");
            }
            other => panic!("unexpected transport: {other:?}"),
        }
        match &exported.mcp_servers[1].transport {
            McpTransport::Stdio { env, .. } => assert_eq!(env["GITHUB_TOKEN"], "<redacted>"),
            other => panic!("unexpected transport: {other:?}"),
        }
    }
}
