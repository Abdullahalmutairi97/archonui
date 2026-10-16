//! MCP servers: importing them from each harness's own config, checking them, and passing ArchonUI's
//! list to harnesses at launch.
#![allow(dead_code)]

use crate::app::settings::{McpServer, McpTransport};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, HashSet},
    env, fs,
    io::{BufRead, BufReader, Write},
    iter::Peekable,
    os::unix::process::CommandExt,
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    str::Chars,
    sync::mpsc::{self, Receiver, RecvTimeoutError},
    thread,
    time::{Duration, Instant},
};

/// Whether a server answered.
#[derive(Clone, Debug, PartialEq)]
pub enum McpStatus {
    Connected { tools: usize },
    Failed(String),
    Off,
}

/// Longest a check may take, from starting the server to its last `tools/list` page.
const CHECK_TIMEOUT: Duration = Duration::from_secs(10);
/// Protocol revision sent in `initialize` and in the `MCP-Protocol-Version` header.
pub(crate) const PROTOCOL: &str = "2025-06-18";

/// Servers already configured in the installed harnesses, read from their config files. Earlier
/// sources win when two share a name; disabled servers come back with `enabled: false`.
pub fn import_servers(cwd: &Path) -> Vec<McpServer> {
    let Some(home) = env::var_os("HOME").map(PathBuf::from) else { return Vec::new() };
    let codex_home = env::var_os("CODEX_HOME").map(PathBuf::from).unwrap_or_else(|| home.join(".codex"));
    import_in(&home, &codex_home, cwd)
}

/// `import_servers` with the home directory and Codex's config dir given.
fn import_in(home: &Path, codex_home: &Path, cwd: &Path) -> Vec<McpServer> {
    let sources = [
        claude(home, cwd),
        codex(codex_home),
        opencode(home),
        pi(home),
        gemini(home),
    ];
    let mut seen = HashSet::new();
    sources.into_iter().flatten().filter(|s| seen.insert(s.name.clone())).collect()
}

/// Claude: `mcpServers` at the top of `~/.claude.json`, under the project's entry there, and in
/// the project's `.mcp.json`.
fn claude(home: &Path, cwd: &Path) -> Vec<McpServer> {
    let mut out = Vec::new();
    if let Some(v) = read_json(&home.join(".claude.json")) {
        out.extend(servers_in(&v["mcpServers"], json_server));
        out.extend(servers_in(&v["projects"][cwd.display().to_string()]["mcpServers"], json_server));
    }
    if let Some(v) = read_json(&cwd.join(".mcp.json")) {
        out.extend(servers_in(&v["mcpServers"], json_server));
    }
    out
}

/// Codex: `[mcp_servers.<name>]` in `config.toml`, converted to JSON so the shared reader applies.
fn codex(codex_home: &Path) -> Vec<McpServer> {
    let Some(table) = fs::read_to_string(codex_home.join("config.toml"))
        .ok()
        .and_then(|text| toml::from_str::<toml::Table>(&text).ok())
    else {
        return Vec::new();
    };
    let Some(servers) = table.get("mcp_servers").and_then(toml::Value::as_table) else { return Vec::new() };
    servers
        .iter()
        .filter_map(|(name, v)| {
            let mut v = serde_json::to_value(v).ok()?;
            if let Some(headers) = v.get("http_headers").cloned() {
                v["headers"] = headers;
            }
            json_server(name, &v)
        })
        .collect()
}

/// OpenCode: the `mcp` map in `opencode.json`, or `.jsonc` with comments.
fn opencode(home: &Path) -> Vec<McpServer> {
    let dir = home.join(".config/opencode");
    let config = ["opencode.json", "opencode.jsonc"].iter().find_map(|f| read_jsonc(&dir.join(f)));
    config.map_or_else(Vec::new, |v| servers_in(&v["mcp"], opencode_server))
}

/// Pi: `mcpServers` in `~/.pi/agent/mcp.json`.
fn pi(home: &Path) -> Vec<McpServer> {
    read_json(&home.join(".pi/agent/mcp.json")).map_or_else(Vec::new, |v| servers_in(&v["mcpServers"], json_server))
}

