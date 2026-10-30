//! Self-update for installs made with install.sh: compare the commit this binary was built from
//! (embedded by build.rs) with the repo's branch on GitHub, and update the same way install.sh does:
//! pull the source checkout, `cargo build --release`, and put the binary in place.
//!
//! Safety: nothing changes until the user clicks. A checkout with local changes or commits that aren't
//! on GitHub is left alone; a failed fetch or build leaves the old binary in place (a failed build also
//! puts the checkout back), and the new binary is renamed over the old one in a single step.

use serde::{Deserialize, Serialize};
use std::{
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

/// The GitHub repo install.sh clones.
pub const REPO: &str = "Abdullahalmutairi97/archonui";
/// The branch install.sh installs by default.
pub const DEFAULT_BRANCH: &str = "main";
/// The commit this binary was built from; empty for a build outside git.
pub const BUILD_COMMIT: &str = env!("ARCHONUI_GIT_COMMIT");
/// The branch this binary was built from; empty when detached or outside git.
pub const BUILD_BRANCH: &str = env!("ARCHONUI_GIT_BRANCH");
/// The one-line installer from the README, shown when the app can't update itself.
pub const INSTALL_COMMAND: &str = "curl -fsSL https://raw.githubusercontent.com/Abdullahalmutairi97/archonui/main/install.sh | sh";
/// The quiet startup check runs at most this often.
pub const CHECK_EVERY_SECS: u64 = 24 * 60 * 60;

/// What the check found.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Status {
    UpToDate,
    /// The branch has `count` commits this build doesn't; `title` is the newest one's first line.
    Behind { count: u32, title: String, sha: String },
    /// This build has commits the branch doesn't have, and the branch has `count` new ones.
    Diverged { count: u32, title: String },
    /// This build is newer than the branch (built from local commits).
    Ahead,
    /// Couldn't tell; the reason is shown as it is.
    Unknown(String),
}

impl Status {
    /// One line for Settings.
    pub fn summary(&self) -> String {
        match self {
            Status::UpToDate => "Up to date".into(),
            Status::Behind { count: 1, title, .. } => format!("1 new commit: {title}"),
            Status::Behind { count, title, .. } => format!("{count} new commits · latest: {title}"),
            Status::Diverged { count, title } => {
                format!("{count} new on GitHub (latest: {title}), but this build has its own commits too")
            }
            Status::Ahead => "This build is newer than GitHub's".into(),
            Status::Unknown(why) => why.clone(),
        }
    }

    pub fn update_available(&self) -> bool {
        matches!(self, Status::Behind { .. })
    }
}

/// The parts of GitHub's compare response (`GET /repos/{repo}/compare/{base}...{head}`) used here.
#[derive(Deserialize)]
struct CompareJson {
    status: String,
    ahead_by: u32,
    #[serde(default)]
    commits: Vec<CompareCommit>,
}

#[derive(Deserialize)]
struct CompareCommit {
    sha: String,
    commit: CommitBody,
}

#[derive(Deserialize)]
struct CommitBody {
    message: String,
}

/// The status for a build at `build_commit`, from GitHub's answer to comparing it with the branch:
/// `Ok((http status, body))`, or `Err` when the request itself failed (no network, no curl).
pub fn decide(build_commit: &str, response: Result<(u16, String), String>) -> Status {
    if build_commit.is_empty() {
        return Status::Unknown("This build doesn't know which commit it came from, so it can't be compared".into());
    }
    let (code, body) = match response {
        Ok(answer) => answer,
        Err(e) => return Status::Unknown(format!("Couldn't reach GitHub ({e}). Check your connection")),
    };
    match code {
        200 => {}
        404 => return Status::Unknown("This build's commit isn't on GitHub (a local build), so it can't be compared".into()),
        403 | 429 => return Status::Unknown("GitHub is rate-limiting update checks. Try again in an hour".into()),
        code => return Status::Unknown(format!("GitHub answered {code}. Try again later")),
    }
    let Ok(compare) = serde_json::from_str::<CompareJson>(&body) else {
        return Status::Unknown("GitHub's answer couldn't be read".into());
    };
    let latest = || {
        compare.commits.last().map_or_else(
            || (String::from("(no title)"), String::new()),
            |c| (c.commit.message.lines().next().unwrap_or("").trim().to_string(), c.sha.clone()),
        )
    };
    match compare.status.as_str() {
        "identical" => Status::UpToDate,
        "ahead" if compare.ahead_by == 0 => Status::UpToDate,
        "ahead" => {
            let (title, sha) = latest();
            Status::Behind { count: compare.ahead_by, title, sha }
        }
        "behind" => Status::Ahead,
        "diverged" => Status::Diverged { count: compare.ahead_by, title: latest().0 },
        other => Status::Unknown(format!("GitHub answered with an unknown status \"{other}\"")),
    }
}

