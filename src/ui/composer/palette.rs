//! The slash palette: local commands, the agent's commands, and the model, effort and mode lists
//! that `/model`, `/effort` and `/mode` open. Ported from `chat.rs` (`palette`, `run_local`).

use super::{picker::Pop, Shell};
use crate::{
    app::workspace::RightTab,
    engine::{Capabilities, Mode},
};
use gpui::Context;

/// Commands ArchonUI handles itself; everything else goes to the agent.
pub const LOCAL_COMMANDS: &[(&str, &str)] = &[
    ("new", "Start a new chat with this agent"),
    ("stop", "Stop the running reply"),
    ("agent", "Hand this chat to another agent"),
    ("model", "Change the model this chat uses"),
    ("effort", "Change the effort (reasoning) level"),
    ("mode", "Change what the agent may do: Auto, Plan or Ask"),
    ("preview", "Open a web address or HTML file in the preview"),
    ("terminal", "Open a terminal in this chat's folder"),
    ("edit", "Open a file in the built-in editor"),
    ("rescan", "Rescan for agents, their commands and models"),
    ("settings", "Open the settings page"),
];

/// What choosing a palette row does.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// Run the slash command in the entry's name.
    Command,
    /// Switch the session to a model; `None` is the agent's default.
    Model(Option<String>),
    /// Switch the session to an effort level; `None` is the agent's default.
    Effort(Option<String>),
    /// Switch the session to a mode.
    Mode(Mode),
    /// Hand the session to the agent with this registry id.
    Agent(&'static str),
}

#[derive(Clone, Debug, PartialEq)]
pub struct PaletteEntry {
    pub name: String,
    pub hint: String,
    pub description: String,
    pub tags: Vec<&'static str>,
    pub action: Action,
}

/// What the palette reads from a session and its agent.
pub struct PaletteContext<'a> {
    pub caps: Option<&'a Capabilities>,
    pub model: Option<&'a str>,
    pub effort: Option<&'a str>,
    pub mode: Mode,
    /// The modes the session's agent enforces.
    pub modes: &'a [Mode],
    /// Pinned model ids for the session's agent, in pin order.
    pub pins: &'a [String],
    /// The session's agent id.
    pub agent: &'a str,
    /// Every enabled agent as (id, name), in detection order.
    pub agents: &'a [(&'static str, &'static str)],
}

/// The palette rows for `input`, or `None` when the input is not a palette query: a bare `/word`
/// (commands), or `/agent …`, `/model …`, `/effort …` or `/mode …` (their lists). Other slash text is a message.
pub fn palette(input: &str, ctx: &PaletteContext) -> Option<Vec<PaletteEntry>> {
    let rest = input.strip_prefix('/')?;
    if let Some(query) = rest.strip_prefix("agent ") {
        return Some(agent_entries(ctx, query.trim()));
    }
    if let Some(query) = rest.strip_prefix("model ") {
        return Some(model_entries(ctx, query.trim()));
    }
    if let Some(query) = rest.strip_prefix("effort ") {
        return Some(effort_entries(ctx, query.trim()));
    }
    if let Some(query) = rest.strip_prefix("mode ") {
        return Some(mode_entries(ctx, query.trim()));
    }
    if rest.contains(char::is_whitespace) {
        return None;
    }
    let local = LOCAL_COMMANDS.iter().map(|(name, description)| PaletteEntry {
        name: name.to_string(),
        hint: String::new(),
        description: description.to_string(),
        tags: vec!["ArchonUI"],
        action: Action::Command,
    });
    // An agent command named like a local one is never reached (local commands run first), so it is not listed twice.
    let agent = ctx.caps.map(|c| &c.commands[..]).unwrap_or_default().iter().filter(|c| !LOCAL_COMMANDS.iter().any(|(name, _)| *name == c.name)).map(|c| PaletteEntry {
        name: c.name.clone(),
        hint: c.hint.clone(),
        description: c.description.clone(),
        tags: Vec::new(),
        action: Action::Command,
    });
    Some(rank(local.chain(agent), rest))
}