/// Gemini: `mcpServers` in `~/.gemini/settings.json`. Gemini's `url` is SSE, which is not imported;
/// its streamable HTTP field is `httpUrl`.
fn gemini(home: &Path) -> Vec<McpServer> {
    read_json(&home.join(".gemini/settings.json"))
        .map_or_else(Vec::new, |v| servers_in(&v["mcpServers"], |n, v| json_transport(n, v, "httpUrl")))
}

/// A server in the `mcpServers` shape used by Claude and Pi.
fn json_server(name: &str, v: &Value) -> Option<McpServer> {
    json_transport(name, v, "url")
}

/// `json_server` with `url_key` naming the streamable HTTP field. Only `stdio` and `http` types
/// are imported; `sse` servers are skipped.
fn json_transport(name: &str, v: &Value, url_key: &str) -> Option<McpServer> {
    let kind = v["type"].as_str().unwrap_or("");
    let transport = if let Some(command) = v["command"].as_str().filter(|_| matches!(kind, "" | "stdio")) {
        McpTransport::Stdio { command: command.into(), args: strings(&v["args"]), env: string_map(&v["env"]) }
    } else if let Some(url) = v[url_key].as_str().filter(|_| matches!(kind, "" | "http" | "streamable-http")) {
        McpTransport::Http { url: url.into(), headers: string_map(&v["headers"]) }
    } else {
        return None;
    };
    Some(McpServer { id: name.into(), name: name.into(), transport, enabled: v["enabled"] != false })
}

/// An OpenCode `mcp` entry: `local` runs `command` (program and arguments in one array), `remote`
/// is a URL.
fn opencode_server(name: &str, v: &Value) -> Option<McpServer> {
    let transport = match v["type"].as_str()? {
        "local" => {
            let command = strings(&v["command"]);
            let (program, args) = command.split_first()?;
            McpTransport::Stdio { command: program.clone(), args: args.to_vec(), env: string_map(&v["environment"]) }
        }
        "remote" => McpTransport::Http { url: v["url"].as_str()?.into(), headers: string_map(&v["headers"]) },
        _ => return None,
    };
    Some(McpServer { id: name.into(), name: name.into(), transport, enabled: v["enabled"] != false })
}

/// The servers under `map`, parsed one by one; entries that do not parse are skipped.
fn servers_in(map: &Value, parse: impl Fn(&str, &Value) -> Option<McpServer>) -> Vec<McpServer> {
    map.as_object().into_iter().flatten().filter_map(|(name, v)| parse(name, v)).collect()
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array().into_iter().flatten().filter_map(Value::as_str).map(String::from).collect()
}

fn string_map(v: &Value) -> BTreeMap<String, String> {
    v.as_object()
        .into_iter()
        .flatten()
        .filter_map(|(k, x)| Some((k.clone(), x.as_str()?.to_string())))
        .collect()
}

fn read_json(path: &Path) -> Option<Value> {
    serde_json::from_str(&fs::read_to_string(path).ok()?).ok()
}

fn read_jsonc(path: &Path) -> Option<Value> {
    serde_json::from_str(&strip_jsonc(&fs::read_to_string(path).ok()?)).ok()
}

