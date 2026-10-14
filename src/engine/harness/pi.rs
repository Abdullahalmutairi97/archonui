//! Pi: `pi -p` with inlined `@file` arguments. Pi has no permission system,
//! so Plan and Ask narrow the tool list instead.

use super::{prompt_arg, strs, AuthStatus, Harness, Mode, Plan, ToolSettings, TurnInput, PLAN_PREAMBLE};
use crate::app::settings::{McpServer, Permission, Permissions};
use crate::engine::{
    attach::{Delivery, Kind},
    discover::{list, probe_jsonl, Capabilities, ModelInfo},
    mcp,
    parse::Protocol,
    skills::Skill,
};
use serde_json::Value;
use std::{env, path::Path, process::Command};

pub struct Pi;

impl Harness for Pi {
    fn id(&self) -> &'static str {
        "pi"
    }

    fn name(&self) -> &'static str {
        "Pi"
    }

    /// Line icon: π.
    fn glyph(&self) -> Option<&'static str> {
        Some("M2.5 4.5h11M5.5 4.5v9M10.5 4.5v6.5c0 1.5.8 2.5 2.3 2")
    }
    fn binary(&self) -> &'static str {
        "pi"
    }

    fn protocol(&self) -> Protocol {
        Protocol::Pi
    }

    fn modes(&self) -> &'static [Mode] {
        &Mode::ALL
    }

    fn preassign_session(&self) -> bool {
        true
    }

    /// Pi inlines `@file` arguments: images as images, everything else as text,
    /// so only images and real text files go that way.
    fn plan(&self, input: &TurnInput) -> Plan {
        let files = Delivery::split(input.files, |a| a.kind == Kind::Image || super::small_text(a));
        let mcp = self.mcp_args(input.tools.mcp);
        let mut a = strs(&["-p", "--mode", "json"]);
        if let Some(model) = input.model {
            a.extend(strs(&["--model", model]));
        }
        if let Some(effort) = input.effort {
            // Pi's thinking levels are lowercase; the Models page labels them in title case.
            a.extend(strs(&["--thinking", &effort.to_lowercase()]));
        }
        a.extend(mode_flags(input.mode));
        a.extend(tool_flags(&input.tools));
        a.extend(mcp.args);
        if let Some(id) = input.session {
            a.extend(strs(&["--session-id", id]));
        }
        for f in &files.native {
            a.push(format!("@{}", f.path.display()));
        }
        a.push(pi_prompt(&files.prompt(input.prompt)));
        Plan { args: a, files: mcp.files, ..Default::default() }
    }

    /// `--mcp-config` (from the pi-mcp-adapter extension) with the enabled servers in a file.
    /// Pi's own MCP config is read as well, so an imported server cannot be switched off here.
    fn mcp_args(&self, servers: &[McpServer]) -> Plan {
        match mcp::config_file("pi.json", servers, false) {
            Some((path, text)) => {
                Plan { args: strs(&["--mcp-config", &path.display().to_string()]), files: vec![(path, text)], ..Default::default() }
            }
            None => Plan::default(),
        }
    }

    fn discover(&self, path: &Path, cwd: &Path) -> Capabilities {
        let mut cmd = Command::new(path);
        cmd.args(["--mode", "rpc", "--no-session"]);
        let Some([cmds, models, state]) = probe_jsonl(
            cmd,
            cwd,
            [
                r#"{"id":"archonui-cmds","type":"get_commands"}"#,
                r#"{"id":"archonui-models","type":"get_available_models"}"#,
                r#"{"id":"archonui-state","type":"get_state"}"#,
            ],
            [
                |v: &Value| v["command"] == "get_commands",
                |v: &Value| v["command"] == "get_available_models",
                |v: &Value| v["command"] == "get_state",
            ],
        ) else {
            return Capabilities::default();
        };
        let mut commands = list(&cmds, "/data/commands", "");
        // Extension commands drive Pi's interactive UI and do nothing in `-p`
        // mode; prompt templates and skills expand into a normal prompt.
        let extensions: Vec<&str> = cmds
            .pointer("/data/commands")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|c| c["source"] == "extension")
            .filter_map(|c| c["name"].as_str())
            .collect();
        commands.retain(|c| !extensions.contains(&c.name.as_str()));

        let pi_id = |m: &Value| Some(format!("{}/{}", m["provider"].as_str()?, m["id"].as_str()?));
        let models = models
            .pointer("/data/models")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|m| {
                let id = pi_id(m)?;
                Some(ModelInfo {
                    label: m["name"].as_str().map_or_else(|| id.clone(), String::from),
                    description: String::new(), // the picker already shows the id
                    efforts: thinking_levels(m),
                    id,
                })
            })
            .collect();
        let current = state.pointer("/data/model");
        Capabilities {
            commands,
            models,
            default_model: current.and_then(pi_id),
            default_efforts: current.map(thinking_levels).unwrap_or_default(),
            default_effort: state.pointer("/data/thinkingLevel").and_then(Value::as_str).map(String::from),
        }
    }

    fn auth_status(&self) -> AuthStatus {
        let Some(home) = env::var_os("HOME") else { return AuthStatus::Missing };
        auth_in(&Path::new(&home).join(".pi/agent"))
    }
}

