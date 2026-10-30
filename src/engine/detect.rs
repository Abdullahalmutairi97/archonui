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
    let mut dirs = path_dirs();
    if let Some(home) = env::var_os("HOME").map(PathBuf::from) {
        push_missing(&mut dirs, user_bin_dirs(&home));
    }
    push_missing(&mut dirs, [PathBuf::from("/usr/local/bin")]);
    dirs
}

/// The dirs on `PATH`, in order.
fn path_dirs() -> Vec<PathBuf> {
    env::var_os("PATH").map(|p| env::split_paths(&p).collect()).unwrap_or_default()
}

/// Appends each of `extra` that `dirs` doesn't already have.
fn push_missing(dirs: &mut Vec<PathBuf>, extra: impl IntoIterator<Item = PathBuf>) {
    for dir in extra {
        if !dirs.contains(&dir) {
            dirs.push(dir);
        }
    }
}

/// Where agent CLIs and the `node` they run on are installed per user: npm/bun/pnpm/volta global
/// bins, the newest nvm and fnm Node versions, mise/asdf shims, the npm `prefix` from `~/.npmrc`,
/// and the agents' own installers. Only dirs that exist, in that order.
pub fn user_bin_dirs(home: &Path) -> Vec<PathBuf> {
    let var = |k: &str| env::var_os(k).map(PathBuf::from).filter(|p| p.is_absolute());
    let data = var("XDG_DATA_HOME").unwrap_or_else(|| home.join(".local/share"));
    bin_dirs_in(home, &data, var("NVM_DIR"), var("FNM_DIR"))
}

fn bin_dirs_in(home: &Path, data: &Path, nvm_dir: Option<PathBuf>, fnm_dir: Option<PathBuf>) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = [
        ".local/bin",
        "bin",
        ".cargo/bin",
        ".npm-global/bin",
        ".npm-packages/bin",
        ".bun/bin",
        ".volta/bin",
        ".opencode/bin",
        ".claude/local",
        ".local/share/pi-node/current/bin",
        ".pi/agent/bin",
        ".yarn/bin",
        ".asdf/shims",
    ]
    .iter()
    .map(|sub| home.join(sub))
    .collect();
    dirs.push(data.join("pnpm"));
    dirs.push(data.join("mise/shims"));
    if let Some(prefix) = npmrc_prefix(home) {
        dirs.push(prefix.join("bin"));
    }
    // nvm: every installed Node, newest first (the CLIs live in the Node they were installed with).
    let nvm = nvm_dir.unwrap_or_else(|| home.join(".nvm"));
    dirs.extend(newest_first(&nvm.join("versions/node")).into_iter().map(|v| v.join("bin")));
    // fnm: its default alias, then every installed Node, newest first.
    for fnm in [fnm_dir, Some(data.join("fnm")), Some(home.join(".fnm"))].into_iter().flatten() {
        dirs.push(fnm.join("aliases/default/bin"));
        dirs.extend(newest_first(&fnm.join("node-versions")).into_iter().map(|v| v.join("installation/bin")));
    }
    let mut unique = Vec::new();
    push_missing(&mut unique, dirs.into_iter().filter(|d| d.is_dir()));
    unique
}

/// The subdirectories of `dir` named like versions (`v22.11.0`), newest first.
fn newest_first(dir: &Path) -> Vec<PathBuf> {
    let key = |p: &PathBuf| -> Vec<u64> {
        let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        name.trim_start_matches('v').split('.').map(|n| n.parse().unwrap_or(0)).collect()
    };
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect())
        .unwrap_or_default();
    found.sort_by_key(|p| std::cmp::Reverse(key(p)));
    found
}

/// The `prefix=` line of `~/.npmrc` (`npm config set prefix ~/.npm-global` writes it), `~` expanded.
fn npmrc_prefix(home: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(home.join(".npmrc")).ok()?;
    let value = text.lines().find_map(|l| l.trim().strip_prefix("prefix"))?.trim_start().strip_prefix('=')?.trim();
    let value = value.trim_matches('"');
    let path = match value.strip_prefix("~/") {
        Some(rest) => home.join(rest),
        None => PathBuf::from(value.replace("${HOME}", &home.to_string_lossy()).replace("$HOME", &home.to_string_lossy())),
    };
    path.is_absolute().then_some(path)
}

/// Adds the per-user install dirs (`user_bin_dirs`) to the end of this process's `PATH`, so an app
/// started from a desktop launcher finds the same agents as a terminal, and the agents it starts find
/// `node`, `npx` and each other. Dirs already on `PATH` keep their place. Call once at startup,
/// before any threads.
pub fn extend_path() {
    let Some(home) = env::var_os("HOME").map(PathBuf::from) else { return };
    let mut dirs = path_dirs();
    let before = dirs.len();
    push_missing(&mut dirs, user_bin_dirs(&home));
    if dirs.len() != before {
        if let Ok(path) = env::join_paths(dirs) {
            env::set_var("PATH", path);
        }
    }
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


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agents_installed_with_nvm_fnm_npm_prefix_or_volta_are_found() {
        let home = std::env::temp_dir().join(format!("archonui-detect-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        let data = home.join(".local/share");
        for dir in [
            ".nvm/versions/node/v18.20.0/bin",
            ".nvm/versions/node/v22.11.0/bin",
            ".nvm/versions/node/v9.0.0/bin",
            ".local/share/fnm/aliases/default/bin",
            ".local/share/fnm/node-versions/v20.1.0/installation/bin",
            ".volta/bin",
            "npm-prefix/bin",
            ".opencode/bin",
        ] {
            std::fs::create_dir_all(home.join(dir)).unwrap();
        }
        std::fs::write(home.join(".npmrc"), "fund=false\nprefix = ~/npm-prefix\n").unwrap();
        let dirs = bin_dirs_in(&home, &data, None, None);
        let rel: Vec<String> =
            dirs.iter().map(|d| d.strip_prefix(&home).unwrap().to_string_lossy().into_owned()).collect();
        assert_eq!(
            rel,
            [
                ".volta/bin",
                ".opencode/bin",
                "npm-prefix/bin",
                ".nvm/versions/node/v22.11.0/bin",
                ".nvm/versions/node/v18.20.0/bin",
                ".nvm/versions/node/v9.0.0/bin",
                ".local/share/fnm/aliases/default/bin",
                ".local/share/fnm/node-versions/v20.1.0/installation/bin",
            ],
            "only dirs that exist, newest Node first"
        );
        let _ = std::fs::remove_dir_all(&home);
    }
}
