//! Files on disk for settings, workspace and projects: platform locations, atomic and debounced
//! writes, and the one-time migration from the old settings file. Sessions stay in `store`.

use super::{
    settings::AppSettings,
    workspace::{RightTab, Workspace, Project, RIGHT_WIDTH_DEFAULT},
};
use crate::store::{self, PanelTab};
use serde::{de::DeserializeOwned, Serialize};
use std::{
    collections::BTreeMap,
    env, fs,
    io::{self, Write},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    sync::{Arc, Condvar, Mutex},
    thread,
    time::Duration,
};

const SETTINGS_FILE: &str = "settings.json";
const WORKSPACE_FILE: &str = "workspace.json";
const PROJECTS_FILE: &str = "projects.json";
/// Quiet time after the last change before a batch of changes is written.
const DEBOUNCE: Duration = Duration::from_millis(300);
/// Longest a change waits while changes keep coming, so a long burst is still written.
const MAX_DELAY: Duration = Duration::from_secs(2);

/// Where the app keeps its files. `config` holds the three JSON files; `data` holds copied assets
/// (wallpapers and icons). Sessions live in `store::dir()`.
#[derive(Clone, Debug)]
pub struct Paths {
    pub config: PathBuf,
    pub data: PathBuf,
}

impl Paths {
    /// The platform's locations, with `ARCHONUI_CONFIG_DIR` taking precedence for the config dir.
    pub fn current() -> Self {
        // Tests never fall back to the user's real config, even when a parallel test unsets the variable.
        if cfg!(test) && std::env::var_os("ARCHONUI_CONFIG_DIR").is_none() {
            let config = std::env::temp_dir().join(format!("archonui-test-config-{}", std::process::id()));
            let data = store::dir().parent().map_or_else(|| PathBuf::from("."), Path::to_path_buf);
            return Self { config, data };
        }
        let config = config_dir_from(
            env_path("ARCHONUI_CONFIG_DIR"),
            env_path("XDG_CONFIG_HOME"),
            env_path("HOME"),
            env_path("APPDATA"),
        );
        let data = store::dir().parent().map_or_else(|| PathBuf::from("."), Path::to_path_buf);
        Self { config, data }
    }
}

fn env_path(key: &str) -> Option<PathBuf> {
    env::var_os(key).map(PathBuf::from)
}

/// The config dir for the given environment: an override, else the platform's `archonui` dir.
fn config_dir_from(
    override_dir: Option<PathBuf>,
    xdg: Option<PathBuf>,
    home: Option<PathBuf>,
    appdata: Option<PathBuf>,
) -> PathBuf {
    if let Some(dir) = override_dir {
        return dir;
    }
    let base = if cfg!(target_os = "windows") {
        appdata
    } else if cfg!(target_os = "macos") {
        home.map(|h| h.join("Library/Application Support"))
    } else {
        xdg.or_else(|| home.map(|h| h.join(".config")))
    };
    base.unwrap_or_else(|| PathBuf::from(".")).join("archonui")
}

/// What `load` read from the config dir, or migrated from the old settings file.
pub struct Loaded {
    pub settings: AppSettings,
    pub workspace: Workspace,
    pub projects: Vec<Project>,
}

/// Reads the config dir. If it has no settings yet, migrates `legacy_settings` (the old UI's file)
/// and writes the migrated settings and workspace, so the migration runs once. A file that fails
/// to parse is moved aside as `*.json.corrupt` and its defaults are used.
pub fn load(config: &Path, legacy_settings: &Path) -> Loaded {
    // Each file loads on its own; the old settings are migrated only on a true first run.
    let any = [SETTINGS_FILE, WORKSPACE_FILE, PROJECTS_FILE].iter().any(|f| config.join(f).exists());
    if any {
        let mut settings: AppSettings = read_or_default(&config.join(SETTINGS_FILE));
        settings.normalize();
        let mut workspace: Workspace = read_or_default(&config.join(WORKSPACE_FILE));
        workspace.normalize();
        let projects = read_or_default(&config.join(PROJECTS_FILE));
        return Loaded { settings, workspace, projects };
    }
    let old = fs::read(legacy_settings).ok().and_then(|b| serde_json::from_slice::<store::Settings>(&b).ok());
    let Some(old) = old else {
        return Loaded { settings: AppSettings::default(), workspace: Workspace::default(), projects: Vec::new() };
    };
    let (mut settings, mut workspace) = migrate(&old);
    settings.normalize();
    workspace.normalize();
    for (name, bytes) in [
        (SETTINGS_FILE, to_json(&settings)),
        (WORKSPACE_FILE, to_json(&workspace)),
    ] {
        if let Err(e) = write_atomic(&config.join(name), &bytes) {
            eprintln!("could not save migrated {name}: {e}");
        }
    }
    Loaded { settings, workspace, projects: Vec::new() }
}

