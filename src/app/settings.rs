//! App settings: everything the settings modal edits. Saved as `settings.json` in the config dir.
//! Missing fields take their defaults, so files from older versions still load.

use crate::{
    engine::Mode,
    store::{AgentDefaults, HarnessConfig, IdeKind, SearchEngine, MAX_PINS},
    ui::{
        fonts::FontChoice,
        theme::{BackgroundStyle, ThemeId},
    },
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap},
    path::PathBuf,
};

/// Layout slider ranges, inclusive, in the units the settings page shows.
pub const GAPS: (f32, f32) = (0., 32.);
pub const RADIUS: (f32, f32) = (0., 20.);
pub const PANEL_BLUR: (f32, f32) = (0., 48.);
pub const WALLPAPER_DIM: (f32, f32) = (0., 80.);
pub const TEXT_SIZE: (f32, f32) = (13., 19.);
pub const TERMINAL_SIZE: (f32, f32) = (10., 24.);
pub const MAX_STEPS: (u32, u32) = (10, 100);

/// Interface size range (fraction of the design size).
pub const UI_SCALE: (f32, f32) = (0.6, 1.5);

/// The interface size new installs start at: the prototype's design shrunk to fit a typical window.
fn default_ui_scale() -> f32 {
    0.85
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    // Appearance
    pub theme: ThemeId,
    /// Per theme (`theme_key`): token name to colour, e.g. `"accent" -> "#c81e2a"`.
    pub custom_colours: HashMap<String, BTreeMap<String, String>>,
    /// Per theme (`theme_key`): the font pairing and any per-slot overrides.
    pub fonts: HashMap<String, FontChoice>,
    pub background: BackgroundStyle,
    /// Copies of the wallpapers the user added, in the data dir.
    pub user_wallpapers: Vec<PathBuf>,
    /// Per theme (`theme_key`): the wallpaper picked instead of the theme's own.
    pub wallpapers: HashMap<String, PathBuf>,
    /// Gap between tiles, px.
    pub gaps: f32,
    /// Tile corner radius, px.
    pub radius: f32,
    /// Blur behind sidebars and glass tiles, px.
    pub panel_blur: f32,
    /// Wallpaper darkening, percent.
    pub wallpaper_dim: f32,
    /// Give unfocused tiles the focused tile's tint.
    pub tint_all: bool,
    /// Message text size, px.
    pub text_size: f32,
    /// Interface size: every design size is multiplied by this, like zooming a page.
    #[serde(default = "default_ui_scale")]
    pub ui_scale: f32,
    /// Reduce motion: `None` follows the system, `Some` forces transitions on or off.
    pub reduce_motion: Option<bool>,
    // Identity
    pub app_name: String,
    pub icon: AppIcon,
    // General
    /// Reopen the tiles that were open when ArchonUI last quit.
    pub restore: bool,
    pub default_project: Option<String>,
    pub default_mode: Mode,
    /// Auto runs shell commands in the CLI's sandbox. Off leaves the harness config as it is.
    pub sandbox: bool,
    /// With `sandbox`: off lets sandboxed commands reach the network.
    pub ask_network: bool,
    pub show_tools: bool,
    pub max_steps: u32,
    pub notify: bool,
    pub sound: bool,
    pub update_channel: UpdateChannel,
    pub analytics: bool,
    // Keyboard
    /// Action to keystroke in GPUI form (`ctrl-shift-enter`). An empty string leaves the action unbound.
    #[serde(deserialize_with = "known_keys")]
    pub keys: HashMap<Action, String>,
    /// The `KEYS_VERSION` `keys` was saved with. Files without it predate the move off Alt.
    #[serde(default)]
    pub keys_version: u32,
    // Harnesses and models
    /// Harness new sessions use when none is picked; `None` for the first enabled one.
    pub default_harness: Option<String>,
    /// Folder new sessions run in; `None` for where ArchonUI was started.
    pub default_folder: Option<PathBuf>,
    /// Harness id to how ArchonUI uses it. Missing ids use `HarnessConfig::default()`.
    pub harnesses: HashMap<String, HarnessConfig>,
    /// Harness id to the model and effort new sessions with it start with.
    pub harness_defaults: HashMap<String, AgentDefaults>,
    /// Start new sessions on the `[1m]` long-context variant of a model when the harness lists one.
    pub long_context: bool,
    /// Name new chats from their first exchange with the harness's cheapest model.
    pub fast_titles: bool,
    /// Harness id to model id to enabled. Models not listed are enabled.
    pub model_enabled: HashMap<String, BTreeMap<String, bool>>,
    /// Harness id to pinned model ids, shown first in the model picker.
    pub pinned_models: HashMap<String, Vec<String>>,
    // Tools
    pub permissions: Permissions,
    pub mcp_servers: Vec<McpServer>,
    /// Skill id to enabled. Skills not listed are enabled.
    pub skills_enabled: BTreeMap<String, bool>,
    // Browser, IDE and terminal
    pub search_engine: SearchEngine,
    pub auto_open_preview: bool,
    pub ide: IdeKind,
    /// Start code-server when ArchonUI opens. Only with VS Code chosen.
    pub ide_autostart: bool,
    /// The code-server program; `None` to find it automatically.
    pub code_server_path: Option<PathBuf>,
    /// Run terminals inside tmux, so they survive closing ArchonUI.
    pub use_tmux: bool,
    /// Terminal text size, px.
    pub terminal_font_size: f32,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            theme: ThemeId::default(),
            custom_colours: HashMap::new(),
            fonts: HashMap::new(),
            background: BackgroundStyle::default(),
            user_wallpapers: Vec::new(),
            wallpapers: HashMap::new(),
            gaps: 14.,
            radius: 9.,
            panel_blur: 24.,
            wallpaper_dim: 0.,
            tint_all: false,
            text_size: 15.,
            ui_scale: default_ui_scale(),
            reduce_motion: None,
            app_name: "archonui".into(),
            icon: AppIcon::default(),
            restore: true,
            default_project: None,
            default_mode: Mode::default(),
            sandbox: false,
            ask_network: true,
            show_tools: true,
            max_steps: 40,
            notify: true,
            sound: false,
            update_channel: UpdateChannel::default(),
            analytics: false,
            keys: default_keys(),
            keys_version: KEYS_VERSION,
            default_harness: None,
            default_folder: None,
            harnesses: HashMap::new(),
            harness_defaults: HashMap::new(),
            long_context: false,
            fast_titles: true,
            model_enabled: HashMap::new(),
            pinned_models: HashMap::new(),
            permissions: Permissions::default(),
            mcp_servers: Vec::new(),
            skills_enabled: BTreeMap::new(),
            search_engine: SearchEngine::default(),
            auto_open_preview: true,
            ide: IdeKind::default(),
            ide_autostart: false,
            code_server_path: None,
            use_tmux: true,
            terminal_font_size: 13.,
        }
    }
}

