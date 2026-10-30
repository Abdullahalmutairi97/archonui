//! A developer switch for demo recordings: `ARCHONUI_AUTOPILOT=<script>` drives the app from a timed
//! script, so a screen recording shows the real app doing real work without anyone at the keyboard.
//! Each line is `<at> <command> [args]`: `at` is seconds from the start, or `+s` after the step before.
//! Lines starting with `#` are comments. Every step is logged, with its time, to
//! `ARCHONUI_AUTOPILOT_LOG`, so sound effects can be laid on the recording later.
//!
//! Commands: `new`, `split`, `agent <id>`, `type <text>` (typed into the focused chat, a key at a time),
//! `send`, `idle [max s]` (waits for every agent to finish), `follow on|off`, `panel <tab>|off`,
//! `url <address>` (typed into the browser's address bar, then opened), `wheel <dy>`, `theme next|<id>`,
//! `settings <page>|close`, `sidebar sessions|projects`, `focus next|prev`, `open <title>`, `opensplit <title>`,
//! `quit`.

use super::shell::Shell;
use crate::app::{settings::Action, workspace::{LeftTab, RightTab}};
use gpui::{AsyncApp, Context, WeakEntity};
use std::{
    io::Write,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

/// One step of the script.
#[derive(Debug, Clone, PartialEq)]
struct Step {
    /// Seconds from the start, or after the previous step when `relative`.
    at: f64,
    relative: bool,
    command: String,
    arg: String,
}

fn parse(script: &str) -> Vec<Step> {
    script
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|line| {
            let (at, rest) = line.split_once(char::is_whitespace)?;
            let (command, arg) = rest.trim().split_once(char::is_whitespace).map_or((rest.trim(), ""), |(c, a)| (c, a.trim()));
            let relative = at.starts_with('+');
            let at: f64 = at.trim_start_matches('+').parse().ok()?;
            Some(Step { at, relative, command: command.to_string(), arg: arg.to_string() })
        })
        .collect()
}

/// Seconds between typed keys: a steady typist with a little unevenness, the same every run.
fn key_gap(i: usize) -> f64 {
    let wobble = ((i * 7919) % 13) as f64 / 13.;
    0.034 + wobble * 0.03
}

struct Log {
    file: Option<std::fs::File>,
    start: Instant,
}

impl Log {
    fn line(&mut self, what: &str) {
        let t = self.start.elapsed().as_secs_f64();
        if let Some(f) = &mut self.file {
            let _ = writeln!(f, "{t:.3} {what}");
        }
    }
}

impl Shell {
    /// Starts the script in `ARCHONUI_AUTOPILOT`, if set.
    pub(super) fn autopilot(&mut self, cx: &mut Context<Self>) {
        let Ok(path) = std::env::var("ARCHONUI_AUTOPILOT") else { return };
        let Ok(script) = std::fs::read_to_string(&path) else { return };
        let steps = parse(&script);
        let file = std::env::var("ARCHONUI_AUTOPILOT_LOG").ok().and_then(|p| std::fs::File::create(p).ok());
        cx.spawn(async move |this, cx| run(this, steps, file, cx).await).detach();
    }
}

async fn wait(cx: &mut AsyncApp, secs: f64) {
    if secs > 0. {
        cx.background_executor().timer(Duration::from_secs_f64(secs)).await;
    }
}

/// Whether any chat has a turn running.
fn any_busy(this: &WeakEntity<Shell>, cx: &mut AsyncApp) -> bool {
    this.update(cx, |this, _| this.state.sessions().iter().any(|s| this.state.busy(&s.id))).unwrap_or(false)
}