/// Settings and panel state from the old `store::Settings` file.
pub fn migrate(old: &store::Settings) -> (AppSettings, Workspace) {
    let settings = AppSettings {
        pinned_models: old.pinned_models.clone(),
        default_folder: old.default_folder.clone(),
        default_harness: old.default_agent.clone(),
        harness_defaults: old.agent_defaults.clone(),
        harnesses: old.harnesses.clone(),
        search_engine: old.search_engine,
        auto_open_preview: old.auto_open_preview,
        ide: old.ide,
        ide_autostart: old.ide_autostart,
        code_server_path: old.code_server_path.clone(),
        use_tmux: old.use_tmux,
        terminal_font_size: old.terminal_font_size,
        ..AppSettings::default()
    };
    let workspace = Workspace {
        right_open: !old.hide_panel,
        right_tab: match old.panel_tab {
            // The Details tab is now a popover on the tile header, so the browser stands in for it.
            PanelTab::Preview | PanelTab::Details => RightTab::Browser,
            PanelTab::Ide => RightTab::Ide,
            PanelTab::Git => RightTab::Git,
            PanelTab::Terminal => RightTab::Terminal,
        },
        right_width: old.panel_width.unwrap_or(RIGHT_WIDTH_DEFAULT),
        ..Workspace::default()
    };
    (settings, workspace)
}

/// The config file at `path` as `T`. Missing files give the default. A file that doesn't parse is
/// copied aside as `*.json.corrupt`, and every field (or list item) that still parses is kept, so one
/// bad value, such as an enum a newer build wrote, doesn't reset the whole file.
fn read_or_default<T: DeserializeOwned + Serialize + Default>(path: &Path) -> T {
    let Ok(bytes) = fs::read(path) else { return T::default() };
    serde_json::from_slice(&bytes).unwrap_or_else(|e| {
        eprintln!("ignoring {}: {e}", path.display());
        let _ = fs::copy(path, path.with_extension("json.corrupt"));
        serde_json::from_slice(&bytes).ok().and_then(salvage).unwrap_or_default()
    })
}

/// `value` as `T`, leaving out the object fields or array items that don't parse.
fn salvage<T: DeserializeOwned + Serialize + Default>(value: serde_json::Value) -> Option<T> {
    use serde_json::Value;
    match value {
        Value::Object(fields) => {
            // Start from what an empty file gives, so fields with their own serde defaults keep them.
            let empty = serde_json::from_value::<T>(Value::Object(Default::default())).unwrap_or_default();
            let Value::Object(mut kept) = serde_json::to_value(empty).ok()? else { return None };
            for (key, field) in fields {
                let previous = kept.insert(key.clone(), field);
                if serde_json::from_value::<T>(Value::Object(kept.clone())).is_err() {
                    match previous {
                        Some(p) => kept.insert(key, p),
                        None => kept.remove(&key),
                    };
                }
            }
            serde_json::from_value(Value::Object(kept)).ok()
        }
        Value::Array(items) => {
            let kept: Vec<Value> = items
                .into_iter()
                .filter(|item| serde_json::from_value::<T>(Value::Array(vec![item.clone()])).is_ok())
                .collect();
            serde_json::from_value(Value::Array(kept)).ok()
        }
        _ => None,
    }
}

pub fn to_json<T: Serialize>(value: &T) -> Vec<u8> {
    serde_json::to_vec_pretty(value).expect("app state serialises")
}

/// Queue the settings, workspace and projects files in `config` for writing.
pub fn save_config(saver: &Saver, config: &Path, settings: &AppSettings, workspace: &Workspace, projects: &[Project]) {
    saver.schedule(config.join(SETTINGS_FILE), to_json(settings));
    saver.schedule(config.join(WORKSPACE_FILE), to_json(workspace));
    saver.schedule(config.join(PROJECTS_FILE), to_json(&projects));
}

