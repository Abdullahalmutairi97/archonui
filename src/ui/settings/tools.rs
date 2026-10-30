//! The Tools page: MCP servers and built-in tool permissions, each marked with the installed agents
//! that apply it, and the terminal and preview settings. Spec: settings.md §7.

use super::{
    controls::{edit, empty_box, icon, link, list_row, listen, path_label, search_box, section, segmented_row, status as status_line},
    flag_row, Slide,
};
use crate::{
    app::settings::{Permission, Permissions, TERMINAL_SIZE},
    engine::{mcp::McpStatus, permission_rows, ToolSupport},
    store::{IdeKind, SearchEngine},
    ui::{
        composer::icons::stroke_icon,
        shell::Shell,
        theme::{Rgba, Theme, AMBER, GREEN},
        widgets::{
            agents::{agent_icons, short_name, AgentMark},
            button::{Button, ButtonStyle},
            section::{card, section_head, SettingRow},
            segmented::Segmented,
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

/// What each built-in row controls, for the notes on agents that ignore it, in `BUILT_IN` order.
const TOOL_NOUNS: [&str; 7] = ["file reading", "file editing", "code search", "shell commands", "the browser panel", "web search", "URL fetching"];

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

/// Each installed agent's id with what its harness applies.
type Support<'a> = [(&'a str, ToolSupport)];

/// How each agent treats an MCP server: lit when it receives the server, struck through when the server
/// is on but the agent can't take MCP servers from ArchonUI.
fn mcp_marks<'a>(agents: &Support<'a>, enabled: bool) -> Vec<AgentMark<'a>> {
    agents
        .iter()
        .map(|&(agent, support)| AgentMark {
            agent,
            lit: enabled && support.mcp,
            struck: enabled && !support.mcp,
            note: (!support.mcp).then(|| format!("{} doesn't take MCP servers from ArchonUI", short_name(agent))),
        })
        .collect()
}

/// How each agent treats built-in row `row` at `level`: lit when it applies the setting, struck through
/// when the user chose Ask or Off and the agent runs the tool as Allow anyway.
fn permission_marks<'a>(agents: &Support<'a>, row: usize, level: Permission) -> Vec<AgentMark<'a>> {
    agents
        .iter()
        .map(|&(agent, support)| {
            let name = short_name(agent);
            let noun = TOOL_NOUNS[row];
            let gated = support.permissions[row];
            let (lit, note) = match level {
                Permission::Ask if !support.ask => (false, Some(format!("{name} can't ask per use, so it runs {noun} as Allow"))),
                _ if !gated => (false, Some(format!("{name} can't switch off {noun}, so it runs as Allow"))),
                _ => (true, None),
            };
            AgentMark { agent, lit, struck: !lit && level != Permission::Allow, note }
        })
        .collect()
}

/// The row description naming the agents that ignore a row's Ask or Off, or empty when none do.
fn ignored_by(marks: &[AgentMark]) -> String {
    let names: Vec<&str> = marks.iter().filter(|m| m.struck).map(|m| short_name(m.agent)).collect();
    if names.is_empty() {
        String::new()
    } else {
        format!("Ignored by {}", names.join(", "))
    }
}

/// How many enabled MCP servers each agent receives, as the Skills page counts skills.
fn mcp_summary(agents: &Support, enabled: usize) -> String {
    agents
        .iter()
        .map(|&(agent, support)| format!("{} {}", short_name(agent), if support.mcp { enabled } else { 0 }))
        .collect::<Vec<_>>()
        .join(" · ")
}

/// How many built-in rows each agent applies as they are set now.
fn permission_summary(agents: &Support, permissions: Permissions) -> String {
    let levels = permission_rows(permissions);
    agents
        .iter()
        .map(|&(agent, support)| {
            let applied = (0..BUILT_IN.len()).filter(|&row| permission_marks(&[(agent, support)], row, levels[row])[0].lit).count();
            format!("{} {applied}/{}", short_name(agent), BUILT_IN.len())
        })
        .collect::<Vec<_>>()
        .join(" · ")
}