/// Removes `//` and `/* */` comments and trailing commas, leaving string contents alone.
fn strip_jsonc(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut in_string = false;
    while let Some(c) = chars.next() {
        if in_string {
            out.push(c);
            match c {
                '\\' => out.extend(chars.next()),
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match c {
            '"' => {
                in_string = true;
                out.push(c);
            }
            '/' if chars.peek() == Some(&'/') => {
                while chars.next_if(|&n| n != '\n').is_some() {}
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                let mut prev = '\0';
                for n in chars.by_ref() {
                    if prev == '*' && n == '/' {
                        break;
                    }
                    prev = n;
                }
            }
            ',' => {
                // A comma before `}` or `]` is dropped; JSON does not allow it.
                if !matches!(next_token(chars.clone()), Some('}' | ']')) {
                    out.push(c);
                }
            }
            _ => out.push(c),
        }
    }
    out
}

/// The first char of `ahead` that is neither whitespace nor part of a comment.
fn next_token(mut ahead: Peekable<Chars>) -> Option<char> {
    loop {
        match ahead.next()? {
            c if c.is_whitespace() => {}
            '/' if ahead.peek() == Some(&'/') => {
                ahead.find(|&n| n == '\n');
            }
            '/' if ahead.peek() == Some(&'*') => {
                ahead.next();
                let mut prev = '\0';
                for n in ahead.by_ref() {
                    if prev == '*' && n == '/' {
                        break;
                    }
                    prev = n;
                }
            }
            c => return Some(c),
        }
    }
}

/// `server` in the `mcpServers` shape Claude and Pi read. `typed` adds Claude's `type` field.
pub(crate) fn json_entry(server: &McpServer, typed: bool) -> Value {
    let (mut v, kind) = match &server.transport {
        McpTransport::Stdio { command, args, env } => (json!({"command": command, "args": args, "env": env}), "stdio"),
        McpTransport::Http { url, headers } => (json!({"url": url, "headers": headers}), "http"),
    };
    if typed {
        v["type"] = json!(kind);
    }
    v
}

/// The `mcpServers` file a harness reads for the enabled `servers`, as (path, text). It lives in
/// `mcp/<name>` beside the sessions folder and is rewritten each turn, so tokens stay out of argv.
/// `None` when no server is enabled.
pub(crate) fn config_file(name: &str, servers: &[McpServer], typed: bool) -> Option<(PathBuf, String)> {
    let map = enabled_entries(servers, typed);
    (!map.is_empty()).then(|| mcp_file(name, map))
}

/// `config_file` with `extra` entries (server name and entry) added, so it is always written.
pub(crate) fn config_file_with(
    name: &str,
    servers: &[McpServer],
    typed: bool,
    extra: Vec<(String, Value)>,
) -> (PathBuf, String) {
    let mut map = enabled_entries(servers, typed);
    map.extend(extra);
    mcp_file(name, map)
}

fn enabled_entries(servers: &[McpServer], typed: bool) -> serde_json::Map<String, Value> {
    servers.iter().filter(|s| s.enabled).map(|s| (s.name.clone(), json_entry(s, typed))).collect()
}

fn mcp_file(name: &str, map: serde_json::Map<String, Value>) -> (PathBuf, String) {
    (file_path(name), json!({"mcpServers": map}).to_string())
}

/// `name` in the `mcp` folder beside the sessions folder, where the files a turn hands its harness live.
pub(crate) fn file_path(name: &str) -> PathBuf {
    crate::store::dir().with_file_name("mcp").join(name)
}

/// The per-chat `--mcp-config` file Claude reads, so a deleted chat can remove it.
pub fn session_file(session: &str) -> PathBuf {
    file_path(&format!("claude-{session}.json"))
}

/// Starts or calls the server and asks for its tools. Blocking, with a 10 s limit. A disabled
/// server is not contacted.
pub fn check(server: &McpServer) -> McpStatus {
    check_within(server, CHECK_TIMEOUT)
}

fn check_within(server: &McpServer, timeout: Duration) -> McpStatus {
    if !server.enabled {
        return McpStatus::Off;
    }
    let deadline = Instant::now() + timeout;
    let result = match &server.transport {
        McpTransport::Stdio { command, args, env } => {
            StdioServer::start(command, args, env, deadline).and_then(|mut s| handshake(&mut s))
        }
        McpTransport::Http { url, headers } => handshake(&mut HttpServer::new(url, headers, deadline)),
    };
    match result {
        Ok(tools) => McpStatus::Connected { tools },
        Err(e) => McpStatus::Failed(e),
    }
}

/// A channel to one server that carries JSON-RPC messages.
trait Exchange {
    /// Sends a request and returns the `result` of the reply with the same `id`.
    fn request(&mut self, id: u64, method: &str, params: Value) -> Result<Value, String>;
    /// Sends a notification. The server sends no reply.
    fn notify(&mut self, method: &str) -> Result<(), String>;
}

/// `initialize`, `notifications/initialized`, then every `tools/list` page. Returns the tool count.
fn handshake(conn: &mut dyn Exchange) -> Result<usize, String> {
    let init = json!({
        "protocolVersion": PROTOCOL,
        "capabilities": {},
        "clientInfo": {"name": "archonui", "version": env!("CARGO_PKG_VERSION")},
    });
    conn.request(1, "initialize", init)?;
    conn.notify("notifications/initialized")?;
    let mut tools = 0;
    let mut cursor: Option<String> = None;
    let mut id = 1;
    loop {
        id += 1;
        let params = cursor.as_ref().map_or_else(|| json!({}), |c| json!({"cursor": c}));
        let page = conn.request(id, "tools/list", params)?;
        tools += page["tools"].as_array().map_or(0, Vec::len);
        cursor = page["nextCursor"].as_str().map(String::from);
        if cursor.is_none() {
            return Ok(tools);
        }
    }
}

/// A server started as a command and spoken to over its stdin and stdout, one JSON message per line.
/// Dropping it kills the command and anything it started.
struct StdioServer {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<String>,
    deadline: Instant,
}

impl StdioServer {
    fn start(
        command: &str,
        args: &[String],
        env: &BTreeMap<String, String>,
        deadline: Instant,
    ) -> Result<Self, String> {
        let mut child = Command::new(command)
            .args(args)
            .envs(env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()
            .map_err(|e| format!("could not start {command}: {e}"))?;
        let stdin = child.stdin.take().expect("piped stdin");
        let stdout = child.stdout.take().expect("piped stdout");
        let (tx, lines) = mpsc::channel();
        // The reader ends at EOF, which happens once the child is killed. A line that is not UTF-8 (a log
        // line) is passed on lossily rather than ending the reader, which would read as the server exiting.
        thread::spawn(move || {
            let mut stdout = BufReader::new(stdout);
            let mut line = Vec::new();
            while stdout.read_until(b'\n', &mut line).is_ok_and(|n| n > 0) {
                let text = String::from_utf8_lossy(&line).trim_end_matches(['\r', '\n']).to_string();
                line.clear();
                if tx.send(text).is_err() {
                    break;
                }
            }
        });
        Ok(Self { child, stdin, lines, deadline })
    }

    fn send(&mut self, msg: &Value) -> Result<(), String> {
        self.stdin
            .write_all(format!("{msg}\n").as_bytes())
            .and_then(|()| self.stdin.flush())
            .map_err(|e| format!("could not write to the server: {e}"))
    }
}

impl Drop for StdioServer {
    fn drop(&mut self) {
        // The child is still ours: it has not been waited on, so its pid cannot be reused.
        unsafe { libc::kill(-(self.child.id() as i32), libc::SIGKILL) };
        let _ = self.child.wait();
    }
}

impl Exchange for StdioServer {
    fn request(&mut self, id: u64, method: &str, params: Value) -> Result<Value, String> {
        self.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))?;
        loop {
            // Checked on every line too, so a server that keeps printing cannot hold the check past its limit.
            let left = self.deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(format!("no reply to {method} in time"));
            }
            let line = match self.lines.recv_timeout(left) {
                Ok(line) => line,
                Err(RecvTimeoutError::Timeout) => return Err(format!("no reply to {method} in time")),
                Err(RecvTimeoutError::Disconnected) => return Err(format!("the server exited before replying to {method}")),
            };
            // Servers may print other lines (logs, notifications); only the reply matters.
            let Ok(msg) = serde_json::from_str::<Value>(&line) else { continue };
            // A request from the server (`roots/list`, `ping`) has its own id, which may equal ours.
            if msg["id"] != id || msg.get("method").is_some() {
                continue;
            }
            return reply_result(method, &msg);
        }
    }

    fn notify(&mut self, method: &str) -> Result<(), String> {
        self.send(&json!({"jsonrpc": "2.0", "method": method}))
    }
}

