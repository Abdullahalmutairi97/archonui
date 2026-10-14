//! The Tools page: MCP servers, built-in tool permissions, and the terminal and preview settings. Spec: settings.md §7.

use super::{
    controls::{caption, edit, heading, listen, section, segmented_row, toggle_row},
    Slide,
};
use crate::{
    app::settings::{McpServer, McpTransport, Permission, Permissions, TERMINAL_SIZE},
    engine::mcp::McpStatus,
    store::{IdeKind, SearchEngine},
    ui::{
        shell::Shell,
        theme::Theme,
        widgets::{
            button::{Button, ButtonStyle},
            section::SettingRow,
            slider::SliderRange,
            toggle::Toggle,
        },
    },
};
use gpui::{div, prelude::*, AnyElement, Context, PathPromptOptions, SharedString};
use crate::ui::scale::px;

/// The built-in tool rows in order, with the label the page shows for each.
const BUILT_IN: [&str; 7] = ["Read files", "Edit files", "Search code", "Run shell commands", "Control the browser panel", "Web search", "Fetch URLs"];

/// Permission options in segmented order.
const PERMISSION_LABELS: [&str; 3] = ["Allow", "Ask", "Off"];

/// Shown under the permission rows: only a harness that can pause for an answer can prompt.
const ASK_NOTE: &str = "Ask: Claude Code in Auto mode asks in ArchonUI before each use. Other harnesses have no per-tool prompt, so they run Ask tools as Allow.";

fn permission_index(p: Permission) -> usize {
    match p {
        Permission::Allow => 0,
        Permission::Ask => 1,
        Permission::Off => 2,
    }
}

fn permission_at(i: usize) -> Permission {
    match i {
        0 => Permission::Allow,
        1 => Permission::Ask,
        _ => Permission::Off,
    }
}

/// The permission slot for built-in row `row`, in `BUILT_IN` order.
fn slot(p: &mut Permissions, row: usize) -> &mut Permission {
    match row {
        0 => &mut p.read,
        1 => &mut p.edit,
        2 => &mut p.grep,
        3 => &mut p.run,
        4 => &mut p.browser,
        5 => &mut p.web,
        _ => &mut p.fetch,
    }
}

fn permission_of(p: &Permissions, row: usize) -> Permission {
    match row {
        0 => p.read,
        1 => p.edit,
        2 => p.grep,
        3 => p.run,
        4 => p.browser,
        5 => p.web,
        _ => p.fetch,
    }
}

/// The two-letter sigil for an MCP server, from its name.
fn sigil(name: &str) -> String {
    name.chars().take(2).collect::<String>()
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

/// The line under an MCP server's name: its command or URL.
fn transport_detail(server: &McpServer) -> String {
    match &server.transport {
        McpTransport::Stdio { command, args, .. } => std::iter::once(command.as_str()).chain(args.iter().map(String::as_str)).collect::<Vec<_>>().join(" "),
        McpTransport::Http { url, .. } => url.clone(),
    }
}

fn transport_label(server: &McpServer) -> &'static str {
    match server.transport {
        McpTransport::Stdio { .. } => "stdio",
        McpTransport::Http { .. } => "http",
    }
}

impl Shell {
    pub(super) fn tools_page(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = self.theme.clone();
        let weak = cx.entity().downgrade();

        let add = Button::new("add-mcp", "+ Add server", &theme).style(ButtonStyle::Outline).on_click(listen(cx, |this, cx| {
            this.toast("Adding a server from here isn't available yet. Add it to settings.json.", cx);
        }));
        let mut list = div()
            .flex()
            .flex_col()
            .rounded(px(10.))
            .border_1()
            .border_color(theme.line2.to_hsla())
            .overflow_hidden();
        if self.state.settings.mcp_servers.is_empty() {
            list = list.child(
                div()
                    .px(px(16.))
                    .py(px(14.))
                    .text_size(px(12.5))
                    .text_color(theme.muted.to_hsla())
                    .child("No MCP servers yet."),
            );
        }
        for (i, server) in self.state.settings.mcp_servers.clone().into_iter().enumerate() {
            list = list.child(self.mcp_row(i, &server, &weak, cx));
        }
        let mcp = div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(heading("MCP SERVERS", None, &theme))
                    .child(add),
            )
            .child(list)
            .into_any_element();

