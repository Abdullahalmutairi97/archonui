//! The app's state: sessions and their live turns, projects, the workspace and settings. Plain data
//! with no UI types, so views read it and send it intents, and the tests run without a window.

use super::{
    notify::{Ended, Outcome},
    persist::{self, Paths, Saver},
    sections::{self, Section},
    settings::AppSettings,
    workspace::{Project, Workspace},
};
use crate::{
    engine::{self, attach::Attachment, title::Exchange, DetectedAgent, Event, Mode, Out, ToolSettings, Turn, TurnOptions},
    preview,
    store::{self, Role, Session, ToolCall, Usage, UNASSIGNED_ICON},
    ui::{
        sigil,
        tiling::Rect,
    },
};
use smol::channel::Receiver;
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    path::PathBuf,
    time::{Duration, Instant},
};

/// The note a turn leaves after its reply when it was stopped.
pub const STOPPED_NOTE: &str = "Stopped.";
/// The first line of the note a turn leaves after its reply when the agent failed.
pub const FAILED_NOTE: &str = "Agent exited with an error:";

/// Where a new session's tile goes.
#[derive(Clone, Copy, Debug)]
pub enum Placement {
    /// Split the focused tile, if the window has room; otherwise show in the focused tile.
    Split { area: Rect, gap: f32 },
    /// Show in the focused tile, replacing it.
    Focused,
    /// No tile: another device asked for the chat over connection `conn`. Kept while empty, until its
    /// first prompt, unless `AppState::expire_requested` finds it left.
    Elsewhere { conn: u64 },
}

/// An empty chat another device asked for: the connection that asked, and when it last changed.
#[derive(Clone, Copy, Debug)]
struct Requested {
    conn: u64,
    since: Instant,
}

/// What the caller must do after `AppState::apply`.
#[derive(Debug, Default, PartialEq)]
pub struct Applied {
    /// A local site or HTML file in the reply, to open in the preview.
    pub preview: Option<String>,
    /// The turn ended, so the agent may have changed files.
    pub finished: bool,
    /// How the turn ended, for the desktop alert and sound.
    pub ended: Option<Ended>,
    /// The chat's first exchange, when a fast title should name it.
    pub exchange: Option<Exchange>,
}

/// Runtime state of a session. Not saved.
#[derive(Default)]
struct Live {
    turn: Option<Turn>,
    status: Option<String>,
    cancelled: bool,
    /// Files waiting to go with the next message.
    pending: Vec<Attachment>,
    /// What the agent did during the latest turn, oldest first.
    activity: Vec<String>,
    started: Option<Instant>,
    /// Row index in the reply's `tools`, by call id.
    tools: HashMap<String, usize>,
    /// When the thinking that is open now began.
    thinking_since: Option<Instant>,
    /// Time spent thinking this turn; `None` if the harness never reported thinking.
    thought: Option<Duration>,
    /// A tool call ran since the last reply text, so the next text may begin the final answer.
    after_tool: bool,
    /// Byte offset in the reply where the text after the latest tool call begins.
    final_start: Option<usize>,
    /// Tokens the turn used so far, when the agent reports them.
    usage: Option<Usage>,
}

impl Live {
    fn log(&mut self, entry: impl Into<String>) {
        let entry = entry.into();
        if self.activity.last() != Some(&entry) {
            self.activity.push(entry);
        }
    }

    /// Adds the thinking that is open now to the turn's total.
    fn stop_thinking(&mut self) {
        if let Some(since) = self.thinking_since.take() {
            *self.thought.get_or_insert_default() += since.elapsed();
        }
    }

    /// Clears what the previous turn left behind, as a new turn starts. Pending files stay.
    fn reset_for_turn(&mut self) {
        self.status = None;
        self.cancelled = false;
        self.started = Some(Instant::now());
        self.activity.clear();
        self.tools.clear();
        self.thought = None;
        self.thinking_since = None;
        self.after_tool = false;
        self.final_start = None;
        self.usage = None;
    }
}

pub struct AppState {
    sessions: Vec<Session>,
    projects: Vec<Project>,
    /// Saved with `save_config`. The focused tile is `workspace.layout.focus`.
    pub workspace: Workspace,
    /// Saved with `save_config`.
    pub settings: AppSettings,
    live: HashMap<String, Live>,
    /// Sessions changed since the last `flush_sessions`.
    dirty: BTreeSet<String>,
    /// Sessions deleted since the last `flush_sessions`; their files go on the next flush.
    removed: Vec<String>,
    /// Remote chats with a turn running on their device.
    remote_busy: BTreeSet<String>,
    /// Empty chats another device started here (`Placement::Elsewhere`), which `drop_abandoned` keeps.
    requested: BTreeMap<String, Requested>,
}

impl AppState {
    /// Loads everything from the platform's config and data dirs.
    pub fn load(paths: &Paths) -> Self {
        let mut loaded = persist::load(&paths.config, &store::settings_path());
        restore_tiles(&mut loaded.workspace, &loaded.settings);
        let mut state = Self::new(store::load_all(), loaded.projects, loaded.workspace, loaded.settings);
        // Persist the sigils assigned to sessions that predate them, so the
        // assignment is stable and stored from the first launch onward.
        let _ = state.flush_sessions();
        state
    }

    /// Builds the state from loaded parts. Sessions without a unique icon get one.
    pub fn new(sessions: Vec<Session>, projects: Vec<Project>, workspace: Workspace, settings: AppSettings) -> Self {
        let mut state = Self {
            sessions,
            projects,
            workspace,
            settings,
            live: HashMap::new(),
            dirty: BTreeSet::new(),
            removed: Vec::new(),
            remote_busy: BTreeSet::new(),
            requested: BTreeMap::new(),
        };
        let mut taken = Vec::new();
        for s in &mut state.sessions {
            if s.icon == UNASSIGNED_ICON || taken.contains(&s.icon) {
                s.icon = sigil::assign(&taken);
                state.dirty.insert(s.id.clone());
            }
            taken.push(s.icon);
        }
        state
    }

    /// Writes the settings, workspace and projects files. Cheap to call after each change, since
    /// the saver coalesces writes.
    pub fn save_config(&self, saver: &Saver, paths: &Paths) {
        persist::save_config(saver, &paths.config, &self.settings, &self.workspace, &self.projects);
    }

    /// Writes changed sessions and deletes removed ones. Blocking, so call it off the UI path or
    /// after a turn ends. Returns a line for every file that could not be written, for the UI to
    /// show; the app keeps running with the change in memory.
    pub fn flush_sessions(&mut self) -> Vec<String> {
        let mut failed = Vec::new();
        for id in std::mem::take(&mut self.dirty) {
            // Remote chats are live copies of another device's files, so they are never saved here.
            if let Some(s) = self.sessions.iter().find(|s| s.id == id).filter(|s| s.origin.is_none()) {
                if let Err(e) = store::save(s) {
                    eprintln!("could not save session {id}: {e}");
                    failed.push(format!("Couldn't save the chat: {e}"));
                    // Keep it dirty so a later flush retries it.
                    self.dirty.insert(id);
                }
            }
        }
        for id in std::mem::take(&mut self.removed) {
            if let Err(e) = store::delete(&id) {
                eprintln!("could not delete session {id}: {e}");
                failed.push(format!("Couldn't delete the chat: {e}"));
                self.removed.push(id);
            }
        }
        failed
    }

    /// Ends every running turn as if the user pressed Stop, keeping each partial reply, then
    /// writes the chats. The window close path calls this, so work isn't lost on quit.
    pub fn shutdown(&mut self) {
        let running: Vec<String> =
            self.live.iter().filter(|(_, l)| l.turn.is_some()).map(|(id, _)| id.clone()).collect();
        for id in running {
            self.stop(&id);
            self.apply(&id, Event::Finished(Ok(())));
        }
        self.list_left_drafts(true);
        self.flush_sessions();
    }

    // Sessions

    pub fn sessions(&self) -> &[Session] {
        &self.sessions
    }

    pub fn session(&self, id: &str) -> Option<&Session> {
        self.sessions.iter().find(|s| s.id == id)
    }

    /// Creates a chat with `agent` in the current project, and shows it as a tile. The chat runs in
    /// the project's folder; `cwd` is used only when there is no current project (or its folder is
    /// gone), so an agent never works outside the project the chat is filed under.
    /// Returns its id. The session isn't saved until it has a message.
    pub fn new_session(&mut self, agent: &str, cwd: PathBuf, placement: Placement) -> String {
        let project = self.workspace.current_project.clone();
        let cwd = project
            .as_deref()
            .and_then(|id| self.project(id))
            .map(|p| p.path.clone())
            .filter(|p| p.is_dir())
            .unwrap_or(cwd);
        let mut s = Session::new(agent, cwd);
        s.project = project;
        let taken: Vec<u16> = self.sessions.iter().map(|s| s.icon).collect();
        s.icon = sigil::seed_icon_for(&s.id, &taken);
        let id = s.id.clone();
        self.sessions.push(s);
        match placement {
            Placement::Split { area, gap } => {
                if !self.workspace.layout.split_focused(&id, area, gap) {
                    self.workspace.layout.open_in_focused(&id);
                }
            }
            Placement::Focused => self.workspace.layout.open_in_focused(&id),
            Placement::Elsewhere { conn } => {
                self.requested.insert(id.clone(), Requested { conn, since: Instant::now() });
            }
        }
        id
    }

    /// Sets the model, effort and mode a chat passes to its agent. `None` model or effort uses the agent's default.
    pub fn set_options(&mut self, id: &str, model: Option<String>, effort: Option<String>, mode: Mode) {
        if let Some(s) = self.sessions.iter_mut().find(|s| s.id == id) {
            // Another model has another window; the agent reports the new one on its next turn.
            if s.model != model {
                s.context_window = None;
            }
            s.model = model;
            s.effort = effort;
            s.mode = mode;
            self.dirty.insert(id.to_string());
        }
    }

    /// Hands chat `id` to `agent`, with the model, effort and mode it starts with. The old agent's
    /// session can't be resumed by another CLI, so the next turn starts a new one and carries the
    /// conversation so far (see `engine::handoff`). Returns false, changing nothing, while a turn
    /// runs, for an unknown chat, or when the chat already uses `agent`.
    pub fn switch_agent(&mut self, id: &str, agent: &str, model: Option<String>, effort: Option<String>, mode: Mode) -> bool {
        // A remote chat's agent runs on its device, which keeps its own.
        if self.busy(id) || self.is_remote(id).is_some() {
            return false;
        }
        let Some(s) = self.sessions.iter_mut().find(|s| s.id == id) else { return false };
        if s.agent == agent {
            return false;
        }
        // Replies from before replies were labelled were written by the agent being left.
        for m in s.messages.iter_mut().filter(|m| m.role == Role::Agent && m.agent.is_none()) {
            m.agent = Some(s.agent.clone());
        }
        s.agent = agent.to_string();
        s.agent_session = None;
        // The new agent starts its own context.
        s.context_used = None;
        s.context_window = None;
        s.handoff = !s.messages.is_empty();
        s.model = model;
        s.effort = effort;
        s.mode = mode;
        self.dirty.insert(id.to_string());
        true
    }

