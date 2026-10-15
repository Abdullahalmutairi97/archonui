//! Claude Code: `claude -p` with stream-json output. Modes are verified in
//! `docs/HARNESS_MAPPING.md`.

use super::{signed_in, strs, AuthStatus, Harness, Mode, Plan, TurnInput};
use crate::app::settings::{McpServer, Permission, Permissions};
use crate::engine::{
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
    path::Path,
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
        let mcp = self.mcp_args(input.tools.mcp);
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
        // `--add-dir` takes several values, so the `=` form keeps it from swallowing the prompt.
        for dir in files.dirs() {
            a.push(format!("--add-dir={}", dir.display()));
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
            Some((path, text)) => Plan { args: vec![mcp_config_flag(&path)], files: vec![(path, text)], ..Default::default() },
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

/// `--mcp-config` takes several values, so a bare one followed by the prompt would read the prompt as
/// another config file. The `=` form takes exactly one (checked against 2.1.300).
fn mcp_config_flag(path: &Path) -> String {
    format!("--mcp-config={}", path.display())
}

/// `--permission-mode` per mode, verified live. Auto is Claude Code's own
/// `auto` mode, as in an interactive session: its safety check approves or
/// blocks each action, so nothing stops to ask (checked against 2.1.300 with
/// `-p`, both argv and stream-json input). Tools on Allow skip the check
/// through `--allowedTools`; tools on Ask are left to it. A permission prompt
/// tool is not passed, because `auto` hands it every call and the turn would
/// ask again. `dontAsk` refuses anything not
/// already allowed; Ask also disallows the tools that change files or run
/// shell commands, so a user's allow rule for Bash cannot write. Tools switched
/// off in the settings and disabled skills join the same `--disallowedTools`
/// list. The `=` form matters: `--disallowedTools` takes several values and
/// would swallow the prompt.
fn tool_flags(input: &TurnInput) -> Vec<String> {
    let p = input.tools.permissions;
    let mut flags = match input.mode {
        Mode::Auto => strs(&["--permission-mode", "auto"]),
        Mode::Plan => strs(&["--permission-mode", "plan"]),
        Mode::Ask => strs(&["--permission-mode", "dontAsk"]),
    };
    if input.mode == Mode::Auto {
        let allowed = tools_at(p, Permission::Allow);
        if !allowed.is_empty() {
            flags.push(format!("--allowedTools={}", allowed.join(",")));
        }
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

    fn flags(mode: Mode, permissions: Permissions) -> Vec<String> {
        let input = TurnInput {
            cwd: Path::new("."),
            prompt: "",
            session: None,
            files: &[],
            model: None,
            effort: None,
            mode,
            tools: harness::ToolSettings { permissions, ..Default::default() },
        };
        tool_flags(&input)
    }

    fn has(flags: &[String], flag: &str) -> bool {
        flags.iter().any(|f| f == flag)
    }

    fn value_of<'a>(flags: &'a [String], prefix: &str) -> Option<&'a str> {
        flags.iter().find_map(|f| f.strip_prefix(prefix))
    }

    #[test]
    fn claude_default_effort_comes_from_its_settings() {
        assert_eq!(effort_from_settings(r#"{"effortLevel":"high"}"#), Some("high".into()));
        assert_eq!(effort_from_settings(r#"{"effortLevel":3}"#), None);
        assert_eq!(effort_from_settings("{}"), None);
        assert_eq!(effort_from_settings("not json"), None);
    }

    #[test]
    fn auto_is_claude_auto_mode_and_never_prompts() {
        for p in [all(Permission::Allow), all(Permission::Ask), Permissions::default()] {
            let f = flags(Mode::Auto, p);
            let i = f.iter().position(|x| x == "--permission-mode").unwrap();
            assert_eq!(f[i + 1], "auto", "{p:?}");
            assert!(!has(&f, "--permission-prompt-tool"), "auto hands every call to a prompt tool");
        }
    }

    #[test]
    fn auto_allows_the_allowed_tools_and_leaves_ask_tools_to_the_check() {
        let f = flags(Mode::Auto, all(Permission::Allow));
        assert_eq!(
            value_of(&f, "--allowedTools="),
            Some("Read,Grep,Glob,LS,Edit,Write,MultiEdit,NotebookEdit,Bash,WebSearch,WebFetch")
        );
        assert_eq!(value_of(&f, "--disallowedTools="), None);
        let p = Permissions { run: Permission::Ask, ..all(Permission::Allow) };
        let f = flags(Mode::Auto, p);
        let allowed = value_of(&f, "--allowedTools=").unwrap();
        assert!(!allowed.split(',').any(|t| t == "Bash"));
        assert_eq!(value_of(&f, "--disallowedTools="), None);
    }

    #[test]
    fn off_tools_stay_disallowed_in_auto() {
        let p = Permissions { read: Permission::Off, edit: Permission::Ask, ..all(Permission::Allow) };
        let f = flags(Mode::Auto, p);
        assert_eq!(value_of(&f, "--disallowedTools="), Some("Read"));
        assert!(!value_of(&f, "--allowedTools=").unwrap().split(',').any(|t| t == "Read"));
    }

    #[test]
    fn plan_and_ask_modes_never_prompt() {
        for mode in [Mode::Plan, Mode::Ask] {
            let f = flags(mode, Permissions::default());
            assert!(!has(&f, "--permission-prompt-tool"), "{mode:?}");
            assert!(value_of(&f, "--allowedTools=").is_none(), "{mode:?}");
        }
        assert!(has(&flags(Mode::Plan, Permissions::default()), "plan"));
        let ask = flags(Mode::Ask, Permissions::default());
        assert!(has(&ask, "dontAsk"));
        assert_eq!(value_of(&ask, "--disallowedTools="), Some("Edit,Write,NotebookEdit,Bash"));
    }

    #[test]
    fn variadic_flags_never_swallow_the_prompt() {
        let files = [Attachment { path: "/data/big.bin".into(), name: "big.bin".into(), kind: Kind::Other, size: 1 }];
        let servers = [McpServer {
            id: "x".into(),
            name: "docs".into(),
            transport: McpTransport::Stdio { command: "docs-mcp".into(), args: vec![], env: Default::default() },
            enabled: true,
        }];
        let input = TurnInput {
            cwd: Path::new("."),
            prompt: "do it",
            session: None,
            files: &files,
            model: None,
            effort: None,
            mode: Mode::Auto,
            tools: harness::ToolSettings { mcp: &servers, ..Default::default() },
        };
        let args = Claude.plan(&input).args;
        assert!(args.last().unwrap().starts_with("do it"), "the prompt is last: {args:?}");
        assert!(!args.iter().any(|a| a == "--add-dir" || a == "--mcp-config"), "{args:?}");
        assert!(args.contains(&"--add-dir=/data".to_string()), "{args:?}");
    }
}
