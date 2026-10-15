//! Chat sessions persisted as one JSON file each under
//! `$ARCHONUI_DATA_DIR`, else `$XDG_DATA_HOME/archonui/sessions`, else
//! `~/.local/share/archonui/sessions`. App settings live in `settings.json`
//! next to that directory.

use crate::{
    engine::{attach::Attachment, Mode},
    ui::sigil::seed_icon_for,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs, io,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Agent,
    Error,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub text: String,
    /// Copies kept under `attachments_dir(session)`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<Attachment>,
    /// Tool calls the agent made while writing this reply, oldest first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<ToolCall>,
    /// Seconds the model spent thinking, when the harness reports it ("thought for Ns").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thought_secs: Option<u32>,
    /// Seconds the whole reply took ("worked for Ns" when no thinking time is known).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub took_secs: Option<u32>,
    /// Registry id of the agent that wrote this reply. A chat can switch agents, so it may differ
    /// from the session's. `None` on user and error rows and on replies saved before switching existed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    /// Byte offset in `text` where the final answer starts: the text after the reply's last tool call.
    /// What comes before it is progress notes. `None` when the reply made no tool calls.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub final_at: Option<usize>,
    /// Tokens and cost of the turn that wrote this reply, when the agent reports them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
}

/// What one turn used, summed over its model calls. `input` excludes cached tokens.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Usage {
    pub input: u64,
    pub output: u64,
    /// Input tokens read from the prompt cache.
    pub cache_read: u64,
    /// Input tokens written to the prompt cache.
    pub cache_write: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
}

impl Usage {
    pub fn add(&mut self, other: Usage) {
        self.input += other.input;
        self.output += other.output;
        self.cache_read += other.cache_read;
        self.cache_write += other.cache_write;
        self.cost_usd = match (self.cost_usd, other.cost_usd) {
            (None, None) => None,
            (a, b) => Some(a.unwrap_or(0.) + b.unwrap_or(0.)),
        };
    }

    /// All input tokens, cached or not.
    pub fn total_input(&self) -> u64 {
        self.input + self.cache_read + self.cache_write
    }

    /// Share of input tokens read from the cache, 0–100. `None` with no input.
    pub fn cache_hit(&self) -> Option<u32> {
        let total = self.total_input();
        (total > 0).then(|| ((self.cache_read as f64 / total as f64) * 100.).round() as u32)
    }
}

/// One tool call in an agent reply, shown as a row above the text.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
    pub kind: ToolKind,
    /// What the call acted on: a path, a pattern, a command or a URL.
    pub target: String,
    /// Result summary, e.g. "+24 -8" or "128 passed".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// `None` while the call is still running.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ok: Option<bool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolKind {
    Read,
    Grep,
    Glob,
    Edit,
    Write,
    Run,
    Web,
    Fetch,
    Task,
    /// Also any kind a newer build wrote, so its chats still load.
    #[serde(other)]
    Other,
}

/// Icon of a session saved before icons existed. `app::AppState` assigns one on load.
pub const UNASSIGNED_ICON: u16 = u16::MAX;

fn unassigned_icon() -> u16 {
    UNASSIGNED_ICON
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    /// Registry id of the agent this session talks to.
    pub agent: String,
    pub title: String,
    /// The title was made automatically (from the first prompt or a fast title), not typed by the user.
    #[serde(default)]
    pub title_auto: bool,
    /// Index into `ui::sigil::sigils()`; unique among sessions.
    #[serde(default = "unassigned_icon")]
    pub icon: u16,
    /// Id of the project this chat belongs to, if any.
    #[serde(default)]
    pub project: Option<String>,
    /// Agents key their own sessions by directory, so resume where we started.
    pub cwd: PathBuf,
    /// The agent's own session id, used to resume it on the next turn.
    pub agent_session: Option<String>,
    /// Model to pass the agent; `None` uses the agent's default.
    #[serde(default)]
    pub model: Option<String>,
    /// Effort level to pass the agent; `None` uses the agent's default.
    #[serde(default)]
    pub effort: Option<String>,
    /// What the agent may do in this chat's turns.
    #[serde(default)]
    pub mode: Mode,
    /// The chat switched agents and the new one hasn't been sent the conversation so far yet.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub handoff: bool,
    /// A terminal was opened for this chat, so the chat is kept even without messages.
    #[serde(default)]
    pub terminal: bool,
    /// Text typed in the composer but not sent yet.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub draft: String,
    /// The user left this chat with an unsent draft, so it stays in the sidebar, even without
    /// messages, until it is deleted. A new chat is hidden until it has a message or this is set.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub listed: bool,
    pub created: u64,
    /// Last activity, in seconds since the epoch. The sidebar sorts by it ("last used").
    pub updated: u64,
    pub messages: Vec<Message>,
}