/// A server reached over streamable HTTP. Requests go through `curl`, which handles TLS and keeps
/// the session id the server hands out.
struct HttpServer<'a> {
    url: &'a str,
    headers: &'a BTreeMap<String, String>,
    session: Option<String>,
    deadline: Instant,
}

impl<'a> HttpServer<'a> {
    fn new(url: &'a str, headers: &'a BTreeMap<String, String>, deadline: Instant) -> Self {
        Self { url, headers, session: None, deadline }
    }

    /// POSTs one message and returns the JSON-RPC messages in the answer (none for a notification).
    fn post(&mut self, body: &Value) -> Result<Vec<Value>, String> {
        let left = self.deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err("no reply in time".into());
        }
        // Headers (bearer tokens), the URL (it may carry a key) and the body go to curl as a config on
        // its stdin, not argv: `/proc/<pid>/cmdline` is readable by every local user.
        let mut headers = vec![
            "Content-Type: application/json".to_string(),
            "Accept: application/json, text/event-stream".into(),
            format!("MCP-Protocol-Version: {PROTOCOL}"),
        ];
        if let Some(session) = &self.session {
            headers.push(format!("Mcp-Session-Id: {session}"));
        }
        headers.extend(self.headers.iter().map(|(name, value)| format!("{name}: {value}")));
        let config = curl_config(self.url, &headers, &body.to_string());
        let mut child = Command::new("curl")
            .args(["-sS", "-i", "-X", "POST", "--max-time", &left.as_secs().max(1).to_string(), "--config", "-"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("could not run curl: {e}"))?;
        if let Some(mut stdin) = child.stdin.take() {
            // A write error means curl stopped early; its exit status reports that.
            let _ = stdin.write_all(config.as_bytes());
        }
        let out = child.wait_with_output().map_err(|e| e.to_string())?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            return Err(format!("could not reach the server: {}", err.trim()));
        }
        let text = String::from_utf8_lossy(&out.stdout);
        let (head, payload) = split_response(&text);
        let status: u16 = head.split_whitespace().nth(1).and_then(|s| s.parse().ok()).unwrap_or(0);
        if status >= 400 || status == 0 {
            return Err(format!("HTTP {status}: {}", payload.trim().chars().take(200).collect::<String>()));
        }
        let mut is_sse = false;
        for line in head.lines() {
            let Some((name, value)) = line.split_once(':') else { continue };
            let value = value.trim();
            if name.eq_ignore_ascii_case("mcp-session-id") {
                self.session = Some(value.to_string());
            } else if name.eq_ignore_ascii_case("content-type") {
                is_sse = value.contains("text/event-stream");
            }
        }
        Ok(if is_sse {
            payload.lines().filter_map(|l| l.strip_prefix("data:")).filter_map(|d| serde_json::from_str(d.trim()).ok()).collect()
        } else {
            match serde_json::from_str::<Value>(payload.trim()) {
                Ok(Value::Array(msgs)) => msgs,
                Ok(msg) => vec![msg],
                Err(_) => Vec::new(),
            }
        })
    }
}

