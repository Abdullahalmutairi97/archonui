//! Claude Code: `claude -p` with stream-json output. Modes are verified in
//! `docs/HARNESS_MAPPING.md`.

use super::{signed_in, strs, AuthStatus, Harness, Mode, Plan, TurnInput};
use crate::app::settings::{McpServer, Permission, Permissions};
use crate::engine::{
    approve::{self, Route},
    attach::{Attachment, Delivery, Kind, CLAUDE_PDF_BYTES},
    discover::{list, probe_jsonl, strings, Capabilities, ModelInfo},
    mcp,
    parse::Protocol,
};
use base64::{engine::general_purpose::STANDARD, write::EncoderWriter};
use serde_json::{json, Value};
use std::{
    fs::File,
    io::{self, Write},
    path::{Path, PathBuf},
    process::Command,
};

pub struct Claude;

impl Harness for Claude {
    fn id(&self) -> &'static str {
        "claude"
    }

    fn name(&self) -> &'static str {
        "Claude Code"
    }

    /// Line icon: asterisk.
    fn glyph(&self) -> Option<&'static str> {
        Some("M8 1.5v13M1.5 8h13M3.4 3.4l9.2 9.2M12.6 3.4l-9.2 9.2")
    }
    fn binary(&self) -> &'static str {
        "claude"
    }

    fn protocol(&self) -> Protocol {
        Protocol::Claude
    }

    fn modes(&self) -> &'static [Mode] {
        &Mode::ALL
    }

    /// Claude Code takes images, PDFs and text as content blocks of a
    /// stream-json message on stdin; other files go by path with `--add-dir`.
    fn plan(&self, input: &TurnInput) -> Plan {
        let mut pdf_budget = CLAUDE_PDF_BYTES;
        let files = Delivery::split(input.files, |a| match a.kind {
            Kind::Image => true,
            Kind::Pdf if a.size <= pdf_budget => {
                pdf_budget -= a.size;
                true
            }
            _ => super::small_text(a),
        });
        let prompt = files.prompt(input.prompt);
        let mcp = match prompt_route(input) {
            Some(route) => approval_mcp(input.tools.mcp, route),
            None => self.mcp_args(input.tools.mcp),
        };
        let mut a = strs(&["-p", "--output-format", "stream-json", "--verbose", "--include-partial-messages"]);
        a.extend(tool_flags(input));
        a.extend(mcp.args);
        if let Some(model) = input.model {
            a.extend(strs(&["--model", model]));
        }
        if let Some(effort) = input.effort {
            a.extend(strs(&["--effort", effort]));
        }
        if let Some(steps) = input.tools.max_steps {
            a.extend(["--max-turns".into(), steps.to_string()]);
        }
        // One `--settings` at most, so every settings object merges into `settings_json`.
        if let Some(settings) = settings_json(input) {
            a.extend(["--settings".into(), settings.to_string()]);
        }
        if let Some(id) = input.session {
            a.extend(strs(&["--resume", id]));
        }
        for dir in files.dirs() {
            a.extend(["--add-dir".into(), dir.display().to_string()]);
        }
        if files.native.is_empty() {
            a.push(super::prompt_arg(&prompt));
            return Plan { args: a, files: mcp.files, ..Default::default() };
        }
        a.extend(strs(&["--input-format", "stream-json"]));
        let native: Vec<Attachment> = files.native.into_iter().cloned().collect();
        Plan {
            args: a,
            stdin: Some(Box::new(move |w| write_claude_message(w, &native, &prompt))),
            env: Vec::new(),
            files: mcp.files,
        }
    }

    /// `--mcp-config` with the enabled servers in a file. Claude still reads its own
    /// MCP config, so a server imported from there stays on when it is switched off here.
    fn mcp_args(&self, servers: &[McpServer]) -> Plan {
        match mcp::config_file("claude.json", servers, true) {
            Some((path, text)) => {
                Plan { args: strs(&["--mcp-config", &path.display().to_string()]), files: vec![(path, text)], ..Default::default() }
            }
            None => Plan::default(),
        }
    }

    fn discover(&self, path: &Path, cwd: &Path) -> Capabilities {
        let mut cmd = Command::new(path);
        cmd.args(["-p", "--input-format", "stream-json", "--output-format", "stream-json"])
            .args(["--verbose", "--no-session-persistence"]);
        // The Agent SDK's `initialize` request returns commands and models.
        let request = r#"{"type":"control_request","request_id":"archonui-init","request":{"subtype":"initialize"}}"#;
        let Some([v]) = probe_jsonl(cmd, cwd, [request], [|v: &Value| v["type"] == "control_response"]) else {
            return Capabilities::default();
        };
        let r = &v["response"]["response"];
        let models = r["models"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|m| {
                let id = m["value"].as_str()?;
                // "default" means "no --model flag", which the picker already offers.
                (id != "default").then(|| ModelInfo {
                    id: id.into(),
                    label: m["displayName"].as_str().unwrap_or(id).into(),
                    description: crate::engine::discover::short(m["description"].as_str().unwrap_or("")),
                    efforts: strings(&m["supportedEffortLevels"]),
                })
            })
            .collect();
        let default = r["models"].as_array().into_iter().flatten().find(|m| m["value"] == "default");
        Capabilities {
            commands: list(&v, "/response/response/commands", "argumentHint"),
            models,
            // The default's resolved id maps back to its alias (e.g. "opus"), so the picker can label it.
            default_model: default.and_then(|d| d["resolvedModel"].as_str()).map(|resolved| {
                r["models"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .find(|m| m["value"] != "default" && m["resolvedModel"] == resolved)
                    .and_then(|m| m["value"].as_str())
                    .unwrap_or(resolved)
                    .to_string()
            }),
            default_efforts: default.map(|m| strings(&m["supportedEffortLevels"])).unwrap_or_default(),
            // Claude Code does not report its default effort, so use the one set in its settings.
            default_effort: settings_effort(),
        }
    }

    fn auth_status(&self) -> AuthStatus {
        match signed_in(&["ANTHROPIC_API_KEY"], &[".claude/.credentials.json"]) {
            // The macOS login lives in the keychain, which this check cannot see.
            AuthStatus::Missing if cfg!(target_os = "macos") => AuthStatus::Unknown,
            status => status,
        }
    }
}