/// Pinned models first, then the agent default, then the rest.
fn model_entries(ctx: &PaletteContext, query: &str) -> Vec<PaletteEntry> {
    let all = ctx.caps.map(|c| &c.models[..]).unwrap_or_default();
    let entry = |m: &crate::engine::ModelInfo| {
        let mut tags = Vec::new();
        if ctx.pins.contains(&m.id) {
            tags.push("pinned");
        }
        if ctx.model == Some(m.id.as_str()) {
            tags.push("current");
        }
        PaletteEntry {
            name: m.label.clone(),
            hint: if m.label == m.id { String::new() } else { m.id.clone() },
            description: m.description.clone(),
            tags,
            action: Action::Model(Some(m.id.clone())),
        }
    };
    // A pin whose model disappeared is skipped.
    let pinned = ctx.pins.iter().filter_map(|id| all.iter().find(|m| &m.id == id)).map(entry);
    let default = PaletteEntry {
        name: "Agent default".into(),
        hint: String::new(),
        description: ctx.caps.and_then(|c| c.default_model.clone()).unwrap_or_default(),
        tags: if ctx.model.is_none() { vec!["current"] } else { Vec::new() },
        action: Action::Model(None),
    };
    let rest = all.iter().filter(|m| !ctx.pins.contains(&m.id)).map(entry);
    rank(pinned.chain(std::iter::once(default)).chain(rest), query)
}

fn effort_entries(ctx: &PaletteContext, query: &str) -> Vec<PaletteEntry> {
    let levels = ctx.caps.map(|c| c.efforts(ctx.model)).unwrap_or_default();
    let default_level = ctx.caps.and_then(|c| c.default_effort.as_deref());
    let default = PaletteEntry {
        name: "Agent default".into(),
        hint: String::new(),
        description: default_level.map(|l| format!("currently {l}")).unwrap_or_default(),
        tags: if ctx.effort.is_none() { vec!["current"] } else { Vec::new() },
        action: Action::Effort(None),
    };
    let levels = levels.iter().map(|l| PaletteEntry {
        name: l.clone(),
        hint: String::new(),
        description: String::new(),
        tags: if ctx.effort == Some(l.as_str()) { vec!["current"] } else { Vec::new() },
        action: Action::Effort(Some(l.clone())),
    });
    rank(std::iter::once(default).chain(levels), query)
}

/// Only the modes the session's agent enforces.
fn mode_entries(ctx: &PaletteContext, query: &str) -> Vec<PaletteEntry> {
    let entries = ctx.modes.iter().map(|&m| PaletteEntry {
        name: m.name().into(),
        hint: String::new(),
        description: m.blurb().into(),
        tags: if ctx.mode == m { vec!["current"] } else { Vec::new() },
        action: Action::Mode(m),
    });
    rank(entries, query)
}

/// The enabled agents, matched by name or id.
fn agent_entries(ctx: &PaletteContext, query: &str) -> Vec<PaletteEntry> {
    let entries = ctx.agents.iter().map(|&(id, name)| PaletteEntry {
        name: name.into(),
        hint: id.into(),
        description: if id == ctx.agent { String::new() } else { "Gets this chat so far with your next message".into() },
        tags: if id == ctx.agent { vec!["current"] } else { Vec::new() },
        action: Action::Agent(id),
    });
    rank(entries, query)
}

/// Prefix matches first, then matches in the name or hint. Order within each group is kept.
pub fn rank(entries: impl Iterator<Item = PaletteEntry>, query: &str) -> Vec<PaletteEntry> {
    let query = query.to_lowercase();
    let (mut prefix, mut contains) = (Vec::new(), Vec::new());
    for e in entries {
        let name = e.name.to_lowercase();
        if name.starts_with(&query) {
            prefix.push(e);
        } else if name.contains(&query) || e.hint.to_lowercase().contains(&query) {
            contains.push(e);
        }
    }
    prefix.append(&mut contains);
    prefix
}

impl Shell {
    /// Palette rows for `input` in session `id`. See [`palette`].
    pub(super) fn palette_entries(&self, id: &str, input: &str) -> Option<Vec<PaletteEntry>> {
        let s = self.state.session(id)?;
        let agents = self.enabled_agents();
        let ctx = PaletteContext {
            caps: self.caps_for(&s.agent),
            model: s.model.as_deref(),
            effort: s.effort.as_deref(),
            mode: s.mode,
            modes: self.supported_modes(&s.agent),
            pins: self.state.settings.pins(&s.agent),
            agent: &s.agent,
            agents: &agents,
        };
        palette(input, &ctx)
    }

