//! File tree listing and change watching for the built-in IDE (no UI).

#![allow(dead_code)]

use ignore::{
    gitignore::{Gitignore, GitignoreBuilder},
    WalkBuilder,
};
use notify::{
    event::{ModifyKind, RenameMode},
    Event, EventKind, RecursiveMode, Watcher as _,
};
use smol::channel::{Receiver, Sender};
use std::{
    collections::{hash_map, HashMap},
    io,
    path::{Component, Path, PathBuf},
    sync::{
        mpsc::{self, RecvTimeoutError},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

/// Always hidden from the tree and from watch events, whatever the ignore files say.
const HIDDEN_NAMES: &[&str] =
    &[".git", "node_modules", "target", ".venv", "__pycache__", ".DS_Store"];

/// How long a path's changes are collected before one `Change` is emitted.
const DEBOUNCE: Duration = Duration::from_millis(150);

/// Events for a path within this long after `Watcher::ignore_own_write` are dropped.
const OWN_WRITE_WINDOW: Duration = Duration::from_secs(1);

/// One child of a directory in the file tree.
pub struct Entry {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
}

/// The direct children of `dir`, directories first, each group sorted case-insensitively.
///
/// Respects the `.gitignore`, `.ignore` and git exclude rules that apply to `dir`,
/// even outside a git repository, but keeps dotfiles and always hides `HIDDEN_NAMES`.
pub fn list_dir(root: &Path, dir: &Path) -> io::Result<Vec<Entry>> {
    if !dir.starts_with(root) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "directory is outside the root"));
    }
    let walker = WalkBuilder::new(dir)
        .max_depth(Some(1))
        .hidden(false)
        .require_git(false)
        .build();
    let mut entries = Vec::new();
    for result in walker {
        let entry = result.map_err(io::Error::other)?;
        if entry.depth() == 0 {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if HIDDEN_NAMES.contains(&name.as_str()) {
            continue;
        }
        let path = entry.path().to_path_buf();
        // `file_type` does not follow symlinks, so a link out of the root is
        // shown but never expanded as a directory.
        let is_dir = entry.file_type().is_some_and(|t| t.is_dir());
        entries.push(Entry { path, name, is_dir });
    }
    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(entries)
}

/// `path` relative to `root`, for display.
pub fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root).unwrap_or(path).to_string_lossy().into_owned()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    Modified,
    Created,
    Removed,
}

/// A debounced change to one path under the watched root.
#[derive(Debug, PartialEq, Eq)]
pub struct Change {
    pub path: PathBuf,
    pub kind: ChangeKind,
}

type OwnWrites = Arc<Mutex<HashMap<PathBuf, Instant>>>;

/// Watches a root directory recursively and reports debounced changes.
pub struct Watcher {
    /// Dropping this wakes the thread that owns the OS watcher, which then drops it.
    _stop: mpsc::Sender<()>,
    own_writes: OwnWrites,
}

impl Watcher {
    /// Starts watching `root` and returns once the watch is in place. Dropping the `Watcher` stops the change stream.
    pub fn new(root: &Path) -> Result<(Self, Receiver<Change>), String> {
        let (watcher, rx, ready) = Self::start(root);
        ready.recv().map_err(|e| e.to_string())??;
        Ok((watcher, rx))
    }

    /// Like `new`, but registers the recursive watch on a background thread. A big root
    /// (a home folder) takes seconds to walk, which must not hold up the window. Changes
    /// made before the watch is in place are not reported; a failure is logged.
    pub fn new_background(root: &Path) -> (Self, Receiver<Change>) {
        let (watcher, rx, ready) = Self::start(root);
        let root = root.to_path_buf();
        thread::spawn(move || {
            if let Ok(Err(e)) = ready.recv() {
                eprintln!("could not watch {}: {e}", root.display());
            }
        });
        (watcher, rx)
    }

