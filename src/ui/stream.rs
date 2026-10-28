//! A session's message stream: user and agent messages, tool-call rows, markdown, code / chart / image
//! blocks and the lightbox. Spec: docs/design/chat.md §1–§2, §6; tools-and-behaviour.md §B11, §C 20–26.

mod blocks;
pub(in crate::ui) use blocks::token_colour;
pub(in crate::ui) mod code;
pub mod demo;
mod markdown;

use super::{shell::Shell, theme::Theme};
use crate::{
    engine::{attach::Attachment, Mode},
    store::{Message, Role, Session, Todo, ToolCall, Usage},
};
use blocks::{logo, message_row};
use gpui::{
    canvas, div, img, list, prelude::*, AnyElement, App, ClipboardItem, Context, Entity, FocusHandle, Hsla,
    KeyDownEvent, ListAlignment, ListState, MouseButton, MouseDownEvent, ObjectFit, Window,
};
use crate::ui::scale::px;
use markdown::{Block, Text};
use std::{collections::HashMap, path::{Path, PathBuf}, rc::Rc};

/// Stream view state, not persisted: each session's list and rows, the open lightbox, and the text selection.
#[derive(Default)]
pub struct StreamUi {
    sessions: HashMap<String, SessionStream>,
    lightbox: Option<Lightbox>,
    selection: Option<Selection>,
    focus: Option<FocusHandle>,
}

/// What a stream draws from its host: the theme, the app name and text size, and the session to show.
pub struct StreamEnv<'a> {
    pub theme: &'a Theme,
    pub app_name: &'a str,
    pub text_size: f32,
    pub session: &'a Session,
    pub busy: bool,
    /// Settings → General "Show tool calls in chat" (chat.md §1.4 item 2).
    pub show_tools: bool,
    /// Where the session runs and on which model, as the tile header names them: `ArchonUI · Sonnet 5.5`.
    pub place: String,
}

/// What a stream asks of the view that hosts it. `Shell` implements it; the stream demo has its own view.
pub trait StreamHost: Sized + 'static {
    fn stream_ui(&mut self) -> &mut StreamUi;
    fn stream_toast(&self, message: String, cx: &mut Context<Self>);
    fn stream_attach(&mut self, session: &str, files: Vec<Attachment>, cx: &mut Context<Self>);
    fn stream_open_link(&mut self, url: &str, cx: &mut Context<Self>);
}

/// One session's list. `items` is what the list renders; each row is cached until its signature changes.
struct SessionStream {
    list: ListState,
    items: Rc<Vec<Rc<Item>>>,
    text_size: f32,
}

/// A message as the stream draws it: its markdown blocks and the facts its header and rows need.
struct Item {
    sig: Sig,
    role: Role,
    tools: Vec<ToolCall>,
    attachments: Vec<Attachment>,
    thought_secs: Option<u32>,
    took_secs: Option<u32>,
    /// Registry id of the agent that wrote a reply.
    agent: Option<String>,
    /// Progress notes written between tool calls, shown dimmed above the final answer. Empty when the
    /// reply has no separate final answer.
    progress: Vec<Block>,
    /// The reply, or its final answer when `progress` is not empty.
    blocks: Vec<Block>,
    usage: Option<Usage>,
    todos: Vec<Todo>,
}

/// What changes when a message grows or a tool row finishes. Streaming only appends, so lengths cover it;
/// the final-answer offset and usage arrive when the turn ends.
type Sig = (usize, usize, usize, usize, Option<u32>, Option<u32>, Option<usize>, bool, u64);

/// What every row of one render needs.
struct RowCtx<H> {
    host: Entity<H>,
    theme: Theme,
    app_name: String,
    session: String,
    mode: Mode,
    /// Index of the reply that is still streaming, if the session is busy.
    streaming: Option<usize>,
    show_tools: bool,
    selection: Option<Selection>,
    /// Replies in this chat come from more than one agent, so each header names its agent.
    mixed_agents: bool,
    /// Body text size in px, from the settings; headings scale from it.
    text_size: f32,
}

