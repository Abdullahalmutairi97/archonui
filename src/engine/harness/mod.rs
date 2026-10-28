//! Agent CLIs ("harnesses"). Each file here describes one CLI: how to run a
//! turn with it, what it offers, and whether it is signed in. A new harness is
//! one file plus a line in `HARNESSES`.

mod claude;
mod codex;
mod gemini;
mod opencode;
mod pi;

use super::{
    approve::Route,
    attach::{Attachment, Kind, TEXT_INLINE_BYTES},
    discover::Capabilities,
    parse::{Out, Protocol},
    session::{spawn, Event},
    skills::Skill,
};
use crate::app::settings::{McpServer, Permission, Permissions};
use serde::{Deserialize, Serialize};
use std::{
    env,
    io::Write,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

/// How much the agent may do in a turn. Each harness maps these to its own flags.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// Edits and commands run without asking.
    #[default]
    Auto,
    /// Investigate and propose a plan; no edits.
    Plan,
    /// Answer questions; no edits.
    Ask,
}

impl Mode {
    pub const ALL: [Mode; 3] = [Mode::Auto, Mode::Plan, Mode::Ask];

    pub fn name(self) -> &'static str {
        match self {
            Mode::Auto => "Auto",
            Mode::Plan => "Plan",
            Mode::Ask => "Ask",
        }
    }

    /// One line for the palette.
    pub fn blurb(self) -> &'static str {
        match self {
            Mode::Auto => "Edits and commands run without asking",
            Mode::Plan => "Investigate and propose a plan; no edits",
            Mode::Ask => "Answer questions; no edits",
        }
    }

    /// The mode called `s`, ignoring case.
    pub fn from_name(s: &str) -> Option<Mode> {
        Self::ALL.into_iter().find(|m| m.name().eq_ignore_ascii_case(s))
    }
}

/// Whether a CLI is ready to run, from a quick offline check.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuthStatus {
    Ok,
    Missing,
    /// The check cannot tell, e.g. the CLI keeps its login in the macOS keychain.
    Unknown,
}

impl AuthStatus {
    pub fn label(self) -> &'static str {
        match self {
            AuthStatus::Ok => "signed in",
            AuthStatus::Missing => "not signed in",
            AuthStatus::Unknown => "unknown",
        }
    }
}

/// What the app's settings add to a turn. The default adds nothing.
#[derive(Clone, Copy, Debug, Default)]
pub struct ToolSettings<'a> {
    /// ArchonUI's MCP servers; only the enabled ones are passed to harnesses.
    pub mcp: &'a [McpServer],
    /// Every skill `skills::discover` found.
    pub skills: &'a [Skill],
    /// Ids of the skills switched off in settings.
    pub disabled_skills: &'a [String],
    pub permissions: Permissions,
    /// Per-tool permissions for MCP servers, by server name then tool name; unlisted tools are Ask.
    pub mcp_tools: Option<&'a std::collections::BTreeMap<String, std::collections::BTreeMap<String, Permission>>>,
    /// Most agent steps per prompt; `None` leaves the CLI's own limit.
    pub max_steps: Option<u32>,
    /// Auto runs shell commands inside the CLI's sandbox. Plan and Ask keep their own limits.
    pub sandbox: bool,
    /// With `sandbox`: `true` keeps sandboxed commands off the network. `false` allows it.
    /// The CLIs run headless and cannot ask, so the choice is made here.
    pub ask_network: bool,
    /// Where a harness that can ask sends its permission prompts. `None` answers nothing. No harness
    /// asks today: Claude's Auto is its own `auto` mode, which a prompt tool would make ask again.
    #[allow(dead_code)]
    pub approval: Option<Route<'a>>,
}

/// Which tool settings a harness really applies, so the settings page can say which ones do nothing.
/// Not read until the settings page shows it.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ToolSupport {
    /// ArchonUI's enabled MCP servers reach the agent.
    pub mcp: bool,
    /// One per built-in tool row, in the settings page's order: read, edit, grep, run, browser, web,
    /// fetch. `true` when setting the row to Off really removes the tool.
    pub permissions: [bool; 7],
    /// The harness can stop and ask before each use of a tool set to Ask.
    pub ask: bool,
    /// Switching a skill off stops this harness from loading it.
    pub skill_toggles: bool,
}

/// `p` as one level per built-in tool row, in `ToolSupport::permissions` order.
#[allow(dead_code)]
pub fn permission_rows(p: Permissions) -> [Permission; 7] {
    [p.read, p.edit, p.grep, p.run, p.browser, p.web, p.fetch]
}

