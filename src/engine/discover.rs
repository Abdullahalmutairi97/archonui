//! What each agent CLI offers: the slash commands it accepts in non-interactive
//! mode and the models it is connected to. Each harness reads its own CLI's
//! listings with the probes here; nothing is hardcoded per user, and no probe
//! calls a model or saves a session.

use super::detect::DetectedAgent;
use serde_json::Value;
use std::{
    io::{BufRead, BufReader, Write},
    os::unix::{fs::OpenOptionsExt, process::CommandExt},
    path::Path,
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::Duration,
};

#[derive(Clone, Debug)]
pub struct AgentCommand {
    pub name: String,
    pub description: String,
    /// Argument hint such as `[issue description]`; may be empty.
    pub hint: String,
}

#[derive(Clone, Debug)]
pub struct ModelInfo {
    /// Value passed to the agent's model flag.
    pub id: String,
    pub label: String,
    pub description: String,
    /// Effort levels the model accepts, in the agent's own order; empty if
    /// it has no effort setting.
    pub efforts: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub struct Capabilities {
    pub commands: Vec<AgentCommand>,
    pub models: Vec<ModelInfo>,
    /// The model the agent uses when none is passed, if it says.
    pub default_model: Option<String>,
    /// Effort levels of the default model.
    pub default_efforts: Vec<String>,
    /// The effort the agent uses when none is passed, if it says.
    pub default_effort: Option<String>,
}

impl Capabilities {
    /// Effort levels for `model`, or for the default model if `None`.
    pub fn efforts(&self, model: Option<&str>) -> &[String] {
        match model {
            None => &self.default_efforts,
            Some(id) => self.models.iter().find(|m| m.id == id).map_or(&[], |m| &m.efforts),
        }
    }
}

/// Capabilities of an installed agent, sorted and without duplicate names.
pub fn discover(agent: &DetectedAgent, cwd: &Path) -> Capabilities {
    let mut caps = agent.harness.discover(&agent.path, cwd);
    caps.commands.sort_by(|a, b| a.name.cmp(&b.name));
    caps.commands.dedup_by(|a, b| a.name == b.name);
    caps.models.dedup_by(|a, b| a.id == b.id);
    caps
}

pub(super) fn strings(v: &Value) -> Vec<String> {
    v.as_array().into_iter().flatten().filter_map(|x| x.as_str().map(String::from)).collect()
}

const PROBE_TIMEOUT: Duration = Duration::from_secs(20);

/// Spawn `cmd`, write the request lines, and return, for each predicate, the
/// first stdout JSON line matching it. `None` unless all are found. The
/// process group is killed afterwards or on timeout.
pub(super) fn probe_jsonl<const N: usize>(
    mut cmd: Command,
    cwd: &Path,
    requests: [&str; N],
    wants: [fn(&Value) -> bool; N],
) -> Option<[Value; N]> {
    let mut child = cmd
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .ok()?;
    let killer = Killer::arm(child.id());
    // Keep stdin open until we have the answers; some CLIs exit on EOF.
    let mut stdin = child.stdin.take().expect("piped stdin");
    let stdout = child.stdout.take().expect("piped stdout");
    // A CLI that exits early fails the write; it is still killed and reaped below, not left a zombie.
    let sent = requests.iter().all(|r| writeln!(stdin, "{r}").is_ok());
    let mut found: [Option<Value>; N] = std::array::from_fn(|_| None);
    // A line that is not UTF-8 is skipped, not taken as the end of the output.
    let lines = sent.then(|| BufReader::new(stdout).lines().filter_map(Result::ok));
    for line in lines.into_iter().flatten() {
        let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
        if let Some(i) = (0..N).find(|&i| found[i].is_none() && wants[i](&v)) {
            found[i] = Some(v);
            if found.iter().all(Option::is_some) {
                break;
            }
        }
    }
    killer.kill_now();
    drop(stdin);
    let _ = child.wait();
    let found: Vec<Value> = found.into_iter().collect::<Option<_>>()?;
    found.try_into().ok()
}

pub(super) fn command(name: &str, description: &str, hint: &str) -> AgentCommand {
    AgentCommand { name: name.into(), description: short(description), hint: hint.into() }
}

/// Map a JSON array of `{name, description, <hint_key>}` objects.
pub(super) fn list(v: &Value, pointer: &str, hint_key: &str) -> Vec<AgentCommand> {
    let Some(items) = v.pointer(pointer).and_then(Value::as_array) else { return Vec::new() };
    items
        .iter()
        .filter_map(|c| {
            let name = c["name"].as_str()?;
            // Internal commands are not meant to be typed.
            if name.starts_with("__") {
                return None;
            }
            let hint = c.get(hint_key).and_then(Value::as_str).unwrap_or("");
            Some(command(name, c["description"].as_str().unwrap_or(""), hint))
        })
        .collect()
}

/// First sentence, capped, so the palette stays one line per command.
pub(super) fn short(text: &str) -> String {
    let line = text.lines().next().unwrap_or("").trim();
    let end = line.find(". ").map_or(line.len(), |i| i + 1);
    let s = &line[..end];
    if s.chars().count() > 90 {
        format!("{}…", s.chars().take(90).collect::<String>().trim_end())
    } else {
        s.to_string()
    }
}

/// Run a command to completion and parse all of stdout as JSON.
pub(super) fn run_json(path: &Path, cwd: &Path, args: &[&str]) -> Option<Value> {
    serde_json::from_str(&run_text(path, cwd, args)?).ok()
}

/// Run a command to completion and return stdout, or `None` on failure or
/// timeout. Stdout goes to a temp file: OpenCode exits before flushing large
/// output into a pipe.
pub(super) fn run_text(path: &Path, cwd: &Path, args: &[&str]) -> Option<String> {
    let out = std::env::temp_dir().join(format!("{}.json", crate::store::new_id()));
    // Owner-only and exclusive: a CLI's output (OpenCode's config) can hold keys, and a planted
    // symlink at a guessable name must not be written through.
    // Opened first: a failed open means the name was taken, so the file there is not ours to remove.
    let Ok(file) = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&out) else {
        return None;
    };
    let Ok(mut child) = Command::new(path)
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(file)
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
    else {
        // A CLI that cannot start leaves no temp file behind.
        let _ = std::fs::remove_file(&out);
        return None;
    };
    let deadline = std::time::Instant::now() + PROBE_TIMEOUT;
    let finished = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.success(),
            Ok(None) if std::time::Instant::now() < deadline => thread::sleep(Duration::from_millis(50)),
            _ => {
                unsafe { libc::kill(-(child.id() as i32), libc::SIGKILL) };
                let _ = child.wait();
                break false;
            }
        }
    };
    let text = std::fs::read_to_string(&out).ok();
    let _ = std::fs::remove_file(&out);
    finished.then_some(text).flatten()
}