    /// Runs the palette row the user chose in session `id`.
    pub(super) fn run_palette_entry(&mut self, id: &str, entry: &PaletteEntry, cx: &mut Context<Self>) {
        match &entry.action {
            Action::Command => self.submit_text(id, &format!("/{}", entry.name), cx),
            Action::Model(model) => {
                self.clear_input(id, cx);
                self.set_model(id, model.clone(), cx);
            }
            Action::Effort(level) => {
                self.clear_input(id, cx);
                self.set_effort(id, level.clone(), cx);
            }
            Action::Mode(mode) => {
                self.clear_input(id, cx);
                self.set_mode(id, *mode, cx);
            }
            Action::Agent(agent) => {
                self.clear_input(id, cx);
                self.switch_agent(id, agent, cx);
            }
        }
    }

    /// Every enabled agent as (id, name), in detection order.
    fn enabled_agents(&self) -> Vec<(&'static str, &'static str)> {
        self.agents.iter().filter(|a| a.enabled).map(|a| (a.harness.id(), a.harness.name())).collect()
    }

    /// Runs `/name arg` if it is a local command. Returns false when the agent should get the text.
    pub(super) fn run_local(&mut self, id: &str, name: &str, arg: &str, cx: &mut Context<Self>) -> bool {
        let Some(s) = self.state.session(id) else { return false };
        let (cwd, agent) = (s.cwd.clone(), s.agent.clone());
        // A chat on another device has its folder there, not here.
        let remote = self.chat_device(id);
        match name {
            "new" => self.new_session_split(cx),
            "stop" => {
                self.stop_chat(id);
            }
            "rescan" => self.rescan(cx),
            // `/agent <name>` takes an agent's id or name, ignoring case.
            "agent" if !arg.is_empty() => {
                let agents = self.enabled_agents();
                match agents.iter().find(|(a, name)| a.eq_ignore_ascii_case(arg) || name.eq_ignore_ascii_case(arg)) {
                    Some(&(agent, _)) => self.switch_agent(id, agent, cx),
                    None => {
                        let names: Vec<&str> = agents.iter().map(|(a, _)| *a).collect();
                        self.toast(format!("No agent '{arg}'. Use one of: {}.", names.join(", ")), cx)
                    }
                }
            }
            "agent" => self.open_popover(id, Pop::Model, cx),
            "settings" => self.open_settings(cx),
            // `/model <id>` sets an id directly, including ones not listed.
            // A value starting with `-` would read as a flag on the agent's command line.
            "model" | "effort" if arg.starts_with('-') => self.toast(format!("'{arg}' isn't a valid {name}."), cx),
            "model" if !arg.is_empty() => self.set_model(id, Some(arg.to_string()), cx),
            "model" => self.open_popover(id, Pop::Model, cx),
            "effort" if !arg.is_empty() => self.set_effort(id, Some(arg.to_string()), cx),
            "effort" => self.open_popover(id, Pop::Model, cx),
            // `/mode <name>` must name a mode this agent enforces.
            "mode" if !arg.is_empty() => match Mode::from_name(arg) {
                Some(mode) if self.supported_modes(&agent).contains(&mode) => self.set_mode(id, mode, cx),
                _ => self.toast(format!("Unknown mode '{arg}'. Use Auto, Plan or Ask."), cx),
            },
            "mode" => self.open_popover(id, Pop::Model, cx),
            "preview" if arg.is_empty() => self.show_tool_tab(RightTab::Browser, cx),
            "preview" => match preview_target(arg, &cwd, remote.as_deref()) {
                Ok(url) => self.open_url(&url, cx),
                Err(message) => self.toast(message, cx),
            },
            "terminal" => self.show_tool_tab(RightTab::Terminal, cx),
            "edit" => {
                self.show_tool_tab(RightTab::Ide, cx);
                // The editor tab shows a note for a remote chat; the file is on its device.
                if !arg.is_empty() && remote.is_none() {
                    self.open_in_editor(cwd.join(arg), cx);
                }
            }
            _ => return false,
        }
        true
    }

