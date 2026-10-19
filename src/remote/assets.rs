//! Appearance files shared between devices through the cloud: custom themes, backgrounds and logos.
//! Each device sends the files the cloud lacks or has older, and takes the ones it lacks or has older.
//! Nothing is ever deleted: a file removed on one device stays on the others.

use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

/// The config folders that are shared, and the file types each may hold.
const DIRS: [(&str, &[&str]); 3] = [
    ("themes", &["json"]),
    ("backgrounds", &["png", "jpg", "jpeg", "webp", "gif"]),
    ("logos", &["png", "jpg", "jpeg", "webp", "gif", "svg"]),
];

/// Largest file that is shared. Bigger ones stay on their device.
pub const MAX_FILE: u64 = 25 * 1024 * 1024;

/// One shared file as a list names it: where it goes, a hash of its bytes, and when it last changed
/// (seconds since the epoch).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssetKey {
    pub dir: String,
    pub name: String,
    pub hash: String,
    pub modified: u64,
}

/// One shared file with its bytes, base64.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssetFile {
    pub dir: String,
    pub name: String,
    pub modified: u64,
    pub data: String,
}

/// Whether `dir`/`name` may be shared: one of the shared folders, a plain file name (no folders, not
/// hidden) with a file type that folder holds.
pub fn allowed(dir: &str, name: &str) -> bool {
    let Some((_, types)) = DIRS.iter().find(|(d, _)| *d == dir) else { return false };
    let plain = !name.is_empty() && name.len() <= 200 && !name.starts_with('.') && !name.contains(['/', '\\', '\0']);
    let ext = Path::new(name).extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase);
    plain && ext.is_some_and(|e| types.contains(&e.as_str()))
}

/// A hash of `bytes` (FNV-1a, 64 bit) as hex. Only to tell copies apart, not for security.
pub fn hash(bytes: &[u8]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// The shared files under `root` (a config dir, or the cloud's assets folder), with their paths.
/// Files that may not be shared, or are over [`MAX_FILE`], are left out.
pub fn manifest(root: &Path) -> Vec<(AssetKey, PathBuf)> {
    let mut out = Vec::new();
    for (dir, _) in DIRS {
        let Ok(entries) = fs::read_dir(root.join(dir)) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            let Ok(meta) = entry.metadata() else { continue };
            if !meta.is_file() || meta.len() > MAX_FILE || !allowed(dir, &name) {
                continue;
            }
            let Ok(bytes) = fs::read(&path) else { continue };
            let modified = meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
            out.push((AssetKey { dir: dir.to_string(), name, hash: hash(&bytes), modified }, path));
        }
    }
    out.sort_by(|a, b| (&a.0.dir, &a.0.name).cmp(&(&b.0.dir, &b.0.name)));
    out
}

/// The files in `theirs` that `mine` should take: ones it lacks, and ones that differ and changed later.
pub fn newer<'a>(mine: &[AssetKey], theirs: &'a [AssetKey]) -> Vec<&'a AssetKey> {
    theirs
        .iter()
        .filter(|t| match mine.iter().find(|m| m.dir == t.dir && m.name == t.name) {
            None => true,
            Some(m) => m.hash != t.hash && t.modified > m.modified,
        })
        .collect()
}

/// Reads `key`'s file under `root` for sending. `None` if it can't be read or isn't shareable.
pub fn load(root: &Path, key: &AssetKey) -> Option<AssetFile> {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    if !allowed(&key.dir, &key.name) {
        return None;
    }
    let bytes = fs::read(root.join(&key.dir).join(&key.name)).ok()?;
    (bytes.len() as u64 <= MAX_FILE).then(|| AssetFile { dir: key.dir.clone(), name: key.name.clone(), modified: key.modified, data: STANDARD.encode(bytes) })
}

