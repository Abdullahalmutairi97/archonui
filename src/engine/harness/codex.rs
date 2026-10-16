//! Codex CLI: `codex exec --json`. Plan and Ask pin the sandbox per turn with
//! `-c sandbox_mode`, which overrides a `danger-full-access` in the user's
//! config and holds on `exec resume` (where `-s` is not accepted). The sandbox
//! setting uses the same flag in Auto, so resumed turns get it too.

use super::{
    planned, prompt_arg, scoped, signed_in, strs, AuthStatus, Harness, Mode, Plan, ToolSettings, ToolSupport, TurnInput,
};
use crate::app::settings::{McpServer, McpTransport, Permission};
use crate::engine::{
    attach::{Delivery, Kind},
    discover::{run_json, short, Capabilities, ModelInfo},
    parse::Protocol,
};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    env,
    path::{Path, PathBuf},
};

pub struct Codex;

impl Harness for Codex {
    fn id(&self) -> &'static str {
        "codex"
    }

    fn name(&self) -> &'static str {
        "Codex CLI"
    }

    /// Line icon: prompt inside a hexagon.
    fn glyph(&self) -> Option<&'static str> {
        Some("M8 1.8l5.4 3.1v6.2L8 14.2l-5.4-3.1V4.9z M6 6.3l2 1.7-2 1.7M8.8 10.2h1.8")
    }
    fn binary(&self) -> &'static str {
        "codex"
    }

    fn protocol(&self) -> Protocol {
        Protocol::Codex
    }

    fn modes(&self) -> &'static [Mode] {
        &Mode::ALL
    }

    /// Codex attaches images with `-i`; it can read anything else itself.
    fn plan(&self, input: &TurnInput) -> Plan {
        let files = Delivery::split(input.files, |a| a.kind == Kind::Image);
        let sandbox = match input.mode {
            Mode::Auto if input.tools.sandbox => Some("workspace-write"),
            // Auto leaves the user's own sandbox setting alone, as before.
            Mode::Auto => None,
            Mode::Plan | Mode::Ask => Some("read-only"),
        };
        let mut a = strs(&["exec"]);
        if input.session.is_some() {
            a.push("resume".into());
        }
        a.extend(strs(&["--json", "--skip-git-repo-check"]));
        if let Some(sandbox) = sandbox {
            a.extend(["-c".into(), format!("sandbox_mode=\"{sandbox}\"")]);
        }
        // Codex cannot ask in exec mode, so network access is allowed only when the setting says so.
        if input.mode == Mode::Auto && input.tools.sandbox && !input.tools.ask_network {
            a.extend(["-c".into(), "sandbox_workspace_write.network_access=true".into()]);
        }
        let mcp = self.mcp_args(input.tools.mcp);
        a.extend(mcp.args);
        a.extend(tool_overrides(&input.tools, config_skills()).into_iter().flat_map(|o| ["-c".to_string(), o]));
        if let Some(model) = input.model {
            a.extend(strs(&["--model", model]));
        }
        if let Some(effort) = input.effort {
            a.extend(["-c".into(), format!("model_reasoning_effort=\"{effort}\"")]);
        }
        if let Some(id) = input.session {
            a.push(id.into());
        }
        for f in &files.native {
            a.extend(["-i".into(), f.path.display().to_string()]);
        }
        // `-i` takes several values, so end it before the prompt.
        if !files.native.is_empty() {
            a.push("--".into());
        }
        a.push(prompt_arg(&scoped(input, planned(input.mode, files.prompt(input.prompt)))));
        Plan { args: a, env: mcp.env, ..Default::default() }
    }

    /// One `-c` override per setting of each server, so a server switched off here is
    /// off in Codex too. Codex's own `mcp_servers` entries are kept for the rest.
    /// Secret values (stdio `env`, HTTP `headers`) are not put on the command line;
    /// they ride in the child's environment and Codex reads them by name.
    fn mcp_args(&self, servers: &[McpServer]) -> Plan {
        let mut env = BTreeMap::new();
        let args = servers
            .iter()
            .flat_map(|s| server_overrides(s, &mut env))
            .flat_map(|o| ["-c".to_string(), o])
            .collect();
        Plan { args, env: env.into_iter().collect(), ..Default::default() }
    }

    /// Codex's config can only switch off its shell and web search tools. A disabled skill is
    /// switched off by folder. `codex exec` cannot ask, so Ask is not applied.
    fn tool_support(&self) -> ToolSupport {
        ToolSupport { mcp: true, permissions: [false, false, false, true, false, true, false], ask: false, skill_toggles: true }
    }

    /// Models come from the catalog, limited to the ones its own picker lists;
    /// the default comes from config.
    fn discover(&self, path: &Path, cwd: &Path) -> Capabilities {
        let catalog = run_json(path, cwd, &["debug", "models"])
            .and_then(|v| v["models"].as_array().cloned())
            .unwrap_or_default();
        let info = |m: &Value| {
            let id = m["slug"].as_str()?;
            Some(ModelInfo {
                id: id.into(),
                label: m["display_name"].as_str().unwrap_or(id).into(),
                description: short(m["description"].as_str().unwrap_or("")),
                efforts: m["supported_reasoning_levels"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|l| l["effort"].as_str().map(String::from))
                    .collect(),
            })
        };
        let models: Vec<ModelInfo> =
            catalog.iter().filter(|m| m["visibility"] == "list").filter_map(info).collect();
        let default_model = config_value("model");
        // The configured model may be hidden from the picker, so look it up in
        // the whole catalog.
        let default_efforts = catalog
            .iter()
            .filter_map(info)
            .find(|m| Some(&m.id) == default_model.as_ref())
            .map(|m| m.efforts)
            .unwrap_or_default();
        Capabilities {
            commands: Vec::new(),
            models,
            default_model,
            default_efforts,
            default_effort: config_value("model_reasoning_effort"),
        }
    }

    fn auth_status(&self) -> AuthStatus {
        signed_in(&["OPENAI_API_KEY"], &[".codex/auth.json"])
    }
}