impl Exchange for HttpServer<'_> {
    fn request(&mut self, id: u64, method: &str, params: Value) -> Result<Value, String> {
        let msgs = self.post(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))?;
        let msg = msgs
            .iter()
            .find(|m| m["id"] == id)
            .ok_or_else(|| format!("no reply to {method}"))?;
        reply_result(method, msg)
    }

    fn notify(&mut self, method: &str) -> Result<(), String> {
        self.post(&json!({"jsonrpc": "2.0", "method": method})).map(drop)
    }
}

/// A curl config (read with `--config -`) that POSTs `body` to `url` with `headers`. A value is quoted with
/// `\` and `"` escaped; CR and LF are dropped, so a header cannot add another header or option.
fn curl_config(url: &str, headers: &[String], body: &str) -> String {
    let quote = |s: &str| {
        let clean: String = s.chars().filter(|c| !matches!(c, '\r' | '\n')).collect();
        format!("\"{}\"", clean.replace('\\', "\\\\").replace('"', "\\\""))
    };
    let mut config = format!("url = {}\n", quote(url));
    for header in headers {
        config.push_str(&format!("header = {}\n", quote(header)));
    }
    // A JSON body never starts with `@`, which would make curl read a file instead.
    config.push_str(&format!("data-binary = {}\n", quote(body)));
    config
}

/// Splits `curl -i` output into the last response's head and body, skipping `100 Continue` and a
/// proxy's `CONNECT` answer, both of which are followed by the real response.
fn split_response(text: &str) -> (&str, &str) {
    let mut rest = text;
    while let Some((head, body)) = rest.split_once("\r\n\r\n") {
        let interim = head.starts_with("HTTP/") && head.split_whitespace().nth(1) == Some("100");
        if interim || (head.starts_with("HTTP/") && body.starts_with("HTTP/")) {
            rest = body;
            continue;
        }
        return (head, body);
    }
    (rest, "")
}