impl AppSettings {
    /// Brings values back into their ranges and fills in keybindings the file left out.
    pub fn normalize(&mut self) {
        // A NaN would be saved as `null`, which no longer parses, so it goes back to the default.
        let d = Self::default();
        self.gaps = fit(self.gaps, GAPS, d.gaps);
        self.radius = fit(self.radius, RADIUS, d.radius);
        self.panel_blur = fit(self.panel_blur, PANEL_BLUR, d.panel_blur);
        self.wallpaper_dim = fit(self.wallpaper_dim, WALLPAPER_DIM, d.wallpaper_dim);
        self.text_size = fit(self.text_size, TEXT_SIZE, d.text_size);
        self.ui_scale = fit(self.ui_scale, UI_SCALE, d.ui_scale);
        self.terminal_font_size = fit(self.terminal_font_size, TERMINAL_SIZE, d.terminal_font_size);
        self.max_steps = self.max_steps.clamp(MAX_STEPS.0, MAX_STEPS.1);
        // Effort is passed to the CLI verbatim, so an older title-case value goes back to its id.
        for defaults in self.harness_defaults.values_mut() {
            if let Some(effort) = defaults.effort.as_mut() {
                effort.make_ascii_lowercase();
            }
        }
        // Defaults that moved off Alt (Hyprland grabs Alt+key). An action still on its old default is upgraded,
        // once per file, so a user who binds the old key again keeps it. The new key is skipped if another
        // action already took it.
        if self.keys_version < KEYS_VERSION {
            for (action, old) in MOVED_DEFAULTS {
                let new = action.default_key();
                let taken = self.keys.iter().any(|(a, k)| *a != action && canonical(k) == canonical(new));
                if self.keys.get(&action).is_some_and(|k| canonical(k) == old) && !taken {
                    self.keys.insert(action, new.to_string());
                }
            }
            self.keys_version = KEYS_VERSION;
        }
        for action in Action::ALL {
            self.keys.entry(action).or_insert_with(|| action.default_key().to_string());
        }
    }

