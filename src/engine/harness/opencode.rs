//! OpenCode: `opencode run --format json`. Auto runs the default `build`
//! agent; Plan and Ask run the read-only `plan` agent.

use super::{
    planned, prompt_arg, scoped, signed_in, strs, AuthStatus, Harness, Mode, Plan, ToolSettings, TurnInput,
};
use crate::app::settings::{McpServer, McpTransport, Permission, Permissions};
use crate::engine::{
    attach::{Delivery, Kind},
    discover::{command, list, run_json, run_text, Capabilities, ModelInfo},
    parse::Protocol,
};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub struct OpenCode;

impl Harness for OpenCode {
    fn id(&self) -> &'static str {
        "opencode"
    }

    fn name(&self) -> &'static str {
        "OpenCode"
    }

    /// Line icon: code brackets.
    fn glyph(&self) -> Option<&'static str> {
        Some("M5.5 4.5L2.5 8l3 3.5M10.5 4.5l3 3.5-3 3.5M9 3.5l-2 9")
    }
    fn binary(&self) -> &'static str {
        "opencode"
    }

    fn protocol(&self) -> Protocol {
        Protocol::OpenCode
    }

    fn modes(&self) -> &'static [Mode] {
        &Mode::ALL
    }

    /// OpenCode attaches images and text with `-f`. It refuses to read outside
    /// the project unless allowed, so by-path files get a permission scoped to
    /// their directories.
    fn plan(&self, input: &TurnInput) -> Plan {
        let files = Delivery::split(input.files, |a| a.kind == Kind::Image || super::small_text(a));
        let mut a = strs(&["run", "--format", "json"]);
        // Without `--dir` OpenCode works in the project it finds, not the chat's folder.
        a.extend(["--dir".into(), input.cwd.display().to_string()]);
        if let Some(model) = input.model {
            a.extend(strs(&["--model", model]));
        }
        if let Some(effort) = input.effort {
            a.extend(strs(&["--variant", effort]));
        }
        // `plan` denies edits; `build` is the default agent.
        if input.mode != Mode::Auto {
            a.extend(strs(&["--agent", "plan"]));
        }
        if let Some(id) = input.session {
            a.extend(strs(&["--session", id]));
        }
        // OpenCode runs slash commands through a flag rather than the message.
        // A bare "/" names no command, so it goes as a message.
        // Plan and Ask share the `plan` agent, so Plan also asks for a plan in the message.
        let message = match input.prompt.strip_prefix('/').filter(|rest| !rest.trim().is_empty()) {
            Some(rest) => {
                let rest = rest.trim_start();
                let (name, args) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
                a.extend(strs(&["--command", name]));
                (!args.trim().is_empty() || !files.by_path.is_empty()).then(|| files.prompt(args.trim()))
            }
            None => Some(scoped(input, planned(input.mode, files.prompt(input.prompt)))),
        };
        for f in &files.native {
            a.extend(["-f".into(), f.path.display().to_string()]);
        }
        // `-f` takes several values, so end it before the message.
        if !files.native.is_empty() {
            a.push("--".into());
        }
        a.extend(message.map(|m| prompt_arg(&m)));
        // MCP servers, tool settings and the step limit share OpenCode's config variable with the
        // folder rules, so all go in `config_env`.
        let env = config_env(&files.dirs(), &input.tools, turn_agent(input.mode)).into_iter().collect();
        Plan { args: a, stdin: None, env, files: Vec::new() }
    }

    /// Commands come from `run --command`: two built-ins, config, and skills.
    /// `opencode models --verbose` lists connected providers' models.
    fn discover(&self, path: &Path, cwd: &Path) -> Capabilities {
        let mut commands = vec![
            command("init", "guided AGENTS.md setup", ""),
            command("review", "review changes, defaults to uncommitted", "[commit|branch|pr]"),
        ];
        let config = run_json(path, cwd, &["debug", "config"]);
        if let Some(Value::Object(map)) = config.as_ref().map(|v| v["command"].clone()) {
            for (name, c) in map {
                commands.push(command(&name, c["description"].as_str().unwrap_or(""), ""));
            }
        }
        if let Some(skills) = run_json(path, cwd, &["debug", "skill"]) {
            commands.extend(list(&skills, "", ""));
        }
        let models = models(&run_text(path, cwd, &["models", "--verbose"]).unwrap_or_default());
        let default_model = config.and_then(|c| c["model"].as_str().map(String::from));
        let default_efforts = models
            .iter()
            .find(|m| Some(&m.id) == default_model.as_ref())
            .map(|m| m.efforts.clone())
            .unwrap_or_default();
        Capabilities { commands, models, default_model, default_efforts, default_effort: None }
    }

    fn auth_status(&self) -> AuthStatus {
        signed_in(&[], &[".local/share/opencode/auth.json"])
    }
}

