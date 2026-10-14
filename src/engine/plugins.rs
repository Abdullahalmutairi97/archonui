//! Extensions: the harnesses' own add-ons (Claude Code plugins, Pi packages, OpenCode plugins),
//! listed, installed, removed and toggled through each CLI. Ids are `<harness>:<spec>`.
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
    /// Stable id: `<harness>:<spec>`, e.g. `claude:design@synced` or `pi:npm:pi-web-access`.
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
/// Pi and OpenCode have no catalog command, so only Claude Code's marketplaces appear.
pub fn available() -> Vec<Extension> {
    all().into_iter().filter(|e| !e.installed).collect()
}

/// Installs the extension `id`. Blocking.
pub fn install(id: &str) -> Result<(), String> {
    let (harness, spec) = split_id(id)?;
    let spec = safe_spec(spec)?;
    let path = binary(harness)?;
    match harness {
        // Claude's CLI accepts `--`; Pi's rejects it ("Unknown option --").
        "claude" => run(&path, &["plugin", "install", "--", spec], INSTALL_TIMEOUT).map(drop),
        "pi" => run(&path, &["install", spec], INSTALL_TIMEOUT).map(drop),
        _ => Err(OPENCODE_NO_EDITS.into()),
    }
}

/// Uninstalls the extension `id`. Blocking.
pub fn remove(id: &str) -> Result<(), String> {
    let (harness, spec) = split_id(id)?;
    let spec = safe_spec(spec)?;
    let path = binary(harness)?;
    match harness {
        "claude" => run(&path, &["plugin", "uninstall", "--", spec], CHANGE_TIMEOUT).map(drop),
        "pi" => run(&path, &["remove", spec], CHANGE_TIMEOUT).map(drop),
        _ => Err(OPENCODE_NO_EDITS.into()),
    }
}

/// Turns the installed extension `id` on or off. Blocking. Pi packages have no off switch;
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
        ("pi", true) => Ok(()),
        ("pi", false) => Err("Pi packages cannot be turned off; remove the package instead".into()),
        _ => Err(OPENCODE_NO_EDITS.into()),
    }
}

/// Every extension the detected harnesses list, installed or not.
fn all() -> Vec<Extension> {
    detect(&load_settings())
        .iter()
        .flat_map(|agent| match agent.harness.id() {
            "claude" => claude_extensions(&agent.path),
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
    std::fs::read_to_string(install_path.join(".claude-plugin/plugin.json"))
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
    let mut child = Command::new(path)
        .args(args)
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
        return Ok(out);
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