        let rows: Vec<SettingRow> = BUILT_IN
            .iter()
            .enumerate()
            .map(|(row, name)| {
                let current = permission_index(permission_of(&self.state.settings.permissions, row));
                segmented_row(
                    format!("permission-{row}"),
                    name,
                    "",
                    &PERMISSION_LABELS,
                    current,
                    &weak,
                    &theme,
                    move |this, i, cx| {
                        *slot(&mut this.state.settings.permissions, row) = permission_at(i);
                        this.save(cx);
                    },
                    None,
                )
            })
            .collect();

        let terminal_text = self.slide(
            Slide::Terminal,
            self.state.settings.terminal_font_size,
            SliderRange { min: TERMINAL_SIZE.0, max: TERMINAL_SIZE.1, step: 1. },
            "px",
            cx,
        );
        let preview = self.state.settings.auto_open_preview;
        let tmux = self.state.settings.use_tmux;
        let extras = vec![
            toggle_row(
                "auto-preview",
                "Open previews automatically",
                "Show a preview when the agent writes a file that has one.",
                preview,
                &weak,
                &theme,
                |this, on, cx| {
                    this.state.settings.auto_open_preview = on;
                    this.save(cx);
                },
                None,
            ),
            toggle_row(
                "use-tmux",
                "Run terminals in tmux",
                "Terminals keep running after ArchonUI closes.",
                tmux,
                &weak,
                &theme,
                |this, on, cx| {
                    this.state.settings.use_tmux = on;
                    this.save(cx);
                },
                None,
            ),
            SettingRow::new("Terminal text size", "", &theme).control(terminal_text),
        ];

