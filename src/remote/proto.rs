//! The messages a host and a client exchange: one JSON object per WebSocket text frame,
//! tagged by `"type"`.
//!
//! A client says `Hello` first and the host answers `Welcome`. After that the client may list
//! chats, subscribe to one (the host sends a `Snapshot`, then every `Event` of its turns), send
//! prompts (with files) and stop turns.

use crate::engine::{Mode, Out};
use serde::{de::DeserializeOwned, Deserialize, Deserializer, Serialize};
use std::path::PathBuf;

/// Bumped on any incompatible change. A host refuses a client with a different version.
pub const VERSION: u32 = 1;
/// Most file bytes one prompt may carry, before base64. Stays under `MAX_MESSAGE_BYTES` once encoded.
pub const MAX_REMOTE_FILES_BYTES: u64 = 40 * 1024 * 1024;
/// Largest message either end accepts. A project copy travels as one message (one frame), so
/// tungstenite's 16 MiB default frame limit would drop the connection for any real repository.
pub const MAX_MESSAGE_BYTES: usize = 512 * 1024 * 1024;

/// Whether a message carrying `payload` bytes (already base64) stays under `MAX_MESSAGE_BYTES`,
/// with room for the rest of the message.
pub fn fits(payload: usize) -> bool {
    payload.saturating_add(64 * 1024) <= MAX_MESSAGE_BYTES
}

/// The largest project copy (git bundle bytes, before base64) that `fits` in one message.
pub const MAX_BUNDLE_BYTES: u64 = ((MAX_MESSAGE_BYTES - 64 * 1024) / 4 * 3) as u64;

/// The WebSocket limits hosts and clients both use.
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
    /// `default_model` / `default_effort` put the agent's default back instead; older hosts ignore them.
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
        default_model: bool,
        #[serde(default)]
        default_effort: bool,
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
    /// Rename a chat on the host. Followers are sent `Renamed`. A host from before this answers `Error`.
    Rename { session: String, title: String },
    /// Delete a chat on the host, unless a reply is running in it. Followers are sent `Deleted`.
    Delete { session: String },
    /// A message from a newer client that this version does not know. The host answers `Error`. Never sent.
    #[serde(other)]
    Unknown,
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
    /// One event of a running turn. `None` when it is a kind of event this version does not know,
    /// from a newer host; skip it. Always `Some` when sent.
    Event {
        session: String,
        #[serde(deserialize_with = "known_out")]
        out: Option<Out>,
    },
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
    /// Something the client asked for failed, e.g. an unknown session id. `about` names the request;
    /// hosts from before it leave it out.
    Error {
        message: String,
        #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "known_about")]
        about: Option<About>,
    },
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
    /// Chat `session` was renamed on the host.
    Renamed { session: String, title: String },
    /// Chat `session` was deleted on the host.
    Deleted { session: String },
    /// A message from a newer host that this version does not know; skip it. Never sent.
    #[serde(other)]
    Unknown,
}

/// Which request a host's `Error` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct About {
    /// The request's `type`, e.g. `"put_project"`; empty when the host could not read it.
    pub request: String,
    /// The project a `put_project` or `get_project` was about.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
}

impl About {
    /// The request `msg`.
    pub fn of(msg: &ClientMsg) -> About {
        let project = match msg {
            ClientMsg::PutProject { project, .. } | ClientMsg::GetProject { project, .. } => Some(project.clone()),
            _ => None,
        };
        About { request: msg.kind().into(), project }
    }

    /// A request read only as JSON, e.g. one that did not decode.
    pub fn of_raw(text: &str) -> About {
        let value: serde_json::Value = serde_json::from_str(text).unwrap_or_default();
        let field = |name: &str| value.get(name).and_then(|v| v.as_str()).map(str::to_string);
        let request = field("type").unwrap_or_default();
        let project = if matches!(request.as_str(), "put_project" | "get_project") { field("project") } else { None };
        About { request, project }
    }
}

