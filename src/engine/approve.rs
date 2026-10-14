//! Approval prompts for Claude Code's `--permission-prompt-tool`. ArchonUI is the MCP server that tool lives
//! on: `archonui --approval-mcp <socket> <session>` answers Claude's `approve` calls by forwarding each
//! one over a Unix socket to the running app, which asks the user and writes the answer back.
//!
//! The socket sits in an owner-only directory and each call carries this run's token (`ARCHONUI_APPROVAL_TOKEN`,
//! handed to the child in the 0600 MCP config), so another process cannot inject an approval or learn one.

use serde::{Deserialize, Serialize};
use super::mcp::PROTOCOL;
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    env, fs,
    io::{self, BufRead, BufReader, Write},
    os::unix::{
        fs::{DirBuilderExt, PermissionsExt},
        io::AsRawFd,
        net::{UnixListener, UnixStream},
    },
    path::{Path, PathBuf},
    process,
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc, Arc, Mutex, OnceLock,
    },
    thread,
    time::Duration,
};

/// How long an approval waits for the user before it is denied.
pub const TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// Name of the MCP tool Claude calls; with the server name it is `mcp__archonui__approve`.
pub const TOOL: &str = "approve";
/// Name of the MCP server in the `--mcp-config` file.
pub const SERVER: &str = "archonui";
/// Environment variable carrying this run's token to the `--approval-mcp` child.
pub const TOKEN_ENV: &str = "ARCHONUI_APPROVAL_TOKEN";
/// Longest request accepted on the socket or on the child's stdin. A longer line is refused. A `Write` of a
/// large file carries the whole file, JSON-escaped, so this sits well above what a model can emit in one turn.
const MAX_LINE: usize = 16 * 1024 * 1024;
/// Most approval connections served at once; beyond this a connection is dropped, which denies it.
const MAX_CONNECTIONS: usize = 64;
/// Longest the listener waits for a connection's request line. The child writes it as soon as it connects,
/// so a connection that stays silent is dropped rather than holding one of the `MAX_CONNECTIONS` slots.
const REQUEST_READ_TIMEOUT: Duration = Duration::from_secs(30);

/// Where a turn's approval requests go: this process's socket and the chat they belong to.
#[derive(Clone, Copy, Debug)]
pub struct Route<'a> {
    pub socket: &'a Path,
    pub session: &'a str,
}

/// The user's answer, in the shape Claude reads back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    Allow,
    Deny(String),
}

impl Verdict {
    /// `{"behavior":"allow","updatedInput":…}` or `{"behavior":"deny","message":…}`. Allowing runs the
    /// tool with the input Claude asked for.
    pub fn reply(&self, input: &Value) -> String {
        match self {
            Verdict::Allow => json!({"behavior": "allow", "updatedInput": input}),
            Verdict::Deny(message) => json!({"behavior": "deny", "message": message}),
        }
        .to_string()
    }
}

/// One tool use waiting for an answer. Dropping it without answering denies the call.
pub struct Request {
    pub session: String,
    pub tool_name: String,
    pub input: Value,
    answer: mpsc::Sender<Verdict>,
}

impl Request {
    /// Sends the answer. Ignored when the request has already timed out or been dropped.
    pub fn answer(&self, verdict: Verdict) {
        let _ = self.answer.send(verdict);
    }
}

/// Tools each chat allows for the rest of its session ("Always allow"). In memory only.
#[derive(Clone, Default)]
pub struct Allowances(Arc<Mutex<HashMap<String, HashSet<String>>>>);

impl Allowances {
    pub fn allow(&self, session: &str, tool: &str) {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).entry(session.into()).or_default().insert(tool.into());
    }

    pub fn allowed(&self, session: &str, tool: &str) -> bool {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).get(session).is_some_and(|tools| tools.contains(tool))
    }
}

/// The request as it crosses the socket, one JSON line each way. Only the app's own `--approval-mcp`
/// child knows `token`, so a call without it is refused.
#[derive(Serialize, Deserialize)]
struct Call {
    session: String,
    tool_name: String,
    input: Value,
    #[serde(default)]
    tool_use_id: String,
    #[serde(default)]
    token: String,
}

