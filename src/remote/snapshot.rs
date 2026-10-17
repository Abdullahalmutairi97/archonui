//! Snapshots of a project's working files for the cloud copy: tracked files plus untracked files
//! not ignored by .gitignore, kept as commits on a private ref. HEAD, the index, branches and the
//! files themselves are never changed.
//!
//! Folders that are not git repositories get a private git dir under the data folder instead,
//! so nothing is written into the project. A snapshot never holds files above the project folder
//! (see `Repo::open`).

use std::ffi::OsString;
use std::fs;
use std::os::unix::ffi::OsStringExt;
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
/// On failure a `dest` made here is removed again, so no half-written copy is left behind.
pub fn restore(bundle: &[u8], commit: &str, dest: &Path) -> Result<(), String> {
    if !is_hex(commit) {
        return Err(format!("not a commit id: {commit:?}"));
    }
    let created = !dest.exists();
    if created {
        fs::create_dir_all(dest).map_err(|e| format!("{}: {e}", dest.display()))?;
    } else {
        let mut entries = fs::read_dir(dest).map_err(|e| format!("{}: {e}", dest.display()))?;
        if entries.next().is_some() {
            return Err(format!("{} is not empty", dest.display()));
        }
    }
    let result = temp_path("restore").and_then(|file| {
        let result = fs::write(&file, bundle).map_err(|e| format!("write bundle: {e}")).and_then(|_| fetch_into(dest, &file, commit));
        let _ = fs::remove_file(&file);
        result
    });
    if result.is_err() && created {
        let _ = fs::remove_dir_all(dest);
    }
    result
}

fn take_in(dir: &Path, device: &str, snapshots: &Path, secrets: bool) -> Result<Option<String>, String> {
    check_device(device)?;
    let repo = Repo::open(dir, snapshots)?;
    let index = temp_path("index")?;
    let result = take_with(&repo, device, &index, secrets);
    let _ = fs::remove_file(&index);
    result
}