/// `-c` overrides (Codex's config reference) for the tools switched off and the disabled skills:
/// `features.shell_tool` for Run, `web_search` for Web search, and `skills.config` entries with
/// `enabled = false`. An override replaces the whole `skills.config` list, so `user_skills` (the
/// user's own entries) are kept in front.
fn tool_overrides(tools: &ToolSettings, user_skills: Vec<toml::Value>) -> Vec<String> {
    let mut out = Vec::new();
    if tools.permissions.run == Permission::Off {
        out.push("features.shell_tool=false".to_string());
    }
    if tools.permissions.web == Permission::Off {
        out.push("web_search=\"disabled\"".to_string());
    }
    let disabled: Vec<toml::Value> = tools
        .skills
        .iter()
        .filter(|s| tools.disabled_skills.contains(&s.id))
        .map(|skill| {
            let mut entry = toml::Table::new();
            entry.insert("path".into(), toml::Value::String(skill.path.display().to_string()));
            entry.insert("enabled".into(), toml::Value::Boolean(false));
            toml::Value::Table(entry)
        })
        .collect();
    if !disabled.is_empty() {
        let entries = user_skills.into_iter().chain(disabled).collect();
        out.push(format!("skills.config={}", toml::Value::Array(entries)));
    }
    out
}

/// The `skills.config` entries in `~/.codex/config.toml`.
fn config_skills() -> Vec<toml::Value> {
    let Some(text) = codex_home().and_then(|home| std::fs::read_to_string(home.join("config.toml")).ok()) else {
        return Vec::new();
    };
    skills_in(&text)
}

/// The `skills.config` entries of TOML `text`.
fn skills_in(text: &str) -> Vec<toml::Value> {
    let Ok(table) = text.parse::<toml::Table>() else { return Vec::new() };
    table
        .get("skills")
        .and_then(|s| s.get("config"))
        .and_then(toml::Value::as_array)
        .cloned()
        .unwrap_or_default()
}

/// The `mcp_servers.<name>` settings for `s`, as `key = value` TOML. Secret
/// values (`env`, `http_headers`) are moved into `env` — the child's
/// environment, which only its owner can read — instead of the command line.
fn server_overrides(s: &McpServer, env: &mut BTreeMap<String, String>) -> Vec<String> {
    let base = format!("mcp_servers.{}", toml_string(&s.name));
    let mut out = vec![format!("{base}.enabled={}", s.enabled)];
    match &s.transport {
        McpTransport::Stdio { command, args, env: server_env } => {
            out.push(format!("{base}.command={}", toml_string(command)));
            if !args.is_empty() {
                out.push(format!("{base}.args={}", toml_array(args)));
            }
            if !server_env.is_empty() {
                let names = forward_env(&s.name, server_env, env);
                if !names.is_empty() {
                    out.push(format!("{base}.env_vars={}", toml_array(&names)));
                }
            }
        }
        McpTransport::Http { url, headers } => {
            out.push(format!("{base}.url={}", toml_string(url)));
            if !headers.is_empty() {
                let map = header_env(&s.name, headers, env);
                out.push(format!("{base}.env_http_headers={}", toml_table(&map)));
            }
        }
    }
    out
}

