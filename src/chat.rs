use crate::{
    engine::{
        self,
        attach::{self, Attachment, Kind},
        Capabilities, DetectedAgent, Event, Mode, Out, Turn, TurnOptions,
    },
    store::{self, Role, Session, Settings},
    thumbs::{self, Thumb},
};
use gpui::{
    div, img, ClipboardEntry, ExternalPaths, ImageFormat, ObjectFit, PathPromptOptions, StyledImage, prelude::*, px, rgb, ClickEvent, Context, Div, ElementId, FocusHandle, Focusable,
    FontWeight, KeyDownEvent, ScrollHandle, SharedString, Stateful, Window,
};
use crate::terminal::{tmux_kill, tmux_session_name, tmux_sessions};
use std::{cmp::Reverse, collections::{HashMap, HashSet}, path::PathBuf};

mod preview_panel;
use preview_panel::Preview;
mod git_panel;
mod settings_panel;
use settings_panel::AgentSetting;
mod terminal_panel;
use terminal_panel::Terms;
mod editor_panel;
use editor_panel::Editors;

/// Commands ArchonUI handles itself; everything else is sent to the agent.
const LOCAL_COMMANDS: &[(&str, &str)] = &[
    ("new", "Start a new chat with this agent"),
    ("stop", "Stop the running reply"),
    ("model", "Change the model this chat uses"),
    ("effort", "Change the effort (reasoning) level"),
    ("mode", "Change what the agent may do: Auto, Plan or Ask"),
    ("preview", "Open a web address or HTML file in the preview"),
    ("terminal", "Open a terminal in this chat's folder"),
    ("edit", "Open a file in the built-in editor"),
    ("rescan", "Rescan for agents, their commands and models"),
    ("settings", "Open the settings page"),
];

#[derive(Clone)]
enum Action {
    /// Run the slash command `name`.
    Command,
    /// Switch this chat to a model; `None` is the agent's default.
    Model(Option<String>),
    /// Switch this chat to an effort level; `None` is the agent's default.
    Effort(Option<String>),
    /// Switch this chat to a mode.
    Mode(Mode),
}

#[derive(Clone)]
struct PaletteEntry {
    name: String,
    hint: String,
    description: String,
    tags: Vec<&'static str>,
    action: Action,
}

/// Runtime state of a session; not persisted.
#[derive(Default)]
struct Live {
    turn: Option<Turn>,
    status: Option<String>,
    cancelled: bool,
    /// Files waiting to go with the next message.
    pending: Vec<Attachment>,
    /// What the agent did during the latest turn, oldest first.
    activity: Vec<String>,
    started: Option<std::time::Instant>,
}

impl Live {
    fn log(&mut self, entry: impl Into<String>) {
        let entry = entry.into();
        if self.activity.last() != Some(&entry) {
            self.activity.push(entry);
        }
        // Keep the panel bounded on long, tool-heavy turns.
        if self.activity.len() > 200 {
            self.activity.drain(..self.activity.len() - 200);
        }
    }
}

pub struct ChatView {
    agents: Vec<DetectedAgent>,
    scanning: bool,
    sessions: Vec<Session>,
    live: HashMap<String, Live>,
    current: Option<String>,
    /// Commands and models per agent; `None` while discovery is running.
    caps: HashMap<&'static str, Option<Capabilities>>,
    palette_index: usize,
    palette_scroll: ScrollHandle,
    /// One-off message under the palette, e.g. why a pin was refused.
    palette_note: Option<String>,
    /// Why the last files could not be attached.
    attach_note: Option<String>,
    /// The browser preview in the right-hand panel.
    preview: Preview,
    /// The shells in the right-hand panel's Terminal tab.
    terms: Terms,
    /// Thumbnails of attached images, keyed by the image's path.
    thumbs: HashMap<PathBuf, Thumb>,
    /// Files from the command line, attached to a new chat once agents are known.
    startup_files: Vec<PathBuf>,
    settings: Settings,
    /// The settings page takes the chat column while this is set.
    show_settings: bool,
    /// The model or effort list open on the settings page, for one agent.
    settings_expanded: Option<(&'static str, AgentSetting)>,
    input: String,
    default_cwd: PathBuf,
    editors: Editors,
    pub focus: FocusHandle,
    scroll: ScrollHandle,
}

impl ChatView {
    pub fn new(default_cwd: PathBuf, startup_files: Vec<PathBuf>, cx: &mut Context<Self>) -> Self {
        let sessions = store::load_all();
        let settings = store::load_settings();
        crate::preview::set_search_engine(settings.search_engine.url_prefix());
        // Shells whose chats didn't load would otherwise run on, unreachable, in tmux.
        let kept: HashSet<String> = sessions.iter().map(|s| tmux_session_name(&s.id)).collect();
        cx.background_executor()
            .spawn(async move {
                for name in tmux_sessions().iter().filter(|name| !kept.contains(*name)) {
                    tmux_kill(name);
                }
            })
            .detach();
        let mut this = Self {
            current: sessions.first().map(|s| s.id.clone()),
            sessions,
            agents: Vec::new(),
            scanning: false,
            live: HashMap::new(),
            caps: HashMap::new(),
            palette_index: 0,
            palette_scroll: ScrollHandle::new(),
            palette_note: None,
            attach_note: None,
            startup_files,
            thumbs: HashMap::new(),
            preview: Preview::default(),
            terms: Terms::default(),
            settings,
            show_settings: false,
            settings_expanded: None,
            input: String::new(),
            default_cwd,
            editors: Editors::default(),
            focus: cx.focus_handle(),
            scroll: ScrollHandle::new(),
        };
        // Open the restored chat at its latest message.
        this.scroll.scroll_to_bottom();
        this.rescan(cx);
        this
    }

