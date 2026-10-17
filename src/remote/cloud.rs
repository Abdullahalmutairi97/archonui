//! The cloud copy kept on the device the user marked as cloud: project snapshots (one bare git
//! repository per project) and each device's chats (JSON files).
//!
//! Layout under the store root: `projects/<project>.git` holds the snapshots as refs
//! `refs/archon/wip/<device>`, `projects/<project>.json` says which device sent what, and
//! `chats/<device>/<session>.json` is one chat each. Ids come from the network, so `safe_id`
//! checks every one before it becomes part of a path.

use super::proto::CloudProject;
use serde_json::{json, Value};
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

/// Where the cloud copy lives.
#[derive(Clone, Debug)]
pub struct Store {
    root: PathBuf,
}

impl Store {
    /// The store under `store::dir().parent()/cloud`.
    pub fn open_default() -> Store {
        let dir = crate::store::dir();
        let base = dir.parent().unwrap_or(dir.as_path()).to_path_buf();
        Store::open(base.join("cloud"))
    }

    /// The store at `root`. A relative root is made absolute, because git runs inside each repository.
    pub fn open(root: PathBuf) -> Store {
        let root = if root.is_absolute() { root } else { std::env::current_dir().map(|cwd| cwd.join(&root)).unwrap_or(root) };
        Store { root }
    }

    /// Adds a bundle for `project` from `device`; afterwards `refs/archon/wip/<device>` points at `commit`.
    /// An empty bundle means nothing is new: the ref must already be at `commit`.
    pub fn put_project(&self, project: &str, name: &str, device: &str, commit: &str, bundle: &[u8]) -> Result<(), String> {
        check_id(project, "project")?;
        check_id(device, "device")?;
        self.make_private()?;
        let repo = self.repo(project);
        if !repo.join("HEAD").exists() {
            let projects = self.root.join("projects");
            fs::create_dir_all(&projects).map_err(|e| format!("could not create {}: {e}", projects.display()))?;
            let out = Command::new("git")
                .args(["init", "--bare", "-q"])
                .arg(&repo)
                .env("GIT_TERMINAL_PROMPT", "0")
                .output()
                .map_err(|e| format!("git init: {e}"))?;
            if !out.status.success() {
                return Err(format!("git init: {}", String::from_utf8_lossy(&out.stderr).trim()));
            }
        }
        let refname = wip_ref(device);
        if !bundle.is_empty() {
            let file = tmp_path(&self.root, "bundle");
            fs::write(&file, bundle).map_err(|e| format!("could not write the bundle: {e}"))?;
            let spec = format!("+{refname}:{refname}");
            let fetched = match file.to_str() {
                Some(path) => git(&repo, &["fetch", "-q", path, spec.as_str()]).map(|_| ()),
                None => Err("bad temp path".to_string()),
            };
            let _ = fs::remove_file(&file);
            fetched?;
        }
        let got = git(&repo, &["rev-parse", "--verify", refname.as_str()])?;
        if got.as_str() != commit {
            return Err(format!("{refname} is at {got}, not {commit}"));
        }
        let meta_path = self.root.join("projects").join(format!("{project}.json"));
        let mut meta = read_meta(&meta_path);
        let mut devices = meta.get("devices").and_then(Value::as_object).cloned().unwrap_or_default();
        devices.insert(device.to_string(), json!({ "commit": commit, "updated": now_secs() }));
        meta.insert("name".to_string(), json!(name));
        meta.insert("devices".to_string(), Value::Object(devices));
        let bytes = serde_json::to_vec(&Value::Object(meta)).map_err(|e| e.to_string())?;
        write_atomic(&meta_path, &bytes)
    }