/// Everything one turn needs from the chat.
pub struct TurnInput<'a> {
    /// Folder the agent runs in.
    pub cwd: &'a Path,
    pub prompt: &'a str,
    /// The agent's own session id; `None` starts a new session.
    pub session: Option<&'a str>,
    pub files: &'a [Attachment],
    pub model: Option<&'a str>,
    pub effort: Option<&'a str>,
    pub mode: Mode,
    pub tools: ToolSettings<'a>,
    /// The folder of the project the chat is filed under, when the chat runs in it. The agent is told
    /// it is in that project (see `scope_note`).
    pub project: Option<&'a Path>,
}

/// Writes the agent's stdin; runs on a background thread.
pub type StdinWriter = Box<dyn FnOnce(&mut dyn Write) -> std::io::Result<()> + Send>;

/// What to run for one turn.
#[derive(Default)]
pub struct Plan {
    pub args: Vec<String>,
    pub stdin: Option<StdinWriter>,
    pub env: Vec<(String, String)>,
    /// Files to write before the CLI starts, as (path, text). Written 0600.
    pub files: Vec<(PathBuf, String)>,
}

pub trait Harness: Send + Sync {
    /// Key stored in each chat, e.g. "claude".
    fn id(&self) -> &'static str;
    /// Display name, e.g. "Claude Code".
    fn name(&self) -> &'static str;
    /// SVG path data for the harness's line icon (16×16 viewBox, stroked). `None` falls back to its
    /// two-letter monogram.
    fn glyph(&self) -> Option<&'static str> {
        None
    }
    /// Executable searched for on `PATH` unless a binary is configured.
    fn binary(&self) -> &'static str;
    fn protocol(&self) -> Protocol;
    /// The modes this CLI enforces. The UI offers only these.
    fn modes(&self) -> &'static [Mode];
    /// The CLI accepts a caller-chosen session id, so one is assigned before
    /// the first turn instead of being read back from its output.
    fn preassign_session(&self) -> bool {
        false
    }
    /// Arguments, environment and stdin for one turn.
    fn plan(&self, input: &TurnInput) -> Plan;
    /// `plan` for the CLI installed at `path`, for a harness whose flags differ between versions.
    fn plan_at(&self, _path: &Path, input: &TurnInput) -> Plan {
        self.plan(input)
    }
    /// Launch wiring for `servers`. Only enabled servers are passed. A harness
    /// without MCP config returns nothing, so its own MCP settings still apply.
    fn mcp_args(&self, _servers: &[McpServer]) -> Plan {
        Plan::default()
    }
    /// Which tool settings `plan` applies. The default applies none.
    #[allow(dead_code)]
    fn tool_support(&self) -> ToolSupport {
        ToolSupport::default()
    }
    /// What the CLI offers, from its own listings. Blocking; missing pieces
    /// come back empty.
    fn discover(&self, _path: &Path, _cwd: &Path) -> Capabilities {
        Capabilities::default()
    }
    /// Offline check for credentials. Never touches the network.
    fn auth_status(&self) -> AuthStatus;
    /// Runs "Reply with exactly: ok" with the defaults and returns the reply,
    /// or why it failed. Gives up after 60 seconds.
    fn test_connection(&self, path: &Path, cwd: &Path) -> Result<String, String> {
        const TIMEOUT: Duration = Duration::from_secs(60);
        let input = TurnInput {
            cwd,
            prompt: "Reply with exactly: ok",
            session: None,
            files: &[],
            model: None,
            effort: None,
            mode: Mode::Ask,
            tools: ToolSettings::default(),
            project: None,
        };
        let turn = spawn(self, path, &input).map_err(|e| e.to_string())?;
        let deadline = Instant::now() + TIMEOUT;
        let mut reply = String::new();
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            // `None` means the timer won: the agent did not finish in time.
            let next = smol::block_on(smol::future::or(
                async { Some(turn.events.recv().await.ok()) },
                async {
                    smol::Timer::after(left).await;
                    None
                },
            ));
            match next {
                None => return Err(format!("no reply within {} s", TIMEOUT.as_secs())),
                Some(None) => return Err("the agent stopped without a reply".into()),
                Some(Some(Event::Out(Out::Text(t)))) => reply.push_str(&t),
                Some(Some(Event::Out(Out::Final(t)))) if reply.is_empty() => reply = t,
                Some(Some(Event::Out(Out::Error(e)))) => return Err(e),
                Some(Some(Event::Out(_))) => {}
                Some(Some(Event::Finished(Ok(())))) => return Ok(reply.trim().to_string()),
                Some(Some(Event::Finished(Err(e)))) => return Err(e),
            }
        }
    }
}

