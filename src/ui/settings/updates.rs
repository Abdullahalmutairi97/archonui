//! Settings → General → Updates: check GitHub for newer commits and update in place (app/update.rs).

use super::controls::listen;
use crate::{
    app::update::{self, LastCheck, Source, Status},
    ui::{
        scale::px,
        shell::Shell,
        widgets::{
            button::{Button, ButtonStyle},
            section::SettingRow,
        },
    },
};
use gpui::{div, prelude::*, Context};
use std::path::PathBuf;

/// The update check and run, not persisted (the last quiet check is kept in the data dir).
#[derive(Default)]
pub struct UpdateUi {
    /// The newest check's result; `None` until one finishes.
    pub(in crate::ui) status: Option<Status>,
    pub(in crate::ui) checking: bool,
    /// An update is running.
    pub(in crate::ui) running: bool,
    /// Progress lines from the running or last update, newest last (the last few are kept).
    pub(in crate::ui) log: Vec<String>,
    /// The last update's outcome: the installed binary, or why it stopped.
    pub(in crate::ui) outcome: Option<Result<PathBuf, String>>,
}

impl UpdateUi {
    /// Whether to show the small "update available" dot.
    pub(in crate::ui) fn dot(&self) -> bool {
        self.status.as_ref().is_some_and(Status::update_available) && !matches!(self.outcome, Some(Ok(_)))
    }
}

const LOG_LINES: usize = 8;

fn home() -> PathBuf {
    std::env::var_os("HOME").map_or_else(|| PathBuf::from("/"), PathBuf::from)
}

fn source() -> Source {
    update::locate(|k| std::env::var(k).ok(), &home())
}

impl Shell {
    fn last_check_path(&self) -> PathBuf {
        self.paths.data.join("update-check.json")
    }

    /// The quiet startup check: at most once a day, only when on, and it never installs anything.
    /// A result from today is reused, so the dot shows straight away.
    pub(in crate::ui) fn startup_update_check(&mut self, cx: &mut Context<Self>) {
        if !self.state.settings.update_check {
            return;
        }
        let last = update::read_last(&self.last_check_path());
        let fresh = last.as_ref().filter(|l| l.commit == update::BUILD_COMMIT && !update::check_due(Some(l.at), update::now_secs()));
        match fresh {
            Some(last) => self.settings.update.status = last.status.clone(),
            None => self.check_updates(cx),
        }
    }

    /// Checks GitHub in the background and records when.
    pub(super) fn check_updates(&mut self, cx: &mut Context<Self>) {
        if self.settings.update.checking {
            return;
        }
        self.settings.update.checking = true;
        cx.notify();
        let path = self.last_check_path();
        cx.spawn(async move |this, cx| {
            let status = cx
                .background_executor()
                .spawn(async move {
                    let status = update::check(&update::follow_branch(update::BUILD_BRANCH));
                    let last = LastCheck { at: update::now_secs(), commit: update::BUILD_COMMIT.into(), status: Some(status.clone()) };
                    update::write_last(&path, &last);
                    status
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.settings.update.checking = false;
                this.settings.update.status = Some(status);
                cx.notify();
            });
        })
        .detach();
    }

    /// Runs the update: pull, build, install, streaming progress into the log.
    fn start_update(&mut self, cx: &mut Context<Self>) {
        let Source::Checkout { src, bin } = source() else { return };
        if self.settings.update.running {
            return;
        }
        let u = &mut self.settings.update;
        u.running = true;
        u.log.clear();
        u.outcome = None;
        cx.notify();
        let (tx, rx) = smol::channel::unbounded::<String>();
        let branch = update::follow_branch(update::BUILD_BRANCH);
        let job = std::thread::spawn(move || {
            let say = |line: String| {
                let _ = tx.send_blocking(line);
            };
            update::run(&src, &bin, &branch, &home(), &say)
        });
        cx.spawn(async move |this, cx| {
            while let Ok(line) = rx.recv().await {
                let keep = this.update(cx, |this, cx| {
                    let log = &mut this.settings.update.log;
                    log.push(line);
                    if log.len() > LOG_LINES {
                        log.remove(0);
                    }
                    cx.notify();
                });
                if keep.is_err() {
                    return;
                }
            }
            // The channel closes when the job ends.
            let outcome = job.join().unwrap_or_else(|_| Err("The update crashed; your current ArchonUI is unchanged".into()));
            let _ = this.update(cx, |this, cx| {
                this.settings.update.running = false;
                if outcome.is_ok() {
                    this.settings.update.status = Some(Status::UpToDate);
                }
                this.settings.update.outcome = Some(outcome);
                cx.notify();
            });
        })
        .detach();
    }

