//! Snapshots of a project's working files for the cloud copy: tracked files plus untracked files
//! not ignored by .gitignore, kept as commits on a private ref. HEAD, the index, branches and the
//! files themselves are never changed.
//!
//! Folders that are not git repositories get a private git dir under the data folder instead,
//! so nothing is written into the project.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::store;

/// Ref that holds this device's snapshots of a project.
pub fn wip_ref(device: &str) -> String {
    format!("refs/archon/wip/{device}")
}

/// Snapshots the working files of `dir` as a commit on `wip_ref(device)`. Returns the new commit
/// id, or None when nothing changed since the last snapshot. Secret-looking files are left out
/// unless `secrets` is true.
pub fn take(dir: &Path, device: &str, secrets: bool) -> Result<Option<String>, String> {
    take_in(dir, device, &snapshots_dir(), secrets)
}

/// A git bundle of `wip_ref(device)` holding only what `since` lacks. A full bundle when `since`
/// is None or unknown here. Empty when `since` is already the current snapshot.
pub fn bundle(dir: &Path, device: &str, since: Option<&str>) -> Result<Vec<u8>, String> {
    bundle_in(dir, device, since, &snapshots_dir())
}

/// The commit of this device's latest snapshot of `dir`, if any.
pub fn current(dir: &Path, device: &str) -> Option<String> {
    check_device(device).ok()?;
    let repo = Repo::open(dir, &snapshots_dir()).ok()?;
    run(repo.git().args(["rev-parse", "--verify", "-q", wip_ref(device).as_str()])).ok()
}

/// Writes the files of `commit` (from a full bundle) into `dest`, which must not exist or be empty.
/// The result is plain files plus a fresh repository whose branch "main" holds the snapshot.
pub fn restore(bundle: &[u8], commit: &str, dest: &Path) -> Result<(), String> {
    if !is_hex(commit) {
        return Err(format!("not a commit id: {commit:?}"));
    }
    if dest.exists() {
        let mut entries = fs::read_dir(dest).map_err(|e| format!("{}: {e}", dest.display()))?;
        if entries.next().is_some() {
            return Err(format!("{} is not empty", dest.display()));
        }
    } else {
        fs::create_dir_all(dest).map_err(|e| format!("{}: {e}", dest.display()))?;
    }
    let file = temp_path("restore");
    fs::write(&file, bundle).map_err(|e| format!("write bundle: {e}"))?;
    let result = fetch_into(dest, &file, commit);
    let _ = fs::remove_file(&file);
    result
}

fn take_in(dir: &Path, device: &str, snapshots: &Path, secrets: bool) -> Result<Option<String>, String> {
    check_device(device)?;
    let repo = Repo::open(dir, snapshots)?;
    let index = temp_path("index");
    let result = take_with(&repo, device, &index, secrets);
    let _ = fs::remove_file(&index);
    result
}

/// The snapshot itself, built in a temporary index so the real one is never touched.
fn take_with(repo: &Repo, device: &str, index: &Path, secrets: bool) -> Result<Option<String>, String> {
    let r = wip_ref(device);
    let has_head = succeeds(repo.git().args(["rev-parse", "--verify", "-q", "HEAD"]));
    if has_head {
        run(indexed(repo, index).args(["read-tree", "HEAD"]))?;
    }
    run(indexed(repo, index).args(["add", "-A"]))?;
    if !secrets {
        drop_secrets(repo, index)?;
    }
    let tree = run(indexed(repo, index).args(["write-tree"]))?;

    let prev = run(repo.git().args(["rev-parse", "--verify", "-q", r.as_str()])).ok();
    if let Some(p) = &prev {
        let spec = format!("{p}^{{tree}}");
        if run(repo.git().args(["rev-parse", spec.as_str()]))? == tree {
            return Ok(None);
        }
    }
    let parent = match prev {
        Some(p) => Some(p),
        None if has_head => Some(run(repo.git().args(["rev-parse", "HEAD"]))?),
        None => None,
    };

    let mut commit = repo.git();
    commit.args(["-c", "commit.gpgsign=false", "commit-tree", tree.as_str()]);
    if let Some(p) = &parent {
        commit.args(["-p", p.as_str()]);
    }
    commit
        .args(["-m", "archonui snapshot"])
        .env("GIT_AUTHOR_NAME", "ArchonUI")
        .env("GIT_AUTHOR_EMAIL", "archonui@localhost")
        .env("GIT_COMMITTER_NAME", "ArchonUI")
        .env("GIT_COMMITTER_EMAIL", "archonui@localhost");
    let id = run(&mut commit)?;
    run(repo.git().args(["update-ref", r.as_str(), id.as_str()]))?;
    Ok(Some(id))
}

