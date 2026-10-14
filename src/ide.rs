//! The IDE: a code-server (VS Code) process shown through the browser view.

use crate::{preview::stop_process_group, store};
use std::{
    env, fs,
    ffi::OsStr,
    io::{self, BufRead, BufReader, Read, Write},
    net::TcpStream,
    os::unix::{fs::PermissionsExt, process::CommandExt},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{mpsc, Arc, Mutex, MutexGuard, PoisonError},
    thread,
    time::{Duration, Instant},
};

/// The cookie code-server reads its login from.
pub const COOKIE: &str = "code-server-session";

/// How long opening a file in the running window may take, counting the waits for the window to join.
const OPEN_TIMEOUT: Duration = Duration::from_secs(10);

/// The error when no code-server can be found.
pub const NOT_INSTALLED: &str =
    "code-server isn't installed. Install it (https://coder.com/docs/code-server/install) or set its path in Settings.";

/// A running code-server on a local port, with the password it asks for.
/// Dropping it stops code-server.
pub struct IdeServer {
    /// Shared with the thread that reports when code-server exits.
    child: Arc<Mutex<Child>>,
    program: PathBuf,
    port: u16,
    password: String,
    exited: smol::channel::Receiver<Option<i32>>,
}

impl IdeServer {
    /// Blocking: starts `program` (code-server) on a free local port and waits until it listens.
    pub fn start(program: &Path) -> Result<Self, String> {
        let data = data_dir();
        // code-server stores its settings and session socket here, so keep it owner-only.
        store::create_private_dir(&data).map_err(|e| format!("Could not create {}: {e}", data.display()))?;
        seed_user_settings(&data);
        // An empty config keeps the user's own code-server config from changing the login or address.
        let config = data.with_extension("yaml");
        fs::write(&config, "").map_err(|e| format!("Could not write {}: {e}", config.display()))?;
        let home = env::var_os("HOME").ok_or("HOME isn't set.")?;
        let extensions = Path::new(&home).join(".local/share/code-server/extensions");
        let password = random_password()?;
        let mut child = unsafe {
            Command::new(program)
                .args(["--auth", "password", "--bind-addr", "127.0.0.1:0", "--config"])
                .arg(&config)
                .arg("--user-data-dir")
                .arg(&data)
                .args([
                    "--disable-telemetry",
                    "--disable-update-check",
                    "--disable-workspace-trust",
                    "--disable-getting-started-override",
                    "--extensions-dir",
                ])
                .arg(&extensions)
                .env("PASSWORD", &password)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .process_group(0)
                // code-server must not outlive ArchonUI, even if ArchonUI is killed.
                .pre_exec(|| {
                    libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL);
                    Ok(())
                })
                .spawn()
        }
        .map_err(|e| match e.kind() {
            io::ErrorKind::NotFound => NOT_INSTALLED.to_string(),
            _ => format!("Could not start code-server: {e}"),
        })?;
        let (lines_tx, lines) = mpsc::channel();
        drain(child.stdout.take().expect("piped stdout"), lines_tx.clone());
        drain(child.stderr.take().expect("piped stderr"), lines_tx);
        let Some(port) = await_port(&lines) else {
            stop_process_group(&mut child);
            return Err("code-server didn't start listening.".into());
        };
        let child = Arc::new(Mutex::new(child));
        let (exit_tx, exited) = smol::channel::bounded(1);
        watch_exit(Arc::clone(&child), exit_tx);
        Ok(Self { child, program: program.to_path_buf(), port, password, exited })
    }

    /// The local port code-server listens on.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// The code-server program this server runs.
    pub fn program(&self) -> &Path {
        &self.program
    }

    /// Receives code-server's exit code (`None` if a signal ended it) when it exits on its own.
    pub fn exited(&self) -> smol::channel::Receiver<Option<i32>> {
        self.exited.clone()
    }

    /// The address of code-server's pages.
    pub fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}/", self.port)
    }

    /// The address that opens `folder` in VS Code.
    pub fn url_for(&self, folder: &Path) -> String {
        format!("{}?folder={}", self.base_url(), percent_encode(&folder.to_string_lossy()))
    }

    /// Logs in with the password and returns the session cookie's value.
    pub fn login_cookie(&self) -> Result<String, String> {
        let unreachable = |e: io::Error| format!("Could not reach code-server: {e}");
        let body = format!("password={}", percent_encode(&self.password));
        let mut stream = TcpStream::connect(("127.0.0.1", self.port)).map_err(unreachable)?;
        stream.set_read_timeout(Some(Duration::from_secs(10))).map_err(unreachable)?;
        write!(
            stream,
            "POST /login HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/x-www-form-urlencoded\r\n\
             Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            self.port,
            body.len()
        )
        .map_err(unreachable)?;
        let mut response = Vec::new();
        stream.read_to_end(&mut response).map_err(unreachable)?;
        let response = String::from_utf8_lossy(&response);
        cookie_value(&response, COOKIE).ok_or_else(|| "code-server rejected the login.".to_string())
    }
}