    /// Removes a chat, its live turn (which stops the agent) and its tile.
    pub fn delete_session(&mut self, id: &str) {
        let remote = self.is_remote(id).is_some();
        self.forget_running(id);
        self.sessions.retain(|s| s.id != id);
        self.workspace.layout.close(id);
        self.requested.remove(id);
        // A remote chat has no file here to delete.
        if !remote {
            self.removed.push(id.to_string());
        }
    }

    /// Drops a chat's live state, busy mark and pending save. The chat itself stays.
    fn forget_running(&mut self, id: &str) {
        self.live.remove(id);
        self.remote_busy.remove(id);
        self.dirty.remove(id);
    }

    // Remote chats: chats that live on another device, shown here as a live copy. Never saved here.

    /// Shows a chat from `origin` (a device id), replacing that device's copy with the same id. `busy` says a
    /// turn runs there now. Ignored when the id could leave the data folder (it becomes a file name once the
    /// chat is continued here) or names one of this device's own chats.
    pub fn put_remote(&mut self, mut session: Session, origin: &str, busy: bool) {
        let id = session.id.clone();
        if !crate::remote::cloud::safe_id(&id) || self.session(&id).is_some_and(|s| s.origin.is_none()) {
            return;
        }
        session.origin = Some(origin.into());
        match self.sessions.iter_mut().find(|s| s.id == id) {
            Some(slot) => *slot = session,
            None => self.sessions.push(session),
        }
        if busy {
            self.remote_busy.insert(id);
        } else {
            self.remote_busy.remove(&id);
        }
    }

    /// Drops every chat from `origin`, with its tile, live state and busy mark. Nothing is deleted on disk.
    pub fn remove_remote(&mut self, origin: &str) {
        let gone: Vec<String> = self.sessions.iter().filter(|s| s.origin.as_deref() == Some(origin)).map(|s| s.id.clone()).collect();
        for id in &gone {
            self.forget_running(id);
        }
        self.sessions.retain(|s| s.origin.as_deref() != Some(origin));
        let layout = &mut self.workspace.layout;
        layout.tiles.retain(|t| !gone.contains(t));
        if layout.focus.as_ref().is_some_and(|f| gone.contains(f)) {
            layout.focus = None;
        }
    }

    /// Marks every chat from `origin` as away: nothing runs or streams, but the chats and their tiles stay until a copy replaces them.
    pub fn remote_away(&mut self, origin: &str) {
        let away: Vec<String> = self.sessions.iter().filter(|s| s.origin.as_deref() == Some(origin)).map(|s| s.id.clone()).collect();
        for id in &away {
            self.forget_running(id);
        }
    }

    /// The device a chat lives on, for a chat shown from another device. `None` for this device's own chats.
    pub fn is_remote(&self, id: &str) -> Option<&str> {
        self.session(id)?.origin.as_deref()
    }

    /// Mirrors a prompt sent from another device into chat `id`: the user's message and an empty reply.
    /// Runs nothing. Events of the turn follow through `remote_event`. Nothing for a chat of this device.
    pub fn remote_started(&mut self, id: &str, text: &str) {
        let Some(s) = self.sessions.iter_mut().find(|s| s.id == id && s.origin.is_some()) else { return };
        // Same titling as `send`: the first message names the chat unless the user named it.
        if s.messages.is_empty() && (s.title.is_empty() || s.title_auto) {
            s.title = store::title_from(text);
            s.title_auto = true;
        }
        s.push(Role::User, text);
        s.draft.clear();
        s.push(Role::Agent, "");
        self.live.entry(id.to_string()).or_default().reset_for_turn();
        self.remote_busy.insert(id.to_string());
    }

    /// Continuing a remote chat `id` here makes a new local session. Copies the remote chat's
    /// history, model, effort, and mode; sets handoff true and handoff_from to `device` (its name).
    /// The remote session stays untouched. Returns the new session's id, or `None` if `id` isn't remote.
    pub fn adopt_remote(&mut self, id: &str, cwd: PathBuf, device: &str) -> Option<String> {
        let remote = self.sessions.iter().find(|s| s.id == id && s.origin.is_some()).cloned()?;
        let mut new = Session::new(&remote.agent, cwd);
        new.title = remote.title;
        new.title_auto = remote.title_auto;
        new.icon = remote.icon;
        new.messages = remote.messages;
        new.model = remote.model;
        new.effort = remote.effort;
        new.mode = remote.mode;
        new.handoff = !new.messages.is_empty();
        new.handoff_from = new.handoff.then(|| device.to_string());
        let new_id = new.id.clone();
        self.sessions.push(new);
        self.dirty.insert(new_id.clone());
        // Update tiles and focus to point to the new id.
        for tile in &mut self.workspace.layout.tiles {
            if tile == id {
                *tile = new_id.clone();
            }
        }
        if self.workspace.layout.focus.as_deref() == Some(id) {
            self.workspace.layout.focus = Some(new_id.clone());
        }
        Some(new_id)
    }

    /// Folds one event of a remote turn into chat `id`. Returns what the caller should do next.
    /// Nothing for a chat of this device (one adopted here, say).
    pub fn remote_event(&mut self, id: &str, out: Out) -> Applied {
        if self.is_remote(id).is_none() {
            return Applied::default();
        }
        self.apply(id, Event::Out(out))
    }

    /// Ends a remote turn in chat `id`. `error` is set when the agent failed on its device.
    /// The preview is not opened: a remote chat's files are on another device.
    pub fn remote_finished(&mut self, id: &str, error: Option<String>, stopped: bool) -> Applied {
        if self.is_remote(id).is_none() {
            return Applied::default();
        }
        self.remote_busy.remove(id);
        // Stopped on any device shows as stopped here too, not as the agent failing.
        self.live.entry(id.to_string()).or_default().cancelled = stopped;
        let result = error.map_or(Ok(()), Err);
        let mut applied = self.apply(id, Event::Finished(result));
        applied.preview = None;
        applied
    }

    /// Sets the chat's title. Returns false if there is no such chat.
    pub fn rename_session(&mut self, id: &str, title: &str) -> bool {
        let Some(s) = self.sessions.iter_mut().find(|s| s.id == id) else { return false };
        s.title = title.into();
        s.title_auto = false;
        self.dirty.insert(id.to_string());
        true
    }

    /// Sets a fast title, unless the user has renamed the chat since it was made. Returns whether it was set.
    pub fn apply_title(&mut self, id: &str, title: &str) -> bool {
        let Some(s) = self.sessions.iter_mut().find(|s| s.id == id) else { return false };
        if !s.title_auto {
            return false;
        }
        s.title = title.into();
        self.dirty.insert(id.to_string());
        true
    }

    /// Records that the chat has a terminal, so it is kept without messages. A chat with no title
    /// yet is titled "Terminal" until its first message.
    pub fn mark_terminal(&mut self, id: &str) {
        let Some(s) = self.sessions.iter_mut().find(|s| s.id == id) else { return };
        if s.terminal {
            return;
        }
        s.terminal = true;
        if s.title.is_empty() {
            s.title = "Terminal".into();
            s.title_auto = true;
        }
        self.dirty.insert(id.to_string());
    }

    /// Puts the chat in `project`, or in no project for `None`. Returns false if there is no such
    /// chat, or it is on another device: project ids are per device, and its next copy would undo it.
    pub fn move_to_project(&mut self, id: &str, project: Option<String>) -> bool {
        let Some(s) = self.sessions.iter_mut().find(|s| s.id == id && s.origin.is_none()) else { return false };
        s.project = project;
        self.dirty.insert(id.to_string());
        true
    }

    /// Removes every chat that isn't open in a tile or running, and returns their ids. Tiled chats
    /// are kept even when they have no messages. Chats shown from other devices are theirs, so they stay.
    pub fn clear_history(&mut self) -> Vec<String> {
        let doomed: Vec<String> = self
            .sessions
            .iter()
            .filter(|s| s.origin.is_none() && !self.workspace.layout.contains(&s.id) && !self.busy(&s.id))
            .map(|s| s.id.clone())
            .collect();
        for id in &doomed {
            self.delete_session(id);
        }
        doomed
    }

    /// The sidebar's groups, most recently used first. Empty groups are left out, and so are new
    /// chats with nothing sent and no draft left behind (`Session::visible`).
    pub fn sections(&self, now: u64) -> Vec<(Section, Vec<&Session>)> {
        sections::sections(&self.sessions, now)
            .into_iter()
            .filter_map(|(section, sessions)| {
                let sessions: Vec<&Session> = sessions.into_iter().filter(|s| s.visible()).collect();
                (!sessions.is_empty()).then_some((section, sessions))
            })
            .collect()
    }

    /// Keeps the composer's unsent text for `id`, so it survives closing the tile or the app.
    pub fn set_draft(&mut self, id: &str, text: &str) {
        let Some(s) = self.sessions.iter_mut().find(|s| s.id == id) else { return };
        if s.draft != text {
            s.draft = text.to_string();
            self.dirty.insert(id.to_string());
            // Typing in a chat another device asked for keeps it from expiring.
            if let Some(r) = self.requested.get_mut(id) {
                r.since = Instant::now();
            }
        }
    }

    /// Lists every chat the user has left with an unsent draft: one that isn't the focused tile,
    /// or any at all when `all` (the window is closing). Once listed, a chat stays in the sidebar
    /// until it is deleted. Chats from other devices are skipped: their device decides. Returns
    /// whether any chat was newly listed.
    pub fn list_left_drafts(&mut self, all: bool) -> bool {
        let focus = self.workspace.layout.focus.clone();
        let mut changed = false;
        for s in &mut self.sessions {
            if s.origin.is_none() && !s.listed && !s.draft.trim().is_empty() && (all || focus.as_deref() != Some(s.id.as_str())) {
                s.listed = true;
                self.dirty.insert(s.id.clone());
                changed = true;
            }
        }
        changed
    }

