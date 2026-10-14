//! Git operations for the Git tab, run with the `git` CLI in a chat's folder.

use std::{
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

/// Longest a local git command may run before it is killed.
const LOCAL_TIMEOUT: Duration = Duration::from_secs(10);
/// Longest a push, pull or commit may run: they wait on the network or on hooks.
const NETWORK_TIMEOUT: Duration = Duration::from_secs(180);
/// Most diff lines returned; the rest is cut off.
const MAX_DIFF_LINES: usize = 2000;

/// The repository state of a chat's folder.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Status {
    /// Whether the folder is inside a git repository.
    pub repo: bool,
    /// The checked-out branch; `None` when detached.
    pub branch: Option<String>,
    /// Whether the repository has a commit yet.
    pub has_commits: bool,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub files: Vec<FileChange>,
}

/// A changed file. A file can be staged and changed again, so it can have both.
#[derive(Debug, Clone, PartialEq)]
pub struct FileChange {
    /// Path relative to the repository root; for a rename, the new path.
    pub path: String,
    /// Status letter in the index (`M`, `A`, `D`, `R`…), if staged.
    pub staged: Option<char>,
    /// Status letter in the working tree, if changed since it was staged.
    pub unstaged: Option<char>,
    pub untracked: bool,
}

/// A commit from `log`.
#[derive(Debug, Clone, PartialEq)]
pub struct Commit {
    pub hash: String,
    pub subject: String,
    pub author: String,
    /// Relative date, e.g. "2 hours ago".
    pub date: String,
}

/// What a finished git command printed.
struct Output {
    success: bool,
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

/// Runs `git <args>` in `dir`, killing it if it outlives `timeout`.
fn run(dir: &Path, args: &[&str], timeout: Duration) -> Result<Output, String> {
    let mut child = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Couldn't run git: {e}"))?;
    // Read on their own threads, so a full pipe can't stall the command.
    let stdout = drain(child.stdout.take());
    let stderr = drain(child.stderr.take());
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            break status;
        }
        if started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("git timed out after {}s", timeout.as_secs()));
        }
        thread::sleep(Duration::from_millis(20));
    };
    Ok(Output {
        success: status.success(),
        code: status.code(),
        stdout: stdout.join().unwrap_or_default(),
        stderr: stderr.join().unwrap_or_default(),
    })
}

/// Reads a child's output pipe to its end on a thread.
fn drain(pipe: Option<impl Read + Send + 'static>) -> thread::JoinHandle<String> {
    thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(mut pipe) = pipe {
            let _ = pipe.read_to_end(&mut bytes);
        }
        String::from_utf8_lossy(&bytes).into_owned()
    })
}

/// `out` if git succeeded, else its error text: stderr, or stdout when stderr is empty.
fn checked(out: Output) -> Result<Output, String> {
    if out.success {
        return Ok(out);
    }
    let text = [out.stderr.trim(), out.stdout.trim()].into_iter().find(|t| !t.is_empty());
    Err(text.unwrap_or("git failed").to_string())
}

/// The last line git printed, which says what it did. Push writes to stderr, pull to stdout.
fn last_line(out: &Output) -> String {
    out.stderr
        .lines()
        .chain(out.stdout.lines())
        .rfind(|line| !line.trim().is_empty())
        .unwrap_or("Done")
        .trim()
        .to_string()
}

/// `path` as a pathspec for exactly that file. Paths from `status` are relative to the
/// repository root, not the chat's folder, so they are matched from the top.
fn pathspec(path: &str) -> String {
    format!(":(top,literal){path}")
}

fn pathspecs(paths: &[String]) -> Vec<String> {
    paths.iter().map(|path| pathspec(path)).collect()
}

/// `head` followed by one argument per pathspec.
fn with_pathspecs<'a>(head: &[&'a str], specs: &'a [String]) -> Vec<&'a str> {
    head.iter().copied().chain(specs.iter().map(String::as_str)).collect()
}

/// Whether `HEAD` names a commit.
fn has_head(dir: &Path) -> Result<bool, String> {
    Ok(run(dir, &["rev-parse", "--verify", "--quiet", "HEAD"], LOCAL_TIMEOUT)?.success)
}

/// The top of the repository that contains `dir`. Status and diff paths are relative to it.
pub fn toplevel(dir: &Path) -> Result<PathBuf, String> {
    let out = checked(run(dir, &["rev-parse", "--show-toplevel"], LOCAL_TIMEOUT)?)?;
    Ok(PathBuf::from(out.stdout.trim()))
}

