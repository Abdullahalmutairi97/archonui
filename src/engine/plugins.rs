//! Extensions: the harnesses' own add-ons (Claude Code plugins, Gemini CLI extensions, Pi packages,
//! OpenCode plugins), listed, installed, removed and toggled through each CLI. Ids are `<harness>:<spec>`.
#![allow(dead_code)]

use super::{agent_name, detect::detect};
use crate::store::load_settings;
use serde_json::Value;
use std::{
    collections::HashSet,
    io::Read,
    os::unix::process::CommandExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{mpsc, Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

#[derive(Clone, Debug, PartialEq)]
pub struct Extension {
    /// Stable id: `<harness>:<spec>`, e.g. `claude:design@synced`, `gemini:conductor` or `pi:npm:pi-web-access`.
    pub id: String,
    pub name: String,
    pub author: String,
    pub version: String,
    pub description: String,
    /// Harness id it belongs to.
    pub harness: String,
    pub installed: bool,
    pub enabled: bool,
}

/// Longest a listing may take. Claude's catalog runs to thousands of entries.
const LIST_TIMEOUT: Duration = Duration::from_secs(60);
/// Longest an install may take, since it can clone a repository.
const INSTALL_TIMEOUT: Duration = Duration::from_secs(300);
/// Longest an uninstall, enable or disable may take.
const CHANGE_TIMEOUT: Duration = Duration::from_secs(60);
const OPENCODE_NO_EDITS: &str = "OpenCode lists its plugins from its config, which ArchonUI does not edit; add them in OpenCode";

/// Installed extensions across harnesses. Blocking.
pub fn installed() -> Vec<Extension> {
    all().into_iter().filter(|e| e.installed).collect()
}

/// Extensions available to install (from the harnesses' marketplaces/registries). Blocking.
/// Gemini, Pi and OpenCode have no catalog command, so only Claude Code's marketplaces appear.
pub fn available() -> Vec<Extension> {
    all().into_iter().filter(|e| !e.installed).collect()
}

/// Installs the extension `id`. Blocking. For Gemini the spec is the source to install from,
/// a git URL or a local path.
pub fn install(id: &str) -> Result<(), String> {
    let (harness, spec) = split_id(id)?;
    let spec = safe_spec(spec)?;
    let path = binary(harness)?;
    match harness {
        // Claude's CLI accepts `--`; Pi's rejects it ("Unknown option --") and Gemini's drops what follows it.
        "claude" => run(&path, &["plugin", "install", "--", spec], INSTALL_TIMEOUT).map(drop),
        "gemini" => gemini_install(&path, &home()?, spec),
        "pi" => run(&path, &["install", spec], INSTALL_TIMEOUT).map(drop),
        _ => Err(no_edits(harness)),
    }
}

/// Uninstalls the extension `id`. Blocking.
pub fn remove(id: &str) -> Result<(), String> {
    let (harness, spec) = split_id(id)?;
    let spec = safe_spec(spec)?;
    let path = binary(harness)?;
    match harness {
        "claude" => run(&path, &["plugin", "uninstall", "--", spec], CHANGE_TIMEOUT).map(drop),
        "gemini" => gemini_uninstall(&path, &home()?, spec),
        "pi" => run(&path, &["remove", spec], CHANGE_TIMEOUT).map(drop),
        _ => Err(no_edits(harness)),
    }
}

/// Turns the installed extension `id` on or off. Blocking. Gemini extensions are switched in the
/// user scope, which a workspace override can still beat. Pi packages have no off switch;
/// turning one on is a no-op and turning one off is an error.
pub fn set_enabled(id: &str, enabled: bool) -> Result<(), String> {
    let (harness, spec) = split_id(id)?;
    let spec = safe_spec(spec)?;
    let path = binary(harness)?;
    match (harness, enabled) {
        ("claude", _) => {
            let verb = if enabled { "enable" } else { "disable" };
            run(&path, &["plugin", verb, "--", spec], CHANGE_TIMEOUT).map(drop)
        }
        ("gemini", _) => gemini_set_enabled(&path, &home()?, spec, enabled),
        ("pi", true) => Ok(()),
        ("pi", false) => Err("Pi packages cannot be turned off; remove the package instead".into()),
        _ => Err(no_edits(harness)),
    }
}

/// Why `harness`'s extensions cannot be changed from ArchonUI.
fn no_edits(harness: &str) -> String {
    match harness {
        "opencode" => OPENCODE_NO_EDITS.into(),
        _ => format!("ArchonUI does not manage {} extensions", agent_name(harness)),
    }
}

/// Every extension the detected harnesses list, installed or not.
fn all() -> Vec<Extension> {
    detect(&load_settings())
        .iter()
        .flat_map(|agent| match agent.harness.id() {
            "claude" => claude_extensions(&agent.path),
            "gemini" => home().map_or_else(|_| Vec::new(), |home| gemini_extensions(&agent.path, &home)),
            "pi" => run(&agent.path, &["list"], LIST_TIMEOUT).map_or_else(|_| Vec::new(), |t| pi_installed(&t)),
            "opencode" => run(&agent.path, &["debug", "config"], LIST_TIMEOUT)
                .ok()
                .and_then(|t| serde_json::from_str::<Value>(&t).ok())
                .map_or_else(Vec::new, |v| opencode_installed(&v)),
            _ => Vec::new(),
        })
        .collect()
}

/// The harness id and spec in `id`, split at the first colon.
fn split_id(id: &str) -> Result<(&str, &str), String> {
    id.split_once(':').filter(|(_, spec)| !spec.is_empty()).ok_or_else(|| format!("not an extension id: {id}"))
}

/// Rejects a spec that the harness CLI would read as a flag rather than a plugin.
fn safe_spec(spec: &str) -> Result<&str, String> {
    if spec.starts_with('-') {
        Err(format!("refusing an extension id that starts with '-': {spec}"))
    } else {
        Ok(spec)
    }
}

/// The detected CLI for `harness`, or why it cannot be used.
fn binary(harness: &str) -> Result<PathBuf, String> {
    detect(&load_settings())
        .into_iter()
        .find(|a| a.harness.id() == harness)
        .map(|a| a.path)
        .ok_or_else(|| format!("{} is not installed", agent_name(harness)))
}

/// Claude Code's installed and catalog plugins from one `plugin list --json --available`.
fn claude_extensions(path: &Path) -> Vec<Extension> {
    run(path, &["plugin", "list", "--json", "--available"], LIST_TIMEOUT)
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .map_or_else(Vec::new, |v| claude_extensions_of(&v))
}

/// `claude_extensions` with the listing parsed. Catalog entries that are already installed are left out.
fn claude_extensions_of(v: &Value) -> Vec<Extension> {
    let installed: HashSet<&str> = v["installed"].as_array().into_iter().flatten().filter_map(|p| p["id"].as_str()).collect();
    let mut out: Vec<Extension> = v["installed"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| {
            let spec = p["id"].as_str()?;
            let manifest = read_manifest(Path::new(p["installPath"].as_str().unwrap_or("")));
            Some(Extension {
                id: format!("claude:{spec}"),
                name: manifest_text(&manifest, "name").unwrap_or_else(|| spec.split('@').next().unwrap_or(spec).into()),
                author: manifest_author(&manifest),
                version: p["version"].as_str().unwrap_or_default().into(),
                description: manifest_text(&manifest, "description").unwrap_or_default(),
                harness: "claude".into(),
                installed: true,
                enabled: p["enabled"] == true,
            })
        })
        .collect();
    out.extend(
        v["available"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|p| {
                let spec = p["pluginId"].as_str()?;
                (!installed.contains(spec)).then(|| Extension {
                    id: format!("claude:{spec}"),
                    name: p["name"].as_str().unwrap_or(spec).into(),
                    author: String::new(),
                    version: String::new(),
                    description: crate::engine::discover::short(p["description"].as_str().unwrap_or("")),
                    harness: "claude".into(),
                    installed: false,
                    enabled: false,
                })
            }),
    );
    out
}

/// The user's home directory, where Gemini keeps its extensions.
fn home() -> Result<PathBuf, String> {
    std::env::var_os("HOME").map(PathBuf::from).ok_or_else(|| "HOME is not set".to_string())
}

/// Runs `gemini args` for the user whose home is `home`, from that home, and returns stdout and
/// stderr together, since Gemini prints its results to stderr. Running from the home matters for
/// the listing: Gemini reports an extension active for its working directory, and the user scope
/// is the home and everything below it.
fn gemini_run(path: &Path, home: &Path, args: &[&str], timeout: Duration) -> Result<String, String> {
    let mut command = Command::new(path);
    command.args(args).current_dir(home).env("HOME", home);
    run_command(command, timeout).map(|(out, err)| out + &err)
}

/// `gemini extensions install`, without its two questions: `--consent` accepts the third-party
/// warning, and trusting the workspace for this one run skips the "Do you trust this folder?"
/// prompt a local path raises (it would wait on stdin until the timeout). The folder is not added to
/// Gemini's trusted folders. `--skip-settings` leaves an extension's settings unasked; they can
/// be set later with `gemini extensions config`.
fn gemini_install(path: &Path, home: &Path, source: &str) -> Result<(), String> {
    let mut command = Command::new(path);
    command
        .args(["extensions", "install", "--consent", "--skip-settings", source])
        .current_dir(home)
        .env("HOME", home)
        .env("GEMINI_CLI_TRUST_WORKSPACE", "true");
    run_command(command, INSTALL_TIMEOUT).map(drop)
}

/// Uninstalls the Gemini extension `name`.
fn gemini_uninstall(path: &Path, home: &Path, name: &str) -> Result<(), String> {
    gemini_run(path, home, &["extensions", "uninstall", name], CHANGE_TIMEOUT).map(drop)
}

/// Turns a Gemini extension on or off in the user scope.
fn gemini_set_enabled(path: &Path, home: &Path, name: &str, enabled: bool) -> Result<(), String> {
    let verb = if enabled { "enable" } else { "disable" };
    gemini_run(path, home, &["extensions", verb, "--scope", "user", name], CHANGE_TIMEOUT).map(drop)
}

/// Gemini's installed extensions from `gemini extensions list --output-format json`.
fn gemini_extensions(path: &Path, home: &Path) -> Vec<Extension> {
    gemini_run(path, home, &["extensions", "list", "--output-format", "json"], LIST_TIMEOUT)
        .map_or_else(|_| Vec::new(), |t| gemini_installed(&t))
}

/// The extensions in Gemini's JSON listing, which starts on the first line that opens an array
/// (other lines, like warnings, may come before it). The listing has no description or author, so
/// those come from each extension's `gemini-extension.json`.
///
/// `isActive` is the state for the directory the listing ran in, which `gemini_run` makes the home:
/// that is the user scope. A workspace override for some other folder does not show here, and
/// one for the home itself counts, as Gemini would count it there.
fn gemini_installed(text: &str) -> Vec<Extension> {
    let Some(start) = text.lines().position(|line| line.trim_start().starts_with('[')) else {
        return Vec::new();
    };
    let json: String = text.lines().skip(start).collect::<Vec<_>>().join("\n");
    let Some(Ok(Value::Array(list))) = serde_json::Deserializer::from_str(&json).into_iter::<Value>().next() else {
        return Vec::new();
    };
    list.iter()
        .filter_map(|e| {
            let name = e["name"].as_str().filter(|n| !n.is_empty())?;
            let manifest = read_json(Path::new(e["path"].as_str().unwrap_or("")), "gemini-extension.json");
            Some(Extension {
                id: format!("gemini:{name}"),
                name: name.into(),
                author: manifest_author(&manifest),
                version: e["version"].as_str().unwrap_or_default().into(),
                description: manifest_text(&manifest, "description").unwrap_or_default(),
                harness: "gemini".into(),
                installed: true,
                enabled: e["isActive"] != false,
            })
        })
        .collect()
}

/// `pi list`: each package is a line indented two spaces, with its path indented below it.
/// Pi marks a package it cannot load with a `(filtered)` suffix, which is dropped here.
fn pi_installed(text: &str) -> Vec<Extension> {
    text.lines()
        .filter(|line| line.starts_with("  ") && !line.starts_with("   "))
        .map(|line| line.trim().trim_end_matches(" (filtered)"))
        .map(|source| Extension {
            id: format!("pi:{source}"),
            name: source.into(),
            author: String::new(),
            version: String::new(),
            description: String::new(),
            harness: "pi".into(),
            installed: true,
            enabled: true,
        })
        .collect()
}

/// The `plugin` array in OpenCode's merged config. Those plugins are all active.
fn opencode_installed(config: &Value) -> Vec<Extension> {
    config["plugin"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(|spec| Extension {
            id: format!("opencode:{spec}"),
            name: spec.into(),
            author: String::new(),
            version: String::new(),
            description: String::new(),
            harness: "opencode".into(),
            installed: true,
            enabled: true,
        })
        .collect()
}

fn read_manifest(install_path: &Path) -> Value {
    read_json(install_path, ".claude-plugin/plugin.json")
}

/// The JSON file at `file` under `dir`, or null.
fn read_json(dir: &Path, file: &str) -> Value {
    // An entry without an install path must not read a manifest from the working directory.
    if dir.as_os_str().is_empty() {
        return Value::Null;
    }
    std::fs::read_to_string(dir.join(file))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or(Value::Null)
}

fn manifest_text(manifest: &Value, key: &str) -> Option<String> {
    manifest[key].as_str().filter(|s| !s.is_empty()).map(String::from)
}

/// The manifest's author, which is either a name or an object with one.
fn manifest_author(manifest: &Value) -> String {
    manifest["author"]["name"].as_str().or_else(|| manifest["author"].as_str()).unwrap_or_default().into()
}

/// Runs `path args` and returns stdout. A failed run gives its stderr, or its exit status when
/// stderr is empty. The process group is killed after `timeout`.
fn run(path: &Path, args: &[&str], timeout: Duration) -> Result<String, String> {
    let mut command = Command::new(path);
    command.args(args);
    run_command(command, timeout).map(|(out, _)| out)
}

/// `run` for a prepared command, giving stdout and stderr of a successful run.
fn run_command(mut command: Command, timeout: Duration) -> Result<(String, String), String> {
    let path = PathBuf::from(command.get_program());
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn()
        .map_err(|e| format!("could not start {}: {e}", path.display()))?;
    // Both pipes are read on their own threads, so a CLI that writes a lot cannot block on a full pipe.
    let out = drain(child.stdout.take());
    let err = drain(child.stderr.take());
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(50)),
            _ => {
                unsafe { libc::kill(-(child.id() as i32), libc::SIGKILL) };
                let _ = child.wait();
                return Err(format!("timed out after {} s", timeout.as_secs()));
            }
        }
    };
    // A process the CLI left running (a daemon, a worker) can hold the pipes open after the CLI exits;
    // its output is all written by then, so wait only briefly for the end of it.
    let grace = Instant::now() + DRAIN_GRACE;
    let (out, err) = (out.finish(grace), err.finish(grace));
    if status.success() {
        return Ok((out, err));
    }
    let err = err.trim();
    Err(if err.is_empty() { format!("exited with {status}") } else { err.to_string() })
}