/// The snapshot itself, built in a temporary index so the real one is never touched. The temporary
/// index starts as a copy of the real one, so git skips files whose stats have not changed instead
/// of reading every file again.
fn take_with(repo: &Repo, device: &str, index: &Path, secrets: bool) -> Result<Option<String>, String> {
    let r = wip_ref(device);
    let has_head = succeeds(repo.git().args(["rev-parse", "--verify", "-q", "HEAD"]));
    let seed = repo.index_path()?;
    let seeded = match copy_index(&seed, index) {
        Ok(_) => true,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
        Err(e) => return Err(format!("copy {}: {e}", seed.display())),
    };
    if !seeded && has_head {
        run(indexed(repo, index).args(["read-tree", "HEAD"]))?;
    }
    match &repo.prefix {
        None => run(indexed(repo, index).args(["add", "-A"]))?,
        // Git runs in the project folder, so "." is just the subfolder.
        Some(_) => run(indexed(repo, index).args(["add", "-A", "--", "."]))?,
    };
    if repo.git_dir.is_some() {
        // A private git dir's index is ours: keep it so the next snapshot can skip unchanged files.
        // Only a speed-up, so a failed save is not an error.
        let _ = save_index(index, &seed);
    }
    if !secrets {
        drop_secrets(repo, index)?;
    }
    let tree = write_tree(repo, index)?;

    let prev = run(repo.git().args(["rev-parse", "--verify", "-q", r.as_str()])).ok();
    if let Some(p) = &prev {
        let spec = format!("{p}^{{tree}}");
        if run(repo.git().args(["rev-parse", spec.as_str()]))? == tree {
            return Ok(None);
        }
    }
    // A subfolder's snapshot starts its own history: HEAD's would carry the whole repository.
    let parent = match prev {
        Some(p) => Some(p),
        None if has_head && repo.prefix.is_none() => Some(run(repo.git().args(["rev-parse", "HEAD"]))?),
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

/// The tree of the temporary index: the whole of it, or just the project's subfolder.
fn write_tree(repo: &Repo, index: &Path) -> Result<String, String> {
    let Some(prefix) = &repo.prefix else { return run(indexed(repo, index).args(["write-tree"])) };
    let mut arg = OsString::from("--prefix=");
    arg.push(prefix);
    match run(indexed(repo, index).arg("write-tree").arg(&arg)) {
        Ok(tree) => Ok(tree),
        // A subfolder with nothing to keep has no tree of its own in the index.
        Err(e) if run(indexed(repo, index).args(["ls-files", "--", "."]))?.is_empty() => {
            run(repo.git().arg("mktree")).map_err(|_| e)
        }
        Err(e) => Err(e),
    }
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

    let file = temp_path("bundle")?;
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
    let repo = Repo { git_dir: None, work: dest.to_path_buf(), prefix: None };
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
    /// The project folder's path in its repository, ending in `/`, when it is a subfolder of one.
    prefix: Option<OsString>,
}

impl Repo {
    /// Snapshots hold the project folder and nothing above it:
    /// - the top of a repository: that repository's files.
    /// - a subfolder of a repository: only that subfolder's files, as the snapshot's root, with no
    ///   history from the repository (a copy restores as just the subfolder). The repository's
    ///   ignore rules still apply.
    /// - a folder its enclosing repository ignores (a home folder kept as a dotfiles repository
    ///   that ignores `*`, say), or one in no repository: a private git dir outside the folder, so
    ///   nothing is written to the enclosing repository.
    fn open(dir: &Path, snapshots: &Path) -> Result<Repo, String> {
        let found = git_cmd().arg("-C").arg(dir).args(["rev-parse", "--show-prefix"]).stderr(Stdio::null()).output();
        if let Some(out) = found.ok().filter(|o| o.status.success()) {
            let mut prefix = out.stdout;
            if prefix.last() == Some(&b'\n') {
                prefix.pop();
            }
            if prefix.is_empty() {
                return Ok(Repo { git_dir: None, work: dir.to_path_buf(), prefix: None });
            }
            if !succeeds(git_cmd().arg("-C").arg(dir).args(["check-ignore", "-q", "."])) {
                let prefix = Some(OsString::from_vec(prefix));
                return Ok(Repo { git_dir: None, work: dir.to_path_buf(), prefix });
            }
        }
        let canon = dir.canonicalize().map_err(|e| format!("{}: {e}", dir.display()))?;
        let name = format!("{:016x}.git", fnv1a(canon.to_string_lossy().as_bytes()));
        let git_dir = snapshots.join(name);
        if !git_dir.join("HEAD").is_file() {
            fs::create_dir_all(snapshots).map_err(|e| format!("{}: {e}", snapshots.display()))?;
            run(git_cmd().arg("init").arg("-q").arg("--bare").arg(&git_dir))?;
        }
        Ok(Repo { git_dir: Some(git_dir), work: dir.to_path_buf(), prefix: None })
    }

    /// A git command for this project, run in the project folder without inherited repo variables.
    fn git(&self) -> Command {
        let mut cmd = git_cmd();
        cmd.current_dir(&self.work);
        if let Some(g) = &self.git_dir {
            cmd.arg(format!("--git-dir={}", g.display()))
                .arg(format!("--work-tree={}", self.work.display()));
        }
        cmd
    }

    /// The index the snapshot starts from: the repository's own (only ever copied), or the one
    /// kept in the private git dir. It may not exist.
    fn index_path(&self) -> Result<PathBuf, String> {
        if let Some(g) = &self.git_dir {
            return Ok(g.join("index"));
        }
        let p = PathBuf::from(run(self.git().args(["rev-parse", "--git-path", "index"]))?);
        Ok(if p.is_absolute() { p } else { self.work.join(p) })
    }
}

/// Variables an outer git (a hook, or `GIT_DIR=… archonui`) passes down that point git at another
/// repository or change what it reads: git's own `rev-parse --local-env-vars`, plus the quarantine
/// path a receive hook sets, under which ref updates are refused.
const REPO_ENV: &[&str] = &[
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_CONFIG",
    "GIT_CONFIG_PARAMETERS",
    "GIT_CONFIG_COUNT",
    "GIT_OBJECT_DIRECTORY",
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_IMPLICIT_WORK_TREE",
    "GIT_GRAFT_FILE",
    "GIT_INDEX_FILE",
    "GIT_NO_REPLACE_OBJECTS",
    "GIT_REPLACE_REF_BASE",
    "GIT_PREFIX",
    "GIT_SHALLOW_FILE",
    "GIT_COMMON_DIR",
    "GIT_QUARANTINE_PATH",
];

/// `git` without the inherited variables that pick a repository, and without password prompts.
/// Code that wants a temporary index sets `GIT_INDEX_FILE` again afterwards.
pub(super) fn git_cmd() -> Command {
    let mut cmd = Command::new("git");
    for var in REPO_ENV {
        cmd.env_remove(var);
    }
    cmd.env("GIT_TERMINAL_PROMPT", "0");
    cmd
}

/// A git command for `repo` that uses the temporary index at `index`. A split index is written
/// whole, so nothing new lands in the repository's git dir.
fn indexed(repo: &Repo, index: &Path) -> Command {
    let mut cmd = repo.git();
    cmd.arg("-c").arg("core.splitIndex=false").env("GIT_INDEX_FILE", index);
    cmd
}

/// Copies the temporary index to `to` through a temp file beside it, so a reader never sees half.
fn save_index(index: &Path, to: &Path) -> std::io::Result<()> {
    let tmp = to.with_extension("archonui-tmp");
    copy_index(index, &tmp)?;
    fs::rename(&tmp, to)
}

/// Copies an index file and keeps its modified time. Git compares each entry's file time with the
/// index file's own time to spot files changed in the same instant the index was written; a copy
/// with a fresh time would hide those changes.
fn copy_index(from: &Path, to: &Path) -> std::io::Result<()> {
    // Read first: an index replaced meanwhile then only looks older, which makes git check more.
    let modified = fs::metadata(from)?.modified()?;
    fs::copy(from, to)?;
    fs::OpenOptions::new().write(true).open(to)?.set_modified(modified)
}

/// Removes secret-looking paths from the temporary index only. The project's files and real index
/// are untouched, so the snapshot simply lacks those paths. Paths reach git as raw bytes through a
/// pathspec file, so names that are not UTF-8 are removed too.
fn drop_secrets(repo: &Repo, index: &Path) -> Result<(), String> {
    let paths = secret_paths(repo, index)?;
    if paths.is_empty() {
        return Ok(());
    }
    let mut specs = Vec::new();
    for p in &paths {
        specs.extend_from_slice(b":(top,literal)");
        specs.extend_from_slice(p);
        specs.push(0);
    }
    let list = temp_path("secrets")?;
    fs::write(&list, &specs).map_err(|e| format!("write {}: {e}", list.display()))?;
    let result = run(
        indexed(repo, index)
            .args(["rm", "-q", "--cached", "--ignore-unmatch", "--pathspec-file-nul"])
            .arg(format!("--pathspec-from-file={}", list.display())),
    );
    let _ = fs::remove_file(&list);
    result.map(|_| ())
}

/// Paths in the temporary index, from the top of the repository, whose file name is a secret.
fn secret_paths(repo: &Repo, index: &Path) -> Result<Vec<Vec<u8>>, String> {
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
        .filter(|p| {
            let name = p.rsplit(|b| *b == b'/').next().unwrap_or_default();
            !p.is_empty() && is_secret(&String::from_utf8_lossy(name))
        })
        .map(<[u8]>::to_vec)
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

/// A path no other call in this process or another one uses, in an owner-only folder beside the
/// snapshots. Not the shared temp dir: a bundle holds the project's files, uncommitted work and all,
/// and is written readable by everyone.
fn temp_path(tag: &str) -> Result<PathBuf, String> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    static N: AtomicU64 = AtomicU64::new(0);
    let dir = snapshots_dir().join("tmp");
    let failed = |e: std::io::Error| format!("{}: {e}", dir.display());
    fs::DirBuilder::new().recursive(true).mode(0o700).create(&dir).map_err(failed)?;
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).map_err(failed)?;
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    Ok(dir.join(format!("archonui-{tag}-{}-{nanos}-{n}", std::process::id())))
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

    /// Everything the user can see in git: index bytes, HEAD, refs other than snapshots, status.
    fn repo_state(dir: &Path) -> (Vec<u8>, Vec<u8>, String, String) {
        let refs = git(dir, &["for-each-ref", "--format=%(refname) %(objectname)"])
            .lines()
            .filter(|l| !l.starts_with("refs/archon/"))
            .collect::<Vec<_>>()
            .join("\n");
        (
            fs::read(dir.join(".git/index")).unwrap(),
            fs::read(dir.join(".git/HEAD")).unwrap(),
            refs,
            // No optional locks: status would otherwise refresh the real index itself.
            git(dir, &["--no-optional-locks", "status", "--porcelain=v1", "-z", "--untracked-files=all"]),
        )
    }

    #[test]
    fn dirty_detached_repo_is_left_byte_identical() {
        if !git_available() {
            return;
        }
        let base = root("dirty");
        let proj = base.join("proj");
        let snaps = base.join("snaps");
        init_repo(&proj);
        fs::write(proj.join("gone.txt"), "x\n").unwrap();
        git(&proj, &["add", "gone.txt"]);
        git(&proj, &["commit", "-q", "-m", "two"]);
        git(&proj, &["checkout", "-q", "--detach"]);
        fs::write(proj.join("staged.txt"), "staged\n").unwrap();
        git(&proj, &["add", "staged.txt"]);
        fs::write(proj.join("tracked.txt"), "edited\n").unwrap();
        fs::remove_file(proj.join("gone.txt")).unwrap();
        fs::write(proj.join("with space é.txt"), "u\n").unwrap();
        fs::write(proj.join("new\nline.txt"), "n\n").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink("tracked.txt", proj.join("link")).unwrap();
        let before = repo_state(&proj);

        let c = take_in(&proj, "n1", &snaps, false).unwrap().expect("snapshot");
        assert_eq!(repo_state(&proj), before);
        let files = snapshot_files(&proj, &c);
        for f in ["staged.txt", "tracked.txt", "\"new\\nline.txt\""] {
            assert!(files.iter().any(|x| x == f), "{f} in {files:?}");
        }
        assert!(files.iter().any(|x| x.starts_with("\"with space")), "{files:?}");
        assert!(!files.iter().any(|x| x == "gone.txt"), "{files:?}");
        let spec = format!("{c}:tracked.txt");
        assert_eq!(git(&proj, &["cat-file", "-p", spec.as_str()]), "edited");

        let _ = fs::remove_file(proj.join("with space é.txt"));
        assert!(take_in(&proj, "n1", &snaps, false).unwrap().is_some());
        assert_eq!(repo_state(&proj).0, before.0);

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn repo_without_commits_is_left_alone() {
        if !git_available() {
            return;
        }
        let base = root("unborn");
        let proj = base.join("proj");
        let snaps = base.join("snaps");
        fs::create_dir_all(&proj).unwrap();
        git(&proj, &["init", "-q"]);
        fs::write(proj.join("a.txt"), "a\n").unwrap();
        let status = git(&proj, &["--no-optional-locks", "status", "--porcelain"]);
        let head = fs::read(proj.join(".git/HEAD")).unwrap();

        let c = take_in(&proj, "n1", &snaps, false).unwrap().expect("snapshot");
        assert_eq!(snapshot_files(&proj, &c), vec!["a.txt".to_string()]);
        assert!(!proj.join(".git/index").exists());
        assert_eq!(fs::read(proj.join(".git/HEAD")).unwrap(), head);
        assert_eq!(git(&proj, &["--no-optional-locks", "status", "--porcelain"]), status);

        let _ = fs::remove_dir_all(&base);
    }

    /// Unchanged tracked files are not read again on each snapshot: the real index's file stats are
    /// reused (from a copy). A counting clean filter shows how many files git read.
    #[test]
    fn unchanged_files_are_not_hashed_again() {
        if !git_available() {
            return;
        }
        let base = root("rehash");
        let proj = base.join("proj");
        let snaps = base.join("snaps");
        init_repo(&proj);
        let log = base.join("clean.log");
        let filter = format!("sh -c 'echo x >> \"{}\"; cat'", log.display());
        git(&proj, &["config", "filter.count.clean", filter.as_str()]);
        fs::write(proj.join(".gitattributes"), "*.txt filter=count\n").unwrap();
        for i in 0..20 {
            fs::write(proj.join(format!("f{i}.txt")), format!("{i}\n")).unwrap();
        }
        git(&proj, &["add", "-A"]);
        git(&proj, &["commit", "-q", "-m", "files"]);
        // Past the racy-git window, so the index's stats are trusted.
        std::thread::sleep(std::time::Duration::from_millis(1100));
        git(&proj, &["update-index", "-q", "--really-refresh"]);
        let index = fs::read(proj.join(".git/index")).unwrap();
        let runs = || fs::read_to_string(&log).map(|s| s.lines().count()).unwrap_or(0);
        let _ = fs::remove_file(&log);

        take_in(&proj, "n1", &snaps, false).unwrap().expect("snapshot");
        assert_eq!(runs(), 0);
        fs::write(proj.join("f3.txt"), "changed\n").unwrap();
        take_in(&proj, "n1", &snaps, false).unwrap().expect("second snapshot");
        // Only the changed file is read (git may run the filter on it twice), not all 21.
        assert!(runs() <= 2, "{} files read again", runs());
        assert_eq!(fs::read(proj.join(".git/index")).unwrap(), index);

        let _ = fs::remove_dir_all(&base);
    }

    /// A file changed in the same instant the index was written, keeping its size and time, is
    /// still seen as changed: the copied index keeps the real one's time, so git checks it.
    #[test]
    fn same_instant_changes_are_not_missed() {
        if !git_available() {
            return;
        }
        let base = root("racy");
        let proj = base.join("proj");
        let snaps = base.join("snaps");
        init_repo(&proj);
        // A rewrite in the same instant also keeps the change time; tests cannot set that, so git
        // is told to ignore it.
        git(&proj, &["config", "core.trustctime", "false"]);
        let file = proj.join("tracked.txt");
        std::thread::sleep(std::time::Duration::from_millis(1100));
        git(&proj, &["update-index", "-q", "--really-refresh"]);
        let when = fs::metadata(&file).unwrap().modified().unwrap();
        let set_time = |p: &Path| fs::OpenOptions::new().write(true).open(p).unwrap().set_modified(when).unwrap();
        set_time(&proj.join(".git/index"));
        take_in(&proj, "n1", &snaps, false).unwrap().expect("first snapshot");

        fs::write(&file, "two\n").unwrap();
        set_time(&file);
        let c = take_in(&proj, "n1", &snaps, false).unwrap().expect("the change is seen");
        let spec = format!("{c}:tracked.txt");
        assert_eq!(git(&proj, &["cat-file", "-p", spec.as_str()]), "two");

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn plain_folders_skip_unchanged_files_too() {
        if !git_available() {
            return;
        }
        let base = root("rehash-plain");
        let folder = base.join("folder");
        let snaps = base.join("snaps");
        fs::create_dir_all(&folder).unwrap();
        for i in 0..20 {
            fs::write(folder.join(format!("f{i}.txt")), format!("{i}\n")).unwrap();
        }
        // Past the racy-git window, so the saved index's stats are trusted.
        std::thread::sleep(std::time::Duration::from_millis(1100));
        take_in(&folder, "n1", &snaps, false).unwrap().expect("first snapshot");
        let private = fs::read_dir(&snaps).unwrap().flatten().next().unwrap().path();
        let log = base.join("clean.log");
        let filter = format!("sh -c 'echo x >> \"{}\"; cat'", log.display());
        git(&private, &["config", "filter.count.clean", filter.as_str()]);
        fs::write(private.join("info/attributes"), "*.txt filter=count\n").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1100));
        let runs = || fs::read_to_string(&log).map(|s| s.lines().count()).unwrap_or(0);

        fs::write(folder.join("f3.txt"), "changed\n").unwrap();
        take_in(&folder, "n1", &snaps, false).unwrap().expect("second snapshot");
        assert!(runs() <= 2, "{} files read again", runs());
        assert!(!folder.join(".git").exists());

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

    #[cfg(unix)]
    #[test]
    fn secret_files_stay_out_under_non_utf8_folders() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;
        if !git_available() {
            return;
        }
        let base = root("secrets-bytes");
        let proj = base.join("proj");
        let snaps = base.join("snaps");
        init_repo(&proj);
        let odd = proj.join(OsStr::from_bytes(b"caf\xe9 dir"));
        fs::create_dir_all(&odd).unwrap();
        fs::write(odd.join(".env"), "KEY=1\n").unwrap();
        fs::write(odd.join("keep.txt"), "keep\n").unwrap();

        let c = take_in(&proj, "n1", &snaps, false).unwrap().expect("snapshot");
        let out = Command::new("git").arg("-C").arg(&proj).args(["ls-tree", "-r", "-z", "--name-only", c.as_str()]).output().unwrap();
        let files: Vec<&[u8]> = out.stdout.split(|b| *b == 0).filter(|p| !p.is_empty()).collect();
        assert!(files.contains(&&b"caf\xe9 dir/keep.txt"[..]), "{files:?}");
        assert!(!files.contains(&&b"caf\xe9 dir/.env"[..]), "{files:?}");
        assert!(odd.join(".env").exists());

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
    fn failed_restore_leaves_no_folder_behind() {
        if !git_available() {
            return;
        }
        let base = root("failed-restore");
        let proj = base.join("proj");
        let snaps = base.join("snaps");
        init_repo(&proj);
        let c = take_in(&proj, "n1", &snaps, false).unwrap().expect("snapshot");
        let dest = base.join("dest");

        assert!(restore(b"not a bundle", &c, &dest).is_err());
        assert!(!dest.exists());

        let full = bundle_in(&proj, "n1", None, &snaps).unwrap();
        restore(&full, &c, &dest).unwrap();
        assert_eq!(fs::read_to_string(dest.join("tracked.txt")).unwrap(), "one\n");

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn temp_files_stay_in_an_owner_only_folder() {
        use std::os::unix::fs::PermissionsExt;
        let file = temp_path("bundle").unwrap();
        let dir = file.parent().unwrap();
        assert_eq!(dir, snapshots_dir().join("tmp"), "not the shared temp dir");
        assert_eq!(fs::metadata(dir).unwrap().permissions().mode() & 0o777, 0o700);
    }

    #[test]
    fn a_subfolder_of_a_repo_snapshots_only_itself() {
        if !git_available() {
            return;
        }
        let base = root("subfolder");
        let repo = base.join("repo");
        let snaps = base.join("snaps");
        init_repo(&repo);
        fs::write(repo.join(".gitignore"), "target/\n*.log\n").unwrap();
        let sub = repo.join("packages/app");
        fs::create_dir_all(sub.join("src")).unwrap();
        fs::write(sub.join("src/main.rs"), "fn main() {}\n").unwrap();
        git(&repo, &["add", "-A"]);
        git(&repo, &["commit", "-q", "-m", "app"]);
        fs::write(sub.join("draft.txt"), "draft\n").unwrap();
        fs::write(sub.join("build.log"), "log\n").unwrap();
        fs::write(repo.join("other.txt"), "not the project\n").unwrap();
        let before = repo_state(&repo);

        let c = take_in(&sub, "n1", &snaps, false).unwrap().expect("snapshot");
        assert_eq!(repo_state(&repo), before);
        assert_eq!(snapshot_files(&repo, &c), vec!["draft.txt".to_string(), "src/main.rs".to_string()]);
        let parent = format!("{c}^");
        assert!(Command::new("git").arg("-C").arg(&repo).args(["rev-parse", "-q", "--verify", parent.as_str()]).output().map(|o| !o.status.success()).unwrap());
        assert_eq!(take_in(&sub, "n1", &snaps, false).unwrap(), None);

        let full = bundle_in(&sub, "n1", None, &snaps).unwrap();
        let dest = base.join("restored");
        restore(&full, &c, &dest).unwrap();
        let mut files: Vec<String> = git(&dest, &["ls-files"]).lines().map(str::to_string).collect();
        files.sort();
        assert_eq!(files, vec!["draft.txt".to_string(), "src/main.rs".to_string()]);
        assert_eq!(fs::read_to_string(dest.join("draft.txt")).unwrap(), "draft\n");
        assert!(!dest.join("tracked.txt").exists() && !dest.join("packages").exists());
        // Only the subfolder's history travels, not the repository's.
        assert_eq!(git(&dest, &["rev-list", "--count", "HEAD"]), "1");

        let empty = repo.join("empty");
        fs::create_dir_all(&empty).unwrap();
        let e = take_in(&empty, "n1", &snaps, false).unwrap().expect("empty snapshot");
        assert!(snapshot_files(&repo, &e).is_empty());

        let _ = fs::remove_dir_all(&base);
    }

    /// A home folder kept as a dotfiles repository that ignores `*`: a project inside it is not
    /// part of that repository, so it gets a private git dir and its files are all kept.
    #[test]
    fn a_folder_ignored_by_its_enclosing_repo_gets_a_private_git_dir() {
        if !git_available() {
            return;
        }
        let base = root("dotfiles");
        let home = base.join("home");
        let snaps = base.join("snaps");
        fs::create_dir_all(&home).unwrap();
        git(&home, &["init", "-q"]);
        git(&home, &["config", "user.email", "t@example.com"]);
        git(&home, &["config", "user.name", "t"]);
        fs::write(home.join(".gitignore"), "*\n").unwrap();
        fs::write(home.join(".bashrc"), "x\n").unwrap();
        git(&home, &["add", "-f", ".gitignore", ".bashrc"]);
        git(&home, &["commit", "-q", "-m", "dotfiles"]);
        let proj = home.join("projects/app");
        fs::create_dir_all(&proj).unwrap();
        fs::write(proj.join("main.rs"), "fn main() {}\n").unwrap();
        let before = repo_state(&home);

        let c = take_in(&proj, "n1", &snaps, false).unwrap().expect("snapshot");
        assert_eq!(repo_state(&home), before);
        assert_eq!(git(&home, &["for-each-ref", "refs/archon/"]), "");
        assert!(!proj.join(".git").exists());
        let full = bundle_in(&proj, "n1", None, &snaps).unwrap();
        let dest = base.join("restored");
        restore(&full, &c, &dest).unwrap();
        assert_eq!(git(&dest, &["ls-files"]), "main.rs");

        let _ = fs::remove_dir_all(&base);
    }

    /// Set by `inherited_git_variables_are_ignored` for the child run of `git_env_child`.
    const CHILD_BASE: &str = "ARCHONUI_TEST_GIT_ENV_BASE";

    /// Run as a child process with `GIT_DIR` and friends pointing at another repository: snapshots,
    /// bundles and the cloud store must still use the project's own. Does nothing when run directly.
    #[test]
    fn git_env_child() {
        let Some(base) = std::env::var_os(CHILD_BASE).map(PathBuf::from) else { return };
        let proj = base.join("proj");
        let snaps = base.join("snaps");
        let c = take_in(&proj, "n1", &snaps, false).unwrap().expect("snapshot");
        let full = bundle_in(&proj, "n1", None, &snaps).unwrap();
        let store = super::super::cloud::Store::open(base.join("cloud"));
        store.put_project("p1", "proj", "n1", &c, &full).unwrap();
        assert_eq!(store.get_project("p1", "n1").unwrap().0, c);
        restore(&full, &c, &base.join("restored")).unwrap();
    }

    #[test]
    fn inherited_git_variables_are_ignored() {
        if !git_available() {
            return;
        }
        let base = root("git-env");
        let proj = base.join("proj");
        let other = base.join("other");
        init_repo(&proj);
        init_repo(&other);
        fs::write(proj.join("notes.txt"), "draft\n").unwrap();
        fs::write(other.join("theirs.txt"), "x\n").unwrap();
        let other_index = fs::read(other.join(".git/index")).unwrap();

        let out = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "remote::snapshot::tests::git_env_child", "--test-threads=1"])
            .env(CHILD_BASE, &base)
            .env("GIT_DIR", other.join(".git"))
            .env("GIT_WORK_TREE", &other)
            .env("GIT_INDEX_FILE", other.join(".git/index"))
            .env("GIT_OBJECT_DIRECTORY", other.join(".git/objects"))
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(out.status.success(), "{text}{}", String::from_utf8_lossy(&out.stderr));
        assert!(text.contains("1 passed"), "the child test ran: {text}");

        assert!(!git(&proj, &["for-each-ref", "refs/archon/"]).is_empty());
        assert_eq!(git(&other, &["for-each-ref", "refs/archon/"]), "");
        assert_eq!(fs::read(other.join(".git/index")).unwrap(), other_index);
        assert_eq!(fs::read_to_string(base.join("restored/notes.txt")).unwrap(), "draft\n");
        assert!(!base.join("restored/theirs.txt").exists());

        let _ = fs::remove_dir_all(&base);
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
