//! The messages a host and a client exchange: one JSON object per WebSocket text frame,
//! tagged by `"type"`.
//!
//! A client says `Hello` first and the host answers `Welcome`. After that the client may list
//! chats, subscribe to one (the host sends a `Snapshot`, then every `Event` of its turns), send
//! prompts (with files) and stop turns.

use crate::engine::{Mode, Out};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::path::PathBuf;

/// Bumped on any incompatible change. A host refuses a client with a different version.
pub const VERSION: u32 = 1;
/// Most file bytes one prompt may carry, before base64. Stays under the 64 MiB frame limit once encoded.
pub const MAX_REMOTE_FILES_BYTES: u64 = 40 * 1024 * 1024;
/// Largest message, and so frame, either side reads. tungstenite sends each message as one frame, so
/// the frame limit (16 MiB by default) must be as large as the message limit, or a prompt with files
/// well under `MAX_REMOTE_FILES_BYTES` drops the connection.
pub const MAX_MESSAGE_BYTES: usize = 64 * 1024 * 1024;

/// The WebSocket limits both a host and a client use.
pub fn ws_config() -> tungstenite::protocol::WebSocketConfig {
    tungstenite::protocol::WebSocketConfig::default()
        .max_message_size(Some(MAX_MESSAGE_BYTES))
        .max_frame_size(Some(MAX_MESSAGE_BYTES))
}

/// A file sent with a prompt.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RemoteFile {
    pub name: String,
    /// base64 of the bytes
    pub data: String,
}

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
    /// `files` are saved in the chat on the host and sent with the prompt.
    Prompt {
        session: String,
        text: String,
        #[serde(default)]
        mode: Option<Mode>,
        #[serde(default)]
        model: Option<String>,
        #[serde(default)]
        effort: Option<String>,
        #[serde(default)]
        files: Vec<RemoteFile>,
    },
    /// Stop the running turn in a chat (Esc).
    Cancel { session: String },
    /// Keep the composer draft in step across devices.
    Draft { session: String, text: String },
    /// Cloud: store a snapshot of a project. `bundle` is a base64 git bundle holding
    /// `refs/archon/wip/<device>` at `commit` (incremental: only what the cloud lacks).
    PutProject { project: String, name: String, device: String, commit: String, bundle: String },
    /// Cloud: replace the stored copy of `device`'s chats (each a `store::Session` as JSON).
    PutChats { device: String, sessions: Vec<serde_json::Value> },
    /// Cloud: list stored projects and which devices have chats stored.
    ListCloud,
    /// Cloud: send the latest snapshot of `project` from `device` as a full bundle.
    GetProject { project: String, device: String },
    /// Cloud: send `device`'s stored chats.
    GetChats { device: String },
    /// Answer to an `Approval`: allow or deny the tool use `request` in chat `session`. `always` also
    /// allows the tool for the rest of the chat's session.
    Answer { session: String, request: u64, allow: bool, always: bool },
}

/// A project snapshot kept on the cloud device.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CloudProject {
    /// The project's id on the device that sent it.
    pub project: String,
    pub name: String,
    /// Device id that sent the snapshot.
    pub device: String,
    pub commit: String,
    /// When it was stored, seconds since the epoch.
    pub updated: u64,
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
    /// The turn ended. `error` is set when the agent failed or could not start. `stopped`: someone
    /// stopped it, so `error` is only the signal that ended the agent.
    Finished {
        session: String,
        error: Option<String>,
        #[serde(default)]
        stopped: bool,
    },
    /// The chat's draft changed on another device.
    Draft { session: String, text: String },
    /// Something the client asked for failed, e.g. an unknown session id.
    Error { message: String },
    /// Cloud: a `PutProject` was stored.
    Stored { project: String, device: String, commit: String },
    /// Cloud: answer to `ListCloud`. `chats` lists device ids with chats stored.
    Cloud { projects: Vec<CloudProject>, chats: Vec<String> },
    /// Cloud: answer to `GetProject`; a full base64 git bundle of `refs/archon/wip/<device>`.
    ProjectData { project: String, device: String, commit: String, bundle: String },
    /// Cloud: answer to `GetChats`.
    Chats { device: String, sessions: Vec<serde_json::Value> },
    /// A tool use in chat `session` waits for an answer. `request` is the host's id for it.
    Approval { session: String, request: u64, tool: String, input: serde_json::Value },
    /// Request `request` in chat `session` was answered or dropped, so clients clear its card.
    ApprovalDone { session: String, request: u64 },
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
            ClientMsg::Prompt { session: "s1".into(), text: "hi".into(), mode: Some(Mode::Plan), model: None, effort: None, files: Vec::new() },
            ClientMsg::Cancel { session: "s1".into() },
            ClientMsg::PutProject { project: "p".into(), name: "app".into(), device: "n1".into(), commit: "abc".into(), bundle: "AAA=".into() },
            ClientMsg::GetChats { device: "n1".into() },
            ClientMsg::Answer { session: "s1".into(), request: 3, allow: true, always: false },
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
            HostMsg::Finished { session: "s1".into(), error: None, stopped: false },
            HostMsg::Finished { session: "s1".into(), error: Some("signal: 15".into()), stopped: true },
            HostMsg::Approval { session: "s1".into(), request: 3, tool: "Bash".into(), input: serde_json::json!({"command": "ls"}) },
            HostMsg::ApprovalDone { session: "s1".into(), request: 3 },
        ];
        for m in msgs {
            assert_eq!(decode::<HostMsg>(&encode(&m)).unwrap(), m);
        }
    }

    #[test]
    fn wire_format_is_tagged() {
        assert_eq!(encode(&ClientMsg::ListSessions), r#"{"type":"list_sessions"}"#);
        let p: ClientMsg = decode(r#"{"type":"prompt","session":"s","text":"x"}"#).unwrap();
        assert_eq!(p, ClientMsg::Prompt { session: "s".into(), text: "x".into(), mode: None, model: None, effort: None, files: Vec::new() });
        // A host from before `stopped` leaves it out.
        let f: HostMsg = decode(r#"{"type":"finished","session":"s","error":null}"#).unwrap();
        assert_eq!(f, HostMsg::Finished { session: "s".into(), error: None, stopped: false });
    }
}