/// Pi reads any argument starting with `@` as a file to include, even after `--`, so a prompt
/// like "@src/main.rs explain this" would become one missing file and no message. A leading space
/// keeps it a message, as `prompt_arg` does for a leading `-`.
fn pi_prompt(p: &str) -> String {
    if p.starts_with('@') {
        format!(" {p}")
    } else {
        prompt_arg(p)
    }
}

/// Pi is signed in when `auth.json` holds at least one credential, or `models.json`
/// defines a provider with its own `apiKey`. Read-only: the config dir is never written.
fn auth_in(dir: &Path) -> AuthStatus {
    let json = |name: &str| std::fs::read_to_string(dir.join(name)).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok());
    if json("auth.json").and_then(|v| v.as_object().map(|m| !m.is_empty())).unwrap_or(false) {
        return AuthStatus::Ok;
    }
    let keyed = json("models.json")
        .and_then(|v| {
            v.pointer("/providers").and_then(Value::as_object).map(|providers| {
                providers
                    .values()
                    .any(|p| p.get("apiKey").and_then(Value::as_str).is_some_and(|k| !k.is_empty()))
            })
        })
        .unwrap_or(false);
    if keyed {
        AuthStatus::Ok
    } else {
        AuthStatus::Missing
    }
}

/// Plan and Ask keep only read tools, and `--no-mcp` because `--tools` would
/// otherwise keep MCP tools enabled. Plan also gets the plan-only instruction
/// as a system prompt addition.
fn mode_flags(mode: Mode) -> Vec<String> {
    const READ_ONLY: &str = "read,grep,find,ls";
    match mode {
        Mode::Auto => Vec::new(),
        Mode::Plan => strs(&["--tools", READ_ONLY, "--no-mcp", "--append-system-prompt", PLAN_PREAMBLE]),
        Mode::Ask => strs(&["--tools", READ_ONLY, "--no-mcp"]),
    }
}

/// `--exclude-tools` for the tools whose permission is Off, and skill flags.
fn tool_flags(tools: &ToolSettings) -> Vec<String> {
    let mut flags = skill_flags(tools);
    let excluded = excluded_tools(tools.permissions);
    if !excluded.is_empty() {
        flags.extend(strs(&["--exclude-tools", &excluded.join(",")]));
    }
    flags
}

/// Pi tools for each permission set to Off. Pi has no browser or web tools
/// built in, so those are not mapped.
fn excluded_tools(p: Permissions) -> Vec<&'static str> {
    let off = |perm: Permission| perm == Permission::Off;
    let mut tools = Vec::new();
    if off(p.read) {
        tools.push("read");
    }
    if off(p.grep) {
        tools.extend(["grep", "find", "ls"]);
    }
    if off(p.edit) {
        tools.extend(["edit", "write"]);
    }
    if off(p.run) {
        tools.push("bash");
    }
    tools
}