    fn rescan(&mut self, cx: &mut Context<Self>) {
        if self.scanning {
            return;
        }
        self.scanning = true;
        cx.notify();
        let settings = self.settings.clone();
        cx.spawn(async move |this, cx| {
            let agents = cx.background_executor().spawn(async move { engine::detect(&settings) }).await;
            this.update(cx, |this, cx| {
                this.scanning = false;
                this.agents = agents;
                this.discover(cx);
                if this.current.is_none() {
                    if let Some(id) = this.preferred_agent() {
                        let (model, effort) = this.agent_defaults(id);
                        this.new_session(id, model, effort, cx);
                    }
                }
                let files = std::mem::take(&mut this.startup_files);
                if !files.is_empty() {
                    this.new_chat_same_agent(cx);
                    this.add_files(files, cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn discover(&mut self, cx: &mut Context<Self>) {
        self.caps.clear();
        for agent in self.agents.clone() {
            let id = agent.harness.id();
            let cwd = self.default_cwd.clone();
            self.caps.insert(id, None);
            cx.spawn(async move |this, cx| {
                let caps = cx
                    .background_executor()
                    .spawn(async move { engine::discover(&agent, &cwd) })
                    .await;
                this.update(cx, |this, cx| {
                    this.caps.insert(id, Some(caps));
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
    }

    fn session_caps(&self) -> Option<&Capabilities> {
        self.caps.get(self.session()?.agent.as_str())?.as_ref()
    }

    /// Entries while the input is a bare `/word` (commands), `/model …` or
    /// `/effort …`, prefix matches first.
    fn palette(&self) -> Option<Vec<PaletteEntry>> {
        let rest = self.input.strip_prefix('/')?;
        if let Some(query) = rest.strip_prefix("model ") {
            return Some(self.model_entries(query.trim()));
        }
        if let Some(query) = rest.strip_prefix("effort ") {
            return Some(self.effort_entries(query.trim()));
        }
        if let Some(query) = rest.strip_prefix("mode ") {
            return Some(self.mode_entries(query.trim()));
        }
        if rest.contains(char::is_whitespace) {
            return None;
        }
        let local = LOCAL_COMMANDS.iter().map(|(name, description)| PaletteEntry {
            name: name.to_string(),
            hint: String::new(),
            description: description.to_string(),
            tags: vec!["ArchonUI"],
            action: Action::Command,
        });
        let commands = self.session_caps().map(|c| &c.commands[..]).unwrap_or_default();
        let agent = commands.iter().map(|c| PaletteEntry {
            name: c.name.clone(),
            hint: c.hint.clone(),
            description: c.description.clone(),
            tags: Vec::new(),
            action: Action::Command,
        });
        Some(rank(local.chain(agent), rest))
    }

    /// Pinned models first, then the agent default, then everything else.
    fn model_entries(&self, query: &str) -> Vec<PaletteEntry> {
        let Some(session) = self.session() else { return Vec::new() };
        let caps = self.session_caps();
        let current = session.model.as_deref();
        let pins = self.settings.pins(&session.agent);
        let all = caps.map(|c| &c.models[..]).unwrap_or_default();
        let entry = |m: &engine::ModelInfo| {
            let mut tags = Vec::new();
            if pins.contains(&m.id) {
                tags.push("pinned");
            }
            if current == Some(m.id.as_str()) {
                tags.push("current");
            }
            PaletteEntry {
                name: m.label.clone(),
                hint: if m.label == m.id { String::new() } else { m.id.clone() },
                description: m.description.clone(),
                tags,
                action: Action::Model(Some(m.id.clone())),
            }
        };
        // Pins keep their pinned order; a pin whose model disappeared is skipped.
        let pinned = pins.iter().filter_map(|id| all.iter().find(|m| &m.id == id)).map(entry);
        let default = PaletteEntry {
            name: "Agent default".into(),
            hint: String::new(),
            description: caps.and_then(|c| c.default_model.clone()).unwrap_or_default(),
            tags: if current.is_none() { vec!["current"] } else { Vec::new() },
            action: Action::Model(None),
        };
        let rest = all.iter().filter(|m| !pins.contains(&m.id)).map(entry);
        rank(pinned.chain(std::iter::once(default)).chain(rest), query)
    }

    fn effort_entries(&self, query: &str) -> Vec<PaletteEntry> {
        let Some(session) = self.session() else { return Vec::new() };
        let caps = self.session_caps();
        let levels = caps.map(|c| c.efforts(session.model.as_deref())).unwrap_or_default();
        let current = session.effort.as_deref();
        let default_level = caps.and_then(|c| c.default_effort.as_deref());
        let default = PaletteEntry {
            name: "Agent default".into(),
            hint: String::new(),
            description: default_level.map(|l| format!("currently {l}")).unwrap_or_default(),
            tags: if current.is_none() { vec!["current"] } else { Vec::new() },
            action: Action::Effort(None),
        };
        let levels = levels.iter().map(|l| PaletteEntry {
            name: l.clone(),
            hint: String::new(),
            description: String::new(),
            tags: if current == Some(l.as_str()) { vec!["current"] } else { Vec::new() },
            action: Action::Effort(Some(l.clone())),
        });
        rank(std::iter::once(default).chain(levels), query)
    }

    /// Only the modes this chat's agent enforces.
    fn mode_entries(&self, query: &str) -> Vec<PaletteEntry> {
        let Some(session) = self.session() else { return Vec::new() };
        let entries = self.supported_modes().iter().map(|&m| PaletteEntry {
            name: m.name().into(),
            hint: String::new(),
            description: m.blurb().into(),
            tags: if session.mode == m { vec!["current"] } else { Vec::new() },
            action: Action::Mode(m),
        });
        rank(entries, query)
    }

    fn supported_modes(&self) -> &'static [Mode] {
        self.session().and_then(|s| self.installed(&s.agent)).map_or(&Mode::ALL, |a| a.harness.modes())
    }

    fn run_entry(&mut self, entry: &PaletteEntry, cx: &mut Context<Self>) {
        match &entry.action {
            Action::Command => {
                self.input = format!("/{}", entry.name);
                self.send(cx);
            }
            Action::Model(model) => self.set_model(model.clone(), cx),
            Action::Effort(level) => self.set_effort(level.clone(), cx),
            Action::Mode(mode) => self.set_mode(*mode, cx),
        }
    }

    fn current_mut(&mut self) -> Option<&mut Session> {
        let id = self.current.clone()?;
        self.sessions.iter_mut().find(|s| s.id == id)
    }

    /// Takes effect on the next message; the agent session carries on. An
    /// effort level the new model doesn't support falls back to the default.
    fn set_model(&mut self, model: Option<String>, cx: &mut Context<Self>) {
        self.input.clear();
        let levels = self.session_caps().map(|c| c.efforts(model.as_deref()).to_vec());
        if let Some(s) = self.current_mut() {
            s.model = model;
            if let (Some(levels), Some(effort)) = (levels, &s.effort) {
                if !levels.contains(effort) {
                    s.effort = None;
                }
            }
            if !s.messages.is_empty() {
                save(s);
            }
        }
        cx.notify();
    }

    fn set_effort(&mut self, level: Option<String>, cx: &mut Context<Self>) {
        self.input.clear();
        if let Some(s) = self.current_mut() {
            s.effort = level;
            if !s.messages.is_empty() {
                save(s);
            }
        }
        cx.notify();
    }

    /// Takes effect on the next message, like a model change.
    fn set_mode(&mut self, mode: Mode, cx: &mut Context<Self>) {
        self.input.clear();
        if let Some(s) = self.current_mut() {
            s.mode = mode;
            if !s.messages.is_empty() {
                save(s);
            }
        }
        cx.notify();
    }

    /// Pin or unpin a model for the current agent (at most `MAX_PINS`).
    fn toggle_pin(&mut self, model: &str, cx: &mut Context<Self>) {
        let Some(agent) = self.session().map(|s| s.agent.clone()) else { return };
        if self.settings.toggle_pin(&agent, model) {
            self.palette_note = None;
            if let Err(e) = store::save_settings(&self.settings) {
                eprintln!("could not save settings: {e}");
            }
        } else {
            self.palette_note =
                Some(format!("You can pin up to {} models. Unpin one first.", store::MAX_PINS));
        }
        cx.notify();
    }

    fn open_picker(&mut self, which: &str, cx: &mut Context<Self>) {
        self.input = format!("/{which} ");
        self.palette_index = 0;
        self.palette_note = None;
        self.palette_scroll.scroll_to_item(0);
        cx.notify();
    }

    /// Display name of this chat's model, e.g. "Opus" or "Default (gpt-5.6-luna)".
    fn model_label(&self) -> String {
        let caps = self.session_caps();
        match self.session().and_then(|s| s.model.as_deref()) {
            Some(id) => caps
                .and_then(|c| c.models.iter().find(|m| m.id == id))
                .map_or_else(|| id.to_string(), |m| m.label.clone()),
            None => match caps.and_then(|c| c.default_model.as_deref()) {
                Some(d) => format!("Default ({d})"),
                None => "Default model".into(),
            },
        }
    }

    /// Header label for the effort button, or `None` if the model has no
    /// effort levels (or they aren't known yet and none is set).
    fn effort_label(&self) -> Option<String> {
        let session = self.session()?;
        let caps = self.session_caps();
        let levels = caps.map(|c| c.efforts(session.model.as_deref())).unwrap_or_default();
        match session.effort.as_deref() {
            Some(level) => Some(format!("Effort: {level}")),
            None if levels.is_empty() => None,
            None => Some(match caps.and_then(|c| c.default_effort.as_deref()) {
                Some(d) => format!("Effort: default ({d})"),
                None => "Effort: default".into(),
            }),
        }
    }

    /// Run an ArchonUI command; returns false if `name` isn't one.
    fn run_local(&mut self, name: &str, arg: &str, cx: &mut Context<Self>) -> bool {
        match name {
            "new" => self.new_chat_same_agent(cx),
            "stop" => self.stop(cx),
            "rescan" => self.rescan(cx),
            "settings" => self.open_settings(cx),
            // `/model <id>` sets an id directly, including ones not listed.
            "model" if !arg.is_empty() => {
                self.set_model(Some(arg.to_string()), cx);
                return true;
            }
            "model" => {
                self.open_picker("model", cx);
                return true;
            }
            "effort" if !arg.is_empty() => {
                self.set_effort(Some(arg.to_string()), cx);
                return true;
            }
            // `/mode <name>` sets a mode directly; it must be one this agent enforces.
            "mode" if !arg.is_empty() => {
                match Mode::from_name(arg) {
                    Some(mode) if self.supported_modes().contains(&mode) => self.set_mode(mode, cx),
                    _ => {
                        self.palette_note = Some(format!("Unknown mode '{arg}'. Use Auto, Plan or Ask."));
                        cx.notify();
                    }
                }
                return true;
            }
            "mode" => {
                self.open_picker("mode", cx);
                return true;
            }
            "effort" => {
                self.open_picker("effort", cx);
                return true;
            }
            "preview" => {
                if arg.is_empty() {
                    self.show_preview(cx);
                } else {
                    let cwd = self.session().map_or_else(|| self.default_cwd.clone(), |s| s.cwd.clone());
                    self.open_preview(crate::preview::normalize_address(arg, &cwd), cx);
                }
            }
            "terminal" => self.show_terminal(cx),
            "edit" => {
                let cwd = self.session().map_or_else(|| self.default_cwd.clone(), |s| s.cwd.clone());
                self.show_editor(cx);
                if !arg.is_empty() {
                    self.open_in_editor(&cwd.join(arg), cx);
                }
            }
            _ => return false,
        }
        self.input.clear();
        cx.notify();
        true
    }

    fn new_chat_same_agent(&mut self, cx: &mut Context<Self>) {
        let agent = self
            .session()
            .and_then(|s| self.installed(&s.agent))
            .map(|a| a.harness.id())
            .or_else(|| self.preferred_agent());
        let Some(agent) = agent else { return };
        // Keep the model and effort when starting over with the same agent.
        let same = self.session().filter(|s| s.agent == agent).map(|s| (s.model.clone(), s.effort.clone()));
        let (model, effort) = same.unwrap_or_else(|| self.agent_defaults(agent));
        self.new_session(agent, model, effort, cx);
    }

    fn session(&self) -> Option<&Session> {
        let id = self.current.as_deref()?;
        self.sessions.iter().find(|s| s.id == id)
    }

    fn installed(&self, agent: &str) -> Option<&DetectedAgent> {
        self.agents.iter().find(|a| a.harness.id() == agent)
    }

    /// The folder new chats run in: the default folder if it still exists, else where ArchonUI started.
    fn new_chat_folder(&self) -> PathBuf {
        self.settings.default_folder.clone().filter(|p| p.is_dir()).unwrap_or_else(|| self.default_cwd.clone())
    }

    /// The agent new chats use when none is picked: the default agent if installed, else the first found.
    fn preferred_agent(&self) -> Option<&'static str> {
        self.settings
            .default_agent
            .as_deref()
            .and_then(|id| self.installed(id))
            .filter(|a| a.enabled)
            .or(self.agents.iter().find(|a| a.enabled))
            .map(|a| a.harness.id())
    }

    /// The model and effort a new chat with `agent` starts with.
    fn agent_defaults(&self, agent: &str) -> (Option<String>, Option<String>) {
        let (model, effort) =
            self.settings.agent_defaults.get(agent).map_or((None, None), |d| (d.model.clone(), d.effort.clone()));
        (model.or(self.settings.harness(agent).default_model), effort)
    }

    fn busy(&self, id: &str) -> bool {
        self.live.get(id).is_some_and(|l| l.turn.is_some())
    }

    fn new_session(&mut self, agent: &'static str, model: Option<String>, effort: Option<String>, cx: &mut Context<Self>) {
        let mut s = Session::new(agent, self.new_chat_folder());
        s.model = model;
        s.effort = effort;
        self.current = Some(s.id.clone());
        self.sessions.push(s);
        self.prune_empty();
        cx.notify();
    }

    fn select(&mut self, id: String, cx: &mut Context<Self>) {
        self.current = Some(id);
        self.prune_empty();
        self.scroll.scroll_to_bottom();
        self.git_changed(cx);
        cx.notify();
    }

    /// Unused "New chat" sessions vanish once you move away from them, unless they have a terminal.
    fn prune_empty(&mut self) {
        let gone = unused_sessions(&self.sessions, self.current.as_deref());
        self.sessions.retain(|s| !gone.contains(&s.id));
        // What a pruned chat left behind: its queued files (pasted images live in its attachments
        // folder), its folder watcher and its Git tab state. It never had messages, so nothing else is lost.
        for id in &gone {
            self.live.remove(id);
            self.forget_editor(id);
            self.forget_git(id);
            if let Err(e) = store::delete(id) {
                eprintln!("could not delete session {id}: {e}");
            }
        }
        let sessions = &self.sessions;
        self.terms.retain(|id| sessions.iter().any(|s| s.id == id));
    }

    fn delete(&mut self, id: &str, cx: &mut Context<Self>) {
        self.live.remove(id); // drops the Turn, which stops the agent
        self.forget_terminal(id, cx);
        self.forget_git(id);
        self.forget_editor(id);
        self.sessions.retain(|s| s.id != id);
        if let Err(e) = store::delete(id) {
            eprintln!("could not delete session {id}: {e}");
        }
        if self.current.as_deref() == Some(id) {
            self.current = self.sessions.iter().max_by_key(|s| s.updated).map(|s| s.id.clone());
        }
        cx.notify();
    }

    /// Start making thumbnails for visible images that don't have one yet.
    fn request_thumbs(&mut self, cx: &mut Context<Self>) {
        let session = self.session();
        let shown = session.into_iter().flat_map(|s| &s.messages).flat_map(|m| &m.attachments);
        let wanted: Vec<PathBuf> = shown
            .chain(self.pending())
            .filter(|a| a.kind == Kind::Image && !self.thumbs.contains_key(&a.path))
            .map(|a| a.path.clone())
            .collect();
        for path in wanted {
            self.thumbs.insert(path.clone(), Thumb::Loading);
            cx.spawn(async move |this, cx| {
                let src = path.clone();
                let result = cx.background_executor().spawn(async move { thumbs::thumbnail(&src) }).await;
                this.update(cx, |this, cx| {
                    let thumb = match result {
                        Ok(p) => Thumb::Ready(p),
                        Err(e) => {
                            eprintln!("thumbnail for {}: {e}", path.display());
                            Thumb::Failed
                        }
                    };
                    this.thumbs.insert(path, thumb);
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
    }

    fn pending(&self) -> &[Attachment] {
        self.current.as_ref().and_then(|id| self.live.get(id)).map_or(&[], |l| &l.pending)
    }

    /// Queue files for the next message, reporting any that can't be sent.
    fn add_files(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        let Some(id) = self.current.clone() else { return };
        let pending = &mut self.live.entry(id).or_default().pending;
        let mut errors = Vec::new();
        for path in paths {
            if pending.len() >= attach::MAX_FILES {
                errors.push(format!("At most {} files per message.", attach::MAX_FILES));
                break;
            }
            if pending.iter().any(|a| a.path == path) {
                continue;
            }
            match Attachment::inspect(&path) {
                Ok(a) => pending.push(a),
                Err(e) => errors.push(e),
            }
        }
        self.attach_note = (!errors.is_empty()).then(|| errors.join(" "));
        cx.notify();
    }

    fn pick_files(&mut self, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Attach".into()),
        });
        cx.spawn(async move |this, cx| {
            let result = picked.await;
            this.update(cx, |this, cx| match result {
                Ok(Ok(Some(paths))) => this.add_files(paths, cx),
                Ok(Ok(None)) => {} // cancelled
                Ok(Err(e)) => {
                    this.attach_note = Some(format!("Could not open the file picker: {e}"));
                    cx.notify();
                }
                Err(_) => {}
            })
            .ok();
        })
        .detach();
    }

    /// Save a pasted image into the session's attachments and queue it.
    fn paste_image(&mut self, image: &gpui::Image, cx: &mut Context<Self>) {
        let Some(id) = self.current.clone() else { return };
        let ext = match image.format {
            ImageFormat::Jpeg => "jpg",
            ImageFormat::Webp => "webp",
            ImageFormat::Gif => "gif",
            ImageFormat::Png => "png",
            _ => {
                self.attach_note = Some("That clipboard image format isn't supported.".into());
                return;
            }
        };
        let name = format!("pasted-{}.{ext}", store::now());
        match store::store_bytes(&id, &name, &image.bytes) {
            Ok(path) => self.add_files(vec![path], cx),
            Err(e) => self.attach_note = Some(format!("Could not save the pasted image: {e}")),
        }
    }

    fn remove_pending(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(id) = self.current.clone() else { return };
        if let Some(live) = self.live.get_mut(&id) {
            if index < live.pending.len() {
                let a = live.pending.remove(index);
                // Pasted images only exist in ArchonUI's copy, unless an earlier message already kept it.
                let kept = self.sessions.iter().any(|s| s.id == id && kept_by_message(s, &a.path));
                if a.path.starts_with(store::attachments_dir(&id)) && !kept {
                    let _ = std::fs::remove_file(&a.path);
                }
            }
        }
        cx.notify();
    }

    /// Keep each message's files with the session, so they survive the
    /// originals moving. Hard links where possible, else a copy.
    fn keep_files(session: &str, files: Vec<Attachment>) -> (Vec<Attachment>, Vec<String>) {
        let dir = store::attachments_dir(session);
        let mut kept = Vec::new();
        let mut errors = Vec::new();
        for mut a in files {
            if !a.path.starts_with(&dir) {
                let linked = std::fs::create_dir_all(&dir).and_then(|()| {
                    let dest = dir.join(&a.name);
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

    fn send(&mut self, cx: &mut Context<Self>) {
        let prompt = self.input.trim().to_string();
        if let Some(rest) = prompt.strip_prefix('/') {
            let (name, arg) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
            if self.run_local(name, arg.trim(), cx) {
                return;
            }
        }
        let Some(id) = self.current.clone() else { return };
        if (prompt.is_empty() && self.pending().is_empty()) || self.busy(&id) {
            return;
        }
        self.watch_chat(&id, cx);
        if !self.sessions.iter().any(|s| s.id == id && self.installed(&s.agent).is_some()) {
            return; // the input bar explains that the agent is missing
        }
        let pending = self.live.get_mut(&id).map(|l| std::mem::take(&mut l.pending)).unwrap_or_default();
        let (files, file_errors) = Self::keep_files(&id, pending);
        self.attach_note = None;
        let Some(session) = self.sessions.iter_mut().find(|s| s.id == id) else { return };
        let Some(agent) = self.agents.iter().find(|a| a.harness.id() == session.agent) else {
            return; // the input bar explains that the agent is missing
        };
        // Every queued file failed to save and there is no text: don't send an empty turn.
        if prompt.is_empty() && files.is_empty() {
            for e in file_errors {
                session.push(Role::Error, e);
            }
            save(session);
            cx.notify();
            return;
        }
        if session.agent_session.is_none() && agent.harness.preassign_session() {
            session.agent_session = Some(store::new_id());
        }
        self.input.clear();
        // The first message titles the chat, even one that had a terminal ("Terminal").
        if session.messages.is_empty() {
            session.title = match files.first() {
                Some(f) if prompt.is_empty() => store::title_from(&f.name),
                _ => store::title_from(&prompt),
            };
        }
        session.push_with(Role::User, prompt.as_str(), files.clone());
        for e in file_errors {
            session.push(Role::Error, e);
        }
        let turn = engine::start_turn(
            agent,
            &session.cwd,
            &prompt,
            session.agent_session.as_deref(),
            TurnOptions {
                model: session.model.as_deref(),
                effort: session.effort.as_deref(),
                mode: session.mode,
                files: &files,
                ..Default::default()
            },
        );
        match turn {
            Ok(turn) => {
                session.push(Role::Agent, "");
                let events = turn.events.clone();
                let live = self.live.entry(id.clone()).or_default();
                *live = Live {
                    turn: Some(turn),
                    pending: std::mem::take(&mut live.pending),
                    started: Some(std::time::Instant::now()),
                    ..Default::default()
                };
                let mut sent = format!("Sent to {}", agent.harness.name());
                if !files.is_empty() {
                    sent.push_str(&format!(" with {} file(s)", files.len()));
                }
                live.log(sent);
                let id = id.clone();
                cx.spawn(async move |this, cx| {
                    while let Ok(ev) = events.recv().await {
                        if this.update(cx, |this, cx| this.apply(&id, ev, cx)).is_err() {
                            break;
                        }
                    }
                })
                .detach();
            }
            Err(e) => session.push(Role::Error, format!("Could not start {}: {e}", agent.harness.name())),
        }
        save(session);
        self.scroll.scroll_to_bottom();
        cx.notify();
    }

    /// Whether the message list is scrolled to (near) its end, as of the last
    /// layout.
    fn at_bottom(&self) -> bool {
        let max = self.scroll.max_offset().height;
        -self.scroll.offset().y >= max - px(40.)
    }

    fn apply(&mut self, id: &str, ev: Event, cx: &mut Context<Self>) {
        // Keep following a reply only while the user hasn't scrolled up to read.
        let follow = self.current.as_deref() == Some(id) && self.at_bottom();
        let mut open_url = None;
        let auto_open = self.settings.auto_open_preview;
        let Some(s) = self.sessions.iter_mut().find(|s| s.id == id) else { return };
        let live = self.live.entry(id.to_string()).or_default();
        let finished = matches!(ev, Event::Finished(_));
        match ev {
            Event::Out(Out::Text(t)) => {
                live.status = None;
                if let Some(m) = s.reply_mut() {
                    m.text.push_str(&t);
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
                }
            }
            // This view shows no usage.
            Event::Out(Out::Usage(_) | Out::Todos(_) | Out::ContextUsed(_) | Out::ContextWindow(_)) => {}
            Event::Out(Out::Status(st)) => {
                live.log(st.clone());
                live.status = Some(st);
            }
            Event::Out(Out::Session(agent_session)) => {
                if s.agent_session.is_none() {
                    s.agent_session = Some(agent_session);
                    save(s);
                }
            }
            Event::Out(Out::Error(e)) => s.push(Role::Error, e),
            Event::Out(Out::ToolStart { .. } | Out::ToolEnd { .. } | Out::Thinking(_)) => {}
            Event::Finished(result) => {
                live.turn = None;
                live.status = None;
                let cancelled = std::mem::take(&mut live.cancelled);
                let ok = result.is_ok();
                let took = live.started.take().map_or(String::new(), |t| {
                    format!(" after {:.1}s", t.elapsed().as_secs_f32())
                });
                live.log(match (&result, cancelled) {
                    (_, true) => format!("Stopped{took}"),
                    (Err(_), _) => format!("Failed{took}"),
                    (Ok(()), _) => format!("Finished{took}"),
                });
                if let Some(m) = s.reply_mut() {
                    m.text = m.text.trim_end().to_string();
                }
                let had_reply = s.reply_mut().is_some_and(|m| !m.text.is_empty());
                s.messages.retain(|m| m.role != Role::Agent || !m.text.is_empty());
                if cancelled {
                    s.push(Role::Error, "Stopped.");
                } else if let Err(e) = result {
                    s.push(Role::Error, format!("Agent exited with an error:\n{e}"));
                } else if !had_reply {
                    s.push(Role::Error, "The agent finished without replying.");
                }
                s.updated = store::now();
                save(s);
                // A local site or HTML file in the reply opens in the preview.
                let reply = s
                    .messages
                    .iter()
                    .rev()
                    .find(|m| m.role == Role::Agent)
                    .filter(|_| ok && had_reply && !cancelled && auto_open);
                if let Some(url) = reply.and_then(|m| crate::preview::find_preview_target(&m.text, &s.cwd)) {
                    open_url = Some(url);
                }
            }
        }
        if let Some(url) = open_url {
            self.open_preview(url, cx);
        }
        if finished {
            // The agent may have changed files.
            self.git_changed(cx);
        }
        if follow {
            self.scroll.scroll_to_bottom();
        }
        cx.notify();
    }

    fn stop(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.current.as_deref() else { return };
        if let Some(live) = self.live.get_mut(id) {
            if let Some(turn) = &live.turn {
                live.cancelled = true;
                turn.cancel();
                cx.notify();
            }
        }
    }

    fn on_key(&mut self, ev: &KeyDownEvent, cx: &mut Context<Self>) {
        let before = self.input.clone();
        self.handle_key(ev, cx);
        if self.input != before {
            self.palette_index = 0;
            self.palette_note = None;
            self.palette_scroll.scroll_to_item(0);
        }
        cx.notify();
    }

    /// Keys the command palette consumes while it is open.
    fn palette_key(&mut self, key: &str, cx: &mut Context<Self>) -> bool {
        let Some(entries) = self.palette() else { return false };
        if key == "escape" {
            self.input.clear();
            return true;
        }
        if entries.is_empty() {
            return false;
        }
        let len = entries.len();
        let sel = self.palette_index.min(len - 1);
        match key {
            "up" | "down" => {
                self.palette_index = if key == "up" { (sel + len - 1) % len } else { (sel + 1) % len };
                self.palette_scroll.scroll_to_item(self.palette_index);
            }
            "tab" => match entries[sel].action {
                Action::Command => self.input = format!("/{} ", entries[sel].name),
                _ => self.run_entry(&entries[sel], cx),
            },
            "enter" => self.run_entry(&entries[sel], cx),
            _ => return false,
        }
        true
    }

    fn handle_key(&mut self, ev: &KeyDownEvent, cx: &mut Context<Self>) {
        let ks = &ev.keystroke;
        let m = ks.modifiers;
        // Escape or Ctrl+, closes the settings page, whichever part has the keyboard.
        if self.show_settings && (ks.key == "escape" || (m.control && ks.key == ",")) {
            self.close_settings(cx);
            return;
        }
        // The editor, the terminal, the address bar or the page itself may have the keyboard.
        if self.editor_key(ev, cx) || self.git_key(ev, cx) || self.terminal_key(ev, cx) || self.preview_key(ev, cx) {
            return;
        }
        // The chat's input is out of sight while the settings page shows.
        if self.show_settings {
            return;
        }
        if !(m.control || m.platform || m.alt) && self.palette_key(&ks.key, cx) {
            return;
        }
        // Ctrl+P pins the highlighted model in the model picker.
        if m.control && ks.key == "p" {
            if let Some(entries) = self.palette() {
                let sel = self.palette_index.min(entries.len().saturating_sub(1));
                if let Some(Action::Model(Some(id))) = entries.get(sel).map(|e| &e.action) {
                    self.toggle_pin(&id.clone(), cx);
                }
            }
            return;
        }
        if m.control && ks.key == "v" {
            let Some(item) = cx.read_from_clipboard() else { return };
            if let Some(ClipboardEntry::Image(image)) =
                item.entries().iter().find(|e| matches!(e, ClipboardEntry::Image(_)))
            {
                self.paste_image(image, cx);
            } else if let Some(text) = item.text() {
                self.input.push_str(&text.replace('\n', " "));
            }
        } else if m.control && ks.key == "n" {
            self.new_chat_same_agent(cx);
        } else if m.control && ks.key == "i" {
            self.toggle_panel(cx);
        } else if m.control && ks.key == "," {
            self.open_settings(cx);
        } else if m.control || m.platform || m.alt {
            return;
        } else {
            match ks.key.as_str() {
                "enter" => self.send(cx),
                "escape" => self.stop(cx),
                "backspace" => {
                    self.input.pop();
                }
                "space" => self.input.push(' '),
                _ => {
                    if let Some(ch) = &ks.key_char {
                        self.input.push_str(ch);
                    }
                }
            }
        }
    }

    fn render_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let new_buttons: Vec<_> = self
            .agents
            .iter()
            .filter(|a| a.enabled)
            .enumerate()
            .map(|(i, a)| {
                let id = a.harness.id();
                button(("new-with", i), a.harness.name()).on_click(cx.listener(
                    move |this, _: &ClickEvent, window, cx| {
                        let (model, effort) = this.agent_defaults(id);
                        this.new_session(id, model, effort, cx);
                        window.focus(&this.focus);
                    },
                ))
            })
            .collect();

        let mut ordered: Vec<&Session> = self.sessions.iter().collect();
        ordered.sort_by_key(|s| Reverse(s.updated));
        let items: Vec<_> = ordered
            .into_iter()
            .enumerate()
            .map(|(i, s)| {
                let selected = self.current.as_deref() == Some(s.id.as_str());
                let title: SharedString =
                    if s.title.is_empty() { "New chat".into() } else { s.title.clone().into() };
                let meta = format!("{} · {}", engine::agent_name(&s.agent), store::ago(s.updated));
                let (select_id, delete_id) = (s.id.clone(), s.id.clone());
                div()
                    .id(ElementId::Name(s.id.clone().into()))
                    .mx_2()
                    .px_3()
                    .py_2()
                    .rounded_md()
                    .cursor_pointer()
                    .when(selected, |d| d.bg(rgb(0x313244)))
                    .hover(|d| d.bg(rgb(0x2a2b3c)))
                    .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                        this.select(select_id.clone(), cx);
                        window.focus(&this.focus);
                    }))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(div().flex_1().min_w_0().truncate().child(title))
                            .when(self.busy(&s.id), |d| {
                                d.child(div().text_color(rgb(0xa6e3a1)).child("●"))
                            })
                            .child(
                                div()
                                    .id(("delete", i))
                                    .px_1()
                                    .rounded_sm()
                                    .text_color(rgb(0x585b70))
                                    .hover(|d| d.text_color(rgb(0xf38ba8)))
                                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                        cx.stop_propagation();
                                        this.delete(&delete_id, cx);
                                    }))
                                    .child("×"),
                            ),
                    )
                    .child(div().text_xs().text_color(rgb(0x7f849c)).child(meta))
            })
            .collect();

        let footer = if self.scanning {
            "Scanning…".to_string()
        } else {
            match self.agents.len() {
                1 => "1 agent".to_string(),
                n => format!("{n} agents"),
            }
        };

        div()
            .w(px(230.))
            .flex_none()
            .h_full()
            .flex()
            .flex_col()
            .bg(rgb(0x181825))
            .border_r_1()
            .border_color(rgb(0x313244))
            .child(section_label("New chat with"))
            .child(div().px_3().pb_3().flex().flex_wrap().gap_1().children(new_buttons).when(
                !self.scanning && self.agents.is_empty(),
                |d| d.child(div().text_xs().text_color(rgb(0x9399b2)).child("No agent CLIs found.")),
            ))
            .child(div().border_t_1().border_color(rgb(0x313244)))
            .child(section_label("Sessions"))
            .child(
                div()
                    .id("sessions")
                    .flex_1()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .children(items),
            )
            .child(
                div()
                    .p_3()
                    .flex()
                    .items_center()
                    .justify_between()
                    .border_t_1()
                    .border_color(rgb(0x313244))
                    .child(div().text_xs().text_color(rgb(0x7f849c)).child(footer))
                    .child(
                        div()
                            .flex()
                            .gap_1()
                            .child(button("settings", "⚙ Settings").on_click(cx.listener(
                                |this, _: &ClickEvent, window, cx| {
                                    this.open_settings(cx);
                                    window.focus(&this.focus);
                                },
                            )))
                            .child(
                                button("rescan", "Rescan")
                                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.rescan(cx))),
                            ),
                    ),
            )
    }
}

impl ChatView {
    fn render_palette(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let entries = self.palette()?;
        let sel = self.palette_index.min(entries.len().saturating_sub(1));
        let pins = self.session().map(|s| self.settings.pins(&s.agent)).unwrap_or_default();
        // Every entry is a direct child of the scroll container so that
        // `scroll_to_item(index)` lines up with the selection.
        let rows: Vec<_> = entries
            .iter()
            .enumerate()
            .map(|(i, e)| {
                let entry = e.clone();
                let hint: String = e.hint.chars().take(40).collect();
                let pin_target = match &e.action {
                    Action::Model(Some(id)) => Some((id.clone(), pins.contains(id))),
                    _ => None,
                };
                div()
                    .id(("command", i))
                    .px_3()
                    .py_1()
                    .rounded_md()
                    .flex()
                    .items_center()
                    .gap_2()
                    .cursor_pointer()
                    .when(i == sel, |d| d.bg(rgb(0x313244)))
                    .hover(|d| d.bg(rgb(0x2a2b3c)))
                    .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                        this.run_entry(&entry, cx);
                        window.focus(&this.focus);
                    }))
                    .child(div().flex_none().child(match e.action {
                        Action::Command => format!("/{}", e.name),
                        _ => e.name.clone(),
                    }))
                    .when(!hint.is_empty(), |d| {
                        d.child(div().flex_none().text_color(rgb(0x6c7086)).child(hint))
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_color(rgb(0x9399b2))
                            .child(e.description.clone()),
                    )
                    .when(!e.tags.is_empty(), |d| {
                        d.child(
                            div()
                                .flex_none()
                                .text_xs()
                                .text_color(rgb(0x89b4fa))
                                .child(e.tags.join(" · ")),
                        )
                    })
                    .children(pin_target.map(|(id, pinned)| {
                        div()
                            .id(("pin", i))
                            .flex_none()
                            .px_1()
                            .rounded_sm()
                            .text_xs()
                            .text_color(if pinned { rgb(0xf9e2af) } else { rgb(0x585b70) })
                            .hover(|d| d.text_color(rgb(0xf9e2af)).bg(rgb(0x45475a)))
                            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                                cx.stop_propagation();
                                this.toggle_pin(&id, cx);
                                window.focus(&this.focus);
                            }))
                            .child(if pinned { "Unpin" } else { "Pin" })
                    }))
            })
            .collect();

        let agent = self.session().map(|s| s.agent.as_str());
        let loading = agent.is_some_and(|a| matches!(self.caps.get(a), Some(None)));
        let picking = if self.input.starts_with("/model ") {
            Some("models")
        } else if self.input.starts_with("/effort ") {
            Some("effort levels")
        } else {
            None
        };
        let mut footer = match (entries.is_empty(), picking) {
            (true, Some(what)) => format!("No matching {what} · Enter uses the typed value as-is"),
            (true, None) => "No matching commands".to_string(),
            (false, Some("models")) => format!(
                "{} of {} · ↑↓ select · Enter choose · Ctrl+P pin (max {}) · Esc close",
                sel + 1,
                entries.len(),
                store::MAX_PINS
            ),
            (false, Some(_)) => format!("{} of {} · ↑↓ select · Enter choose · Esc close", sel + 1, entries.len()),
            (false, None) => {
                format!("{} of {} · ↑↓ select · Tab complete · Enter run · Esc close", sel + 1, entries.len())
            }
        };
        if loading {
            footer.push_str(if picking.is_some() { " · loading models…" } else { " · loading agent commands…" });
        }

        Some(
            div()
                .mx_3()
                .mt_2()
                .p_1()
                .rounded_lg()
                .bg(rgb(0x181825))
                .border_1()
                .border_color(rgb(0x313244))
                .child(
                    div()
                        .id("palette-list")
                        .max_h(px(300.))
                        .overflow_y_scroll()
                        .track_scroll(&self.palette_scroll)
                        .children(rows),
                )
                .children(self.palette_note.clone().map(|note| {
                    div().px_3().pt_1().text_xs().text_color(rgb(0xf38ba8)).child(note)
                }))
                .child(div().px_3().pt_1().text_xs().text_color(rgb(0x6c7086)).child(footer)),
        )
    }
}

impl ChatView {
    fn toggle_panel(&mut self, cx: &mut Context<Self>) {
        self.settings.hide_panel = !self.settings.hide_panel;
        self.save_settings();
        cx.notify();
    }

    fn save_settings(&self) {
        if let Err(e) = store::save_settings(&self.settings) {
            eprintln!("could not save settings: {e}");
        }
    }

    /// The right-hand panel: this chat's settings, its files, and what the
    /// agent is doing.
    fn render_details(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let panel = div().id("details").flex_1().min_h_0().overflow_y_scroll().flex().flex_col();
        let Some(s) = self.session() else {
            return panel.child(section_label("Details")).child(
                div().px_4().text_xs().text_color(rgb(0x7f849c)).child("No chat selected."),
            );
        };
        let agent = self.installed(&s.agent);
        let live = self.live.get(&s.id);

        let mut rows = vec![
            ("Agent", match agent {
                Some(a) => format!("{} {}", a.harness.name(), a.version),
                None => format!("{} (not installed)", engine::agent_name(&s.agent)),
            }),
            ("Model", self.model_label()),
            ("Mode", s.mode.name().to_string()),
        ];
        if let Some(a) = agent {
            rows.push(("Sign-in", a.auth.label().to_string()));
        }
        if let Some(effort) = self.effort_label() {
            rows.push(("Effort", effort.trim_start_matches("Effort: ").to_string()));
        }
        rows.push(("Folder", s.cwd.display().to_string()));
        rows.push(("Messages", s.messages.iter().filter(|m| m.role != Role::Error).count().to_string()));
        rows.push(("Started", match store::ago(s.created).as_str() {
            "now" => "just now".to_string(),
            ago => format!("{ago} ago"),
        }));
        let info = rows.into_iter().map(|(k, v)| {
            div()
                .px_4()
                .py_1()
                .child(div().text_xs().text_color(rgb(0x7f849c)).child(k))
                .child(div().text_xs().child(v))
        });
        // Session ids are long; click to copy rather than show in full.
        let ids = [("ArchonUI session", Some(s.id.clone())), ("Agent session", s.agent_session.clone())];
        let id_rows = ids.into_iter().enumerate().map(|(i, (k, v))| {
            let value = v.clone();
            div()
                .id(("copy-id", i))
                .px_4()
                .py_1()
                .when(value.is_some(), |d| {
                    d.cursor_pointer().hover(|d| d.bg(rgb(0x232436))).on_click(cx.listener(
                        move |_, _: &ClickEvent, _, cx| {
                            if let Some(v) = &value {
                                cx.write_to_clipboard(gpui::ClipboardItem::new_string(v.clone()));
                            }
                        },
                    ))
                })
                .child(div().text_xs().text_color(rgb(0x7f849c)).child(match v {
                    Some(_) => format!("{k} · click to copy"),
                    None => k.to_string(),
                }))
                .child(div().text_xs().truncate().child(v.unwrap_or_else(|| "not started yet".into())))
        });

        let files: Vec<&Attachment> = s.messages.iter().flat_map(|m| &m.attachments).collect();
        let total: u64 = files.iter().map(|a| a.size).sum();
        let file_rows = files.iter().enumerate().map(|(i, a)| {
            let path = a.path.clone();
            div()
                .id(("details-file", i))
                .mx_2()
                .px_2()
                .py_1()
                .rounded_md()
                .flex()
                .items_center()
                .gap_2()
                .cursor_pointer()
                .hover(|d| d.bg(rgb(0x2a2b3c)))
                .on_click(cx.listener(move |_, _: &ClickEvent, _, _| open_file(&path)))
                .child(attachment_view(a, self.thumbs.get(&a.path), px(28.)))
                .child(div().flex_1().min_w_0().text_xs().truncate().child(a.name.clone()))
                .child(div().flex_none().text_xs().text_color(rgb(0x7f849c)).child(attach::human_size(a.size)))
        });

        let activity: Vec<String> = live.map(|l| l.activity.clone()).unwrap_or_default();
        let busy = live.is_some_and(|l| l.turn.is_some());

        panel
            .child(section_label("Chat"))
            .children(info)
            .children(id_rows)
            .child(div().mt_2().border_t_1().border_color(rgb(0x313244)))
            .child(section_label(if files.is_empty() {
                "Files".to_string()
            } else {
                format!("Files · {} · {}", files.len(), attach::human_size(total))
            }))
            .when(files.is_empty(), |d| {
                d.child(div().px_4().text_xs().text_color(rgb(0x7f849c)).child("Nothing attached yet."))
            })
            .children(file_rows)
            .child(div().mt_2().border_t_1().border_color(rgb(0x313244)))
            .child(section_label(if busy { "Activity · working" } else { "Activity" }))
            .when(activity.is_empty(), |d| {
                d.child(div().px_4().text_xs().text_color(rgb(0x7f849c)).child("No reply yet in this run of ArchonUI."))
            })
            .children(activity.into_iter().map(|a| {
                div().px_4().py_0p5().text_xs().text_color(rgb(0xbac2de)).child(format!("• {a}"))
            }))
            .child(div().h_4())
    }

    /// A sent message's files: images two per row, then one chip per file.
    /// Rows are built by hand because a wrapping flex row inside a
    /// shrink-to-fit bubble doesn't grow the bubble's height.
    fn message_attachments(&self, files: &[Attachment]) -> impl IntoElement {
        let (images, others): (Vec<&Attachment>, Vec<&Attachment>) =
            files.iter().partition(|a| a.kind == Kind::Image && !matches!(self.thumbs.get(&a.path), Some(Thumb::Failed)));
        let image_rows = images.chunks(2).map(|row| {
            div()
                .flex()
                .gap_2()
                .children(row.iter().map(|a| attachment_view(a, self.thumbs.get(&a.path), px(220.))))
        });
        div()
            .flex()
            .flex_col()
            .gap_2()
            .children(image_rows)
            .children(others.into_iter().map(|a| attachment_view(a, None, px(220.))))
    }

    /// Chips for the files queued for the next message.
    fn render_pending(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let pending = self.pending();
        if pending.is_empty() {
            return None;
        }
        let total: u64 = pending.iter().map(|a| a.size).sum();
        let chips = pending.iter().enumerate().map(|(i, a)| {
            div()
                .flex()
                .items_center()
                .gap_1()
                .pr_1()
                .rounded_md()
                .bg(rgb(0x313244))
                .child(attachment_view(a, self.thumbs.get(&a.path), px(48.)))
                .child(
                    div()
                        .id(("unattach", i))
                        .px_1()
                        .rounded_sm()
                        .cursor_pointer()
                        .text_color(rgb(0x7f849c))
                        .hover(|d| d.text_color(rgb(0xf38ba8)))
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.remove_pending(i, cx)))
                        .child("×"),
                )
        });
        Some(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(div().flex().flex_wrap().gap_2().children(chips))
                .child(div().text_xs().text_color(rgb(0x6c7086)).child(format!(
                    "{} file(s), {} · up to {} files, {} each",
                    pending.len(),
                    attach::human_size(total),
                    attach::MAX_FILES,
                    attach::human_size(attach::MAX_FILE_BYTES),
                ))),
        )
    }
}

/// An image thumbnail (at most `size` wide) or a labelled file chip.
fn attachment_view(a: &Attachment, thumb: Option<&Thumb>, size: gpui::Pixels) -> gpui::AnyElement {
    let thumb = match thumb {
        Some(Thumb::Ready(p)) => Some(p.clone()),
        Some(Thumb::Failed) => None,
        Some(Thumb::Loading) | None if a.kind == Kind::Image => {
            return div().flex_none().size(size).rounded_md().bg(rgb(0x313244)).into_any_element();
        }
        _ => None,
    };
    if let Some(thumb) = thumb {
        let large = size > px(100.);
        // `ObjectFit::Cover` paints past the element's bounds, so a clipping
        // box keeps small square thumbnails square.
        return div()
            .flex_none()
            .w(size)
            .h(if large { px(160.) } else { size })
            .overflow_hidden()
            .rounded_md()
            .child(
                img(thumb)
                    .size_full()
                    .object_fit(if large { ObjectFit::Contain } else { ObjectFit::Cover }),
            )
            .into_any_element();
    }
    let label = match a.kind {
        Kind::Image => "IMAGE",
        Kind::Pdf => "PDF",
        Kind::Text => "TEXT",
        _ => "FILE",
    };
    div()
        .flex()
        .items_center()
        .gap_2()
        .px_2()
        .py_1()
        .rounded_md()
        .bg(rgb(0x1e1e2e))
        .max_w(px(260.))
        .child(div().flex_none().text_xs().text_color(rgb(0x89b4fa)).child(label))
        .child(div().min_w_0().truncate().child(a.name.clone()))
        .child(div().flex_none().text_xs().text_color(rgb(0x7f849c)).child(attach::human_size(a.size)))
        .into_any_element()
}

/// Whether a message in `session` already keeps this file, so removing it must not delete the copy.
fn kept_by_message(session: &Session, path: &std::path::Path) -> bool {
    session.messages.iter().any(|m| m.attachments.iter().any(|f| f.path == path))
}

/// The chats that vanish when moving away: no messages, no terminal, and not `current`.
fn unused_sessions(sessions: &[Session], current: Option<&str>) -> Vec<String> {
    sessions
        .iter()
        .filter(|s| s.messages.is_empty() && !s.terminal && Some(s.id.as_str()) != current)
        .map(|s| s.id.clone())
        .collect()
}

/// Entries whose name (or hint, e.g. a model id) contains `query`, with
/// name-prefix matches first.
fn rank(entries: impl Iterator<Item = PaletteEntry>, query: &str) -> Vec<PaletteEntry> {
    let query = query.to_lowercase();
    let (mut prefix, mut contains) = (Vec::new(), Vec::new());
    for e in entries {
        let name = e.name.to_lowercase();
        if name.starts_with(&query) {
            prefix.push(e);
        } else if name.contains(&query) || e.hint.to_lowercase().contains(&query) {
            contains.push(e);
        }
    }
    prefix.append(&mut contains);
    prefix
}

/// Open a file in the desktop's default app.
fn open_file(path: &std::path::Path) {
    match std::process::Command::new("xdg-open").arg(path).spawn() {
        // Reap it so it doesn't linger as a zombie.
        Ok(mut child) => drop(std::thread::spawn(move || child.wait())),
        Err(e) => eprintln!("could not open {}: {e}", path.display()),
    }
}

fn save(s: &Session) {
    if let Err(e) = store::save(s) {
        eprintln!("could not save session {}: {e}", s.id);
    }
}

fn section_label(text: impl Into<SharedString>) -> Div {
    div()
        .px_4()
        .pt_3()
        .pb_2()
        .text_xs()
        .font_weight(FontWeight::BOLD)
        .text_color(rgb(0x9399b2))
        .child(text.into())
}

fn button(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Stateful<Div> {
    div()
        .id(id)
        .px_2()
        .py_1()
        .rounded_md()
        .text_xs()
        .cursor_pointer()
        .bg(rgb(0x313244))
        .hover(|d| d.bg(rgb(0x45475a)))
        .child(label.into())
}

impl Focusable for ChatView {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for ChatView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.request_thumbs(cx);
        self.preview_before_render(window, cx);
        self.terminal_before_render(window, cx);
        self.editor_before_render(window, cx);
        let session = self.session();
        let agent = session.and_then(|s| self.installed(&s.agent));
        let busy = session.is_some_and(|s| self.busy(&s.id));

        let title: SharedString = match session {
            Some(s) if !s.title.is_empty() => s.title.clone().into(),
            Some(_) => "New chat".into(),
            None => "ArchonUI".into(),
        };
        let subtitle = match session {
            Some(s) => {
                let status = if busy {
                    self.live[&s.id].status.clone().unwrap_or_else(|| "thinking…".into())
                } else if agent.is_some() {
                    "ready".into()
                } else {
                    "not installed".into()
                };
                format!("{} · {status} · {}", engine::agent_name(&s.agent), s.cwd.display())
            }
            None if self.scanning => "scanning for agents…".into(),
            None => "no session".into(),
        };

        let bubbles: Vec<_> = session
            .map(|s| &s.messages[..])
            .unwrap_or_default()
            .iter()
            .enumerate()
            .map(|(i, m)| {
                let user = m.role == Role::User;
                let text: SharedString =
                    if m.text.is_empty() { "…".into() } else { m.text.clone().into() };
                let bg = match m.role {
                    Role::User => rgb(0x3b5bdb),
                    Role::Agent => rgb(0x2b2d3a),
                    Role::Error => rgb(0x4a2530),
                };
                let has_text = !m.text.is_empty() || m.attachments.is_empty();
                div()
                    .id(("msg", i))
                    .w_full()
                    .flex()
                    .when(user, |d| d.justify_end())
                    .child(if m.attachments.is_empty() {
                        div()
                            .max_w(px(560.))
                            .max_w(gpui::relative(0.85))
                            .min_w_0()
                            .px_3()
                            .py_2()
                            .rounded_lg()
                            .bg(bg)
                            .child(text)
                    } else {
                        // A fixed width: text in a shrink-to-fit column (and
                        // with only a max width) is measured wider than it
                        // ends up, so it would overflow the bubble.
                        div()
                            .w(px(480.))
                            .max_w(gpui::relative(0.9))
                            .px_3()
                            .py_2()
                            .rounded_lg()
                            .bg(bg)
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(self.message_attachments(&m.attachments))
                            .when(has_text, |d| d.child(text))
                    })
            })
            .collect();
        let empty_hint = bubbles.is_empty().then(|| {
            div().m_auto().text_color(rgb(0x6c7086)).child(match session {
                Some(s) => format!("Start a conversation with {}.", engine::agent_name(&s.agent)),
                None => "Pick an agent under “New chat with” to start.".into(),
            })
        });

        let (shown, input_color) = match session {
            Some(s) if agent.is_none() && !self.scanning => (
                format!("{} is not installed on this machine.", engine::agent_name(&s.agent)).into(),
                rgb(0xf38ba8),
            ),
            _ if self.input.is_empty() => (
                SharedString::from("Message · / commands · drop or paste files · Enter send · Esc stop"),
                rgb(0x6c7086),
            ),
            _ => (SharedString::from(format!("{}▏", self.input)), rgb(0xf2f2f7)),
        };

        let palette = self.render_palette(cx);

        let header = div()
            .px_4()
            .py_3()
            .flex()
            .items_center()
            .justify_between()
            .border_b_1()
            .border_color(rgb(0x313244))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .child(div().font_weight(FontWeight::BOLD).truncate().child(title))
                    .child(div().text_xs().text_color(rgb(0x9399b2)).truncate().child(subtitle)),
            )
            .child(
                div()
                    .flex()
                    .flex_none()
                    .gap_2()
                    .when(session.is_some(), |d| {
                        d.child(button("model", format!("{} ▾", self.model_label())).on_click(
                            cx.listener(|this, _: &ClickEvent, window, cx| {
                                this.open_picker("model", cx);
                                window.focus(&this.focus);
                            }),
                        ))
                    })
                    .children(self.session().map(|s| {
                        button("mode", format!("{} ▾", s.mode.name())).on_click(cx.listener(
                            |this, _: &ClickEvent, window, cx| {
                                this.open_picker("mode", cx);
                                window.focus(&this.focus);
                            },
                        ))
                    }))
                    .children(self.effort_label().map(|label| {
                        button("effort", format!("{label} ▾")).on_click(cx.listener(
                            |this, _: &ClickEvent, window, cx| {
                                this.open_picker("effort", cx);
                                window.focus(&this.focus);
                            },
                        ))
                    }))
                    .when(busy, |d| {
                        d.child(
                            button("stop", "Stop")
                                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.stop(cx))),
                        )
                    })
                    .child(
                        button("panel", if self.settings.hide_panel { "Panel ◂" } else { "Panel ▸" })
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.toggle_panel(cx))),
                    ),
            );

        div()
            .key_context("Chat")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, _, cx| this.on_key(ev, cx)))
            .on_mouse_down(gpui::MouseButton::Left, cx.listener(|this, _, _, cx| {
                this.preview_blur(cx);
                this.term_blur(cx);
                this.editor_blur(cx);
                this.git_blur(cx);
            }))
            .on_mouse_move(cx.listener(|this, ev: &gpui::MouseMoveEvent, window, cx| {
                this.panel_resize_move(ev, window, cx);
                this.preview_drag_move(ev.position);
                if ev.pressed_button == Some(gpui::MouseButton::Left) {
                    this.term_drag_move(ev.position, cx);
                    this.editor_drag_move(ev.position, cx);
                }
            }))
            .on_mouse_up(gpui::MouseButton::Left, cx.listener(|this, ev: &gpui::MouseUpEvent, _, cx| {
                this.preview_mouse_release(ev.position, ev.click_count as u32);
                this.panel_resize_end(cx);
                this.term_drag_end(cx);
                this.editor_drag_end(cx);
            }))
            .size_full()
            .flex()
            .bg(rgb(0x1e1e2e))
            .text_color(rgb(0xf2f2f7))
            .text_sm()
            .child(self.render_sidebar(cx))
            .child(
                div()
                    .id("chat-column")
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .flex()
                    .flex_col()
                    .drag_over::<ExternalPaths>(|style, _, _, _| style.bg(rgb(0x262838)))
                    .on_drop(cx.listener(|this, paths: &ExternalPaths, window, cx| {
                        this.add_files(paths.paths().to_vec(), cx);
                        window.focus(&this.focus);
                    }))
                    .when(self.show_settings, |d| d.child(self.render_settings(cx)))
                    .when(!self.show_settings, |d| {
                        d.child(header)
                            .child(
                                div()
                                    .id("messages")
                                    .flex_1()
                                    .overflow_y_scroll()
                                    .track_scroll(&self.scroll)
                                    .p_4()
                                    .flex()
                                    .flex_col()
                                    .gap_3()
                                    .children(bubbles)
                                    .children(empty_hint),
                            )
                            .children(palette)
                            .child(
                                div()
                                    .p_3()
                                    .border_t_1()
                                    .border_color(rgb(0x313244))
                                    .flex()
                                    .flex_col()
                                    .gap_2()
                                    .children(self.attach_note.clone().map(|note| {
                                        div().text_xs().text_color(rgb(0xf38ba8)).child(note)
                                    }))
                                    .children(self.render_pending(cx))
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap_2()
                                            .child(
                                                button("attach", "+ Attach")
                                                    .flex_none()
                                                    .py_2()
                                                    .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                                        this.pick_files(cx);
                                                        window.focus(&this.focus);
                                                    })),
                                            )
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .min_w_0()
                                                    .px_3()
                                                    .py_2()
                                                    .rounded_lg()
                                                    .bg(rgb(0x2b2d3a))
                                                    .text_color(input_color)
                                                    .child(shown),
                                            ),
                                    ),
                            )
                    })
            )
            .when(!self.settings.hide_panel, |d| d.child(self.render_panel(window, cx)))
    }
}

