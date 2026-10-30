//! The General page: startup, projects in the sidebar, agent behaviour, notifications and data.

use super::{
    controls::{button_row, empty_box, icon, listen, path_label, section, segmented_row},
    flag_row, Slide,
};
use crate::{
    app::settings::{AppSettings, McpTransport, MAX_STEPS},
    engine::Mode,
    ui::{
        composer::icons::stroke_icon,
        shell::Shell,
        widgets::{
            button::{Button, ButtonStyle},
            section::SettingRow,
            slider::SliderRange,
        },
    },
};
use gpui::{div, prelude::*, AnyElement, Context, PathPromptOptions};
use crate::ui::scale::px;

/// `settings` with every MCP secret blanked, so an export can be shared. Key and
/// header names are kept; only their values are replaced.
fn redact_secrets(mut settings: AppSettings) -> AppSettings {
    for server in &mut settings.mcp_servers {
        match &mut server.transport {
            McpTransport::Stdio { env, args, .. } => {
                redact_values(env);
                redact_args(args);
            }
            McpTransport::Http { headers, url } => {
                redact_values(headers);
                *url = redact_url(url);
            }
        }
    }
    settings
}

/// `url` without credentials (`user:pass@`), query string or fragment, which often carry API keys.
fn redact_url(url: &str) -> String {
    let end = url.find(['?', '#']).unwrap_or(url.len());
    let base = &url[..end];
    match base.split_once("://") {
        Some((scheme, rest)) => {
            let (authority, path) = rest.split_at(rest.find('/').unwrap_or(rest.len()));
            let host = authority.rsplit_once('@').map_or(authority, |(_, host)| host);
            format!("{scheme}://{host}{path}")
        }
        None => base.to_string(),
    }
}

/// Blanks the value of command-line arguments that look like secrets: `--api-key=abc`, `--token abc`
/// and `--header "Authorization: Bearer x"`.
fn redact_args(args: &mut [String]) {
    let secret = |a: &str| {
        let a = a.to_ascii_lowercase();
        ["token", "key", "secret", "password", "passwd", "auth", "bearer"].iter().any(|w| a.contains(w))
    };
    // `--header "Authorization: Bearer x"` and curl's `-H`, whose values can carry any credential.
    let header = |flag: &str| flag == "-H" || flag.eq_ignore_ascii_case("--header") || flag.eq_ignore_ascii_case("--headers");
    let leaks = |a: &str| {
        let a = a.to_ascii_lowercase();
        a.contains("authorization:") || a.contains("bearer ")
    };
    // An auth scheme given as its own argument, so the token follows it: `--authorization Bearer x`.
    let scheme = |a: &str| ["bearer", "basic", "token"].iter().any(|w| a.eq_ignore_ascii_case(w));
    let mut hide_next = false;
    for arg in args.iter_mut() {
        if std::mem::take(&mut hide_next) {
            hide_next = scheme(arg);
            *arg = "<redacted>".into();
        } else if arg.starts_with('-') {
            match arg.split_once('=') {
                // curl's attached form, `-HAuthorization: Bearer x`.
                _ if arg.len() > 2 && arg.starts_with("-H") => *arg = "-H<redacted>".into(),
                Some((flag, _)) if secret(flag) || header(flag) => *arg = format!("{flag}=<redacted>"),
                None if secret(arg) || header(arg) => hide_next = true,
                _ if leaks(arg) => *arg = "<redacted>".into(),
                _ => {}
            }
        } else if leaks(arg) {
            *arg = "<redacted>".into();
        }
    }
}

fn redact_values(values: &mut std::collections::BTreeMap<String, String>) {
    for value in values.values_mut() {
        *value = "<redacted>".into();
    }
}