impl Session {
    pub fn new(agent: &str, cwd: PathBuf) -> Self {
        let now = now();
        let id = new_id();
        Self {
            icon: seed_icon_for(&id, &[]),
            project: None,
            id,
            agent: agent.to_string(),
            title: String::new(),
            title_auto: false,
            cwd,
            agent_session: None,
            model: None,
            effort: None,
            mode: Mode::default(),
            handoff: false,
            terminal: false,
            draft: String::new(),
            listed: false,
            created: now,
            updated: now,
            messages: Vec::new(),
        }
    }

    /// Whether the sidebar lists this chat: it has a message, a terminal, or a draft the user left.
    pub fn visible(&self) -> bool {
        !self.messages.is_empty() || self.terminal || self.listed
    }

    pub fn push(&mut self, role: Role, text: impl Into<String>) {
        self.push_with(role, text, Vec::new());
    }

    pub fn push_with(&mut self, role: Role, text: impl Into<String>, attachments: Vec<Attachment>) {
        let agent = (role == Role::Agent).then(|| self.agent.clone());
        self.messages.push(Message { role, text: text.into(), attachments, tools: Vec::new(), thought_secs: None, took_secs: None, agent, final_at: None, usage: None });
        self.updated = now();
    }

    /// The reply currently being written (the latest agent message).
    pub fn reply_mut(&mut self) -> Option<&mut Message> {
        self.messages.iter_mut().rev().find(|m| m.role == Role::Agent)
    }
}

pub fn title_from(prompt: &str) -> String {
    let line = prompt.lines().next().unwrap_or("").trim();
    if line.chars().count() <= 40 {
        line.to_string()
    } else {
        format!("{}…", line.chars().take(40).collect::<String>().trim_end())
    }
}

/// Creates `path` and its missing parents, with the directories readable and
/// writable by this user only (0700). Existing directories keep their mode.
pub fn create_private_dir(path: &Path) -> io::Result<()> {
    fs::DirBuilder::new().recursive(true).mode(0o700).create(path)
}

/// Writes `bytes` to `path`, creating or truncating it 0600.
fn write_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    use io::Write;
    let mut file = fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(path)?;
    file.write_all(bytes)
}

pub fn dir() -> PathBuf {
    if let Some(d) = std::env::var_os("ARCHONUI_DATA_DIR") {
        return d.into();
    }
    // Tests never fall back to the user's real data, even when a parallel test unsets the variable.
    if cfg!(test) {
        return std::env::temp_dir().join(format!("archonui-test-data-{}/sessions", std::process::id()));
    }
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("archonui/sessions")
}

/// Most models that can be pinned per agent.
pub const MAX_PINS: usize = 3;