/// The `effortLevel` in `~/.claude/settings.json`, which Claude Code uses when no `--effort` is passed.
fn settings_effort() -> Option<String> {
    let home = std::env::var_os("HOME")?;
    let text = std::fs::read_to_string(Path::new(&home).join(".claude/settings.json")).ok()?;
    effort_from_settings(&text)
}

/// The `effortLevel` in Claude Code's settings JSON, when it names a level as a string.
fn effort_from_settings(text: &str) -> Option<String> {
    let settings: Value = serde_json::from_str(text).ok()?;
    settings["effortLevel"].as_str().map(String::from)
}

/// The approval route when this turn's Ask permissions need ArchonUI to prompt: Auto, a route from the app,
/// and at least one permission on Ask. Plan and Ask already refuse the tools; a headless run has no one to ask.
fn prompt_route<'a>(input: &TurnInput<'a>) -> Option<Route<'a>> {
    let p = input.tools.permissions;
    let asks = [p.read, p.edit, p.grep, p.run, p.browser, p.web, p.fetch].contains(&Permission::Ask);
    (input.mode == Mode::Auto && asks).then_some(input.tools.approval).flatten()
}

/// `--mcp-config` with the enabled servers and the `archonui` approval server that `--permission-prompt-tool`
/// names. The file is named for the chat: the server's arguments name the chat, so chats must not share it.
/// The run's token rides in the server's environment, out of argv, and the app refuses a call without it.
fn approval_mcp(servers: &[McpServer], route: Route) -> Plan {
    let server = json!({
        "type": "stdio",
        "command": command_path(&std::env::current_exe().unwrap_or_else(|_| PathBuf::from("archonui"))),
        "args": ["--approval-mcp", route.socket.display().to_string(), route.session],
        "env": {approve::TOKEN_ENV: approve::token()},
    });
    let name = format!("claude-{}.json", route.session);
    let (path, text) = mcp::config_file_with(&name, servers, true, vec![(approve::SERVER.into(), server)]);
    Plan { args: strs(&["--mcp-config", &path.display().to_string()]), files: vec![(path, text)], ..Default::default() }
}

