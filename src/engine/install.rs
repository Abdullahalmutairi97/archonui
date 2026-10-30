//! Installing, updating and signing in to agent CLIs from the Harnesses page. Agents go in for this
//! user only: the official install scripts put Claude Code and OpenCode in `~/.local/bin`, and npm
//! packages get `--prefix ~/.local`. Only what needs root (Node itself, curl, or a CLI that was
//! installed system-wide) runs through `pkexec`, so the desktop asks for the password and ArchonUI
//! never sees it.

use std::{
    env,
    ffi::CString,
    io::{BufRead, BufReader},
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

/// How an agent CLI is installed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Source {
    /// An official script, piped from curl to bash.
    Script(&'static str),
    /// An npm package.
    Npm(&'static str),
}

/// Pi's package before it moved. It gets no new releases, so an update replaces it.
const OLD_PI: &str = "@mariozechner/pi-coding-agent";

fn source(id: &str) -> Option<Source> {
    Some(match id {
        "claude" => Source::Script("https://claude.ai/install.sh"),
        "opencode" => Source::Script("https://opencode.ai/install"),
        "codex" => Source::Npm("@openai/codex"),
        "gemini" => Source::Npm("@google/gemini-cli"),
        "pi" => Source::Npm("@earendil-works/pi-coding-agent"),
        _ => return None,
    })
}

/// The command a user runs in a terminal to sign in, for the Sign in button.
pub fn sign_in_command(id: &str) -> Option<&'static str> {
    Some(match id {
        "claude" => "claude",
        "codex" => "codex login",
        "gemini" => "gemini",
        "opencode" => "opencode auth login",
        "pi" => "pi",
        _ => return None,
    })
}

/// One command of a job.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Step {
    pub program: String,
    pub args: Vec<String>,
    /// Runs as root, through `pkexec`.
    pub root: bool,
}

impl Step {
    fn user(program: impl Into<String>, args: &[&str]) -> Self {
        Step { program: program.into(), args: args.iter().map(|a| a.to_string()).collect(), root: false }
    }

    fn root(mut self) -> Self {
        self.root = true;
        self
    }
}

/// What the steps depend on, read from the system by `Host::here`, or made up in tests.
#[derive(Clone, Debug, Default)]
pub struct Host {
    pub home: PathBuf,
    pub has_curl: bool,
    pub has_npm: bool,
    /// The distro's package manager, e.g. "apt-get", if one is known.
    pub packages: Option<&'static str>,
}

impl Host {
    pub fn here() -> Self {
        Host {
            home: env::var_os("HOME").map(PathBuf::from).unwrap_or_default(),
            has_curl: find("curl").is_some(),
            has_npm: find("npm").is_some(),
            packages: package_manager(),
        }
    }
}

/// The steps that install agent `id` for this user.
pub fn install_steps(id: &str, host: &Host) -> Result<Vec<Step>, String> {
    let source = source(id).ok_or_else(|| format!("ArchonUI can't install {id}"))?;
    let mut steps = Vec::new();
    match source {
        Source::Script(url) => {
            if !host.has_curl {
                steps.push(system_install(host, &["curl"])?);
            }
            steps.push(Step::user("bash", &["-c", &format!("curl -fsSL {url} | bash")]));
        }
        Source::Npm(package) => {
            if !host.has_npm {
                steps.push(system_install(host, &["nodejs", "npm"])?);
            }
            let prefix = host.home.join(".local");
            steps.push(Step::user("npm", &["install", "-g", "--prefix", &prefix.to_string_lossy(), package]));
        }
    }
    Ok(steps)
}

/// The steps that update agent `id`, installed at `path`. A script install updates itself; an npm
/// install is updated in the folder it is in, as root when this user can't write there.
pub fn update_steps(id: &str, path: &Path) -> Result<Vec<Step>, String> {
    let program = path.to_string_lossy();
    match (id, source(id)) {
        ("claude", _) => Ok(vec![Step::user(program, &["update"])]),
        ("opencode", _) => Ok(vec![Step::user(program, &["upgrade"])]),
        (_, Some(Source::Npm(package))) => {
            let name = crate::engine::agent_name(id);
            let (prefix, installed) = npm_prefix(path, package)
                .ok_or_else(|| format!("{name} wasn't installed with npm here, so update it the way you installed it"))?;
            // The npm that came with this Node, else the one on PATH.
            let npm = [prefix.join("bin/npm")].into_iter().find(|p| p.is_file()).map_or("npm".to_string(), |p| p.to_string_lossy().into_owned());
            let root = !writable(&prefix);
            let prefix = prefix.to_string_lossy();
            let mut steps = Vec::new();
            if installed != package {
                steps.push(Step { root, ..Step::user(&npm, &["uninstall", "-g", "--prefix", &prefix, installed]) });
            }
            steps.push(Step { root, ..Step::user(&npm, &["install", "-g", "--prefix", &prefix, &format!("{package}@latest")]) });
            Ok(steps)
        }
        _ => Err(format!("ArchonUI can't update {id}")),
    }
}