        vec![
            mcp,
            section("BUILT-IN TOOLS", rows, &theme),
            caption(ASK_NOTE, &theme),
            section("TERMINAL AND PREVIEW", extras, &theme),
            self.workspace_tools(&weak, &theme, cx),
        ]
    }

    /// The Workspace Tools section: the IDE engine, the code-server program and the browser's search engine.
    fn workspace_tools(&self, weak: &gpui::WeakEntity<Shell>, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let ide_index = if self.state.settings.ide == IdeKind::VsCode { 1 } else { 0 };
        let ide = segmented_row(
            "ide-engine",
            "IDE engine",
            "The editor the IDE tab shows.",
            &["Built-in", "VS Code"],
            ide_index,
            weak,
            theme,
            |this, i, cx| {
                this.state.settings.ide = if i == 1 { IdeKind::VsCode } else { IdeKind::Builtin };
                this.save(cx);
            },
            None,
        );

        let detected = crate::ide::find_code_server(self.state.settings.code_server_path.as_deref())
            .map_or_else(|| "Not found".to_string(), |p| p.display().to_string());
        let configured = self.state.settings.code_server_path.is_some();
        let code_server = SettingRow::new("code-server path", "The program that runs VS Code. Found automatically when unset.", theme).control(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(path_text(&detected, theme))
                .child(
                    Button::new("code-server-choose", "Choose…", theme)
                        .style(ButtonStyle::Outline)
                        .on_click(listen(cx, |this, cx| this.pick_code_server(cx))),
                )
                .when(configured, |d| {
                    d.child(Button::new("code-server-reset", "Reset", theme).style(ButtonStyle::Outline).on_click(listen(cx, |this, cx| {
                        this.state.settings.code_server_path = None;
                        this.save(cx);
                    })))
                }),
        );

        let names: Vec<&str> = SearchEngine::ALL.iter().map(|e| e.name()).collect();
        let engine_index = SearchEngine::ALL.iter().position(|e| *e == self.state.settings.search_engine).unwrap_or(0);
        let engine = segmented_row(
            "search-engine",
            "Search engine",
            "Searches from the address bar go here.",
            &names,
            engine_index,
            weak,
            theme,
            |this, i, cx| {
                if let Some(engine) = SearchEngine::ALL.get(i).copied() {
                    this.state.settings.search_engine = engine;
                    crate::preview::set_search_engine(engine.url_prefix());
                    this.save(cx);
                }
            },
            None,
        );

        section("WORKSPACE TOOLS", vec![ide, code_server, engine], theme)
    }

    /// Opens a file picker for the code-server program and stores the pick.
    fn pick_code_server(&mut self, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions { files: true, directories: false, multiple: false, prompt: None });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else { return };
            let Some(path) = paths.into_iter().next() else { return };
            let _ = this.update(cx, |this, cx| {
                this.state.settings.code_server_path = Some(path);
                this.save(cx);
            });
        })
        .detach();
    }

    /// One MCP server: sigil, name and command, transport, tool count, live status, and its toggle.
    fn mcp_row(&mut self, index: usize, server: &McpServer, weak: &gpui::WeakEntity<Shell>, cx: &mut Context<Self>) -> AnyElement {
        let theme = self.theme.clone();
        let status = self.settings.mcp.get(&server.id).cloned();
        let (text, colour, tools, failed) = match (&status, server.enabled) {
            (_, false) => ("Off".to_string(), theme.faint, String::new(), false),
            (None, true) => ("Not checked".to_string(), theme.faint, String::new(), false),
            (Some(None), true) => ("Checking…".to_string(), theme.muted, String::new(), false),
            (Some(Some(McpStatus::Connected { tools })), true) => {
                ("Connected".to_string(), theme.alt, format!("{tools} tools"), false)
            }
            (Some(Some(McpStatus::Failed(error))), true) => (error.clone(), theme.acc, String::new(), true),
            (Some(Some(McpStatus::Off)), true) => ("Off".to_string(), theme.faint, String::new(), false),
        };
        let id = server.id.clone();
        let toggle_id = server.id.clone();
        let toggle = Toggle::new(SharedString::from(format!("mcp-{}", server.id)), server.enabled, &theme).on_change(edit(
            weak,
            move |this: &mut Shell, on: bool, cx| {
                if let Some(server) = this.state.settings.mcp_servers.iter_mut().find(|s| s.id == toggle_id) {
                    server.enabled = on;
                }
                this.save(cx);
                if on {
                    this.check_mcp(Some(toggle_id.clone()), cx);
                }
            },
        ));
        let mut row = div()
            .flex()
            .items_center()
            .gap(px(14.))
            .px(px(16.))
            .py(px(13.))
            .border_b_1()
            .border_color(theme.line.to_hsla())
            .child(
                div()
                    .w(px(34.))
                    .h(px(34.))
                    .rounded(px(8.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(theme.line2.to_hsla())
                    .text_size(px(12.5))
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_color(if server.enabled { theme.acc2.to_hsla() } else { theme.muted.to_hsla() })
                    .child(sigil(&server.name)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .child(div().text_size(px(14.)).child(server.name.clone()))
                    .child(
                        div()
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .text_size(px(12.))
                            .text_color(theme.muted.to_hsla())
                            .child(transport_detail(server)),
                    ),
            )
            .child(
                div()
                    .text_size(px(11.))
                    .px(px(8.))
                    .py(px(3.))
                    .rounded(px(5.))
                    .bg(theme.line.to_hsla())
                    .text_color(theme.alt.to_hsla())
                    .child(transport_label(server)),
            )
            .child(div().w(px(64.)).text_size(px(12.)).text_color(theme.muted.to_hsla()).text_right().child(tools))
            .child(
                div()
                    .w(px(150.))
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap(px(7.))
                    .text_size(px(12.))
                    .text_color(colour.to_hsla())
                    .child(div().flex_1().min_w(px(0.)).overflow_hidden().text_ellipsis().whitespace_nowrap().text_right().child(text))
                    .child(div().w(px(7.)).h(px(7.)).rounded_full().flex_none().bg(colour.to_hsla())),
            );
        if failed {
            row = row.child(
                Button::new(SharedString::from(format!("reconnect-{index}")), "Reconnect", &theme)
                    .style(ButtonStyle::Filled)
                    .on_click(listen(cx, move |this, cx| this.check_mcp(Some(id.clone()), cx))),
            );
        }
        row.child(toggle).into_any_element()
    }
}