    /// Every stored snapshot, one per project and device, newest first.
    pub fn projects(&self) -> Vec<CloudProject> {
        let mut out = Vec::new();
        let Ok(entries) = fs::read_dir(self.root.join("projects")) else { return out };
        for entry in entries.flatten() {
            let path = entry.path();
            if !is_json(&path) {
                continue;
            }
            let Some(project) = path.file_stem().and_then(|s| s.to_str()) else { continue };
            if !safe_id(project) {
                continue;
            }
            let meta = read_meta(&path);
            let name = meta.get("name").and_then(Value::as_str).unwrap_or("").to_string();
            let Some(devices) = meta.get("devices").and_then(Value::as_object) else { continue };
            for (device, info) in devices {
                if !safe_id(device) {
                    continue;
                }
                let Some(commit) = info.get("commit").and_then(Value::as_str) else { continue };
                out.push(CloudProject {
                    project: project.to_string(),
                    name: name.clone(),
                    device: device.clone(),
                    commit: commit.to_string(),
                    updated: info.get("updated").and_then(Value::as_u64).unwrap_or(0),
                });
            }
        }
        out.sort_by(|a, b| b.updated.cmp(&a.updated));
        out
    }

    /// A full bundle of `refs/archon/wip/<device>` and its commit.
    pub fn get_project(&self, project: &str, device: &str) -> Result<(String, Vec<u8>), String> {
        check_id(project, "project")?;
        check_id(device, "device")?;
        let repo = self.repo(project);
        if !repo.join("HEAD").exists() {
            return Err(format!("No snapshot of project {project} is stored"));
        }
        let refname = wip_ref(device);
        let commit = git(&repo, &["rev-parse", "--verify", refname.as_str()])?;
        let file = tmp_path(&self.root, "out");
        let file_str = file.to_str().ok_or("bad temp path")?;
        let result = git(&repo, &["bundle", "create", file_str, refname.as_str()])
            .and_then(|_| fs::read(&file).map_err(|e| format!("could not read the bundle: {e}")));
        let _ = fs::remove_file(&file);
        Ok((commit, result?))
    }

    /// Replaces `device`'s stored chats. Each value needs a string `id`; others are skipped.
    pub fn put_chats(&self, device: &str, sessions: &[Value]) -> Result<(), String> {
        check_id(device, "device")?;
        self.make_private()?;
        let dir = self.root.join("chats").join(device);
        fs::create_dir_all(&dir).map_err(|e| format!("could not create {}: {e}", dir.display()))?;
        let mut keep = HashSet::new();
        for session in sessions {
            let Some(id) = session.get("id").and_then(Value::as_str) else { continue };
            if !safe_id(id) {
                continue;
            }
            let file = format!("{id}.json");
            let bytes = serde_json::to_vec(session).map_err(|e| e.to_string())?;
            write_atomic(&dir.join(&file), &bytes)?;
            keep.insert(file);
        }
        let entries = fs::read_dir(&dir).map_err(|e| format!("could not read {}: {e}", dir.display()))?;
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.ends_with(".json") && !keep.contains(&name) {
                let _ = fs::remove_file(entry.path());
            }
        }
        Ok(())
    }

    /// The chats stored for `device`, newest first.
    pub fn chats(&self, device: &str) -> Vec<Value> {
        if !safe_id(device) {
            return Vec::new();
        }
        let Ok(entries) = fs::read_dir(self.root.join("chats").join(device)) else { return Vec::new() };
        let mut out: Vec<Value> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| is_json(p))
            .filter_map(|p| fs::read(&p).ok())
            .filter_map(|b| serde_json::from_slice::<Value>(&b).ok())
            .collect();
        out.sort_by(|a, b| updated(b).cmp(&updated(a)));
        out
    }

    /// Device ids with chats stored.
    pub fn chat_devices(&self) -> Vec<String> {
        let chats = self.root.join("chats");
        let Ok(entries) = fs::read_dir(&chats) else { return Vec::new() };
        let mut out: Vec<String> = entries
            .flatten()
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|d| safe_id(d) && has_chat(&chats.join(d)))
            .collect();
        out.sort();
        out
    }

    /// Creates the store root owner-only (0700), and makes one left by an older run so. The copies hold
    /// uncommitted work, maybe secrets, so other accounts on the cloud device must not read them.
    fn make_private(&self) -> Result<(), String> {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
        let failed = |e: std::io::Error| format!("could not create {}: {e}", self.root.display());
        fs::DirBuilder::new().recursive(true).mode(0o700).create(&self.root).map_err(failed)?;
        fs::set_permissions(&self.root, fs::Permissions::from_mode(0o700)).map_err(failed)
    }

    /// The bare repository of `project`.
    fn repo(&self, project: &str) -> PathBuf {
        self.root.join("projects").join(format!("{project}.git"))
    }
}