#[derive(Clone, Serialize, Deserialize)]
pub struct Settings {
    /// Agent id -> pinned model ids, shown first in the model picker.
    #[serde(default)]
    pub pinned_models: HashMap<String, Vec<String>>,
    /// Whether the right-hand panel (Preview / Details / Terminal) is hidden.
    #[serde(default, alias = "hide_details")]
    pub hide_panel: bool,
    /// Which tab the right-hand panel shows.
    #[serde(default)]
    pub panel_tab: PanelTab,
    /// Width of the right-hand panel in pixels; `None` for the default.
    #[serde(default)]
    pub panel_width: Option<f32>,
    /// Folder new chats run in; `None` for where ArchonUI was started (or `ARCHONUI_CWD`).
    #[serde(default)]
    pub default_folder: Option<PathBuf>,
    /// Agent id new chats use when none is chosen; `None` for the first one found.
    #[serde(default)]
    pub default_agent: Option<String>,
    /// Agent id -> model and effort new chats with that agent start with.
    #[serde(default)]
    pub agent_defaults: HashMap<String, AgentDefaults>,
    /// Where the preview's address bar sends searches.
    #[serde(default)]
    pub search_engine: SearchEngine,
    /// Open local sites and HTML files from agent replies in the preview.
    #[serde(default = "yes")]
    pub auto_open_preview: bool,
    /// The editor the IDE tab shows.
    #[serde(default)]
    pub ide: IdeKind,
    /// Start VS Code (code-server) when ArchonUI opens, so the IDE tab is ready. Only with VS Code chosen.
    #[serde(default = "no")]
    pub ide_autostart: bool,
    /// The code-server program; `None` to find it automatically.
    #[serde(default)]
    pub code_server_path: Option<PathBuf>,
    /// Run terminals inside tmux, so they survive closing ArchonUI.
    #[serde(default = "yes")]
    pub use_tmux: bool,
    /// Terminal text size in pixels.
    #[serde(default = "default_terminal_font_size")]
    pub terminal_font_size: f32,
    /// Harness id -> how ArchonUI uses it. Missing ids use `HarnessConfig::default()`.
    #[serde(default)]
    pub harnesses: HashMap<String, HarnessConfig>,
}

impl Default for Settings {
    fn default() -> Self {
        // Same values as an empty settings file.
        serde_json::from_str("{}").expect("all settings have defaults")
    }
}

fn yes() -> bool {
    true
}

fn no() -> bool {
    false
}

fn default_terminal_font_size() -> f32 {
    13.
}

#[derive(Clone, Serialize, Deserialize)]
pub struct HarnessConfig {
    /// Off hides the harness from new chats; existing chats still use it.
    #[serde(default = "yes")]
    pub enabled: bool,
    /// Binary to run instead of searching `PATH`.
    #[serde(default)]
    pub binary: Option<PathBuf>,
    /// Model new chats with this harness start with.
    #[serde(default)]
    pub default_model: Option<String>,
}

impl Default for HarnessConfig {
    fn default() -> Self {
        Self { enabled: true, binary: None, default_model: None }
    }
}

#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AgentDefaults {
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub effort: Option<String>,
}

#[derive(Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SearchEngine {
    #[default]
    DuckDuckGo,
    Google,
    Bing,
    Brave,
    Startpage,
}

impl SearchEngine {
    pub const ALL: [SearchEngine; 5] =
        [Self::DuckDuckGo, Self::Google, Self::Bing, Self::Brave, Self::Startpage];

    pub fn name(self) -> &'static str {
        match self {
            Self::DuckDuckGo => "DuckDuckGo",
            Self::Google => "Google",
            Self::Bing => "Bing",
            Self::Brave => "Brave Search",
            Self::Startpage => "Startpage",
        }
    }

    /// Search URL; the form-encoded query is appended.
    pub fn url_prefix(self) -> &'static str {
        match self {
            Self::DuckDuckGo => "https://duckduckgo.com/?q=",
            Self::Google => "https://www.google.com/search?q=",
            Self::Bing => "https://www.bing.com/search?q=",
            Self::Brave => "https://search.brave.com/search?q=",
            Self::Startpage => "https://www.startpage.com/do/search?q=",
        }
    }
}

#[derive(Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PanelTab {
    #[default]
    Preview,
    Details,
    Terminal,
    Ide,
    Git,
}

/// The editor the IDE tab shows.
#[derive(Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IdeKind {
    #[default]
    Builtin,
    VsCode,
}

impl Settings {
    pub fn harness(&self, id: &str) -> HarnessConfig {
        self.harnesses.get(id).cloned().unwrap_or_default()
    }

    pub fn pins(&self, agent: &str) -> &[String] {
        self.pinned_models.get(agent).map_or(&[], Vec::as_slice)
    }

    /// Pin or unpin `model`. Returns false if pinning would exceed `MAX_PINS`.
    pub fn toggle_pin(&mut self, agent: &str, model: &str) -> bool {
        let pins = self.pinned_models.entry(agent.to_string()).or_default();
        if let Some(i) = pins.iter().position(|m| m == model) {
            pins.remove(i);
        } else if pins.len() >= MAX_PINS {
            return false;
        } else {
            pins.push(model.to_string());
        }
        true
    }
}