/// The repository state of `dir`. A folder outside any repository has `repo: false`.
pub fn status(dir: &Path) -> Result<Status, String> {
    if !run(dir, &["rev-parse", "--is-inside-work-tree"], LOCAL_TIMEOUT)?.success {
        return Ok(Status::default());
    }
    let args = ["status", "--porcelain=v2", "--branch", "--untracked-files=all", "-z"];
    let out = checked(run(dir, &args, LOCAL_TIMEOUT)?)?;
    Ok(parse_status(&out.stdout))
}

/// Parses the output of `git status --porcelain=v2 --branch -z`.
fn parse_status(text: &str) -> Status {
    let mut status = Status { repo: true, ..Status::default() };
    let mut fields = text.split('\0').filter(|field| !field.is_empty());
    while let Some(field) = fields.next() {
        if let Some(header) = field.strip_prefix("# ") {
            let (key, value) = header.split_once(' ').unwrap_or((header, ""));
            match key {
                "branch.oid" => status.has_commits = value != "(initial)",
                "branch.head" => status.branch = (value != "(detached)").then(|| value.to_string()),
                "branch.upstream" => status.upstream = Some(value.to_string()),
                "branch.ab" => {
                    let mut counts = value.split_whitespace();
                    status.ahead = counts.next().and_then(|a| a.trim_start_matches('+').parse().ok()).unwrap_or(0);
                    status.behind = counts.next().and_then(|b| b.trim_start_matches('-').parse().ok()).unwrap_or(0);
                }
                _ => {}
            }
            continue;
        }
        match field.split(' ').next() {
            Some("1") => {
                let parts: Vec<&str> = field.splitn(9, ' ').collect();
                status.files.push(change(parts[1], parts[8]));
            }
            Some("2") => {
                let parts: Vec<&str> = field.splitn(10, ' ').collect();
                // A rename is followed by its original path, as a field of its own.
                fields.next();
                status.files.push(change(parts[1], parts[9]));
            }
            Some("u") => {
                let parts: Vec<&str> = field.splitn(11, ' ').collect();
                status.files.push(change(parts[1], parts[10]));
            }
            Some("?") => {
                if let Some(path) = field.strip_prefix("? ") {
                    status.files.push(FileChange {
                        path: path.to_string(),
                        staged: None,
                        unstaged: None,
                        untracked: true,
                    });
                }
            }
            _ => {}
        }
    }
    status
}

/// A tracked file from its `XY` status: `X` for the index, `Y` for the working tree, `.` for unchanged.
fn change(xy: &str, path: &str) -> FileChange {
    let mut letters = xy.chars();
    let staged = letters.next().filter(|c| *c != '.');
    let unstaged = letters.next().filter(|c| *c != '.');
    FileChange { path: path.to_string(), staged, unstaged, untracked: false }
}

/// The diff of `path`: against the index when `staged`, else against the index's version in the
/// working tree. An untracked file is shown as all added. Long diffs are cut off.
pub fn diff(dir: &Path, path: &str, staged: bool, untracked: bool) -> Result<String, String> {
    let out = if untracked {
        let file = toplevel(dir)?.join(path).to_string_lossy().into_owned();
        // `--no-index` exits with 1 when the files differ, which is the usual case here.
        let out = run(dir, &["diff", "--no-index", "--no-color", "--", "/dev/null", file.as_str()], LOCAL_TIMEOUT)?;
        if out.code == Some(1) {
            out
        } else {
            checked(out)?
        }
    } else {
        let spec = pathspec(path);
        let mut args = vec!["diff", "--no-color"];
        if staged {
            args.push("--cached");
        }
        args.extend(["--", spec.as_str()]);
        checked(run(dir, &args, LOCAL_TIMEOUT)?)?
    };
    let mut lines = out.stdout.lines();
    let mut text: Vec<&str> = lines.by_ref().take(MAX_DIFF_LINES).collect();
    if lines.next().is_some() {
        text.push("… (truncated)");
    }
    Ok(text.join("\n"))
}

/// Added and removed line counts per file, from `git diff --numstat`: the index against `HEAD` when
/// `staged`, else the working tree against the index. Paths are relative to the repository root.
/// A binary file counts as 0 and 0.
pub fn numstat(dir: &Path, staged: bool) -> Result<Vec<(String, u32, u32)>, String> {
    let mut args = vec!["diff", "--numstat", "--no-color"];
    if staged {
        args.push("--cached");
    }
    let out = checked(run(dir, &args, LOCAL_TIMEOUT)?)?;
    Ok(parse_numstat(&out.stdout))
}