impl ClientMsg {
    /// The message's `type` on the wire; empty for `Unknown`.
    pub fn kind(&self) -> &'static str {
        match self {
            ClientMsg::Hello { .. } => "hello",
            ClientMsg::ListSessions => "list_sessions",
            ClientMsg::Subscribe { .. } => "subscribe",
            ClientMsg::Unsubscribe { .. } => "unsubscribe",
            ClientMsg::NewSession { .. } => "new_session",
            ClientMsg::Prompt { .. } => "prompt",
            ClientMsg::Cancel { .. } => "cancel",
            ClientMsg::Draft { .. } => "draft",
            ClientMsg::PutProject { .. } => "put_project",
            ClientMsg::PutChats { .. } => "put_chats",
            ClientMsg::ListCloud => "list_cloud",
            ClientMsg::GetProject { .. } => "get_project",
            ClientMsg::GetChats { .. } => "get_chats",
            ClientMsg::Answer { .. } => "answer",
            ClientMsg::Rename { .. } => "rename",
            ClientMsg::Delete { .. } => "delete",
            ClientMsg::Unknown => "",
        }
    }
}

impl HostMsg {
    /// An `Error` answering request `about`.
    pub fn error(about: &About, message: impl Into<String>) -> HostMsg {
        HostMsg::Error { message: message.into(), about: Some(about.clone()) }
    }
}

/// Reads an `Error`'s `about`, or `None` when it is in a shape this version does not know.
fn known_about<'de, D: Deserializer<'de>>(d: D) -> Result<Option<About>, D::Error> {
    let value = serde_json::Value::deserialize(d)?;
    Ok(About::deserialize(value).ok())
}

/// Reads an event of a turn, or `None` when it is one this version does not know.
fn known_out<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Out>, D::Error> {
    let value = serde_json::Value::deserialize(d)?;
    Ok(Out::deserialize(value).ok())
}

/// The `Error` a host sends for a `ClientMsg::Unknown`: a request from a newer client.
pub const UNKNOWN_REQUEST: &str = "This device's archonUI does not know that request; update it to match the other device";

/// Whether `message`, from a host's `Error`, means the host is too old to know the request. Hosts
/// from before `ClientMsg::Unknown` answered with the decode error, e.g.
/// "bad message: unknown variant `rename`, expected one of ...".
pub fn is_unknown_request(message: &str) -> bool {
    message == UNKNOWN_REQUEST || message.starts_with("bad message: unknown variant `")
}

pub fn encode<T: Serialize>(msg: &T) -> String {
    serde_json::to_string(msg).expect("protocol messages always serialize")
}