/// The settings file the old UI wrote. `app::persist` migrates it once.
pub fn settings_path() -> PathBuf {
    dir().with_file_name("settings.json")
}

pub fn load_settings() -> Settings {
    fs::read(settings_path()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

pub fn save_settings(s: &Settings) -> io::Result<()> {
    // Through `write_atomic`, which syncs the file before the rename, so a crash can't leave an empty settings file.
    crate::app::persist::write_atomic(&settings_path(), &serde_json::to_vec_pretty(s)?)
}

pub fn load_all() -> Vec<Session> {
    load_from(&dir())
}

pub fn save(s: &Session) -> io::Result<()> {
    save_to(&dir(), s)
}

/// Deletes the session and its attachment copies.
pub fn delete(id: &str) -> io::Result<()> {
    let ok = |r: io::Result<()>| match r {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    };
    ok(fs::remove_dir_all(attachments_dir(id)))?;
    // The per-chat MCP config holds secrets, so it goes with the chat.
    ok(fs::remove_file(crate::engine::mcp::session_file(id)))?;
    ok(fs::remove_file(dir().join(format!("{id}.json"))))
}

/// Where a session's attachment copies live: `attachments/<id>/` next to
/// the sessions directory.
pub fn attachments_dir(session: &str) -> PathBuf {
    dir().with_file_name("attachments").join(session)
}

/// Copy `src` into the session's attachments, under a name not yet taken.
pub fn store_attachment(session: &str, src: &Path) -> io::Result<PathBuf> {
    let name = src.file_name().map_or_else(|| "file".into(), |n| n.to_string_lossy().into_owned());
    let dest = free_path(session, &name)?;
    fs::copy(src, &dest)?;
    fs::set_permissions(&dest, fs::Permissions::from_mode(0o600))?;
    Ok(dest)
}

/// Save bytes (e.g. a pasted image) into the session's attachments.
pub fn store_bytes(session: &str, name: &str, bytes: &[u8]) -> io::Result<PathBuf> {
    let dest = free_path(session, name)?;
    write_private(&dest, bytes)?;
    Ok(dest)
}

fn free_path(session: &str, name: &str) -> io::Result<PathBuf> {
    let dir = attachments_dir(session);
    create_private_dir(&dir)?;
    // Only the last component, so a name like `../x` or `/x` can't land outside the session's folder.
    let name = Path::new(name).file_name().map_or_else(|| "file".into(), |n| n.to_string_lossy().into_owned());
    let mut dest = dir.join(&name);
    let mut n = 2;
    while dest.exists() {
        dest = dir.join(format!("{n}-{name}"));
        n += 1;
    }
    Ok(dest)
}

pub(crate) fn load_from(dir: &Path) -> Vec<Session> {
    let Ok(entries) = fs::read_dir(dir) else { return Vec::new() };
    let mut sessions: Vec<Session> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .filter_map(|p| {
            let mut s: Session = serde_json::from_slice(&fs::read(&p).ok()?).ok()?;
            // The file name is the id; never trust a path-like id from contents. A dot id (`..json`)
            // would make the attachments dir the parent of every chat's, which `delete` removes.
            s.id = p.file_stem()?.to_str().filter(|id| !id.starts_with('.'))?.to_string();
            // A reply cut off by quitting mid-turn may have been saved empty. One with tool rows is kept,
            // as `AppState` keeps it at the end of a turn.
            s.messages.retain(|m| m.role != Role::Agent || !m.text.is_empty() || !m.tools.is_empty());
            s.visible().then_some(s)
        })
        .collect();
    sessions.sort_by_key(|s| std::cmp::Reverse(s.updated));
    sessions
}

fn save_to(dir: &Path, s: &Session) -> io::Result<()> {
    // Synced before the rename, so a crash can't leave an empty file that hides the chat on the next load.
    crate::app::persist::write_atomic(&dir.join(format!("{}.json", s.id)), &serde_json::to_vec_pretty(s)?)
}

pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

pub fn new_id() -> String {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    format!("archonui-{nanos:x}-{:x}", std::process::id())
}

/// "now", "5m", "3h", "2d" since `ts`.
pub fn ago(ts: u64) -> String {
    let d = now().saturating_sub(ts);
    match d {
        0..60 => "now".into(),
        60..3600 => format!("{}m", d / 60),
        3600..86400 => format!("{}h", d / 3600),
        _ => format!("{}d", d / 86400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_without_mode_loads_as_auto() {
        let mut v = serde_json::to_value(Session::new("codex", "/tmp".into())).unwrap();
        assert_eq!(v["mode"], "auto");
        v.as_object_mut().unwrap().remove("mode");
        let s: Session = serde_json::from_value(v).unwrap();
        assert_eq!(s.mode, crate::engine::Mode::Auto);
    }

    #[test]
    fn harness_config_defaults_and_roundtrips() {
        let mut s = Settings::default();
        assert!(s.harness("codex").enabled, "unlisted harnesses are enabled");
        s.harnesses.insert(
            "codex".into(),
            HarnessConfig { enabled: false, binary: Some("/opt/codex".into()), default_model: Some("gpt-x".into()) },
        );
        let mut v = serde_json::to_value(&s).unwrap();
        let back: Settings = serde_json::from_value(v.clone()).unwrap();
        let codex = back.harness("codex");
        assert!(!codex.enabled);
        assert_eq!(codex.binary.as_deref(), Some(std::path::Path::new("/opt/codex")));
        assert_eq!(codex.default_model.as_deref(), Some("gpt-x"));
        // Settings files written before `harnesses` existed still load.
        v.as_object_mut().unwrap().remove("harnesses");
        let old: Settings = serde_json::from_value(v).unwrap();
        assert!(old.harnesses.is_empty());
    }

    #[test]
    fn roundtrip_and_cleanup() {
        let dir = std::env::temp_dir().join(format!("archonui-test-{}", new_id()));
        let mut s = Session::new("codex", "/tmp".into());
        s.push(Role::User, "hello");
        s.push(Role::Agent, "");
        s.agent_session = Some("thread-1".into());
        save_to(&dir, &s).unwrap();
        save_to(&dir, &Session::new("pi", "/tmp".into())).unwrap(); // empty: skipped

        let loaded = load_from(&dir);
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, s.id);
        assert_eq!(loaded[0].agent_session.as_deref(), Some("thread-1"));
        assert_eq!(loaded[0].messages.len(), 1, "empty partial reply dropped");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn saved_sessions_are_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("archonui-test-{}", new_id()));
        let mut s = Session::new("codex", "/tmp".into());
        s.push(Role::User, "hi");
        save_to(&dir, &s).unwrap();
        let file = dir.join(format!("{}.json", s.id));
        assert_eq!(fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(fs::metadata(&dir).unwrap().permissions().mode() & 0o777, 0o700);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn chats_with_a_terminal_are_kept_without_messages() {
        let dir = std::env::temp_dir().join(format!("archonui-test-{}", new_id()));
        let mut terminal = Session::new("pi", "/tmp".into());
        terminal.terminal = true;
        save_to(&dir, &terminal).unwrap();
        save_to(&dir, &Session::new("pi", "/tmp".into())).unwrap(); // neither: skipped

        let loaded = load_from(&dir);
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, terminal.id);
        assert!(loaded[0].terminal);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn settings_default_like_an_empty_file() {
        let s = Settings::default();
        assert!(s.auto_open_preview && !s.ide_autostart && s.use_tmux);
        assert!(s.ide == IdeKind::Builtin);
        assert_eq!(s.terminal_font_size, 13.);
        assert!(s.search_engine == SearchEngine::DuckDuckGo);
        // Files from older versions keep their values and gain the new defaults.
        let old: Settings = serde_json::from_str(r#"{"hide_details": true, "panel_tab": "git"}"#).unwrap();
        assert!(old.hide_panel && old.panel_tab == PanelTab::Git && old.use_tmux);
    }

    #[test]
    fn pins_are_capped() {
        let mut s = Settings::default();
        for m in ["a", "b", "c"] {
            assert!(s.toggle_pin("pi", m));
        }
        assert!(!s.toggle_pin("pi", "d"), "a fourth pin is refused");
        assert!(s.toggle_pin("pi", "b"), "unpinning works");
        assert_eq!(s.pins("pi"), ["a", "c"]);
        assert!(s.pins("codex").is_empty());
    }

    #[test]
    fn legacy_session_format_loads_with_defaults() {
        let dir = std::env::temp_dir().join(format!("archonui-legacy-{}", new_id()));
        let legacy_json = r#"{
            "id": "archonui-old-session-123",
            "agent": "codex",
            "title": "Legacy chat",
            "cwd": "/home/archonminipc/ArchonUI",
            "agent_session": "01a121ad-8c49-78d3-bbcc-e0412d08826a",
            "created": 1791566380,
            "updated": 1791566396,
            "messages": [
                {
                    "role": "user",
                    "text": "hello"
                },
                {
                    "role": "agent",
                    "text": "hello back"
                }
            ]
        }"#;
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("archonui-old-session-123.json");
        fs::write(&path, legacy_json).unwrap();

        let loaded = load_from(&dir);
        assert_eq!(loaded.len(), 1);
        let s = &loaded[0];
        assert_eq!(s.id, "archonui-old-session-123");
        assert_eq!(s.agent, "codex");
        assert_eq!(s.title, "Legacy chat");
        assert!(!s.title_auto);
        assert_eq!(s.icon, UNASSIGNED_ICON);
        assert_eq!(s.project, None);
        assert_eq!(s.model, None);
        assert_eq!(s.effort, None);
        assert_eq!(s.mode, Mode::Auto);
        assert!(!s.terminal);
        assert_eq!(s.messages.len(), 2);
        assert!(s.messages[0].attachments.is_empty());
        assert!(s.messages[0].tools.is_empty());
        assert_eq!(s.messages[0].thought_secs, None);
        assert_eq!(s.messages[0].took_secs, None);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn new_session_format_loads_in_legacy_loader() {
        let dir = std::env::temp_dir().join(format!("archonui-new-{}", new_id()));
        let mut s = Session::new("pi", "/tmp".into());
        s.push(Role::User, "hi");
        s.push(Role::Agent, "response");
        s.icon = 42;
        s.title_auto = true;
        s.mode = Mode::Plan;
        s.messages[1].tools.push(ToolCall {
            kind: ToolKind::Read,
            target: "foo.rs".into(),
            detail: Some("+1 -1".into()),
            ok: Some(true),
        });
        s.messages[1].thought_secs = Some(5);
        s.messages[1].took_secs = Some(10);
        save_to(&dir, &s).unwrap();

        let loaded = load_from(&dir);
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].icon, 42);
        assert_eq!(loaded[0].mode, Mode::Plan);
        assert_eq!(loaded[0].messages[1].tools.len(), 1);
        assert_eq!(loaded[0].messages[1].thought_secs, Some(5));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn tool_only_replies_and_unknown_tool_kinds_survive_a_reload() {
        let dir = std::env::temp_dir().join(format!("archonui-test-{}", new_id()));
        let mut s = Session::new("opencode", "/tmp".into());
        s.push(Role::User, "list");
        s.push(Role::Agent, "");
        s.messages[1].tools.push(ToolCall { kind: ToolKind::Grep, target: "x".into(), detail: None, ok: Some(true) });
        let mut v = serde_json::to_value(&s).unwrap();
        v["messages"][1]["tools"][0]["kind"] = "from-the-future".into();
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(format!("{}.json", s.id)), v.to_string()).unwrap();

        let loaded = load_from(&dir);
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].messages.len(), 2, "the tool-only reply is kept");
        assert_eq!(loaded[0].messages[1].tools[0].kind, ToolKind::Other);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn dot_ids_are_not_loaded() {
        let dir = std::env::temp_dir().join(format!("archonui-test-{}", new_id()));
        let mut s = Session::new("pi", "/tmp".into());
        s.push(Role::User, "hi");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("..json"), serde_json::to_vec(&s).unwrap()).unwrap();
        assert!(load_from(&dir).is_empty());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn saving_leaves_no_temp_files() {
        let dir = std::env::temp_dir().join(format!("archonui-test-{}", new_id()));
        let mut s = Session::new("pi", "/tmp".into());
        s.push(Role::User, "hi");
        save_to(&dir, &s).unwrap();
        save_to(&dir, &s).unwrap();
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn titles_are_truncated() {
        assert_eq!(title_from("short"), "short");
        assert!(title_from(&"x".repeat(80)).ends_with('…'));
    }
}