    pub fn harness(&self, id: &str) -> HarnessConfig {
        self.harnesses.get(id).cloned().unwrap_or_default()
    }

    /// Whether `model` of `harness` is enabled. Models not listed are.
    pub fn model_enabled(&self, harness: &str, model: &str) -> bool {
        self.model_enabled.get(harness).and_then(|m| m.get(model)).copied().unwrap_or(true)
    }

    pub fn pins(&self, harness: &str) -> &[String] {
        self.pinned_models.get(harness).map_or(&[], Vec::as_slice)
    }

    /// Pin or unpin `model`. Returns false if pinning would exceed `MAX_PINS`.
    pub fn toggle_pin(&mut self, harness: &str, model: &str) -> bool {
        let pins = self.pinned_models.entry(harness.to_string()).or_default();
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

/// `v` in `range`, or `default` when it isn't a number.
fn fit(v: f32, (lo, hi): (f32, f32), default: f32) -> f32 {
    if v.is_finite() {
        v.clamp(lo, hi)
    } else {
        default
    }
}

/// The keybindings in a settings file, leaving out actions this build doesn't know (a newer build's), so
/// they don't fail the whole file.
fn known_keys<'de, D: serde::Deserializer<'de>>(d: D) -> Result<HashMap<Action, String>, D::Error> {
    let raw = HashMap::<String, String>::deserialize(d)?;
    Ok(raw
        .into_iter()
        .filter_map(|(action, key)| Some((serde_json::from_value(serde_json::Value::String(action)).ok()?, key)))
        .collect())
}

/// Stable key for a theme in the settings maps. `ThemeId` isn't `Hash`, so maps key by this.
pub fn theme_key(theme: ThemeId) -> &'static str {
    match theme {
        ThemeId::ArchonUI => "archonui",
        ThemeId::BloodMoon => "blood-moon",
        ThemeId::Moonlight => "moonlight",
        ThemeId::Wreck => "wreck",
        ThemeId::Citadel => "citadel",
        ThemeId::Neon => "neon",
        ThemeId::Violet => "violet",
        ThemeId::Ink => "ink",
        ThemeId::Bonfire => "bonfire",
        ThemeId::Hunter => "hunter",
        ThemeId::Shards => "shards",
        ThemeId::Vale => "vale",
    }
}

/// The app icon: follow the theme, a theme's preset icon, or a file the user uploaded.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AppIcon {
    #[default]
    Theme,
    Preset(ThemeId),
    /// A copy in the data dir's `icons/`.
    Custom(PathBuf),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UpdateChannel {
    #[default]
    Stable,
    Beta,
    Nightly,
}

/// What the agent may do with one built-in tool category.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Permission {
    Allow,
    Ask,
    Off,
}

/// Permission per built-in tool category. Defaults per settings.md §7.2.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Permissions {
    pub read: Permission,
    pub edit: Permission,
    pub grep: Permission,
    pub run: Permission,
    pub browser: Permission,
    pub web: Permission,
    pub fetch: Permission,
}

impl Default for Permissions {
    fn default() -> Self {
        Self {
            read: Permission::Allow,
            edit: Permission::Ask,
            grep: Permission::Allow,
            run: Permission::Ask,
            browser: Permission::Ask,
            web: Permission::Allow,
            fetch: Permission::Allow,
        }
    }
}