/// `opencode models --verbose` prints each model id on its own line followed
/// by a pretty-printed JSON object; its `variants` are the effort levels.
fn models(text: &str) -> Vec<ModelInfo> {
    let mut out = Vec::new();
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        let id = line.trim();
        if !id.contains('/') || id.contains(char::is_whitespace) || line.starts_with(' ') {
            continue;
        }
        let mut json = String::new();
        for l in lines.by_ref() {
            json.push_str(l);
            json.push('\n');
            if l == "}" {
                break;
            }
        }
        let v: Value = serde_json::from_str(&json).unwrap_or_default();
        out.push(ModelInfo {
            id: id.into(),
            label: v["name"].as_str().unwrap_or(id).into(),
            description: String::new(), // the picker already shows the id
            efforts: v["variants"].as_object().map(|o| o.keys().cloned().collect()).unwrap_or_default(),
        });
    }
    out
}

/// The agent a turn runs: `build` in Auto (OpenCode's default), `plan` otherwise.
fn turn_agent(mode: Mode) -> &'static str {
    match mode {
        Mode::Auto => "build",
        Mode::Plan | Mode::Ask => "plan",
    }
}

/// `OPENCODE_CONFIG_CONTENT`, which OpenCode merges over its own config: the
/// folders files may be read from, every server (switched-off ones with
/// `enabled: false`), `deny` for the tools switched off and the disabled skills, and
/// the step limit of `agent`. Merged into any value the user already set. `None` when
/// there is none of these.
fn config_env(dirs: &[PathBuf], tools: &ToolSettings, agent: &str) -> Option<(String, String)> {
    const VAR: &str = "OPENCODE_CONFIG_CONTENT";
    let (servers, steps) = (tools.mcp, tools.max_steps);
    let denied = denied_permissions(tools.permissions);
    let skills: Vec<&str> = tools
        .skills
        .iter()
        .filter(|s| tools.disabled_skills.contains(&s.id))
        .map(|s| s.name.as_str())
        .collect();
    if dirs.is_empty() && servers.is_empty() && steps.is_none() && denied.is_empty() && skills.is_empty() {
        return None;
    }
    let mut config: Value = std::env::var(VAR)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}));
    let rules = object_at(object_at(&mut config, "permission"), "external_directory");
    for d in dirs {
        rules[format!("{}/*", d.display())] = json!("allow");
    }
    for s in servers {
        object_at(&mut config, "mcp")[&s.name] = mcp_entry(s);
    }
    for key in denied {
        object_at(&mut config, "permission")[key] = json!("deny");
    }
    for name in skills {
        object_at(object_at(&mut config, "permission"), "skill")[name] = json!("deny");
    }
    if let Some(steps) = steps {
        object_at(object_at(&mut config, "agent"), agent)["steps"] = json!(steps);
    }
    Some((VAR.into(), config.to_string()))
}

/// `v[key]` as an object. A user's value there that isn't an object (e.g. `"permission": "allow"`)
/// is replaced, since indexing into it would panic.
fn object_at<'a>(v: &'a mut Value, key: &str) -> &'a mut Value {
    if !v[key].is_object() {
        v[key] = json!({});
    }
    &mut v[key]
}

/// OpenCode permission keys (from its permissions docs) for each permission set to Off. `edit`
/// covers writes and patches too.
fn denied_permissions(p: Permissions) -> Vec<&'static str> {
    let off = |perm: Permission| perm == Permission::Off;
    let mut keys = Vec::new();
    if off(p.read) {
        keys.push("read");
    }
    if off(p.grep) {
        keys.extend(["grep", "glob"]);
    }
    if off(p.edit) {
        keys.push("edit");
    }
    if off(p.run) {
        keys.push("bash");
    }
    if off(p.web) {
        keys.push("websearch");
    }
    if off(p.fetch) {
        keys.push("webfetch");
    }
    keys
}