/// The rows of `git diff --numstat`: `added<TAB>removed<TAB>path`, with `-` counts for binary files.
fn parse_numstat(text: &str) -> Vec<(String, u32, u32)> {
    text.lines()
        .filter_map(|line| {
            let mut fields = line.splitn(3, '\t');
            let added = fields.next()?.parse().unwrap_or(0);
            let removed = fields.next()?.parse().unwrap_or(0);
            Some((fields.next()?.to_string(), added, removed))
        })
        .collect()
}

/// Stages `paths`, including deletions. Does nothing for an empty list.
pub fn stage(dir: &Path, paths: &[String]) -> Result<(), String> {
    if paths.is_empty() {
        return Ok(());
    }
    let specs = pathspecs(paths);
    checked(run(dir, &with_pathspecs(&["add", "--"], &specs), LOCAL_TIMEOUT)?).map(|_| ())
}

/// Stages every change in the repository.
pub fn stage_all(dir: &Path) -> Result<(), String> {
    checked(run(dir, &["add", "-A"], LOCAL_TIMEOUT)?).map(|_| ())
}

/// Unstages `paths`, keeping their working-tree changes. Does nothing for an empty list.
pub fn unstage(dir: &Path, paths: &[String]) -> Result<(), String> {
    if paths.is_empty() {
        return Ok(());
    }
    unstage_specs(dir, &pathspecs(paths))
}

/// Unstages every change in the repository, keeping the working tree.
pub fn unstage_all(dir: &Path) -> Result<(), String> {
    unstage_specs(dir, &[":(top)".to_string()])
}

/// Unstages what `specs` match. Without a commit there is no `HEAD` to restore from, so the
/// index entries are removed instead.
fn unstage_specs(dir: &Path, specs: &[String]) -> Result<(), String> {
    let args = if has_head(dir)? {
        with_pathspecs(&["restore", "--staged", "--"], specs)
    } else {
        with_pathspecs(&["rm", "--cached", "-r", "--"], specs)
    };
    checked(run(dir, &args, LOCAL_TIMEOUT)?).map(|_| ())
}

/// Commits the staged changes with `message`. Returns git's summary line, e.g.
/// `[main 1a2b3c4] Subject`.
pub fn commit(dir: &Path, message: &str) -> Result<String, String> {
    // Commit hooks (linters, tests) can take a while.
    let out = checked(run(dir, &["commit", "-m", message], NETWORK_TIMEOUT)?)?;
    Ok(out.stdout.lines().next().unwrap_or_default().trim().to_string())
}

/// Pushes the current branch to its upstream. Returns git's last line.
pub fn push(dir: &Path) -> Result<String, String> {
    Ok(last_line(&checked(run(dir, &["push"], NETWORK_TIMEOUT)?)?))
}

/// Fast-forwards the current branch from its upstream. Returns git's last line.
pub fn pull(dir: &Path) -> Result<String, String> {
    Ok(last_line(&checked(run(dir, &["pull", "--ff-only"], NETWORK_TIMEOUT)?)?))
}

/// Merges the current branch's upstream into it, same as `pull` but creating a merge commit
/// when the branches have diverged instead of failing. Returns git's last line.
pub fn auto_merge(dir: &Path) -> Result<String, String> {
    Ok(last_line(&checked(run(dir, &["pull", "--no-rebase", "--no-edit"], NETWORK_TIMEOUT)?)?))
}

/// Creates a repository in `dir`. Returns git's last line.
pub fn init(dir: &Path) -> Result<String, String> {
    Ok(last_line(&checked(run(dir, &["init"], LOCAL_TIMEOUT)?)?))
}

/// The newest `n` commits of the current branch. Empty when there are none yet.
pub fn log(dir: &Path, n: usize) -> Result<Vec<Commit>, String> {
    if !has_head(dir)? {
        return Ok(Vec::new());
    }
    let count = n.to_string();
    let args = ["log", "-n", count.as_str(), "--format=%h%x1f%s%x1f%an%x1f%cr"];
    let out = checked(run(dir, &args, LOCAL_TIMEOUT)?)?;
    Ok(out
        .stdout
        .lines()
        .filter_map(|line| {
            let mut fields = line.split('\x1f');
            Some(Commit {
                hash: fields.next()?.to_string(),
                subject: fields.next()?.to_string(),
                author: fields.next()?.to_string(),
                date: fields.next()?.to_string(),
            })
        })
        .collect())
}