/// Every harness ArchonUI knows. Add a new one here.
pub static HARNESSES: &[&dyn Harness] = &[
    &claude::Claude,
    &codex::Codex,
    &gemini::Gemini,
    &opencode::OpenCode,
    &pi::Pi,
];

pub fn harness(id: &str) -> Option<&'static dyn Harness> {
    HARNESSES.iter().copied().find(|h| h.id() == id)
}

/// Read-only instruction for CLIs with no read-only mode of their own.
const PLAN_PREAMBLE: &str = "Plan only: investigate and propose a plan; do not modify files.";

/// `prompt`, with the plan-only instruction in front when planning.
fn planned(mode: Mode, prompt: String) -> String {
    match mode {
        Mode::Plan => format!("{PLAN_PREAMBLE}\n\n{prompt}"),
        Mode::Auto | Mode::Ask => prompt,
    }
}

/// Tells the agent which project its chat is in, so a request that names no project means this one.
/// Context, not a limit: the user can still point it anywhere.
fn scope_note(project: &Path) -> String {
    format!(
        "This chat is in the project at {}. When the user asks for something without naming a project \
         (\"fix the UI\", \"the app\", \"this project\"), they mean this one.",
        project.display()
    )
}

/// How replies should be laid out. ArchonUI renders Markdown, so structure shows as headings,
/// lists and separate paragraphs instead of one wall of text.
const FORMAT_NOTE: &str = "Your replies are shown in a chat that renders Markdown. Make them easy to scan: \
     short paragraphs of 1-3 sentences separated by a blank line; `##` or `###` headings to split a longer \
     reply into sections; bulleted or numbered lists for steps, options and changes; **bold** for the key \
     term at the start of a list item; tables for comparisons; and fenced code blocks with a language for \
     code. A short answer stays a short paragraph, without headings.\n\n\
     The chat also draws charts and diagrams. When numbers or a flow say it faster than prose, use a fenced \
     `viz` block holding one JSON object: {\"type\":\"bar\"|\"line\"|\"area\",\"title\",\"unit\",\"x\":[labels],\
     \"series\":[{\"name\",\"data\":[numbers]}],\"stacked\":bool}; {\"type\":\"pie\"|\"donut\",\"title\",\
     \"data\":[{\"label\",\"value\"}]}; {\"type\":\"scatter\",\"title\",\"xLabel\",\"yLabel\",\
     \"series\":[{\"name\",\"points\":[[x,y,\"label\"]]}]}; {\"type\":\"table\",\"title\",\"columns\",\
     \"rows\"}; or {\"type\":\"stats\",\"title\",\"items\":[{\"label\",\"value\",\"delta\",\
     \"good\":\"up\"|\"down\",\"trend\":[numbers]}]}. For a flowchart, sequence, gantt, mind map or git \
     graph, use a fenced `mermaid <title>` block with Mermaid source. Keep diagrams compact enough to read in \
     a narrow column: lay a flow of more than four steps out top to bottom (`flowchart TB`).";

/// Everything ArchonUI tells the agent about the chat: the reply format, and the project if any.
fn chat_note(input: &TurnInput) -> String {
    match input.project {
        Some(project) => format!("{FORMAT_NOTE}\n\n{}", scope_note(project)),
        None => FORMAT_NOTE.to_string(),
    }
}

/// `prompt` with the chat note in front, on the first turn of an agent session, for CLIs with no
/// system prompt flag. Later turns resume the session, which already has it.
/// A slash command is sent as typed: text in front of it would stop the CLI reading it as a command.
fn scoped(input: &TurnInput, prompt: String) -> String {
    if input.session.is_some() || prompt.starts_with('/') {
        return prompt;
    }
    format!("{}\n\n{prompt}", chat_note(input))
}

fn strs(xs: &[&str]) -> Vec<String> {
    xs.iter().map(|s| s.to_string()).collect()
}

/// A prompt starting with `-` would be parsed as a flag; a leading space
/// keeps it positional without changing what the model sees in practice.
fn prompt_arg(p: &str) -> String {
    if p.starts_with('-') {
        format!(" {p}")
    } else {
        p.to_string()
    }
}

fn small_text(a: &Attachment) -> bool {
    a.kind == Kind::Text && a.size <= TEXT_INLINE_BYTES
}