    /// Starts the installed binary a moment after this one quits, then quits.
    fn restart_now(&mut self, bin: PathBuf, cx: &mut Context<Self>) {
        use std::os::unix::process::CommandExt;
        let started = std::process::Command::new("sh")
            .args(["-c", "sleep 1; exec \"$0\""])
            .arg(&bin)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .process_group(0)
            .spawn();
        match started {
            Ok(_) => {
                self.saver.flush();
                cx.quit();
            }
            Err(e) => self.toast(format!("Couldn't restart ({e}). Quit and open ArchonUI again"), cx),
        }
    }

    /// The Updates rows: version and status with the action button, then progress or the command to run.
    pub(super) fn update_rows(&mut self, cx: &mut Context<Self>) -> Vec<SettingRow> {
        let theme = self.theme.clone();
        let u = &self.settings.update;
        let commit = &update::BUILD_COMMIT[..update::BUILD_COMMIT.len().min(7)];
        let version = if commit.is_empty() {
            format!("Version {}", env!("CARGO_PKG_VERSION"))
        } else {
            format!("Version {} · {commit}", env!("CARGO_PKG_VERSION"))
        };
        let source = source();
        let status_text = if u.running {
            "Updating…".to_string()
        } else if u.checking {
            "Checking GitHub…".to_string()
        } else {
            match (&u.outcome, &u.status) {
                (Some(Ok(_)), _) => "Updated. Restart to use the new version".to_string(),
                (Some(Err(_)), _) => "The update stopped. Your current ArchonUI keeps working".to_string(),
                (None, Some(status)) => status.summary(),
                (None, None) => "Not checked yet".to_string(),
            }
        };
        let available = u.status.as_ref().is_some_and(Status::update_available);
        let control = match (&u.outcome, &source) {
            (Some(Ok(bin)), _) => {
                let bin = bin.clone();
                Button::new("update-restart", "Restart now", &theme)
                    .on_click(listen(cx, move |this, cx| this.restart_now(bin.clone(), cx)))
                    .into_any_element()
            }
            _ if u.running || u.checking => Button::new("update-busy", "Working…", &theme)
                .style(ButtonStyle::Outline)
                .disabled("Wait for it to finish")
                .into_any_element(),
            (_, Source::Checkout { .. }) if available => Button::new("update-run", "Update now", &theme)
                .on_click(listen(cx, |this, cx| this.start_update(cx)))
                .into_any_element(),
            (_, Source::Elsewhere { .. }) if available => Button::new("update-copy", "Copy command", &theme)
                .style(ButtonStyle::Outline)
                .on_click(listen(cx, |this, cx| {
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(update::INSTALL_COMMAND.to_string()));
                    this.toast("Copied. Run it in a terminal", cx);
                }))
                .into_any_element(),
            _ => Button::new("update-check", "Check for updates", &theme)
                .style(ButtonStyle::Outline)
                .on_click(listen(cx, |this, cx| this.check_updates(cx)))
                .into_any_element(),
        };
        let mut row = SettingRow::new(version, status_text, &theme).control(control);

        let muted = theme.muted.to_hsla();
        let mono = theme.code_font.clone();
        let lines: Vec<String> = match (&u.outcome, &source) {
            (Some(Err(why)), _) => why.lines().map(str::to_string).collect(),
            _ if u.running || !u.log.is_empty() => u.log.clone(),
            (_, Source::Elsewhere { .. }) if available => vec![
                "Not installed with install.sh, so ArchonUI can't update itself. To update, run:".to_string(),
                update::INSTALL_COMMAND.to_string(),
            ],
            _ => Vec::new(),
        };
        if !lines.is_empty() {
            let detail = div().flex().flex_col().pt(px(6.)).gap(px(2.)).children(lines.into_iter().map(|line| {
                div()
                    .text_size(px(11.5))
                    .text_color(muted)
                    .font_family(mono.clone())
                    .overflow_hidden()
                    .text_ellipsis()
                    .whitespace_nowrap()
                    .child(line)
            }));
            row = row.detail(detail);
        }
        vec![row]
    }
}