/// The text selected in one block of one message. `anchor` and `head` are byte offsets into the block's text.
#[derive(Clone, Debug, PartialEq)]
struct Selection {
    session: String,
    item: usize,
    block: usize,
    anchor: usize,
    head: usize,
}

impl Selection {
    fn range(&self) -> std::ops::Range<usize> {
        self.anchor.min(self.head)..self.anchor.max(self.head)
    }
}

/// An image opened in the lightbox. `focus` takes the keyboard so Escape closes it.
#[derive(Clone)]
struct Lightbox {
    path: PathBuf,
    name: String,
    caption: String,
    focus: FocusHandle,
}

impl Item {
    fn from_message(message: &Message, cwd: &Path) -> Self {
        let (progress, blocks) = match (message.role, message.final_at) {
            (Role::Agent, Some(at)) if message.text.is_char_boundary(at) => {
                let (progress, next) = markdown::parse_from(&message.text[..at], cwd, 0);
                (progress, markdown::parse_from(&message.text[at..], cwd, next).0)
            }
            (Role::Agent, _) => (Vec::new(), markdown::parse(&message.text, cwd)),
            (Role::User | Role::Error, _) => {
                (Vec::new(), vec![Block::Text(Text { id: 0, heading: None, text: message.text.clone(), marks: Vec::new() })])
            }
        };
        Self {
            sig: signature(message),
            role: message.role,
            tools: message.tools.clone(),
            attachments: message.attachments.clone(),
            thought_secs: message.thought_secs,
            took_secs: message.took_secs,
            agent: message.agent.clone(),
            progress,
            blocks,
            usage: message.usage,
            todos: message.todos.clone(),
        }
    }
}

/// Whether the replies in `messages` were written by more than one agent.
fn mixed_agents(messages: &[Message]) -> bool {
    let mut agents = messages.iter().filter_map(|m| m.agent.as_deref());
    agents.next().is_some_and(|first| agents.any(|a| a != first))
}

fn signature(message: &Message) -> Sig {
    (
        message.text.len(),
        message.tools.len(),
        message.tools.iter().filter(|tool| tool.ok.is_some()).count(),
        message.attachments.len(),
        message.thought_secs,
        message.took_secs,
        message.final_at,
        message.usage.is_some(),
        todo_sig(&message.todos),
    )
}

/// Changes whenever a checklist item is added, removed, renamed or changes status.
fn todo_sig(todos: &[Todo]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for t in todos {
        t.text.hash(&mut h);
        (t.status as u8).hash(&mut h);
    }
    h.finish()
}