/// The `result` of a JSON-RPC reply, or its error message.
fn reply_result(method: &str, msg: &Value) -> Result<Value, String> {
    match msg.get("error") {
        Some(err) => Err(format!("{method}: {}", err["message"].as_str().map_or_else(|| err.to_string(), String::from))),
        None => Ok(msg["result"].clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A fresh folder under the temp dir for one test.
    fn scratch(name: &str) -> PathBuf {
        static N: AtomicUsize = AtomicUsize::new(0);
        let dir = env::temp_dir().join(format!("archonui-mcp-{name}-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn stdio(name: &str, command: &str, args: &[&str]) -> McpServer {
        McpServer {
            id: name.into(),
            name: name.into(),
            transport: McpTransport::Stdio {
                command: command.into(),
                args: args.iter().map(|a| a.to_string()).collect(),
                env: BTreeMap::new(),
            },
            enabled: true,
        }
    }

    /// Answers `initialize` and `tools/list` (two tools, first page only) over stdio.
    const FAKE_SERVER: &str = r#"
import json, sys
for line in sys.stdin:
    msg = json.loads(line)
    if "id" not in msg:
        continue
    if msg["method"] == "initialize":
        result = {"protocolVersion": "2025-06-18", "capabilities": {"tools": {}}, "serverInfo": {"name": "fake", "version": "1"}}
    else:
        result = {"tools": [{"name": "a"}, {"name": "b"}]}
    print("log line that is not JSON", flush=True)
    sys.stdout.buffer.write(b"\xff\xfe binary log\n")
    # A request of the server's own that reuses the client's id is not the reply.
    print(json.dumps({"jsonrpc": "2.0", "id": msg["id"], "method": "roots/list"}), flush=True)
    print(json.dumps({"jsonrpc": "2.0", "id": msg["id"], "result": result}), flush=True)
"#;

    #[test]
    fn imports_every_harness_format_and_dedupes_by_name() {
        let home = scratch("import");
        let cwd = home.join("project");
        let codex_home = home.join("codex");
        write(
            &home.join(".claude.json"),
            r#"{"mcpServers": {"serena": {"command": "serena", "args": ["start"]}},
                "projects": {"CWD": {"mcpServers": {"proj": {"type": "http", "url": "https://p.test/mcp", "headers": {"X": "1"}}}}}}"#
                .replace("CWD", &cwd.display().to_string())
                .as_str(),
        );
        write(&cwd.join(".mcp.json"), r#"{"mcpServers": {"old": {"type": "sse", "url": "https://sse.test"}}}"#);
        write(
            &codex_home.join("config.toml"),
            r#"
model = "gpt"

[mcp_servers.serena]
command = "other"

[mcp_servers.pg]
command = "psql"
args = ["-q"]
env = { PGHOST = "localhost" }

[mcp_servers.pg.tls]
verify = true

[mcp_servers.docs]
url = "https://docs.test/mcp"
http_headers = { Authorization = "Bearer t" }
enabled = false
"#,
        );
        write(
            &home.join(".config/opencode/opencode.jsonc"),
            r#"{
                // a comment with "quotes" and a , comma
                "mcp": {
                    "local-one": {"type": "local", "command": ["npx", "-y", "pkg"], "environment": {"K": "v"},},
                    "remote-one": {"type": "remote", "url": "https://r.test/mcp", "enabled": false},
                },
            }"#,
        );
        write(&home.join(".pi/agent/mcp.json"), r#"{"mcpServers": {"ctx": {"url": "https://ctx.test/mcp"}}}"#);
        write(
            &home.join(".gemini/settings.json"),
            r#"{"mcpServers": {"gem": {"httpUrl": "https://g.test/mcp"}, "gem-sse": {"url": "https://s.test"}}}"#,
        );

        let servers = import_in(&home, &codex_home, &cwd);
        let names: Vec<&str> = servers.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["serena", "proj", "docs", "pg", "local-one", "remote-one", "ctx", "gem"]);
        let by_name = |n: &str| servers.iter().find(|s| s.name == n).unwrap().clone();
        assert_eq!(by_name("serena").transport, McpTransport::Stdio { command: "serena".into(), args: vec!["start".into()], env: BTreeMap::new() });
        assert!(matches!(by_name("proj").transport, McpTransport::Http { ref url, .. } if url == "https://p.test/mcp"));
        assert_eq!(
            by_name("pg").transport,
            McpTransport::Stdio {
                command: "psql".into(),
                args: vec!["-q".into()],
                env: BTreeMap::from([("PGHOST".into(), "localhost".into())]),
            }
        );
        assert!(!by_name("docs").enabled);
        assert!(matches!(by_name("docs").transport, McpTransport::Http { ref headers, .. } if headers["Authorization"] == "Bearer t"));
        assert_eq!(
            by_name("local-one").transport,
            McpTransport::Stdio {
                command: "npx".into(),
                args: vec!["-y".into(), "pkg".into()],
                env: BTreeMap::from([("K".into(), "v".into())]),
            }
        );
        assert!(!by_name("remote-one").enabled);
        assert!(matches!(by_name("gem").transport, McpTransport::Http { ref url, .. } if url == "https://g.test/mcp"));
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn missing_or_broken_config_files_are_skipped() {
        let home = scratch("broken");
        write(&home.join(".pi/agent/mcp.json"), "{ not json");
        assert!(import_in(&home, &home.join("codex"), &home).is_empty());
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn strip_jsonc_keeps_strings_intact() {
        let text = "{\"url\": \"https://x.test//y\", /* block */ \"a\": [1, 2,], // tail\n\"b\": \"q\\\"/*\"}";
        let v: Value = serde_json::from_str(&strip_jsonc(text)).unwrap();
        assert_eq!(v["url"], "https://x.test//y");
        assert_eq!(v["a"], json!([1, 2]));
        assert_eq!(v["b"], "q\"/*");
    }

    #[test]
    fn strip_jsonc_drops_trailing_commas_before_comments() {
        assert_eq!(strip_jsonc("{\"a\": 1, // note\n}"), "{\"a\": 1 \n}");
        assert_eq!(strip_jsonc("[1, /* two */ ]"), "[1  ]");
        let v: Value = serde_json::from_str(&strip_jsonc("{\"a\": [1, // c\n], /* d */ \"b\": [2, /* e */ 3, /* f */ ],}")).unwrap();
        assert_eq!(v, json!({"a": [1], "b": [2, 3]}));
    }

    #[test]
    fn json_entries_match_each_shape() {
        let s = stdio("fs", "node", &["srv.js"]);
        assert_eq!(json_entry(&s, true), json!({"command": "node", "args": ["srv.js"], "env": {}, "type": "stdio"}));
        assert_eq!(json_entry(&s, false), json!({"command": "node", "args": ["srv.js"], "env": {}}));
        let h = McpServer {
            id: "h".into(),
            name: "h".into(),
            transport: McpTransport::Http { url: "https://h.test".into(), headers: BTreeMap::new() },
            enabled: true,
        };
        assert_eq!(json_entry(&h, true), json!({"url": "https://h.test", "headers": {}, "type": "http"}));
    }

    #[test]
    fn disabled_servers_are_not_contacted() {
        let mut s = stdio("off", "/definitely/not/a/command", &[]);
        s.enabled = false;
        assert_eq!(check(&s), McpStatus::Off);
    }

    #[test]
    fn stdio_check_counts_tools_over_newline_json() {
        let dir = scratch("stdio");
        let script = dir.join("fake_server.py");
        write(&script, FAKE_SERVER);
        let s = stdio("fake", "python3", &["-I", &script.display().to_string()]);
        assert_eq!(check_within(&s, Duration::from_secs(20)), McpStatus::Connected { tools: 2 });
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn stdio_check_reports_a_missing_command_and_a_silent_server() {
        let missing = stdio("gone", "/no/such/archonui-test-binary", &[]);
        assert!(matches!(check_within(&missing, Duration::from_secs(5)), McpStatus::Failed(e) if e.starts_with("could not start")));
        let dir = scratch("silent");
        let script = dir.join("silent.py");
        write(&script, "import time\ntime.sleep(60)\n");
        let silent = stdio("silent", "python3", &["-I", &script.display().to_string()]);
        let started = Instant::now();
        let status = check_within(&silent, Duration::from_secs(1));
        assert!(matches!(status, McpStatus::Failed(ref e) if e.contains("in time")), "{status:?}");
        assert!(started.elapsed() < Duration::from_secs(10), "the check gave up at its deadline");
        let _ = fs::remove_dir_all(&dir);
    }

    /// One-shot local HTTP server: answers each POST by method. Returns the URL.
    fn spawn_http_server() -> String {
        use std::io::Read;
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/mcp", listener.local_addr().unwrap());
        thread::spawn(move || {
            'accept: for stream in listener.incoming().map_while(Result::ok) {
                let mut stream = stream;
                let mut raw = Vec::new();
                let mut buf = [0u8; 4096];
                // Read headers, then the body by Content-Length.
                let (head_end, length, authorized) = loop {
                    let n = stream.read(&mut buf).unwrap_or(0);
                    if n == 0 {
                        continue 'accept;
                    }
                    raw.extend_from_slice(&buf[..n]);
                    if let Some(i) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&raw[..i]).to_lowercase();
                        let len = head.lines().find_map(|l| l.strip_prefix("content-length:")).map_or(0, |v| v.trim().parse::<usize>().unwrap_or(0));
                        break (i + 4, len, head.lines().any(|l| l == "x-test: \"quoted\" \\ value"));
                    }
                };
                if !authorized {
                    let _ = stream.write_all(b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\n\r\n");
                    continue;
                }
                while raw.len() < head_end + length {
                    let n = stream.read(&mut buf).unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    raw.extend_from_slice(&buf[..n]);
                }
                let body: Value = serde_json::from_slice(&raw[head_end..]).unwrap_or(Value::Null);
                let id = body["id"].clone();
                let response = match body["method"].as_str().unwrap_or("") {
                    "initialize" => {
                        let json = json!({"jsonrpc": "2.0", "id": id, "result": {"protocolVersion": PROTOCOL}}).to_string();
                        format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nMcp-Session-Id: s-1\r\nContent-Length: {}\r\n\r\n{json}", json.len())
                    }
                    "notifications/initialized" => "HTTP/1.1 202 Accepted\r\nContent-Length: 0\r\n\r\n".to_string(),
                    _ => {
                        // Answer tools/list as an SSE stream, the other form servers use.
                        let event = format!("event: message\ndata: {}\n\n", json!({"jsonrpc": "2.0", "id": id, "result": {"tools": [{"name": "x"}, {"name": "y"}, {"name": "z"}]}}));
                        format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\n\r\n{event}", event.len())
                    }
                };
                let _ = stream.write_all(response.as_bytes());
            }
        });
        url
    }

    #[test]
    fn http_check_follows_session_and_reads_sse_replies() {
        let server = McpServer {
            id: "web".into(),
            name: "web".into(),
            // The header arrives intact through curl's config quoting; the server answers 401 otherwise.
            transport: McpTransport::Http {
                url: spawn_http_server(),
                headers: BTreeMap::from([("X-Test".into(), "\"quoted\" \\ value".into())]),
            },
            enabled: true,
        };
        assert_eq!(check_within(&server, Duration::from_secs(20)), McpStatus::Connected { tools: 3 });
    }

    #[test]
    fn split_response_skips_interim_responses() {
        let text = "HTTP/1.1 100 Continue\r\n\r\nHTTP/1.1 200 OK\r\nA: b\r\n\r\n{}";
        assert_eq!(split_response(text), ("HTTP/1.1 200 OK\r\nA: b", "{}"));
        let proxied = "HTTP/1.1 200 Connection established\r\n\r\nHTTP/2 200\r\ncontent-type: application/json\r\n\r\n{}";
        assert_eq!(split_response(proxied), ("HTTP/2 200\r\ncontent-type: application/json", "{}"));
    }

    #[test]
    fn curl_config_quotes_values_and_drops_line_breaks() {
        let config = curl_config(
            "https://h.test/mcp?key=a\"b",
            &["Authorization: Bearer x\r\nX-Injected: 1".into()],
            r#"{"a":"q\"\\n"}"#,
        );
        assert_eq!(
            config,
            concat!(
                "url = \"https://h.test/mcp?key=a\\\"b\"\n",
                "header = \"Authorization: Bearer xX-Injected: 1\"\n",
                "data-binary = \"{\\\"a\\\":\\\"q\\\\\\\"\\\\\\\\n\\\"}\"\n",
            )
        );
    }
}