/// The text of `path` at `HEAD`. `None` if `dir` is not in a repository, the file is not in `HEAD`
/// (it is new), it is binary, or git fails.
pub fn head_version(dir: &Path, path: &Path) -> Option<String> {
    let top = toplevel(dir).ok()?;
    let rel = path.strip_prefix(&top).ok()?.to_string_lossy().into_owned();
    let out = run(dir, &["show", &format!("HEAD:{rel}")], LOCAL_TIMEOUT).ok()?;
    (out.success && !out.stdout.contains('\0')).then_some(out.stdout)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Removes the test folder when dropped, even if the test panics.
    struct Cleanup(PathBuf);

    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Runs git in `dir` for test setup, panicking on failure.
    fn git(dir: &Path, args: &[&str]) {
        let ok = Command::new("git").arg("-C").arg(dir).args(args).status().unwrap().success();
        assert!(ok, "git {args:?} failed");
    }

    fn file(path: &str, staged: Option<char>, unstaged: Option<char>, untracked: bool) -> FileChange {
        FileChange { path: path.to_string(), staged, unstaged, untracked }
    }

    fn configure_identity(dir: &Path) {
        git(dir, &["config", "user.name", "ArchonUI"]);
        git(dir, &["config", "user.email", "archonui@localhost"]);
        git(dir, &["config", "commit.gpgsign", "false"]);
    }

    #[test]
    fn repository_lifecycle() {
        let dir = std::env::temp_dir().join(format!("archonui-git-test-{}", std::process::id()));
        let _cleanup = Cleanup(dir.clone());
        std::fs::create_dir_all(&dir).unwrap();

        assert!(!status(&dir).unwrap().repo);

        init(&dir).unwrap();
        let s = status(&dir).unwrap();
        assert!(s.repo && !s.has_commits && s.files.is_empty());
        assert!(log(&dir, 10).unwrap().is_empty());

        std::fs::write(dir.join("a.txt"), "one\n").unwrap();
        assert_eq!(status(&dir).unwrap().files, vec![file("a.txt", None, None, true)]);

        stage(&dir, &["a.txt".to_string()]).unwrap();
        assert_eq!(status(&dir).unwrap().files, vec![file("a.txt", Some('A'), None, false)]);

        // Identity for commits, set in this test repository only.
        git(&dir, &["config", "user.name", "ArchonUI"]);
        git(&dir, &["config", "user.email", "archonui@localhost"]);
        git(&dir, &["config", "commit.gpgsign", "false"]);
        let summary = commit(&dir, "first").unwrap();
        assert!(summary.contains("first"), "{summary}");
        let commits = log(&dir, 10).unwrap();
        assert_eq!(commits.len(), 1);
        assert_eq!(commits[0].subject, "first");
        assert_eq!(commits[0].author, "ArchonUI");
        assert!(status(&dir).unwrap().files.is_empty());

        std::fs::write(dir.join("a.txt"), "one\ntwo\n").unwrap();
        assert_eq!(status(&dir).unwrap().files, vec![file("a.txt", None, Some('M'), false)]);
        let text = diff(&dir, "a.txt", false, false).unwrap();
        assert!(text.lines().any(|line| line == "+two"), "{text}");

        stage(&dir, &["a.txt".to_string()]).unwrap();
        unstage_all(&dir).unwrap();
        assert_eq!(status(&dir).unwrap().files, vec![file("a.txt", None, Some('M'), false)]);

        // Paths are relative to the repository root, so a chat in a subfolder matches the same files.
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("sub").join("c.txt"), "hello\n").unwrap();
        let sub = dir.join("sub");
        let untracked = status(&sub).unwrap().files.into_iter().find(|f| f.untracked).unwrap();
        assert_eq!(untracked.path, "sub/c.txt");
        let text = diff(&sub, "sub/c.txt", false, true).unwrap();
        assert!(text.lines().any(|line| line == "+hello"), "{text}");
        stage(&sub, &["sub/c.txt".to_string()]).unwrap();
        assert!(status(&dir).unwrap().files.contains(&file("sub/c.txt", Some('A'), None, false)));
        unstage(&sub, &["sub/c.txt".to_string()]).unwrap();
        assert!(status(&dir).unwrap().files.contains(&file("sub/c.txt", None, None, true)));
    }

    #[test]
    fn unstage_without_commits() {
        let dir = std::env::temp_dir().join(format!("archonui-git-unstage-{}", std::process::id()));
        let _cleanup = Cleanup(dir.clone());
        std::fs::create_dir_all(&dir).unwrap();
        init(&dir).unwrap();
        std::fs::write(dir.join("a.txt"), "one\n").unwrap();
        stage_all(&dir).unwrap();
        unstage_all(&dir).unwrap();
        assert_eq!(status(&dir).unwrap().files, vec![file("a.txt", None, None, true)]);
    }

    #[test]
    fn parses_porcelain_v2_with_rename_and_untracked() {
        let text = "# branch.oid 1234\0# branch.head main\0# branch.upstream origin/main\0# branch.ab +1 -2\0\
            2 R. N... 100644 100644 100644 abc def R100 new name.txt\0old.txt\0\
            1 .M N... 100644 100644 100644 abc abc src/lib.rs\0\
            ? untracked.txt\0";
        let s = parse_status(text);
        assert!(s.repo && s.has_commits);
        assert_eq!(s.branch.as_deref(), Some("main"));
        assert_eq!(s.upstream.as_deref(), Some("origin/main"));
        assert_eq!((s.ahead, s.behind), (1, 2));
        assert_eq!(
            s.files,
            vec![
                file("new name.txt", Some('R'), None, false),
                file("src/lib.rs", None, Some('M'), false),
                file("untracked.txt", None, None, true),
            ]
        );
    }

    #[test]
    fn parses_detached_repository_without_commits() {
        let s = parse_status("# branch.oid (initial)\0# branch.head (detached)\0");
        assert!(s.repo && !s.has_commits);
        assert_eq!(s.branch, None);
        assert!(s.files.is_empty());
    }

    #[test]
    fn head_version_is_the_committed_text() {
        let dir = std::env::temp_dir().join(format!("archonui-head-test-{}", std::process::id()));
        let _cleanup = Cleanup(dir.clone());
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        assert_eq!(head_version(&dir, &dir.join("sub/a.txt")), None);

        init(&dir).unwrap();
        git(&dir, &["config", "user.name", "ArchonUI"]);
        git(&dir, &["config", "user.email", "archonui@localhost"]);
        git(&dir, &["config", "commit.gpgsign", "false"]);
        std::fs::write(dir.join("sub/a.txt"), "one\n").unwrap();
        std::fs::write(dir.join("bin.dat"), b"\0\x01").unwrap();
        stage_all(&dir).unwrap();
        commit(&dir, "first").unwrap();
        std::fs::write(dir.join("sub/a.txt"), "two\n").unwrap();

        assert_eq!(head_version(&dir, &dir.join("sub/a.txt")).as_deref(), Some("one\n"));
        assert_eq!(head_version(&dir, &dir.join("bin.dat")), None);
        std::fs::write(dir.join("new.txt"), "x").unwrap();
        assert_eq!(head_version(&dir, &dir.join("new.txt")), None);
    }

    #[test]
    fn auto_merge_joins_diverged_branches_pull_ff_only_cannot() {
        let root = std::env::temp_dir().join(format!("archonui-git-merge-test-{}", std::process::id()));
        let _cleanup = Cleanup(root.clone());
        let remote = root.join("remote.git");
        let a = root.join("a");
        let b = root.join("b");
        std::fs::create_dir_all(&remote).unwrap();
        git(&remote, &["init", "--bare", "-b", "main"]);

        git(&root, &["clone", remote.to_str().unwrap(), a.to_str().unwrap()]);
        configure_identity(&a);
        std::fs::write(a.join("base.txt"), "base\n").unwrap();
        git(&a, &["add", "base.txt"]);
        commit(&a, "base").unwrap();
        push(&a).unwrap();

        git(&root, &["clone", remote.to_str().unwrap(), b.to_str().unwrap()]);
        configure_identity(&b);

        // `a` and `b` each commit independently, so `b`'s upstream diverges from its own history.
        std::fs::write(a.join("from-a.txt"), "a\n").unwrap();
        git(&a, &["add", "from-a.txt"]);
        commit(&a, "from a").unwrap();
        push(&a).unwrap();

        std::fs::write(b.join("from-b.txt"), "b\n").unwrap();
        git(&b, &["add", "from-b.txt"]);
        commit(&b, "from b").unwrap();

        // A plain fast-forward pull cannot reconcile diverged history.
        assert!(pull(&b).is_err());

        let summary = auto_merge(&b).unwrap();
        assert!(!summary.is_empty());
        assert!(b.join("from-a.txt").is_file());
        assert!(b.join("from-b.txt").is_file());
        assert!(status(&b).unwrap().files.is_empty());

        let commits = log(&b, 10).unwrap();
        let subjects: Vec<&str> = commits.iter().map(|c| c.subject.as_str()).collect();
        assert!(subjects.contains(&"from a"));
        assert!(subjects.contains(&"from b"));
    }
}