/// The npm prefix the CLI at `path` was installed under, and the package found there: `package`, or
/// Pi's old name. `None` when it isn't an npm install.
fn npm_prefix<'a>(path: &Path, package: &'a str) -> Option<(PathBuf, &'a str)> {
    // npm puts `<prefix>/bin/<cli>` next to `<prefix>/lib/node_modules/<package>`.
    let prefix = path.parent()?.parent()?;
    let modules = prefix.join("lib/node_modules");
    [package, OLD_PI]
        .into_iter()
        .take(if package.ends_with("/pi-coding-agent") { 2 } else { 1 })
        .find(|p| modules.join(p).is_dir())
        .map(|p| (prefix.to_path_buf(), p))
}

/// The step that installs `names` with the distro's package manager, as root.
fn system_install(host: &Host, names: &[&str]) -> Result<Step, String> {
    let Some(manager) = host.packages else {
        return Err(format!("Install {} with your package manager first", names.join(" and ")));
    };
    let mut args: Vec<&str> = match manager {
        "pacman" => vec!["-S", "--needed", "--noconfirm"],
        "zypper" => vec!["--non-interactive", "install"],
        _ => vec!["install", "-y"],
    };
    args.extend(names);
    Ok(Step::user(manager, &args).root())
}

/// The package manager of this distro, from the ones ArchonUI knows.
fn package_manager() -> Option<&'static str> {
    ["apt-get", "pacman", "dnf", "zypper"].into_iter().find(|m| find(m).is_some())
}

/// `program` on `PATH`.
fn find(program: &str) -> Option<PathBuf> {
    env::split_paths(&env::var_os("PATH")?).map(|d| d.join(program)).find(|p| p.is_file())
}

/// Whether this user can write in `dir`.
fn writable(dir: &Path) -> bool {
    let Ok(c) = CString::new(dir.as_os_str().as_bytes()) else { return false };
    // SAFETY: `access` only reads the NUL-terminated path.
    unsafe { libc::access(c.as_ptr(), libc::W_OK) == 0 }
}

/// Runs `steps` in order, passing each output line to `say`. Stops at the first that fails and
/// returns why, with the last lines it printed.
pub fn run(steps: &[Step], say: &dyn Fn(String)) -> Result<(), String> {
    for step in steps {
        let mut cmd = if step.root {
            let pkexec = find("pkexec").ok_or_else(|| format!("This needs root, and pkexec is missing. Run in a terminal: sudo {}", shown(step)))?;
            // pkexec wants a full path and starts with a clean environment.
            let program = find(&step.program).unwrap_or_else(|| PathBuf::from(&step.program));
            let mut cmd = Command::new(pkexec);
            cmd.arg(program);
            cmd
        } else {
            Command::new(&step.program)
        };
        cmd.args(&step.args);
        say(format!("$ {}{}", if step.root { "sudo " } else { "" }, shown(step)));
        let mut child = cmd
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Couldn't start {}: {e}", step.program))?;
        // stderr is read on its own thread so neither pipe fills up and stalls the other.
        let stderr = child.stderr.take().map(|err| std::thread::spawn(move || BufReader::new(err).lines().map_while(Result::ok).collect::<Vec<_>>()));
        let mut tail: Vec<String> = Vec::new();
        if let Some(out) = child.stdout.take() {
            for line in BufReader::new(out).lines().map_while(Result::ok) {
                keep(&mut tail, line.clone());
                say(line);
            }
        }
        for line in stderr.and_then(|t| t.join().ok()).unwrap_or_default() {
            keep(&mut tail, line.clone());
            say(line);
        }
        let status = child.wait().map_err(|e| format!("{}: {e}", step.program))?;
        if status.success() {
            continue;
        }
        // pkexec exits 126 when the password prompt is dismissed and 127 when it is refused.
        if step.root && matches!(status.code(), Some(126 | 127)) {
            return Err("The password prompt was cancelled, so nothing was installed".into());
        }
        return Err(format!("`{}` failed:\n{}", shown(step), tail.join("\n")));
    }
    Ok(())
}

/// Keeps the last few lines of output for an error message.
fn keep(tail: &mut Vec<String>, line: String) {
    if tail.len() == 8 {
        tail.remove(0);
    }
    tail.push(line);
}