impl Shell {
    pub(super) fn general_page(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = self.theme.clone();
        let weak = cx.entity().downgrade();
        let s = &self.state.settings;
        let restore = s.restore;
        let mode = Mode::ALL.iter().position(|m| *m == s.default_mode).unwrap_or(0);
        let (sandbox, ask_network, show_tools) = (s.sandbox, s.ask_network, s.show_tools);
        let max_steps = s.max_steps;
        let (notify, sound) = (s.notify, s.sound);
        let steps = self.slide(
            Slide::MaxSteps,
            max_steps as f32,
            SliderRange { min: MAX_STEPS.0 as f32, max: MAX_STEPS.1 as f32, step: 5. },
            "",
            cx,
        );

        let folder = self.state.settings.default_folder.clone();
        let folder_text = folder.as_ref().map_or_else(|| "Automatic".to_string(), |p| tilde(p));
        let folder_row = SettingRow::new("Default folder", "", &theme).control(
            div()
                .flex()
                .items_center()
                .gap(px(10.))
                .child(path_label(&folder_text, folder.is_some(), &theme))
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
        let data_text = tilde(&data_dir);
        let data_folder_row = SettingRow::new("Data folder", "", &theme).control(
            div()
                .flex()
                .items_center()
                .gap(px(10.))
                .child(path_label(&data_text, true, &theme))
                .child(
                    Button::new("data-open", "Open", &theme).style(ButtonStyle::Outline).on_click(listen(cx, move |_this, _cx| {
                        // Reaped on a thread, so the launcher doesn't linger as a zombie.
                        if let Ok(mut child) = std::process::Command::new("xdg-open").arg(&data_dir).spawn() {
                            std::thread::spawn(move || {
                                let _ = child.wait();
                            });
                        }
                    })),
                ),
        );

        let startup = section(
            "Startup",
            vec![
                flag_row("restore", "Restore sessions on launch", "", restore, &weak, &theme, |s, on| s.restore = on),
                folder_row,
            ],
            &theme,
        );

        let agent = section(
            "Agent",
            vec![
                segmented_row(
                    "default-mode",
                    "Default mode",
                    "",
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
                flag_row("sandbox", "Run commands in sandbox", "No network by default", sandbox, &weak, &theme, |s, on| s.sandbox = on),
                flag_row("ask-network", "Ask before network access", "", ask_network, &weak, &theme, |s, on| s.ask_network = on),
                flag_row("show-tools", "Show tool calls in chat", "", show_tools, &weak, &theme, |s, on| s.show_tools = on),
                SettingRow::new("Max steps per turn", "", &theme).control(steps),
            ],
            &theme,
        );

        let notifications = section(
            "Notifications",
            vec![
                flag_row("notify", "Notify when a session finishes", "Only when in the background", notify, &weak, &theme, |s, on| s.notify = on),
                flag_row("sound", "Play a sound", "", sound, &weak, &theme, |s, on| s.sound = on),
            ],
            &theme,
        );

        let update_check = self.state.settings.update_check;
        let mut update_rows = self.update_rows(cx);
        update_rows.push(flag_row(
            "update-check",
            "Check for updates daily",
            "Looks at GitHub once a day when ArchonUI starts and shows a dot. It never installs without a click.",
            update_check,
            &weak,
            &theme,
            |s, on| s.update_check = on,
        ));
        let updates = section("Updates", update_rows, &theme);

        let data = section(
            "Data",
            vec![
                button_row(
                    "export-settings",
                    "Export settings",
                    "Secrets left out",
                    "Export",
                    ButtonStyle::Outline,
                    &weak,
                    &theme,
                    |this, cx| this.export_settings(cx),
                ),
                button_row(
                    "clear-history",
                    "Clear session history",
                    "Keeps open tiles",
                    if self.settings.confirm_clear { "Click again to confirm" } else { "Clear history" },
                    ButtonStyle::Danger,
                    &weak,
                    &theme,
                    |this, cx| this.clear_history_clicked(cx),
                ),
                data_folder_row,
            ],
            &theme,
        );

        let pins = self.project_pins(cx);
        vec![startup, pins, agent, notifications, updates, data]
    }

    /// The first click on Clear history asks for a second within three seconds; the second clears.
    fn clear_history_clicked(&mut self, cx: &mut Context<Self>) {
        if !self.settings.confirm_clear {
            self.settings.confirm_clear = true;
            cx.notify();
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(std::time::Duration::from_secs(3)).await;
                let _ = this.update(cx, |this, cx| {
                    this.settings.confirm_clear = false;
                    cx.notify();
                });
            })
            .detach();
            return;
        }
        self.settings.confirm_clear = false;
        let cleared = self.state.clear_history();
        self.share_deleted_all(&cleared);
        let cleared = cleared.len();
        self.save(cx);
        self.toast(format!("Cleared {cleared} sessions"), cx);
    }

