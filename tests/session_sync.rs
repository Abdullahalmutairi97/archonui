//! End-to-end tests for sharing chats between devices: a real `archonui --serve` host on loopback, and
//! clients speaking real WebSocket frames to it, either the real `archonui --remote` command line or a
//! raw socket here. The agent is a stand-in `claude` script that prints stream-json, so no real CLI,
//! account or network is needed.
//!
//! Each test gets its own temporary home, config, data and runtime folders, and a `PATH` of its own,
//! so the user's real chats and settings are never read or written. A stand-in `tailscale` sits first
//! on that `PATH`, so the real one is never run. Hosts bind `127.0.0.1:0` only.

use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde_json::{json, Value};
use std::{
    io::{self, BufRead, BufReader},
    net::TcpStream,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};
use tungstenite::{stream::MaybeTlsStream, Message, WebSocket};

/// Generous: these only bound how long a broken test hangs, not how long a passing one takes.
const TIMEOUT: Duration = Duration::from_secs(60);
/// Must match `proto::VERSION`.
const VERSION: u64 = 1;
/// Must match `proto::MAX_REMOTE_FILES_BYTES`.
const MAX_REMOTE_FILES_BYTES: usize = 40 * 1024 * 1024;
/// The reply the stand-in agent streams in three pieces, then repeats whole as Claude Code does.
const REPLY: &str = "Hello there";
/// A prompt containing this makes the stand-in agent stream a little, then wait for `Env::release`.
const HOLD: &str = "@@HOLD@@";

/// A stand-in `claude`: answers `--version`, records each run's arguments and stdin under `calls/`,
/// and prints stream-json like Claude Code with `--include-partial-messages`.
const FAKE_CLAUDE: &str = r#"#!/bin/sh
case "$1" in --version) echo "2.0.0 (Claude Code)"; exit 0;; esac
here=$(cd "$(dirname "$0")/.." && pwd)
mkdir -p "$here/calls"
input=$(cat)
printf '%s\n' "$@" > "$here/calls/$$.args"
printf '%s' "$input" > "$here/calls/$$.stdin"
echo '{"type":"system","subtype":"init","session_id":"fake-session"}'
echo '{"type":"stream_event","event":{"type":"message_start","message":{"role":"assistant","content":[]}}}'
case "$* $input" in
*@@HOLD@@*)
  echo '{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Working"}}}'
  while [ ! -f "$here/release" ]; do sleep 0.05; done
  rm -f "$here/release"
  echo '{"type":"assistant","message":{"content":[{"type":"text","text":"Working"}]}}'
  echo '{"type":"result","subtype":"success","result":"Working","session_id":"fake-session"}'
  exit 0;;
esac
echo '{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hel"}}}'
echo '{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"lo "}}}'
echo '{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"there"}}}'
echo '{"type":"assistant","message":{"content":[{"type":"text","text":"Hello there"}]}}'
echo '{"type":"result","subtype":"success","result":"Hello there","session_id":"fake-session"}'
"#;

/// A stand-in `tailscale` that is never connected, so discovery fails the way it does on a machine
/// without Tailscale.
const FAKE_TAILSCALE: &str = "#!/bin/sh\necho 'Tailscale is stopped.' >&2\nexit 1\n";

/// One test's private folders. Removed on drop.
struct Env {
    root: PathBuf,
}