    /// Deletes every chat the user has left empty: nothing sent, typed or attached, no terminal, and
    /// not the focused tile (its tile was closed or replaced, or another chat was focused). Its tile
    /// closes with it. A chat another device started here is kept until its first message. Returns
    /// the ids that went.
    pub fn drop_abandoned(&mut self) -> Vec<String> {
        self.prune_requested();
        let focus = self.workspace.layout.focus.as_deref();
        let doomed: Vec<String> = self
            .sessions
            .iter()
            .filter(|s| !s.visible() && s.draft.trim().is_empty() && !self.requested.contains_key(&s.id))
            .filter(|s| focus != Some(s.id.as_str()) && !self.busy(&s.id) && self.pending(&s.id).is_empty())
            .map(|s| s.id.clone())
            .collect();
        for id in &doomed {
            self.delete_session(id);
        }
        doomed
    }

    /// Deletes the empty chats other devices asked for and then left: the asking connection closed
    /// (`open` says false), or nothing was sent or typed in them for `idle` up to `now`. Chats that
    /// got a message are ordinary chats and stay. Returns the ids that went.
    pub fn expire_requested(&mut self, open: impl Fn(u64) -> bool, idle: Duration, now: Instant) -> Vec<String> {
        self.prune_requested();
        let doomed: Vec<String> = self
            .requested
            .iter()
            .filter(|(id, _)| !self.busy(id) && !self.workspace.layout.contains(id))
            .filter(|(_, r)| !open(r.conn) || now.saturating_duration_since(r.since) >= idle)
            .map(|(id, _)| id.clone())
            .collect();
        for id in &doomed {
            self.delete_session(id);
        }
        doomed
    }

    /// Files to go with the next message to `id`.
    pub fn add_pending(&mut self, id: &str, files: Vec<Attachment>) {
        self.live.entry(id.to_string()).or_default().pending.extend(files);
    }

    pub fn pending(&self, id: &str) -> &[Attachment] {
        self.live.get(id).map_or(&[], |l| l.pending.as_slice())
    }

    /// Drops the pending file at `index`, if there is one.
    pub fn remove_pending(&mut self, id: &str, index: usize) {
        if let Some(live) = self.live.get_mut(id).filter(|l| index < l.pending.len()) {
            live.pending.remove(index);
        }
    }

    /// Forgets the chat requests (`requested`) whose chat has since got a message: those are ordinary chats.
    fn prune_requested(&mut self) {
        let sessions = &self.sessions;
        self.requested.retain(|id, _| sessions.iter().any(|s| &s.id == id && !s.visible()));
    }

    /// Whether a turn is running in `id`, here or on the chat's device.
    pub fn busy(&self, id: &str) -> bool {
        self.live.get(id).is_some_and(|l| l.turn.is_some()) || self.remote_busy.contains(id)
    }

    /// The agent's latest status line for `id`, while it runs.
    pub fn status(&self, id: &str) -> Option<&str> {
        self.live.get(id).and_then(|l| l.status.as_deref())
    }

    /// What the agent did during the latest turn in `id`, oldest first.
    pub fn activity(&self, id: &str) -> &[String] {
        self.live.get(id).map_or(&[], |l| l.activity.as_slice())
    }

    /// Starts a turn in `id` with `prompt`, using the agent in `agents` that the session names.
    /// Returns the turn's events for the caller to feed to `apply`. Returns `None` when nothing is
    /// sent: an empty prompt with no files, a turn already running, or the session's agent not
    /// installed. A failure to start is shown in the chat as an error message.
    pub fn send(&mut self, id: &str, prompt: &str, agents: &[DetectedAgent]) -> Option<Receiver<Event>> {
        let has_files = !self.pending(id).is_empty();
        if (prompt.is_empty() && !has_files) || self.busy(id) {
            return None;
        }
        let project_dir = self
            .session(id)
            .and_then(|s| s.project.as_deref())
            .and_then(|p| self.project(p))
            .map(|p| p.path.clone())
            .filter(|p| p.is_dir());
        let session = self.sessions.iter_mut().find(|s| s.id == id)?;
        let agent = agents.iter().find(|a| a.harness.id() == session.agent)?;
        // A chat filed under a project works in that project's folder. Only before the agent has a
        // session of its own: agents keep resumable sessions per folder, so a started chat stays put.
        if let Some(dir) = project_dir.as_ref().filter(|_| session.agent_session.is_none() && session.messages.is_empty()) {
            session.cwd = dir.clone();
        }
        let files = self.live.get_mut(id).map(|l| std::mem::take(&mut l.pending)).unwrap_or_default();
        let (files, file_errors) = keep_files(id, files);
        if session.agent_session.is_none() && agent.harness.preassign_session() {
            session.agent_session = Some(store::new_id());
        }
        // The first message titles the chat, unless the user named it first.
        if session.messages.is_empty() && (session.title.is_empty() || session.title_auto) {
            session.title = match files.first() {
                Some(f) if prompt.is_empty() => store::title_from(&f.name),
                _ => store::title_from(prompt),
            };
            session.title_auto = true;
        }
        // After an agent switch the new agent is sent the chat so far, once. A slash command goes to
        // the agent as typed, and the conversation waits for the next message.
        let handoff = (session.handoff && !prompt.starts_with('/')).then(|| engine::handoff::prompt_from(&session.messages, prompt, session.handoff_from.as_deref()));
        session.push_with(Role::User, prompt, files.clone());
        session.draft.clear();
        for e in file_errors {
            session.push(Role::Error, e);
        }
        let skills = engine::skills::discover(&session.cwd);
        let disabled_skills: Vec<String> = skills
            .iter()
            .filter(|s| self.settings.skills_enabled.get(&s.id) == Some(&false))
            .map(|s| s.id.clone())
            .collect();
        let turn = engine::start_turn(
            agent,
            &session.cwd,
            handoff.as_deref().unwrap_or(prompt),
            session.agent_session.as_deref(),
            TurnOptions {
                model: session.model.as_deref(),
                effort: session.effort.as_deref(),
                mode: session.mode,
                files: &files,
                tools: ToolSettings {
                    mcp: &self.settings.mcp_servers,
                    skills: &skills,
                    disabled_skills: &disabled_skills,
                    permissions: self.settings.permissions,
                    mcp_tools: Some(&self.settings.mcp_tools),
                    max_steps: Some(self.settings.max_steps),
                    sandbox: self.settings.sandbox,
                    ask_network: self.settings.ask_network,
                    approval: Some(engine::approve::Route { socket: engine::approve::socket(), session: id }),
                },
                // The agent is told which project it is in, while the chat runs in that project's folder.
                project: project_dir.as_deref().filter(|dir| *dir == session.cwd),
            },
        );
        self.dirty.insert(id.to_string());
        match turn {
            Ok(turn) => {
                if handoff.is_some() {
                    session.handoff = false;
                    session.handoff_from = None;
                }
                session.push(Role::Agent, "");
                let events = turn.events.clone();
                let mut sent = format!("Sent to {}", agent.harness.name());
                if !files.is_empty() {
                    sent.push_str(&format!(" with {} file(s)", files.len()));
                }
                if handoff.is_some() {
                    sent.push_str(", with the chat so far");
                }
                let live = self.live.entry(id.to_string()).or_default();
                live.turn = Some(turn);
                live.reset_for_turn();
                live.log(sent);
                Some(events)
            }
            Err(e) => {
                session.push(Role::Error, format!("Could not start {}: {e}", agent.harness.name()));
                None
            }
        }
    }

    /// Folds one event from a turn in `id` into the session. Returns what the caller should do next.
    pub fn apply(&mut self, id: &str, ev: Event) -> Applied {
        let auto_open = self.settings.auto_open_preview;
        let fast_titles = self.settings.fast_titles;
        self.dirty.insert(id.to_string());
        let Some(s) = self.sessions.iter_mut().find(|s| s.id == id) else { return Applied::default() };
        let live = self.live.entry(id.to_string()).or_default();
        let mut applied = Applied::default();
        match ev {
            Event::Out(Out::Text(t)) => {
                live.status = None;
                if let Some(m) = s.reply_mut() {
                    if std::mem::take(&mut live.after_tool) {
                        live.final_start = Some(m.text.len());
                    }
                    m.text.push_str(&t);
                }
            }
            Event::Out(Out::Usage(u)) => live.usage.get_or_insert_default().add(u),
            Event::Out(Out::ContextUsed(n)) => s.context_used = Some(n),
            Event::Out(Out::ContextWindow(n)) => s.context_window = Some(n),
            Event::Out(Out::Todos(todos)) => {
                if let Some(m) = s.reply_mut() {
                    m.todos = todos;
                }
            }
            Event::Out(Out::Break) => {
                if let Some(m) = s.reply_mut().filter(|m| !m.text.is_empty()) {
                    if !m.text.ends_with("\n\n") {
                        m.text.push_str(if m.text.ends_with('\n') { "\n" } else { "\n\n" });
                    }
                }
            }
            Event::Out(Out::Final(text)) => {
                if let Some(m) = s.reply_mut().filter(|m| m.text.trim().is_empty()) {
                    m.text = text;
                    // The recorded offset was into the whitespace that was just replaced.
                    live.final_start = None;
                }
            }
            Event::Out(Out::Status(st)) => {
                live.log(st.clone());
                live.status = Some(st);
            }
            Event::Out(Out::ToolStart { id, kind, target }) => {
                live.after_tool = true;
                live.final_start = None;
                if let Some(m) = s.reply_mut() {
                    match live.tools.get(&id).copied() {
                        // A call can be reported again as its details arrive; the row stays.
                        Some(i) => {
                            if let Some(t) = m.tools.get_mut(i) {
                                t.kind = kind;
                                t.target = target;
                            }
                        }
                        None => {
                            live.tools.insert(id, m.tools.len());
                            m.tools.push(ToolCall { kind, target, detail: None, ok: None });
                        }
                    }
                }
            }
            Event::Out(Out::ToolEnd { id, ok, detail }) => {
                if let (Some(&i), Some(m)) = (live.tools.get(&id), s.reply_mut()) {
                    if let Some(t) = m.tools.get_mut(i) {
                        t.ok = Some(ok);
                        t.detail = detail;
                    }
                }
            }
            Event::Out(Out::Thinking(true)) => {
                live.thought.get_or_insert_default();
                live.thinking_since.get_or_insert_with(Instant::now);
            }
            Event::Out(Out::Thinking(false)) => live.stop_thinking(),
            Event::Out(Out::Session(agent_session)) => {
                if s.agent_session.is_none() {
                    s.agent_session = Some(agent_session);
                }
            }
            // After Stop, an agent's complaint about being killed (OpenCode 2: "Transport: The socket
            // connection was closed unexpectedly") is noise; `Finished` reports the stop.
            Event::Out(Out::Error(_)) if live.cancelled => {}
            Event::Out(Out::Error(e)) => s.push(Role::Error, e),
            Event::Finished(result) => {
                applied.finished = true;
                live.turn = None;
                live.status = None;
                let cancelled = std::mem::take(&mut live.cancelled);
                let ok = result.is_ok();
                let elapsed = live.started.take().map(|t| t.elapsed());
                live.stop_thinking();
                let thought = live.thought.take();
                let took = elapsed.map_or(String::new(), |t| format!(" after {:.1}s", t.as_secs_f32()));
                live.log(match (&result, cancelled) {
                    (_, true) => format!("Stopped{took}"),
                    (Err(_), _) => format!("Failed{took}"),
                    (Ok(()), _) => format!("Finished{took}"),
                });
                let final_start = live.final_start.take();
                live.after_tool = false;
                let usage = live.usage.take();
                if let Some(m) = s.reply_mut() {
                    m.text = m.text.trim_end().to_string();
                    m.thought_secs = thought.map(|t| t.as_secs() as u32);
                    m.took_secs = elapsed.map(|t| t.as_secs() as u32);
                    // Progress notes come first only when there is text before the final answer.
                    m.final_at = final_start.filter(|&i| i < m.text.len() && m.text.get(..i).is_some_and(|p| !p.trim().is_empty()));
                    m.usage = usage;
                }
                let had_reply = s.reply_mut().is_some_and(|m| !m.text.is_empty());
                s.messages.retain(|m| m.role != Role::Agent || !m.text.is_empty() || !m.tools.is_empty());
                let outcome = match (cancelled, &result) {
                    (true, _) => Outcome::Stopped,
                    (false, Err(e)) => Outcome::Failed(e.clone()),
                    (false, Ok(())) => Outcome::Finished,
                };
                if cancelled {
                    s.push(Role::Error, STOPPED_NOTE);
                } else if let Err(e) = result {
                    s.push(Role::Error, format!("{FAILED_NOTE}\n{e}"));
                } else if !had_reply {
                    s.push(Role::Error, "The agent finished without replying.");
                }
                s.updated = store::now();
                let reply_text = s.messages.iter().rev().find(|m| m.role == Role::Agent).map_or_else(String::new, |m| m.text.clone());
                applied.ended = Some(Ended { title: s.title.clone(), outcome, reply: reply_text });
                // The first exchange of a chat that is still auto-titled gets a fast title.
                if fast_titles && ok && had_reply && !cancelled && s.title_auto && s.messages.len() == 2 && s.messages[0].role == Role::User {
                    applied.exchange = Some(Exchange { prompt: s.messages[0].text.clone(), reply: s.messages[1].text.clone() });
                }
                // A local site or HTML file in the reply opens in the preview.
                let reply = s
                    .messages
                    .iter()
                    .rev()
                    .find(|m| m.role == Role::Agent)
                    .filter(|_| ok && had_reply && !cancelled && auto_open);
                applied.preview = reply.and_then(|m| preview::find_preview_target(&m.text, &s.cwd));
            }
        }
        applied
    }