/// An MCP server ArchonUI passes to harnesses that support MCP.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct McpServer {
    pub id: String,
    pub name: String,
    pub transport: McpTransport,
    pub enabled: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum McpTransport {
    Stdio {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        env: BTreeMap<String, String>,
    },
    Http {
        url: String,
        #[serde(default)]
        headers: BTreeMap<String, String>,
    },
}

/// Every action with a keybinding. Bindings are GPUI keystroke strings.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    SplitNewSession,
    CloseTile,
    FocusNext,
    FocusPrev,
    CycleMode,
    NextTheme,
    Send,
    Newline,
    CloseDialog,
    ToggleSidebar,
    TogglePanel,
    NewSession,
    SessionSwitcher,
}

impl Action {
    pub const ALL: [Action; 13] = [
        Action::SplitNewSession,
        Action::CloseTile,
        Action::FocusNext,
        Action::FocusPrev,
        Action::CycleMode,
        Action::NextTheme,
        Action::Send,
        Action::Newline,
        Action::CloseDialog,
        Action::ToggleSidebar,
        Action::TogglePanel,
        Action::NewSession,
        Action::SessionSwitcher,
    ];

    /// The keystroke this action starts with (settings.md §4, plus the four new ones).
    pub fn default_key(self) -> &'static str {
        match self {
            Action::SplitNewSession => "ctrl-shift-enter",
            Action::CloseTile => "ctrl-shift-w",
            Action::FocusNext => "ctrl-shift-right",
            Action::FocusPrev => "ctrl-shift-left",
            Action::CycleMode => "shift-tab",
            Action::NextTheme => "ctrl-shift-t",
            Action::Send => "enter",
            Action::Newline => "shift-enter",
            Action::CloseDialog => "escape",
            Action::ToggleSidebar => "ctrl-b",
            Action::TogglePanel => "ctrl-j",
            Action::NewSession => "ctrl-n",
            Action::SessionSwitcher => "ctrl-k",
        }
    }
}

pub fn default_keys() -> HashMap<Action, String> {
    Action::ALL.into_iter().map(|a| (a, a.default_key().to_string())).collect()
}

/// The version of the default keybindings. Bumped when defaults move, so `normalize` upgrades older files.
const KEYS_VERSION: u32 = 1;

/// Actions whose default moved in `KEYS_VERSION` 1, with their old default (canonical form).
const MOVED_DEFAULTS: [(Action, &str); 5] = [
    (Action::SplitNewSession, "alt-enter"),
    (Action::CloseTile, "alt-q"),
    (Action::FocusNext, "alt-right"),
    (Action::FocusPrev, "alt-left"),
    (Action::NextTheme, "alt-t"),
];

/// Keystrokes bound to more than one action, with those actions. Modifier order and case don't
/// matter (`shift-alt-q` is `alt-shift-q`). Empty bindings are unbound and never conflict.
pub fn conflicts(keys: &HashMap<Action, String>) -> Vec<(String, Vec<Action>)> {
    let mut by_key: BTreeMap<String, Vec<Action>> = BTreeMap::new();
    for (action, keystroke) in keys {
        if !keystroke.is_empty() {
            by_key.entry(canonical(keystroke)).or_default().push(*action);
        }
    }
    by_key
        .into_iter()
        .filter(|(_, actions)| actions.len() > 1)
        .map(|(key, mut actions)| {
            actions.sort_by_key(|a| Action::ALL.iter().position(|x| x == a));
            (key, actions)
        })
        .collect()
}