    /// The agent's modes, or every mode when the agent is not installed.
    pub(super) fn supported_modes(&self, agent: &str) -> &'static [Mode] {
        self.agents.iter().find(|a| a.harness.id() == agent).map_or(&Mode::ALL[..], |a| a.harness.modes())
    }
}
/// Where `/preview arg` goes for a chat in `cwd`. For a chat on `remote`, a path relative to its
/// folder is refused: that folder is on the other device, and this one's copy of it isn't the chat's.
fn preview_target(arg: &str, cwd: &std::path::Path, remote: Option<&str>) -> Result<String, String> {
    let url = crate::preview::normalize_address(arg, cwd);
    let full_path = arg.starts_with('/') || arg.starts_with("~/") || arg.to_ascii_lowercase().starts_with("file://");
    match remote {
        Some(device) if url.starts_with("file://") && !full_path => Err(format!(
            "This chat's folder is on {device}. /preview takes a web address or a full path on this device."
        )),
        _ => Ok(url),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_refuses_paths_relative_to_a_remote_chats_folder() {
        let cwd = std::env::temp_dir().join(format!("archonui-preview-{}", std::process::id()));
        std::fs::create_dir_all(&cwd).unwrap();
        std::fs::write(cwd.join("index.html"), "<p>hi</p>").unwrap();
        for relative in ["./index.html", "../x.html", "index.html"] {
            assert!(preview_target(relative, &cwd, Some("laptop")).is_err(), "{relative}");
            assert!(preview_target(relative, &cwd, None).is_ok_and(|u| u.starts_with("file://")), "{relative}");
        }
        for fine in ["https://example.com", "localhost:3000", "/tmp/page.html", "~/page.html"] {
            assert_eq!(preview_target(fine, &cwd, Some("laptop")), preview_target(fine, &cwd, None), "{fine}");
        }
        std::fs::remove_dir_all(&cwd).ok();
    }

    fn entry(name: &str, hint: &str) -> PaletteEntry {
        PaletteEntry {
            name: name.into(),
            hint: hint.into(),
            description: String::new(),
            tags: Vec::new(),
            action: Action::Command,
        }
    }

    #[test]
    fn rank_puts_prefix_matches_first_and_drops_the_rest() {
        let entries = vec![entry("restart", ""), entry("terminal", ""), entry("stop", "")];
        let names: Vec<String> = rank(entries.into_iter(), "st").into_iter().map(|e| e.name).collect();
        assert_eq!(names, vec!["stop", "restart"]);
    }

    #[test]
    fn rank_matches_hints_after_names() {
        let names: Vec<String> = rank(vec![entry("review", "fix tests"), entry("tests", "")].into_iter(), "test")
            .into_iter()
            .map(|e| e.name)
            .collect();
        assert_eq!(names, vec!["tests", "review"]);
    }

    #[test]
    fn bare_command_word_is_a_palette_query_but_sentences_are_not() {
        let ctx = PaletteContext { caps: None, model: None, effort: None, mode: Mode::Auto, modes: &Mode::ALL, pins: &[], agent: "pi", agents: &[] };
        let names: Vec<String> = palette("/se", &ctx).unwrap().into_iter().map(|e| e.name).collect();
        assert_eq!(names, vec!["settings"]);
        assert!(palette("/review the code", &ctx).is_none());
        assert!(palette("hello", &ctx).is_none());
    }

    #[test]
    fn agent_list_offers_the_enabled_agents_and_marks_the_current_one() {
        let agents = [("claude", "Claude Code"), ("codex", "Codex CLI")];
        let ctx = PaletteContext { caps: None, model: None, effort: None, mode: Mode::Auto, modes: &Mode::ALL, pins: &[], agent: "codex", agents: &agents };
        let rows = palette("/agent ", &ctx).unwrap();
        let picked: Vec<(&str, &Action, &[&str])> = rows.iter().map(|e| (e.name.as_str(), &e.action, &e.tags[..])).collect();
        assert_eq!(picked, vec![("Claude Code", &Action::Agent("claude"), &[][..]), ("Codex CLI", &Action::Agent("codex"), &["current"][..])]);
        let found: Vec<String> = palette("/agent cla", &ctx).unwrap().into_iter().map(|e| e.name).collect();
        assert_eq!(found, vec!["Claude Code"]);
    }

    #[test]
    fn mode_list_offers_only_the_supported_modes() {
        let ctx = PaletteContext { caps: None, model: None, effort: None, mode: Mode::Plan, modes: &[Mode::Auto, Mode::Ask], pins: &[], agent: "pi", agents: &[] };
        let rows = palette("/mode ", &ctx).unwrap();
        let names: Vec<&str> = rows.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["Auto", "Ask"]);
        assert!(rows.iter().all(|e| e.tags.is_empty()));
    }
}