impl Drop for IdeServer {
    fn drop(&mut self) {
        stop_process_group(&mut lock_child(&self.child));
    }
}

/// Locks `child`, which stays usable even if a thread panicked while holding the lock.
fn lock_child(child: &Mutex<Child>) -> MutexGuard<'_, Child> {
    child.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The guard for a string a background thread writes, ignoring poisoning.
fn lock_string(text: &Mutex<String>) -> MutexGuard<'_, String> {
    text.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Checks `child` every two seconds and sends its exit code on `exited` once it has exited.
fn watch_exit(child: Arc<Mutex<Child>>, exited: smol::channel::Sender<Option<i32>>) {
    thread::spawn(move || loop {
        thread::sleep(Duration::from_secs(2));
        let status = lock_child(&child).try_wait();
        match status {
            Ok(None) => {}
            Ok(Some(status)) => {
                let _ = exited.send_blocking(status.code());
                return;
            }
            Err(_) => return,
        }
    });
}

/// The code-server to run: `configured` if it's an executable file, else the first one on
/// `PATH` or in the usual install locations.
pub fn find_code_server(configured: Option<&Path>) -> Option<PathBuf> {
    let home = env::var_os("HOME").map(PathBuf::from);
    find_in(configured, home.as_deref(), &env::var_os("PATH").unwrap_or_default())
}

/// `find_code_server` with `home` and `path` passed in.
fn find_in(configured: Option<&Path>, home: Option<&Path>, path: &OsStr) -> Option<PathBuf> {
    if let Some(program) = configured.filter(|p| is_executable(p)) {
        return Some(program.to_path_buf());
    }
    let mut candidates: Vec<PathBuf> = env::split_paths(path)
        .filter(|dir| !dir.as_os_str().is_empty())
        .map(|dir| dir.join("code-server"))
        .collect();
    if let Some(home) = home {
        candidates.push(home.join(".local/bin/code-server"));
        candidates.extend(newest_install(&home.join(".local/opt")));
        candidates.extend(newest_install(&home.join(".local/lib")));
        candidates.push(home.join(".npm-global/bin/code-server"));
    }
    candidates.extend(
        ["/usr/local/bin/code-server", "/usr/bin/code-server", "/opt/code-server/bin/code-server"].map(PathBuf::from),
    );
    candidates.into_iter().find(|p| is_executable(p))
}

/// Where code-server keeps its data. Its session socket is in here, which `--reuse-window` finds the window by.
fn data_dir() -> PathBuf {
    store::dir().with_file_name("code-server")
}

/// ArchonUI's code-server user settings file, inside the `--user-data-dir` `start` passes.
fn user_settings(data: &Path) -> PathBuf {
    data.join("User").join("settings.json")
}

