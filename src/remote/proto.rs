//! The messages a host and a client exchange: one JSON object per WebSocket text frame,
//! tagged by `"type"`.
//!
//! A client says `Hello` first and the host answers `Welcome`. After that the client may list
//! chats, subscribe to one (the host sends a `Snapshot`, then every `Event` of its turns), send
//! prompts and stop turns. Attachments are not sent yet.

use crate::engine::{Mode, Out};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::path::PathBuf;

/// Bumped on any incompatible change. A host refuses a client with a different version.
pub const VERSION: u32 = 1;

/// Client to host.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMsg {
    /// First message. `device` is the client's Tailscale host name, for display.
    Hello { device: String, version: u32 },
    /// Ask for `Sessions`.
    ListSessions,
    /// Send a `Snapshot` of the chat now and stream its events from here on.
    Subscribe { session: String },
    Unsubscribe { session: String },
    /// A new chat on the host. The host answers `Created`.
    NewSession { agent: String, cwd: PathBuf, project: Option<String> },
    /// Send a prompt in a chat. `model`, `effort` and `mode` replace the chat's when given.
    Prompt {
        session: String,
        text: String,
        #[serde(default)]
        mode: Option<Mode>,
        #[serde(default)]
        model: Option<String>,
        #[serde(default)]
        effort: Option<String>,
    },
    /// Stop the running turn in a chat (Esc).
    Cancel { session: String },
    /// Keep the composer draft in step across devices.
    Draft { session: String, text: String },
}

/// One chat in the host's list.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SessionSummary {
    pub id: String,
    pub title: String,
    /// Registry id of the agent, e.g. `"claude"`.
    pub agent: String,
    pub project: Option<String>,
    pub cwd: PathBuf,
    /// Last activity, seconds since the epoch.
    pub updated: u64,
    /// A turn is running on the host now.
    pub busy: bool,
    pub messages: usize,
}

/// Host to client.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HostMsg {
    /// Answer to `Hello`. `device` is the host's Tailscale host name.
    Welcome { device: String, version: u32 },
    /// Every visible chat on the host, newest first.
    Sessions { sessions: Vec<SessionSummary> },
    /// The whole chat, as `store::Session` serializes, so the client can show its history.
    Snapshot { session: serde_json::Value, busy: bool },
    /// A chat was created by `NewSession`.
    Created { session: SessionSummary },
    /// A prompt was accepted and a turn started (also sent for prompts from other devices).
    Started { session: String, text: String },
    /// One event of a running turn.
    Event { session: String, out: Out },
    /// The turn ended. `error` is set when the agent failed or could not start.
    Finished { session: String, error: Option<String> },
    /// The chat's draft changed on another device.
    Draft { session: String, text: String },
    /// Something the client asked for failed, e.g. an unknown session id.
    Error { message: String },
}

pub fn encode<T: Serialize>(msg: &T) -> String {
    serde_json::to_string(msg).expect("protocol messages always serialize")
}

pub fn decode<T: DeserializeOwned>(text: &str) -> Result<T, String> {
    serde_json::from_str(text).map_err(|e| format!("bad message: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::ToolKind;

    #[test]
    fn client_messages_round_trip() {
        let msgs = [
            ClientMsg::Hello { device: "laptop".into(), version: VERSION },
            ClientMsg::ListSessions,
            ClientMsg::Prompt { session: "s1".into(), text: "hi".into(), mode: Some(Mode::Plan), model: None, effort: None },
            ClientMsg::Cancel { session: "s1".into() },
        ];
        for m in msgs {
            assert_eq!(decode::<ClientMsg>(&encode(&m)).unwrap(), m);
        }
    }

    #[test]
    fn host_events_round_trip() {
        let msgs = [
            HostMsg::Event { session: "s1".into(), out: Out::Text("Hel".into()) },
            HostMsg::Event { session: "s1".into(), out: Out::ToolStart { id: "t1".into(), kind: ToolKind::Edit, target: "a.rs".into() } },
            HostMsg::Event { session: "s1".into(), out: Out::Thinking(true) },
            HostMsg::Finished { session: "s1".into(), error: None },
        ];
        for m in msgs {
            assert_eq!(decode::<HostMsg>(&encode(&m)).unwrap(), m);
        }
    }

    #[test]
    fn wire_format_is_tagged() {
        assert_eq!(encode(&ClientMsg::ListSessions), r#"{"type":"list_sessions"}"#);
        let p: ClientMsg = decode(r#"{"type":"prompt","session":"s","text":"x"}"#).unwrap();
        assert_eq!(p, ClientMsg::Prompt { session: "s".into(), text: "x".into(), mode: None, model: None, effort: None });
    }
}