    /// Every project with a pin: pinned ones show in the sidebar's Projects list.
    fn project_pins(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = &self.theme;
        let projects: Vec<(String, String, String)> =
            self.state.projects().iter().map(|p| (p.id.clone(), p.name.clone(), tilde(&p.path))).collect();
        let head = crate::ui::widgets::section::section_head("Projects", None, Vec::new(), theme);
        if projects.is_empty() {
            return div().flex().flex_col().child(head).child(empty_box(Some(icon::FOLDER), "No projects yet", theme)).into_any_element();
        }
        let hidden = &self.state.settings.hidden_projects;
        let mut rows = Vec::new();
        for (id, name, path) in projects {
            let pinned = !hidden.contains(&id);
            let colour = if pinned { theme.acc2 } else { theme.muted };
            let line = theme.line.to_hsla();
            let pin = div()
                .id(gpui::SharedString::from(format!("pin-{id}")))
                .flex_none()
                .size(px(30.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(7.))
                .cursor_pointer()
                .hover(move |s| s.bg(line))
                .on_click(cx.listener(move |this, _, _, cx| {
                    let hidden = &mut this.state.settings.hidden_projects;
                    if let Some(i) = hidden.iter().position(|h| *h == id) {
                        hidden.remove(i);
                    } else {
                        hidden.push(id.clone());
                    }
                    this.save(cx);
                }))
                .child(stroke_icon(icon::PIN, 16., 1.4, colour.to_hsla()));
            rows.push(
                div()
                    .flex()
                    .items_center()
                    .gap(px(14.))
                    .px(px(18.))
                    .py(px(12.))
                    .bg(theme.tile.to_hsla())
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .when(!pinned, |d| d.opacity(0.5))
                            .child(name)
                            .child(
                                div()
                                    .font_family(theme.code_font.clone())
                                    .text_size(px(12.))
                                    .text_color(theme.muted.to_hsla())
                                    .whitespace_nowrap()
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .child(path),
                            ),
                    )
                    .when(!pinned, |d| {
                        d.child(div().text_size(px(12.)).text_color(theme.muted.to_hsla()).child("Hidden from the sidebar"))
                    })
                    .child(pin)
                    .into_any_element(),
            );
        }
        div().flex().flex_col().child(head).child(crate::ui::widgets::section::card(rows, theme)).into_any_element()
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

/// `path` with the home folder shown as `~`.
pub(crate) fn tilde(path: &std::path::Path) -> String {
    match std::env::var_os("HOME").map(std::path::PathBuf::from) {
        Some(home) if !home.as_os_str().is_empty() => match path.strip_prefix(&home) {
            Ok(rest) if rest.as_os_str().is_empty() => "~".to_string(),
            Ok(rest) => format!("~/{}", rest.display()),
            Err(_) => path.display().to_string(),
        },
        _ => path.display().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::settings::{McpServer, McpTransport};
    use std::collections::BTreeMap;

    #[test]
    fn urls_lose_credentials_query_and_fragment() {
        assert_eq!(redact_url("https://u:p@h.test:8080/mcp?key=abc#x"), "https://h.test:8080/mcp");
        assert_eq!(redact_url("https://h.test?token=1"), "https://h.test");
        assert_eq!(redact_url("https://h.test/a/b"), "https://h.test/a/b");
    }

    #[test]
    fn header_style_arguments_lose_their_credentials() {
        let mut args = [
            "--header",
            "Authorization: Bearer sk-123",
            "-H",
            "Authorization: Bearer x",
            "--header=Authorization: Bearer x",
            "Authorization: Bearer y",
            "--port=1",
        ]
        .map(String::from)
        .to_vec();
        redact_args(&mut args);
        assert_eq!(
            args,
            ["--header", "<redacted>", "-H", "<redacted>", "--header=<redacted>", "<redacted>", "--port=1"]
        );

        let mut args = ["-HAuthorization: Bearer z", "--authorization", "Bearer", "sk-1", "--no-headers", "--port", "1"]
            .map(String::from)
            .to_vec();
        redact_args(&mut args);
        assert_eq!(args, ["-H<redacted>", "--authorization", "<redacted>", "<redacted>", "--no-headers", "--port", "1"]);
    }

    #[test]
    fn an_export_blanks_mcp_credentials() {
        let mut settings = AppSettings::default();
        settings.mcp_servers.push(McpServer {
            id: "docs".into(),
            name: "docs".into(),
            transport: McpTransport::Http {
                url: "https://d.test/mcp?key=abc".into(),
                headers: BTreeMap::from([("Authorization".to_string(), "Bearer secret".to_string())]),
            },
            enabled: true,
        });
        settings.mcp_servers.push(McpServer {
            id: "gh".into(),
            name: "gh".into(),
            transport: McpTransport::Stdio {
                command: "gh".into(),
                args: ["--port=1", "--api-key=abc", "--token", "xyz", "serve"].map(String::from).to_vec(),
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
            McpTransport::Stdio { env, args, .. } => {
                assert_eq!(env["GITHUB_TOKEN"], "<redacted>");
                assert_eq!(args, &["--port=1", "--api-key=<redacted>", "--token", "<redacted>", "serve"]);
            }
            other => panic!("unexpected transport: {other:?}"),
        }
    }
}