/// Seeds ArchonUI's own code-server user settings with the keys VS Code needs to open dark and uncluttered in the
/// panel. Only keys that are absent are written, so the user's own choices are kept. A settings file that isn't
/// the JSON object code-server writes is left untouched.
fn seed_user_settings(data: &Path) {
    let path = user_settings(data);
    let mut settings = match fs::read_to_string(&path) {
        Ok(text) => match serde_json::from_str::<serde_json::Value>(&text) {
            Ok(serde_json::Value::Object(map)) => map,
            _ => return,
        },
        Err(_) => serde_json::Map::new(),
    };
    let defaults: [(&str, serde_json::Value); 4] = [
        ("workbench.colorTheme", serde_json::json!("Default Dark Modern")),
        ("workbench.secondarySideBar.defaultVisibility", serde_json::json!("hidden")),
        ("chat.commandCenter.enabled", serde_json::json!(false)),
        ("workbench.startupEditor", serde_json::json!("none")),
    ];
    let mut added = false;
    for (key, value) in defaults {
        if !settings.contains_key(key) {
            settings.insert(key.into(), value);
            added = true;
        }
    }
    if !added || fs::create_dir_all(path.parent().expect("settings.json has a parent")).is_err() {
        return;
    }
    if let Ok(text) = serde_json::to_string_pretty(&serde_json::Value::Object(settings)) {
        let _ = fs::write(&path, text);
    }
}

/// Opens `file` in the window of the code-server that `program` runs. The window joins the server once its page
/// loads, so this retries until it does. It gives up after 10 seconds.
pub fn open_in_window(program: &Path, file: &Path) -> Result<(), String> {
    let data = data_dir();
    let config = data.with_extension("yaml");
    let deadline = Instant::now() + OPEN_TIMEOUT;
    let mut note = String::new();
    loop {
        if reuse_window(program, &data, &config, file, deadline, &mut note)? {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(open_timeout(file, &note));
        }
        thread::sleep(Duration::from_millis(500));
    }
}

/// Why an open timed out: the file, and code-server's own last words when its last call printed any.
fn open_timeout(file: &Path, stderr: &str) -> String {
    let note = stderr.trim();
    if note.is_empty() {
        format!("VS Code didn't open {} in time.", file.display())
    } else {
        format!("VS Code didn't open {} in time: {note}", file.display())
    }
}

/// One `code-server --reuse-window <file>` call, stopped at `deadline`. `Ok(true)` when it opened the file.
/// What the call writes to stderr is left in `note`, for the timeout error the caller reports.
fn reuse_window(
    program: &Path,
    data: &Path,
    config: &Path,
    file: &Path,
    deadline: Instant,
    note: &mut String,
) -> Result<bool, String> {
    let mut child = Command::new(program)
        .arg("--config")
        .arg(config)
        .arg("--user-data-dir")
        .arg(data)
        .arg("--reuse-window")
        .arg(file)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Could not run code-server: {e}"))?;
    // Draining on its own thread keeps a chatty code-server from blocking on a full pipe.
    let errors = Arc::new(Mutex::new(String::new()));
    if let Some(mut stderr) = child.stderr.take() {
        let sink = Arc::clone(&errors);
        thread::spawn(move || {
            let mut text = String::new();
            let _ = stderr.read_to_string(&mut text);
            *lock_string(&sink) = text;
        });
    }
    loop {
        if let Some(status) = child.try_wait().map_err(|e| format!("Could not check code-server: {e}"))? {
            *note = lock_string(&errors).trim().to_string();
            return Ok(status.success());
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            // Give the drain thread a moment to see the closed pipe.
            for _ in 0..20 {
                if !lock_string(&errors).is_empty() {
                    break;
                }
                thread::sleep(Duration::from_millis(5));
            }
            *note = lock_string(&errors).trim().to_string();
            return Ok(false);
        }
        thread::sleep(Duration::from_millis(50));
    }
}

/// The newest runnable `code-server-*/bin/code-server` in `dir`.
fn newest_install(dir: &Path) -> Option<PathBuf> {
    let mut installs: Vec<PathBuf> = fs::read_dir(dir)
        .ok()?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.file_name().is_some_and(|name| name.to_string_lossy().starts_with("code-server-")))
        .collect();
    installs.sort_by_key(|path| version_numbers(path));
    installs.iter().rev().map(|path| path.join("bin/code-server")).find(|p| is_executable(p))
}

