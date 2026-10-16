//! The approval card: a tool use the agent wants to run, shown in its chat above the composer until the user
//! answers. Requests come from `engine::approve`. Enter in the composer allows (see `allow_first_approval`);
//! Esc denies the request in the focused chat.

use super::{
    shell::Shell,
    widgets::{
        button::{Button, ButtonStyle},
        kbd::Kbd,
    },
};
use crate::engine::approve::{self, Allowances, Request, Verdict};
use gpui::{div, prelude::*, AnyElement, App, Context, KeyDownEvent, Window};
use crate::ui::scale::px;
use serde_json::Value;
use std::{collections::VecDeque, time::Duration};

/// What the agent is told when the user denies a call.
pub(super) const DENIED: &str = "The user denied this action.";
/// Longest input shown on the card, in characters.
const SUMMARY_CHARS: usize = 400;

/// Approval requests waiting for an answer, across chats, oldest first.
#[derive(Default)]
pub struct ApprovalUi {
    pub(super) queue: VecDeque<Pending>,
    next_id: u64,
    allowances: Allowances,
}

pub(super) struct Pending {
    pub(super) id: u64,
    pub(super) request: Request,
}

/// Starts answering approval requests on this process's socket. Requests reach the shell as they arrive.
pub(super) fn start(cx: &mut Context<Shell>) -> ApprovalUi {
    let (requests, incoming) = smol::channel::unbounded();
    let allowances = Allowances::default();
    if let Err(e) = approve::listen(approve::socket(), requests, allowances.clone(), approve::TIMEOUT) {
        eprintln!("could not listen for approvals on {}: {e}", approve::socket().display());
    }
    cx.spawn(async move |this, cx| {
        while let Ok(request) = incoming.recv().await {
            if this.update(cx, |this, cx| this.approval_arrived(request, cx)).is_err() {
                break;
            }
        }
    })
    .detach();
    ApprovalUi { allowances, ..Default::default() }
}

impl Shell {
    /// A request from one of this device's own chats: queued, and shown to the devices following the chat.
    fn approval_arrived(&mut self, request: Request, cx: &mut Context<Self>) {
        if self.state.session(&request.session).is_none() {
            request.answer(Verdict::Deny("Unknown session.".into()));
            return;
        }
        let (session, tool, input) = (request.session.clone(), request.tool_name.clone(), request.input.clone());
        let id = self.queue_approval(request, cx);
        self.share_approval(&session, id, &tool, &input);
    }

