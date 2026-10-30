//! Keeps two processes from writing the same chats. Each keeps the chats in memory and writes its own
//! copy, so a second writer would undo the first one's changes. `archonui --serve` holds an exclusive
//! lock on the chats folder and the app a shared one, so any number of app windows still run together
//! as before, but never alongside a `--serve` on the same folder. The lock is `flock` on the folder
//! itself: no file is added, and it goes away with the process.

use std::{fs::File, io, os::fd::AsRawFd, path::Path};

/// A held lock. Dropping it releases the lock.
pub struct Lock(File);

/// Locks `dir` without waiting, creating it if needed. `Ok(None)` when another process holds a lock
/// that conflicts.
pub fn try_lock(dir: &Path, exclusive: bool) -> io::Result<Option<Lock>> {
    crate::store::create_private_dir(dir)?;
    let file = File::open(dir)?;
    let op = (if exclusive { libc::LOCK_EX } else { libc::LOCK_SH }) | libc::LOCK_NB;
    // SAFETY: `flock` on a descriptor this function owns.
    if unsafe { libc::flock(file.as_raw_fd(), op) } == 0 {
        return Ok(Some(Lock(file)));
    }
    let e = io::Error::last_os_error();
    if e.raw_os_error() == Some(libc::EWOULDBLOCK) {
        Ok(None)
    } else {
        Err(e)
    }
}

/// The message when the other kind of process already has the chats in `dir`.
pub fn busy_message(dir: &Path, serve: bool) -> String {
    if serve {
        format!(
            "the archonUI app or another `archonui --serve` is using the chats in {}. Quit it first, or give this one its own folder with ARCHONUI_DATA_DIR.",
            dir.display()
        )
    } else {
        format!(
            "`archonui --serve` is using the chats in {} on this device. Stop it first, or run it with its own ARCHONUI_DATA_DIR.",
            dir.display()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_locks_coexist_and_exclude_an_exclusive_one() {
        let dir = std::env::temp_dir().join(format!("archonui-lock-{}", crate::store::new_id()));
        let a = try_lock(&dir, false).unwrap();
        let b = try_lock(&dir, false).unwrap();
        assert!(a.is_some() && b.is_some(), "app windows run together");
        assert!(try_lock(&dir, true).unwrap().is_none(), "no --serve while the app runs");
        drop((a, b));
        // Another test's child can hold a copy of the descriptor between its fork and exec.
        let serve = (0..50)
            .find_map(|_| try_lock(&dir, true).unwrap().or_else(|| {
                std::thread::sleep(std::time::Duration::from_millis(20));
                None
            }));
        assert!(serve.is_some());
        assert!(try_lock(&dir, false).unwrap().is_none(), "no app while --serve runs");
        assert!(try_lock(&dir, true).unwrap().is_none(), "no second --serve");
        drop(serve);
        let app = (0..50).find_map(|_| try_lock(&dir, false).unwrap().or_else(|| {
            std::thread::sleep(std::time::Duration::from_millis(20));
            None
        }));
        assert!(app.is_some());
        std::fs::remove_dir_all(&dir).ok();
    }
}
