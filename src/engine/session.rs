use super::{
    attach::Attachment,
    detect::DetectedAgent,
    harness::{Harness, Mode, ToolSettings, TurnInput},
    parse::{Out, Parser},
};
use smol::channel::{unbounded, Receiver};
use std::{
    collections::VecDeque,
    fs::{self, OpenOptions},
    io::{self, BufRead, BufReader, Write},
    os::unix::{fs::OpenOptionsExt, process::CommandExt},
    path::Path,
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
    time::Duration,
};

pub enum Event {
    Out(Out),
    /// The agent process exited; `Err` carries the status and stderr tail.
    Finished(Result<(), String>),
}

/// How long a cancelled agent gets to exit on SIGTERM before its group is sent SIGKILL.
const KILL_AFTER: Duration = Duration::from_secs(3);

/// How long output may still arrive after the agent exits. Output already in the pipe is drained in this time.
const OUTPUT_GRACE: Duration = Duration::from_secs(2);
/// How long to wait for the stderr tail once the agent has exited.
const STDERR_GRACE: Duration = Duration::from_millis(500);

/// One running prompt. Dropping it stops the agent.
pub struct Turn {
    pub events: Receiver<Event>,
    group: Arc<Group>,
}

/// The agent's process group. `reaped` is set, under the lock, before the
/// child is reaped, and signals are sent only while holding the lock with
/// `reaped` false, so the pid (and group id) can't have been reused by then.
struct Group {
    pid: u32,
    reaped: Mutex<bool>,
}

impl Group {
    /// Sends `signal` to the whole group unless the agent was already reaped.
    /// `false` when it was.
    fn signal(&self, signal: i32) -> bool {
        let reaped = self.reaped.lock().unwrap_or_else(|e| e.into_inner());
        if !*reaped {
            unsafe { libc::kill(-(self.pid as i32), signal) };
        }
        !*reaped
    }

    /// Blocks until the child has exited, without reaping it, then marks it reaped.
    /// The caller reaps it right after.
    fn wait_exit(&self) {
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        loop {
            let r = unsafe { libc::waitid(libc::P_PID, self.pid, &mut info, libc::WEXITED | libc::WNOWAIT) };
            if r == 0 || io::Error::last_os_error().kind() != io::ErrorKind::Interrupted {
                break;
            }
        }
        *self.reaped.lock().unwrap_or_else(|e| e.into_inner()) = true;
    }
}

impl Turn {
    /// Terminate the agent and anything it spawned (tools, shells). An agent
    /// that ignores SIGTERM gets SIGKILL after `KILL_AFTER`.
    pub fn cancel(&self) {
        if self.group.signal(libc::SIGTERM) {
            let group = self.group.clone();
            thread::spawn(move || {
                thread::sleep(KILL_AFTER);
                group.signal(libc::SIGKILL);
            });
        }
    }

    #[cfg(test)]
    pub fn mock() -> Self {
        let (_, events) = unbounded();
        Self { events, group: Arc::new(Group { pid: 0, reaped: Mutex::new(true) }) }
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
    /// The chat's project folder, which the agent is told it is in.
    pub project: Option<&'a Path>,
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
        project: opts.project,
    };
    spawn(agent.harness, &agent.path, &input)
}