    /// Queues a request, whatever its chat, and returns its id. The listener denies it after `approve::TIMEOUT`;
    /// the card goes a second later, so the listener's "Timed out" is the answer the agent gets.
    pub(super) fn queue_approval(&mut self, request: Request, cx: &mut Context<Self>) -> u64 {
        let id = self.approvals.next_id;
        self.approvals.next_id += 1;
        self.approvals.queue.push_back(Pending { id, request });
        cx.spawn(async move |this, cx| {
            smol::Timer::after(approve::TIMEOUT + Duration::from_secs(1)).await;
            let _ = this.update(cx, |this, cx| {
                if let Some(at) = this.approvals.queue.iter().position(|p| p.id == id) {
                    if let Some(gone) = this.approvals.queue.remove(at) {
                        this.share_approval_done(&gone.request.session, id);
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
        id
    }

    /// Answers request `id`. With `always`, the tool is also allowed for the rest of the chat's session, and
    /// the other waiting requests for that tool in the chat get the same answer. A request from another
    /// device is not allowed here: its host applies `always`.
    pub(super) fn answer_approval(&mut self, id: u64, verdict: Verdict, always: bool, cx: &mut Context<Self>) {
        let Some(at) = self.approvals.queue.iter().position(|p| p.id == id) else { return };
        let Some(pending) = self.approvals.queue.remove(at) else { return };
        if always {
            let (session, tool) = (&pending.request.session, &pending.request.tool_name);
            match self.remote.forwarded.get_mut(&id) {
                Some(forwarded) => forwarded.always = true,
                None => self.approvals.allowances.allow(session, tool),
            }
            let (same, rest): (Vec<Pending>, Vec<Pending>) = self
                .approvals
                .queue
                .drain(..)
                .partition(|p| &p.request.session == session && &p.request.tool_name == tool);
            self.approvals.queue = rest.into();
            for p in same {
                self.share_approval_done(&p.request.session, p.id);
                p.request.answer(Verdict::Allow);
            }
        }
        self.share_approval_done(&pending.request.session, id);
        pending.request.answer(verdict);
        cx.notify();
    }

    /// The turn in chat `session` ended, so its waiting requests are denied: the agent that asked is gone.
    pub(super) fn end_approvals(&mut self, session: &str) {
        let (ended, rest): (Vec<Pending>, Vec<Pending>) =
            self.approvals.queue.drain(..).partition(|p| p.request.session == session);
        self.approvals.queue = rest.into();
        for p in ended {
            self.share_approval_done(session, p.id);
            p.request.answer(Verdict::Deny("The chat stopped before an answer.".into()));
        }
    }

    /// Allows the first request waiting in chat `session`. Enter in its composer calls this before sending,
    /// because the composer's input takes Enter before the shell sees it. Returns true when a request was waiting.
    pub(super) fn allow_first_approval(&mut self, session: &str, cx: &mut Context<Self>) -> bool {
        let Some(id) = self.approvals.queue.iter().find(|p| p.request.session == session).map(|p| p.id) else {
            return false;
        };
        self.answer_approval(id, Verdict::Allow, false, cx);
        true
    }

    /// Esc denies the request shown in the focused chat. Returns true when consumed.
    pub(super) fn approval_key(&mut self, ev: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) -> bool {
        let mods = ev.keystroke.modifiers;
        if mods.shift || mods.alt || mods.control || mods.platform || ev.keystroke.key != "escape" {
            return false;
        }
        let Some(session) = self.state.workspace.layout.focus.clone() else { return false };
        let Some(id) = self.approvals.queue.iter().find(|p| p.request.session == session).map(|p| p.id) else {
            return false;
        };
        self.answer_approval(id, Verdict::Deny(DENIED.into()), false, cx);
        true
    }

    /// The card for chat `session`: its first waiting request, or nothing.
    pub(super) fn render_approval(&self, session: &str, cx: &mut Context<Self>) -> AnyElement {
        let theme = &self.theme;
        let waiting = self.approvals.queue.iter().filter(|p| p.request.session == session).count();
        let Some(pending) = self.approvals.queue.iter().find(|p| p.request.session == session) else {
            return div().into_any_element();
        };
        let (id, tool) = (pending.id, pending.request.tool_name.clone());
        let shell = cx.entity();
        let allow = {
            let shell = shell.clone();
            move |_: &mut Window, cx: &mut App| shell.update(cx, |this, cx| this.answer_approval(id, Verdict::Allow, false, cx))
        };
        let deny = {
            let shell = shell.clone();
            move |_: &mut Window, cx: &mut App| {
                shell.update(cx, |this, cx| this.answer_approval(id, Verdict::Deny(DENIED.into()), false, cx))
            }
        };
        let always = move |_: &mut Window, cx: &mut App| {
            shell.update(cx, |this, cx| this.answer_approval(id, Verdict::Allow, true, cx))
        };
        div()
            .flex_none()
            .flex()
            .flex_col()
            .gap(px(8.))
            .mx(px(12.))
            .mb(px(8.))
            .p(px(12.))
            .rounded(px(8.))
            .border_1()
            .border_color(theme.line2.to_hsla())
            .bg(theme.line.to_hsla())
            .text_size(px(12.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(div().text_color(theme.acc2.to_hsla()).child("▸"))
                    .child(div().text_color(theme.muted.to_hsla()).child("Claude wants to run:"))
                    .child(div().text_color(theme.fg.to_hsla()).child(tool.clone()))
                    .when(waiting > 1, |row| {
                        row.child(div().ml_auto().text_color(theme.faint.to_hsla()).child(format!("1 of {waiting}")))
                    }),
            )
            .child(
                div()
                    .max_h(px(120.))
                    .overflow_hidden()
                    .font_family(theme.code_font.clone())
                    .text_color(theme.alt.to_hsla())
                    .child(summary(&tool, &pending.request.input)),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        Button::new(("approval-allow", id), "Allow", theme)
                            .style(ButtonStyle::Filled)
                            .on_click(allow),
                    )
                    .child(Kbd::new("Enter", theme))
                    .child(Button::new(("approval-deny", id), "Deny", theme).on_click(deny))
                    .child(Kbd::new("Esc", theme))
                    .child(
                        Button::new(
                            ("approval-always", id),
                            format!("Always allow {tool} for this session"),
                            theme,
                        )
                        .style(ButtonStyle::Ghost)
                        .on_click(always),
                    ),
            )
            .into_any_element()
    }
}

/// The part of a tool's input a person needs to judge it: the command, path, URL or pattern. Other inputs
/// show as JSON.
fn summary(tool: &str, input: &Value) -> String {
    let key = match tool {
        "Bash" => "command",
        "Read" | "Edit" | "Write" | "MultiEdit" => "file_path",
        "NotebookEdit" => "notebook_path",
        "WebFetch" => "url",
        "WebSearch" => "query",
        "Grep" | "Glob" => "pattern",
        _ => "",
    };
    let text = match input.get(key).and_then(Value::as_str) {
        Some(text) => text.to_string(),
        None => input.to_string(),
    };
    if text.chars().count() > SUMMARY_CHARS {
        let cut: String = text.chars().take(SUMMARY_CHARS).collect();
        format!("{cut}…")
    } else {
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn summary_shows_the_part_a_person_judges() {
        assert_eq!(summary("Bash", &json!({"command": "rm -rf build", "description": "clean"})), "rm -rf build");
        assert_eq!(summary("Edit", &json!({"file_path": "/src/a.rs", "old_string": "x"})), "/src/a.rs");
        assert_eq!(summary("WebFetch", &json!({"url": "https://example.com"})), "https://example.com");
        assert_eq!(summary("Mystery", &json!({"a": 1})), r#"{"a":1}"#);
    }

    #[test]
    fn summary_cuts_long_input() {
        let long = "x".repeat(SUMMARY_CHARS + 5);
        let shown = summary("Bash", &json!({ "command": long }));
        assert_eq!(shown.chars().count(), SUMMARY_CHARS + 1);
        assert!(shown.ends_with('…'));
    }
}