/// True for ids that are safe as file and ref names: 1 to 128 ASCII letters, digits, `-` or `_`.
/// Ids come from the network, so this is what keeps them from reaching outside the store.
pub fn safe_id(s: &str) -> bool {
    !s.is_empty() && s.len() <= 128 && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn check_id(id: &str, what: &str) -> Result<(), String> {
    if safe_id(id) {
        Ok(())
    } else {
        Err(format!("bad {what} id"))
    }
}

fn wip_ref(device: &str) -> String {
    format!("refs/archon/wip/{device}")
}

/// Runs `git -C repo args` and returns its trimmed stdout, or the error text.
fn git(repo: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map_err(|e| format!("git {}: {e}", args.join(" ")))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        Err(format!("git {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim()))
    }
}

/// A file name in `dir` that no other write is using.
fn tmp_path(dir: &Path, tag: &str) -> PathBuf {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    dir.join(format!(".tmp-{tag}-{}-{nanos}-{n}", std::process::id()))
}

/// Writes `bytes` to `path` through a temp file in the same folder, so a reader never sees half a file.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let dir = path.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(dir).map_err(|e| format!("could not create {}: {e}", dir.display()))?;
    let tmp = tmp_path(dir, "write");
    fs::write(&tmp, bytes).map_err(|e| format!("could not write {}: {e}", tmp.display()))?;
    fs::rename(&tmp, path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        format!("could not save {}: {e}", path.display())
    })
}

/// A project's metadata file as a JSON object; empty when missing or not an object.
fn read_meta(path: &Path) -> serde_json::Map<String, Value> {
    match fs::read(path).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()) {
        Some(Value::Object(map)) => map,
        _ => serde_json::Map::new(),
    }
}

fn is_json(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()) == Some("json")
}

/// Whether `dir` holds at least one stored chat.
fn has_chat(dir: &Path) -> bool {
    fs::read_dir(dir).map(|rd| rd.flatten().any(|e| is_json(&e.path()))).unwrap_or(false)
}

