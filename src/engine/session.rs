use super::{
    attach::Attachment,
    detect::DetectedAgent,
    harness::{Harness, Mode, ToolSettings, TurnInput},
    parse::{Out, Parser},
};
use smol::channel::{unbounded, Receiver};
use std::{
    collections::VecDeque,
    fs::OpenOptions,
    io::{self, BufRead, BufReader, Write},
    os::unix::{fs::OpenOptionsExt, process::CommandExt},
    path::Path,
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
};

pub enum Event {
    Out(Out),
    /// The agent process exited; `Err` carries the status and stderr tail.
    Finished(Result<(), String>),
}

/// One running prompt. Dropping it stops the agent.
pub struct Turn {
    pub events: Receiver<Event>,
    pid: u32,
    exited: Arc<AtomicBool>,
}

impl Turn {
    /// Terminate the agent and anything it spawned (tools, shells).
    pub fn cancel(&self) {
        // The child is reaped only after `exited` is set, so its pid (and
        // process group id) can't have been reused while this is false.
        if !self.exited.load(Ordering::SeqCst) {
            unsafe { libc::kill(-(self.pid as i32), libc::SIGTERM) };
        }
    }

    #[cfg(test)]
    pub fn mock() -> Self {
        let (_, events) = unbounded();
        Self { events, pid: 0, exited: Arc::new(AtomicBool::new(true)) }
    }
}

impl Drop for Turn {
    fn drop(&mut self) {
        self.cancel();
    }
}

/// Overrides of the agent's defaults for one turn.
#[derive(Default, Clone, Copy)]
pub struct TurnOptions<'a> {
    pub model: Option<&'a str>,
    pub effort: Option<&'a str>,
    pub mode: Mode,
    pub files: &'a [Attachment],
    pub tools: ToolSettings<'a>,
}

/// Run one prompt against `agent` in `cwd`, resuming `session` if given.
pub fn start_turn(
    agent: &DetectedAgent,
    cwd: &Path,
    prompt: &str,
    session: Option<&str>,
    opts: TurnOptions,
) -> io::Result<Turn> {
    let input = TurnInput {
        cwd,
        prompt,
        session,
        files: opts.files,
        model: opts.model,
        effort: opts.effort,
        mode: opts.mode,
        tools: opts.tools,
    };
    spawn(agent.harness, &agent.path, &input)
}

/// Start the CLI at `path` for one turn. Shared by chats and the connection test.
pub(super) fn spawn<H: Harness + ?Sized>(harness: &H, path: &Path, input: &TurnInput) -> io::Result<Turn> {
    let plan = harness.plan(input);
    for (file, text) in &plan.files {
        write_private(file, text)?;
    }
    let mut child = Command::new(path)
        .args(plan.args)
        .envs(plan.env)
        .current_dir(input.cwd)
        .stdin(if plan.stdin.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn()?;
    let pid = child.id();
    if let (Some(write), Some(stdin)) = (plan.stdin, child.stdin.take()) {
        // Large attachments are encoded here rather than on the UI thread.
        // Closing stdin afterwards tells the agent the message is complete;
        // a write error surfaces as the agent's own error.
        thread::spawn(move || {
            let mut stdin = io::BufWriter::new(stdin);
            let _ = write(&mut stdin).and_then(|()| stdin.flush());
        });
    }
    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().expect("piped stderr");
    let exited = Arc::new(AtomicBool::new(false));
    let (tx, rx) = unbounded();

    let stderr_tail = thread::spawn(move || {
        let mut tail = VecDeque::new();
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            if tail.len() == 12 {
                tail.pop_front();
            }
            tail.push_back(line);
        }
        Vec::from(tail).join("\n")
    });

    let mut parser = Parser::new(harness.protocol(), input.cwd);
    let exited_flag = exited.clone();
    thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            for out in parser.parse(&line) {
                let _ = tx.send_blocking(Event::Out(out));
            }
        }
        let status = child.wait();
        exited_flag.store(true, Ordering::SeqCst);
        let tail = stderr_tail.join().unwrap_or_default();
        let result = match status {
            Ok(s) if s.success() => Ok(()),
            Ok(s) => Err(format!("{s}\n{tail}").trim().to_string()),
            Err(e) => Err(e.to_string()),
        };
        let _ = tx.send_blocking(Event::Finished(result));
    });

    Ok(Turn { events: rx, pid, exited })
}

/// Writes `text` to `path`, creating its folder, readable by this user only.
fn write_private(path: &Path, text: &str) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        crate::store::create_private_dir(dir)?;
    }
    let mut file = OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(path)?;
    file.write_all(text.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn private_files_are_owner_only_and_replace_old_text() {
        let dir = std::env::temp_dir().join(format!("archonui-session-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let path = dir.join("mcp/claude.json");
        write_private(&path, "{\"old\": true}").unwrap();
        write_private(&path, "{}").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "{}");
        assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        let _ = fs::remove_dir_all(&dir);
    }
}