/// Asks GitHub to compare `commit` with `branch`. Uses curl, which install.sh needs too.
pub fn fetch_compare(commit: &str, branch: &str) -> Result<(u16, String), String> {
    let url = format!("https://api.github.com/repos/{REPO}/compare/{commit}...{branch}");
    let out = Command::new("curl")
        .args(["-sS", "-m", "15", "-H", "Accept: application/vnd.github+json", "-H", "User-Agent: archonui", "-w", "\n%{http_code}", &url])
        .output()
        .map_err(|e| format!("curl: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(if err.is_empty() { "curl failed".into() } else { err });
    }
    split_status(&String::from_utf8_lossy(&out.stdout))
}

/// curl's `-w "\n%{http_code}"` output split into the status and the body.
fn split_status(out: &str) -> Result<(u16, String), String> {
    let (body, code) = out.rsplit_once('\n').ok_or("no answer")?;
    let code = code.trim().parse::<u16>().map_err(|_| "no answer".to_string())?;
    Ok((code, body.to_string()))
}

/// Checks this build against `branch` (the network part runs here; call it off the UI thread).
/// `ARCHONUI_UPDATE_COMMIT` stands in for the build's commit (a developer switch for testing).
pub fn check(branch: &str) -> Status {
    let commit = std::env::var("ARCHONUI_UPDATE_COMMIT").ok().filter(|c| !c.is_empty()).unwrap_or_else(|| BUILD_COMMIT.to_string());
    if commit.is_empty() {
        return decide(&commit, Err(String::new()));
    }
    let status = decide(&commit, fetch_compare(&commit, branch));
    // A build from a branch GitHub doesn't have (a local feature branch) is compared with main instead.
    let missing = matches!(&status, Status::Unknown(why) if why.contains("isn't on GitHub"));
    if missing && branch != DEFAULT_BRANCH {
        return decide(&commit, fetch_compare(&commit, DEFAULT_BRANCH));
    }
    status
}

/// The branch to follow: the one this build came from when it's a normal branch name, else main.
pub fn follow_branch(build_branch: &str) -> String {
    let ok = !build_branch.is_empty() && build_branch.chars().all(|c| c.is_ascii_alphanumeric() || "-_./".contains(c));
    if ok { build_branch.to_string() } else { DEFAULT_BRANCH.to_string() }
}

/// Whether the quiet startup check is due: never checked, or at least a day ago (or the clock moved back).
pub fn check_due(last: Option<u64>, now: u64) -> bool {
    last.map_or(true, |last| now < last || now - last >= CHECK_EVERY_SECS)
}

/// Where the app came from, by install.sh's layout.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    /// install.sh's checkout and the installed binary.
    Checkout { src: PathBuf, bin: PathBuf },
    /// Not installed from a checkout: show `INSTALL_COMMAND` and why.
    Elsewhere { why: String },
}