    fn start(root: &Path) -> (Self, Receiver<Change>, mpsc::Receiver<Result<(), String>>) {
        let (raw_tx, raw_rx) = mpsc::channel();
        let (stop_tx, stop_rx) = mpsc::channel::<()>();
        let (ready_tx, ready_rx) = mpsc::channel();
        let watch_root = root.to_path_buf();
        thread::spawn(move || {
            let result = notify::recommended_watcher(raw_tx).map_err(|e| e.to_string()).and_then(|mut watcher| {
                watcher.watch(&watch_root, RecursiveMode::Recursive).map_err(|e| e.to_string())?;
                Ok(watcher)
            });
            match result {
                Ok(watcher) => {
                    ready_tx.send(Ok(())).ok();
                    // Keep the OS watcher alive until the `Watcher` is dropped.
                    stop_rx.recv().ok();
                    drop(watcher);
                }
                Err(e) => {
                    ready_tx.send(Err(e)).ok();
                }
            }
        });

        let (tx, rx) = smol::channel::unbounded();
        let own_writes = OwnWrites::default();
        let filter = Filter {
            root: root.to_path_buf(),
            gitignore: load_gitignore(root),
            own_writes: own_writes.clone(),
        };
        thread::spawn(move || debounce(raw_rx, tx, filter));
        (Self { _stop: stop_tx, own_writes }, rx, ready_rx)
    }

    /// Suppresses events for `path` for the next second. Call before the editor saves it.
    pub fn ignore_own_write(&self, path: &Path) {
        let mut own_writes = self.own_writes.lock().unwrap();
        own_writes.retain(|_, at| at.elapsed() < OWN_WRITE_WINDOW);
        own_writes.insert(path.to_path_buf(), Instant::now());
    }
}

/// Decides which raw events reach the debouncer.
struct Filter {
    root: PathBuf,
    gitignore: Gitignore,
    own_writes: OwnWrites,
}

impl Filter {
    fn is_ignored(&self, path: &Path) -> bool {
        let Ok(rel) = path.strip_prefix(&self.root) else {
            return false;
        };
        let hidden_component = rel.components().any(|component| {
            matches!(component, Component::Normal(name) if HIDDEN_NAMES.iter().any(|h| name == *h))
        });
        if hidden_component || is_temp_name(path) {
            return true;
        }
        self.gitignore.matched_path_or_any_parents(rel, path.is_dir()).is_ignore()
    }

    fn is_own_write(&self, path: &Path) -> bool {
        self.own_writes
            .lock()
            .unwrap()
            .get(path)
            .is_some_and(|at| at.elapsed() < OWN_WRITE_WINDOW)
    }
}

fn load_gitignore(root: &Path) -> Gitignore {
    let mut builder = GitignoreBuilder::new(root);
    builder.add(root.join(".gitignore"));
    builder.build().unwrap_or_else(|_| Gitignore::empty())
}

/// Editor and tool scratch files: backups, swap files, atomic-save temp files.
fn is_temp_name(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    name.ends_with('~')
        || name.ends_with(".swp")
        || name.ends_with(".swx")
        || name.ends_with(".tmp")
        || name.starts_with(".#")
        || name.contains(".tmp.")
}

/// The changes a raw notify event means, per path.
fn classify(kind: &EventKind, paths: &[PathBuf]) -> Vec<(PathBuf, ChangeKind)> {
    let all = |change: ChangeKind| -> Vec<(PathBuf, ChangeKind)> {
        paths.iter().map(|path| (path.clone(), change)).collect()
    };
    match kind {
        EventKind::Create(_) => all(ChangeKind::Created),
        EventKind::Remove(_) => all(ChangeKind::Removed),
        EventKind::Modify(ModifyKind::Name(RenameMode::From)) => all(ChangeKind::Removed),
        EventKind::Modify(ModifyKind::Name(RenameMode::To)) => all(ChangeKind::Created),
        EventKind::Modify(ModifyKind::Name(RenameMode::Both)) => match paths {
            [from, to] => {
                vec![(from.clone(), ChangeKind::Removed), (to.clone(), ChangeKind::Created)]
            }
            _ => Vec::new(),
        },
        // Platforms that report renames without a direction: the path's existence decides.
        EventKind::Modify(ModifyKind::Name(_)) => paths
            .iter()
            .map(|path| {
                let kind = if path.exists() { ChangeKind::Created } else { ChangeKind::Removed };
                (path.clone(), kind)
            })
            .collect(),
        EventKind::Modify(_) => paths
            .iter()
            .filter(|path| !path.is_dir())
            .map(|path| (path.clone(), ChangeKind::Modified))
            .collect(),
        EventKind::Access(_) | EventKind::Any | EventKind::Other => Vec::new(),
    }
}