fn bundle_in(dir: &Path, device: &str, since: Option<&str>, snapshots: &Path) -> Result<Vec<u8>, String> {
    check_device(device)?;
    let repo = Repo::open(dir, snapshots)?;
    let r = wip_ref(device);
    let cur = run(repo.git().args(["rev-parse", "--verify", "-q", r.as_str()]))
        .map_err(|_| format!("no snapshot of this project for {device} yet"))?;
    let base = since.filter(|s| {
        is_hex(s) && {
            let spec = format!("{s}^{{commit}}");
            succeeds(repo.git().args(["cat-file", "-e", spec.as_str()]))
        }
    });
    if base == Some(cur.as_str()) {
        return Ok(Vec::new());
    }

    let file = temp_path("bundle");
    let out = match create_bundle(&repo, &r, base, &file) {
        Ok(true) => fs::read(&file).map_err(|e| format!("read bundle: {e}")),
        Ok(false) => Ok(Vec::new()),
        Err(e) => Err(e),
    };
    let _ = fs::remove_file(&file);
    out
}

/// Writes a bundle of `r` (minus `base`) to `file`. False when there was nothing to bundle.
fn create_bundle(repo: &Repo, r: &str, base: Option<&str>, file: &Path) -> Result<bool, String> {
    let mut cmd = repo.git();
    cmd.arg("bundle").arg("create").arg(file).arg(r);
    if let Some(b) = base {
        cmd.arg(format!("^{b}"));
    }
    match run(&mut cmd) {
        Ok(_) => Ok(true),
        Err(e) if e.contains("empty bundle") => Ok(false),
        Err(e) => Err(e),
    }
}

fn fetch_into(dest: &Path, bundle: &Path, commit: &str) -> Result<(), String> {
    let repo = Repo { git_dir: None, work: dest.to_path_buf() };
    run(repo.git().args(["init", "-q"]))?;
    run(repo.git().args(["symbolic-ref", "HEAD", "refs/heads/main"]))?;
    run(repo.git().arg("fetch").arg("-q").arg(bundle).arg("+refs/archon/wip/*:refs/archon/wip/*"))?;
    run(repo.git().args(["update-ref", "refs/heads/main", commit]))?;
    run(repo.git().args(["reset", "-q", "--hard", "main"]))?;
    Ok(())
}

/// Where git runs for a project: its own repository, or a private git dir outside the project.
struct Repo {
    /// Private git dir for a folder that is not a repository. None: the folder's own repository.
    git_dir: Option<PathBuf>,
    /// The project folder (the work tree).
    work: PathBuf,
}

impl Repo {
    fn open(dir: &Path, snapshots: &Path) -> Result<Repo, String> {
        if succeeds(Command::new("git").arg("-C").arg(dir).args(["rev-parse", "--git-dir"])) {
            return Ok(Repo { git_dir: None, work: dir.to_path_buf() });
        }
        let canon = dir.canonicalize().map_err(|e| format!("{}: {e}", dir.display()))?;
        let name = format!("{:016x}.git", fnv1a(canon.to_string_lossy().as_bytes()));
        let git_dir = snapshots.join(name);
        if !git_dir.join("HEAD").is_file() {
            fs::create_dir_all(snapshots).map_err(|e| format!("{}: {e}", snapshots.display()))?;
            run(Command::new("git").arg("init").arg("-q").arg("--bare").arg(&git_dir))?;
        }
        Ok(Repo { git_dir: Some(git_dir), work: dir.to_path_buf() })
    }

    /// A git command for this project, run in the project folder without inherited repo variables.
    fn git(&self) -> Command {
        let mut cmd = Command::new("git");
        cmd.env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .env("GIT_TERMINAL_PROMPT", "0")
            .current_dir(&self.work);
        if let Some(g) = &self.git_dir {
            cmd.arg(format!("--git-dir={}", g.display()))
                .arg(format!("--work-tree={}", self.work.display()));
        }
        cmd
    }
}