/// install.sh's paths: the checkout is `$ARCHONUI_SRC` or `$XDG_DATA_HOME/archonui-src`
/// (`~/.local/share/archonui-src`), and the binary goes to `$ARCHONUI_BIN_DIR` or `~/.local/bin`.
pub fn locate(env: impl Fn(&str) -> Option<String>, home: &Path) -> Source {
    let data = env("XDG_DATA_HOME").filter(|d| !d.is_empty()).map_or_else(|| home.join(".local/share"), PathBuf::from);
    let src = env("ARCHONUI_SRC").filter(|s| !s.is_empty()).map_or_else(|| data.join("archonui-src"), PathBuf::from);
    let bin_dir = env("ARCHONUI_BIN_DIR").filter(|s| !s.is_empty()).map_or_else(|| home.join(".local/bin"), PathBuf::from);
    if !src.join(".git").exists() || !src.join("Cargo.toml").is_file() {
        return Source::Elsewhere {
            why: format!("ArchonUI wasn't installed from a source checkout (none at {}), so it can't update itself", src.display()),
        };
    }
    Source::Checkout { src, bin: bin_dir.join("archonui") }
}

/// The reason not to touch a checkout, if any: `porcelain` is `git status --porcelain -uno` and
/// `fast_forward` whether the checkout's HEAD is an ancestor of the fetched branch.
pub fn blocked(src: &Path, porcelain: &str, fast_forward: bool, branch: &str) -> Option<String> {
    if !porcelain.trim().is_empty() {
        return Some(format!(
            "The checkout at {} has local changes, so it wasn't touched. Commit or stash them, then try again",
            src.display()
        ));
    }
    if !fast_forward {
        return Some(format!(
            "The checkout at {} has commits that aren't on origin/{branch}, so it wasn't touched",
            src.display()
        ));
    }
    None
}

fn git(src: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git").arg("-C").arg(src).args(args).output().map_err(|e| format!("git: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// cargo on PATH, else rustup's default place (install.sh installs it there).
fn cargo(home: &Path) -> PathBuf {
    let rustup = home.join(".cargo/bin/cargo");
    let on_path = std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join("cargo").is_file()));
    if !on_path && rustup.is_file() { rustup } else { PathBuf::from("cargo") }
}

/// Updates the checkout at `src` to `branch`, builds it and installs the binary at `bin`, reporting
/// progress lines through `say`. Returns the installed binary. Blocking; run it off the UI thread.
pub fn run(src: &Path, bin: &Path, branch: &str, home: &Path, say: &dyn Fn(String)) -> Result<PathBuf, String> {
    say(format!("Checking the checkout at {}", src.display()));
    let porcelain = git(src, &["status", "--porcelain", "--untracked-files=no"])?;
    if let Some(why) = blocked(src, &porcelain, true, branch) {
        return Err(why);
    }
    let old = git(src, &["rev-parse", "HEAD"])?;
    // Follow the branch the checkout is on (install.sh's ARCHONUI_BRANCH), else the one asked for.
    let checked_out = git(src, &["symbolic-ref", "--short", "-q", "HEAD"]).ok().filter(|b| !b.is_empty());
    let branch = checked_out.as_deref().unwrap_or(branch);
    say(format!("Fetching origin/{branch}"));
    git(src, &["fetch", "--quiet", "origin", branch])
        .map_err(|e| format!("Couldn't fetch from GitHub ({e}). Check your connection; nothing was changed"))?;
    let fast_forward = git(src, &["merge-base", "--is-ancestor", "HEAD", "FETCH_HEAD"]).is_ok();
    if let Some(why) = blocked(src, "", fast_forward, branch) {
        return Err(why);
    }
    let new = git(src, &["rev-parse", "FETCH_HEAD"])?;
    let title = git(src, &["log", "-1", "--format=%s", "FETCH_HEAD"]).unwrap_or_default();
    say(format!("Moving to {} {title}", &new[..new.len().min(7)]));
    git(src, &["reset", "--hard", "--quiet", "FETCH_HEAD"])?;

    say("Building (cargo build --release); this takes a few minutes".into());
    // Any failure from here on puts the checkout back at the commit the installed binary was built from.
    let restore = |why: String| {
        let _ = git(src, &["reset", "--hard", "--quiet", &old]);
        why
    };
    build(src, home, say).map_err(restore)?;

    let target = std::env::var_os("CARGO_TARGET_DIR").map_or_else(|| src.join("target"), PathBuf::from);
    install_binary(&target.join("release/archonui"), bin).map_err(|e| format!("Couldn't install the new binary ({e}). Your current ArchonUI is unchanged"))?;
    say(format!("Installed {}", bin.display()));
    let desktop = src.join("scripts/install-desktop.sh");
    if desktop.is_file() {
        match Command::new(&desktop).arg(bin).stdout(Stdio::null()).stderr(Stdio::null()).status() {
            Ok(s) if s.success() => say("Updated the launcher entry".into()),
            _ => say("The launcher entry wasn't updated (install-desktop.sh failed); the app itself is updated".into()),
        }
    }
    Ok(bin.to_path_buf())
}