#[cfg(test)]
mod tests {
    use super::{kept_by_message, unused_sessions, Role, Session};
    use crate::engine::attach::{Attachment, Kind};
    use std::path::PathBuf;

    #[test]
    fn files_kept_by_earlier_messages_are_recognised() {
        let file = |path: &str| Attachment { path: PathBuf::from(path), name: "a.png".into(), kind: Kind::Image, size: 1 };
        let mut s = Session::new("claude", PathBuf::from("/tmp"));
        s.push_with(Role::User, "look", vec![file("/attachments/s/a.png")]);
        assert!(kept_by_message(&s, &PathBuf::from("/attachments/s/a.png")));
        assert!(!kept_by_message(&s, &PathBuf::from("/attachments/s/b.png")));
    }

    #[test]
    fn only_unused_empty_chats_are_pruned() {
        let new = |terminal: bool, message: bool| {
            let mut s = Session::new("claude", PathBuf::from("/tmp"));
            s.terminal = terminal;
            if message {
                s.push(Role::User, "hi");
            }
            s
        };
        let sessions = vec![new(false, false), new(false, false), new(true, false), new(false, true)];
        let current = sessions[1].id.clone();
        assert_eq!(unused_sessions(&sessions, Some(&current)), vec![sessions[0].id.clone()]);
        assert_eq!(unused_sessions(&sessions, None).len(), 2);
    }
}
