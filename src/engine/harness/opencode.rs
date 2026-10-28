//! OpenCode: `opencode run --format json`. Auto runs the default `build`
//! agent; Plan and Ask run the read-only `plan` agent.
//!
//! OpenCode 2 rewrote the CLI: `run` lost `--dir`, `--variant` and `--command`, the listings moved
//! to its HTTP API (`opencode api`), credentials moved into its database, and turns run in a shared
//! background service unless `--standalone` is given. Both generations are supported; the binary's
//! `--version` picks one.

use super::{
    planned, prompt_arg, scoped, signed_in, strs, AuthStatus, Harness, Mode, Plan, ToolSettings, ToolSupport, TurnInput,
};
use crate::app::settings::{McpServer, McpTransport, Permission, Permissions};
use crate::engine::{
    attach::{Delivery, Kind},
    discover::{command, list, run_json, run_text, AgentCommand, Capabilities, ModelInfo},
    parse::Protocol,
};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    env,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::Mutex,
};

pub struct OpenCode;

/// Which OpenCode CLI is installed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Gen {
    /// 1.x: `--dir`, `--variant`, `--command`, `models --verbose`, `auth.json`.
    V1,
    /// 2.x and later.
    V2,
}

/// The generation of the OpenCode at `path`, from `--version` ("1.4.3", "opencode v2.1.2").
/// Asked once per binary. A binary that doesn't answer counts as the current one.
fn generation(path: &Path) -> Gen {
    static SEEN: Mutex<Option<HashMap<PathBuf, Gen>>> = Mutex::new(None);
    let mut seen = SEEN.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(&g) = seen.get_or_insert_with(HashMap::new).get(path) {
        return g;
    }
    let out = Command::new(path).arg("--version").stdin(Stdio::null()).stderr(Stdio::null()).output();
    let g = out.ok().map_or(Gen::V2, |o| gen_of(&String::from_utf8_lossy(&o.stdout)));
    seen.get_or_insert_with(HashMap::new).insert(path.to_path_buf(), g);
    g
}

/// The generation named by a `--version` line.
fn gen_of(version: &str) -> Gen {
    let major = version
        .split(|c: char| !c.is_ascii_digit())
        .find(|s| !s.is_empty())
        .and_then(|s| s.parse::<u32>().ok());
    match major {
        Some(0 | 1) => Gen::V1,
        _ => Gen::V2,
    }
}

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

    /// The current CLI's flags; `plan_at` picks by the installed version.
    fn plan(&self, input: &TurnInput) -> Plan {
        plan(input, Gen::V2)
    }

    fn plan_at(&self, path: &Path, input: &TurnInput) -> Plan {
        plan(input, generation(path))
    }

    /// Off denies the matching OpenCode permissions (see `denied_permissions`); it has no browser
    /// tool. A disabled skill is denied by name. `opencode run` cannot ask, so Ask is not applied.
    fn tool_support(&self) -> ToolSupport {
        ToolSupport { mcp: true, permissions: [true, true, true, true, false, true, true], ask: false, skill_toggles: true }
    }

    fn discover(&self, path: &Path, cwd: &Path) -> Capabilities {
        match generation(path) {
            Gen::V1 => discover_v1(path, cwd),
            Gen::V2 => discover_v2(path, cwd),
        }
    }

    /// OpenCode 1 keeps keys in `auth.json`, OpenCode 2 in its database. Its free models need no
    /// sign-in, so finding no credential is `Unknown`, never `Missing`.
    fn auth_status(&self) -> AuthStatus {
        if signed_in(&["OPENCODE_API_KEY"], &[".local/share/opencode/auth.json"]) == AuthStatus::Ok {
            return AuthStatus::Ok;
        }
        let data = env::var_os("XDG_DATA_HOME")
            .filter(|d| !d.is_empty())
            .map(PathBuf::from)
            .or_else(|| env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")));
        match data {
            Some(d) if stored_credential(&d.join("opencode")) => AuthStatus::Ok,
            _ => AuthStatus::Unknown,
        }
    }
}

/// Whether OpenCode 2's database in `dir` holds a credential. Its rows have ids starting `cred_`;
/// the file is scanned for one rather than opened, since ArchonUI has no SQLite. A recently saved
/// one may still be in the write-ahead log.
fn stored_credential(dir: &Path) -> bool {
    const MARK: &[u8] = b"cred_";
    ["opencode.db", "opencode.db-wal"]
        .iter()
        .filter_map(|f| std::fs::read(dir.join(f)).ok())
        .any(|bytes| bytes.windows(MARK.len()).any(|w| w == MARK))
}