/// Write `bytes` to `path` through a temp file and a rename, so a crash never leaves half a file.
/// The temp name is unique to this process and write, so two windows or instances never share one.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let dir = path.parent().filter(|d| !d.as_os_str().is_empty());
    if let Some(dir) = dir {
        store::create_private_dir(dir)?;
    }
    let name = path.file_name().map_or_else(|| "file".into(), |n| n.to_string_lossy().into_owned());
    let tmp = path.with_file_name(format!("{name}.{}.{}.tmp", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    let written = (|| {
        let mut file = fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&tmp, path)
    })();
    if written.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    written?;
    // The rename is only durable once the directory is synced too.
    if let Some(dir) = dir {
        let _ = fs::File::open(dir).and_then(|d| d.sync_all());
    }
    Ok(())
}

/// Copy `src` into `dir` under a name not taken yet. Returns the copy's path.
pub fn copy_asset(dir: &Path, src: &Path) -> io::Result<PathBuf> {
    store::create_private_dir(dir)?;
    let name = src.file_name().map_or_else(|| "asset".into(), |n| n.to_string_lossy().into_owned());
    let mut dest = dir.join(&name);
    let mut n = 2;
    while dest.exists() {
        dest = dir.join(format!("{n}-{name}"));
        n += 1;
    }
    fs::copy(src, &dest)?;
    Ok(dest)
}

/// Writes files on a background thread. Writes queued within `DEBOUNCE` of each other are
/// coalesced, so a burst (dragging a divider, typing a name) hits the disk once. A failed write
/// is reported on `errors`, so the UI can tell the user.
pub struct Saver {
    shared: Arc<Shared>,
    errors: smol::channel::Receiver<String>,
}

struct Shared {
    pending: Mutex<BTreeMap<PathBuf, Vec<u8>>>,
    changed: Condvar,
    /// Held while a batch is written, so the worker and `flush` never write at once.
    writing: Mutex<()>,
    /// One line per file that could not be written.
    errors: smol::channel::Sender<String>,
}

impl Saver {
    pub fn new() -> Self {
        let (errors, failed) = smol::channel::unbounded();
        let shared = Arc::new(Shared {
            pending: Mutex::new(BTreeMap::new()),
            changed: Condvar::new(),
            writing: Mutex::new(()),
            errors,
        });
        let worker = shared.clone();
        thread::Builder::new().name("archonui-save".into()).spawn(move || worker.run()).expect("save thread starts");
        Self { shared, errors: failed }
    }

    /// A line for every file that could not be written, such as a read-only config folder.
    pub fn errors(&self) -> smol::channel::Receiver<String> {
        self.errors.clone()
    }

    /// Queue `bytes` for `path`, replacing what was queued for it.
    pub fn schedule(&self, path: PathBuf, bytes: Vec<u8>) {
        self.shared.pending.lock().expect("save queue").insert(path, bytes);
        self.shared.changed.notify_all();
    }

    /// Write everything queued now, on this thread. Call before quitting.
    pub fn flush(&self) {
        self.shared.write_pending();
    }
}

/// Changes still waiting out the debounce are written when the saver goes, such as on quit.
impl Drop for Saver {
    fn drop(&mut self) {
        self.flush();
    }
}

impl Shared {
    fn run(&self) {
        loop {
            let mut pending = self.pending.lock().expect("save queue");
            pending = self.changed.wait_while(pending, |p| p.is_empty()).expect("save queue");
            // Wait for a quiet DEBOUNCE before writing, so a burst of changes is written once, but no
            // longer than MAX_DELAY, so changes that never stop are still written.
            let first = std::time::Instant::now();
            loop {
                let left = MAX_DELAY.saturating_sub(first.elapsed());
                if left.is_zero() {
                    break;
                }
                let (guard, timeout) = self.changed.wait_timeout(pending, DEBOUNCE.min(left)).expect("save queue");
                pending = guard;
                if timeout.timed_out() {
                    break;
                }
            }
            drop(pending);
            self.write_pending();
        }
    }