/// Runs `cargo build --release` on the checkout at `src`, passing its stderr to `say`. The error names
/// the last lines of it when the build fails.
fn build(src: &Path, home: &Path, say: &dyn Fn(String)) -> Result<(), String> {
    let mut child = Command::new(cargo(home))
        .args(["build", "--release", "--color", "never", "--manifest-path"])
        .arg(src.join("Cargo.toml"))
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Couldn't start cargo ({e}). Your current ArchonUI is unchanged"))?;
    let mut tail: Vec<String> = Vec::new();
    if let Some(stderr) = child.stderr.take() {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            tail.push(line.clone());
            if tail.len() > 6 {
                tail.remove(0);
            }
            say(line);
        }
    }
    let built = child.wait().map_err(|e| format!("cargo: {e}"))?;
    if !built.success() {
        return Err(format!(
            "The build failed, so nothing was installed and your current ArchonUI is unchanged:\n{}",
            tail.join("\n")
        ));
    }
    Ok(())
}

/// Copies `built` next to `bin` and renames it over `bin`, so the old binary is replaced in one step and
/// a running copy keeps working.
pub fn install_binary(built: &Path, bin: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let dir = bin.parent().ok_or_else(|| std::io::Error::other("no folder"))?;
    std::fs::create_dir_all(dir)?;
    let staged = dir.join(".archonui.new");
    std::fs::copy(built, &staged)?;
    std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755))?;
    std::fs::rename(&staged, bin)
}

/// The last startup check, kept in the data dir.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LastCheck {
    pub at: u64,
    pub commit: String,
    pub status: Option<Status>,
}

pub fn read_last(path: &Path) -> Option<LastCheck> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

pub fn write_last(path: &Path, last: &LastCheck) {
    if let Ok(json) = serde_json::to_vec_pretty(last) {
        let _ = std::fs::write(path, json);
    }
}