/// Arguments, environment and stdin for one turn on OpenCode `gen`.
fn plan(input: &TurnInput, gen: Gen) -> Plan {
    let files = Delivery::split(input.files, |a| a.kind == Kind::Image || super::small_text(a));
    let mut a = strs(&["run", "--format", "json"]);
    match gen {
        // Without `--dir` OpenCode 1 works in the project it finds, not the chat's folder.
        Gen::V1 => a.extend(["--dir".into(), input.cwd.display().to_string()]),
        // OpenCode 2 works in its current folder. `--standalone` runs the turn in a private server
        // that is a child of this process: the shared background service ignores
        // `OPENCODE_CONFIG_CONTENT`, and keeps running a turn after Stop kills the client. `run`
        // cannot answer permission prompts, so `--auto` approves what isn't denied, as OpenCode 1 did.
        Gen::V2 => a.extend(strs(&["--standalone", "--auto"])),
    }
    match (gen, input.model, input.effort) {
        (Gen::V1, model, effort) => {
            if let Some(model) = model {
                a.extend(strs(&["--model", model]));
            }
            if let Some(effort) = effort {
                a.extend(strs(&["--variant", effort]));
            }
        }
        // OpenCode 2 names the variant in the model, `provider/model#variant`. A level without a
        // model has nothing to attach to, so the default model runs at its default level.
        (Gen::V2, Some(model), Some(effort)) => a.extend(["--model".into(), format!("{model}#{effort}")]),
        (Gen::V2, Some(model), None) => a.extend(strs(&["--model", model])),
        (Gen::V2, None, _) => {}
    }
    // `plan` denies edits; `build` is the default agent.
    if input.mode != Mode::Auto {
        a.extend(strs(&["--agent", "plan"]));
    }
    if let Some(id) = input.session {
        a.extend(strs(&["--session", id]));
    }
    // A bare "/" names no command, so it goes as a message.
    // Plan and Ask share the `plan` agent, so Plan also asks for a plan in the message.
    let command = input.prompt.strip_prefix('/').filter(|rest| !rest.trim().is_empty());
    let message = match (command, gen) {
        // OpenCode 1 runs slash commands through a flag rather than the message.
        (Some(rest), Gen::V1) => {
            let rest = rest.trim_start();
            let (name, args) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
            a.extend(strs(&["--command", name]));
            (!args.trim().is_empty() || !files.by_path.is_empty()).then(|| files.prompt(args.trim()))
        }
        // OpenCode 2 reads a slash command from the message itself, so nothing goes in front.
        (Some(rest), Gen::V2) => Some(files.prompt(&format!("/{}", rest.trim_start()))),
        (None, _) => Some(scoped(input, planned(input.mode, files.prompt(input.prompt)))),
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
    let env = config_env(&files.dirs(), &input.tools, turn_agent(input.mode), gen).into_iter().collect();
    Plan { args: a, stdin: None, env, files: Vec::new() }
}

/// OpenCode 1. Commands come from `run --command`: two built-ins, config, and skills.
/// `opencode models --verbose` lists connected providers' models.
fn discover_v1(path: &Path, cwd: &Path) -> Capabilities {
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
    capabilities(commands, models, default_model)
}

/// OpenCode 2 answers from its HTTP API. `--standalone` keeps a listing from starting the shared
/// background service, and reads this folder's project config.
fn discover_v2(path: &Path, cwd: &Path) -> Capabilities {
    let api = |route: &str| run_json(path, cwd, &["api", "--standalone", "GET", route]);
    // A fresh server can answer before it has loaded its commands, so the two built-ins are
    // always offered and the listing adds the rest.
    let mut commands = vec![
        command("init", "guided AGENTS.md setup", ""),
        command("review", "review changes, defaults to uncommitted", "[commit|branch|pr]"),
    ];
    for c in api("/api/command").map(|v| list(&v, "/data", "hint")).unwrap_or_default() {
        if !commands.iter().any(|k| k.name == c.name) {
            commands.push(c);
        }
    }
    let models = api("/api/model").map(|v| api_models(&v)).unwrap_or_default();
    let default_model = api("/api/model/default").and_then(|v| api_model_id(&v["data"]));
    capabilities(commands, models, default_model)
}

fn capabilities(commands: Vec<AgentCommand>, models: Vec<ModelInfo>, default_model: Option<String>) -> Capabilities {
    let default_efforts = models
        .iter()
        .find(|m| Some(&m.id) == default_model.as_ref())
        .map(|m| m.efforts.clone())
        .unwrap_or_default();
    Capabilities { commands, models, default_model, default_efforts, default_effort: None }
}

/// `provider/model` for one model of OpenCode 2's `/api/model`.
fn api_model_id(m: &Value) -> Option<String> {
    Some(format!("{}/{}", m["providerID"].as_str()?, m["id"].as_str()?))
}

/// The enabled models in OpenCode 2's `/api/model`; their `variants` are the effort levels.
fn api_models(v: &Value) -> Vec<ModelInfo> {
    let Some(items) = v["data"].as_array() else { return Vec::new() };
    items
        .iter()
        .filter(|m| m["enabled"].as_bool() != Some(false))
        .filter_map(|m| {
            let id = api_model_id(m)?;
            Some(ModelInfo {
                label: m["name"].as_str().unwrap_or(&id).into(),
                description: String::new(), // the picker already shows the id
                efforts: m["variants"]
                    .as_array()
                    .map(|vs| vs.iter().filter_map(|x| x["id"].as_str().map(String::from)).collect())
                    .unwrap_or_default(),
                id,
            })
        })
        .collect()
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
fn config_env(dirs: &[PathBuf], tools: &ToolSettings, agent: &str, gen: Gen) -> Option<(String, String)> {
    const VAR: &str = "OPENCODE_CONFIG_CONTENT";
    let (servers, steps) = (tools.mcp, tools.max_steps);
    let denied = denied_permissions(tools.permissions, gen);
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
/// covers writes and patches too. OpenCode 2 renamed `bash` to `shell`.
fn denied_permissions(p: Permissions, gen: Gen) -> Vec<&'static str> {
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
        keys.push(match gen {
            Gen::V1 => "bash",
            Gen::V2 => "shell",
        });
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
        let (var, text) = config_env(&[PathBuf::from("/data/files")], &tools, "build", Gen::V2).unwrap();
        assert_eq!(var, "OPENCODE_CONFIG_CONTENT");
        let config: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(config["permission"]["external_directory"]["/data/files/*"], "allow");
        assert_eq!(config["mcp"]["local"]["command"], json!(["npx", "-y", "pkg"]));
        assert_eq!(config["mcp"]["local"]["environment"]["K"], "v");
        assert_eq!(config["mcp"]["remote"]["type"], "remote");
        assert_eq!(config["mcp"]["remote"]["enabled"], false);
        assert!(config_env(&[], &ToolSettings::default(), "build", Gen::V2).is_none());
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
        let args = plan(&input, Gen::V1).args;
        let at = args.iter().position(|a| a == "--command").expect("a command flag");
        assert_eq!(args[at + 1], "review");
        assert_eq!(OpenCode.plan(&input).args.last().map(String::as_str), Some("/review the diff"));
    }

    fn turn<'a>(prompt: &'a str, model: Option<&'a str>, effort: Option<&'a str>, session: Option<&'a str>) -> TurnInput<'a> {
        TurnInput {
            cwd: Path::new("/work"),
            prompt,
            session,
            files: &[],
            model,
            effort,
            mode: Mode::Auto,
            tools: ToolSettings::default(),
            project: None,
        }
    }

    fn has_pair(args: &[String], flag: &str, value: &str) -> bool {
        args.windows(2).any(|w| w[0] == flag && w[1] == value)
    }

    #[test]
    fn version_lines_pick_the_generation() {
        assert_eq!(gen_of("1.14.3\n"), Gen::V1);
        assert_eq!(gen_of("0.9.1"), Gen::V1);
        assert_eq!(gen_of("opencode v2.1.2\n"), Gen::V2);
        assert_eq!(gen_of("3.0.0"), Gen::V2);
        assert_eq!(gen_of(""), Gen::V2, "an unreadable version counts as the current CLI");
    }

    /// OpenCode 2.1.2 rejects `--dir` ("Unrecognized flag"), so every turn failed before it began.
    #[test]
    fn v2_turns_use_only_flags_opencode_2_accepts() {
        let a = plan(&turn("hi", Some("opencode-go/glm-5.3"), Some("high"), Some("ses_1")), Gen::V2).args;
        for gone in ["--dir", "--variant", "--command"] {
            assert!(!a.contains(&gone.to_string()), "{gone} in {a:?}");
        }
        assert!(a.contains(&"--standalone".to_string()), "a private server, so Stop stops it and config applies");
        assert!(a.contains(&"--auto".to_string()));
        assert!(has_pair(&a, "--model", "opencode-go/glm-5.3#high"));
        assert!(has_pair(&a, "--session", "ses_1"));
        let a = plan(&turn("hi", Some("opencode/big-pickle"), None, None), Gen::V2).args;
        assert!(has_pair(&a, "--model", "opencode/big-pickle"));
        let a = plan(&turn("hi", None, Some("high"), None), Gen::V2).args;
        assert!(!a.iter().any(|x| x.contains("high") || x == "--model"), "{a:?}");
    }

    #[test]
    fn v1_turns_keep_the_old_flags() {
        let a = plan(&turn("hi", Some("anthropic/x"), Some("high"), None), Gen::V1).args;
        assert!(has_pair(&a, "--dir", "/work"));
        assert!(has_pair(&a, "--model", "anthropic/x"));
        assert!(has_pair(&a, "--variant", "high"));
        assert!(!a.contains(&"--standalone".to_string()));
    }

    #[test]
    fn run_off_denies_the_shell_under_its_name_for_each_generation() {
        let tools = ToolSettings { permissions: Permissions { run: Permission::Off, ..Permissions::default() }, ..Default::default() };
        let config = |gen| -> Value { serde_json::from_str(&config_env(&[], &tools, "build", gen).unwrap().1).unwrap() };
        assert_eq!(config(Gen::V1)["permission"]["bash"], "deny");
        assert_eq!(config(Gen::V2)["permission"]["shell"], "deny");
        assert!(config(Gen::V2)["permission"].get("bash").is_none());
    }

    /// Sanitized from `opencode api GET /api/model` and `/api/model/default` on OpenCode 2.1.2.
    #[test]
    fn v2_model_listing() {
        let v: Value = serde_json::from_str(include_str!("../fixtures/opencode2-api-model.json")).unwrap();
        let models = api_models(&v);
        assert_eq!(models.len(), 3, "the disabled model is left out");
        assert_eq!(models[0].id, "opencode-go/kimi-k3.1-pro");
        assert_eq!(models[0].label, "Kimi K3.1 Pro");
        assert_eq!(models[0].efforts, ["low", "high", "max"]);
        assert!(models[1].efforts.is_empty());
        let default = serde_json::json!({"data": {"id": "big-pickle", "providerID": "opencode"}});
        assert_eq!(api_model_id(&default["data"]).as_deref(), Some("opencode/big-pickle"));
        let commands = list(&serde_json::json!({"data": [{"name": "init", "description": "guided AGENTS.md setup"}]}), "/data", "hint");
        assert_eq!(commands[0].name, "init");
    }

    #[test]
    fn a_v2_credential_counts_as_signed_in() {
        let dir = std::env::temp_dir().join(format!("archonui-oc-auth-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(!stored_credential(&dir), "no database");
        std::fs::write(dir.join("opencode.db"), b"SQLite format 3\0 no rows").unwrap();
        assert!(!stored_credential(&dir));
        std::fs::write(dir.join("opencode.db-wal"), b"....cred_15f1052520 opencode-go OpenCode Go").unwrap();
        assert!(stored_credential(&dir), "a credential still in the write-ahead log");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn step_limit_goes_to_the_turn_agent_only() {
        assert_eq!(turn_agent(Mode::Auto), "build");
        assert_eq!(turn_agent(Mode::Plan), "plan");
        assert_eq!(turn_agent(Mode::Ask), "plan");
        let tools = ToolSettings { max_steps: Some(40), ..Default::default() };
        let (_, text) = config_env(&[], &tools, "plan", Gen::V2).unwrap();
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
            agents: Vec::new(),
        }];
        let off = ["/s/review".to_string()];
        let tools = ToolSettings {
            skills: &skills,
            disabled_skills: &off,
            permissions: Permissions { grep: Permission::Off, run: Permission::Off, fetch: Permission::Off, ..Permissions::default() },
            ..Default::default()
        };
        let (_, text) = config_env(&[PathBuf::from("/data/files")], &tools, "build", Gen::V2).unwrap();
        let config: Value = serde_json::from_str(&text).unwrap();
        for key in ["grep", "glob", "shell", "webfetch"] {
            assert_eq!(config["permission"][key], "deny", "{key}");
        }
        assert!(config["permission"].get("read").is_none());
        assert_eq!(config["permission"]["skill"]["review"], "deny");
        assert_eq!(config["permission"]["external_directory"]["/data/files/*"], "allow");
    }
}