impl Env {
    fn new(tag: &str) -> Env {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("archonui-sync-{}-{n}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for dir in ["bin", "home", "cfg", "data/sessions", "run", "work"] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        std::fs::set_permissions(root.join("run"), std::fs::Permissions::from_mode(0o700)).unwrap();
        for (name, body) in [("claude", FAKE_CLAUDE), ("tailscale", FAKE_TAILSCALE)] {
            let path = root.join("bin").join(name);
            std::fs::write(&path, body).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        Env { root }
    }

    fn home(&self) -> PathBuf {
        self.root.join("home")
    }

    fn data(&self) -> PathBuf {
        self.root.join("data")
    }

    /// An existing folder for chats to run in.
    fn work(&self) -> PathBuf {
        self.root.join("work")
    }

    /// The binary with only this test's folders and `PATH` in its environment.
    fn command(&self) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_archonui"));
        cmd.env_clear()
            .env("PATH", format!("{}:/usr/bin:/bin", self.root.join("bin").display()))
            .env("HOME", self.home())
            .env("ARCHONUI_CONFIG_DIR", self.root.join("cfg"))
            .env("ARCHONUI_DATA_DIR", self.data().join("sessions"))
            .env("XDG_CONFIG_HOME", self.root.join("cfg"))
            .env("XDG_DATA_HOME", self.data())
            .env("XDG_RUNTIME_DIR", self.root.join("run"))
            .env("HOSTNAME", "testhost")
            .current_dir(self.work());
        cmd
    }

    /// Lets a held turn of the stand-in agent finish.
    fn release(&self) {
        std::fs::write(self.root.join("release"), b"").unwrap();
    }

    /// Everything the stand-in agent was given, every run: arguments and stdin.
    fn agent_input(&self) -> String {
        let mut all = String::new();
        for entry in std::fs::read_dir(self.root.join("calls")).into_iter().flatten().flatten() {
            all.push_str(&std::fs::read_to_string(entry.path()).unwrap_or_default());
        }
        all
    }

    /// `archonui --remote 127.0.0.1:<port> <args>`.
    fn remote(&self, port: u16, args: &[&str]) -> Output {
        self.command().arg("--remote").arg(format!("127.0.0.1:{port}")).args(args).output().unwrap()
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// A running `archonui --serve --bind 127.0.0.1:0`. Killed on drop.
struct Host {
    child: Child,
    port: u16,
    log: Arc<Mutex<String>>,
}

impl Host {
    fn start(env: &Env) -> Host {
        let mut child = env
            .command()
            .args(["--serve", "--bind", "127.0.0.1:0"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let log = Arc::new(Mutex::new(String::new()));
        let (tx, rx) = mpsc::channel();
        let stderr = child.stderr.take().unwrap();
        let lines = log.clone();
        thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                if let Some(addr) = line.strip_prefix("archonui host listening on ") {
                    let _ = tx.send(addr.to_string());
                }
                let mut log = lines.lock().unwrap();
                log.push_str(&line);
                log.push('\n');
            }
        });
        let addr = rx.recv_timeout(TIMEOUT).unwrap_or_else(|_| {
            let _ = child.kill();
            panic!("the host did not start listening:\n{}", log.lock().unwrap())
        });
        let addr: std::net::SocketAddr = addr.parse().unwrap();
        assert!(addr.ip().is_loopback(), "{addr}");
        assert_ne!(addr.port(), 0);
        Host { child, port: addr.port(), log }
    }

    fn log(&self) -> String {
        self.log.lock().unwrap().clone()
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// What a raw client read.
#[derive(Debug)]
enum Got {
    Msg(Value),
    Closed,
}

/// A raw WebSocket client: sends and reads protocol messages as JSON.
struct Ws {
    ws: WebSocket<MaybeTlsStream<TcpStream>>,
}

impl Ws {
    /// Connects without saying hello.
    fn connect(port: u16) -> Ws {
        let (ws, _) = tungstenite::connect(format!("ws://127.0.0.1:{port}")).unwrap();
        if let MaybeTlsStream::Plain(s) = ws.get_ref() {
            s.set_read_timeout(Some(Duration::from_millis(100))).unwrap();
        }
        Ws { ws }
    }

    /// Connects and does the handshake.
    fn hello(port: u16, device: &str) -> Ws {
        let mut ws = Ws::connect(port);
        ws.send(json!({"type": "hello", "device": device, "version": VERSION}));
        let welcome = ws.expect("welcome", |_| true);
        assert_eq!(welcome["type"], "welcome", "{welcome}");
        assert_eq!(welcome["version"], VERSION);
        ws
    }

    fn send(&mut self, msg: Value) {
        self.ws.send(Message::text(msg.to_string())).unwrap();
    }

    /// The next message, or `Closed`. Panics after `TIMEOUT`.
    fn next(&mut self) -> Got {
        let deadline = Instant::now() + TIMEOUT;
        loop {
            match self.ws.read() {
                Ok(Message::Text(t)) => return Got::Msg(serde_json::from_str(t.as_str()).unwrap()),
                Ok(Message::Close(_)) => return Got::Closed,
                Ok(_) => {}
                Err(tungstenite::Error::Io(e)) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) => {
                    assert!(Instant::now() < deadline, "no message within {TIMEOUT:?}");
                }
                Err(_) => return Got::Closed,
            }
        }
    }

    /// Reads until a message `pick` accepts, and returns it. Panics on a close or after `TIMEOUT`.
    fn expect(&mut self, what: &str, pick: impl Fn(&Value) -> bool) -> Value {
        self.collect(what, pick).1
    }

    /// Reads until a message `pick` accepts. Returns the messages before it, and it.
    fn collect(&mut self, what: &str, pick: impl Fn(&Value) -> bool) -> (Vec<Value>, Value) {
        let mut seen = Vec::new();
        loop {
            match self.next() {
                Got::Msg(m) if pick(&m) => return (seen, m),
                Got::Msg(m) => seen.push(m),
                Got::Closed => panic!("closed while waiting for {what}; saw {seen:?}"),
            }
        }
    }

    /// Sends `ListSessions` and returns the list.
    fn sessions(&mut self) -> Vec<Value> {
        self.send(json!({"type": "list_sessions"}));
        let m = self.expect("sessions", |m| m["type"] == "sessions");
        m["sessions"].as_array().unwrap().clone()
    }

    /// Asks for a new chat and returns the host's answer: `created` or `error`.
    fn new_session(&mut self, agent: &str, cwd: &str) -> Value {
        self.send(json!({"type": "new_session", "agent": agent, "cwd": cwd, "project": null}));
        self.expect("created or error", |m| m["type"] == "created" || m["type"] == "error")
    }

    /// A new claude chat in `cwd`; returns its id.
    fn new_chat(&mut self, cwd: &Path) -> String {
        let m = self.new_session("claude", cwd.to_str().unwrap());
        assert_eq!(m["type"], "created", "{m}");
        m["session"]["id"].as_str().unwrap().to_string()
    }

    /// Subscribes and returns the snapshot.
    fn subscribe(&mut self, id: &str) -> Value {
        self.send(json!({"type": "subscribe", "session": id}));
        self.expect("snapshot", |m| m["type"] == "snapshot" || m["type"] == "error")
    }

    fn prompt(&mut self, id: &str, text: &str, files: Value) {
        self.send(json!({"type": "prompt", "session": id, "text": text, "files": files}));
    }
}

fn is_finished(m: &Value, id: &str) -> bool {
    m["type"] == "finished" && m["session"] == id
}

fn is_text(m: &Value, id: &str, text: &str) -> bool {
    m["type"] == "event" && m["session"] == id && m["out"]["type"] == "text" && m["out"]["data"] == text
}

/// The reply text streamed in `events`, in order.
fn streamed(events: &[Value], id: &str) -> String {
    events
        .iter()
        .filter(|m| m["type"] == "event" && m["session"] == id && m["out"]["type"] == "text")
        .map(|m| m["out"]["data"].as_str().unwrap())
        .collect()
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
fn command_line_lists_creates_and_asks() {
    let env = Env::new("cli");
    let host = Host::start(&env);

    let out = env.remote(host.port, &["list"]);
    assert!(out.status.success(), "{}{}", stderr(&out), host.log());
    assert_eq!(stdout(&out).trim(), "", "a fresh host has no chats");

    let work = env.work();
    let out = env.remote(host.port, &["new", "claude", work.to_str().unwrap(), "say", "hi"]);
    assert!(out.status.success(), "{}{}", stderr(&out), host.log());
    assert_eq!(stdout(&out).matches(REPLY).count(), 1, "the reply is printed once: {:?}", stdout(&out));
    let id = stderr(&out).lines().find_map(|l| l.strip_prefix("created ")).expect("created <id>").trim().to_string();

    let out = env.remote(host.port, &["list"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let line = stdout(&out).lines().find(|l| l.starts_with(&id)).map(str::to_string);
    assert!(line.as_deref().is_some_and(|l| l.contains("claude")), "{}", stdout(&out));

    let out = env.remote(host.port, &["ask", &id, "and", "again"]);
    assert!(out.status.success(), "{}{}", stderr(&out), host.log());
    assert_eq!(stdout(&out).matches(REPLY).count(), 1, "{:?}", stdout(&out));

    let out = env.remote(host.port, &["show", &id]);
    assert!(out.status.success(), "{}", stderr(&out));
    let shown = stdout(&out);
    assert!(shown.contains("user: say hi") && shown.contains("user: and again"), "{shown}");
    assert_eq!(shown.matches(REPLY).count(), 2, "one reply per turn: {shown}");

    // The chat is saved on the host, in this test's data folder.
    assert!(env.data().join("sessions").join(format!("{id}.json")).is_file());

    let out = env.remote(host.port, &["ask", "no-such-chat", "hi"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("There is no chat no-such-chat"), "{}", stderr(&out));
}

#[test]
fn new_chat_folder_and_agent_choices() {
    let env = Env::new("new");
    let host = Host::start(&env);
    let mut c = Ws::hello(host.port, "laptop");

    let named = c.new_session("claude", env.work().to_str().unwrap());
    assert_eq!(named["type"], "created", "{named}");
    assert_eq!(named["session"]["cwd"], env.work().to_str().unwrap());
    assert_eq!(named["session"]["agent"], "claude");

    let home = c.new_session("claude", "~");
    assert_eq!(home["type"], "created", "{home}");
    assert_eq!(home["session"]["cwd"], env.home().to_str().unwrap(), "~ is the host's home");

    let missing = env.root.join("not-there");
    let m = c.new_session("claude", missing.to_str().unwrap());
    assert_eq!(m["type"], "error");
    assert!(m["message"].as_str().unwrap().contains("There is no folder"), "{m}");

    let m = c.new_session("nosuchagent", env.work().to_str().unwrap());
    assert_eq!(m["type"], "error");
    assert!(m["message"].as_str().unwrap().contains("is not set up on this device"), "{m}");

    // Both exist on the host. Empty chats stay out of the list, as they do in the app.
    for created in [&named, &home] {
        let snap = c.subscribe(created["session"]["id"].as_str().unwrap());
        assert_eq!(snap["type"], "snapshot", "{snap}");
    }
    assert!(c.sessions().is_empty());
}

#[test]
fn a_reply_streams_once_and_is_saved_once() {
    let env = Env::new("stream");
    let host = Host::start(&env);
    let mut c = Ws::hello(host.port, "laptop");
    let id = c.new_chat(&env.work());
    assert_eq!(c.subscribe(&id)["type"], "snapshot");

    c.prompt(&id, "say hi", json!([]));
    let (events, done) = c.collect("finished", |m| is_finished(m, &id));
    assert_eq!(done["error"], Value::Null, "{done}\n{}", host.log());
    assert_eq!(done["stopped"], false);
    let started: Vec<&Value> = events.iter().filter(|m| m["type"] == "started").collect();
    assert_eq!(started.len(), 1, "{events:?}");
    assert_eq!(started[0]["text"], "say hi");
    assert_eq!(streamed(&events, &id), REPLY, "each piece arrives once, in order");

    // The host keeps the reply once, too.
    let snap = c.subscribe(&id);
    assert_eq!(snap["busy"], false);
    let messages = snap["session"]["messages"].as_array().unwrap();
    let replies: Vec<&str> = messages.iter().filter(|m| m["role"] == "agent").map(|m| m["text"].as_str().unwrap()).collect();
    assert_eq!(replies, [REPLY], "{messages:?}");
}

#[test]
fn cancel_mid_turn_shows_stopped() {
    let env = Env::new("cancel");
    let host = Host::start(&env);
    let mut c = Ws::hello(host.port, "laptop");
    let id = c.new_chat(&env.work());
    c.subscribe(&id);

    c.prompt(&id, HOLD, json!([]));
    c.expect("the first piece", |m| is_text(m, &id, "Working"));
    let busy = c.sessions();
    assert_eq!(busy[0]["busy"], true, "{busy:?}");

    c.send(json!({"type": "cancel", "session": id}));
    let done = c.expect("finished", |m| is_finished(m, &id));
    assert_eq!(done["stopped"], true, "{done}");

    let after = c.sessions();
    assert_eq!(after[0]["busy"], false, "{after:?}");
    // Nothing to stop any more.
    c.send(json!({"type": "cancel", "session": id}));
    let m = c.expect("error", |m| m["type"] == "error");
    assert!(m["message"].as_str().unwrap().contains("No reply is running"), "{m}");
}

#[test]
fn two_clients_follow_one_chat_and_a_second_prompt_is_refused() {
    let env = Env::new("two");
    let host = Host::start(&env);
    let mut a = Ws::hello(host.port, "laptop");
    let mut b = Ws::hello(host.port, "phone");
    let id = a.new_chat(&env.work());
    assert_eq!(a.subscribe(&id)["type"], "snapshot");
    assert_eq!(b.subscribe(&id)["type"], "snapshot");

    a.prompt(&id, HOLD, json!([]));
    for c in [&mut a, &mut b] {
        let (before, _) = c.collect("the first piece", |m| is_text(m, &id, "Working"));
        assert!(before.iter().any(|m| m["type"] == "started" && m["text"] == HOLD), "{before:?}");
    }

    // B's prompt while A's turn runs is refused, to B only.
    b.prompt(&id, "me too", json!([]));
    let refused = b.expect("error", |m| m["type"] == "error");
    assert!(refused["message"].as_str().unwrap().contains("already running"), "{refused}");
    let snap = b.subscribe(&id);
    assert_eq!(snap["busy"], true);

    env.release();
    let (a_seen, a_done) = a.collect("finished", |m| is_finished(m, &id));
    let (b_seen, b_done) = b.collect("finished", |m| is_finished(m, &id));
    for done in [&a_done, &b_done] {
        assert_eq!(done["error"], Value::Null, "{done}");
        assert_eq!(done["stopped"], false);
    }
    assert!(!a_seen.iter().any(|m| m["type"] == "error"), "A never saw B's refusal: {a_seen:?}");
    assert!(!a_seen.iter().chain(&b_seen).any(|m| m["type"] == "started"), "the refused prompt started nothing");

    // The refused prompt left no trace in the chat.
    let snap = a.subscribe(&id);
    let users: Vec<&str> = snap["session"]["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|m| m["role"] == "user")
        .map(|m| m["text"].as_str().unwrap())
        .collect();
    assert_eq!(users, [HOLD]);
}

#[test]
fn a_small_attachment_reaches_the_agent() {
    let env = Env::new("attach");
    let host = Host::start(&env);
    let mut c = Ws::hello(host.port, "laptop");
    let id = c.new_chat(&env.work());
    c.subscribe(&id);

    let files = json!([{"name": "notes.txt", "data": STANDARD.encode("remember the milk\n")}]);
    c.prompt(&id, "read my notes", files);
    let done = c.expect("finished", |m| is_finished(m, &id));
    assert_eq!(done["error"], Value::Null, "{done}\n{}", host.log());

    let saved = env.data().join("attachments").join(&id).join("notes.txt");
    assert_eq!(std::fs::read_to_string(&saved).unwrap(), "remember the milk\n");
    let given = env.agent_input();
    assert!(given.contains("remember the milk") || given.contains(saved.to_str().unwrap()), "{given}");
}

#[test]
fn attachments_just_under_the_limit_are_accepted() {
    let env = Env::new("attach-big");
    let host = Host::start(&env);
    let mut c = Ws::hello(host.port, "laptop");
    let id = c.new_chat(&env.work());
    c.subscribe(&id);

    // Larger than one default WebSocket frame (16 MiB) once encoded, within the protocol's limit.
    let bytes = vec![7u8; MAX_REMOTE_FILES_BYTES - 1024];
    c.prompt(&id, "here is a big file", json!([{"name": "big.bin", "data": STANDARD.encode(&bytes)}]));
    let (seen, done) = c.collect("finished", |m| is_finished(m, &id) || m["type"] == "error");
    assert_eq!(done["type"], "finished", "{done}\n{}", host.log());
    assert_eq!(done["error"], Value::Null, "{done}");
    assert!(seen.iter().any(|m| m["type"] == "started"), "{seen:?}");
    let saved = env.data().join("attachments").join(&id).join("big.bin");
    assert_eq!(std::fs::metadata(saved).unwrap().len(), bytes.len() as u64);
}

#[test]
fn attachments_over_the_limit_are_refused_and_the_connection_stays() {
    let env = Env::new("attach-over");
    let host = Host::start(&env);
    let mut c = Ws::hello(host.port, "laptop");
    let id = c.new_chat(&env.work());
    c.subscribe(&id);

    let bytes = vec![7u8; MAX_REMOTE_FILES_BYTES + 1];
    c.prompt(&id, "too big", json!([{"name": "huge.bin", "data": STANDARD.encode(&bytes)}]));
    let m = c.expect("error", |m| m["type"] == "error" || m["type"] == "started");
    assert_eq!(m["type"], "error", "{m}");
    assert!(m["message"].as_str().unwrap().contains("too large"), "{m}");

    // Nothing was saved and nothing ran; the connection still works.
    assert!(!env.data().join("attachments").join(&id).join("huge.bin").exists());
    let snap = c.subscribe(&id);
    assert_eq!(snap["busy"], false, "{snap}");
    assert_eq!(snap["session"]["messages"], json!([]), "the refused prompt left nothing in the chat");
    assert!(env.agent_input().is_empty(), "the agent never ran");
}

#[test]
fn a_different_protocol_version_is_refused() {
    let env = Env::new("version");
    let host = Host::start(&env);

    let mut c = Ws::connect(host.port);
    c.send(json!({"type": "hello", "device": "old", "version": VERSION + 1}));
    match c.next() {
        Got::Msg(m) => {
            assert_eq!(m["type"], "error", "{m}");
            assert!(m["message"].as_str().unwrap().contains(&format!("Protocol version {}", VERSION + 1)), "{m}");
        }
        Got::Closed => panic!("closed without saying why"),
    }
    assert!(matches!(c.next(), Got::Closed), "the host hangs up");

    // Anything before Hello is refused the same way.
    let mut c = Ws::connect(host.port);
    c.send(json!({"type": "list_sessions"}));
    match c.next() {
        Got::Msg(m) => assert!(m["message"].as_str().unwrap().contains("Say hello first"), "{m}"),
        Got::Closed => panic!("closed without saying why"),
    }
    assert!(matches!(c.next(), Got::Closed));

    // The host is still fine for a client that speaks its version.
    let mut ok = Ws::hello(host.port, "new");
    assert!(ok.sessions().is_empty());
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "init.defaultBranch=main"])
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("HOME", dir)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

#[test]
fn cloud_store_round_trip() {
    let env = Env::new("cloud");
    let host = Host::start(&env);
    let mut c = Ws::hello(host.port, "laptop");

    // A project snapshot as a device sends it: a bundle of refs/archon/wip/<device>.
    let repo = env.root.join("project");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q"]);
    std::fs::write(repo.join("a.txt"), "hello\n").unwrap();
    git(&repo, &["add", "a.txt"]);
    git(&repo, &["commit", "-q", "-m", "first"]);
    let commit = git(&repo, &["rev-parse", "HEAD"]);
    git(&repo, &["update-ref", "refs/archon/wip/dev1", &commit]);
    let bundle = env.root.join("p.bundle");
    git(&repo, &["bundle", "create", "-q", bundle.to_str().unwrap(), "refs/archon/wip/dev1"]);
    let data = STANDARD.encode(std::fs::read(&bundle).unwrap());

    c.send(json!({"type": "put_project", "project": "p1", "name": "app", "device": "dev1", "commit": commit, "bundle": data}));
    let m = c.expect("stored", |m| m["type"] == "stored" || m["type"] == "error");
    assert_eq!(m, json!({"type": "stored", "project": "p1", "device": "dev1", "commit": commit}));

    let chat = json!({"id": "c1", "title": "from the laptop", "updated": 5, "messages": []});
    c.send(json!({"type": "put_chats", "device": "dev1", "sessions": [chat]}));
    c.send(json!({"type": "list_cloud"}));
    let m = c.expect("cloud", |m| m["type"] == "cloud" || m["type"] == "error");
    assert_eq!(m["type"], "cloud", "{m}");
    assert_eq!(m["chats"], json!(["dev1"]));
    let projects = m["projects"].as_array().unwrap();
    assert_eq!(projects.len(), 1, "{m}");
    assert_eq!(projects[0]["project"], "p1");
    assert_eq!(projects[0]["name"], "app");
    assert_eq!(projects[0]["device"], "dev1");
    assert_eq!(projects[0]["commit"], commit.as_str());

    c.send(json!({"type": "get_chats", "device": "dev1"}));
    let m = c.expect("chats", |m| m["type"] == "chats");
    assert_eq!(m["sessions"], json!([chat]));

    c.send(json!({"type": "get_project", "project": "p1", "device": "dev1"}));
    let m = c.expect("project data", |m| m["type"] == "project_data" || m["type"] == "error");
    assert_eq!(m["type"], "project_data", "{m}");
    assert_eq!(m["commit"], commit.as_str());
    let back = env.root.join("back.bundle");
    std::fs::write(&back, STANDARD.decode(m["bundle"].as_str().unwrap()).unwrap()).unwrap();
    let heads = git(&repo, &["bundle", "list-heads", back.to_str().unwrap()]);
    assert!(heads.contains(&commit) && heads.contains("refs/archon/wip/dev1"), "{heads}");

    // Ids from the network never become paths.
    c.send(json!({"type": "get_project", "project": "../p1", "device": "dev1"}));
    let m = c.expect("error", |m| m["type"] == "error");
    assert!(m["message"].as_str().unwrap().contains("bad project id"), "{m}");

    // The store lives in this test's data folder.
    assert!(env.data().join("cloud/projects/p1.git").is_dir());
    assert!(env.data().join("cloud/chats/dev1/c1.json").is_file());
}