pub fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    const AHEAD: &str = include_str!("../engine/fixtures/github-compare-ahead.json");

    #[test]
    fn new_commits_on_github_are_an_update() {
        let status = decide("565f2c1", Ok((200, AHEAD.into())));
        assert_eq!(
            status,
            Status::Behind {
                count: 2,
                title: "themes: eight built-in themes".into(),
                sha: "bcc0450000000000000000000000000000000000".into()
            }
        );
        assert!(status.update_available());
        assert_eq!(status.summary(), "2 new commits · latest: themes: eight built-in themes");
    }

    #[test]
    fn every_other_answer_is_not_an_update() {
        let body = |status: &str, ahead: u32| format!(r#"{{"status":"{status}","ahead_by":{ahead},"behind_by":0,"commits":[]}}"#);
        assert_eq!(decide("abc", Ok((200, body("identical", 0)))), Status::UpToDate);
        assert_eq!(decide("abc", Ok((200, body("behind", 0)))), Status::Ahead);
        assert!(matches!(decide("abc", Ok((200, body("diverged", 3)))), Status::Diverged { count: 3, .. }));
        for (response, says) in [
            (Ok((404, "{}".to_string())), "isn't on GitHub"),
            (Ok((403, "{}".to_string())), "rate-limiting"),
            (Ok((502, "".to_string())), "502"),
            (Ok((200, "not json".to_string())), "couldn't be read"),
            (Err("Could not resolve host: api.github.com".to_string()), "Couldn't reach GitHub"),
        ] {
            let status = decide("abc", response);
            assert!(!status.update_available());
            assert!(status.summary().contains(says), "{status:?}");
        }
        assert!(decide("", Ok((200, AHEAD.into()))).summary().contains("doesn't know"), "a build outside git");
        assert_eq!(
            decide("abc", Ok((200, body("ahead", 1).replace("[]", r#"[{"sha":"f00","commit":{"message":"fix: a thing\n\nbody"}}]"#)))).summary(),
            "1 new commit: fix: a thing"
        );
    }

    #[test]
    fn curl_output_splits_into_status_and_body() {
        assert_eq!(split_status("{\"a\":1}\n200").unwrap(), (200, "{\"a\":1}".to_string()));
        assert_eq!(split_status("\n404").unwrap(), (404, String::new()));
        assert!(split_status("garbage").is_err());
    }

    #[test]
    fn the_quiet_check_runs_at_most_daily() {
        assert!(check_due(None, 1_000));
        assert!(!check_due(Some(1_000), 1_000 + CHECK_EVERY_SECS - 1));
        assert!(check_due(Some(1_000), 1_000 + CHECK_EVERY_SECS));
        assert!(check_due(Some(5_000), 1_000), "a clock that went back checks again");
    }

    #[test]
    fn the_branch_followed_is_the_build_branch_or_main() {
        assert_eq!(follow_branch("main"), "main");
        assert_eq!(follow_branch("feature/self-update"), "feature/self-update");
        assert_eq!(follow_branch(""), "main");
        assert_eq!(follow_branch("weird branch;rm"), "main");
    }

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("archonui-update-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn install_sh_layout_is_found_and_anything_else_shows_the_command() {
        let home = temp("home");
        let none = |_: &str| None;
        assert!(matches!(locate(none, &home), Source::Elsewhere { .. }), "no checkout");
        let src = home.join(".local/share/archonui-src");
        std::fs::create_dir_all(src.join(".git")).unwrap();
        std::fs::write(src.join("Cargo.toml"), "").unwrap();
        assert_eq!(locate(none, &home), Source::Checkout { src: src.clone(), bin: home.join(".local/bin/archonui") });
        let custom = home.join("code/archonui");
        std::fs::create_dir_all(custom.join(".git")).unwrap();
        std::fs::write(custom.join("Cargo.toml"), "").unwrap();
        let env = |k: &str| match k {
            "ARCHONUI_SRC" => Some(custom.display().to_string()),
            "ARCHONUI_BIN_DIR" => Some("/opt/bin".into()),
            _ => None,
        };
        assert_eq!(locate(env, &home), Source::Checkout { src: custom, bin: PathBuf::from("/opt/bin/archonui") });
        std::fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn a_dirty_or_diverged_checkout_is_left_alone() {
        let src = Path::new("/src");
        assert!(blocked(src, "", true, "main").is_none());
        assert!(blocked(src, " M src/ui/theme.rs\n", true, "main").unwrap().contains("local changes"));
        assert!(blocked(src, "", false, "main").unwrap().contains("aren't on origin/main"));
    }

    fn sh_git(dir: &Path, args: &[&str]) {
        let ok = Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "init.defaultBranch=main"])
            .arg("-C")
            .arg(dir)
            .args(args)
            .output()
            .unwrap()
            .status
            .success();
        assert!(ok, "git {args:?}");
    }

    #[test]
    fn a_failed_update_changes_nothing() {
        let dir = temp("run");
        let origin = dir.join("origin");
        std::fs::create_dir_all(&origin).unwrap();
        sh_git(&origin, &["init", "-q"]);
        std::fs::write(origin.join("Cargo.toml"), "one").unwrap();
        sh_git(&origin, &["add", "."]);
        sh_git(&origin, &["commit", "-qm", "one"]);
        let src = dir.join("src");
        sh_git(&dir, &["clone", "-q", "--depth", "1", &format!("file://{}", origin.display()), "src"]);
        std::fs::write(origin.join("Cargo.toml"), "two").unwrap();
        sh_git(&origin, &["commit", "-qam", "two"]);
        let head = || git(&src, &["rev-parse", "HEAD"]).unwrap();
        let before = head();
        let bin = dir.join("bin/archonui");
        let quiet = |_: String| {};

        // Local changes: refused before fetching.
        std::fs::write(src.join("Cargo.toml"), "mine").unwrap();
        let err = run(&src, &bin, "main", &dir, &quiet).unwrap_err();
        assert!(err.contains("local changes"), "{err}");
        assert_eq!(std::fs::read_to_string(src.join("Cargo.toml")).unwrap(), "mine", "the user's edit is kept");
        sh_git(&src, &["checkout", "-q", "Cargo.toml"]);

        // No network (the origin is gone): refused, nothing moved.
        sh_git(&src, &["remote", "set-url", "origin", &format!("file://{}", dir.join("gone").display())]);
        let err = run(&src, &bin, "main", &dir, &quiet).unwrap_err();
        assert!(err.contains("Couldn't fetch"), "{err}");
        assert_eq!(head(), before);

        // A build failure (Cargo.toml isn't a manifest) puts the checkout back and installs nothing.
        sh_git(&src, &["remote", "set-url", "origin", &format!("file://{}", origin.display())]);
        let err = run(&src, &bin, "main", &dir, &quiet).unwrap_err();
        assert!(err.contains("build failed") || err.contains("cargo"), "{err}");
        assert_eq!(head(), before, "the checkout matches the binary still installed");
        assert!(!bin.exists());

        // A good commit: fast-forwarded, built, installed, and the progress was reported.
        std::fs::write(origin.join("Cargo.toml"), "[package]\nname = \"archonui\"\nversion = \"0.0.0\"\nedition = \"2021\"\n[workspace]\n").unwrap();
        std::fs::create_dir_all(origin.join("src")).unwrap();
        std::fs::write(origin.join("src/main.rs"), "fn main() {}\n").unwrap();
        std::fs::write(origin.join(".gitignore"), "target\n").unwrap();
        sh_git(&origin, &["add", "."]);
        sh_git(&origin, &["commit", "-qm", "three: a crate that builds"]);
        let lines = std::sync::Mutex::new(Vec::new());
        let say = |line: String| lines.lock().unwrap().push(line);
        let installed = run(&src, &bin, "main", &dir, &say).unwrap();
        assert_eq!(installed, bin);
        assert!(bin.is_file());
        assert_eq!(head(), git(&origin, &["rev-parse", "HEAD"]).unwrap());
        let lines = lines.into_inner().unwrap();
        assert!(lines.iter().any(|l| l.contains("three: a crate that builds")), "{lines:?}");
        assert!(lines.iter().any(|l| l.starts_with("Installed")), "{lines:?}");

        // Local commits that aren't on GitHub: refused.
        std::fs::write(src.join("notes"), "x").unwrap();
        sh_git(&src, &["add", "notes"]);
        sh_git(&src, &["commit", "-qm", "mine"]);
        let err = run(&src, &bin, "main", &dir, &quiet).unwrap_err();
        assert!(err.contains("aren't on origin/main"), "{err}");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_new_binary_replaces_the_old_in_one_step() {
        let dir = temp("bin");
        let built = dir.join("built");
        std::fs::write(&built, b"new").unwrap();
        let bin = dir.join("bin/archonui");
        std::fs::create_dir_all(bin.parent().unwrap()).unwrap();
        std::fs::write(&bin, b"old").unwrap();
        install_binary(&built, &bin).unwrap();
        assert_eq!(std::fs::read(&bin).unwrap(), b"new");
        assert!(!dir.join("bin/.archonui.new").exists());
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&bin).unwrap().permissions().mode() & 0o777, 0o755);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn last_check_round_trips() {
        let dir = temp("last");
        let path = dir.join("update-check.json");
        assert!(read_last(&path).is_none());
        let last = LastCheck { at: 42, commit: "abc".into(), status: Some(Status::Behind { count: 1, title: "t".into(), sha: "s".into() }) };
        write_last(&path, &last);
        assert_eq!(read_last(&path), Some(last));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
