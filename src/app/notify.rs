//! Tells the user a turn ended: a desktop notification while ArchonUI is in the background, and a
//! sound. The decisions are pure; `spawn` runs the helper programs without waiting for them.

use std::{
    io,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

/// Longest notification body, in characters.
const BODY_CHARS: usize = 120;
/// The freedesktop sound played when a turn finishes.
const COMPLETE_SOUND: &str = "/usr/share/sounds/freedesktop/stereo/complete.oga";

/// How a turn ended.
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    Finished,
    Failed(String),
    /// The user stopped the turn.
    Stopped,
}

/// A turn that ended in a chat, as the alert needs it.
#[derive(Clone, Debug, PartialEq)]
pub struct Ended {
    /// The chat's title at the end of the turn.
    pub title: String,
    pub outcome: Outcome,
    /// The reply's text; empty when there was none.
    pub reply: String,
}

/// What to show and play for a turn that ended.
#[derive(Debug, PartialEq)]
pub struct Alert {
    pub desktop: bool,
    pub sound: bool,
}

/// The alert for a turn that ended. A desktop notification needs the setting on and ArchonUI in the
/// background. Sound follows its own setting. A turn the user stopped alerts nobody.
pub fn alert(notify: bool, sound: bool, window_active: bool, outcome: &Outcome) -> Alert {
    let stopped = *outcome == Outcome::Stopped;
    Alert { desktop: notify && !window_active && !stopped, sound: sound && !stopped }
}

/// The notification body: the start of the reply, or `Finished`, or `Failed: …` with the error on one line.
pub fn body(outcome: &Outcome, reply: &str) -> String {
    let text = match outcome {
        Outcome::Failed(error) => format!("Failed: {}", error.split_whitespace().collect::<Vec<_>>().join(" ")),
        _ if reply.trim().is_empty() => "Finished".to_string(),
        _ => reply.trim().to_string(),
    };
    text.chars().take(BODY_CHARS).collect()
}

/// `notify-send` arguments: the app name, the icon when there is one, then the summary and body.
/// `--` ends the options, so a body that starts with `-` is still text; the app name uses
/// `--app-name=` so one starting with `-` is not read as an option either.
pub fn notify_args(app: &str, icon: Option<&Path>, summary: &str, body: &str) -> Vec<String> {
    let mut args = vec![format!("--app-name={app}")];
    if let Some(icon) = icon {
        args.extend(["-i".to_string(), icon.display().to_string()]);
    }
    args.extend(["--".to_string(), summary.to_string(), body.to_string()]);
    args
}

/// The program and arguments that play the finished sound: `canberra-gtk-play` when installed,
/// else `paplay` on the freedesktop sound, else nothing.
pub fn sound_command(has_program: impl Fn(&str) -> bool, has_file: impl Fn(&Path) -> bool) -> Option<(String, Vec<String>)> {
    if has_program("canberra-gtk-play") {
        return Some(("canberra-gtk-play".into(), vec!["-i".into(), "complete".into()]));
    }
    (has_program("paplay") && has_file(Path::new(COMPLETE_SOUND))).then(|| ("paplay".into(), vec![COMPLETE_SOUND.into()]))
}

/// Whether `program` is on `PATH`.
pub fn on_path(program: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|paths| std::env::split_paths(&paths).any(|dir| dir.join(program).is_file()))
}

/// Writes `bytes` to `cache/name` unless that file is already there, and returns its path.
pub fn cache_file(cache: &Path, name: &str, bytes: &[u8]) -> io::Result<PathBuf> {
    let path = cache.join(name);
    // Atomic, so an interrupted write can't leave a truncated file that is then reused as the cache.
    if !path.is_file() {
        super::persist::write_atomic(&path, bytes)?;
    }
    Ok(path)
}

/// Runs `program` with `args` in the background. Nothing waits for it, and a failure to start is logged.
pub fn spawn(program: &str, args: &[String]) {
    let child = Command::new(program).args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn();
    match child {
        Ok(mut child) => {
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
        Err(e) => eprintln!("could not run {program}: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_desktop_alert_needs_the_setting_and_a_background_window() {
        let finished = Outcome::Finished;
        assert!(alert(true, false, false, &finished).desktop);
        assert!(!alert(true, false, true, &finished).desktop, "the window is active");
        assert!(!alert(false, false, false, &finished).desktop, "notifications are off");
    }

    #[test]
    fn a_stopped_turn_alerts_nobody() {
        assert_eq!(alert(true, true, false, &Outcome::Stopped), Alert { desktop: false, sound: false });
        assert_eq!(alert(true, true, false, &Outcome::Failed("x".into())), Alert { desktop: true, sound: true });
    }

    #[test]
    fn sound_follows_its_own_setting() {
        assert!(alert(false, true, true, &Outcome::Finished).sound);
        assert!(!alert(true, false, false, &Outcome::Finished).sound);
    }

    #[test]
    fn the_body_is_the_reply_or_a_status() {
        assert_eq!(body(&Outcome::Finished, "  Done. \n"), "Done.");
        assert_eq!(body(&Outcome::Finished, "  "), "Finished");
        assert_eq!(body(&Outcome::Finished, &"a".repeat(200)).chars().count(), 120);
        assert_eq!(body(&Outcome::Failed("exit status: 1\nno  key\nfound".into()), ""), "Failed: exit status: 1 no key found");
    }

    #[test]
    fn notify_send_gets_the_app_icon_and_text_after_options() {
        let args = notify_args("archonui", Some(Path::new("/c/icon.png")), "Fix it", "- step");
        assert_eq!(args, ["--app-name=archonui", "-i", "/c/icon.png", "--", "Fix it", "- step"]);
        assert_eq!(notify_args("archonui", None, "t", "b"), ["--app-name=archonui", "--", "t", "b"]);
    }

    #[test]
    fn sound_prefers_canberra_then_paplay_then_nothing() {
        let all = |_: &str| true;
        let file = |_: &Path| true;
        assert_eq!(sound_command(all, file), Some(("canberra-gtk-play".into(), vec!["-i".into(), "complete".into()])));
        let no_canberra = |p: &str| p != "canberra-gtk-play";
        assert_eq!(sound_command(no_canberra, file), Some(("paplay".into(), vec![COMPLETE_SOUND.into()])));
        assert_eq!(sound_command(no_canberra, |_| false), None, "no sound file, nothing plays");
        assert_eq!(sound_command(|_| false, file), None, "no player, nothing plays");
    }

    #[test]
    fn cache_files_are_written_once() {
        let dir = std::env::temp_dir().join(format!("archonui-notify-test-{}", std::process::id()));
        let first = cache_file(&dir, "logo.png", b"one").unwrap();
        let again = cache_file(&dir, "logo.png", b"two").unwrap();
        assert_eq!(first, again);
        assert_eq!(std::fs::read(&first).unwrap(), b"one");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