/// This run's token, shared with the `--approval-mcp` child through the MCP config's environment.
/// 32 random bytes, made once per process.
pub fn token() -> &'static str {
    static TOKEN: OnceLock<String> = OnceLock::new();
    TOKEN.get_or_init(random_token)
}

/// 32 random bytes as hex, from `/dev/urandom`; a time- and pid-mixed fallback if it cannot be read.
fn random_token() -> String {
    let mut bytes = [0u8; 32];
    if fs::File::open("/dev/urandom").is_ok_and(|mut f| io::Read::read_exact(&mut f, &mut bytes).is_ok()) {
        return bytes.iter().map(|b| format!("{b:02x}")).collect();
    }
    let mut x = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as u64)
        ^ (process::id() as u64).rotate_left(32);
    for b in &mut bytes {
        x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        *b = (x >> 33) as u8;
    }
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The token matches this run's, compared without an early exit.
fn token_ok(given: &str) -> bool {
    let (given, expected) = (given.as_bytes(), token().as_bytes());
    given.len() == expected.len() && given.iter().zip(expected).fold(0u8, |a, (x, y)| a | (x ^ y)) == 0
}

/// This process's approval socket. It lives in an owner-only directory: `$XDG_RUNTIME_DIR/archonui`,
/// else `approvals` inside the sessions folder. Never a world-writable directory.
pub fn socket() -> &'static Path {
    static SOCKET: OnceLock<PathBuf> = OnceLock::new();
    SOCKET.get_or_init(|| {
        let dir = socket_dir(env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from), crate::store::dir());
        dir.join(format!("archonui-{}.sock", process::id()))
    })
}

/// The directory for the socket, given the runtime dir (usually set) and the sessions folder.
fn socket_dir(runtime: Option<PathBuf>, sessions: PathBuf) -> PathBuf {
    runtime.map_or_else(|| sessions.join("approvals"), |dir| dir.join("archonui"))
}

/// True when the peer runs as this user. The 0700 socket directory already keeps other users out;
/// this makes it explicit.
fn same_user(stream: &UnixStream) -> bool {
    let mut cred = libc::ucred { pid: 0, uid: 0, gid: 0 };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    let ok = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut cred as *mut libc::ucred).cast(),
            &mut len,
        )
    };
    ok == 0 && cred.uid == unsafe { libc::geteuid() }
}

/// Answers approval requests on `socket` in background threads. A request no allowance covers goes
/// to `requests` and waits up to `timeout` for its answer; otherwise it is denied "Timed out".
pub fn listen(
    socket: &Path,
    requests: smol::channel::Sender<Request>,
    allowances: Allowances,
    timeout: Duration,
) -> io::Result<()> {
    if let Some(dir) = socket.parent() {
        fs::DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
        // `mode` applies only to a directory it creates; one left by an older run keeps its mode.
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
    }
    let _ = fs::remove_file(socket);
    if let Some(dir) = socket.parent() {
        remove_stale_sockets(dir);
    }
    let listener = bind_private(socket)?;
    let live = Arc::new(AtomicUsize::new(0));
    thread::spawn(move || {
        for stream in listener.incoming().map_while(Result::ok) {
            if live.fetch_add(1, Ordering::AcqRel) >= MAX_CONNECTIONS {
                live.fetch_sub(1, Ordering::AcqRel);
                continue;
            }
            let (requests, allowances, live) = (requests.clone(), allowances.clone(), live.clone());
            thread::spawn(move || {
                let _ = serve_call(stream, &requests, &allowances, timeout);
                live.fetch_sub(1, Ordering::AcqRel);
            });
        }
    });
    Ok(())
}

/// Removes `archonui-<pid>.sock` files whose process is gone, left by runs that were killed.
fn remove_stale_sockets(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for path in entries.filter_map(Result::ok).map(|e| e.path()) {
        let pid = path
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| n.strip_prefix("archonui-")?.strip_suffix(".sock")?.parse::<u32>().ok());
        if pid.is_some_and(|pid| !Path::new(&format!("/proc/{pid}")).exists()) {
            let _ = fs::remove_file(path);
        }
    }
}

/// Binds `path` with the socket created 0600. The umask spans only the bind, so there is no window
/// where another user could connect to a group- or other-accessible socket.
fn bind_private(path: &Path) -> io::Result<UnixListener> {
    let previous = unsafe { libc::umask(0o077) };
    let bound = UnixListener::bind(path);
    unsafe { libc::umask(previous) };
    let listener = bound?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(listener)
}