    /// Whether the running turn in `id` was told to stop, so its end is a stop rather than a failure.
    pub fn stopping(&self, id: &str) -> bool {
        self.live.get(id).is_some_and(|l| l.cancelled)
    }

    /// Stops the turn in `id`, if one runs. The turn's `Finished` event follows.
    /// Returns false for a remote chat: the caller sends the cancel to its device.
    pub fn stop(&mut self, id: &str) -> bool {
        if self.is_remote(id).is_some() {
            return false;
        }
        let Some(live) = self.live.get_mut(id) else { return false };
        let Some(turn) = &live.turn else { return false };
        live.cancelled = true;
        turn.cancel();
        true
    }

    // Projects

    pub fn projects(&self) -> &[Project] {
        &self.projects
    }

    pub fn project(&self, id: &str) -> Option<&Project> {
        self.projects.iter().find(|p| p.id == id)
    }

    /// Adds a project for `path` and returns its id.
    pub fn add_project(&mut self, name: &str, path: PathBuf) -> String {
        let id = store::new_id();
        self.projects.push(Project { id: id.clone(), name: name.into(), path, created: store::now() });
        id
    }

    /// Adds a project another device already has, under its id there, so this device's copy and that
    /// one are one project in the cloud. Returns false if the id is unsafe or already a project here.
    pub fn add_shared_project(&mut self, id: &str, name: &str, path: PathBuf) -> bool {
        if !crate::remote::cloud::safe_id(id) || self.project(id).is_some() {
            return false;
        }
        self.projects.push(Project { id: id.into(), name: name.into(), path, created: store::now() });
        true
    }

    /// Renames the project and moves it to `path`. Returns false if there is no such project.
    pub fn update_project(&mut self, id: &str, name: &str, path: PathBuf) -> bool {
        let Some(p) = self.projects.iter_mut().find(|p| p.id == id) else { return false };
        p.name = name.into();
        p.path = path;
        true
    }

    /// Removes the project. Its chats stay, ungrouped. Returns false if there is no such project.
    pub fn remove_project(&mut self, id: &str) -> bool {
        let before = self.projects.len();
        self.projects.retain(|p| p.id != id);
        if self.workspace.current_project.as_deref() == Some(id) {
            self.workspace.current_project = None;
        }
        // A chat from another device names that device's projects, so it is left alone.
        for s in self.sessions.iter_mut().filter(|s| s.project.as_deref() == Some(id) && s.origin.is_none()) {
            s.project = None;
            self.dirty.insert(s.id.clone());
        }
        self.projects.len() != before
    }
}

/// With "Restore sessions on launch" off, the window starts with no tiles open.
fn restore_tiles(workspace: &mut Workspace, settings: &AppSettings) {
    if !settings.restore {
        workspace.layout.tiles.clear();
        workspace.layout.focus = None;
    }
}