fn updated(v: &Value) -> u64 {
    v.get("updated").and_then(Value::as_u64).unwrap_or(0)
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Stdio;

    /// A fresh directory for one test.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("archon-cloud-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    /// False when git is not installed; the git tests then do nothing.
    fn have_git() -> bool {
        Command::new("git").arg("--version").output().map(|o| o.status.success()).unwrap_or(false)
    }

    fn path_str(p: &Path) -> &str {
        p.to_str().expect("utf8 path")
    }

    /// Runs git in `repo` and returns its trimmed stdout.
    fn git_in(repo: &Path, args: &[&str], stdin: Option<&[u8]>) -> String {
        use std::io::Write;
        let mut child = Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(args)
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@t")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@t")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("run git");
        if let Some(input) = stdin {
            child.stdin.take().expect("stdin").write_all(input).expect("write stdin");
        }
        let out = child.wait_with_output().expect("git output");
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// Commits `text` as `file.txt` on `refs/archon/wip/n1` in `repo`; returns the commit.
    fn commit_wip(repo: &Path, text: &str, parent: Option<&str>) -> String {
        let blob = git_in(repo, &["hash-object", "-w", "--stdin"], Some(text.as_bytes()));
        let tree = git_in(repo, &["mktree"], Some(format!("100644 blob {blob}\tfile.txt\n").as_bytes()));
        let mut args = vec!["commit-tree", tree.as_str(), "-m", "wip"];
        if let Some(p) = parent {
            args.push("-p");
            args.push(p);
        }
        let commit = git_in(repo, &args, None);
        git_in(repo, &["update-ref", "refs/archon/wip/n1", commit.as_str()], None);
        commit
    }

    /// Bundles `refs/archon/wip/n1` from `repo` into `file`, excluding `exclude` when given.
    fn make_bundle(repo: &Path, file: &Path, exclude: Option<&str>) -> Vec<u8> {
        let excl = exclude.map(|e| format!("^{e}"));
        let mut args = vec!["bundle", "create", path_str(file), "refs/archon/wip/n1"];
        if let Some(e) = excl.as_deref() {
            args.push(e);
        }
        git_in(repo, &args, None);
        fs::read(file).expect("read bundle")
    }

    #[test]
    fn ids_are_plain_names() {
        assert!(safe_id("ok-1_A"));
        assert!(safe_id("n1234"));
        assert!(safe_id(&"a".repeat(128)));
        assert!(!safe_id(""));
        assert!(!safe_id("../x"));
        assert!(!safe_id("a/b"));
        assert!(!safe_id("a.b"));
        assert!(!safe_id(&"a".repeat(129)));
    }

    #[test]
    fn project_snapshots_are_stored_and_grow() {
        if !have_git() {
            return;
        }
        let base = scratch("snapshots");
        let src = base.join("src");
        fs::create_dir_all(&src).expect("source dir");
        git_in(&src, &["init", "-q"], None);
        let first = commit_wip(&src, "one\n", None);
        let store = Store::open(base.join("cloud"));

        let full = make_bundle(&src, &base.join("full.bundle"), None);
        store.put_project("p1", "app", "n1", &first, &full).expect("full bundle");
        let listed = store.projects();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].project, "p1");
        assert_eq!(listed[0].name, "app");
        assert_eq!(listed[0].device, "n1");
        assert_eq!(listed[0].commit, first);

        // The bundle we send back is accepted by a fresh repository.
        let (commit, bytes) = store.get_project("p1", "n1").expect("get project");
        assert_eq!(commit, first);
        let got = base.join("got.bundle");
        fs::write(&got, &bytes).expect("write bundle");
        let dest = base.join("dest");
        fs::create_dir_all(&dest).expect("dest dir");
        git_in(&dest, &["init", "-q"], None);
        git_in(&dest, &["fetch", "-q", path_str(&got), "+refs/archon/wip/n1:refs/archon/wip/n1"], None);
        assert_eq!(git_in(&dest, &["rev-parse", "refs/archon/wip/n1"], None), first);

        // A second commit goes up as an increment on top of the first.
        let second = commit_wip(&src, "two\n", Some(first.as_str()));
        let inc = make_bundle(&src, &base.join("inc.bundle"), Some(first.as_str()));
        store.put_project("p1", "app", "n1", &second, &inc).expect("increment");
        assert_eq!(store.get_project("p1", "n1").expect("get again").0, second);
        assert_eq!(store.projects().len(), 1);

        // An empty bundle means nothing new; the ref must already be at the commit.
        store.put_project("p1", "app", "n1", &second, &[]).expect("nothing new");
        assert!(store.put_project("p1", "app", "n1", &first, &[]).is_err());

        // Bad ids are refused.
        assert!(store.put_project("../evil", "x", "n1", &first, &full).is_err());
        assert!(store.put_project("p1", "x", "a/b", &first, &full).is_err());
    }

    #[test]
    fn chats_replace_and_drop_stale_files() {
        let base = scratch("chats");
        let store = Store::open(base.join("cloud"));
        let a = json!({ "id": "s-a", "title": "a", "updated": 1 });
        let b = json!({ "id": "s-b", "title": "b", "updated": 2 });
        let no_id = json!({ "title": "skipped" });
        store.put_chats("n1", &[a.clone(), b.clone(), no_id]).expect("two chats");
        let dir = base.join("cloud").join("chats").join("n1");
        assert!(dir.join("s-a.json").exists());
        assert!(dir.join("s-b.json").exists());
        assert_eq!(store.chats("n1"), vec![b.clone(), a.clone()]);

        store.put_chats("n1", &[a.clone()]).expect("one chat");
        assert!(!dir.join("s-b.json").exists());
        assert_eq!(store.chats("n1"), vec![a]);
        assert_eq!(store.chat_devices(), vec!["n1".to_string()]);
        assert!(store.put_chats("../n1", &[]).is_err());
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(base.join("cloud")).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700, "other accounts can't read the cloud copy");
        assert!(store.chats("../n1").is_empty());
    }
}