/// Reads one call from `stream`, gets its verdict and writes the verdict back. A peer that is not
/// this user, a call without this run's token, and a malformed or overlong line are all refused.
fn serve_call(
    mut stream: UnixStream,
    requests: &smol::channel::Sender<Request>,
    allowances: &Allowances,
    timeout: Duration,
) -> io::Result<()> {
    if !same_user(&stream) {
        return Ok(());
    }
    stream.set_read_timeout(Some(timeout.min(REQUEST_READ_TIMEOUT)))?;
    let line = read_line(&mut BufReader::new(&stream), MAX_LINE)?;
    let Some(line) = line else { return Ok(()) };
    let call: Call = serde_json::from_slice(&line).map_err(io::Error::other)?;
    if !token_ok(&call.token) {
        let reply = Verdict::Deny("ArchonUI did not send this approval request.".into()).reply(&call.input);
        writeln!(stream, "{reply}")?;
        return Ok(());
    }
    let verdict = if allowances.allowed(&call.session, &call.tool_name) {
        Verdict::Allow
    } else {
        let (answer, waiting) = mpsc::channel();
        let request = Request { session: call.session, tool_name: call.tool_name, input: call.input.clone(), answer };
        if requests.send_blocking(request).is_err() {
            Verdict::Deny("ArchonUI is not showing approvals.".into())
        } else {
            match waiting.recv_timeout(timeout) {
                Ok(verdict) => verdict,
                Err(mpsc::RecvTimeoutError::Timeout) => Verdict::Deny("Timed out".into()),
                Err(mpsc::RecvTimeoutError::Disconnected) => Verdict::Deny("The chat stopped before an answer.".into()),
            }
        }
    };
    writeln!(stream, "{}", verdict.reply(&call.input))
}

/// Runs the approval MCP server on stdin and stdout until stdin closes. Each `approve` call is asked of
/// the app at `socket` for chat `session`, carrying this run's token from the environment.
pub fn run_server(socket: &Path, session: &str) -> io::Result<()> {
    let token = env::var(TOKEN_ENV).unwrap_or_default();
    let ask = |args: &Value| ask_app(socket, session, &token, args);
    serve(io::stdin().lock(), io::stdout().lock(), ask)
}

/// Forwards one `approve` call's arguments to the app and returns its answer. A call the app cannot
/// answer is denied, with the reason as the message.
fn ask_app(socket: &Path, session: &str, token: &str, args: &Value) -> String {
    let deny = |message: &str| Verdict::Deny(message.into()).reply(&args["input"]);
    let Some(tool_name) = args["tool_name"].as_str().filter(|t| !t.is_empty()) else {
        return deny("The approval request has no tool name.");
    };
    let call = Call {
        session: session.into(),
        tool_name: tool_name.into(),
        input: args["input"].clone(),
        tool_use_id: args["tool_use_id"].as_str().unwrap_or_default().into(),
        token: token.into(),
    };
    let exchange = || -> io::Result<String> {
        let mut stream = UnixStream::connect(socket)?;
        stream.set_read_timeout(Some(TIMEOUT))?;
        writeln!(stream, "{}", serde_json::to_string(&call).map_err(io::Error::other)?)?;
        let mut reply = String::new();
        BufReader::new(stream).read_line(&mut reply)?;
        if reply.trim().is_empty() {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        Ok(reply.trim().to_string())
    };
    exchange().unwrap_or_else(|e| match e.kind() {
        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut => deny("Timed out"),
        _ => deny("ArchonUI is not running approvals."),
    })
}

/// Answers JSON-RPC messages read from `input`, one per line, writing each response to `out`.
/// `ask` answers an `approve` call's arguments with the text Claude reads back.
pub fn serve(input: impl BufRead, out: impl Write, ask: impl FnMut(&Value) -> String) -> io::Result<()> {
    serve_within(input, out, ask, MAX_LINE)
}

/// `serve` with the longest line given. An overlong line is skipped and answered with an error, so one huge
/// message does not take the server, and every later approval of the turn, down with it.
fn serve_within(
    mut input: impl BufRead,
    mut out: impl Write,
    mut ask: impl FnMut(&Value) -> String,
    max: usize,
) -> io::Result<()> {
    loop {
        let line = match read_line(&mut input, max) {
            Ok(Some(line)) => line,
            Ok(None) => return Ok(()),
            Err(e) if e.kind() == io::ErrorKind::InvalidData => {
                skip_line(&mut input)?;
                writeln!(out, "{}", error_reply(Value::Null, -32600, "the message is too long"))?;
                out.flush()?;
                continue;
            }
            Err(e) => return Err(e),
        };
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        let reply = match serde_json::from_slice::<Value>(&line) {
            Ok(msg) => respond(&msg, &mut ask),
            Err(e) => Some(error_reply(Value::Null, -32700, &format!("parse error: {e}"))),
        };
        if let Some(reply) = reply {
            writeln!(out, "{reply}")?;
            out.flush()?;
        }
    }
}

/// Reads one line, newline included, of at most `max` bytes. `None` at end of input. A line that
/// reaches `max` without a newline is an error, so a client cannot make us buffer without bound.
fn read_line(reader: &mut impl BufRead, max: usize) -> io::Result<Option<Vec<u8>>> {
    let mut line = Vec::new();
    loop {
        let available = match reader.fill_buf() {
            Ok(available) => available,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        if available.is_empty() {
            return Ok((!line.is_empty()).then_some(line));
        }
        let (done, used) = match available.iter().position(|&b| b == b'\n') {
            Some(i) => (true, i + 1),
            None => (false, available.len()),
        };
        if line.len() + used > max {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "the approval request is too long"));
        }
        line.extend_from_slice(&available[..used]);
        reader.consume(used);
        if done {
            return Ok(Some(line));
        }
    }
}