/// A git command for `repo` that uses the temporary index at `index`.
fn indexed(repo: &Repo, index: &Path) -> Command {
    let mut cmd = repo.git();
    cmd.env("GIT_INDEX_FILE", index);
    cmd
}

/// Removes secret-looking paths from the temporary index only. The project's files and real index
/// are untouched, so the snapshot simply lacks those paths.
fn drop_secrets(repo: &Repo, index: &Path) -> Result<(), String> {
    let paths = secret_paths(repo, index)?;
    for chunk in paths.chunks(500) {
        let specs: Vec<String> = chunk.iter().map(|p| format!(":(top,literal){p}")).collect();
        run(indexed(repo, index).args(["rm", "-q", "--cached", "--ignore-unmatch", "--"]).args(&specs))?;
    }
    Ok(())
}

/// Paths in the temporary index, from the top of the repository, whose file name is a secret.
fn secret_paths(repo: &Repo, index: &Path) -> Result<Vec<String>, String> {
    let out = indexed(repo, index)
        .args(["ls-files", "-z", "--full-name", "--", ":/"])
        .output()
        .map_err(|e| format!("git ls-files: {e}"))?;
    if !out.status.success() {
        return Err(format!("git ls-files: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(out
        .stdout
        .split(|b| *b == 0)
        .filter_map(|p| std::str::from_utf8(p).ok())
        .filter(|p| is_secret(p.rsplit('/').next().unwrap_or_default()))
        .map(str::to_string)
        .collect())
}

/// Whether a file name looks like a secret: env files, private keys, certificates and credentials.
/// Example and template env files are not secrets.
fn is_secret(name: &str) -> bool {
    if matches!(name, ".env.example" | ".env.sample" | ".env.template") {
        return false;
    }
    let env = name == ".env" || name.starts_with(".env.");
    let key = [".pem", ".key", ".p12", ".pfx"].iter().any(|ext| name.ends_with(*ext));
    let ssh = matches!(name, "id_rsa" | "id_ed25519" | "id_ecdsa" | "id_dsa");
    let creds = matches!(name, ".npmrc" | ".pypirc" | ".netrc" | "credentials.json");
    let service = name.starts_with("service-account") && name.ends_with(".json");
    env || key || ssh || creds || service
}

/// Runs `cmd` and returns its trimmed stdout, or the git error with the arguments that failed.
fn run(cmd: &mut Command) -> Result<String, String> {
    let label = cmd.get_args().map(|a| a.to_string_lossy().into_owned()).collect::<Vec<_>>().join(" ");
    let out = cmd.output().map_err(|e| format!("git {label}: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        Err(format!("git {label}: {}", String::from_utf8_lossy(&out.stderr).trim()))
    }
}

/// Whether `cmd` exits successfully. Output is discarded.
fn succeeds(cmd: &mut Command) -> bool {
    cmd.stdout(Stdio::null()).stderr(Stdio::null()).status().map(|s| s.success()).unwrap_or(false)
}

/// Where private git dirs for non-repository folders live: `snapshots/` beside the data folder.
fn snapshots_dir() -> PathBuf {
    let data = store::dir();
    match data.parent() {
        Some(p) => p.join("snapshots"),
        None => data.join("snapshots"),
    }
}

/// Device ids become part of a ref name, so only plain characters are allowed.
fn check_device(device: &str) -> Result<(), String> {
    let ok = !device.is_empty() && device.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    if ok {
        Ok(())
    } else {
        Err(format!("bad device id: {device:?}"))
    }
}

/// Commit ids come from peers and reach git as arguments, so they must look like hex.
fn is_hex(s: &str) -> bool {
    (4..=64).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// FNV-1a, 64-bit. Stable across runs, so a project always maps to the same private git dir.
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// A path in the temp dir that no other call in this process or another one uses.
fn temp_path(tag: &str) -> PathBuf {
    static N: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("archonui-{tag}-{}-{nanos}-{n}", std::process::id()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git_available() -> bool {
        Command::new("git").arg("--version").output().map(|o| o.status.success()).unwrap_or(false)
    }

    /// A fresh test folder, emptied first in case an earlier run left it behind.
    fn root(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("archonui-snapshot-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    fn git(dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git").arg("-C").arg(dir).args(args).output().unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// A repository with one commit: `.gitignore` ignores `target/`, and `tracked.txt` is committed.
    fn init_repo(dir: &Path) {
        fs::create_dir_all(dir).unwrap();
        git(dir, &["init", "-q"]);
        git(dir, &["config", "user.email", "t@example.com"]);
        git(dir, &["config", "user.name", "t"]);
        fs::write(dir.join(".gitignore"), "target/\n").unwrap();
        fs::write(dir.join("tracked.txt"), "one\n").unwrap();
        git(dir, &["add", "-A"]);
        git(dir, &["commit", "-q", "-m", "init"]);
    }

    #[test]
    fn take_snapshots_without_touching_the_repo() {
        if !git_available() {
            return;
        }
        let base = root("take");
        let proj = base.join("proj");
        let snaps = base.join("snaps");
        init_repo(&proj);
        fs::write(proj.join("notes.txt"), "draft\n").unwrap();
        fs::create_dir_all(proj.join("target")).unwrap();
        fs::write(proj.join("target/x"), "built\n").unwrap();
        let head = git(&proj, &["rev-parse", "HEAD"]);
        let status_before = git(&proj, &["status", "--porcelain"]);

        let c1 = take_in(&proj, "n1", &snaps, false).unwrap().expect("first snapshot");
        assert_eq!(git(&proj, &["status", "--porcelain"]), status_before);
        assert_eq!(git(&proj, &["rev-parse", "HEAD"]), head);
        assert_eq!(take_in(&proj, "n1", &snaps, false).unwrap(), None);

        fs::write(proj.join("tracked.txt"), "two\n").unwrap();
        let c2 = take_in(&proj, "n1", &snaps, false).unwrap().expect("second snapshot");
        assert_ne!(c1, c2);
        let parent = format!("{c2}^");
        assert_eq!(git(&proj, &["rev-parse", parent.as_str()]), c1);

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn full_bundle_restores_working_files() {
        if !git_available() {
            return;
        }
        let base = root("restore");
        let proj = base.join("proj");
        let snaps = base.join("snaps");
        init_repo(&proj);
        fs::write(proj.join("notes.txt"), "draft\n").unwrap();
        fs::create_dir_all(proj.join("target")).unwrap();
        fs::write(proj.join("target/x"), "built\n").unwrap();
        take_in(&proj, "n1", &snaps, false).unwrap();
        fs::write(proj.join("tracked.txt"), "two\n").unwrap();
        let c2 = take_in(&proj, "n1", &snaps, false).unwrap().expect("second snapshot");

        let full = bundle_in(&proj, "n1", None, &snaps).unwrap();
        assert!(!full.is_empty());
        let dest = base.join("restored");
        restore(&full, &c2, &dest).unwrap();

        assert_eq!(fs::read_to_string(dest.join("notes.txt")).unwrap(), "draft\n");
        assert_eq!(fs::read_to_string(dest.join("tracked.txt")).unwrap(), "two\n");
        assert!(!dest.join("target").exists());
        assert_eq!(git(&dest, &["rev-parse", "HEAD"]), c2);
        assert_eq!(git(&dest, &["rev-parse", "--abbrev-ref", "HEAD"]), "main");

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn incremental_bundles_are_small_and_empty_when_current() {
        if !git_available() {
            return;
        }
        let base = root("incremental");
        let proj = base.join("proj");
        let snaps = base.join("snaps");
        init_repo(&proj);
        let c1 = take_in(&proj, "n1", &snaps, false).unwrap().expect("first snapshot");
        fs::write(proj.join("tracked.txt"), "two\n").unwrap();
        let c2 = take_in(&proj, "n1", &snaps, false).unwrap().expect("second snapshot");

        let full = bundle_in(&proj, "n1", None, &snaps).unwrap();
        let inc = bundle_in(&proj, "n1", Some(c1.as_str()), &snaps).unwrap();
        assert!(!inc.is_empty());
        assert!(inc.len() <= full.len());
        assert!(bundle_in(&proj, "n1", Some(c2.as_str()), &snaps).unwrap().is_empty());
        // An id this repository does not know gives a full bundle.
        assert!(!bundle_in(&proj, "n1", Some("abcdef0123456789"), &snaps).unwrap().is_empty());

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn plain_folder_snapshot_leaves_no_git_dir() {
        if !git_available() {
            return;
        }
        let base = root("plain");
        let folder = base.join("folder");
        let snaps = base.join("snaps");
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("a.txt"), "a\n").unwrap();
        fs::write(folder.join("skip.log"), "log\n").unwrap();
        fs::write(folder.join(".gitignore"), "*.log\n").unwrap();

        let c = take_in(&folder, "n1", &snaps, false).unwrap().expect("snapshot");
        assert!(!folder.join(".git").exists());
        assert_eq!(take_in(&folder, "n1", &snaps, false).unwrap(), None);

        let full = bundle_in(&folder, "n1", None, &snaps).unwrap();
        let dest = base.join("restored");
        restore(&full, &c, &dest).unwrap();
        assert!(dest.join("a.txt").exists());
        assert!(!dest.join("skip.log").exists());

        let _ = fs::remove_dir_all(&base);
    }

    /// Adds secret-looking files, and look-alikes that are not secrets, to `dir`.
    fn add_secret_files(dir: &Path) {
        fs::write(dir.join(".env"), "KEY=1\n").unwrap();
        fs::write(dir.join(".env.example"), "KEY=\n").unwrap();
        fs::create_dir_all(dir.join("keys")).unwrap();
        fs::write(dir.join("keys/server.pem"), "pem\n").unwrap();
        fs::write(dir.join("id_rsa.pub"), "pub\n").unwrap();
    }

    fn snapshot_files(dir: &Path, commit: &str) -> Vec<String> {
        git(dir, &["ls-tree", "-r", "--name-only", commit]).lines().map(str::to_string).collect()
    }

    #[test]
    fn secret_files_stay_out_by_default() {
        if !git_available() {
            return;
        }
        let base = root("secrets");
        let proj = base.join("proj");
        let snaps = base.join("snaps");
        init_repo(&proj);
        add_secret_files(&proj);
        let status_before = git(&proj, &["status", "--porcelain"]);

        let c = take_in(&proj, "n1", &snaps, false).unwrap().expect("snapshot");
        let files = snapshot_files(&proj, &c);
        assert!(files.contains(&"tracked.txt".to_string()));
        assert!(files.contains(&".env.example".to_string()));
        assert!(files.contains(&"id_rsa.pub".to_string()));
        assert!(!files.contains(&".env".to_string()));
        assert!(!files.contains(&"keys/server.pem".to_string()));
        assert_eq!(git(&proj, &["status", "--porcelain"]), status_before);
        assert!(proj.join(".env").exists());

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn secret_files_are_copied_when_asked_for() {
        if !git_available() {
            return;
        }
        let base = root("secrets-on");
        let proj = base.join("proj");
        let snaps = base.join("snaps");
        init_repo(&proj);
        add_secret_files(&proj);

        let c = take_in(&proj, "n1", &snaps, true).unwrap().expect("snapshot");
        let files = snapshot_files(&proj, &c);
        assert!(files.contains(&".env".to_string()));
        assert!(files.contains(&"keys/server.pem".to_string()));

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn secret_names() {
        for name in [".env", ".env.local", "server.pem", "api.key", "cert.p12", "cert.pfx", "id_rsa", "id_ed25519", "id_ecdsa", "id_dsa", ".npmrc", ".pypirc", ".netrc", "credentials.json", "service-account-prod.json"] {
            assert!(is_secret(name), "{name}");
        }
        for name in [".env.example", ".env.sample", ".env.template", "environment.txt", "id_rsa.pub", "main.rs", "service-account.txt", "keys.json", "credentials.toml"] {
            assert!(!is_secret(name), "{name}");
        }
    }

    #[test]
    fn restore_refuses_a_non_empty_dest() {
        if !git_available() {
            return;
        }
        let base = root("nonempty");
        let dest = base.join("dest");
        fs::create_dir_all(&dest).unwrap();
        fs::write(dest.join("keep.txt"), "mine\n").unwrap();

        let err = restore(b"", "abcdef0123456789", &dest).unwrap_err();
        assert!(err.contains("not empty"), "{err}");
        assert!(dest.join("keep.txt").exists());

        let _ = fs::remove_dir_all(&base);
    }
}