impl StreamUi {
    /// The message list for `env.session`, or its empty state. Rows are cached per message and re-parsed only
    /// when their signature changes; the list is spliced to match so the changed heights are measured again.
    pub fn render<H: StreamHost>(&mut self, host: Entity<H>, env: StreamEnv, cx: &mut App) -> AnyElement {
        let focus = self.focus.get_or_insert_with(|| cx.focus_handle()).clone();
        let entry = self.sessions.entry(env.session.id.clone()).or_insert_with(|| SessionStream {
            list: ListState::new(0, ListAlignment::Bottom, px(480.)),
            items: Rc::new(Vec::new()),
            text_size: env.text_size,
        });

        let previous = entry.items.clone();
        let mut items: Vec<Rc<Item>> = Vec::with_capacity(env.session.messages.len());
        let mut changed = Vec::new();
        for (ix, message) in env.session.messages.iter().enumerate() {
            match previous.get(ix) {
                Some(item) if item.sig == signature(message) => items.push(item.clone()),
                _ => {
                    changed.push(ix);
                    items.push(Rc::new(Item::from_message(message, &env.session.cwd)));
                }
            }
        }

        let (old_len, new_len) = (entry.list.item_count(), items.len());
        if env.text_size != entry.text_size {
            // Text size changes every height, so every kept row is measured again.
            entry.text_size = env.text_size;
            changed.extend(0..old_len.min(new_len));
        }
        if new_len > old_len {
            entry.list.splice(old_len..old_len, new_len - old_len);
        } else if new_len < old_len {
            entry.list.splice(new_len..old_len, 0);
        }
        for ix in changed.into_iter().filter(|&ix| ix < old_len.min(new_len)) {
            entry.list.splice(ix..ix + 1, 1);
        }

        let streaming = (env.busy && items.last().is_some_and(|item| item.role == Role::Agent)).then(|| new_len - 1);
        let items = Rc::new(items);
        entry.items = items.clone();
        let list_state = entry.list.clone();

        let body = if items.is_empty() {
            empty_state(&env)
        } else {
            let ctx = Rc::new(RowCtx {
                host: host.clone(),
                theme: env.theme.clone(),
                app_name: env.app_name.to_string(),
                session: env.session.id.clone(),
                mode: env.session.mode.clone(),
                streaming,
                show_tools: env.show_tools,
                selection: self.selection.clone(),
                mixed_agents: mixed_agents(&env.session.messages),
                text_size: env.text_size,
            });
            div()
                .flex_1()
                .min_h_0()
                .px(px(18.))
                .pt(px(18.))
                .pb(px(8.))
                .child(
                    list(list_state, move |ix, _window, _cx| message_row(&ctx, ix, &items[ix], items.get(ix + 1).map(|i| &**i)))
                        .size_full(),
                )
                .into_any_element()
        };

        div()
            .id("stream")
            .track_focus(&focus)
            .on_key_down(copy_on_ctrl_c(host))
            .size_full()
            .flex()
            .flex_col()
            .font_family(env.theme.chat_font.clone())
            .text_size(px(env.text_size))
            .text_color(env.theme.fg.to_hsla())
            .child(body)
            .into_any_element()
    }

    /// The open lightbox, drawn over the window. Any click closes it (tools-and-behaviour.md §B11).
    pub fn lightbox_overlay<H: StreamHost>(&self, host: Entity<H>, theme: &Theme) -> Option<AnyElement> {
        let lightbox = self.lightbox.clone()?;
        let close = {
            let host = host.clone();
            move |_: &MouseDownEvent, _: &mut Window, cx: &mut App| close_lightbox(&host, cx)
        };
        // Escape closes too. The overlay holds focus while open (see `Lightbox::focus`). A window-level listener
        // from a paint-time canvas would not work: its dispatch node is a leaf, never on the focus path.
        let escape = {
            let host = host.clone();
            move |event: &KeyDownEvent, _: &mut Window, cx: &mut App| {
                if event.keystroke.key == "escape" {
                    close_lightbox(&host, cx);
                }
            }
        };
        // The stream takes focus on mouse-down, after the thumbnail's own handler, so the lightbox claims focus back
        // when it paints. `focus` is a no-op once held, so this does not redraw.
        let claim_focus = {
            let focus = lightbox.focus.clone();
            canvas(
                |_, _, _| (),
                move |_, _, window, _cx| {
                    if !focus.is_focused(window) {
                        focus.focus(window);
                    }
                },
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full()
        };
        let scrim: Hsla = gpui::rgba(0x000000d9).into();
        let caption = (!lightbox.caption.is_empty()).then(|| {
            div().text_color(theme.muted.to_hsla()).child(format!("· {}", lightbox.caption))
        });
        Some(
            div()
                .track_focus(&lightbox.focus)
                .on_key_down(escape)
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .occlude()
                .bg(scrim)
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(14.))
                .cursor_pointer()
                .on_mouse_down(MouseButton::Left, close)
                .child(claim_focus)
                .child(
                    div()
                        .w(px(1440.))
                        .h(px(810.))
                        .rounded(px(10.))
                        .overflow_hidden()
                        .child(img(lightbox.path).size_full().object_fit(ObjectFit::Contain)),
                )
                .child(
                    div()
                        .flex()
                        .gap(px(4.))
                        .text_size(px(13.))
                        .text_color(theme.fg.to_hsla())
                        .child(lightbox.name)
                        .children(caption),
                )
                .into_any_element(),
        )
    }