/// Discards input up to and including the next newline, without buffering it.
fn skip_line(reader: &mut impl BufRead) -> io::Result<()> {
    loop {
        let available = match reader.fill_buf() {
            Ok(available) => available,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        if available.is_empty() {
            return Ok(());
        }
        match available.iter().position(|&b| b == b'\n') {
            Some(i) => {
                reader.consume(i + 1);
                return Ok(());
            }
            None => {
                let n = available.len();
                reader.consume(n);
            }
        }
    }
}

/// The response to one JSON-RPC message, or `None` for a notification (a message without an id).
pub fn respond(msg: &Value, ask: &mut dyn FnMut(&Value) -> String) -> Option<Value> {
    let id = msg.get("id")?.clone();
    let method = msg["method"].as_str().unwrap_or_default();
    let result = match method {
        "initialize" => Ok(json!({
            "protocolVersion": PROTOCOL,
            "capabilities": {"tools": {}},
            "serverInfo": {"name": SERVER, "version": env!("CARGO_PKG_VERSION")},
        })),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({"tools": [approve_tool()]})),
        "tools/call" if msg["params"]["name"] == TOOL => {
            let text = ask(&msg["params"]["arguments"]);
            Ok(json!({"content": [{"type": "text", "text": text}]}))
        }
        "tools/call" => Err((-32602, format!("unknown tool: {}", msg["params"]["name"]))),
        other => Err((-32601, format!("method not found: {other}"))),
    };
    Some(match result {
        Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
        Err((code, message)) => error_reply(id, code, &message),
    })
}