/// Adds `vars` under their own names to `env`, so Codex's `env_vars` forwards
/// them into the stdio server. A name another server already claimed with a
/// different value is skipped, because forwarding cannot rename it.
fn forward_env(server: &str, vars: &BTreeMap<String, String>, env: &mut BTreeMap<String, String>) -> Vec<String> {
    let mut names = Vec::new();
    for (name, value) in vars {
        if env.get(name).is_some_and(|existing| existing != value) {
            eprintln!("MCP server {server:?}: {name} is already set by another server; skipping it");
            continue;
        }
        env.insert(name.clone(), value.clone());
        names.push(name.clone());
    }
    names
}

/// Header values move into `env` under generated names; Codex reads them back by
/// header name through `env_http_headers`.
fn header_env(server: &str, headers: &BTreeMap<String, String>, env: &mut BTreeMap<String, String>) -> BTreeMap<String, String> {
    headers
        .iter()
        .map(|(header, value)| {
            let name = unique_env_name(header_var(server, header), env);
            env.insert(name.clone(), value.clone());
            (header.clone(), name)
        })
        .collect()
}

/// A valid, upper-case environment variable name for `server`'s `header`.
fn header_var(server: &str, header: &str) -> String {
    let sanitize = |s: &str| -> String {
        s.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_uppercase() } else { '_' }).collect()
    };
    format!("ARCHONUI_MCP_{}_{}", sanitize(server), sanitize(header))
}

/// `base`, with `_2`, `_3`, … appended until it names no variable in `env`.
fn unique_env_name(base: String, env: &BTreeMap<String, String>) -> String {
    if !env.contains_key(&base) {
        return base;
    }
    (2..).map(|n| format!("{base}_{n}")).find(|c| !env.contains_key(c)).expect("a free name exists")
}

/// `s` as a TOML basic string. JSON escapes are valid TOML escapes.
fn toml_string(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_default()
}

fn toml_array(items: &[String]) -> String {
    let quoted: Vec<String> = items.iter().map(|s| toml_string(s)).collect();
    format!("[{}]", quoted.join(", "))
}

fn toml_table(map: &BTreeMap<String, String>) -> String {
    let pairs: Vec<String> = map.iter().map(|(k, v)| format!("{} = {}", toml_string(k), toml_string(v))).collect();
    format!("{{{}}}", pairs.join(", "))
}

/// A top-level `key = "..."` in `~/.codex/config.toml`.
fn config_value(key: &str) -> Option<String> {
    top_level_string(&std::fs::read_to_string(codex_home()?.join("config.toml")).ok()?, key)
}

/// `$CODEX_HOME`, else `~/.codex`.
fn codex_home() -> Option<PathBuf> {
    env::var_os("CODEX_HOME").map(PathBuf::from).or_else(|| env::var_os("HOME").map(|h| Path::new(&h).join(".codex")))
}

/// The string value of top-level `key` in TOML `text`. Parsed as TOML, so a trailing
/// `# comment` or single quotes don't end up in the value.
fn top_level_string(text: &str, key: &str) -> Option<String> {
    let table: toml::Table = text.parse().ok()?;
    table.get(key)?.as_str().map(String::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_values_ignore_comments_quotes_and_tables() {
        let text = "model = \"gpt-5\" # picked in the TUI\nmodel_reasoning_effort = 'high'\n\n[profiles.fast]\nmodel = \"mini\"\n";
        assert_eq!(top_level_string(text, "model").as_deref(), Some("gpt-5"));
        assert_eq!(top_level_string(text, "model_reasoning_effort").as_deref(), Some("high"));
        assert_eq!(top_level_string("[x]\nmodel = \"m\"\n", "model"), None, "keys inside tables are not top-level");
    }

    #[test]
    fn off_tools_and_disabled_skills_become_overrides() {
        use crate::app::settings::Permissions;
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
            permissions: Permissions { run: Permission::Off, web: Permission::Off, read: Permission::Off, ..Permissions::default() },
            ..Default::default()
        };
        let user = skills_in("[[skills.config]]\npath = \"/u/mine\"\nenabled = false\n");
        let o = tool_overrides(&tools, user);
        assert_eq!(o[0], "features.shell_tool=false");
        assert_eq!(o[1], "web_search=\"disabled\"");
        // The user's own entries stay, since the override replaces the list.
        let list: toml::Table = format!("x = {}", o[2].strip_prefix("skills.config=").unwrap()).parse().unwrap();
        let list = list["x"].as_array().unwrap();
        assert_eq!(list[0]["path"].as_str(), Some("/u/mine"));
        assert_eq!((list[1]["path"].as_str(), list[1]["enabled"].as_bool()), (Some("/s/review"), Some(false)));
        assert!(tool_overrides(&ToolSettings::default(), Vec::new()).is_empty());
    }
}