/// Kills a probe's process group after `PROBE_TIMEOUT` unless disarmed.
/// Callers reap the child only after `kill_now`, so the pid cannot
/// have been reused while the killer may still fire.
struct Killer {
    pid: u32,
    done: Arc<AtomicBool>,
}

impl Killer {
    fn arm(pid: u32) -> Self {
        let done = Arc::new(AtomicBool::new(false));
        let flag = done.clone();
        thread::spawn(move || {
            thread::sleep(PROBE_TIMEOUT);
            if !flag.swap(true, Ordering::SeqCst) {
                unsafe { libc::kill(-(pid as i32), libc::SIGKILL) };
            }
        });
        Self { pid, done }
    }

    fn kill_now(&self) {
        if !self.done.swap(true, Ordering::SeqCst) {
            unsafe { libc::kill(-(self.pid as i32), libc::SIGKILL) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_probe_that_exits_early_is_reaped() {
        let cwd = std::env::temp_dir();
        // Exits before reading, so the request write may fail; either way nothing is found.
        let mut cmd = Command::new("/bin/sh");
        cmd.args(["-c", "exit 0"]);
        let start = std::time::Instant::now();
        assert!(probe_jsonl(cmd, &cwd, ["{}"], [|_: &Value| true]).is_none());
        assert!(start.elapsed() < PROBE_TIMEOUT, "returned without waiting for the timeout");
        let mut cmd = Command::new("/bin/sh");
        cmd.args(["-c", "read line; echo '{\"ok\":1}'; sleep 60"]);
        let [v] = probe_jsonl(cmd, &cwd, ["{}"], [|v: &Value| v["ok"] == 1]).expect("answer found");
        assert_eq!(v["ok"], 1);
    }

    #[test]
    fn run_text_output_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("archonui-disc-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("cli.sh");
        std::fs::write(&script, "#!/bin/sh\nstat -c %a /proc/self/fd/1\n").unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(run_text(&script, &dir, &[]).as_deref().map(str::trim), Some("300")); // write-only, owner-only
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn run_text_reports_failure_for_a_missing_binary() {
        assert_eq!(run_text(Path::new("/nonexistent/archonui-cli"), &std::env::temp_dir(), &[]), None);
    }

    #[test]
    fn short_takes_first_sentence() {
        assert_eq!(short("Enable debug logging. And more."), "Enable debug logging.");
        assert!(short(&"word ".repeat(40)).ends_with('…'));
    }

    #[test]
    fn list_skips_internal_commands() {
        let v: Value = serde_json::json!({"c": [
            {"name": "debug", "description": "Enable debug logging", "argumentHint": "[issue]"},
            {"name": "__remote-workflow", "description": ""}
        ]});
        let cmds = list(&v, "/c", "argumentHint");
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].hint, "[issue]");
    }
}