/// `keystroke` with lower-case parts and modifiers in a fixed order.
fn canonical(keystroke: &str) -> String {
    let lower = keystroke.to_ascii_lowercase();
    let mut parts: Vec<&str> = lower.split('-').collect();
    let key = parts.pop().unwrap_or_default();
    let rank = |m: &str| ["ctrl", "alt", "shift", "cmd"].iter().position(|x| *x == m).unwrap_or(4);
    parts.sort_by_key(|m| rank(m));
    parts.push(key);
    parts.join("-")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_spec() {
        let s = AppSettings::default();
        assert_eq!((s.gaps, s.radius, s.panel_blur, s.wallpaper_dim, s.text_size), (14., 9., 24., 0., 15.));
        assert!(!s.tint_all && s.restore && !s.sandbox && s.ask_network && s.show_tools && s.notify);
        assert!(!s.sound && !s.analytics && s.fast_titles);
        assert_eq!((s.max_steps, s.default_mode, s.app_name.as_str()), (40, Mode::Auto, "archonui"));
        assert_eq!(s.update_channel, UpdateChannel::Stable);
        assert_eq!(s.reduce_motion, None, "motion follows the system until set");
        assert_eq!(s.permissions.edit, Permission::Ask);
        assert_eq!(s.permissions.read, Permission::Allow);
        assert_eq!(s.keys[&Action::Send], "enter");
        assert!(conflicts(&s.keys).is_empty());
    }

    #[test]
    fn json_round_trips() {
        let mut s = AppSettings::default();
        s.theme = ThemeId::Neon;
        s.icon = AppIcon::Preset(ThemeId::Vale);
        s.custom_colours.entry("neon".into()).or_default().insert("accent".into(), "#c81e2a".into());
        s.model_enabled.entry("pi".into()).or_default().insert("gpt".into(), false);
        s.mcp_servers.push(McpServer {
            id: "gh".into(),
            name: "GitHub".into(),
            transport: McpTransport::Http { url: "https://example.test".into(), headers: BTreeMap::new() },
            enabled: true,
        });
        s.mcp_servers.push(McpServer {
            id: "pg".into(),
            name: "Postgres".into(),
            transport: McpTransport::Stdio {
                command: "psql".into(),
                args: vec!["-q".into()],
                env: BTreeMap::from([("PGHOST".into(), "localhost".into())]),
            },
            enabled: false,
        });
        let json = serde_json::to_value(&s).unwrap();
        let back: AppSettings = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(serde_json::to_value(&back).unwrap(), json);
        assert_eq!(back.theme, ThemeId::Neon);
        assert_eq!(back.keys[&Action::CloseTile], "ctrl-shift-w");
    }

    #[test]
    fn old_alt_defaults_migrate_and_custom_keys_stay() {
        // A file from before the move has no `keys_version`.
        let mut s: AppSettings = serde_json::from_str(r#"{"keys": {"close_tile": "alt-q", "next_theme": "ctrl-alt-t"}}"#).unwrap();
        s.normalize();
        assert_eq!(s.keys[&Action::CloseTile], "ctrl-shift-w");
        assert_eq!(s.keys[&Action::NextTheme], "ctrl-alt-t", "user-customised keys are kept");
        assert!(conflicts(&s.keys).is_empty());
    }

    #[test]
    fn the_alt_migration_runs_once_and_only_for_the_action_it_was_default_for() {
        let json = r#"{"keys": {"session_switcher": "alt-q", "split_new_session": "alt-enter", "new_session": "ctrl-shift-enter"}}"#;
        let mut s: AppSettings = serde_json::from_str(json).unwrap();
        s.normalize();
        assert_eq!(s.keys[&Action::SessionSwitcher], "alt-q", "alt-q was never this action's default");
        assert_eq!(s.keys[&Action::SplitNewSession], "alt-enter", "the new default is taken by another action");
        assert!(conflicts(&s.keys).is_empty());

        // Once saved, binding the old key again sticks.
        let mut s: AppSettings = serde_json::from_value(serde_json::to_value(&s).unwrap()).unwrap();
        s.keys.insert(Action::CloseTile, "alt-q".into());
        let mut back: AppSettings = serde_json::from_value(serde_json::to_value(&s).unwrap()).unwrap();
        back.normalize();
        assert_eq!(back.keys[&Action::CloseTile], "alt-q");
    }

    #[test]
    fn keys_for_unknown_actions_are_dropped_not_the_file() {
        let mut s: AppSettings =
            serde_json::from_str(r#"{"gaps": 20, "keys": {"send": "ctrl-enter", "from_the_future": "ctrl-f"}}"#).unwrap();
        s.normalize();
        assert_eq!(s.gaps, 20.);
        assert_eq!(s.keys[&Action::Send], "ctrl-enter");
        assert_eq!(s.keys.len(), Action::ALL.len());
    }

    #[test]
    fn non_numbers_go_back_to_defaults() {
        let mut s = AppSettings { gaps: f32::NAN, terminal_font_size: 99., ui_scale: f32::INFINITY, ..Default::default() };
        s.normalize();
        assert_eq!((s.gaps, s.terminal_font_size, s.ui_scale), (14., TERMINAL_SIZE.1, 0.85));
    }

    #[test]
    fn reduce_motion_round_trips_and_defaults_to_the_system() {
        let mut s = AppSettings::default();
        s.reduce_motion = Some(true);
        let back: AppSettings = serde_json::from_value(serde_json::to_value(&s).unwrap()).unwrap();
        assert_eq!(back.reduce_motion, Some(true));
        let older: AppSettings = serde_json::from_str(r#"{"gaps": 12}"#).unwrap();
        assert_eq!(older.reduce_motion, None);
    }

    #[test]
    fn saved_efforts_are_normalised_to_harness_ids() {
        let mut s = AppSettings::default();
        s.harness_defaults.insert(
            "claude".into(),
            AgentDefaults { model: Some("haiku".into()), effort: Some("High".into()) },
        );
        s.normalize();
        assert_eq!(s.harness_defaults["claude"].effort.as_deref(), Some("high"));
        s.harness_defaults.get_mut("claude").unwrap().effort = None;
        s.normalize();
        assert_eq!(s.harness_defaults["claude"].effort, None, "a missing effort stays missing");
    }

    #[test]
    fn missing_fields_take_defaults() {
        let mut s: AppSettings = serde_json::from_str(r#"{"gaps": 20, "theme": "vale"}"#).unwrap();
        s.normalize();
        assert_eq!((s.gaps, s.theme, s.radius, s.max_steps), (20., ThemeId::Vale, 9., 40));
        assert_eq!(s.keys.len(), Action::ALL.len());
    }

    #[test]
    fn settings_from_older_versions_still_load() {
        let s: AppSettings = serde_json::from_str(r#"{"extensions_enabled": {"x": false}, "gaps": 20}"#).unwrap();
        assert_eq!(s.gaps, 20.);
        assert!(s.fast_titles, "a file without the setting turns fast titles on");
    }

    #[test]
    fn out_of_range_values_are_clamped() {
        let mut s = AppSettings { gaps: 99., max_steps: 3, text_size: 1., wallpaper_dim: -5., ..Default::default() };
        s.keys.remove(&Action::Newline);
        s.normalize();
        assert_eq!((s.gaps, s.max_steps, s.text_size, s.wallpaper_dim), (32., 10, 13., 0.));
        assert_eq!(s.keys[&Action::Newline], "shift-enter", "missing keys come back");
    }

    #[test]
    fn keybinding_conflicts_are_found() {
        let mut keys = default_keys();
        assert!(conflicts(&keys).is_empty());
        keys.insert(Action::NewSession, "shift-alt-enter".into());
        keys.insert(Action::SplitNewSession, "alt-shift-enter".into());
        assert_eq!(conflicts(&keys), vec![("alt-shift-enter".to_string(), vec![Action::SplitNewSession, Action::NewSession])]);
        keys.insert(Action::NewSession, String::new());
        keys.insert(Action::SplitNewSession, String::new());
        assert!(conflicts(&keys).is_empty(), "unbound actions never conflict");
    }

    #[test]
    fn pins_are_capped() {
        let mut s = AppSettings::default();
        for m in ["a", "b", "c"] {
            assert!(s.toggle_pin("pi", m));
        }
        assert!(!s.toggle_pin("pi", "d"), "a fourth pin is refused");
        assert!(s.toggle_pin("pi", "b"), "unpinning works");
        assert_eq!(s.pins("pi"), ["a", "c"]);
        assert!(s.pins("codex").is_empty());
    }

    #[test]
    fn unlisted_models_and_harnesses_are_enabled() {
        let s = AppSettings::default();
        assert!(s.model_enabled("pi", "anything"));
        assert!(s.harness("pi").enabled);
    }
}
