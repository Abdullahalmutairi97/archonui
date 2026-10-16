//! Gemini CLI: `gemini -p` with stream-json output. Flags and settings keys are checked against
//! Gemini CLI 0.64.0's bundle; see `docs/HARNESS_MAPPING.md`.

use super::{
    planned, prompt_arg, scoped, signed_in, strs, AuthStatus, Harness, Mode, Plan, ToolSettings, ToolSupport, TurnInput,
};
use crate::app::settings::{Permission, Permissions};
use crate::engine::{attach::Delivery, mcp, parse::Protocol};
use serde_json::Value;
use std::{
    collections::hash_map::DefaultHasher,
    env,
    hash::{Hash, Hasher},
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};

pub struct Gemini;

impl Harness for Gemini {
    fn id(&self) -> &'static str {
        "gemini"
    }

    fn name(&self) -> &'static str {
        "Gemini CLI"
    }

    /// Line icon: four-point sparkle.
    fn glyph(&self) -> Option<&'static str> {
        Some("M8 1.5C8.6 5.5 10.5 7.4 14.5 8C10.5 8.6 8.6 10.5 8 14.5C7.4 10.5 5.5 8.6 1.5 8C5.5 7.4 7.4 5.5 8 1.5Z")
    }
    fn binary(&self) -> &'static str {
        "gemini"
    }

    fn protocol(&self) -> Protocol {
        Protocol::Gemini
    }

    fn modes(&self) -> &'static [Mode] {
        &Mode::ALL
    }

    /// Gemini has no image flag, so every attachment goes by path: its
    /// directories are included and the prompt lists the files.
    fn plan(&self, input: &TurnInput) -> Plan {
        let files = Delivery::split(input.files, |_| false);
        let mut a = Vec::new();
        if let Some(model) = input.model {
            a.extend(strs(&["-m", model]));
        }
        let approval = match input.mode {
            Mode::Auto => "auto_edit",
            Mode::Plan | Mode::Ask => "plan",
        };
        a.extend(strs(&["--approval-mode", approval]));
        // Headless Gemini (0.64.0) exits in a folder the user never trusted, and outside a trusted
        // folder it also overrides `--approval-mode` to `default`. ArchonUI chooses the folder, as
        // `claude -p` does, so the turn trusts it.
        a.push("--skip-trust".into());
        for dir in files.dirs() {
            a.extend(["--include-directories".into(), dir.display().to_string()]);
        }
        if let Some(id) = input.session {
            a.extend(strs(&["--resume", id]));
        }
        a.extend(strs(&["--output-format", "stream-json"]));
        let mut policies = Vec::new();
        if let Some((path, text)) = policy_file(&input.tools) {
            // `--policy` replaces the `policyPaths` of Gemini's settings, so those are passed too.
            let paths = settings_policies(input.cwd).into_iter().chain([path.display().to_string()]);
            a.extend(paths.flat_map(|p| ["--policy".to_string(), p]));
            policies.push((path, text));
        }
        // Plan and Ask share `--approval-mode plan`, so Plan also asks for a plan in the prompt.
        a.extend(["-p".into(), prompt_arg(&scoped(input, planned(input.mode, files.prompt(input.prompt))))]);
        Plan { args: a, files: policies, ..Default::default() }
    }

    /// Off denies the matching built-in tools (see `denied_tools`); Gemini has no browser tool. A
    /// disabled skill cannot be activated. ArchonUI's MCP servers do not reach Gemini: `gemini -p`
    /// reads servers only from its settings files, and it skips system settings files that root
    /// does not own. Headless Gemini cannot ask, so Ask is not applied.
    fn tool_support(&self) -> ToolSupport {
        ToolSupport { mcp: false, permissions: [true, true, true, true, false, true, true], ask: false, skill_toggles: true }
    }

    fn auth_status(&self) -> AuthStatus {
        signed_in(&["GEMINI_API_KEY", "GOOGLE_API_KEY"], &[".gemini/oauth_creds.json"])
    }
}


/// A Gemini policy file (TOML) that denies the tools switched off and the activation of each
/// switched-off skill, as (path, text). A user-tier rule at the top priority, so only an admin
/// policy outranks it; Gemini drops a tool denied outright from the tools the model sees. Written
/// 0600 under a name made from its text, so chats with different settings never share one. `None`
/// when nothing is switched off.
fn policy_file(tools: &ToolSettings) -> Option<(PathBuf, String)> {
    let text = policy_text(tools)?;
    let mut hash = DefaultHasher::new();
    text.hash(&mut hash);
    Some((mcp::file_path(&format!("gemini-policy-{:016x}.toml", hash.finish())), text))
}

/// The policy `policy_file` writes, or `None` when it would have no rules.
fn policy_text(tools: &ToolSettings) -> Option<String> {
    let rule = |tool: toml::Value, args: Option<String>| {
        let mut r = toml::Table::new();
        r.insert("toolName".into(), tool);
        if let Some(args) = args {
            r.insert("argsPattern".into(), args.into());
        }
        r.insert("decision".into(), "deny".into());
        r.insert("priority".into(), 999.into());
        r.insert("denyMessage".into(), "Switched off in ArchonUI's settings.".into());
        toml::Value::Table(r)
    };
    let denied = denied_tools(tools.permissions);
    let mut rules = Vec::new();
    if !denied.is_empty() {
        rules.push(rule(denied.into_iter().map(toml::Value::from).collect::<Vec<_>>().into(), None));
    }
    for skill in tools.skills.iter().filter(|s| tools.disabled_skills.contains(&s.id)) {
        rules.push(rule("activate_skill".into(), Some(skill_pattern(&skill.name))));
    }
    if rules.is_empty() {
        return None;
    }
    let mut file = toml::Table::new();
    file.insert("rule".into(), rules.into());
    let body = toml::to_string(&file).ok()?;
    Some(format!("# Written by ArchonUI for one Gemini turn: the tools and skills switched off in its settings.\n{body}"))
}