/// The numbers in a path's file name, so `code-server-10.0.0` sorts after `code-server-9.9.9`.
fn version_numbers(path: &Path) -> Vec<u64> {
    let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    name.split(|c: char| !c.is_ascii_digit()).filter_map(|n| n.parse().ok()).collect()
}

/// Whether `path` is a file that can be run.
fn is_executable(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

/// Sends each line of `pipe` to `lines`. Keeps reading after the receiver is
/// gone so the process never blocks on a full pipe.
fn drain(pipe: impl Read + Send + 'static, lines: mpsc::Sender<String>) {
    thread::spawn(move || {
        for line in BufReader::new(pipe).lines().map_while(Result::ok) {
            let _ = lines.send(line);
        }
    });
}

/// The port code-server reports, waiting up to 30 seconds for it.
fn await_port(lines: &mpsc::Receiver<String>) -> Option<u16> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let line = lines.recv_timeout(deadline.saturating_duration_since(Instant::now())).ok()?;
        if let Some(port) = listening_port(&line) {
            return Some(port);
        }
    }
}

/// The port from code-server's "HTTP server listening on http://127.0.0.1:PORT/" line.
fn listening_port(line: &str) -> Option<u16> {
    let (_, rest) = line.split_once("HTTP server listening on http://127.0.0.1:")?;
    rest.split('/').next()?.parse().ok()
}

/// The value of the `name` cookie in a response's Set-Cookie headers.
fn cookie_value(response: &str, name: &str) -> Option<String> {
    response.lines().find_map(|line| {
        let (header, rest) = line.split_once(':')?;
        if !header.eq_ignore_ascii_case("set-cookie") {
            return None;
        }
        let (key, value) = rest.trim().split(';').next()?.split_once('=')?;
        (key == name).then(|| value.to_string())
    })
}