/// Copies attachments into the session's attachments dir, so the chat keeps them. Files already
/// there are kept as they are. Returns the kept files and an error line for each one that failed.
fn keep_files(session: &str, files: Vec<Attachment>) -> (Vec<Attachment>, Vec<String>) {
    let dir = store::attachments_dir(session);
    let mut kept = Vec::new();
    let mut errors = Vec::new();
    for mut a in files {
        if !a.path.starts_with(&dir) {
            // A hard link is free; fall back to a copy across filesystems.
            let linked = std::fs::create_dir_all(&dir).and_then(|()| {
                // Only the name's last part, so it can't point outside the dir.
                let name = std::path::Path::new(&a.name).file_name().ok_or(std::io::ErrorKind::InvalidInput)?;
                let dest = dir.join(name);
                if dest.exists() {
                    return Err(std::io::ErrorKind::AlreadyExists.into());
                }
                std::fs::hard_link(&a.path, &dest).map(|()| dest)
            });
            match linked.or_else(|_| store::store_attachment(session, &a.path)) {
                Ok(dest) => a.path = dest,
                Err(e) => {
                    errors.push(format!("Could not attach {}: {e}", a.name));
                    continue;
                }
            }
        }
        kept.push(a);
    }
    (kept, errors)
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_project_from_another_device_keeps_its_id_and_comes_in_once() {
        let mut app = state();
        assert!(app.add_shared_project("p-123", "app", "/home/me/app".into()));
        assert_eq!(app.project("p-123").map(|p| p.name.as_str()), Some("app"));
        assert!(!app.add_shared_project("p-123", "app", "/home/me/app-2".into()), "already here");
        assert!(!app.add_shared_project("../x", "bad", "/tmp".into()), "unsafe id");
        assert_eq!(app.projects().len(), 1);
    }

    use super::*;
    use crate::{store::ToolKind, ui::tiling::Layout};
    use smol::channel::unbounded;

    /// A rect big enough for several tiles at the minimum size.
    const WIDE: Rect = Rect { x: 0., y: 0., w: 2000., h: 1200. };

    fn state() -> AppState {
        AppState::new(Vec::new(), Vec::new(), Workspace::default(), AppSettings::default())
    }

    #[test]
    #[ignore = "sets ARCHONUI_DATA_DIR for the whole process; run alone with --ignored"]
    fn load_persists_sigils_assigned_to_old_sessions() {
        let _env = lock_env();
        let dir = std::env::temp_dir().join(format!("archonui-load-{}", store::new_id()));
        let data = dir.join("data");
        let cfg = dir.join("cfg");
        std::fs::create_dir_all(&data).unwrap();
        // Old sessions: no `icon` field, so `AppState::new` assigns one.
        let old = |id: &str| format!(
            r#"{{"id":"{id}","agent":"pi","title":"hi","cwd":"/tmp","created":1,"updated":2,"messages":[{{"role":"user","text":"hi"}},{{"role":"agent","text":"ok"}}]}}"#
        );
        for id in ["s1", "s2"] {
            std::fs::write(data.join(format!("{id}.json")), old(id)).unwrap();
        }
        // Point the default-path lookups at the temp dir, then restore them.
        let data_env = std::env::var_os("ARCHONUI_DATA_DIR");
        let cfg_env = std::env::var_os("ARCHONUI_CONFIG_DIR");
        std::env::set_var("ARCHONUI_DATA_DIR", &data);
        std::env::set_var("ARCHONUI_CONFIG_DIR", &cfg);
        let _guard = EnvGuard { data_env, cfg_env };

        AppState::load(&Paths::current());

        for id in ["s1", "s2"] {
            let bytes = std::fs::read(data.join(format!("{id}.json"))).unwrap();
            let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert!(v.get("icon").is_some_and(|i| i.as_u64().is_some()), "{id} icon stored");
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// Held by every test that points the data dir somewhere, since the env is shared by all test threads.
    static ENV: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn lock_env() -> std::sync::MutexGuard<'static, ()> {
        ENV.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Restores the two env vars on drop, so tests don't leak a temp dir into each other.
    struct EnvGuard {
        data_env: Option<std::ffi::OsString>,
        cfg_env: Option<std::ffi::OsString>,
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            fn restore(key: &str, value: Option<std::ffi::OsString>) {
                match value {
                    Some(v) => std::env::set_var(key, v),
                    None => std::env::remove_var(key),
                }
            }
            restore("ARCHONUI_DATA_DIR", self.data_env.take());
            restore("ARCHONUI_CONFIG_DIR", self.cfg_env.take());
        }
    }

    #[test]
    fn new_chats_stay_out_of_the_sidebar_until_sent_or_left_with_a_draft() {
        let mut app = state();
        let now = store::now();
        let listed = |app: &AppState| -> Vec<String> {
            app.sections(now).into_iter().flat_map(|(_, v)| v.into_iter().map(|s| s.id.clone())).collect()
        };
        let a = app.new_session("pi", "/tmp".into(), Placement::Focused);
        assert!(listed(&app).is_empty());
        // Typing in the focused chat doesn't list it.
        app.set_draft(&a, "half a thought");
        assert!(!app.list_left_drafts(false));
        assert!(listed(&app).is_empty());
        // Moving to another chat leaves the draft behind, so it is listed and the new one isn't.
        let b = app.new_session("pi", "/tmp".into(), Placement::Focused);
        assert!(app.list_left_drafts(false));
        assert_eq!(listed(&app), [a.clone()]);
        // Coming back and clearing the draft keeps it listed until it is deleted.
        app.workspace.layout.open_in_focused(&a);
        app.set_draft(&a, "");
        assert_eq!(listed(&app), [a.clone()]);
        app.delete_session(&a);
        assert!(listed(&app).is_empty());
        // Closing the window lists the focused chat's draft too; an empty one stays hidden.
        app.set_draft(&b, "on my way out");
        app.new_session("pi", "/tmp".into(), Placement::Focused);
        app.workspace.layout.open_in_focused(&b);
        app.list_left_drafts(true);
        assert_eq!(listed(&app), [b]);
    }

    #[test]
    fn closing_an_empty_chat_deletes_it() {
        let mut app = state();
        let empty = app.new_session("pi", "/tmp".into(), Placement::Focused);
        // The focused chat stays while it's empty.
        assert!(app.drop_abandoned().is_empty());
        let drafted = app.new_session("pi", "/tmp".into(), Placement::Split { area: WIDE, gap: 8. });
        app.set_draft(&drafted, "keep me");
        // Focus moved to the new chat, so the empty one goes, tile and all.
        assert_eq!(app.drop_abandoned(), [empty.clone()]);
        assert!(app.session(&empty).is_none());
        assert!(!app.workspace.layout.contains(&empty));
        // An empty chat with a file waiting to be sent is kept when left.
        let attached = app.new_session("pi", "/tmp".into(), Placement::Split { area: WIDE, gap: 8. });
        app.add_pending(&attached, vec![Attachment { name: "a.txt".into(), path: "/tmp/a.txt".into(), kind: engine::attach::Kind::Text, size: 1 }]);
        app.workspace.layout.focus(&drafted);
        assert!(app.drop_abandoned().is_empty());
        // A chat with a draft is listed when its tile closes, not deleted.
        app.workspace.layout.close(&drafted);
        app.workspace.layout.focus(&attached);
        app.list_left_drafts(false);
        assert!(app.drop_abandoned().is_empty());
        assert!(app.session(&drafted).is_some_and(|s| s.listed));
        // Emptying the waiting files and leaving deletes it.
        app.remove_pending(&attached, 0);
        app.workspace.layout.open_in_focused(&drafted);
        assert_eq!(app.drop_abandoned(), [attached.clone()]);
        assert!(app.session(&attached).is_none());
    }

    #[test]
    fn listed_drafts_load_back() {
        let dir = std::env::temp_dir().join(format!("archonui-drafts-{}", store::new_id()));
        let mut s = Session::new("pi", "/tmp".into());
        s.draft = "unsent".into();
        s.listed = true;
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(format!("{}.json", s.id)), serde_json::to_vec(&s).unwrap()).unwrap();
        let hidden = Session::new("pi", "/tmp".into());
        std::fs::write(dir.join(format!("{}.json", hidden.id)), serde_json::to_vec(&hidden).unwrap()).unwrap();
        let loaded = store::load_from(&dir);
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].draft, "unsent");
    }

    #[test]
    fn new_session_placement() {
        let mut app = state();
        let first = app.new_session("pi", "/tmp".into(), Placement::Focused);
        assert_eq!(app.workspace.layout.tiles, [first.as_str()]);

        let second = app.new_session("pi", "/tmp".into(), Placement::Split { area: WIDE, gap: 8. });
        assert_eq!(app.workspace.layout.tiles, [first.as_str(), second.as_str()]);
        assert_eq!(app.workspace.layout.focus.as_deref(), Some(second.as_str()));

        // Split at the tile cap, or where the area is too small, shows in the focused tile.
        let tiny = Rect { x: 0., y: 0., w: 100., h: 100. };
        let third = app.new_session("pi", "/tmp".into(), Placement::Split { area: tiny, gap: 8. });
        assert_eq!(app.workspace.layout.tiles, [first.as_str(), third.as_str()]);
        assert_eq!(app.workspace.layout.focus.as_deref(), Some(third.as_str()));
        assert_eq!(app.sessions().len(), 3, "every new chat is kept");
    }

    #[test]
    fn new_session_joins_the_current_project() {
        let mut app = state();
        let p = app.add_project("core", "/code/core".into());
        app.workspace.current_project = Some(p.clone());
        let id = app.new_session("pi", "/tmp".into(), Placement::Focused);
        assert_eq!(app.session(&id).unwrap().project.as_deref(), Some(p.as_str()));
    }

    #[test]
    fn new_session_runs_in_the_project_folder() {
        let dir = std::env::temp_dir().join(format!("archonui-project-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut app = state();
        let p = app.add_project("replica", dir.clone());
        app.workspace.current_project = Some(p);
        let id = app.new_session("pi", "/home".into(), Placement::Focused);
        assert_eq!(app.session(&id).unwrap().cwd, dir, "the project folder wins over the default folder");

        // A project whose folder is gone falls back to the folder given.
        let gone = app.add_project("gone", dir.join("missing"));
        app.workspace.current_project = Some(gone);
        let id = app.new_session("pi", "/tmp".into(), Placement::Focused);
        assert_eq!(app.session(&id).unwrap().cwd, PathBuf::from("/tmp"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn icons_are_unique() {
        let mut app = state();
        let ids: Vec<String> = (0..20).map(|_| app.new_session("pi", "/tmp".into(), Placement::Focused)).collect();
        let icons: BTreeSet<u16> = app.sessions().iter().map(|s| s.icon).collect();
        assert_eq!(icons.len(), ids.len());

        // Sessions loaded with duplicate or missing icons are fixed up on load.
        let mut a = Session::new("pi", "/tmp".into());
        let mut b = Session::new("pi", "/tmp".into());
        let mut c = Session::new("pi", "/tmp".into());
        a.icon = 7;
        b.icon = 7;
        c.icon = UNASSIGNED_ICON;
        let loaded = AppState::new(vec![a, b, c], Vec::new(), Workspace::default(), AppSettings::default());
        let icons: BTreeSet<u16> = loaded.sessions().iter().map(|s| s.icon).collect();
        assert_eq!(icons.len(), 3);
        assert!(!icons.contains(&UNASSIGNED_ICON));
    }

    #[test]
    fn project_crud() {
        let mut app = state();
        let id = app.add_project("core", "/code/core".into());
        assert_eq!(app.project(&id).unwrap().name, "core");

        assert!(app.update_project(&id, "archonui core", "/code/archonui".into()));
        assert_eq!(app.project(&id).unwrap().path, PathBuf::from("/code/archonui"));
        assert!(!app.update_project("missing", "x", "/x".into()));

        let session = app.new_session("pi", "/tmp".into(), Placement::Focused);
        app.sessions.iter_mut().for_each(|s| s.project = Some(id.clone()));
        app.workspace.current_project = Some(id.clone());
        assert!(app.remove_project(&id));
        assert!(app.projects().is_empty());
        assert!(app.workspace.current_project.is_none());
        assert_eq!(app.session(&session).unwrap().project, None, "chats stay, ungrouped");
        assert!(!app.remove_project(&id));
    }

    #[test]
    fn clear_history_keeps_tiled_sessions() {
        let mut app = state();
        let tiled = app.new_session("pi", "/tmp".into(), Placement::Focused);
        let old = Session::new("pi", "/tmp".into());
        let old_id = old.id.clone();
        app.sessions.push(old);
        app.workspace.layout = Layout { tiles: vec![tiled.clone()], ..Layout::default() };

        assert_eq!(app.clear_history(), [old_id]);
        assert_eq!(app.sessions().len(), 1);
        assert_eq!(app.sessions()[0].id, tiled);
        assert!(app.workspace.layout.contains(&tiled));
    }

    #[test]
    fn move_to_project_leaves_other_devices_chats() {
        let mut app = state();
        let mut theirs = Session::new("pi", "/tmp".into());
        theirs.push(Role::User, "hi");
        let id = theirs.id.clone();
        app.put_remote(theirs, "laptop", false);
        assert!(!app.move_to_project(&id, Some("p1".into())));
        assert!(app.session(&id).is_some_and(|s| s.project.is_none()));
    }

    #[test]
    fn a_requested_chat_expires_when_left_idle_or_its_asker_goes() {
        let mut app = state();
        let idle = Duration::from_secs(600);
        let asked = app.new_session("pi", "/tmp".into(), Placement::Elsewhere { conn: 7 });
        let open = |conn: u64| conn == 7;
        let now = Instant::now();
        assert!(app.expire_requested(open, idle, now).is_empty());
        // A draft typed in it counts as use.
        app.set_draft(&asked, "half a prompt");
        assert!(app.expire_requested(open, idle, now + Duration::from_secs(300)).is_empty());
        assert_eq!(app.expire_requested(open, idle, Instant::now() + idle), [asked.clone()]);
        assert!(app.session(&asked).is_none());

        // Its asker's connection closing drops it at once.
        let asked = app.new_session("pi", "/tmp".into(), Placement::Elsewhere { conn: 8 });
        assert_eq!(app.expire_requested(open, idle, Instant::now()), [asked]);

        // One that got a message is an ordinary chat, and a chat of this window is never touched.
        let used = app.new_session("pi", "/tmp".into(), Placement::Elsewhere { conn: 9 });
        app.sessions.iter_mut().find(|s| s.id == used).unwrap().push(Role::User, "hi");
        let mine = app.new_session("pi", "/tmp".into(), Placement::Focused);
        assert!(app.expire_requested(|_| false, idle, Instant::now() + idle).is_empty());
        assert!(app.session(&used).is_some() && app.session(&mine).is_some());
    }

    #[test]
    fn a_draft_left_in_another_devices_chat_is_not_listed_here() {
        let mut app = state();
        let theirs = Session::new("pi", "/tmp".into());
        let id = theirs.id.clone();
        app.put_remote(theirs, "laptop", false);
        app.set_draft(&id, "half a prompt");
        assert!(!app.list_left_drafts(true));
        assert!(app.session(&id).is_some_and(|s| !s.listed));
    }

    #[test]
    fn clear_history_keeps_other_devices_chats() {
        let mut app = state();
        let mut theirs = Session::new("pi", "/tmp".into());
        theirs.push(Role::User, "hi");
        let id = theirs.id.clone();
        app.put_remote(theirs, "laptop", false);
        assert!(app.clear_history().is_empty());
        assert!(app.session(&id).is_some());
    }

    #[test]
    fn sections_group_by_last_use() {
        let mut app = state();
        let now = 10 * 86_400 + 12 * 3600;
        let mut recent = Session::new("pi", "/tmp".into());
        recent.updated = now - 60;
        let mut older = Session::new("pi", "/tmp".into());
        older.updated = now - 5 * 86_400;
        app.sessions = vec![older, recent];
        for s in &mut app.sessions {
            s.push(Role::User, "hi");
        }
        app.sessions[0].updated = now - 5 * 86_400;
        app.sessions[1].updated = now - 60;
        let groups = app.sections(now);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].0, Section::Today);
        assert_eq!(groups[1].0, Section::Earlier);
        assert!(groups[0].1[0].updated > groups[1].1[0].updated);
    }

    #[test]
    fn apply_builds_the_reply_from_the_event_stream() {
        let mut app = state();
        let id = app.new_session("pi", "/tmp".into(), Placement::Focused);
        let s = app.sessions.iter_mut().find(|s| s.id == id).unwrap();
        s.push(Role::User, "hi");
        s.push(Role::Agent, "");

        let (tx, rx) = unbounded();
        for ev in [
            Event::Out(Out::Text("Hel".into())),
            Event::Out(Out::Text("lo".into())),
            Event::Out(Out::Break),
            Event::Out(Out::Text("two".into())),
            Event::Out(Out::Session("thread-9".into())),
            Event::Out(Out::Status("Running ls".into())),
            Event::Out(Out::Error("warning".into())),
            Event::Finished(Ok(())),
        ] {
            tx.try_send(ev).unwrap();
        }
        drop(tx);

        let mut applied = Vec::new();
        smol::block_on(async {
            while let Ok(ev) = rx.recv().await {
                applied.push(app.apply(&id, ev));
            }
        });

        let s = app.session(&id).unwrap();
        assert_eq!(s.agent_session.as_deref(), Some("thread-9"));
        assert_eq!(s.messages.len(), 3, "user, reply, warning");
        assert_eq!(s.messages[1].text, "Hello\n\ntwo");
        assert_eq!(s.messages[2].role, Role::Error);
        assert_eq!(app.activity(&id), ["Running ls", "Finished"]);
        assert!(app.status(&id).is_none());
        assert_eq!(applied.iter().filter(|a| a.finished).count(), 1);
        assert!(applied.last().unwrap().finished);
    }

    #[test]
    fn apply_reports_failures_and_empty_replies() {
        let mut app = state();
        let id = app.new_session("pi", "/tmp".into(), Placement::Focused);
        let s = app.sessions.iter_mut().find(|s| s.id == id).unwrap();
        s.push(Role::User, "hi");
        s.push(Role::Agent, "");

        app.apply(&id, Event::Finished(Err("exit 2".into())));
        let s = app.session(&id).unwrap();
        assert_eq!(s.messages.len(), 2, "the empty reply is dropped");
        assert!(s.messages[1].text.starts_with("Agent exited with an error:"));
    }

    #[test]
    fn apply_records_tool_rows_and_thinking_time() {
        let mut app = state();
        let id = app.new_session("claude", "/tmp".into(), Placement::Focused);
        let s = app.sessions.iter_mut().find(|s| s.id == id).unwrap();
        s.push(Role::User, "fix it");
        s.push(Role::Agent, "");

        for ev in [
            Event::Out(Out::ToolStart { id: "t1".into(), kind: ToolKind::Edit, target: "src/server/cache.ts".into() }),
            Event::Out(Out::Thinking(true)),
            Event::Out(Out::Thinking(false)),
            Event::Out(Out::ToolEnd { id: "t1".into(), ok: true, detail: Some("+41".into()) }),
            Event::Out(Out::ToolStart { id: "t2".into(), kind: ToolKind::Run, target: "pnpm test".into() }),
            Event::Out(Out::ToolEnd { id: "t2".into(), ok: true, detail: Some("128 passed".into()) }),
            Event::Out(Out::Text("Done.".into())),
            Event::Finished(Ok(())),
        ] {
            app.apply(&id, ev);
        }

        let reply = &app.session(&id).unwrap().messages[1];
        assert_eq!(reply.text, "Done.");
        assert_eq!(
            reply.tools,
            [
                ToolCall {
                    kind: ToolKind::Edit,
                    target: "src/server/cache.ts".into(),
                    detail: Some("+41".into()),
                    ok: Some(true),
                },
                ToolCall {
                    kind: ToolKind::Run,
                    target: "pnpm test".into(),
                    detail: Some("128 passed".into()),
                    ok: Some(true),
                },
            ]
        );
        assert!(reply.thought_secs.is_some(), "thinking was reported");
    }

    #[test]
    fn thought_time_needs_thinking_events_and_tool_only_replies_are_kept() {
        let mut app = state();
        let id = app.new_session("opencode", "/tmp".into(), Placement::Focused);
        let s = app.sessions.iter_mut().find(|s| s.id == id).unwrap();
        s.push(Role::User, "list");
        s.push(Role::Agent, "");

        app.apply(&id, Event::Out(Out::ToolStart { id: "g".into(), kind: ToolKind::Grep, target: "hello".into() }));
        app.apply(&id, Event::Out(Out::ToolEnd { id: "g".into(), ok: false, detail: None }));
        app.apply(&id, Event::Finished(Ok(())));

        let s = app.session(&id).unwrap();
        assert_eq!(s.messages[1].tools.len(), 1, "the tool row survives an empty reply");
        assert_eq!(s.messages[1].thought_secs, None, "no thinking was reported");
        assert_eq!(s.messages[2].role, Role::Error);
    }

    #[test]
    fn the_text_after_the_last_tool_call_is_the_final_answer() {
        let mut app = state();
        let id = app.new_session("claude", "/tmp".into(), Placement::Focused);
        let s = app.sessions.iter_mut().find(|s| s.id == id).unwrap();
        s.push(Role::User, "fix it");
        s.push(Role::Agent, "");
        let tool = |n: &str| Event::Out(Out::ToolStart { id: n.into(), kind: ToolKind::Run, target: "cargo test".into() });
        let text = |t: &str| Event::Out(Out::Text(t.into()));
        for ev in [text("Looking."), tool("a"), Event::Out(Out::Break), text("Tests fail; fixing."), tool("b"), Event::Out(Out::Break), text("Fixed it."), text(" All pass.")] {
            app.apply(&id, ev);
        }
        app.apply(&id, Event::Out(Out::Usage(Usage { input: 5, output: 3, ..Usage::default() })));
        app.apply(&id, Event::Out(Out::Usage(Usage { input: 1, output: 1, ..Usage::default() })));
        app.apply(&id, Event::Finished(Ok(())));
        let m = &app.session(&id).unwrap().messages[1];
        let at = m.final_at.expect("a final answer");
        assert_eq!(&m.text[at..], "Fixed it. All pass.");
        assert_eq!(m.usage.map(|u| (u.input, u.output)), Some((6, 4)));

        // A reply with no text before its tool calls has nothing to set apart.
        let id = app.new_session("claude", "/tmp".into(), Placement::Focused);
        let s = app.sessions.iter_mut().find(|s| s.id == id).unwrap();
        s.push(Role::User, "go");
        s.push(Role::Agent, "");
        for ev in [tool("c"), text("Done.")] {
            app.apply(&id, ev);
        }
        app.apply(&id, Event::Finished(Ok(())));
        assert_eq!(app.session(&id).unwrap().messages[1].final_at, None);
    }

    #[test]
    fn a_final_answer_that_replaces_whitespace_does_not_split_a_char() {
        let mut app = state();
        let id = app.new_session("claude", "/tmp".into(), Placement::Focused);
        let s = app.sessions.iter_mut().find(|s| s.id == id).unwrap();
        s.push(Role::User, "hi");
        s.push(Role::Agent, "");
        let tool = |n: &str| Event::Out(Out::ToolStart { id: n.into(), kind: ToolKind::Run, target: "ls".into() });
        let text = |t: &str| Event::Out(Out::Text(t.into()));
        for ev in [tool("a"), text("\n\n"), tool("b"), text("\n"), Event::Out(Out::Final("héllo".into())), Event::Finished(Ok(()))] {
            app.apply(&id, ev);
        }
        let m = &app.session(&id).unwrap().messages[1];
        assert_eq!(m.text, "héllo");
        assert_eq!(m.final_at, None);
    }

    #[test]
    fn stop_without_a_turn_does_nothing() {
        let mut app = state();
        assert!(!app.stop("nothing"));
        assert!(!app.busy("nothing"));
    }

    /// A chat whose first message was sent, with the agent's reply still streaming in.
    fn first_turn(app: &mut AppState, title_auto: bool) -> String {
        let id = app.new_session("claude", "/tmp".into(), Placement::Focused);
        let s = app.sessions.iter_mut().find(|s| s.id == id).unwrap();
        s.title = "fix the cache".into();
        s.title_auto = title_auto;
        s.push(Role::User, "fix the cache");
        s.push(Role::Agent, "");
        app.apply(&id, Event::Out(Out::Text("Done.".into())));
        id
    }

    #[test]
    fn the_first_exchange_of_an_auto_titled_chat_asks_for_a_fast_title() {
        let mut app = state();
        let id = first_turn(&mut app, true);
        let applied = app.apply(&id, Event::Finished(Ok(())));
        assert_eq!(applied.exchange, Some(Exchange { prompt: "fix the cache".into(), reply: "Done.".into() }));
        let ended = applied.ended.unwrap();
        assert_eq!((ended.outcome, ended.reply.as_str()), (Outcome::Finished, "Done."));

        assert!(app.apply_title(&id, "Cache fix"));
        assert_eq!(app.session(&id).unwrap().title, "Cache fix");
    }

    #[test]
    fn fast_titles_need_the_setting_an_auto_title_and_a_first_exchange() {
        let mut app = state();
        app.settings.fast_titles = false;
        let off = first_turn(&mut app, true);
        assert!(app.apply(&off, Event::Finished(Ok(()))).exchange.is_none(), "the setting is off");

        app.settings.fast_titles = true;
        let typed = first_turn(&mut app, false);
        assert!(app.apply(&typed, Event::Finished(Ok(()))).exchange.is_none(), "the title was typed");

        let second = first_turn(&mut app, true);
        app.apply(&second, Event::Finished(Ok(())));
        let s = app.sessions.iter_mut().find(|s| s.id == second).unwrap();
        s.push(Role::User, "and the tests?");
        s.push(Role::Agent, "");
        app.apply(&second, Event::Out(Out::Text("Yes.".into())));
        assert!(app.apply(&second, Event::Finished(Ok(()))).exchange.is_none(), "only the first exchange");
    }

    #[test]
    fn a_renamed_chat_keeps_its_name_over_a_fast_title() {
        let mut app = state();
        let id = first_turn(&mut app, true);
        assert!(app.rename_session(&id, "Mine"));
        assert!(!app.apply_title(&id, "Cache fix"));
        assert_eq!(app.session(&id).unwrap().title, "Mine");
    }

    #[test]
    fn a_failed_turn_reports_its_outcome_without_a_fast_title() {
        let mut app = state();
        let id = first_turn(&mut app, true);
        let applied = app.apply(&id, Event::Finished(Err("exit 2".into())));
        assert!(applied.exchange.is_none());
        assert_eq!(applied.ended.unwrap().outcome, Outcome::Failed("exit 2".into()));
    }

    #[test]
    fn restore_off_starts_without_tiles() {
        let mut w = Workspace::default();
        w.layout.tiles = vec!["a".into(), "b".into()];
        w.layout.focus = Some("b".into());
        restore_tiles(&mut w, &AppSettings::default());
        assert_eq!(w.layout.tiles.len(), 2, "restore is on by default");
        restore_tiles(&mut w, &AppSettings { restore: false, ..Default::default() });
        assert!(w.layout.tiles.is_empty() && w.layout.focus.is_none());
    }

    #[test]
    fn a_chat_named_before_its_first_message_keeps_the_name() {
        let mut app = state();
        let id = app.new_session("pi", "/tmp".into(), Placement::Focused);
        assert!(app.rename_session(&id, "Mine"));
        let pi = engine::harness("pi").unwrap();
        let agents = [DetectedAgent {
            harness: pi,
            path: "/nonexistent/archonui-test-pi".into(),
            version: String::new(),
            enabled: true,
            auth: pi.auth_status(),
        }];
        app.send(&id, "hello there", &agents);
        let s = app.session(&id).unwrap();
        assert_eq!(s.title, "Mine");
        assert!(!s.title_auto);

        // A terminal chat's placeholder title gives way to the first message.
        let term = app.new_session("pi", "/tmp".into(), Placement::Focused);
        app.mark_terminal(&term);
        app.send(&term, "build it", &agents);
        assert_eq!(app.session(&term).unwrap().title, "build it");
    }

    #[test]
    fn switching_agents_starts_a_new_agent_session_that_gets_the_chat_so_far() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("archonui-switch-{}", store::new_id()));
        std::fs::create_dir_all(&dir).unwrap();
        // A fake CLI that records its arguments and stdin.
        let seen = dir.join("seen");
        let bin = dir.join("fake-agent");
        std::fs::write(&bin, format!("#!/bin/sh\nprintf '%s\\n' \"$@\" > '{0}'\ncat >> '{0}'\n", seen.display())).unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        let detected = |id: &str| {
            let h = engine::harness(id).unwrap();
            DetectedAgent { harness: h, path: bin.clone(), version: String::new(), enabled: true, auth: h.auth_status() }
        };
        let agents = [detected("pi"), detected("codex")];
        let run = |app: &mut AppState, id: &str, prompt: &str| {
            let events = app.send(id, prompt, &agents).expect("turn starts");
            while let Ok(e) = smol::block_on(events.recv()) {
                if app.apply(id, e).finished {
                    break;
                }
            }
            std::fs::read_to_string(&seen).unwrap()
        };

        let mut app = state();
        let id = app.new_session("pi", dir.clone(), Placement::Focused);
        assert!(!app.switch_agent(&id, "pi", None, None, Mode::Auto), "already on pi");
        app.switch_agent(&id, "codex", None, None, Mode::Auto);
        assert!(!app.session(&id).unwrap().handoff, "an empty chat has nothing to hand over");
        app.switch_agent(&id, "pi", None, None, Mode::Auto);

        run(&mut app, &id, "fix the parser");
        // The fake CLI says nothing; stand in its reply.
        let s = app.sessions.iter_mut().find(|s| s.id == id).unwrap();
        s.messages.retain(|m| m.role != Role::Error);
        s.push(Role::Agent, "Fixed it.");
        assert_eq!(s.messages[1].agent.as_deref(), Some("pi"), "replies record their agent");
        assert!(s.agent_session.is_some(), "pi's session was assigned");

        assert!(app.switch_agent(&id, "codex", Some("gpt-5".into()), None, Mode::Plan));
        let s = app.session(&id).unwrap();
        assert_eq!((s.agent.as_str(), s.agent_session.as_deref(), s.model.as_deref(), s.mode), ("codex", None, Some("gpt-5"), Mode::Plan));
        assert!(s.handoff);

        // A slash command goes through as typed and leaves the handoff for the next message.
        let slash = run(&mut app, &id, "/status");
        assert!(!slash.contains("<conversation>"), "{slash}");
        assert!(app.session(&id).unwrap().handoff);

        let first = run(&mut app, &id, "now add a test");
        assert!(first.contains("[User]\nfix the parser") && first.contains("[Pi]\nFixed it."), "{first}");
        assert!(first.contains("The user's new message:\n\nnow add a test"), "{first}");
        let s = app.session(&id).unwrap();
        assert!(!s.handoff);
        assert_eq!(s.messages.iter().filter(|m| m.role == Role::User).last().unwrap().text, "now add a test", "the chat shows what was typed");

        let next = run(&mut app, &id, "thanks");
        assert!(!next.contains("<conversation>"), "only the first message carries the chat: {next}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn deleted_sessions_leave_their_tiles() {
        let mut app = state();
        let id = app.new_session("pi", "/tmp".into(), Placement::Focused);
        app.delete_session(&id);
        assert!(app.sessions().is_empty());
        assert!(app.workspace.layout.tiles.is_empty());
    }

    #[test]
    fn flush_sessions_reports_errors_when_dir_is_read_only() {
        let _env = lock_env();
        let dir = std::env::temp_dir().join(format!("archonui-ro-test-{}", store::new_id()));
        std::fs::write(&dir, b"blocker").unwrap();
        let data_env = std::env::var_os("ARCHONUI_DATA_DIR");
        std::env::set_var("ARCHONUI_DATA_DIR", &dir);
        let _guard = EnvGuard { data_env, cfg_env: None };

        let mut app = state();
        let id = app.new_session("pi", "/tmp".into(), Placement::Focused);
        let s = app.sessions.iter_mut().find(|s| s.id == id).unwrap();
        s.push(Role::User, "test");
        s.push(Role::Agent, "reply");
        app.dirty.insert(id.clone());

        let failed = app.flush_sessions();
        assert!(!failed.is_empty(), "reports save failure");
        assert!(failed[0].contains("Couldn't save the chat"));
        assert!(app.dirty.contains(&id), "a failed save stays dirty for the next flush");

        let _ = std::fs::remove_file(dir);
    }

    #[test]
    fn failed_flush_keeps_chats_queued_for_retry() {
        let _env = lock_env();
        let dir = std::env::temp_dir().join(format!("archonui-retry-test-{}", store::new_id()));
        std::fs::write(&dir, b"blocker").unwrap();
        let data_env = std::env::var_os("ARCHONUI_DATA_DIR");
        std::env::set_var("ARCHONUI_DATA_DIR", &dir);
        let _guard = EnvGuard { data_env, cfg_env: None };

        let mut app = state();
        let id = app.new_session("pi", "/tmp".into(), Placement::Focused);
        let s = app.sessions.iter_mut().find(|s| s.id == id).unwrap();
        s.push(Role::User, "test");
        s.push(Role::Agent, "reply");
        app.dirty.insert(id.clone());
        app.removed.push("archonui-gone-chat".into());

        assert_eq!(app.flush_sessions().len(), 2, "both the save and the delete fail");
        assert!(app.dirty.contains(&id));
        assert_eq!(app.removed, ["archonui-gone-chat"]);

        // Make the data dir writable, then the queued work goes through on the next flush.
        std::fs::remove_file(&dir).unwrap();
        std::fs::create_dir_all(&dir).unwrap();
        assert!(app.flush_sessions().is_empty());
        assert!(app.dirty.is_empty());
        assert!(app.removed.is_empty());
        assert!(dir.join(format!("{id}.json")).exists(), "the chat is saved on retry");

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn errors_after_stop_are_dropped() {
        let mut app = state();
        let id = app.new_session("opencode", "/tmp".into(), Placement::Focused);
        app.sessions.iter_mut().find(|s| s.id == id).unwrap().push(Role::User, "long task");
        let live = app.live.entry(id.clone()).or_default();
        live.turn = Some(engine::Turn::mock());
        app.apply(&id, Event::Out(Out::Error("rate limited".into())));
        assert!(app.stop(&id));
        app.apply(&id, Event::Out(Out::Error("Transport: The socket connection was closed unexpectedly.".into())));
        let errors: Vec<_> = app.sessions.iter().find(|s| s.id == id).unwrap().messages.iter().filter(|m| m.role == Role::Error).map(|m| m.text.clone()).collect();
        assert_eq!(errors, ["rate limited"]);
    }

    #[test]
    fn shutdown_stops_running_turns_and_saves_partial_reply() {
        let _env = lock_env();
        let dir = std::env::temp_dir().join(format!("archonui-shutdown-test-{}", store::new_id()));
        std::fs::create_dir_all(&dir).unwrap();
        let data_env = std::env::var_os("ARCHONUI_DATA_DIR");
        std::env::set_var("ARCHONUI_DATA_DIR", &dir);
        let _guard = EnvGuard { data_env, cfg_env: None };

        let mut app = state();
        let id = app.new_session("pi", "/tmp".into(), Placement::Focused);
        let s = app.sessions.iter_mut().find(|s| s.id == id).unwrap();
        s.push(Role::User, "long task");
        s.push(Role::Agent, "partial reply here");
        let live = app.live.entry(id.clone()).or_default();
        live.started = Some(Instant::now());
        live.turn = Some(engine::Turn::mock());

        app.shutdown();

        // After shutdown, live.turn should be None and session on disk should contain the reply and Stopped
        let saved = store::load_all().into_iter().find(|s| s.id == id).expect("session persisted");
        assert_eq!(saved.messages.len(), 3);
        assert_eq!(saved.messages[1].text, "partial reply here");
        assert_eq!(saved.messages[2].role, Role::Error);
        assert_eq!(saved.messages[2].text, "Stopped.");

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_remote_turn_streams_into_its_chat() {
        let mut app = state();
        let chat = Session::new("pi", "/home/host".into());
        let id = chat.id.clone();
        app.put_remote(chat, "laptop", false);
        assert_eq!(app.is_remote(&id), Some("laptop"));
        assert!(!app.busy(&id));

        app.remote_started(&id, "say hello");
        assert!(app.busy(&id));
        app.remote_event(&id, Out::Text("Hel".into()));
        app.remote_event(&id, Out::Text("lo".into()));
        assert!(app.busy(&id), "busy until the turn ends");
        let applied = app.remote_finished(&id, None, false);
        assert!(applied.finished);
        assert!(!app.busy(&id));

        let s = app.session(&id).unwrap();
        assert_eq!(s.title, "say hello");
        let reply = s.messages.iter().rev().find(|m| m.role == Role::Agent).unwrap();
        assert_eq!(reply.text, "Hello");

        // Stopping a remote chat is the caller's job, sent to its device.
        assert!(!app.stop(&id));
        // Deleting a remote chat drops it from memory without a file delete here.
        app.delete_session(&id);
        assert!(app.session(&id).is_none());
        assert!(app.removed.is_empty());
    }

    #[test]
    fn remote_chats_are_never_saved_here() {
        let _env = lock_env();
        let dir = std::env::temp_dir().join(format!("archonui-remote-test-{}", store::new_id()));
        std::fs::create_dir_all(&dir).unwrap();
        let data_env = std::env::var_os("ARCHONUI_DATA_DIR");
        std::env::set_var("ARCHONUI_DATA_DIR", &dir);
        let _guard = EnvGuard { data_env, cfg_env: None };

        let mut app = state();
        let chat = Session::new("pi", "/home/host".into());
        let id = chat.id.clone();
        app.put_remote(chat, "laptop", false);
        app.remote_started(&id, "hi");
        app.remote_event(&id, Out::Text("there".into()));
        app.remote_finished(&id, Some("exit 2".into()), false);
        assert!(app.flush_sessions().is_empty());
        assert!(app.dirty.is_empty());
        assert!(!dir.join(format!("{id}.json")).exists(), "no file for a remote chat");

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_remote_turn_stopped_on_its_device_shows_as_stopped() {
        let mut app = state();
        let chat = Session::new("pi", "/home/host".into());
        let id = chat.id.clone();
        app.put_remote(chat, "pc", false);
        app.remote_started(&id, "hi");
        app.remote_event(&id, Out::Text("par".into()));
        let applied = app.remote_finished(&id, Some("signal: 15 (SIGTERM)".into()), true);
        assert!(matches!(applied.ended.unwrap().outcome, Outcome::Stopped));
        assert_eq!(app.session(&id).unwrap().messages.last().unwrap().text, STOPPED_NOTE);
    }

    #[test]
    fn an_adopted_remote_chat_continues_here_with_its_history() {
        let mut app = state();
        let mut chat = Session::new("claude", "/home/pc/app".into());
        chat.agent_session = Some("pc-thread".into());
        chat.push(Role::User, "fix the bug");
        chat.push(Role::Agent, "Fixed.");
        let id = chat.id.clone();
        app.put_remote(chat, "pc", false);
        let new_id = app.adopt_remote(&id, "/home/laptop/app".into(), "pc").expect("adopts remote chat");
        assert_ne!(new_id, id, "new id differs from old");
        let s = app.session(&new_id).unwrap();
        assert!(s.origin.is_none() && s.handoff && s.agent_session.is_none());
        assert_eq!(s.handoff_from.as_deref(), Some("pc"), "the agent is told the files are on the pc");
        assert_eq!(s.cwd, PathBuf::from("/home/laptop/app"));
        assert_eq!(s.messages.len(), 2, "history copied");
        assert!(app.dirty.contains(&new_id), "saved here from now on");
        let remote = app.session(&id).unwrap();
        assert_eq!(remote.origin.as_deref(), Some("pc"), "original remote session untouched");
    }

    #[test]
    fn turns_from_another_device_never_touch_a_chat_of_this_device() {
        let mut app = state();
        let mut chat = Session::new("claude", "/home/pc/app".into());
        chat.push(Role::User, "fix the bug");
        chat.push(Role::Agent, "Fixed.");
        let id = chat.id.clone();
        app.put_remote(chat, "pc", false);
        let new_id = app.adopt_remote(&id, "/home/laptop/app".into(), "pc").expect("adopts remote");
        app.dirty.clear();
        // The pc comes back and goes on with its own copy: that copy changes, the one continued here doesn't.
        app.remote_started(&id, "and the tests?");
        app.remote_event(&id, Out::Text("Done.".into()));
        app.remote_finished(&id, None, false);
        assert_eq!(app.session(&id).unwrap().messages.len(), 4, "the pc's copy follows the pc");
        let local = app.session(&new_id).unwrap();
        assert_eq!(local.messages.len(), 2, "the chat continued here keeps only its own messages");
        assert!(local.origin.is_none());
        // Turns are never applied to a chat of this device, even one named by id.
        assert_eq!(app.remote_event(&new_id, Out::Text("x".into())), Applied::default());
        assert_eq!(app.remote_finished(&new_id, None, false), Applied::default());
        assert_eq!(app.session(&new_id).unwrap().messages.len(), 2);
        assert!(!app.dirty.contains(&new_id), "nothing of this device's chat changed");
        // Nor can a remote chat's agent be switched here: it runs on its device.
        let remote = Session::new("claude", "/home/pc".into());
        let remote_id = remote.id.clone();
        app.put_remote(remote, "pc", false);
        assert!(!app.switch_agent(&remote_id, "codex", None, None, Mode::default()));
        assert_eq!(app.session(&remote_id).unwrap().agent, "claude");
    }

    #[test]
    fn a_remote_chat_id_never_leaves_its_folder_or_replaces_a_local_chat() {
        let mut app = state();
        // An id that would leave the data folder once the chat is saved, adopted or deleted here.
        for bad in ["../../..", "a/b", "", ".."] {
            let mut chat = Session::new("pi", "/home/host".into());
            chat.id = bad.into();
            app.put_remote(chat, "pc", false);
            assert!(app.session(bad).is_none(), "{bad:?} is refused");
            assert!(app.adopt_remote(bad, "/tmp".into(), "pc").is_none());
        }
        let local = app.new_session("pi", "/tmp".into(), Placement::Focused);
        let mut chat = Session::new("pi", "/home/host".into());
        chat.id = local.clone();
        chat.title = "forged".into();
        app.put_remote(chat, "pc", false);
        assert!(app.is_remote(&local).is_none());
        assert_ne!(app.session(&local).unwrap().title, "forged");
    }

    #[test]
    fn removing_a_device_drops_its_chats_and_tiles() {
        let mut app = state();
        let local = app.new_session("pi", "/tmp".into(), Placement::Focused);
        let remote = Session::new("pi", "/home/host".into());
        let remote_id = remote.id.clone();
        app.put_remote(remote, "laptop", true);
        app.workspace.layout = Layout { tiles: vec![local.clone(), remote_id.clone()], focus: Some(remote_id.clone()), ..Layout::default() };

        app.remove_remote("laptop");
        assert!(app.session(&remote_id).is_none());
        assert!(!app.busy(&remote_id));
        assert_eq!(app.workspace.layout.tiles, [local.as_str()]);
        assert!(app.workspace.layout.focus.is_none(), "focus left the removed tile");
        assert!(app.session(&local).is_some(), "this device's chats stay");
        assert!(app.removed.is_empty(), "nothing to delete on disk");
    }

    #[test]
    fn marking_a_device_away_keeps_its_chats_and_tiles() {
        let mut app = state();
        let local = app.new_session("pi", "/tmp".into(), Placement::Focused);
        let mut from_cloud = Session::new("pi", "/home/host".into());
        let cloud_id = from_cloud.id.clone();
        from_cloud.title = "Cloud Copy".into();
        app.put_remote(from_cloud, "pc", false);
        let busy_chat = Session::new("pi", "/home/host".into());
        let busy_id = busy_chat.id.clone();
        app.put_remote(busy_chat, "pc", true);
        app.workspace.layout = Layout { tiles: vec![local.clone(), cloud_id.clone(), busy_id.clone()], focus: Some(busy_id.clone()), ..Layout::default() };

        app.remote_away("pc");
        assert!(app.session(&cloud_id).is_some(), "cloud chat stays");
        assert!(app.session(&busy_id).is_some(), "busy chat stays");
        assert!(!app.busy(&busy_id), "busy mark removed");
        assert_eq!(app.workspace.layout.tiles, [local.as_str(), cloud_id.as_str(), busy_id.as_str()], "tiles unchanged");
        assert_eq!(app.workspace.layout.focus.as_deref(), Some(busy_id.as_str()), "focus unchanged");
        assert!(app.session(&local).is_some(), "this device's chats stay");
    }

    #[test]
    fn adopting_a_remote_chat_creates_a_new_local_one_keeping_the_remote() {
        let mut app = state();
        let mut chat = Session::new("claude", "/home/pc/app".into());
        chat.push(Role::User, "fix the bug");
        chat.push(Role::Agent, "Fixed.");
        let old_id = chat.id.clone();
        app.put_remote(chat, "pc", false);
        // Put the remote chat in a tile with focus.
        app.workspace.layout = Layout {
            tiles: vec![old_id.clone()],
            focus: Some(old_id.clone()),
            ..Layout::default()
        };
        let new_id = app.adopt_remote(&old_id, "/home/laptop/app".into(), "pc").expect("adopts remote chat");
        // New id differs from old.
        assert_ne!(new_id, old_id);
        // New session has the expected properties.
        let new_sess = app.session(&new_id).unwrap();
        assert!(new_sess.origin.is_none(), "origin is None");
        assert_eq!(new_sess.messages.len(), 2, "same messages");
        assert!(new_sess.handoff, "handoff is true");
        assert_eq!(new_sess.handoff_from.as_deref(), Some("pc"));
        // Old remote session still exists untouched.
        let old_sess = app.session(&old_id).unwrap();
        assert_eq!(old_sess.origin.as_deref(), Some("pc"), "origin unchanged");
        // Tile and focus point to the new id.
        assert_eq!(app.workspace.layout.tiles, vec![new_id.clone()]);
        assert_eq!(app.workspace.layout.focus.as_deref(), Some(new_id.as_str()));
        // New id is marked dirty for saving.
        assert!(app.dirty.contains(&new_id));
    }
}