/// Matches `activate_skill`'s arguments (stringified JSON) when they name the skill `name`.
fn skill_pattern(name: &str) -> String {
    let json = format!("\"name\":{}", Value::from(name));
    json.chars().fold(String::new(), |mut out, c| {
        if "\\.+*?()|[]{}^$/".contains(c) {
            out.push('\\');
        }
        out.push(c);
        out
    })
}

/// The `policyPaths` of Gemini's user, workspace and (when root owns it, as Gemini requires)
/// system settings, which Gemini would otherwise read itself.
fn settings_policies(cwd: &Path) -> Vec<String> {
    let home = env::var_os("GEMINI_CLI_HOME").or_else(|| env::var_os("HOME")).map(PathBuf::from);
    let system = system_settings_path();
    let system = std::fs::metadata(&system).is_ok_and(|m| m.uid() == 0).then_some(system);
    let mut paths: Vec<String> = Vec::new();
    for file in [home.map(|h| h.join(".gemini/settings.json")), Some(cwd.join(".gemini/settings.json")), system] {
        let Some(settings) = file.and_then(|f| mcp::read_jsonc(&f)) else { continue };
        for p in settings["policyPaths"].as_array().into_iter().flatten().filter_map(Value::as_str) {
            if !paths.iter().any(|x| x == p) {
                paths.push(p.to_string());
            }
        }
    }
    paths
}

/// Gemini's system settings file: `GEMINI_CLI_SYSTEM_SETTINGS_PATH` when set, else its path for
/// this platform.
fn system_settings_path() -> PathBuf {
    if let Some(path) = env::var_os("GEMINI_CLI_SYSTEM_SETTINGS_PATH").filter(|p| !p.is_empty()) {
        return path.into();
    }
    PathBuf::from(if cfg!(target_os = "macos") {
        "/Library/Application Support/GeminiCli/settings.json"
    } else {
        "/etc/gemini-cli/settings.json"
    })
}

/// Gemini's built-in tools (names from its 0.64.0 bundle) for each permission set to Off.
fn denied_tools(p: Permissions) -> Vec<&'static str> {
    let off = |perm: Permission| perm == Permission::Off;
    let mut tools = Vec::new();
    if off(p.read) {
        tools.extend(["read_file", "read_many_files"]);
    }
    if off(p.grep) {
        tools.extend(["grep_search", "glob", "list_directory"]);
    }
    if off(p.edit) {
        tools.extend(["replace", "write_file"]);
    }
    if off(p.run) {
        tools.push("run_shell_command");
    }
    if off(p.web) {
        tools.push("google_web_search");
    }
    if off(p.fetch) {
        tools.push("web_fetch");
    }
    tools
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::skills::Skill;
    use std::path::Path;

    fn skill(name: &str) -> Skill {
        Skill {
            id: format!("/s/{name}"),
            name: name.into(),
            description: String::new(),
            command: format!("/{name}"),
            source: "Project".into(),
            path: PathBuf::from(format!("/s/{name}")),
        }
    }

    #[test]
    fn switched_off_tools_and_skills_become_deny_rules() {
        let skills = [skill("review"), skill("a.b")];
        let off = ["/s/review".to_string(), "/s/a.b".to_string()];
        let tools = ToolSettings {
            skills: &skills,
            disabled_skills: &off,
            permissions: Permissions { run: Permission::Off, fetch: Permission::Off, ..Permissions::default() },
            ..Default::default()
        };
        let policy: toml::Table = policy_text(&tools).unwrap().parse().unwrap();
        let rules = policy["rule"].as_array().unwrap();
        assert_eq!(rules[0]["toolName"], toml::Value::from(vec!["run_shell_command", "web_fetch"]));
        assert_eq!(rules[0]["decision"].as_str(), Some("deny"));
        assert_eq!(rules[0]["priority"].as_integer(), Some(999));
        assert_eq!(rules[1]["toolName"].as_str(), Some("activate_skill"));
        assert_eq!(rules[1]["argsPattern"].as_str(), Some(r#""name":"review""#));
        assert_eq!(rules[2]["argsPattern"].as_str(), Some(r#""name":"a\.b""#), "regex characters are escaped");
        assert!(policy_text(&ToolSettings::default()).is_none(), "nothing switched off, no policy");
    }

    #[test]
    fn the_policy_flag_comes_before_the_prompt() {
        let tools = ToolSettings { permissions: Permissions { read: Permission::Off, ..Permissions::default() }, ..Default::default() };
        let input = TurnInput {
            cwd: Path::new("/nonexistent-archonui-cwd"),
            prompt: "do it",
            session: None,
            files: &[],
            model: None,
            effort: None,
            mode: Mode::Auto,
            tools,
            project: None,
        };
        let plan = Gemini.plan(&input);
        let (path, text) = &plan.files[0];
        assert!(text.contains(r#"toolName = ["read_file", "read_many_files"]"#), "{text}");
        let at = plan.args.iter().position(|a| a == &path.display().to_string()).expect("the policy is passed");
        assert_eq!(plan.args[at - 1], "--policy");
        assert_eq!(plan.args[plan.args.len() - 2], "-p");
        assert!(plan.args.last().unwrap().ends_with("do it"));
        assert!(plan.env.is_empty());
    }

    #[test]
    fn mcp_servers_are_not_claimed() {
        assert!(!Gemini.tool_support().mcp);
    }
}