impl Shell {
    pub(super) fn tools_page(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = self.theme.clone();
        let weak = cx.entity().downgrade();
        let support = self.tool_support();

        let mut built_in = Vec::new();
        for (row, name) in BUILT_IN.iter().enumerate() {
            let level = permission_of(&self.state.settings.permissions, row);
            let marks = permission_marks(&support, row, level);
            let note = ignored_by(&marks);
            let seg = Segmented::new(format!("permission-{row}"), PERMISSION_LABELS, permission_index(level), &theme).on_select(edit(
                &weak,
                move |this: &mut Shell, i: usize, cx| {
                    *slot(&mut this.state.settings.permissions, row) = permission_at(i);
                    this.save(cx);
                },
            ));
            built_in.push(
                list_row(
                    *name,
                    (!note.is_empty()).then(|| note.into()),
                    vec![agent_icons(format!("permission-agents-{row}"), &marks, &theme), seg.into_any_element()],
                    &theme,
                )
                .py(px(12.))
                .into_any_element(),
            );
        }
        let built_in = div()
            .flex()
            .flex_col()
            .child(section_head("Built-in tools", None, Vec::new(), &theme))
            .child(card(built_in, &theme))
            .into_any_element();

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
            flag_row("auto-preview", "Open previews automatically", "", preview, &weak, &theme, |s, on| s.auto_open_preview = on),
            flag_row("use-tmux", "Run terminals in tmux", "Keep running after closing", tmux, &weak, &theme, |s, on| s.use_tmux = on),
            SettingRow::new("Terminal text size", "", &theme).control(terminal_text),
        ];