    /// Copies the selected text to the clipboard (Ctrl+C in the stream).
    fn copy_selection(&self, cx: &mut App) {
        if let Some(text) = self.selected_text() {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    fn selected_text(&self) -> Option<String> {
        let selection = self.selection.as_ref()?;
        let item = self.sessions.get(&selection.session)?.items.get(selection.item)?;
        let text = find_text(&item.blocks, selection.block)?;
        text.text.get(selection.range()).filter(|s| !s.is_empty()).map(str::to_string)
    }
}

fn close_lightbox<H: StreamHost>(host: &Entity<H>, cx: &mut App) {
    host.update(cx, |host, cx| {
        host.stream_ui().lightbox = None;
        cx.notify();
    });
}

/// The text block with `id`, looking inside lists, quotes and tables.
fn find_text(blocks: &[Block], id: usize) -> Option<&Text> {
    blocks.iter().find_map(|block| match block {
        Block::Text(text) => (text.id == id).then_some(text),
        Block::List { items, .. } => items.iter().find_map(|item| find_text(item, id)),
        Block::Quote(inner) => find_text(inner, id),
        Block::Table { head, rows } => head.iter().chain(rows.iter().flatten()).find(|cell| cell.id == id),
        _ => None,
    })
}

/// Ctrl+C (or Cmd+C) copies the stream's selection.
fn copy_on_ctrl_c<H: StreamHost>(host: Entity<H>) -> impl Fn(&KeyDownEvent, &mut Window, &mut App) + 'static {
    move |event, _window, cx| {
        let keystroke = &event.keystroke;
        if (keystroke.modifiers.control || keystroke.modifiers.platform) && keystroke.key == "c" {
            host.update(cx, |host, cx| host.stream_ui().copy_selection(cx));
        }
    }
}

/// The empty state of a session with no messages (chat.md §1.2).
fn empty_state(env: &StreamEnv) -> AnyElement {
    let theme = env.theme;
    div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(12.))
        .child(div().opacity(0.8).children(logo(theme, 64.)))
        .child(
            div()
                .font_family(theme.display_font.clone())
                .text_size(px(28.))
                .text_color(theme.fg.to_hsla())
                .text_center()
                .child(format!("What should {} work on?", env.app_name)),
        )
        .child(
            div()
                .text_size(px(12.))
                .text_color(theme.muted.to_hsla())
                .child(format!("in {}", env.place)),
        )
        .into_any_element()
}

impl Shell {
    /// The scrolling message list for `session_id`.
    pub(super) fn render_stream(
        &mut self,
        session_id: &str,
        _focused: bool,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(session) = self.state.session(session_id) else {
            return div().into_any_element();
        };
        let env = StreamEnv {
            theme: &self.theme,
            app_name: &self.state.settings.app_name,
            text_size: self.state.settings.text_size,
            session,
            busy: self.state.busy(session_id),
            show_tools: self.state.settings.show_tools,
            place: format!("{} · {}", self.session_place(session), self.trigger_label(session)),
        };
        self.stream.render(cx.entity(), env, cx)
    }

    /// The image lightbox, if open (drawn by `render_overlays`).
    pub(super) fn render_lightbox(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        self.stream.lightbox_overlay(cx.entity(), &self.theme)
    }

    /// Opens a link in the Browser tab. Only web and mail links: agent output can carry `file://` or
    /// `javascript:` targets.
    pub(super) fn open_url(&mut self, url: &str, cx: &mut Context<Self>) {
        if !super::shell::link_allowed(url) {
            self.toast("Only web and mail links open", cx);
            return;
        }
        self.browser_open(url, cx);
    }
}

impl StreamHost for Shell {
    fn stream_ui(&mut self) -> &mut StreamUi {
        &mut self.stream
    }

    fn stream_toast(&self, message: String, cx: &mut Context<Self>) {
        self.toast(message, cx);
    }

    fn stream_attach(&mut self, session: &str, files: Vec<Attachment>, cx: &mut Context<Self>) {
        self.state.add_pending(session, files);
        cx.notify();
    }

    fn stream_open_link(&mut self, url: &str, cx: &mut Context<Self>) {
        self.open_url(url, cx);
    }
}