/// `encode(&ClientMsg::PutProject { .. })` with `bundle` base64-encoded straight into the frame, so a
/// big copy is not held twice more (as base64, then as JSON) and can be built off the UI thread.
pub fn put_project_frame(project: &str, name: &str, device: &str, commit: &str, bundle: &[u8]) -> String {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    let field = |s: &str| serde_json::Value::from(s).to_string();
    let mut out = String::with_capacity(bundle.len().div_ceil(3) * 4 + 256);
    out.push_str(r#"{"type":"put_project","project":"#);
    out.push_str(&field(project));
    out.push_str(r#","name":"#);
    out.push_str(&field(name));
    out.push_str(r#","device":"#);
    out.push_str(&field(device));
    out.push_str(r#","commit":"#);
    out.push_str(&field(commit));
    out.push_str(r#","bundle":""#);
    STANDARD.encode_string(bundle, &mut out);
    out.push_str(r#""}"#);
    out
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
            ClientMsg::Prompt { session: "s1".into(), text: "hi".into(), mode: Some(Mode::Plan), model: None, effort: None, default_model: false, default_effort: false, files: Vec::new() },
            ClientMsg::Cancel { session: "s1".into() },
            ClientMsg::PutProject { project: "p".into(), name: "app".into(), device: "n1".into(), commit: "abc".into(), bundle: "AAA=".into() },
            ClientMsg::GetChats { device: "n1".into() },
            ClientMsg::Answer { session: "s1".into(), request: 3, allow: true, always: false },
            ClientMsg::Rename { session: "s1".into(), title: "Fix it".into() },
            ClientMsg::Delete { session: "s1".into() },
        ];
        for m in msgs {
            assert_eq!(decode::<ClientMsg>(&encode(&m)).unwrap(), m);
        }
    }

    #[test]
    fn host_events_round_trip() {
        let msgs = [
            HostMsg::Event { session: "s1".into(), out: Some(Out::Text("Hel".into())) },
            HostMsg::Event { session: "s1".into(), out: Some(Out::ToolStart { id: "t1".into(), kind: ToolKind::Edit, target: "a.rs".into() }) },
            HostMsg::Event { session: "s1".into(), out: Some(Out::Thinking(true)) },
            HostMsg::Finished { session: "s1".into(), error: None, stopped: false },
            HostMsg::Finished { session: "s1".into(), error: Some("signal: 15".into()), stopped: true },
            HostMsg::Approval { session: "s1".into(), request: 3, tool: "Bash".into(), input: serde_json::json!({"command": "ls"}) },
            HostMsg::ApprovalDone { session: "s1".into(), request: 3 },
            HostMsg::Renamed { session: "s1".into(), title: "Fix it".into() },
            HostMsg::Deleted { session: "s1".into() },
        ];
        for m in msgs {
            assert_eq!(decode::<HostMsg>(&encode(&m)).unwrap(), m);
        }
    }

    #[test]
    fn known_messages_keep_their_wire_format() {
        let event = HostMsg::Event { session: "s1".into(), out: Some(Out::Text("Hel".into())) };
        assert_eq!(encode(&event), r#"{"type":"event","session":"s1","out":{"type":"text","data":"Hel"}}"#);
        let event: HostMsg = decode(r#"{"type":"event","session":"s1","out":{"type":"thinking","data":true}}"#).unwrap();
        assert_eq!(event, HostMsg::Event { session: "s1".into(), out: Some(Out::Thinking(true)) });
    }

    #[test]
    fn messages_from_a_newer_peer_are_read_as_unknown() {
        let host: HostMsg = decode(r#"{"type":"something_new","session":"s1","n":3}"#).unwrap();
        assert_eq!(host, HostMsg::Unknown);
        let client: ClientMsg = decode(r#"{"type":"something_new","x":{"y":1}}"#).unwrap();
        assert_eq!(client, ClientMsg::Unknown);
        // A new kind of event, or a known one with data this version can't read, loses only that event.
        let event: HostMsg = decode(r#"{"type":"event","session":"s1","out":{"type":"image","data":"AAA="}}"#).unwrap();
        assert_eq!(event, HostMsg::Event { session: "s1".into(), out: None });
        let usage = r#"{"type":"event","session":"s1","out":{"type":"context_used","data":"lots"}}"#;
        assert_eq!(decode::<HostMsg>(usage).unwrap(), HostMsg::Event { session: "s1".into(), out: None });
        // A new tool kind already reads as `Other`.
        let tool = r#"{"type":"event","session":"s1","out":{"type":"tool_start","data":{"id":"t","kind":"teleport","target":"x"}}}"#;
        let other = Out::ToolStart { id: "t".into(), kind: ToolKind::Other, target: "x".into() };
        assert_eq!(decode::<HostMsg>(tool).unwrap(), HostMsg::Event { session: "s1".into(), out: Some(other) });
        // A known message that is broken is still an error.
        assert!(decode::<HostMsg>(r#"{"type":"event","out":{"type":"text","data":"x"}}"#).is_err());
        assert!(decode::<ClientMsg>(r#"{"type":"prompt","session":"s"}"#).is_err());
    }

    /// Every message's exact JSON. Devices update at different times, so a change here breaks an
    /// older peer: only add optional fields (`#[serde(default)]`) or new variants, never rename,
    /// drop or retype a field. Rename and Delete are new, and only reach hosts that know them.
    #[test]
    fn every_message_keeps_its_wire_format() {
        use serde_json::json;
        let client: Vec<(ClientMsg, &str)> = vec![
            (ClientMsg::Hello { device: "pc".into(), version: 1 }, r#"{"type":"hello","device":"pc","version":1}"#),
            (ClientMsg::ListSessions, r#"{"type":"list_sessions"}"#),
            (ClientMsg::Subscribe { session: "s1".into() }, r#"{"type":"subscribe","session":"s1"}"#),
            (ClientMsg::Unsubscribe { session: "s1".into() }, r#"{"type":"unsubscribe","session":"s1"}"#),
            (
                ClientMsg::NewSession { agent: "claude".into(), cwd: "/w".into(), project: None },
                r#"{"type":"new_session","agent":"claude","cwd":"/w","project":null}"#,
            ),
            (
                ClientMsg::Prompt {
                    session: "s1".into(),
                    text: "hi".into(),
                    mode: Some(Mode::Plan),
                    model: Some("m".into()),
                    effort: Some("high".into()),
                    default_model: false,
                    default_effort: false,
                    files: vec![RemoteFile { name: "a.txt".into(), data: "QQ==".into() }],
                },
                r#"{"type":"prompt","session":"s1","text":"hi","mode":"plan","model":"m","effort":"high","default_model":false,"default_effort":false,"files":[{"name":"a.txt","data":"QQ=="}]}"#,
            ),
            (
                ClientMsg::Prompt {
                    session: "s1".into(),
                    text: "hi".into(),
                    mode: None,
                    model: None,
                    effort: None,
                    default_model: true,
                    default_effort: true,
                    files: Vec::new(),
                },
                r#"{"type":"prompt","session":"s1","text":"hi","mode":null,"model":null,"effort":null,"default_model":true,"default_effort":true,"files":[]}"#,
            ),
            (ClientMsg::Cancel { session: "s1".into() }, r#"{"type":"cancel","session":"s1"}"#),
            (ClientMsg::Draft { session: "s1".into(), text: "d".into() }, r#"{"type":"draft","session":"s1","text":"d"}"#),
            (
                ClientMsg::PutProject { project: "p".into(), name: "app".into(), device: "n1".into(), commit: "abc".into(), bundle: "AAA=".into() },
                r#"{"type":"put_project","project":"p","name":"app","device":"n1","commit":"abc","bundle":"AAA="}"#,
            ),
            (
                ClientMsg::PutChats { device: "n1".into(), sessions: vec![json!({"id": "c1"})] },
                r#"{"type":"put_chats","device":"n1","sessions":[{"id":"c1"}]}"#,
            ),
            (ClientMsg::ListCloud, r#"{"type":"list_cloud"}"#),
            (ClientMsg::GetProject { project: "p".into(), device: "n1".into() }, r#"{"type":"get_project","project":"p","device":"n1"}"#),
            (ClientMsg::GetChats { device: "n1".into() }, r#"{"type":"get_chats","device":"n1"}"#),
            (
                ClientMsg::Answer { session: "s1".into(), request: 3, allow: true, always: false },
                r#"{"type":"answer","session":"s1","request":3,"allow":true,"always":false}"#,
            ),
            (ClientMsg::Rename { session: "s1".into(), title: "T".into() }, r#"{"type":"rename","session":"s1","title":"T"}"#),
            (ClientMsg::Delete { session: "s1".into() }, r#"{"type":"delete","session":"s1"}"#),
        ];
        for (msg, wire) in client {
            assert_eq!(encode(&msg), wire);
            assert_eq!(decode::<ClientMsg>(wire).unwrap(), msg, "{wire}");
        }

        let summary = SessionSummary {
            id: "s1".into(),
            title: "T".into(),
            agent: "claude".into(),
            project: Some("p".into()),
            cwd: "/w".into(),
            updated: 5,
            busy: false,
            messages: 2,
        };
        let summary_wire = r#"{"id":"s1","title":"T","agent":"claude","project":"p","cwd":"/w","updated":5,"busy":false,"messages":2}"#;
        let cloud = CloudProject { project: "p".into(), name: "app".into(), device: "n1".into(), commit: "abc".into(), updated: 7 };
        let host: Vec<(HostMsg, String)> = vec![
            (HostMsg::Welcome { device: "vps".into(), version: 1 }, r#"{"type":"welcome","device":"vps","version":1}"#.into()),
            (HostMsg::Sessions { sessions: vec![summary.clone()] }, format!(r#"{{"type":"sessions","sessions":[{summary_wire}]}}"#)),
            (HostMsg::Snapshot { session: json!({"id": "s1"}), busy: true }, r#"{"type":"snapshot","session":{"id":"s1"},"busy":true}"#.into()),
            (HostMsg::Created { session: summary }, format!(r#"{{"type":"created","session":{summary_wire}}}"#)),
            (HostMsg::Started { session: "s1".into(), text: "hi".into() }, r#"{"type":"started","session":"s1","text":"hi"}"#.into()),
            (
                HostMsg::Event { session: "s1".into(), out: Some(Out::Text("Hel".into())) },
                r#"{"type":"event","session":"s1","out":{"type":"text","data":"Hel"}}"#.into(),
            ),
            (HostMsg::Event { session: "s1".into(), out: Some(Out::Break) }, r#"{"type":"event","session":"s1","out":{"type":"break"}}"#.into()),
            (
                HostMsg::Event { session: "s1".into(), out: Some(Out::Final("Hello".into())) },
                r#"{"type":"event","session":"s1","out":{"type":"final","data":"Hello"}}"#.into(),
            ),
            (
                HostMsg::Event { session: "s1".into(), out: Some(Out::Session("x".into())) },
                r#"{"type":"event","session":"s1","out":{"type":"session","data":"x"}}"#.into(),
            ),
            (
                HostMsg::Event { session: "s1".into(), out: Some(Out::ToolStart { id: "t".into(), kind: ToolKind::Run, target: "ls".into() }) },
                r#"{"type":"event","session":"s1","out":{"type":"tool_start","data":{"id":"t","kind":"run","target":"ls"}}}"#.into(),
            ),
            (
                HostMsg::Event { session: "s1".into(), out: Some(Out::ToolEnd { id: "t".into(), ok: true, detail: None }) },
                r#"{"type":"event","session":"s1","out":{"type":"tool_end","data":{"id":"t","ok":true,"detail":null}}}"#.into(),
            ),
            (
                HostMsg::Finished { session: "s1".into(), error: None, stopped: false },
                r#"{"type":"finished","session":"s1","error":null,"stopped":false}"#.into(),
            ),
            (
                HostMsg::Finished { session: "s1".into(), error: Some("signal: 15 (SIGTERM)".into()), stopped: true },
                r#"{"type":"finished","session":"s1","error":"signal: 15 (SIGTERM)","stopped":true}"#.into(),
            ),
            (HostMsg::Draft { session: "s1".into(), text: "d".into() }, r#"{"type":"draft","session":"s1","text":"d"}"#.into()),
            (HostMsg::Error { message: "no".into(), about: None }, r#"{"type":"error","message":"no"}"#.into()),
            (
                HostMsg::Error { message: "no".into(), about: Some(About { request: "prompt".into(), project: None }) },
                r#"{"type":"error","message":"no","about":{"request":"prompt"}}"#.into(),
            ),
            (
                HostMsg::Error { message: "no".into(), about: Some(About { request: "put_project".into(), project: Some("p".into()) }) },
                r#"{"type":"error","message":"no","about":{"request":"put_project","project":"p"}}"#.into(),
            ),
            (
                HostMsg::Stored { project: "p".into(), device: "n1".into(), commit: "abc".into() },
                r#"{"type":"stored","project":"p","device":"n1","commit":"abc"}"#.into(),
            ),
            (
                HostMsg::Cloud { projects: vec![cloud], chats: vec!["n1".into()] },
                r#"{"type":"cloud","projects":[{"project":"p","name":"app","device":"n1","commit":"abc","updated":7}],"chats":["n1"]}"#.into(),
            ),
            (
                HostMsg::ProjectData { project: "p".into(), device: "n1".into(), commit: "abc".into(), bundle: "AAA=".into() },
                r#"{"type":"project_data","project":"p","device":"n1","commit":"abc","bundle":"AAA="}"#.into(),
            ),
            (
                HostMsg::Chats { device: "n1".into(), sessions: vec![json!({"id": "c1"})] },
                r#"{"type":"chats","device":"n1","sessions":[{"id":"c1"}]}"#.into(),
            ),
            (
                HostMsg::Approval { session: "s1".into(), request: 3, tool: "Bash".into(), input: json!({"command": "ls"}) },
                r#"{"type":"approval","session":"s1","request":3,"tool":"Bash","input":{"command":"ls"}}"#.into(),
            ),
            (HostMsg::ApprovalDone { session: "s1".into(), request: 3 }, r#"{"type":"approval_done","session":"s1","request":3}"#.into()),
            (HostMsg::Renamed { session: "s1".into(), title: "T".into() }, r#"{"type":"renamed","session":"s1","title":"T"}"#.into()),
            (HostMsg::Deleted { session: "s1".into() }, r#"{"type":"deleted","session":"s1"}"#.into()),
        ];
        for (msg, wire) in host {
            assert_eq!(encode(&msg), wire);
            assert_eq!(decode::<HostMsg>(&wire).unwrap(), msg, "{wire}");
        }
    }

    /// Frames as older peers send them (a 0dce615 host has no `stopped`), and as newer ones might
    /// (an extra field), still read here.
    #[test]
    fn frames_from_older_peers_still_decode() {
        let old_finished = r#"{"type":"finished","session":"s1","error":"signal: 15 (SIGTERM)"}"#;
        assert_eq!(
            decode::<HostMsg>(old_finished).unwrap(),
            HostMsg::Finished { session: "s1".into(), error: Some("signal: 15 (SIGTERM)".into()), stopped: false }
        );
        // A field this version does not know, from a newer peer, is ignored rather than refused.
        let newer = r#"{"type":"started","session":"s1","text":"hi","by":"laptop"}"#;
        assert_eq!(decode::<HostMsg>(newer).unwrap(), HostMsg::Started { session: "s1".into(), text: "hi".into() });
        let newer = r#"{"type":"cancel","session":"s1","why":"esc"}"#;
        assert_eq!(decode::<ClientMsg>(newer).unwrap(), ClientMsg::Cancel { session: "s1".into() });
        let bare = r#"{"type":"prompt","session":"s1","text":"hi"}"#;
        assert!(matches!(decode::<ClientMsg>(bare).unwrap(), ClientMsg::Prompt { files, mode: None, .. } if files.is_empty()));
        // `out` must still be there: an older host always sends it.
        assert!(decode::<HostMsg>(r#"{"type":"event","session":"s1"}"#).is_err());
        // A host from before `about` names no request.
        let old_error = r#"{"type":"error","message":"git fetch: fatal: bad object"}"#;
        assert_eq!(decode::<HostMsg>(old_error).unwrap(), HostMsg::Error { message: "git fetch: fatal: bad object".into(), about: None });
        // An `about` in a shape this version can't read loses only the `about`.
        let odd = r#"{"type":"error","message":"no","about":{"kind":7}}"#;
        assert_eq!(decode::<HostMsg>(odd).unwrap(), HostMsg::Error { message: "no".into(), about: None });
        let odd = r#"{"type":"error","message":"no","about":"put_project"}"#;
        assert_eq!(decode::<HostMsg>(odd).unwrap(), HostMsg::Error { message: "no".into(), about: None });
    }

    /// Hosts at 0dce615 and d30ae41 read `Error` as `{ message }` with no `deny_unknown_fields`, so a
    /// new `Error` with `about` still reads there, as this stand-in for their type shows.
    #[test]
    fn older_peers_read_an_error_with_about() {
        #[derive(Deserialize, Debug, PartialEq)]
        #[serde(tag = "type", rename_all = "snake_case")]
        enum OldHostMsg {
            Error { message: String },
        }
        let about = About { request: "put_project".into(), project: Some("p".into()) };
        let wire = encode(&HostMsg::error(&about, "disk full"));
        assert_eq!(decode::<OldHostMsg>(&wire).unwrap(), OldHostMsg::Error { message: "disk full".into() });
    }

    #[test]
    fn errors_name_the_request() {
        let put = ClientMsg::PutProject { project: "p".into(), name: "app".into(), device: "n1".into(), commit: "abc".into(), bundle: "AAA=".into() };
        assert_eq!(About::of(&put), About { request: "put_project".into(), project: Some("p".into()) });
        let prompt = ClientMsg::Prompt { session: "s".into(), text: "x".into(), mode: None, model: None, effort: None, default_model: false, default_effort: false, files: Vec::new() };
        assert_eq!(About::of(&prompt), About { request: "prompt".into(), project: None });
        // `kind` matches the tag each message carries on the wire.
        for msg in [put, prompt, ClientMsg::ListCloud, ClientMsg::PutChats { device: "n1".into(), sessions: Vec::new() }, ClientMsg::Delete { session: "s".into() }] {
            let wire: serde_json::Value = serde_json::from_str(&encode(&msg)).unwrap();
            assert_eq!(wire["type"], msg.kind());
        }
        // A frame that did not decode is named from its JSON when it can be.
        let bad = r#"{"type":"put_project","project":"p","bundle":7}"#;
        assert_eq!(About::of_raw(bad), About { request: "put_project".into(), project: Some("p".into()) });
        assert_eq!(About::of_raw("not json"), About { request: String::new(), project: None });
    }

    #[test]
    fn an_older_host_refusing_a_request_is_recognised() {
        // Word for word what hosts at 0dce615 (no `Unknown` yet) and d30ae41 answer a `rename`.
        let first = "bad message: unknown variant `rename`, expected one of `hello`, `list_sessions`, `subscribe`, `unsubscribe`, `new_session`, `prompt`, `cancel`, `draft`, `put_project`, `put_chats`, `list_cloud`, `get_project`, `get_chats`, `answer` at line 1 column 16";
        let later = "This device's archonUI does not know that request; update it to match the other device";
        assert!(is_unknown_request(first));
        assert!(is_unknown_request(later));
        assert!(is_unknown_request(UNKNOWN_REQUEST));
        // Other failures are not the host being too old.
        assert!(!is_unknown_request("bad message: missing field `session` at line 1 column 17"));
        assert!(!is_unknown_request("There is no chat s1 on this device"));
        assert!(!is_unknown_request("git fetch: fatal: bad object"));
    }

    #[test]
    fn a_put_project_frame_is_what_encode_makes() {
        use base64::{engine::general_purpose::STANDARD, Engine as _};
        for bundle in [&b""[..], b"x", b"\x00\xffbundle\n\"bytes\"", &[7u8; 1000][..]] {
            let (project, name, device, commit) = ("p1", "my \"app\" \\ é", "n1", "abc123");
            let msg = ClientMsg::PutProject {
                project: project.into(),
                name: name.into(),
                device: device.into(),
                commit: commit.into(),
                bundle: STANDARD.encode(bundle),
            };
            assert_eq!(put_project_frame(project, name, device, commit, bundle), encode(&msg));
        }
    }

    #[test]
    fn the_largest_bundle_just_fits() {
        let base64 = |n: u64| (n as usize).div_ceil(3) * 4;
        assert!(fits(base64(MAX_BUNDLE_BYTES)));
        assert!(!fits(base64(MAX_BUNDLE_BYTES + 1)));
    }

    #[test]
    fn wire_format_is_tagged() {
        assert_eq!(encode(&ClientMsg::ListSessions), r#"{"type":"list_sessions"}"#);
        let p: ClientMsg = decode(r#"{"type":"prompt","session":"s","text":"x"}"#).unwrap();
        assert_eq!(p, ClientMsg::Prompt { session: "s".into(), text: "x".into(), mode: None, model: None, effort: None, default_model: false, default_effort: false, files: Vec::new() });
        // A host from before `stopped` leaves it out.
        let f: HostMsg = decode(r#"{"type":"finished","session":"s","error":null}"#).unwrap();
        assert_eq!(f, HostMsg::Finished { session: "s".into(), error: None, stopped: false });
    }
}