/// The path to hand a child that runs this same program. `/proc/self/exe` gains a " (deleted)"
/// suffix once the file is replaced (rebuilding or updating while ArchonUI runs) and no shell can
/// execute that; the stripped path is the replacement, which is what a child should run.
fn command_path(path: &Path) -> String {
    let path = path.to_string_lossy();
    path.strip_suffix(" (deleted)").unwrap_or(&path).to_string()
}

/// `--permission-mode` per mode, verified live. `auto` was tried and stalls
/// in `-p` mode, so Auto uses `acceptEdits`. `dontAsk` refuses anything not
/// already allowed; Ask also disallows the tools that change files or run
/// shell commands, so a user's allow rule for Bash cannot write. Tools switched
/// off in the settings and disabled skills join the same `--disallowedTools`
/// list. The `=` form matters: `--disallowedTools` takes several values and
/// would swallow the prompt.
///
/// Auto with an Ask permission uses `default`, because `acceptEdits` would approve edits before the prompt
/// tool was asked. Allow tools go in `--allowedTools` so they run without a prompt; Ask tools are in neither
/// list, so they reach the prompt tool.
fn tool_flags(input: &TurnInput) -> Vec<String> {
    let p = input.tools.permissions;
    let prompting = prompt_route(input).is_some();
    let mut flags = match (input.mode, prompting) {
        (Mode::Auto, true) => strs(&["--permission-mode", "default"]),
        (Mode::Auto, false) => strs(&["--permission-mode", "acceptEdits"]),
        (Mode::Plan, _) => strs(&["--permission-mode", "plan"]),
        (Mode::Ask, _) => strs(&["--permission-mode", "dontAsk"]),
    };
    if input.mode == Mode::Auto {
        let allowed = tools_at(p, Permission::Allow);
        if !allowed.is_empty() {
            flags.push(format!("--allowedTools={}", allowed.join(",")));
        }
    }
    if prompting {
        flags.push("--permission-prompt-tool".into());
        flags.push(format!("mcp__{}__{}", approve::SERVER, approve::TOOL));
    }
    let mut denied: Vec<String> = match input.mode {
        Mode::Ask => strs(&["Edit", "Write", "NotebookEdit", "Bash"]),
        Mode::Auto | Mode::Plan => Vec::new(),
    };
    let skills = input.tools.skills.iter().filter(|s| input.tools.disabled_skills.contains(&s.id));
    denied.extend(tools_at(p, Permission::Off).into_iter().map(String::from));
    denied.extend(skills.map(|s| format!("Skill({})", s.name)));
    let mut unique: Vec<String> = Vec::new();
    for tool in denied {
        if !unique.contains(&tool) {
            unique.push(tool);
        }
    }
    if !unique.is_empty() {
        flags.push(format!("--disallowedTools={}", unique.join(",")));
    }
    flags
}

/// The `--settings` object for the turn, or `None` when there is nothing to add. Only Auto
/// gets the sandbox: Plan's read-only promise is not tested with auto-allowed shell commands.
fn settings_json(input: &TurnInput) -> Option<Value> {
    if !input.tools.sandbox || input.mode != Mode::Auto {
        return None;
    }
    let mut sandbox = json!({"enabled": true, "autoAllowBashIfSandboxed": true});
    if !input.tools.ask_network {
        sandbox["network"] = json!({"allowedDomains": ["*"]});
    }
    Some(json!({"sandbox": sandbox}))
}

/// Claude tools for each permission set to `level`. Browser has no built-in Claude tool, so it is not mapped.
fn tools_at(p: Permissions, level: Permission) -> Vec<&'static str> {
    let at = |perm: Permission| perm == level;
    let mut tools = Vec::new();
    if at(p.read) {
        tools.push("Read");
    }
    if at(p.grep) {
        tools.extend(["Grep", "Glob", "LS"]);
    }
    if at(p.edit) {
        tools.extend(["Edit", "Write", "MultiEdit", "NotebookEdit"]);
    }
    if at(p.run) {
        tools.push("Bash");
    }
    if at(p.web) {
        tools.push("WebSearch");
    }
    if at(p.fetch) {
        tools.push("WebFetch");
    }
    tools
}