/// `Ok` if any of `env_vars` is set or any of `home_files` (relative to the
/// home directory) exists.
fn signed_in(env_vars: &[&str], home_files: &[&str]) -> AuthStatus {
    let set = env_vars.iter().any(|v| env::var_os(v).is_some_and(|x| !x.is_empty()));
    let file = env::var_os("HOME")
        .map(PathBuf::from)
        .is_some_and(|home| home_files.iter().any(|f| home.join(f).is_file()));
    if set || file {
        AuthStatus::Ok
    } else {
        AuthStatus::Missing
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::settings::{McpTransport, Permission};
    use std::collections::BTreeMap;

    fn input(mode: Mode, session: Option<&str>) -> TurnInput<'_> {
        TurnInput {
            cwd: Path::new("/work"),
            prompt: "do it",
            session,
            files: &[],
            model: None,
            effort: None,
            mode,
            tools: ToolSettings::default(),
            project: None,
        }
    }

    fn args(id: &str, mode: Mode, session: Option<&str>) -> Vec<String> {
        harness(id).unwrap().plan(&input(mode, session)).args
    }

    fn has_pair(args: &[String], flag: &str, value: &str) -> bool {
        args.windows(2).any(|w| w[0] == flag && w[1] == value)
    }

    #[test]
    fn mode_names_round_trip() {
        for m in Mode::ALL {
            assert_eq!(Mode::from_name(&m.name().to_lowercase()), Some(m));
        }
        assert_eq!(Mode::from_name("yolo"), None);
        assert_eq!(Mode::default(), Mode::Auto);
    }

    #[test]
    fn the_agent_is_told_its_project() {
        let project = Path::new("/code/replica");
        let note = format!("{FORMAT_NOTE}\n\n{}", scope_note(project));
        let with = |id: &str, mode: Mode, session: Option<&str>| {
            harness(id).unwrap().plan(&TurnInput { project: Some(project), ..input(mode, session) }).args
        };
        // System prompt flags carry it on every turn; Pi's Plan instruction shares the one flag.
        assert!(has_pair(&with("claude", Mode::Auto, Some("s1")), "--append-system-prompt", &note));
        let pi_plan = with("pi", Mode::Plan, Some("s1"));
        assert_eq!(pi_plan.iter().filter(|a| *a == "--append-system-prompt").count(), 1);
        assert!(has_pair(&pi_plan, "--append-system-prompt", &format!("{PLAN_PREAMBLE}\n\n{note}")));
        // The others get it in front of a new session's first prompt only.
        for id in ["codex", "gemini", "opencode"] {
            let first = with(id, Mode::Auto, None);
            assert!(first.iter().any(|a| a.starts_with(&note) && a.ends_with("do it")), "{id}: {first:?}");
            assert!(!with(id, Mode::Auto, Some("s1")).iter().any(|a| a.contains(&note)), "{id} resumed");
        }
        // No project: the format note alone.
        for h in HARNESSES {
            let args = h.plan(&input(Mode::Auto, None)).args;
            assert!(!args.iter().any(|a| a.contains("This chat is in the project")), "{}", h.id());
            assert!(args.iter().any(|a| a.starts_with(FORMAT_NOTE)), "{}: {args:?}", h.id());
        }
    }

    #[test]
    fn a_slash_command_gets_no_note() {
        for id in ["codex", "gemini"] {
            let turn = TurnInput { prompt: "/review", ..input(Mode::Auto, None) };
            let args = harness(id).unwrap().plan(&turn).args;
            assert_eq!(args.last().map(String::as_str), Some("/review"), "{id}");
        }
    }

    #[test]
    fn every_harness_ships_a_glyph_that_parses() {
        for h in HARNESSES {
            let d = h.glyph().unwrap_or_else(|| panic!("{} has no glyph", h.id()));
            crate::ui::composer::icons::parse_path(d).unwrap_or_else(|e| panic!("{}: {e}", h.id()));
        }
    }

    #[test]
    fn every_harness_offers_only_known_modes_with_unique_ids() {
        let mut ids: Vec<&str> = HARNESSES.iter().map(|h| h.id()).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), HARNESSES.len());
        for h in HARNESSES {
            assert!(!h.modes().is_empty(), "{} offers no modes", h.id());
        }
    }

    #[test]
    fn claude_modes() {
        let auto = args("claude", Mode::Auto, None);
        assert!(has_pair(&auto, "--permission-mode", "auto"));
        let plan = args("claude", Mode::Plan, None);
        assert!(has_pair(&plan, "--permission-mode", "plan"));
        let ask = args("claude", Mode::Ask, None);
        assert!(has_pair(&ask, "--permission-mode", "dontAsk"));
        // Bash is disallowed too, so a user's allow rule for it cannot write.
        assert!(ask.contains(&"--disallowedTools=Edit,Write,NotebookEdit,Bash".to_string()));
        assert_eq!(ask.last().map(String::as_str), Some("do it"));
        assert!(has_pair(&args("claude", Mode::Auto, Some("s1")), "--resume", "s1"));
    }

    #[test]
    fn codex_sets_sandbox_on_fresh_and_resumed_turns() {
        let fresh = args("codex", Mode::Ask, None);
        assert!(has_pair(&fresh, "-c", "sandbox_mode=\"read-only\""));
        assert!(!fresh.contains(&"resume".to_string()));
        let resumed = args("codex", Mode::Ask, Some("t1"));
        assert!(resumed.contains(&"resume".to_string()));
        assert!(has_pair(&resumed, "-c", "sandbox_mode=\"read-only\""));
        assert!(!resumed.iter().any(|a| a == "-s"), "`exec resume` does not accept -s");
        let auto = args("codex", Mode::Auto, Some("t1"));
        assert!(!auto.iter().any(|a| a.starts_with("sandbox_mode")), "Auto keeps the user's sandbox");
    }

    #[test]
    fn plan_mode_prepends_the_read_only_instruction() {
        let plan = args("codex", Mode::Plan, None);
        assert!(plan.last().unwrap().contains("\n\nPlan only:"));
        assert!(plan.last().unwrap().ends_with("do it"));
        assert_eq!(args("codex", Mode::Ask, None).last().map(String::as_str), Some(format!("{FORMAT_NOTE}\n\ndo it").as_str()));
    }

    #[test]
    fn opencode_uses_the_plan_agent_outside_auto() {
        // OpenCode 2 runs in the turn's working folder; `--dir` was removed.
        assert!(!args("opencode", Mode::Auto, None).contains(&"--dir".to_string()));
        assert!(!args("opencode", Mode::Auto, None).contains(&"--agent".to_string()));
        assert!(has_pair(&args("opencode", Mode::Ask, None), "--agent", "plan"));
        assert!(has_pair(&args("opencode", Mode::Plan, None), "--agent", "plan"));
    }

    #[test]
    fn plan_preamble_reaches_opencode_and_gemini_as_documented() {
        for id in ["opencode", "gemini"] {
            let plan = args(id, Mode::Plan, None);
            assert!(plan.last().unwrap().starts_with(&format!("{FORMAT_NOTE}\n\n{PLAN_PREAMBLE}")), "{id}: {plan:?}");
            assert!(plan.last().unwrap().ends_with("do it"), "{id}: {plan:?}");
            assert_eq!(args(id, Mode::Ask, None).last().cloned(), Some(format!("{FORMAT_NOTE}\n\ndo it")), "{id}");
        }
    }

    #[test]
    fn opencode_sends_a_bare_slash_as_a_message() {
        let turn = TurnInput { prompt: "/", ..input(Mode::Auto, None) };
        let a = harness("opencode").unwrap().plan(&turn).args;
        assert!(!a.contains(&"--command".to_string()), "{a:?}");
        assert_eq!(a.last().map(String::as_str), Some("/"));
        // OpenCode 2 reads the command from the message, so the message is sent as typed.
        let turn = TurnInput { prompt: "/review main", ..input(Mode::Plan, None) };
        let a = harness("opencode").unwrap().plan(&turn).args;
        assert!(!a.contains(&"--command".to_string()), "{a:?}");
        assert_eq!(a.last().map(String::as_str), Some("/review main"), "a command gets no preamble");
    }

    #[test]
    fn pi_narrows_tools_outside_auto() {
        let auto = args("pi", Mode::Auto, None);
        assert!(!auto.contains(&"--tools".to_string()));
        let ask = args("pi", Mode::Ask, None);
        assert!(has_pair(&ask, "--tools", "read,grep,find,ls"));
        assert!(ask.contains(&"--no-mcp".to_string()));
        assert!(has_pair(&ask, "--append-system-prompt", FORMAT_NOTE), "Ask gets only the format note");
        assert!(args("pi", Mode::Plan, None).contains(&"--append-system-prompt".to_string()));
    }

    #[test]
    fn unverified_harnesses_map_modes_as_documented() {
        assert!(has_pair(&args("gemini", Mode::Auto, None), "--approval-mode", "auto_edit"));
        assert!(has_pair(&args("gemini", Mode::Ask, None), "--approval-mode", "plan"));
        // Without it, headless Gemini refuses untrusted folders and drops the approval mode.
        for mode in Mode::ALL {
            assert!(args("gemini", mode, None).contains(&"--skip-trust".to_string()), "{mode:?}");
        }
    }

    #[test]
    fn prompts_starting_with_dash_stay_positional() {
        let p = prompt_arg("-rf everything");
        assert_eq!(p, " -rf everything");
        assert_eq!(prompt_arg("fix it"), "fix it");
    }

    fn skill(id: &str, name: &str) -> Skill {
        Skill {
            id: id.into(),
            name: name.into(),
            description: String::new(),
            command: format!("/{name}"),
            source: "Personal".into(),
            path: PathBuf::from(id),
            agents: Vec::new(),
        }
    }

    #[test]
    fn switched_off_tools_and_skills_reach_claude_and_pi() {
        let skills = [skill("/s/review", "review"), skill("/s/fmt", "fmt")];
        let off = ["/s/review".to_string()];
        let tools = ToolSettings {
            skills: &skills,
            disabled_skills: &off,
            permissions: Permissions { run: Permission::Off, web: Permission::Off, ..Permissions::default() },
            ..Default::default()
        };
        let turn = TurnInput { tools, ..input(Mode::Auto, None) };
        let claude = harness("claude").unwrap().plan(&turn).args;
        assert!(claude.contains(&"--disallowedTools=Bash,WebSearch,Skill(review)".to_string()), "{claude:?}");
        let pi = harness("pi").unwrap().plan(&turn).args;
        assert!(has_pair(&pi, "--exclude-tools", "bash"));
        assert!(pi.contains(&"--no-skills".to_string()));
        assert!(has_pair(&pi, "--skill", "/s/fmt"));
        assert!(!pi.contains(&"/s/review".to_string()));
    }

    #[test]
    fn nothing_switched_off_leaves_launch_flags_alone() {
        let pi = args("pi", Mode::Auto, None);
        assert!(!pi.iter().any(|a| matches!(a.as_str(), "--exclude-tools" | "--no-skills" | "--skill" | "--mcp-config")), "{pi:?}");
        let claude = args("claude", Mode::Auto, None);
        assert!(!claude.iter().any(|a| a.starts_with("--disallowedTools")));
    }

    #[test]
    fn servers_reach_claude_pi_and_codex() {
        let servers = [
            McpServer {
                id: "docs".into(),
                name: "docs".into(),
                transport: McpTransport::Http {
                    url: "https://d.test/mcp".into(),
                    headers: BTreeMap::from([("Authorization".to_string(), "Bearer t".to_string())]),
                },
                enabled: true,
            },
            McpServer {
                id: "pg".into(),
                name: "pg".into(),
                transport: McpTransport::Stdio { command: "psql".into(), args: vec!["-q".into()], env: BTreeMap::new() },
                enabled: false,
            },
        ];
        let tools = ToolSettings { mcp: &servers, ..Default::default() };
        let turn = TurnInput { tools, ..input(Mode::Auto, None) };

        let codex = harness("codex").unwrap().plan(&turn);
        assert!(has_pair(&codex.args, "-c", "mcp_servers.\"docs\".url=\"https://d.test/mcp\""), "{:?}", codex.args);
        assert!(has_pair(&codex.args, "-c", "mcp_servers.\"pg\".enabled=false"));
        assert!(
            codex.args.iter().any(|a| a.starts_with("mcp_servers.\"docs\".env_http_headers=")),
            "headers are referenced by name: {:?}",
            codex.args
        );
        assert!(!codex.args.iter().any(|a| a.contains("Bearer t")), "the secret stays off argv: {:?}", codex.args);
        assert!(codex.env.contains(&("ARCHONUI_MCP_DOCS_AUTHORIZATION".to_string(), "Bearer t".to_string())), "{:?}", codex.env);

        let claude = harness("claude").unwrap().plan(&turn);
        assert!(claude.args.contains(&format!("--mcp-config={}", claude.files[0].0.display())), "{:?}", claude.args);
        let doc: serde_json::Value = serde_json::from_str(&claude.files[0].1).unwrap();
        assert_eq!(doc["mcpServers"]["docs"]["type"], "http");
        assert!(doc["mcpServers"].get("pg").is_none(), "disabled servers are not passed");
        assert!(claude.files[0].0.ends_with("mcp/claude.json"));

        // Pi gets an extension that registers the enabled servers; `--mcp-config` is not a Pi flag.
        let pi = harness("pi").unwrap().plan(&turn);
        assert!(has_pair(&pi.args, "--extension", &pi.files[0].0.display().to_string()), "{:?}", pi.args);
        assert!(!pi.args.iter().any(|a| a == "--mcp-config"));
        assert!(pi.files[0].1.contains(r#""docs":{"url":"https://d.test/mcp","headers":{"Authorization":"Bearer t"}}"#), "{}", pi.files[0].1);
        assert!(!pi.files[0].1.contains("psql"), "disabled servers are not passed");
    }

    /// One setting at a time: (permission row switched Off, server enabled, skill disabled).
    fn plan_for(h: &dyn Harness, row: Option<usize>, server: bool, skill_off: bool) -> (Vec<String>, Vec<(String, String)>, Vec<String>) {
        let servers = [McpServer {
            id: "t".into(),
            name: "t".into(),
            transport: McpTransport::Stdio { command: "t-mcp".into(), args: vec![], env: BTreeMap::new() },
            enabled: true,
        }];
        let skills = [skill("/s/review", "review")];
        let off = ["/s/review".to_string()];
        let mut levels = [Permission::Allow; 7];
        if let Some(i) = row {
            levels[i] = Permission::Off;
        }
        let [read, edit, grep, run, browser, web, fetch] = levels;
        let tools = ToolSettings {
            mcp: if server { &servers } else { &[] },
            skills: &skills,
            disabled_skills: if skill_off { &off } else { &[] },
            permissions: Permissions { read, edit, grep, run, browser, web, fetch },
            ..Default::default()
        };
        assert_eq!(permission_rows(tools.permissions), levels);
        let plan = h.plan(&TurnInput { tools, ..input(Mode::Auto, None) });
        (plan.args, plan.env, plan.files.into_iter().map(|(_, text)| text).collect())
    }

    #[test]
    fn each_harness_reports_the_tool_settings_it_applies() {
        for h in HARNESSES {
            let support = h.tool_support();
            let base = plan_for(*h, None, false, false);
            for (i, applied) in support.permissions.into_iter().enumerate() {
                assert_eq!(plan_for(*h, Some(i), false, false) != base, applied, "{} row {i}", h.id());
            }
            assert_eq!(plan_for(*h, None, true, false) != base, support.mcp, "{} mcp", h.id());
            assert_eq!(plan_for(*h, None, false, true) != base, support.skill_toggles, "{} skills", h.id());
            assert!(!support.ask, "{}: no harness can ask while it runs headless", h.id());
        }
    }

    #[test]
    fn codex_moves_stdio_env_off_the_command_line_by_name() {
        let servers = [
            McpServer {
                id: "gh".into(),
                name: "gh".into(),
                transport: McpTransport::Stdio {
                    command: "gh-mcp".into(),
                    args: vec![],
                    env: BTreeMap::from([("GITHUB_TOKEN".to_string(), "s3cret".to_string())]),
                },
                enabled: true,
            },
            McpServer {
                id: "pg".into(),
                name: "pg".into(),
                transport: McpTransport::Stdio {
                    command: "pg-mcp".into(),
                    args: vec![],
                    env: BTreeMap::from([("GITHUB_TOKEN".to_string(), "other".to_string())]),
                },
                enabled: true,
            },
        ];
        let tools = ToolSettings { mcp: &servers, ..Default::default() };
        let plan = harness("codex").unwrap().plan(&TurnInput { tools, ..input(Mode::Auto, None) });
        assert!(plan.env.contains(&("GITHUB_TOKEN".to_string(), "s3cret".to_string())), "{:?}", plan.env);
        assert!(plan.args.iter().any(|a| a == "mcp_servers.\"gh\".env_vars=[\"GITHUB_TOKEN\"]"), "{:?}", plan.args);
        // The conflicting server is skipped rather than handed the other server's secret.
        assert!(!plan.args.iter().any(|a| a == "mcp_servers.\"pg\".env_vars=[\"GITHUB_TOKEN\"]"), "{:?}", plan.args);
        assert!(!plan.args.iter().any(|a| a.contains("s3cret") || a.contains("other")), "{:?}", plan.args);
    }

    fn plan_with(id: &str, mode: Mode, session: Option<&str>, tools: ToolSettings) -> Vec<String> {
        harness(id).unwrap().plan(&TurnInput { tools, ..input(mode, session) }).args
    }

    #[test]
    fn claude_caps_steps_and_sandboxes_auto_only() {
        let sandbox = ToolSettings { sandbox: true, ask_network: false, max_steps: Some(40), ..Default::default() };
        let auto = plan_with("claude", Mode::Auto, None, sandbox);
        assert!(has_pair(&auto, "--max-turns", "40"));
        assert_eq!(auto.iter().filter(|a| *a == "--settings").count(), 1, "{auto:?}");
        let at = auto.iter().position(|a| a == "--settings").unwrap();
        let doc: serde_json::Value = serde_json::from_str(&auto[at + 1]).unwrap();
        assert_eq!(
            doc,
            serde_json::json!({"sandbox": {"enabled": true, "autoAllowBashIfSandboxed": true, "network": {"allowedDomains": ["*"]}}})
        );

        let blocked = plan_with("claude", Mode::Auto, None, ToolSettings { ask_network: true, ..sandbox });
        let at = blocked.iter().position(|a| a == "--settings").unwrap();
        let doc: serde_json::Value = serde_json::from_str(&blocked[at + 1]).unwrap();
        assert_eq!(doc, serde_json::json!({"sandbox": {"enabled": true, "autoAllowBashIfSandboxed": true}}));

        // Plan and Ask keep their own permission modes; the sandbox is for Auto only.
        for mode in [Mode::Plan, Mode::Ask] {
            let args = plan_with("claude", mode, None, sandbox);
            assert!(!args.iter().any(|a| a == "--settings"), "{mode:?}: {args:?}");
            assert!(has_pair(&args, "--max-turns", "40"));
        }

        let unset = plan_with("claude", Mode::Auto, None, ToolSettings { max_steps: None, ..Default::default() });
        assert!(!unset.iter().any(|a| a == "--max-turns" || a == "--settings"), "{unset:?}");
    }

    #[test]
    fn codex_sandboxes_auto_and_allows_network_only_when_asked_off() {
        let sandbox = ToolSettings { sandbox: true, ask_network: false, ..Default::default() };
        let auto = plan_with("codex", Mode::Auto, None, sandbox);
        assert!(has_pair(&auto, "-c", "sandbox_mode=\"workspace-write\""));
        assert!(has_pair(&auto, "-c", "sandbox_workspace_write.network_access=true"));

        let resumed = plan_with("codex", Mode::Auto, Some("t1"), sandbox);
        assert!(resumed.contains(&"resume".to_string()));
        assert!(has_pair(&resumed, "-c", "sandbox_mode=\"workspace-write\""));
        assert!(!resumed.iter().any(|a| a == "-s"), "`exec resume` does not accept -s");

        let blocked = plan_with("codex", Mode::Auto, None, ToolSettings { ask_network: true, ..sandbox });
        assert!(has_pair(&blocked, "-c", "sandbox_mode=\"workspace-write\""));
        assert!(!blocked.iter().any(|a| a.contains("network_access")));

        let ask = plan_with("codex", Mode::Ask, None, sandbox);
        assert!(has_pair(&ask, "-c", "sandbox_mode=\"read-only\""));
        assert!(!ask.iter().any(|a| a.contains("network_access")));

        let off = plan_with("codex", Mode::Auto, None, ToolSettings::default());
        assert!(!off.iter().any(|a| a.starts_with("sandbox_mode") || a.contains("network_access")));
    }

    #[test]
    fn opencode_step_limit_reaches_the_config_for_each_mode() {
        let steps = ToolSettings { max_steps: Some(40), ..Default::default() };
        for (mode, agent) in [(Mode::Auto, "build"), (Mode::Plan, "plan"), (Mode::Ask, "plan")] {
            let plan = harness("opencode").unwrap().plan(&TurnInput { tools: steps, ..input(mode, None) });
            let (_, text) = plan.env.iter().find(|(k, _)| k == "OPENCODE_CONFIG_CONTENT").expect("config env");
            let config: serde_json::Value = serde_json::from_str(text).unwrap();
            assert_eq!(config["agent"][agent]["steps"], 40, "{mode:?}");
        }
    }

    #[test]
    fn codex_and_pi_ignore_the_step_limit() {
        let steps = ToolSettings { max_steps: Some(40), ..Default::default() };
        for id in ["codex", "pi"] {
            assert_eq!(plan_with(id, Mode::Auto, None, steps), plan_with(id, Mode::Auto, None, ToolSettings::default()), "{id}");
        }
    }

    #[test]
    fn pi_ignores_the_sandbox_settings() {
        let sandbox = ToolSettings { sandbox: true, ask_network: false, ..Default::default() };
        assert_eq!(plan_with("pi", Mode::Auto, None, sandbox), plan_with("pi", Mode::Auto, None, ToolSettings::default()));
    }
}