async fn run(this: WeakEntity<Shell>, steps: Vec<Step>, file: Option<std::fs::File>, cx: &mut AsyncApp) {
    let mut log = Log { file, start: Instant::now() };
    let epoch = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0., |d| d.as_secs_f64());
    log.line(&format!("start epoch={epoch:.3}"));
    let mut previous = 0.;
    for step in steps {
        let due = if step.relative { previous + step.at } else { step.at };
        wait(cx, due - log.start.elapsed().as_secs_f64()).await;
        let (command, arg) = (step.command, step.arg);
        match command.as_str() {
            "type" => {
                log.line(&format!("type-start {}", arg.chars().count()));
                let mut typed = String::new();
                for (i, c) in arg.chars().enumerate() {
                    typed.push(c);
                    let text = typed.clone();
                    let ok = this.update(cx, |this, cx| {
                        let Some(id) = this.state.workspace.layout.focus.clone() else { return };
                        this.set_draft(&id, &text, cx);
                    });
                    if ok.is_err() {
                        return;
                    }
                    log.line(&format!("key {c}"));
                    wait(cx, key_gap(i)).await;
                }
                log.line("type-end");
            }
            "url" => {
                let mut typed = String::new();
                for (i, c) in arg.chars().enumerate() {
                    typed.push(c);
                    let text = typed.clone();
                    let _ = this.update(cx, |this, cx| this.autopilot_address(&text, false, cx));
                    log.line(&format!("key {c}"));
                    wait(cx, key_gap(i)).await;
                }
                let _ = this.update(cx, |this, cx| this.autopilot_address(&arg, true, cx));
                log.line("enter");
            }
            "idle" => {
                let max: f64 = arg.parse().unwrap_or(240.);
                let began = Instant::now();
                // A turn takes a moment to start: wait for it to be running first, then for it to end.
                while began.elapsed().as_secs_f64() < 10. && !any_busy(&this, cx) {
                    wait(cx, 0.1).await;
                }
                loop {
                    if !any_busy(&this, cx) || began.elapsed().as_secs_f64() > max {
                        break;
                    }
                    wait(cx, 0.2).await;
                }
                log.line("idle");
            }
            "quit" => {
                log.line("quit");
                let _ = cx.update(|cx| cx.quit());
                return;
            }
            _ => {
                let done = this.update(cx, |this, cx| this.autopilot_step(&command, &arg, cx));
                if done.is_err() {
                    return;
                }
                log.line(&format!("{command} {arg}"));
            }
        }
        previous = log.start.elapsed().as_secs_f64();
    }
}

impl Shell {
    fn autopilot_step(&mut self, command: &str, arg: &str, cx: &mut Context<Self>) {
        let focused = self.state.workspace.layout.focus.clone();
        match command {
            "new" => self.run_action(Action::NewSession, cx),
            "split" => self.run_action(Action::SplitNewSession, cx),
            "agent" => {
                if let Some(id) = focused {
                    self.switch_agent(&id, arg, cx);
                }
            }
            "send" => {
                if let Some(id) = focused {
                    self.composer_enter(&id, cx);
                }
            }
            "follow" => self.set_follow(arg != "off", cx),
            "panel" => match arg {
                "off" => {
                    self.state.workspace.right_open = false;
                    self.widths_changed(false, cx);
                }
                tab => {
                    let tab = match tab {
                        "browser" => RightTab::Browser,
                        "files" => RightTab::Files,
                        "git" => RightTab::Git,
                        "terminal" => RightTab::Terminal,
                        _ => RightTab::Ide,
                    };
                    self.show_tool_tab(tab, cx);
                }
            },
            "wheel" => self.autopilot_wheel(arg.parse().unwrap_or(400.)),
            "theme" => {
                if arg == "next" {
                    self.run_action(Action::NextTheme, cx);
                } else {
                    self.state.settings.theme = arg.to_string();
                    self.refresh_theme(cx);
                }
            }
            "settings" => {
                if arg == "close" {
                    self.settings_close(cx);
                } else {
                    if !self.settings.open {
                        self.settings_open(cx);
                    }
                    if let Some(page) = super::settings::Page::from_label(arg) {
                        self.settings.page = page;
                    }
                    cx.notify();
                }
            }
            "sidebar" => {
                self.state.workspace.left_tab = if arg == "sessions" { LeftTab::Sessions } else { LeftTab::Projects };
                cx.notify();
            }
            "open" | "opensplit" => {
                let wanted = arg.to_lowercase();
                let Some(id) = self.state.sessions().iter().find(|s| s.title.to_lowercase().contains(&wanted)).map(|s| s.id.clone()) else { return };
                let split = command == "opensplit";
                match self.tiles.area {
                    Some((area, gap)) if split => {
                        if !self.state.workspace.layout.open_in_new_split(&id, area, gap) {
                            self.state.workspace.layout.open_in_focused(&id);
                        }
                    }
                    _ => self.state.workspace.layout.open_in_focused(&id),
                }
                self.save(cx);
            }
            "focus" => self.run_action(if arg == "prev" { Action::FocusPrev } else { Action::FocusNext }, cx),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scripts_read_times_commands_and_text() {
        let steps = parse("# demo\n0 new\n+1.5 type Add a heat shimmer\n 3 theme next\nnot-a-time x\n");
        assert_eq!(steps.len(), 3);
        assert_eq!(steps[1], Step { at: 1.5, relative: true, command: "type".into(), arg: "Add a heat shimmer".into() });
        assert_eq!((steps[2].at, steps[2].relative, steps[2].arg.as_str()), (3., false, "next"));
    }
}