/// Start the CLI at `path` for one turn. Shared by chats and the connection test.
pub(super) fn spawn<H: Harness + ?Sized>(harness: &H, path: &Path, input: &TurnInput) -> io::Result<Turn> {
    let plan = harness.plan_at(path, input);
    for (file, text) in &plan.files {
        write_private(file, text)?;
    }
    let mut cmd = Command::new(path);
    cmd.args(plan.args)
        .envs(plan.env)
        .current_dir(input.cwd)
        .stdin(if plan.stdin.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    #[cfg(target_os = "linux")]
    end_with_parent(&mut cmd);
    let mut child = cmd.spawn()?;
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
    let group = Arc::new(Group { pid, reaped: Mutex::new(false) });
    let (tx, rx) = unbounded();
    // Set when `Finished` is sent; output read after that is dropped. A lock, not a flag, so a line
    // being sent can't land after `Finished`.
    let finished = Arc::new(Mutex::new(false));
    // Shared rather than returned, so the lines read so far are kept even if the pipe never closes.
    let stderr_tail = Arc::new(Mutex::new(VecDeque::new()));
    let (stderr_tx, stderr_rx) = mpsc::channel();
    let (eof_tx, eof_rx) = mpsc::channel();

    let tail = stderr_tail.clone();
    thread::spawn(move || {
        // Bytes, not `lines()`: a line that isn't UTF-8 must not stop the drain, or the pipe fills.
        for bytes in BufReader::new(stderr).split(b'\n').map_while(Result::ok) {
            let line = String::from_utf8_lossy(&bytes).trim_end_matches('\r').to_string();
            if benign(&line) {
                continue;
            }
            let mut tail = tail.lock().unwrap_or_else(|e| e.into_inner());
            if tail.len() == 12 {
                tail.pop_front();
            }
            tail.push_back(line);
        }
        let _ = stderr_tx.send(());
    });

    let mut parser = Parser::new(harness.protocol(), input.cwd);
    let reader_tx = tx.clone();
    let reader_finished = finished.clone();
    thread::spawn(move || {
        // Bytes, not `lines()`: `map_while` would end the stream at the first non-UTF-8 line.
        for bytes in BufReader::new(stdout).split(b'\n').map_while(Result::ok) {
            let line = String::from_utf8_lossy(&bytes);
            let outs = parser.parse(line.trim_end_matches('\r'));
            let finished = reader_finished.lock().unwrap_or_else(|e| e.into_inner());
            if *finished {
                break;
            }
            for out in outs {
                let _ = reader_tx.send_blocking(Event::Out(out));
            }
        }
        let _ = eof_tx.send(());
    });

    // A background process the agent started can hold stdout and stderr open after the agent exits,
    // so the result waits only a short grace period for the pipes to close.
    let reaper = group.clone();
    thread::spawn(move || {
        reaper.wait_exit();
        let status = child.wait();
        let _ = eof_rx.recv_timeout(OUTPUT_GRACE);
        let _ = stderr_rx.recv_timeout(STDERR_GRACE);
        let tail = Vec::from(stderr_tail.lock().unwrap_or_else(|e| e.into_inner()).clone()).join("\n");
        let result = match status {
            Ok(s) if s.success() => Ok(()),
            Ok(s) => Err(format!("{s}\n{tail}").trim().to_string()),
            Err(e) => Err(e.to_string()),
        };
        let mut finished = finished.lock().unwrap_or_else(|e| e.into_inner());
        *finished = true;
        let _ = tx.send_blocking(Event::Finished(result));
    });

    Ok(Turn { events: rx, group })
}

/// A stderr line that says nothing about a failure, so it is kept out of the error a failed turn
/// shows. Pi prints this one on the first turn of every chat, because ArchonUI picks the session id.
fn benign(line: &str) -> bool {
    line.starts_with("Warning: No project session found with id ")
}

/// Writes `text` to `path`, creating its folder, readable by this user only.
/// The text goes to a new file that is renamed over `path`, so a reader never sees a partial file.
fn write_private(path: &Path, text: &str) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        crate::store::create_private_dir(dir)?;
    }
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let tmp = path.with_extension(format!("{}.{}.tmp", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    let written = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&tmp)
        .and_then(|mut file| file.write_all(text.as_bytes()));
    let result = written.and_then(|()| fs::rename(&tmp, path));
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

/// Has the agent sent SIGTERM when ArchonUI dies, even by SIGKILL or a crash, so it never keeps
/// running tools on its own. Its own process group means nothing else would reach it. The kernel
/// sends it when the spawning thread exits, so agents are only started from threads that live as
/// long as the app (the UI thread, the executor's pool, the `--serve` loop).
#[cfg(target_os = "linux")]
fn end_with_parent(cmd: &mut Command) {
    let parent = std::process::id();
    // SAFETY: the closure runs between fork and exec, so it only makes async-signal-safe system
    // calls (prctl, getppid) and allocates nothing.
    unsafe {
        cmd.pre_exec(move || {
            libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
            // The parent may have died before prctl, too late to be signalled: don't start orphaned.
            if libc::getppid() as u32 != parent {
                return Err(io::Error::from_raw_os_error(libc::ESRCH));
            }
            Ok(())
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use crate::engine::{
        harness::{AuthStatus, Plan},
        parse::Protocol,
    };

    /// Runs the prompt as a `sh -c` script.
    struct Shell;

    impl Harness for Shell {
        fn id(&self) -> &'static str {
            "sh"
        }
        fn name(&self) -> &'static str {
            "sh"
        }
        fn binary(&self) -> &'static str {
            "sh"
        }
        fn protocol(&self) -> Protocol {
            Protocol::Text
        }
        fn modes(&self) -> &'static [Mode] {
            &Mode::ALL
        }
        fn plan(&self, input: &TurnInput) -> Plan {
            Plan { args: vec!["-c".into(), input.prompt.into()], ..Default::default() }
        }
        fn auth_status(&self) -> AuthStatus {
            AuthStatus::Ok
        }
    }

    fn run_shell(script: &str) -> Turn {
        let input = TurnInput {
            cwd: Path::new("/"),
            prompt: script,
            session: None,
            files: &[],
            model: None,
            effort: None,
            mode: Mode::Auto,
            tools: ToolSettings::default(),
            project: None,
        };
        spawn(&Shell, Path::new("/bin/sh"), &input).unwrap()
    }

    /// The `Finished` event, skipping output, or `None` after `secs`.
    fn finished_within(turn: &Turn, secs: u64) -> Option<Result<(), String>> {
        let deadline = std::time::Instant::now() + Duration::from_secs(secs);
        loop {
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            let next = smol::block_on(smol::future::or(async { turn.events.recv().await.ok() }, async {
                smol::Timer::after(left).await;
                None
            }))?;
            if let Event::Finished(result) = next {
                return Some(result);
            }
        }
    }

    #[test]
    fn cancel_kills_an_agent_that_ignores_sigterm() {
        let turn = run_shell("trap '' TERM; sleep 60");
        thread::sleep(Duration::from_millis(200));
        turn.cancel();
        let result = finished_within(&turn, KILL_AFTER.as_secs() + 5).expect("the agent was killed");
        assert!(result.is_err(), "a killed agent reports its signal");
        // Once reaped, cancelling again sends nothing.
        assert!(!turn.group.signal(0));
        turn.cancel();
    }

    #[test]
    fn a_failure_shows_the_real_error_not_pis_session_warning() {
        let turn = run_shell(
            "echo \"Warning: No project session found with id 'archonui-1'; creating a new session with that id.\" >&2; \
             echo 'Error: Unknown option: --x' >&2; exit 1",
        );
        let result = finished_within(&turn, 5).expect("the turn finished");
        assert_eq!(result, Err("exit status: 1\nError: Unknown option: --x".to_string()));
    }

    #[test]
    fn a_background_process_does_not_hold_up_the_finish() {
        // `sleep` inherits stdout, so the pipe stays open after the shell exits.
        let turn = run_shell("sleep 8 & echo hi");
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let mut text = String::new();
        let result = loop {
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            let next = smol::block_on(smol::future::or(async { turn.events.recv().await.ok() }, async {
                smol::Timer::after(left).await;
                None
            }));
            match next {
                Some(Event::Out(Out::Text(t))) => text.push_str(&t),
                Some(Event::Finished(result)) => break result,
                Some(_) => {}
                None => panic!("the turn did not finish within 5 seconds"),
            }
        };
        assert_eq!(text, "hi\n", "output before the exit arrives before Finished");
        assert_eq!(result, Ok(()));
    }

    #[test]
    fn a_failed_exit_keeps_its_stderr_when_a_background_process_holds_the_pipe() {
        let turn = run_shell("sleep 4 >&2 & echo boom >&2; exit 3");
        let result = loop {
            match smol::block_on(turn.events.recv()) {
                Ok(Event::Finished(result)) => break result,
                Ok(_) => {}
                Err(_) => panic!("the turn ended without Finished"),
            }
        };
        assert_eq!(result, Err("exit status: 3\nboom".to_string()));
    }

    /// The kernel ends the agent when its parent goes. A thread that exits stands in for a killed
    /// app: the death signal follows the spawning thread.
    #[cfg(target_os = "linux")]
    #[test]
    fn an_agent_ends_when_its_parent_does() {
        let turn = thread::spawn(|| run_shell("sleep 30")).join().unwrap();
        let result = finished_within(&turn, 5).expect("the agent was ended");
        let err = result.expect_err("ended by a signal");
        assert!(err.contains("signal: 15"), "{err}");
    }

    #[test]
    fn a_finished_turn_is_marked_reaped() {
        let turn = run_shell("echo hi");
        assert_eq!(finished_within(&turn, 10), Some(Ok(())));
        assert!(*turn.group.reaped.lock().unwrap());
    }

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

    #[test]
    fn a_reader_keeps_the_old_text_when_the_file_is_replaced() {
        use std::io::Read;
        let dir = std::env::temp_dir().join(format!("archonui-session-replace-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let path = dir.join("mcp/claude.json");
        write_private(&path, "{\"old\": true}").unwrap();
        let mut reader = fs::File::open(&path).unwrap();
        write_private(&path, "{}").unwrap();
        let mut old = String::new();
        reader.read_to_string(&mut old).unwrap();
        assert_eq!(old, "{\"old\": true}");
        assert_eq!(fs::read_to_string(&path).unwrap(), "{}");
        assert_eq!(fs::read_dir(path.parent().unwrap()).unwrap().count(), 1, "no temp file is left behind");
        let _ = fs::remove_dir_all(&dir);
    }
}