/// An OpenCode `mcp` entry: a local command as one argv array, or a remote URL.
fn mcp_entry(s: &McpServer) -> Value {
    match &s.transport {
        McpTransport::Stdio { command, args, env } => {
            let mut argv = vec![command.clone()];
            argv.extend(args.iter().cloned());
            json!({"type": "local", "command": argv, "environment": env, "enabled": s.enabled})
        }
        McpTransport::Http { url, headers } => json!({"type": "remote", "url": url, "headers": headers, "enabled": s.enabled}),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn verbose_listing() {
        let text = "a/one\n{\n  \"name\": \"One\",\n  \"variants\": {\"low\": {}, \"high\": {}}\n}\nb/two\n{\n  \"variants\": {}\n}\n";
        let models = models(text);
        assert_eq!(models.len(), 2);
        assert_eq!((models[0].label.as_str(), &models[0].efforts[..]), ("One", &["low".to_string(), "high".to_string()][..]));
        assert!(models[1].efforts.is_empty());
    }

    #[test]
    fn config_env_merges_folder_rules_and_servers() {
        let servers = [
            McpServer {
                id: "local".into(),
                name: "local".into(),
                transport: McpTransport::Stdio {
                    command: "npx".into(),
                    args: vec!["-y".into(), "pkg".into()],
                    env: BTreeMap::from([("K".to_string(), "v".to_string())]),
                },
                enabled: true,
            },
            McpServer {
                id: "remote".into(),
                name: "remote".into(),
                transport: McpTransport::Http { url: "https://r.test/mcp".into(), headers: BTreeMap::new() },
                enabled: false,
            },
        ];
        let tools = ToolSettings { mcp: &servers, ..Default::default() };
        let (var, text) = config_env(&[PathBuf::from("/data/files")], &tools, "build").unwrap();
        assert_eq!(var, "OPENCODE_CONFIG_CONTENT");
        let config: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(config["permission"]["external_directory"]["/data/files/*"], "allow");
        assert_eq!(config["mcp"]["local"]["command"], json!(["npx", "-y", "pkg"]));
        assert_eq!(config["mcp"]["local"]["environment"]["K"], "v");
        assert_eq!(config["mcp"]["remote"]["type"], "remote");
        assert_eq!(config["mcp"]["remote"]["enabled"], false);
        assert!(config_env(&[], &ToolSettings::default(), "build").is_none());
    }

    #[test]
    fn a_space_after_the_slash_still_names_the_command() {
        let input = TurnInput {
            cwd: Path::new("/"),
            prompt: "/ review the diff",
            session: None,
            files: &[],
            model: None,
            effort: None,
            mode: Mode::Auto,
            tools: crate::engine::harness::ToolSettings::default(),
            project: None,
        };
        let args = OpenCode.plan(&input).args;
        let at = args.iter().position(|a| a == "--command").expect("a command flag");
        assert_eq!(args[at + 1], "review");
    }

    #[test]
    fn step_limit_goes_to_the_turn_agent_only() {
        assert_eq!(turn_agent(Mode::Auto), "build");
        assert_eq!(turn_agent(Mode::Plan), "plan");
        assert_eq!(turn_agent(Mode::Ask), "plan");
        let tools = ToolSettings { max_steps: Some(40), ..Default::default() };
        let (_, text) = config_env(&[], &tools, "plan").unwrap();
        let config: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(config["agent"]["plan"]["steps"], 40);
        assert!(config["agent"].get("build").is_none());
    }

    #[test]
    fn off_tools_and_disabled_skills_are_denied() {
        use crate::engine::skills::Skill;
        let skills = [Skill {
            id: "/s/review".into(),
            name: "review".into(),
            description: String::new(),
            command: "/review".into(),
            source: "Personal".into(),
            path: PathBuf::from("/s/review"),
        }];
        let off = ["/s/review".to_string()];
        let tools = ToolSettings {
            skills: &skills,
            disabled_skills: &off,
            permissions: Permissions { grep: Permission::Off, run: Permission::Off, fetch: Permission::Off, ..Permissions::default() },
            ..Default::default()
        };
        let (_, text) = config_env(&[PathBuf::from("/data/files")], &tools, "build").unwrap();
        let config: Value = serde_json::from_str(&text).unwrap();
        for key in ["grep", "glob", "bash", "webfetch"] {
            assert_eq!(config["permission"][key], "deny", "{key}");
        }
        assert!(config["permission"].get("read").is_none());
        assert_eq!(config["permission"]["skill"]["review"], "deny");
        assert_eq!(config["permission"]["external_directory"]["/data/files/*"], "allow");
    }
}