    fn write_pending(&self) {
        let _writing = self.writing.lock().expect("save writer");
        let batch = std::mem::take(&mut *self.pending.lock().expect("save queue"));
        for (path, bytes) in batch {
            if let Err(e) = write_atomic(&path, &bytes) {
                eprintln!("could not save {}: {e}", path.display());
                let name = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
                let _ = self.errors.send_blocking(format!("Couldn't save {name}: {e}"));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        env::temp_dir().join(format!("archonui-persist-{name}-{}", store::new_id()))
    }

    /// A settings file as the old UI wrote it.
    const OLD_SETTINGS: &str = r#"{
        "pinned_models": {"claude": ["opus", "sonnet"]},
        "hide_details": true,
        "panel_tab": "git",
        "panel_width": 600.0,
        "default_folder": "/home/u/code",
        "default_agent": "codex",
        "agent_defaults": {"codex": {"model": "gpt-5", "effort": "high"}},
        "search_engine": "google",
        "auto_open_preview": false,
        "ide": "vscode",
        "use_tmux": false,
        "terminal_font_size": 14.0,
        "harnesses": {"pi": {"enabled": false, "binary": "/opt/pi", "default_model": "m1"}}
    }"#;

    #[test]
    fn config_dir_honours_the_override_and_platform_fallbacks() {
        let home = Some(PathBuf::from("/home/u"));
        assert_eq!(config_dir_from(Some("/x".into()), None, home.clone(), None), PathBuf::from("/x"));
        if cfg!(target_os = "linux") {
            assert_eq!(config_dir_from(None, None, home.clone(), None), PathBuf::from("/home/u/.config/archonui"));
            let xdg = Some(PathBuf::from("/xdg"));
            assert_eq!(config_dir_from(None, xdg, home, None), PathBuf::from("/xdg/archonui"));
        }
    }

    #[test]
    fn old_settings_migrate_once() {
        let dir = temp("migrate");
        let legacy = dir.join("legacy/settings.json");
        let config = dir.join("config");
        write_atomic(&legacy, OLD_SETTINGS.as_bytes()).unwrap();

        let loaded = load(&config, &legacy);
        let s = &loaded.settings;
        assert_eq!(s.pins("claude"), ["opus", "sonnet"]);
        assert_eq!(s.default_harness.as_deref(), Some("codex"));
        assert_eq!(s.default_folder, Some(PathBuf::from("/home/u/code")));
        assert_eq!(s.harness_defaults["codex"].model.as_deref(), Some("gpt-5"));
        assert!(s.search_engine == store::SearchEngine::Google);
        assert!(!s.auto_open_preview && !s.use_tmux);
        assert!(s.ide == store::IdeKind::VsCode);
        assert_eq!(s.terminal_font_size, 14.);
        assert!(!s.harness("pi").enabled);
        assert_eq!(s.gaps, 14., "settings the old file lacked take defaults");

        let w = &loaded.workspace;
        assert!(!w.right_open);
        assert_eq!(w.right_tab, RightTab::Git);
        assert_eq!(w.right_width, 600.);

        // The migrated files are now the source: removing the old file changes nothing.
        fs::remove_file(&legacy).unwrap();
        let again = load(&config, &legacy);
        assert!(again.settings.search_engine == store::SearchEngine::Google);
        assert_eq!(again.workspace.right_tab, RightTab::Git);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn settings_round_trip_through_the_config_dir() {
        let dir = temp("roundtrip");
        let config = dir.join("config");
        let mut settings = AppSettings { gaps: 22., ..Default::default() };
        settings.app_name = "ember".into();
        let workspace = Workspace { right_width: 700., ..Default::default() };
        write_atomic(&config.join(SETTINGS_FILE), &to_json(&settings)).unwrap();
        write_atomic(&config.join(WORKSPACE_FILE), &to_json(&workspace)).unwrap();
        let project = Project { id: "p1".into(), name: "core".into(), path: "/code/core".into(), created: 5 };
        write_atomic(&config.join(PROJECTS_FILE), &to_json(&vec![project.clone()])).unwrap();

        let loaded = load(&config, &dir.join("missing.json"));
        assert_eq!(loaded.settings.app_name, "ember");
        assert_eq!(loaded.settings.gaps, 22.);
        assert_eq!(loaded.workspace.right_width, 700.);
        assert_eq!(loaded.projects, vec![project]);
        assert!(!config.join("settings.json.tmp").exists(), "no temp file left behind");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn written_config_files_are_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = temp("perms");
        let file = dir.join(SETTINGS_FILE);
        write_atomic(&file, b"{}").unwrap();
        assert_eq!(fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(fs::metadata(&dir).unwrap().permissions().mode() & 0o777, 0o700);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn unreadable_files_are_moved_aside() {
        let dir = temp("corrupt");
        let config = dir.join("config");
        write_atomic(&config.join(SETTINGS_FILE), b"{ not json").unwrap();
        let loaded = load(&config, &dir.join("none.json"));
        assert_eq!(loaded.settings.gaps, 14.);
        assert!(config.join("settings.json.corrupt").exists());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn one_bad_value_keeps_the_rest_of_the_file() {
        let dir = temp("salvage");
        let config = dir.join("config");
        // A theme from a newer build, and a width saved as `null` (a NaN).
        write_atomic(&config.join(SETTINGS_FILE), br#"{"gaps": 22, "theme": "from-the-future", "app_name": "ember"}"#).unwrap();
        write_atomic(&config.join(WORKSPACE_FILE), br#"{"right_width": null, "right_tab": "git"}"#).unwrap();
        let projects = r#"[{"id": "p1", "name": "core", "path": "/c", "created": 1}, {"id": 5}]"#;
        write_atomic(&config.join(PROJECTS_FILE), projects.as_bytes()).unwrap();

        let loaded = load(&config, &dir.join("none.json"));
        assert_eq!((loaded.settings.gaps, loaded.settings.app_name.as_str()), (22., "ember"));
        assert_eq!(loaded.workspace.right_tab, RightTab::Git);
        assert_eq!(loaded.workspace.right_width, RIGHT_WIDTH_DEFAULT);
        assert_eq!(loaded.projects.len(), 1, "the broken project is dropped, the good one kept");
        assert!(config.join("settings.json.corrupt").exists(), "the original is kept aside");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn writes_leave_no_temp_files() {
        let dir = temp("tmpfiles");
        let file = dir.join(WORKSPACE_FILE);
        for i in 0..3 {
            write_atomic(&file, format!("{i}").as_bytes()).unwrap();
        }
        assert_eq!(fs::read(&file).unwrap(), b"2");
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn dropping_the_saver_writes_what_is_queued() {
        let dir = temp("saver-drop");
        let path = dir.join("workspace.json");
        let saver = Saver::new();
        saver.schedule(path.clone(), b"last".to_vec());
        drop(saver);
        assert_eq!(fs::read(&path).unwrap(), b"last");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_change_that_never_stops_is_still_written() {
        let dir = temp("saver-busy");
        let path = dir.join("workspace.json");
        let saver = Saver::new();
        let start = std::time::Instant::now();
        while !path.exists() && start.elapsed() < MAX_DELAY * 3 {
            saver.schedule(path.clone(), b"busy".to_vec());
            thread::sleep(Duration::from_millis(50));
        }
        assert!(path.exists(), "written within MAX_DELAY even though changes kept coming");
        drop(saver);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn saver_coalesces_and_flushes() {
        let dir = temp("saver");
        let path = dir.join("workspace.json");
        let saver = Saver::new();
        saver.schedule(path.clone(), b"first".to_vec());
        saver.schedule(path.clone(), b"second".to_vec());
        thread::sleep(Duration::from_millis(900));
        assert_eq!(fs::read(&path).unwrap(), b"second");

        saver.schedule(path.clone(), b"third".to_vec());
        saver.flush();
        assert_eq!(fs::read(&path).unwrap(), b"third", "flush writes without waiting");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn real_legacy_settings_migrate() {
        let dir = temp("real-legacy");
        let legacy = dir.join("legacy/settings.json");
        let config = dir.join("config");
        let real_json = r#"{
  "pinned_models": {
    "pi": []
  },
  "hide_panel": false,
  "panel_tab": "ide",
  "panel_width": null
}"#;
        write_atomic(&legacy, real_json.as_bytes()).unwrap();
        let loaded = load(&config, &legacy);
        assert!(loaded.workspace.right_open);
        assert_eq!(loaded.workspace.right_tab, RightTab::Ide);
        assert_eq!(loaded.workspace.right_width, RIGHT_WIDTH_DEFAULT);
        assert_eq!(loaded.settings.pins("pi"), Vec::<String>::new());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn saver_reports_write_errors() {
        let dir = temp("saver-err");
        fs::create_dir_all(&dir).unwrap();
        let blocked = dir.join("blocked");
        fs::write(&blocked, b"file").unwrap();
        let target = blocked.join("workspace.json");

        let saver = Saver::new();
        let errors = saver.errors();
        saver.schedule(target, b"test".to_vec());
        saver.flush();

        let err = errors.try_recv().expect("error received");
        assert!(err.contains("Couldn't save workspace.json"), "expected error message, got: {err}");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn copy_asset_avoids_existing_names() {
        let dir = temp("assets");
        fs::create_dir_all(&dir).unwrap();
        let src = dir.join("wall.png");
        fs::write(&src, b"png").unwrap();
        let first = copy_asset(&dir.join("walls"), &src).unwrap();
        let second = copy_asset(&dir.join("walls"), &src).unwrap();
        assert_eq!(first.file_name().unwrap(), "wall.png");
        assert_eq!(second.file_name().unwrap(), "2-wall.png");
        fs::remove_dir_all(dir).unwrap();
    }
}