/// Pi's skill discovery is off, and each enabled skill is loaded by path, once any
/// skill is switched off. Without a switched-off skill, Pi's own discovery stays as it is.
fn skill_flags(tools: &ToolSettings) -> Vec<String> {
    let off = |s: &Skill| tools.disabled_skills.contains(&s.id);
    if !tools.skills.iter().any(off) {
        return Vec::new();
    }
    let mut flags = strs(&["--no-skills"]);
    for skill in tools.skills.iter().filter(|s| !off(s)) {
        flags.extend(["--skill".into(), skill.path.display().to_string()]);
    }
    flags
}

/// Pi's own rule (`getSupportedThinkingLevels`): a `null` map entry disables a
/// level, and `xhigh`/`max` must be mapped explicitly.
fn thinking_levels(model: &Value) -> Vec<String> {
    if model["reasoning"] != true {
        return Vec::new();
    }
    let map = &model["thinkingLevelMap"];
    ["off", "minimal", "low", "medium", "high", "xhigh", "max"]
        .into_iter()
        .filter(|level| match map.get(level) {
            Some(Value::Null) => false,
            Some(_) => true,
            None => !matches!(*level, "xhigh" | "max"),
        })
        .map(String::from)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thinking_levels_follow_pi_rules() {
        let m = serde_json::json!({"reasoning": true, "thinkingLevelMap": {"off": null, "minimal": null, "max": "max"}});
        assert_eq!(thinking_levels(&m), ["low", "medium", "high", "max"]);
        assert!(thinking_levels(&serde_json::json!({"reasoning": false})).is_empty());
    }

    #[test]
    fn model_and_effort_reach_the_command_line() {
        let input = TurnInput {
            cwd: Path::new("/work"),
            prompt: "do it",
            session: None,
            files: &[],
            model: Some("deepseek/deepseek-flash"),
            effort: Some("High"),
            mode: Mode::Auto,
            tools: ToolSettings::default(),
        };
        let args = Pi.plan(&input).args;
        let pair = |flag: &str| args.windows(2).find(|w| w[0] == flag).map(|w| w[1].as_str());
        assert_eq!(pair("--model"), Some("deepseek/deepseek-flash"));
        assert_eq!(pair("--thinking"), Some("high"));
    }

    #[test]
    fn prompts_starting_with_at_stay_messages() {
        assert_eq!(pi_prompt("@src/main.rs explain"), " @src/main.rs explain");
        assert_eq!(pi_prompt("-x"), " -x");
        assert_eq!(pi_prompt("fix it"), "fix it");
        let input = TurnInput {
            cwd: Path::new("/work"),
            prompt: "@notes.md summarise",
            session: None,
            files: &[],
            model: None,
            effort: None,
            mode: Mode::Auto,
            tools: ToolSettings::default(),
        };
        assert_eq!(Pi.plan(&input).args.last().map(String::as_str), Some(" @notes.md summarise"));
    }

    #[test]
    fn auth_reads_the_config_dir_and_never_writes_it() {
        let dir = env::temp_dir().join(format!("archonui-pi-auth-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // `pi` writes an empty auth.json before any login.
        std::fs::write(dir.join("auth.json"), "{}").unwrap();
        assert_eq!(auth_in(&dir), AuthStatus::Missing);
        std::fs::write(dir.join("auth.json"), r#"{"deepseek":{"type":"api_key","key":"sk"}}"#).unwrap();
        assert_eq!(auth_in(&dir), AuthStatus::Ok);
        // A key on a custom provider in models.json counts too.
        std::fs::write(dir.join("auth.json"), "{}").unwrap();
        std::fs::write(dir.join("models.json"), r#"{"providers":{"local":{"apiKey":"k"}}}"#).unwrap();
        assert_eq!(auth_in(&dir), AuthStatus::Ok);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