/// How long to keep reading a CLI's output after it exits.
const DRAIN_GRACE: Duration = Duration::from_secs(2);

/// A pipe being read on its own thread.
struct Drain {
    text: Arc<Mutex<Vec<u8>>>,
    done: mpsc::Receiver<()>,
}

impl Drain {
    /// What was read, once the pipe closes or `deadline` passes. Invalid UTF-8 is replaced, not dropped.
    fn finish(self, deadline: Instant) -> String {
        let _ = self.done.recv_timeout(deadline.saturating_duration_since(Instant::now()));
        String::from_utf8_lossy(&self.text.lock().unwrap_or_else(|e| e.into_inner())).into_owned()
    }
}

fn drain<R: Read + Send + 'static>(pipe: Option<R>) -> Drain {
    let text = Arc::new(Mutex::new(Vec::new()));
    let (tx, done) = mpsc::channel();
    let sink = text.clone();
    thread::spawn(move || {
        if let Some(mut pipe) = pipe {
            let mut buf = [0u8; 8192];
            loop {
                match pipe.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => sink.lock().unwrap_or_else(|e| e.into_inner()).extend_from_slice(&buf[..n]),
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(_) => break,
                }
            }
        }
        let _ = tx.send(());
    });
    Drain { text, done }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pi_packages_come_from_the_indented_source_lines() {
        let text = "User packages:\n  /opt/pkg/runtime/node_modules/pi-mcp-adapter (filtered)\n    /opt/pkg/runtime/node_modules/pi-mcp-adapter\n  npm:pi-web-access\n    /home/u/.pi/agent/npm/node_modules/pi-web-access\n";
        let found: Vec<String> = pi_installed(text).into_iter().map(|e| e.id).collect();
        assert_eq!(found, ["pi:/opt/pkg/runtime/node_modules/pi-mcp-adapter", "pi:npm:pi-web-access"]);
        assert!(pi_installed("").is_empty());
    }

    #[test]
    fn claude_listing_splits_installed_from_catalog() {
        let listing = r#"{
            "installed": [{"id": "design@synced", "version": "1.2", "enabled": false, "installPath": "/nonexistent/design"}],
            "available": [
                {"pluginId": "design@synced", "name": "design", "description": "Already here."},
                {"pluginId": "lint@market", "name": "lint", "description": "Lints code. More."}
            ]
        }"#;
        let v: Value = serde_json::from_str(listing).unwrap();
        let found = claude_extensions_of(&v);
        let rows: Vec<(&str, bool, bool)> = found.iter().map(|e| (e.id.as_str(), e.installed, e.enabled)).collect();
        assert_eq!(rows, [("claude:design@synced", true, false), ("claude:lint@market", false, false)]);
        assert_eq!(found[0].name, "design");
        assert_eq!(found[1].description, "Lints code.");
    }

    #[test]
    fn gemini_listing_reads_the_json_and_each_manifest() {
        let dir = std::env::temp_dir().join(format!("archonui-gemini-list-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("gemini-extension.json"),
            r#"{"name": "conductor", "version": "0.4.0", "description": "Plans tracks.", "author": {"name": "Google"}}"#,
        )
        .unwrap();
        let text = format!(
            "Loaded cached credentials.\n[\n  {{\"name\": \"conductor\", \"version\": \"0.4.0\", \"path\": {:?}, \"isActive\": true}},\n  {{\"name\": \"off\", \"version\": \"1.0.0\", \"path\": \"/nonexistent/off\", \"isActive\": false}},\n  {{\"version\": \"9\"}}\n]\n",
            dir.to_str().unwrap()
        );
        let found = gemini_installed(&text);
        std::fs::remove_dir_all(&dir).unwrap();
        let rows: Vec<(&str, &str, bool, bool)> =
            found.iter().map(|e| (e.id.as_str(), e.version.as_str(), e.installed, e.enabled)).collect();
        assert_eq!(rows, [("gemini:conductor", "0.4.0", true, true), ("gemini:off", "1.0.0", true, false)]);
        assert_eq!((found[0].description.as_str(), found[0].author.as_str()), ("Plans tracks.", "Google"));
        assert_eq!((found[1].description.as_str(), found[1].author.as_str()), ("", ""));
        assert!(gemini_installed("[]\n").is_empty());
        assert!(gemini_installed("No extensions installed.\n").is_empty());
        assert!(gemini_installed("[ not json").is_empty());
    }

    #[test]
    fn harnesses_without_extension_edits_say_why() {
        assert_eq!(no_edits("opencode"), OPENCODE_NO_EDITS);
        assert!(no_edits("codex").contains("does not manage"), "{}", no_edits("codex"));
    }

    /// Drives the real Gemini CLI in a throwaway home: install a local extension, turn it off and
    /// on, then uninstall it. Run with `cargo test gemini_cli_round_trip -- --ignored`.
    #[test]
    #[ignore = "needs the Gemini CLI"]
    fn gemini_cli_round_trip() {
        let path = binary("gemini").expect("Gemini CLI");
        let home = std::env::temp_dir().join(format!("archonui-gemini-home-{}", std::process::id()));
        let source = home.join("src/archon-test-ext");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(
            source.join("gemini-extension.json"),
            r#"{"name": "archon-test-ext", "version": "0.0.1", "description": "ArchonUI test extension"}"#,
        )
        .unwrap();
        let state = || -> Vec<(String, String, String, bool)> {
            gemini_extensions(&path, &home).into_iter().map(|e| (e.id, e.version, e.description, e.enabled)).collect()
        };
        let row = |enabled| vec![("gemini:archon-test-ext".into(), "0.0.1".into(), "ArchonUI test extension".into(), enabled)];

        gemini_install(&path, &home, source.to_str().unwrap()).unwrap();
        assert_eq!(state(), row(true));
        gemini_set_enabled(&path, &home, "archon-test-ext", false).unwrap();
        assert_eq!(state(), row(false));
        gemini_set_enabled(&path, &home, "archon-test-ext", true).unwrap();
        assert_eq!(state(), row(true));
        gemini_uninstall(&path, &home, "archon-test-ext").unwrap();
        assert_eq!(state(), []);
        std::fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn split_id_splits_at_the_first_colon() {
        assert_eq!(split_id("pi:npm:pi-web-access"), Ok(("pi", "npm:pi-web-access")));
        assert!(split_id("claude").is_err());
        assert!(split_id("claude:").is_err());
    }

    #[test]
    fn specs_that_look_like_flags_are_refused() {
        assert_eq!(safe_spec("design@synced"), Ok("design@synced"));
        assert!(safe_spec("--config").is_err());
        assert!(safe_spec("-y").is_err());
    }

    #[test]
    fn run_returns_stdout_or_stderr() {
        let sh = Path::new("/bin/sh");
        assert_eq!(run(sh, &["-c", "printf ok"], Duration::from_secs(10)).as_deref(), Ok("ok"));
        assert_eq!(run(sh, &["-c", "echo nope >&2; exit 3"], Duration::from_secs(10)), Err("nope".into()));
        assert!(run(sh, &["-c", "exit 3"], Duration::from_secs(10)).unwrap_err().starts_with("exited with"));
    }

    #[test]
    fn run_does_not_wait_for_a_leftover_process_holding_the_pipe() {
        let started = Instant::now();
        let out = run(Path::new("/bin/sh"), &["-c", "sleep 4 & printf ok"], Duration::from_secs(30));
        assert_eq!(out.as_deref(), Ok("ok"));
        assert!(started.elapsed() < Duration::from_secs(4), "{:?}", started.elapsed());
    }

    #[test]
    fn run_keeps_output_that_is_not_utf8() {
        let out = run(Path::new("/bin/sh"), &["-c", r"printf 'a\377b'"], Duration::from_secs(10)).unwrap();
        assert_eq!(out, "a\u{fffd}b");
    }

    #[test]
    fn run_gives_up_at_the_timeout() {
        let started = Instant::now();
        let err = run(Path::new("/bin/sh"), &["-c", "sleep 30"], Duration::from_secs(1)).unwrap_err();
        assert!(err.starts_with("timed out"), "{err}");
        assert!(started.elapsed() < Duration::from_secs(10));
    }
}