/// Folds `kind` into the pending change for `path`; the first event starts the window.
fn merge(pending: &mut HashMap<PathBuf, (ChangeKind, Instant)>, path: PathBuf, kind: ChangeKind) {
    match pending.entry(path) {
        hash_map::Entry::Vacant(entry) => {
            entry.insert((kind, Instant::now() + DEBOUNCE));
        }
        hash_map::Entry::Occupied(mut entry) => match (entry.get().0, kind) {
            // Created then Removed never existed as far as the UI is concerned.
            (ChangeKind::Created, ChangeKind::Removed) => {
                entry.remove();
            }
            (ChangeKind::Created, ChangeKind::Modified) => {}
            _ => entry.get_mut().0 = kind,
        },
    }
}

/// Collects raw events and emits one `Change` per path per debounce window.
fn debounce(raw: mpsc::Receiver<notify::Result<Event>>, out: Sender<Change>, filter: Filter) {
    let mut pending: HashMap<PathBuf, (ChangeKind, Instant)> = HashMap::new();
    loop {
        let next_due = pending.values().map(|&(_, due)| due).min();
        let received = match next_due {
            Some(due) => raw.recv_timeout(due.saturating_duration_since(Instant::now())),
            None => raw.recv().map_err(|_| RecvTimeoutError::Disconnected),
        };
        match received {
            Ok(Ok(event)) => {
                for (path, kind) in classify(&event.kind, &event.paths) {
                    if !filter.is_ignored(&path) && !filter.is_own_write(&path) {
                        merge(&mut pending, path, kind);
                    }
                }
            }
            Ok(Err(_)) | Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }

        let now = Instant::now();
        let due: Vec<PathBuf> = pending
            .iter()
            .filter(|(_, value)| value.1 <= now)
            .map(|(path, _)| path.clone())
            .collect();
        for path in due {
            if let Some((kind, _)) = pending.remove(&path) {
                if out.send_blocking(Change { path, kind }).is_err() {
                    return;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use smol::channel::TryRecvError;
    use std::{
        fs,
        sync::atomic::{AtomicUsize, Ordering},
    };

    /// A fresh empty directory under the system temp dir, removed on drop.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            static COUNTER: AtomicUsize = AtomicUsize::new(0);
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let name = format!("archonui-ws-test-{}-{}", std::process::id(), n);
            let path = std::env::temp_dir().join(name);
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn names(entries: &[Entry]) -> Vec<&str> {
        entries.iter().map(|entry| entry.name.as_str()).collect()
    }

    #[test]
    fn list_dir_orders_and_filters() {
        let dir = TempDir::new();
        let root = dir.0.as_path();
        fs::write(root.join(".gitignore"), "*.log\n").unwrap();
        fs::write(root.join("debug.log"), "").unwrap();
        fs::write(root.join(".env"), "").unwrap();
        fs::write(root.join("zeta.txt"), "").unwrap();
        fs::write(root.join("Alpha.rs"), "").unwrap();
        fs::write(root.join("beta.md"), "").unwrap();
        fs::create_dir(root.join("b_dir")).unwrap();
        fs::create_dir(root.join("A_dir")).unwrap();
        for hidden in [".git", "node_modules", "target", ".venv", "__pycache__"] {
            fs::create_dir(root.join(hidden)).unwrap();
        }
        fs::write(root.join(".DS_Store"), "").unwrap();

        let entries = list_dir(root, root).unwrap();
        assert_eq!(
            names(&entries),
            [
                "A_dir", "b_dir", ".env", ".gitignore", "Alpha.rs", "beta.md", "zeta.txt"
            ]
        );
        assert!(entries[0].is_dir && entries[1].is_dir && !entries[2].is_dir);
        assert_eq!(entries[0].path, root.join("A_dir"));

        let nested = list_dir(root, &root.join("A_dir")).unwrap();
        assert!(nested.is_empty());
        assert!(list_dir(root, Path::new("/")).is_err());
    }

    #[test]
    fn list_dir_does_not_expand_symlinked_directories() {
        let dir = TempDir::new();
        let root = dir.0.as_path();
        fs::create_dir(root.join("real")).unwrap();
        std::os::unix::fs::symlink(root.join("real"), root.join("linked")).unwrap();

        let entries = list_dir(root, root).unwrap();
        let linked = entries.iter().find(|e| e.name == "linked").expect("linked entry");
        assert!(!linked.is_dir, "a symlink is not treated as a directory");
        assert!(entries.iter().find(|e| e.name == "real").is_some_and(|e| e.is_dir));
    }

    /// Next change within `within`, or `None` if none arrives or the stream closed.
    fn next_change(rx: &Receiver<Change>, within: Duration) -> Option<Change> {
        let deadline = Instant::now() + within;
        loop {
            match rx.try_recv() {
                Ok(change) => return Some(change),
                Err(TryRecvError::Closed) => return None,
                Err(TryRecvError::Empty) => {}
            }
            if Instant::now() >= deadline {
                return None;
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn expect_change(rx: &Receiver<Change>, path: &Path, kind: ChangeKind) {
        let change = next_change(rx, Duration::from_secs(3))
            .unwrap_or_else(|| panic!("no change for {}", path.display()));
        assert_eq!(change, Change { path: path.to_path_buf(), kind });
    }

    fn expect_silence(rx: &Receiver<Change>) {
        let change = next_change(rx, Duration::from_millis(600));
        assert_eq!(change, None);
    }

    #[test]
    fn watcher_reports_debounced_changes() {
        let dir = TempDir::new();
        let root = dir.0.as_path();
        fs::write(root.join(".gitignore"), "*.log\n").unwrap();
        let (watcher, rx) = Watcher::new(root).unwrap();

        let file = root.join("a.txt");
        fs::write(&file, "x").unwrap();
        expect_change(&rx, &file, ChangeKind::Created);
        expect_silence(&rx);

        for i in 0..5 {
            fs::write(&file, format!("write {i}")).unwrap();
        }
        expect_change(&rx, &file, ChangeKind::Modified);
        expect_silence(&rx);

        fs::remove_file(&file).unwrap();
        expect_change(&rx, &file, ChangeKind::Removed);
        expect_silence(&rx);

        let sub = root.join("sub");
        fs::create_dir(&sub).unwrap();
        expect_change(&rx, &sub, ChangeKind::Created);
        expect_silence(&rx);

        fs::create_dir(root.join("node_modules")).unwrap();
        fs::write(root.join("node_modules").join("x.js"), "x").unwrap();
        fs::write(root.join("a.txt~"), "x").unwrap();
        fs::write(root.join(".#a.txt"), "x").unwrap();
        fs::write(root.join("a.txt.tmp.42"), "x").unwrap();
        fs::write(root.join("debug.log"), "x").unwrap();
        expect_silence(&rx);

        let own = root.join("own.txt");
        watcher.ignore_own_write(&own);
        fs::write(&own, "saved by the editor").unwrap();
        expect_silence(&rx);

        let other = root.join("other.txt");
        fs::write(&other, "x").unwrap();
        expect_change(&rx, &other, ChangeKind::Created);
    }

    #[test]
    fn dropping_watcher_closes_changes() {
        let dir = TempDir::new();
        let (watcher, rx) = Watcher::new(&dir.0).unwrap();
        drop(watcher);
        assert_eq!(next_change(&rx, Duration::from_secs(3)), None);
        assert!(rx.is_closed());
    }
}