        vec![
            self.mcp_section(&support, &weak, cx),
            built_in,
            section("Terminal and preview", extras, &theme),
            self.workspace_tools(&weak, &theme, cx),
        ]
    }

    /// MCP servers: searchable by server and tool name, each opening onto its tools with a permission
    /// for every one.
    fn mcp_section(&mut self, support: &Support, weak: &gpui::WeakEntity<Shell>, cx: &mut Context<Self>) -> AnyElement {
        let theme = self.theme.clone();
        let query = self.query("mcp", cx);
        let servers = self.state.settings.mcp_servers.clone();
        let reaches: Vec<&str> = support.iter().filter(|(_, s)| s.mcp).map(|(a, _)| short_name(a)).collect();
        let reaches = if reaches.is_empty() { "No agent takes MCP servers".to_string() } else { reaches.join(", ") };
        let mut rows = Vec::new();
        for (i, server) in servers.iter().enumerate() {
            let status = self.settings.mcp.get(&server.id).cloned();
            let all_tools: Vec<String> = match &status {
                Some(Some(McpStatus::Connected { tools })) => tools.clone(),
                _ => Vec::new(),
            };
            let name_hit = query.is_empty() || server.name.to_lowercase().contains(&query);
            let tools: Vec<String> = if name_hit { all_tools.clone() } else { all_tools.iter().filter(|t| t.to_lowercase().contains(&query)).cloned().collect() };
            if !name_hit && tools.is_empty() {
                continue;
            }
            let open = self.settings.mcp_open.contains(&server.id) || !name_hit;
            let (dot, word, busy) = mcp_mark(&status, server.enabled, &theme);
            let note = if all_tools.is_empty() { reaches.clone() } else { format!("{reaches} · {} tools", all_tools.len()) };
            let (open_id, check_id, toggle_id) = (server.id.clone(), server.id.clone(), server.id.clone());
            // The chevron points down when open and to the side when closed, as if turned a quarter.
            let chevron = stroke_icon(if open { icon::CHEV } else { "M6 4l4 4-4 4" }, 14., 1.4, theme.muted.to_hsla());
            let head = div()
                .id(("mcp-head", i))
                .flex_1()
                .flex_basis(px(180.))
                .min_w(px(0.))
                .flex()
                .items_center()
                .gap(px(10.))
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| {
                    if !this.settings.mcp_open.remove(&open_id) {
                        this.settings.mcp_open.insert(open_id.clone());
                    }
                    cx.notify();
                }))
                .child(div().flex_none().child(chevron))
                .child(
                    div()
                        .min_w(px(0.))
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .child(server.name.clone())
                        .child(div().text_size(px(12.)).text_color(theme.muted.to_hsla()).child(note)),
                );
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
            let mut row = div().flex().flex_col().bg(theme.tile.to_hsla()).child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_x(px(16.))
                    .gap_y(px(8.))
                    .px(px(18.))
                    .py(px(11.))
                    .child(head)
                    .child(
                        div()
                            .ml_auto()
                            .flex()
                            .items_center()
                            .gap(px(14.))
                            .child(status_line(dot, word, &theme))
                            .when(server.enabled && !busy, |d| {
                                d.child(link(("mcp-reconnect", i), "Reconnect", theme.acc2, listen(cx, move |this, cx| this.check_mcp(Some(check_id.clone()), cx))))
                            })
                            .child(toggle),
                    ),
            );
            if open {
                let key = format!("mcp:{}", server.id);
                let limit = self.limit(&key, 6);
                let left = tools.len().saturating_sub(limit);
                let mut list = div().flex().flex_col().pt(px(2.)).pr(px(18.)).pb(px(10.)).pl(px(42.)).bg(theme.panel.to_hsla());
                if tools.is_empty() {
                    let why = if server.enabled { "Connect to see its tools" } else { "Turn it on to see its tools" };
                    list = list.child(div().py(px(6.)).text_size(px(12.)).text_color(theme.muted.to_hsla()).child(why));
                }
                let levels = self.state.settings.mcp_tools.get(&server.name).cloned().unwrap_or_default();
                for (t, tool) in tools.iter().take(limit).enumerate() {
                    let level = levels.get(tool).copied().unwrap_or(Permission::Ask);
                    list = list.child(mcp_tool_row(&server.name, i, t, tool, level, weak, &theme));
                }
                if left > 0 {
                    list = list.child(div().pt(px(10.)).pb(px(2.)).child(link(("mcp-more", i), format!("Show {} more · {left} left", left.min(10)), theme.acc2, listen(cx, move |this, cx| this.show_more(&key, 6, 10, cx)))));
                }
                row = row.child(list);
            }
            rows.push(row.into_any_element());
        }
        let title = if rows.is_empty() { "MCP servers".to_string() } else { format!("MCP servers · {}", rows.len()) };
        let body = if rows.is_empty() {
            empty_box(None, if servers.is_empty() { "No MCP servers yet" } else { "No matching servers" }, &theme)
        } else {
            card(rows, &theme).into_any_element()
        };
        let search = (!servers.is_empty()).then(|| self.settings.searches.get("mcp").map(|(input, _)| search_box(input, &theme))).flatten();
        div().flex().flex_col().child(section_head(title, None, Vec::new(), &theme)).children(search).child(body).into_any_element()
    }

    /// Each installed agent's id with the tool settings its harness applies, in the Skills page's order.
    fn tool_support(&self) -> Vec<(&'static str, ToolSupport)> {
        self.agents.iter().map(|a| (a.harness.id(), a.harness.tool_support())).collect()
    }

    /// The Workspace Tools section: the IDE engine, the code-server program and the browser's search engine.
    fn workspace_tools(&self, weak: &gpui::WeakEntity<Shell>, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let ide_index = if self.state.settings.ide == IdeKind::VsCode { 1 } else { 0 };
        let ide = segmented_row(
            "ide-engine",
            "IDE engine",
            "",
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

        let detected = crate::ide::find_code_server(self.state.settings.code_server_path.as_deref());
        let configured = self.state.settings.code_server_path.is_some();
        let shown = detected.as_deref().map_or_else(|| "Automatic".to_string(), super::general::tilde);
        let code_server = SettingRow::new("code-server path", "Found automatically if empty", theme).control(
            div()
                .flex()
                .items_center()
                .gap(px(10.))
                .child(path_label(&shown, detected.is_some(), theme))
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
            "",
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

        section("Workspace tools", vec![ide, code_server, engine], theme)
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
}

/// The status dot, its word, and whether a check is running, for a server that is `enabled` and was last checked as `status`.
fn mcp_mark(status: &Option<Option<McpStatus>>, enabled: bool, theme: &Theme) -> (Rgba, &'static str, bool) {
    match (status, enabled) {
        (_, false) => (theme.fg.alpha(0.4), "Off", false),
        (Some(None), true) => (theme.fg.alpha(0.4), "Connecting…", true),
        (Some(Some(McpStatus::Connected { .. })), true) => (GREEN, "Connected", false),
        (Some(Some(McpStatus::Failed(_))), true) => (AMBER, "Unreachable", false),
        _ => (theme.fg.alpha(0.4), "Not checked", false),
    }
}

/// One tool of an open MCP server: its name, and the Allow, Ask and Off well that sets its permission.
/// `server` names the server that keys the permissions; `i` and `t` place the server and the tool, for ids.
fn mcp_tool_row(server: &str, i: usize, t: usize, tool: &str, level: Permission, weak: &gpui::WeakEntity<Shell>, theme: &Theme) -> AnyElement {
    let (server, tool_name) = (server.to_string(), tool.to_string());
    div()
        .flex()
        .items_center()
        .gap(px(12.))
        .py(px(6.))
        .border_b_1()
        .border_color(theme.line.to_hsla())
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .font_family(theme.code_font.clone())
                .text_size(px(12.))
                .overflow_hidden()
                .text_ellipsis()
                .whitespace_nowrap()
                .child(tool_name.clone()),
        )
        .child(small_seg(
            SharedString::from(format!("mcp-tool-{i}-{t}")),
            permission_index(level),
            theme,
            edit(weak, move |this: &mut Shell, choice: usize, cx| {
                let level = permission_at(choice);
                let tools = this.state.settings.mcp_tools.entry(server.clone()).or_default();
                if level == Permission::Ask {
                    tools.remove(&tool_name);
                } else {
                    tools.insert(tool_name.clone(), level);
                }
                if tools.is_empty() {
                    this.state.settings.mcp_tools.remove(&server);
                }
                this.save(cx);
            }),
        ))
        .into_any_element()
}

/// Allow, Ask and Off in a small well, as each MCP tool row shows them.
fn small_seg(
    id: SharedString,
    selected: usize,
    theme: &Theme,
    on_pick: impl Fn(usize, &mut gpui::Window, &mut gpui::App) + 'static,
) -> AnyElement {
    let on_pick = std::rc::Rc::new(on_pick);
    let mut well = div().flex().flex_none().gap(px(2.)).p(px(2.)).rounded(px(7.)).bg(theme.line.to_hsla());
    for (i, label) in PERMISSION_LABELS.iter().enumerate() {
        let on = i == selected;
        let on_pick = on_pick.clone();
        well = well.child(
            div()
                .id(gpui::ElementId::NamedInteger(id.clone(), i as u64))
                .px(px(10.))
                .py(px(3.))
                .rounded(px(5.))
                .text_size(px(12.))
                .cursor_pointer()
                .when(on, |d| d.bg(theme.line2.to_hsla()).text_color(theme.fg.to_hsla()))
                .when(!on, |d| d.text_color(theme.muted.to_hsla()))
                .on_click(move |_, window, cx| on_pick(i, window, cx))
                .child(*label),
        );
    }
    well.into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every row gated, asks per use, takes MCP: an agent that applies everything.
    const FULL: ToolSupport = ToolSupport { mcp: true, permissions: [true; 7], ask: true, skill_toggles: true };
    /// No MCP, can't ask, and can't switch off web search (row 5).
    const PARTIAL: ToolSupport = ToolSupport { mcp: false, permissions: [true, true, true, true, true, false, true], ask: false, skill_toggles: false };

    fn agents() -> Vec<(&'static str, ToolSupport)> {
        vec![("claude", FULL), ("pi", PARTIAL)]
    }

    fn lit<'a>(marks: &[AgentMark<'a>]) -> Vec<(&'a str, bool, bool)> {
        marks.iter().map(|m| (m.agent, m.lit, m.struck)).collect()
    }

    #[test]
    fn mcp_reaches_only_agents_that_take_it() {
        let on = mcp_marks(&agents(), true);
        assert_eq!(lit(&on), [("claude", true, false), ("pi", false, true)]);
        assert_eq!(on[0].note, None);
        assert_eq!(on[1].note.as_deref(), Some("Pi doesn't take MCP servers from ArchonUI"));
        // A server that's off reaches nobody, and nobody is ignoring a choice.
        assert_eq!(lit(&mcp_marks(&agents(), false)), [("claude", false, false), ("pi", false, false)]);
    }

    #[test]
    fn off_is_ignored_where_the_tool_cant_be_switched_off() {
        let marks = permission_marks(&agents(), 5, Permission::Off);
        assert_eq!(lit(&marks), [("claude", true, false), ("pi", false, true)]);
        assert_eq!(marks[1].note.as_deref(), Some("Pi can't switch off web search, so it runs as Allow"));
        assert_eq!(ignored_by(&marks), "Ignored by Pi");
        // A row Pi can gate is honoured by both.
        let marks = permission_marks(&agents(), 0, Permission::Off);
        assert_eq!(lit(&marks), [("claude", true, false), ("pi", true, false)]);
        assert_eq!(ignored_by(&marks), "");
    }

    #[test]
    fn ask_is_ignored_by_agents_that_cant_prompt() {
        let marks = permission_marks(&agents(), 0, Permission::Ask);
        assert_eq!(lit(&marks), [("claude", true, false), ("pi", false, true)]);
        assert_eq!(marks[1].note.as_deref(), Some("Pi can't ask per use, so it runs file reading as Allow"));
    }

    #[test]
    fn allow_fades_but_never_strikes() {
        let marks = permission_marks(&agents(), 5, Permission::Allow);
        assert_eq!(lit(&marks), [("claude", true, false), ("pi", false, false)]);
        // The note still warns before the user picks Off.
        assert!(marks[1].note.is_some());
        assert_eq!(ignored_by(&marks), "");
    }

    #[test]
    fn default_support_applies_nothing() {
        let none = [("gemini", ToolSupport::default())];
        assert_eq!(lit(&permission_marks(&none, 3, Permission::Off)), [("gemini", false, true)]);
        assert_eq!(lit(&mcp_marks(&none, true)), [("gemini", false, true)]);
    }

    #[test]
    fn summaries_count_per_agent() {
        assert_eq!(mcp_summary(&agents(), 3), "Claude 3 · Pi 0");
        let allow = Permission::Allow;
        let p = Permissions { read: allow, edit: allow, grep: allow, run: Permission::Ask, browser: allow, web: Permission::Off, fetch: allow };
        // Pi ignores web search Off and can't ask before shell commands.
        assert_eq!(permission_summary(&agents(), p), "Claude 7/7 · Pi 5/7");
        assert_eq!(mcp_summary(&[], 2), "");
    }
}