fn error_reply(id: Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

/// The one tool the server offers: the arguments Claude's `--permission-prompt-tool` contract sends.
fn approve_tool() -> Value {
    json!({
        "name": TOOL,
        "description": "Asks the ArchonUI user whether a tool use may run.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "tool_name": {"type": "string"},
                "input": {"type": "object"},
                "tool_use_id": {"type": "string"},
            },
            "required": ["tool_name", "input"],
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn no_ask(_: &Value) -> String {
        unreachable!("no approve call expected")
    }

    fn call(method: &str, id: Option<u64>, params: Value) -> Value {
        let mut msg = json!({"jsonrpc": "2.0", "method": method, "params": params});
        if let Some(id) = id {
            msg["id"] = json!(id);
        }
        msg
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = env::temp_dir().join(format!("archonui-approve-{name}-{}", process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn initialize_and_list_offer_one_approve_tool() {
        let mut ask = no_ask;
        let init = respond(&call("initialize", Some(1), json!({})), &mut ask).unwrap();
        assert_eq!(init["id"], 1);
        assert_eq!(init["result"]["protocolVersion"], PROTOCOL);
        assert_eq!(init["result"]["serverInfo"]["name"], SERVER);
        let list = respond(&call("tools/list", Some(2), json!({})), &mut ask).unwrap();
        let tools = list["result"]["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["name"], TOOL);
        assert_eq!(tools[0]["inputSchema"]["required"], json!(["tool_name", "input"]));
    }

    #[test]
    fn notifications_get_no_reply_and_unknown_methods_get_errors() {
        let mut ask = no_ask;
        assert!(respond(&call("notifications/initialized", None, json!({})), &mut ask).is_none());
        let missing = respond(&call("resources/list", Some(7), json!({})), &mut ask).unwrap();
        assert_eq!(missing["error"]["code"], -32601);
        assert_eq!(missing["id"], 7);
        let wrong = respond(&call("tools/call", Some(8), json!({"name": "other"})), &mut ask).unwrap();
        assert_eq!(wrong["error"]["code"], -32602);
    }

    #[test]
    fn approve_call_passes_arguments_and_returns_the_answer_as_text() {
        let mut seen = Vec::new();
        let mut ask = |args: &Value| {
            seen.push(args.clone());
            Verdict::Allow.reply(&args["input"])
        };
        let params = json!({"name": TOOL, "arguments": {"tool_name": "Bash", "input": {"command": "echo hi"}}});
        let reply = respond(&call("tools/call", Some(3), params), &mut ask).unwrap();
        assert_eq!(seen[0]["tool_name"], "Bash");
        let text = reply["result"]["content"][0]["text"].as_str().unwrap();
        assert_eq!(text, r#"{"behavior":"allow","updatedInput":{"command":"echo hi"}}"#);
    }

    #[test]
    fn serve_reads_lines_and_skips_blank_ones() {
        let input = concat!(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#, "\n",
            "\n",
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#, "\n",
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#, "\n",
        );
        let mut out = Vec::new();
        serve(Cursor::new(input), &mut out, no_ask).unwrap();
        let replies: Vec<Value> =
            String::from_utf8(out).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
        assert_eq!(replies.len(), 2, "the notification gets no reply");
        assert_eq!(replies[0]["id"], 1);
        assert_eq!(replies[1]["id"], 2);
    }

    #[test]
    fn malformed_json_gets_a_parse_error() {
        let mut out = Vec::new();
        serve(Cursor::new("{not json\n"), &mut out, no_ask).unwrap();
        let reply: Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(reply["error"]["code"], -32700);
        assert!(reply["id"].is_null());
    }

    #[test]
    fn allowances_are_per_session_and_per_tool() {
        let allowances = Allowances::default();
        allowances.allow("chat-a", "Bash");
        assert!(allowances.allowed("chat-a", "Bash"));
        assert!(!allowances.allowed("chat-a", "Edit"));
        assert!(!allowances.allowed("chat-b", "Bash"));
    }

    #[test]
    fn socket_round_trip_asks_the_app_and_returns_its_answer() {
        let dir = scratch("round-trip");
        let path = dir.join("app.sock");
        let (requests, incoming) = smol::channel::unbounded();
        let allowances = Allowances::default();
        listen(&path, requests, allowances.clone(), Duration::from_secs(5)).unwrap();

        // The "UI": answers the first request with a deny, then the caller sees the message.
        let ui = thread::spawn(move || {
            let request = smol::block_on(incoming.recv()).unwrap();
            assert_eq!(request.session, "chat-a");
            assert_eq!(request.tool_name, "Bash");
            assert_eq!(request.input["command"], "rm -rf /tmp/x");
            request.answer(Verdict::Deny("no".into()));
        });
        let args = json!({"tool_name": "Bash", "input": {"command": "rm -rf /tmp/x"}, "tool_use_id": "t1"});
        let reply: Value = serde_json::from_str(&ask_app(&path, "chat-a", token(), &args)).unwrap();
        ui.join().unwrap();
        assert_eq!(reply, json!({"behavior": "deny", "message": "no"}));

        // An allowance answers without asking the UI at all.
        allowances.allow("chat-a", "Bash");
        let reply: Value = serde_json::from_str(&ask_app(&path, "chat-a", token(), &args)).unwrap();
        assert_eq!(reply, json!({"behavior": "allow", "updatedInput": {"command": "rm -rf /tmp/x"}}));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_call_without_this_runs_token_is_refused() {
        let dir = scratch("no-token");
        let path = dir.join("app.sock");
        let (requests, incoming) = smol::channel::unbounded();
        let allowances = Allowances::default();
        listen(&path, requests, allowances.clone(), Duration::from_secs(5)).unwrap();
        let args = json!({"tool_name": "Bash", "input": {"command": "echo spoofed"}});
        // The child of another run, or a process that never saw the token.
        let wrong = "ab".repeat(32);
        for token in ["", "00", wrong.as_str()] {
            let reply: Value = serde_json::from_str(&ask_app(&path, "chat-a", token, &args)).unwrap();
            assert_eq!(reply["behavior"], "deny", "token {token:?}");
        }
        assert!(incoming.try_recv().is_err(), "no card may be queued");
        assert!(!allowances.allowed("chat-a", "Bash"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_socket_message_cannot_set_an_allowance() {
        let dir = scratch("always");
        let path = dir.join("app.sock");
        let (requests, incoming) = smol::channel::unbounded();
        let allowances = Allowances::default();
        listen(&path, requests, allowances.clone(), Duration::from_millis(200)).unwrap();
        let ui = thread::spawn(move || {
            let request = smol::block_on(incoming.recv()).unwrap();
            request.answer(Verdict::Allow);
        });
        // "always" is not part of the protocol: the cache is only set by the Allow / Always allow buttons.
        let mut stream = UnixStream::connect(&path).unwrap();
        let call = json!({
            "session": "chat-a", "tool_name": "Bash", "input": {"command": "echo hi"},
            "token": token(), "always": true,
        });
        writeln!(stream, "{call}").unwrap();
        let mut reply = String::new();
        BufReader::new(stream).read_line(&mut reply).unwrap();
        ui.join().unwrap();
        assert_eq!(serde_json::from_str::<Value>(&reply).unwrap()["behavior"], "allow");
        assert!(!allowances.allowed("chat-a", "Bash"), "a socket message must not cache an allowance");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_overlong_call_is_refused() {
        let dir = scratch("overlong");
        let path = dir.join("app.sock");
        let (requests, incoming) = smol::channel::unbounded();
        let allowances = Allowances::default();
        listen(&path, requests, allowances, Duration::from_secs(5)).unwrap();
        let mut stream = UnixStream::connect(&path).unwrap();
        let call = json!({"session": "chat-a", "tool_name": "Bash", "input": {"command": "x".repeat(MAX_LINE)}});
        let _ = writeln!(stream, "{call}");
        // The listener drops the connection without answering, which the child reads as a deny. Closing
        // while the client is still writing can surface as a reset instead of end-of-file.
        let mut reply = String::new();
        assert_eq!(BufReader::new(stream).read_line(&mut reply).unwrap_or(0), 0, "no answer is written");
        assert!(reply.is_empty(), "{reply}");
        assert!(incoming.try_recv().is_err(), "no card may be queued");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_socket_directory_is_owner_only_and_never_the_temp_dir() {
        let runtime = socket_dir(Some(PathBuf::from("/run/user/1000")), PathBuf::from("/data/sessions"));
        assert_eq!(runtime, Path::new("/run/user/1000/archonui"));
        let fallback = socket_dir(None, PathBuf::from("/data/sessions"));
        assert_eq!(fallback, Path::new("/data/sessions/approvals"));
        assert!(!fallback.starts_with(env::temp_dir()));

        let dir = scratch("socket-dir");
        let socket = dir.join("nested/archonui-1.sock");
        let (requests, _incoming) = smol::channel::unbounded();
        listen(&socket, requests, Allowances::default(), Duration::from_millis(50)).unwrap();
        let parent = fs::metadata(socket.parent().unwrap()).unwrap().permissions().mode() & 0o777;
        assert_eq!(parent, 0o700, "the socket's directory is owner-only");
        let mode = fs::metadata(&socket).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "the socket is owner-only");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_line_stops_at_the_cap() {
        let mut reader = Cursor::new(vec![b'a'; 32]);
        assert!(read_line(&mut reader, 16).is_err());
        let mut reader = Cursor::new(b"short\nlonger line\n".to_vec());
        assert_eq!(read_line(&mut reader, 16).unwrap().unwrap(), b"short\n".to_vec());
        assert_eq!(read_line(&mut reader, 16).unwrap().unwrap(), b"longer line\n".to_vec());
        assert!(read_line(&mut reader, 16).unwrap().is_none());
    }

    #[test]
    fn an_unanswered_request_times_out() {
        let dir = scratch("timeout");
        let path = dir.join("app.sock");
        let (requests, incoming) = smol::channel::unbounded();
        listen(&path, requests, Allowances::default(), Duration::from_millis(100)).unwrap();
        // Hold the request without answering it, so the listener's timer decides.
        let (held_tx, _held) = mpsc::channel();
        thread::spawn(move || held_tx.send(smol::block_on(incoming.recv()).unwrap()));
        let args = json!({"tool_name": "Edit", "input": {}});
        let reply: Value = serde_json::from_str(&ask_app(&path, "chat-a", token(), &args)).unwrap();
        assert_eq!(reply, json!({"behavior": "deny", "message": "Timed out"}));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_overlong_stdin_message_is_answered_and_the_server_keeps_going() {
        let input = format!(
            "{}\n{}\n",
            json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"x": "y".repeat(64)}}),
            r#"{"jsonrpc":"2.0","id":2,"method":"ping"}"#,
        );
        let mut out = Vec::new();
        serve_within(Cursor::new(input), &mut out, no_ask, 64).unwrap();
        let replies: Vec<Value> =
            String::from_utf8(out).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
        assert_eq!(replies.len(), 2, "{replies:?}");
        assert_eq!(replies[0]["error"]["code"], -32600);
        assert_eq!(replies[1]["id"], 2);
        assert_eq!(replies[1]["result"], json!({}));
    }

    #[test]
    fn a_silent_connection_is_dropped() {
        let dir = scratch("silent");
        let path = dir.join("app.sock");
        let (requests, _incoming) = smol::channel::unbounded();
        listen(&path, requests, Allowances::default(), Duration::from_millis(200)).unwrap();
        let stream = UnixStream::connect(&path).unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let started = std::time::Instant::now();
        let mut buf = String::new();
        // The listener gives up on the request line and closes; the read sees end-of-file, not our timeout.
        assert_eq!(BufReader::new(stream).read_line(&mut buf).unwrap(), 0);
        assert!(started.elapsed() < Duration::from_secs(4));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_existing_socket_directory_is_made_owner_only() {
        let dir = scratch("loose-dir");
        let parent = dir.join("sockets");
        fs::create_dir(&parent).unwrap();
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o755)).unwrap();
        let (requests, _incoming) = smol::channel::unbounded();
        listen(&parent.join("archonui-1.sock"), requests, Allowances::default(), Duration::from_millis(50)).unwrap();
        assert_eq!(fs::metadata(&parent).unwrap().permissions().mode() & 0o777, 0o700);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn same_user_identifies_local_stream() {
        let (a, b) = UnixStream::pair().unwrap();
        assert!(same_user(&a));
        assert!(same_user(&b));
    }

    #[test]
    fn concurrent_connections_are_capped() {
        let dir = scratch("concurrency");
        let path = dir.join("app.sock");
        let (requests, _incoming) = smol::channel::unbounded();
        listen(&path, requests, Allowances::default(), Duration::from_secs(5)).unwrap();

        // Fill up to MAX_CONNECTIONS with held connections (no newline sent).
        let mut held = Vec::new();
        for _ in 0..MAX_CONNECTIONS {
            held.push(UnixStream::connect(&path).unwrap());
        }

        // The next connection exceeds MAX_CONNECTIONS and must be dropped immediately.
        let overflow = UnixStream::connect(&path).unwrap();
        overflow.set_read_timeout(Some(Duration::from_millis(500))).unwrap();
        let mut buf = String::new();
        assert_eq!(BufReader::new(overflow).read_line(&mut buf).unwrap_or(0), 0);
        assert!(buf.is_empty(), "overflow connection was dropped");

        drop(held);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_app_denies_the_call() {
        let args = json!({"tool_name": "Bash", "input": {}});
        let reply: Value =
            serde_json::from_str(&ask_app(Path::new("/nonexistent/archonui.sock"), "chat-a", token(), &args)).unwrap();
        assert_eq!(reply["behavior"], "deny");
    }
}