/// Writes `file` under `root`, keeping its change time, unless what is there is the same or newer.
/// Returns whether anything was written.
pub fn save(root: &Path, file: &AssetFile) -> Result<bool, String> {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    if !allowed(&file.dir, &file.name) {
        return Err(format!("{}/{} can't be shared", file.dir, file.name));
    }
    let bytes = STANDARD.decode(&file.data).map_err(|e| format!("{}: {e}", file.name))?;
    if bytes.len() as u64 > MAX_FILE {
        return Err(format!("{} is too big to share", file.name));
    }
    let dir = root.join(&file.dir);
    let path = dir.join(&file.name);
    if let Ok(meta) = fs::metadata(&path) {
        let theirs = meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
        if theirs >= file.modified || fs::read(&path).is_ok_and(|old| old == bytes) {
            return Ok(false);
        }
    }
    fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    // Written beside it and renamed, so a half-written file is never read as a theme or image.
    let temp = dir.join(format!(".{}.part", file.name));
    fs::write(&temp, &bytes).map_err(|e| format!("{}: {e}", temp.display()))?;
    if let Ok(f) = fs::File::options().write(true).open(&temp) {
        let _ = f.set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(file.modified));
    }
    fs::rename(&temp, &path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("archonui-assets-{name}-{}", crate::store::new_id()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn key(dir: &str, name: &str, hash: &str, modified: u64) -> AssetKey {
        AssetKey { dir: dir.into(), name: name.into(), hash: hash.into(), modified }
    }

    #[test]
    fn only_plain_files_of_the_right_type_in_the_shared_folders() {
        assert!(allowed("themes", "custom-1.json"));
        assert!(allowed("backgrounds", "Beach.JPG"));
        assert!(allowed("logos", "mark.svg"));
        assert!(!allowed("themes", "evil.sh"));
        assert!(!allowed("backgrounds", "../../.bashrc.png"));
        assert!(!allowed("backgrounds", "a/b.png"));
        assert!(!allowed("backgrounds", ".hidden.png"));
        assert!(!allowed("sessions", "x.json"));
        assert!(!allowed("themes", ""));
    }

    #[test]
    fn a_device_takes_what_it_lacks_and_what_changed_later() {
        let mine = [key("themes", "a.json", "1", 10), key("themes", "b.json", "2", 50), key("logos", "c.png", "3", 10)];
        let theirs = [key("themes", "a.json", "9", 20), key("themes", "b.json", "8", 40), key("logos", "c.png", "3", 99), key("backgrounds", "d.png", "4", 1)];
        let got: Vec<&str> = newer(&mine, &theirs).iter().map(|k| k.name.as_str()).collect();
        assert_eq!(got, ["a.json", "d.png"], "b is older there, c is the same file");
    }

    #[test]
    fn files_go_round_and_keep_their_change_time() {
        let (from, to) = (temp("from"), temp("to"));
        fs::create_dir_all(from.join("backgrounds")).unwrap();
        fs::write(from.join("backgrounds/sea.png"), b"pixels").unwrap();
        fs::write(from.join("backgrounds/notes.txt"), b"not shared").unwrap();
        let listed = manifest(&from);
        assert_eq!(listed.len(), 1);
        let file = load(&from, &listed[0].0).unwrap();
        assert!(save(&to, &file).unwrap());
        assert_eq!(fs::read(to.join("backgrounds/sea.png")).unwrap(), b"pixels");
        assert_eq!(manifest(&to)[0].0, listed[0].0, "same hash and change time");
        assert!(!save(&to, &file).unwrap(), "the same file isn't written again");
        let older = AssetFile { data: "b2xk".into(), modified: file.modified.saturating_sub(100), ..file.clone() };
        assert!(!save(&to, &older).unwrap(), "an older copy never replaces a newer one");
        let bad = AssetFile { dir: "backgrounds".into(), name: "../x.png".into(), modified: 1, data: String::new() };
        assert!(save(&to, &bad).is_err());
        fs::remove_dir_all(from).ok();
        fs::remove_dir_all(to).ok();
    }
}