/// Percent-encodes everything but unreserved characters and `/`.
fn percent_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// 32 hex characters from the kernel's random source.
fn random_password() -> Result<String, String> {
    let mut bytes = [0u8; 16];
    fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut bytes))
        .map_err(|e| format!("Could not read /dev/urandom: {e}"))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_listening_port() {
        assert_eq!(listening_port("HTTP server listening on http://127.0.0.1:41365/"), Some(41365));
        assert_eq!(listening_port("[12:00:00] HTTP server listening on http://127.0.0.1:8080/"), Some(8080));
        assert_eq!(listening_port("Extension host started"), None);
    }

    #[test]
    fn reads_the_session_cookie() {
        let response = "HTTP/1.1 302 Found\r\nLocation: /\r\nSet-Cookie: code-server-session=abc123; Path=/; HttpOnly\r\n\r\n";
        assert_eq!(cookie_value(response, COOKIE).as_deref(), Some("abc123"));
        assert_eq!(cookie_value("HTTP/1.1 200 OK\r\n\r\n", COOKIE), None);
    }

    #[test]
    fn opens_folders_with_encoded_paths() {
        let server = IdeServer {
            child: Arc::new(Mutex::new(Command::new("true").spawn().unwrap())),
            program: PathBuf::new(),
            port: 8080,
            password: String::new(),
            exited: smol::channel::bounded(1).1,
        };
        assert_eq!(server.url_for(Path::new("/tmp/my folder")), "http://127.0.0.1:8080/?folder=/tmp/my%20folder");
    }

    /// Removes the test folder when dropped, even if the test panics.
    struct Cleanup(PathBuf);

    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// Writes an executable file at `path`, creating its folders.
    fn fake_program(path: PathBuf) -> PathBuf {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[test]
    fn finds_code_server() {
        let dir = env::temp_dir().join(format!("archonui-ide-test-{}", std::process::id()));
        let _cleanup = Cleanup(dir.clone());
        let configured = fake_program(dir.join("configured/code-server"));
        let on_path = fake_program(dir.join("path/code-server"));
        let home = dir.join("home");
        fake_program(home.join(".local/opt/code-server-9.9.9/bin/code-server"));
        let newest = fake_program(home.join(".local/opt/code-server-10.0.0/bin/code-server"));
        let path = env::join_paths([dir.join("empty"), dir.join("path")]).unwrap();
        let no_path = OsStr::new("");

        // A configured program wins over everything else.
        assert_eq!(find_in(Some(&configured), Some(&home), &path), Some(configured.clone()));
        // A configured path that isn't there falls through to the search.
        assert_eq!(find_in(Some(&dir.join("missing")), Some(&home), &path), Some(on_path));
        // Without PATH, the newest version in ~/.local/opt is used (10.0.0, not 9.9.9).
        assert_eq!(find_in(None, Some(&home), no_path), Some(newest));
    }

    #[test]
    fn seeds_only_absent_code_server_settings() {
        let dir = env::temp_dir().join(format!("archonui-ide-settings-{}", std::process::id()));
        let _cleanup = Cleanup(dir.clone());

        // Nothing there yet: every default is written.
        seed_user_settings(&dir);
        let seeded: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(user_settings(&dir)).unwrap()).unwrap();
        assert_eq!(seeded["workbench.colorTheme"], "Default Dark Modern");
        assert_eq!(seeded["workbench.secondarySideBar.defaultVisibility"], "hidden");
        assert_eq!(seeded["chat.commandCenter.enabled"], false);
        assert_eq!(seeded["workbench.startupEditor"], "none");

        // The user's own choice and unrelated keys survive; only absent keys are added.
        fs::write(user_settings(&dir), r#"{"workbench.colorTheme":"Monokai","editor.fontSize":14}"#).unwrap();
        seed_user_settings(&dir);
        let kept: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(user_settings(&dir)).unwrap()).unwrap();
        assert_eq!(kept["workbench.colorTheme"], "Monokai");
        assert_eq!(kept["editor.fontSize"], 14);
        assert_eq!(kept["workbench.startupEditor"], "none");
        assert_eq!(kept["workbench.secondarySideBar.defaultVisibility"], "hidden");

        // A file that isn't the JSON object code-server writes is left alone.
        fs::write(user_settings(&dir), "// not json").unwrap();
        seed_user_settings(&dir);
        assert_eq!(fs::read_to_string(user_settings(&dir)).unwrap(), "// not json");
    }

    #[test]
    fn an_open_timeout_reports_code_servers_stderr() {
        let file = Path::new("/tmp/x.md");
        assert_eq!(open_timeout(file, "  \n"), "VS Code didn't open /tmp/x.md in time.");
        assert_eq!(
            open_timeout(file, "error: window closed\n"),
            "VS Code didn't open /tmp/x.md in time: error: window closed"
        );
    }

    #[test]
    fn reports_the_exit_code() {
        let child = Arc::new(Mutex::new(Command::new("sh").args(["-c", "exit 3"]).spawn().unwrap()));
        let (exited_tx, exited) = smol::channel::bounded(1);
        watch_exit(child, exited_tx);
        assert_eq!(smol::block_on(exited.recv()).unwrap(), Some(3));
    }

    /// Starts a real code-server, logs in and loads its page. Needs `code-server` on PATH:
    /// `cargo test ide_server_starts -- --ignored`.
    #[test]
    #[ignore]
    fn ide_server_starts() {
        let program = find_code_server(None).expect("code-server isn't installed");
        let server = IdeServer::start(&program).unwrap();
        let cookie = server.login_cookie().unwrap();
        assert!(!cookie.is_empty());
        let mut stream = TcpStream::connect(("127.0.0.1", server.port)).unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
        write!(stream, "GET / HTTP/1.1\r\nHost: 127.0.0.1\r\nCookie: {COOKIE}={cookie}\r\nConnection: close\r\n\r\n").unwrap();
        let mut response = Vec::new();
        stream.read_to_end(&mut response).unwrap();
        let status = String::from_utf8_lossy(&response).lines().next().unwrap_or_default().to_string();
        assert!(status.starts_with("HTTP/1.1 200"), "{status}");
        let group = -(lock_child(&server.child).id() as i32);
        drop(server);
        let gone = (0..100).any(|_| {
            thread::sleep(Duration::from_millis(20));
            let signalled = unsafe { libc::kill(group, 0) };
            signalled != 0
        });
        assert!(gone, "code-server's process group is still running");
    }
}
