use super::harness::{AuthStatus, Harness, HARNESSES};
use crate::store::Settings;
use std::{
    env,
    io::Read,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

#[derive(Clone)]
pub struct DetectedAgent {
    pub harness: &'static dyn Harness,
    pub path: PathBuf,
    pub version: String,
    /// Off in settings: existing chats keep working, new ones don't offer it.
    pub enabled: bool,
    pub auth: AuthStatus,
}

/// Find every known harness installed on this machine, at its configured
/// binary if one is set. Blocking; probes versions in parallel with a short
/// timeout each.
pub fn detect(settings: &Settings) -> Vec<DetectedAgent> {
    let dirs = search_dirs();
    let probes: Vec<_> = HARNESSES
        .iter()
        .filter_map(|&harness| {
            let config = settings.harness(harness.id());
            let path = match &config.binary {
                Some(p) => is_executable(p).then(|| p.clone())?,
                None => dirs.iter().map(|d| d.join(harness.binary())).find(|p| is_executable(p))?,
            };
            let (enabled, auth) = (config.enabled, harness.auth_status());
            Some(thread::spawn(move || {
                let version = probe_version(&path);
                DetectedAgent { harness, path, version, enabled, auth }
            }))
        })
        .collect();
    probes.into_iter().filter_map(|h| h.join().ok()).collect()
}

/// `PATH` plus common per-user install dirs, since a desktop launcher often
/// starts apps with a shorter `PATH` than an interactive shell.
fn search_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> =
        env::var_os("PATH").map(|p| env::split_paths(&p).collect()).unwrap_or_default();
    if let Some(home) = env::var_os("HOME").map(PathBuf::from) {
        for sub in [
            ".local/bin",
            ".cargo/bin",
            ".npm-global/bin",
            ".bun/bin",
            ".opencode/bin",
            ".local/share/pi-node/current/bin",
            ".pi/agent/bin",
        ] {
            dirs.push(home.join(sub));
        }
    }
    dirs.push("/usr/local/bin".into());
    dirs
}

fn is_executable(p: &Path) -> bool {
    p.metadata().is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

fn probe_version(path: &Path) -> String {
    let unknown = || "unknown version".to_string();
    let Ok(mut child) = Command::new(path)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    else {
        return unknown();
    };
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(50)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return unknown();
            }
        }
    }
    let mut text = String::new();
    if let Some(mut out) = child.stdout.take() {
        let _ = out.read_to_string(&mut text);
    }
    // Pull "1.2.3" out of lines like "codex-cli 0.154.0" or "Agent v0.21.5+g9241b0c".
    text.split_whitespace()
        .map(|t| t.trim_start_matches('v'))
        .find(|t| t.starts_with(|c: char| c.is_ascii_digit()) && t.contains('.'))
        .map(|t| t.split('+').next().unwrap_or(t).to_string())
        .unwrap_or_else(unknown)
}