/// One stream-json user message, base64-encoding files straight from disk.
fn write_claude_message(w: &mut dyn Write, files: &[Attachment], prompt: &str) -> io::Result<()> {
    w.write_all(br#"{"type":"user","message":{"role":"user","content":["#)?;
    for f in files {
        if f.kind == Kind::Text {
            let block = json!({
                "type": "document",
                "title": f.name,
                "source": {"type": "text", "media_type": "text/plain", "data": std::fs::read_to_string(&f.path)?},
            });
            serde_json::to_writer(&mut *w, &block)?;
        } else {
            let kind = if f.kind == Kind::Pdf { "document" } else { "image" };
            write!(
                w,
                r#"{{"type":"{kind}","source":{{"type":"base64","media_type":"{}","data":""#,
                f.media_type()
            )?;
            {
                let mut enc = EncoderWriter::new(&mut *w, &STANDARD);
                io::copy(&mut File::open(&f.path)?, &mut enc)?;
                enc.finish()?;
            }
            w.write_all(b"\"}}")?;
        }
        w.write_all(b",")?;
    }
    serde_json::to_writer(&mut *w, &json!({"type": "text", "text": prompt}))?;
    w.write_all(b"]}}\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::settings::McpTransport;
    use crate::engine::harness;
    use std::path::Path;

    /// Every category at `level`.
    fn all(level: Permission) -> Permissions {
        Permissions {
            read: level,
            edit: level,
            grep: level,
            run: level,
            browser: level,
            web: level,
            fetch: level,
        }
    }

    fn flags(mode: Mode, permissions: Permissions, route: Option<Route>) -> Vec<String> {
        let input = TurnInput {
            cwd: Path::new("."),
            prompt: "",
            session: None,
            files: &[],
            model: None,
            effort: None,
            mode,
            tools: harness::ToolSettings { permissions, approval: route, ..Default::default() },
        };
        tool_flags(&input)
    }

    fn has(flags: &[String], flag: &str) -> bool {
        flags.iter().any(|f| f == flag)
    }

    fn value_of<'a>(flags: &'a [String], prefix: &str) -> Option<&'a str> {
        flags.iter().find_map(|f| f.strip_prefix(prefix))
    }

    fn route() -> Route<'static> {
        Route { socket: Path::new("/run/user/1000/archonui-1.sock"), session: "chat-a" }
    }

    const PROMPT_TOOL: &str = "mcp__archonui__approve";

    #[test]
    fn claude_default_effort_comes_from_its_settings() {
        assert_eq!(effort_from_settings(r#"{"effortLevel":"high"}"#), Some("high".into()));
        assert_eq!(effort_from_settings(r#"{"effortLevel":3}"#), None);
        assert_eq!(effort_from_settings("{}"), None);
        assert_eq!(effort_from_settings("not json"), None);
    }

    #[test]
    fn auto_without_ask_keeps_accept_edits_and_allows_the_allowed_tools() {
        let f = flags(Mode::Auto, all(Permission::Allow), Some(route()));
        assert!(has(&f, "--permission-mode") && has(&f, "acceptEdits"));
        assert!(!has(&f, "--permission-prompt-tool"));
        assert_eq!(
            value_of(&f, "--allowedTools="),
            Some("Read,Grep,Glob,LS,Edit,Write,MultiEdit,NotebookEdit,Bash,WebSearch,WebFetch")
        );
        assert_eq!(value_of(&f, "--disallowedTools="), None);
    }

    #[test]
    fn auto_with_ask_prompts_and_keeps_ask_tools_out_of_both_lists() {
        // Default permissions: edit and run are Ask, browser has no Claude tool.
        let f = flags(Mode::Auto, Permissions::default(), Some(route()));
        assert!(has(&f, "default"), "acceptEdits would approve edits before the prompt");
        assert!(!has(&f, "acceptEdits"));
        assert_eq!(value_of(&f, "--allowedTools="), Some("Read,Grep,Glob,LS,WebSearch,WebFetch"));
        let allowed = value_of(&f, "--allowedTools=").unwrap();
        assert!(!allowed.contains("Bash") && !allowed.contains("Edit"));
        assert_eq!(value_of(&f, "--disallowedTools="), None);
        let i = f.iter().position(|x| x == "--permission-prompt-tool").unwrap();
        assert_eq!(f[i + 1], PROMPT_TOOL);
    }

    #[test]
    fn ask_run_is_neither_allowed_nor_disallowed() {
        let p = Permissions { run: Permission::Ask, ..all(Permission::Allow) };
        let f = flags(Mode::Auto, p, Some(route()));
        let allowed = value_of(&f, "--allowedTools=").unwrap();
        assert!(!allowed.split(',').any(|t| t == "Bash"));
        assert!(has(&f, PROMPT_TOOL));
    }

    #[test]
    fn off_tools_are_disallowed_even_while_prompting() {
        let p = Permissions { read: Permission::Off, edit: Permission::Ask, ..all(Permission::Allow) };
        let f = flags(Mode::Auto, p, Some(route()));
        assert_eq!(value_of(&f, "--disallowedTools="), Some("Read"));
        assert!(!value_of(&f, "--allowedTools=").unwrap().split(',').any(|t| t == "Read"));
        assert!(has(&f, PROMPT_TOOL));
    }

    #[test]
    fn auto_with_ask_but_no_route_falls_back_to_accept_edits() {
        let f = flags(Mode::Auto, Permissions::default(), None);
        assert!(has(&f, "acceptEdits"));
        assert!(!has(&f, "--permission-prompt-tool"));
    }

    #[test]
    fn plan_and_ask_modes_never_prompt() {
        for mode in [Mode::Plan, Mode::Ask] {
            let f = flags(mode, Permissions::default(), Some(route()));
            assert!(!has(&f, "--permission-prompt-tool"), "{mode:?}");
            assert!(value_of(&f, "--allowedTools=").is_none(), "{mode:?}");
        }
        let plan = flags(Mode::Plan, Permissions::default(), Some(route()));
        assert!(has(&plan, "plan"));
        let ask = flags(Mode::Ask, Permissions::default(), Some(route()));
        assert!(has(&ask, "dontAsk"));
        assert_eq!(value_of(&ask, "--disallowedTools="), Some("Edit,Write,NotebookEdit,Bash"));
    }

    #[test]
    fn approval_config_names_the_archonui_server_with_the_chat() {
        let servers = [McpServer {
            id: "x".into(),
            name: "docs".into(),
            transport: McpTransport::Stdio { command: "docs-mcp".into(), args: vec![], env: Default::default() },
            enabled: true,
        }];
        let plan = approval_mcp(&servers, route());
        let (path, text) = plan.files.into_iter().next().unwrap();
        assert!(path.ends_with("mcp/claude-chat-a.json"), "{}", path.display());
        let config: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(config["mcpServers"]["docs"]["command"], "docs-mcp");
        assert_eq!(config["mcpServers"]["archonui"]["type"], "stdio");
        assert_eq!(
            config["mcpServers"]["archonui"]["args"],
            json!(["--approval-mcp", "/run/user/1000/archonui-1.sock", "chat-a"])
        );
        assert_eq!(config["mcpServers"]["archonui"]["env"][approve::TOKEN_ENV], approve::token());
        assert_eq!(plan.args[0], "--mcp-config");
    }

    #[test]
    fn the_approval_command_is_a_runnable_path() {
        assert_eq!(command_path(Path::new("/opt/archonui/archonui")), "/opt/archonui/archonui");
        assert_eq!(command_path(Path::new("/opt/archonui/archonui (deleted)")), "/opt/archonui/archonui");
        // Whatever binary is running, the path handed to the child names a file that still exists.
        let exe = std::env::current_exe().unwrap();
        let command = command_path(&exe);
        assert_eq!(Path::new(&command), exe.as_path(), "a live path is unchanged");
        assert!(Path::new(&command).is_file(), "the child can run {command}");
    }
}
