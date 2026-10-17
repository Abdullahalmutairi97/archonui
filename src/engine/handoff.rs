//! Handing a chat to another agent. Agents keep their own sessions and can't read each other's, so
//! the first message after a switch carries the conversation so far as text.

use super::agent_name;
use crate::store::{Message, Role};

/// Most characters of earlier conversation sent. The newest messages are kept.
const MAX_CHARS: usize = 60_000;
/// Most characters kept from one message; longer ones keep their start and end.
const MAX_MESSAGE_CHARS: usize = 6_000;
/// Most tool calls listed under one reply.
const MAX_TOOLS: usize = 20;

/// `prompt` with the conversation in `messages` in front of it, for an agent that takes over the chat.
/// `messages` is what was said before `prompt`. Error rows and empty replies are left out.
pub fn prompt(messages: &[Message], prompt: &str) -> String {
    prompt_from(messages, prompt, None)
}

/// `prompt`, for a chat continued here from `device` (its name) when set: its files are on that
/// device, so the agent is not told they are already changed on this disk.
pub fn prompt_from(messages: &[Message], prompt: &str, device: Option<&str>) -> String {
    let mut parts: Vec<String> = messages.iter().filter_map(entry).collect();
    let mut omitted = 0;
    while parts.len() > 1 && parts.iter().map(|p| p.len() + 2).sum::<usize>() > MAX_CHARS {
        parts.remove(0);
        omitted += 1;
    }
    if parts.is_empty() {
        return prompt.to_string();
    }
    let mut out = match device {
        None => String::from(
            "This chat was started with another coding agent and has been handed to you. Here is the \
             conversation so far, oldest first. Files it changed are already changed on disk.\n\n<conversation>\n",
        ),
        Some(device) => format!(
            "This chat ran on another device ({device}) and continues here, on this one. Here is the \
             conversation so far, oldest first. Files it changed were changed on {device}, so this \
             folder may not have those changes.\n\n<conversation>\n"
        ),
    };
    if omitted > 0 {
        out.push_str(&format!("({omitted} earlier messages left out)\n\n"));
    }
    out.push_str(&parts.join("\n\n"));
    out.push_str("\n</conversation>\n\nContinue from there. The user's new message:\n\n");
    out.push_str(prompt);
    out
}

/// One message as `[Speaker]` and its text, or `None` for rows the new agent doesn't need.
fn entry(m: &Message) -> Option<String> {
    let speaker = match m.role {
        Role::User => "User".to_string(),
        Role::Agent => m.agent.as_deref().map_or("Agent", agent_name).to_string(),
        Role::Error => return None,
    };
    let mut body = clip(m.text.trim());
    if !m.attachments.is_empty() {
        let names: Vec<&str> = m.attachments.iter().map(|a| a.name.as_str()).collect();
        body.push_str(&format!("\n(attached: {})", names.join(", ")));
    }
    if !m.tools.is_empty() {
        let mut lines: Vec<String> = m
            .tools
            .iter()
            .take(MAX_TOOLS)
            .map(|t| {
                let kind = format!("{:?}", t.kind).to_lowercase();
                let failed = if t.ok == Some(false) { " (failed)" } else { "" };
                format!("- {kind} {}{failed}", t.target)
            })
            .collect();
        if m.tools.len() > MAX_TOOLS {
            lines.push(format!("- … {} more", m.tools.len() - MAX_TOOLS));
        }
        body = format!("(tools used:\n{})\n{body}", lines.join("\n"));
    }
    let body = body.trim();
    (!body.is_empty()).then(|| format!("[{speaker}]\n{body}"))
}

/// `text` cut to `MAX_MESSAGE_CHARS`, keeping its start and end.
fn clip(text: &str) -> String {
    let n = text.chars().count();
    if n <= MAX_MESSAGE_CHARS {
        return text.to_string();
    }
    let half = MAX_MESSAGE_CHARS / 2;
    let head: String = text.chars().take(half).collect();
    let tail: String = text.chars().skip(n - half).collect();
    format!("{head}\n… [{} characters cut] …\n{tail}", n - 2 * half)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{ToolCall, ToolKind};

    fn msg(role: Role, text: &str, agent: Option<&str>) -> Message {
        Message {
            role,
            text: text.into(),
            attachments: Vec::new(),
            tools: Vec::new(),
            thought_secs: None,
            took_secs: None,
            agent: agent.map(str::to_string),
            final_at: None,
            usage: None,
            todos: Vec::new(),
        }
    }

    #[test]
    fn carries_the_conversation_labelled_by_speaker_before_the_prompt() {
        let mut reply = msg(Role::Agent, "Done.", Some("claude"));
        reply.tools.push(ToolCall { kind: ToolKind::Edit, target: "src/a.rs".into(), detail: None, ok: Some(true) });
        let history = [msg(Role::User, "fix a.rs", None), reply, msg(Role::Error, "boom", None)];
        let p = prompt(&history, "now test it");
        assert!(p.contains("[User]\nfix a.rs"), "{p}");
        assert!(p.contains("[Claude Code]\n(tools used:\n- edit src/a.rs)\nDone."), "{p}");
        assert!(!p.contains("boom"), "error rows are left out");
        assert!(p.ends_with("The user's new message:\n\nnow test it"), "{p}");
    }

    #[test]
    fn a_chat_from_another_device_is_not_said_to_have_changed_files_here() {
        let history = [msg(Role::User, "fix a.rs", None), msg(Role::Agent, "Done.", Some("claude"))];
        let p = prompt_from(&history, "now test it", Some("pc"));
        assert!(p.contains("another device (pc)"), "{p}");
        assert!(!p.contains("already changed on disk"), "{p}");
        assert!(p.contains("[User]\nfix a.rs") && p.ends_with("now test it"), "{p}");
        assert!(prompt(&history, "x").contains("already changed on disk"));
    }

    #[test]
    fn nothing_to_hand_over_leaves_the_prompt_alone() {
        assert_eq!(prompt(&[], "hi"), "hi");
        assert_eq!(prompt(&[msg(Role::Agent, "  ", Some("codex"))], "hi"), "hi");
    }

    #[test]
    fn long_chats_keep_the_newest_messages() {
        let big = "x".repeat(MAX_MESSAGE_CHARS);
        let history: Vec<Message> = (0..20).map(|i| msg(Role::User, &format!("{i}{big}"), None)).collect();
        let p = prompt(&history, "go");
        assert!(p.len() < MAX_CHARS + 2_000, "{}", p.len());
        assert!(p.contains("earlier messages left out"));
        assert!(p.contains("19x"), "the newest message is kept");
        assert!(!p.contains("\n0x"), "the oldest is dropped");
    }

    #[test]
    fn a_long_message_keeps_its_start_and_end() {
        let text = format!("start{}end", "y".repeat(MAX_MESSAGE_CHARS * 2));
        let clipped = clip(&text);
        assert!(clipped.starts_with("start") && clipped.ends_with("end"));
        assert!(clipped.contains("characters cut"));
    }
}