/// A step as a shell line, for messages.
fn shown(step: &Step) -> String {
    std::iter::once(step.program.as_str()).chain(step.args.iter().map(String::as_str)).collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host(curl: bool, npm: bool, packages: Option<&'static str>) -> Host {
        Host { home: PathBuf::from("/home/u"), has_curl: curl, has_npm: npm, packages }
    }

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("archonui-install-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn scripts_install_for_this_user_without_root() {
        let steps = install_steps("claude", &host(true, false, Some("apt-get"))).unwrap();
        assert_eq!(steps, vec![Step::user("bash", &["-c", "curl -fsSL https://claude.ai/install.sh | bash"])]);
        let steps = install_steps("opencode", &host(true, false, None)).unwrap();
        assert!(steps.iter().all(|s| !s.root));
    }

    #[test]
    fn npm_packages_go_under_the_home_local_prefix() {
        let steps = install_steps("codex", &host(true, true, None)).unwrap();
        assert_eq!(steps, vec![Step::user("npm", &["install", "-g", "--prefix", "/home/u/.local", "@openai/codex"])]);
        let pi = install_steps("pi", &host(true, true, None)).unwrap();
        assert_eq!(pi[0].args.last().unwrap(), "@earendil-works/pi-coding-agent");
    }

    #[test]
    fn missing_node_or_curl_is_installed_first_as_root() {
        let steps = install_steps("gemini", &host(true, false, Some("pacman"))).unwrap();
        assert_eq!(steps[0], Step::user("pacman", &["-S", "--needed", "--noconfirm", "nodejs", "npm"]).root());
        assert!(!steps[1].root);
        let steps = install_steps("claude", &host(false, true, Some("apt-get"))).unwrap();
        assert_eq!(steps[0], Step::user("apt-get", &["install", "-y", "curl"]).root());
        let zypper = install_steps("codex", &host(true, false, Some("zypper"))).unwrap();
        assert_eq!(zypper[0].args, ["--non-interactive", "install", "nodejs", "npm"]);
    }

    #[test]
    fn without_a_known_package_manager_it_says_what_to_install() {
        let err = install_steps("codex", &host(true, false, None)).unwrap_err();
        assert!(err.contains("nodejs and npm"), "{err}");
        assert!(install_steps("nope", &host(true, true, None)).is_err());
    }

    #[test]
    fn script_installs_update_themselves() {
        let path = Path::new("/home/u/.local/bin/claude");
        assert_eq!(update_steps("claude", path).unwrap(), vec![Step::user("/home/u/.local/bin/claude", &["update"])]);
        let path = Path::new("/home/u/.opencode/bin/opencode");
        assert_eq!(update_steps("opencode", path).unwrap()[0].args, ["upgrade"]);
    }

    #[test]
    fn npm_installs_update_in_their_own_prefix() {
        let dir = temp("npm");
        let prefix = dir.as_path();
        std::fs::create_dir_all(prefix.join("lib/node_modules/@openai/codex")).unwrap();
        std::fs::create_dir_all(prefix.join("bin")).unwrap();
        let steps = update_steps("codex", &prefix.join("bin/codex")).unwrap();
        let p = prefix.to_string_lossy();
        assert_eq!(steps, vec![Step::user("npm", &["install", "-g", "--prefix", &p, "@openai/codex@latest"])]);
    }

    #[test]
    fn pi_under_its_old_name_is_replaced_by_the_new_package() {
        let dir = temp("oldpi");
        let prefix = dir.as_path();
        std::fs::create_dir_all(prefix.join("lib/node_modules").join(OLD_PI)).unwrap();
        let steps = update_steps("pi", &prefix.join("bin/pi")).unwrap();
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].args[..2], ["uninstall", "-g"]);
        assert_eq!(steps[0].args.last().unwrap(), OLD_PI);
        assert_eq!(steps[1].args.last().unwrap(), "@earendil-works/pi-coding-agent@latest");
    }

    #[test]
    fn a_cli_not_installed_by_npm_is_not_updated() {
        let dir = temp("notnpm");
        let err = update_steps("gemini", &dir.join("bin/gemini")).unwrap_err();
        assert!(err.contains("the way you installed it"), "{err}");
    }

    #[test]
    fn run_stops_at_the_first_failure_with_its_output() {
        let lines = std::cell::RefCell::new(Vec::new());
        let say = |l: String| lines.borrow_mut().push(l);
        let ok = Step::user("sh", &["-c", "echo one"]);
        let bad = Step::user("sh", &["-c", "echo broken >&2; exit 3"]);
        let never = Step::user("sh", &["-c", "echo never"]);
        let err = run(&[ok, bad, never], &say).unwrap_err();
        assert!(err.contains("broken"), "{err}");
        let lines = lines.into_inner();
        assert!(lines.contains(&"one".to_string()));
        assert!(!lines.contains(&"never".to_string()));
    }

    #[test]
    fn every_harness_can_be_installed_and_signed_in_to() {
        for h in crate::engine::harness::HARNESSES {
            assert!(source(h.id()).is_some(), "{} has no install source", h.id());
            assert!(sign_in_command(h.id()).is_some(), "{} has no sign-in command", h.id());
        }
    }
}
