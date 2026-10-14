//! The settings page: it takes the chat column while open, and every control
//! saves as soon as it changes.

use super::{button, open_file, section_label, ChatView};
use crate::{
    engine::{Capabilities, DetectedAgent, ModelInfo},
    preview,
    store::{self, IdeKind, SearchEngine, Settings},
};
use gpui::{
    div, prelude::*, px, rgb, ClickEvent, Context, Div, ElementId, FontWeight, PathPromptOptions, SharedString,
    Stateful,
};
use std::path::{Path, PathBuf};

/// A per-agent setting, whose choices list inline under its button.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum AgentSetting {
    Model,
    Effort,
}

/// One entry in an agent's inline list; `value` `None` is the agent's own default.
struct Choice {
    value: Option<String>,
    label: String,
    hint: String,
    pinned: bool,
}

impl ChatView {
    pub(super) fn open_settings(&mut self, cx: &mut Context<Self>) {
        self.show_settings = true;
        self.settings_expanded = None;
        cx.notify();
    }

    pub(super) fn close_settings(&mut self, cx: &mut Context<Self>) {
        self.show_settings = false;
        self.settings_expanded = None;
        cx.notify();
    }

    /// Saves a changed setting and applies it to the preview's search engine.
    fn settings_changed(&mut self, cx: &mut Context<Self>) {
        preview::set_search_engine(self.settings.search_engine.url_prefix());
        self.save_settings();
        cx.notify();
    }

    /// Asks for one file or folder and stores it with `apply`; cancelling changes nothing.
    fn choose_path(&mut self, files: bool, apply: fn(&mut Settings, PathBuf), cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files,
            directories: !files,
            multiple: false,
            prompt: Some("Choose".into()),
        });
        cx.spawn(async move |this, cx| {
            let result = picked.await;
            this.update(cx, |this, cx| match result {
                Ok(Ok(Some(paths))) => {
                    if let Some(path) = paths.into_iter().next() {
                        apply(&mut this.settings, path);
                        this.settings_changed(cx);
                    }
                }
                Ok(Ok(None)) => {} // cancelled
                Ok(Err(e)) => eprintln!("could not open the file picker: {e}"),
                Err(_) => {}
            })
            .ok();
        })
        .detach();
    }

    /// Opens an agent's model or effort list, or closes it if it is already open.
    fn toggle_agent_list(&mut self, agent: &'static str, which: AgentSetting, cx: &mut Context<Self>) {
        let open = Some((agent, which));
        self.settings_expanded = if self.settings_expanded == open { None } else { open };
        cx.notify();
    }

    /// Sets an agent's default model or effort (`None` for the agent's own) and closes its list.
    fn set_agent_default(&mut self, agent: &'static str, which: AgentSetting, value: Option<String>, cx: &mut Context<Self>) {
        self.settings_expanded = None;
        let defaults = self.settings.agent_defaults.entry(agent.to_string()).or_default();
        match which {
            AgentSetting::Model => {
                // An effort the new model doesn't offer goes back to the agent's default.
                let levels = self
                    .caps
                    .get(agent)
                    .and_then(Option::as_ref)
                    .map(|c| c.efforts(value.as_deref()).to_vec())
                    .unwrap_or_default();
                defaults.model = value;
                if defaults.effort.as_ref().is_some_and(|e| !levels.contains(e)) {
                    defaults.effort = None;
                }
            }
            AgentSetting::Effort => defaults.effort = value,
        }
        self.settings_changed(cx);
    }

    /// The settings page, shown in place of the chat.
    pub(super) fn render_settings(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let settings = &self.settings;
        let header = div()
            .px_4()
            .py_3()
            .flex()
            .items_center()
            .justify_between()
            .border_b_1()
            .border_color(rgb(0x313244))
            .child(div().font_weight(FontWeight::BOLD).child("Settings"))
            .child(
                button("settings-done", "Done")
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.close_settings(cx))),
            );

        let folder = settings.default_folder.as_ref();
        let folder_text = folder.map_or("Not set".to_string(), |p| p.display().to_string());
        let folder_note = match folder {
            Some(_) => "New chats run here, if it still exists.".to_string(),
            None => format!("Folder ArchonUI was started in: {}", self.default_cwd.display()),
        };
        let agent_choices: Vec<(Option<String>, String)> = std::iter::once((None, "First found".to_string()))
            .chain(self.agents.iter().map(|a| (Some(a.harness.id().to_string()), a.harness.name().to_string())))
            .collect();

        let server = settings.code_server_path.as_ref();
        let server_text = server.map_or("Found automatically".to_string(), |p| p.display().to_string());
        let status = self.ide_status();

        let data_dir = store::dir().parent().map(Path::to_path_buf).unwrap_or_default();
        let terminal_size = settings.terminal_font_size;

        let page = div()
            .id("settings-page")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .pb_4()
            .child(section_label("General"))
            .child(row(
                "Default folder for new chats",
                folder_note,
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(path_text(folder_text))
                    .child(choose_button("folder-choose", false, |s, p| s.default_folder = Some(p), cx))
                    .when(folder.is_some(), |d| {
                        d.child(reset_button("folder-reset", |s| s.default_folder = None, cx))
                    }),
            ))
            .child(row(
                "Default agent",
                "New chats use this agent when none is picked.",
                choice_group(
                    "default-agent",
                    agent_choices,
                    &settings.default_agent,
                    |s, agent| s.default_agent = agent,
                    cx,
                ),
            ))
            .child(section_label("Agents"))
            .children(self.agents.is_empty().then(|| muted_line("No agent CLIs found.")))
            .children(self.agents.iter().enumerate().map(|(i, a)| self.agent_block(i, a, cx)))
            .child(section_label("Preview"))
            .child(row(
                "Search engine",
                "Searches from the address bar go here.",
                choice_group(
                    "search-engine",
                    SearchEngine::ALL.iter().map(|e| (*e, e.name().to_string())).collect(),
                    &settings.search_engine,
                    |s, engine| s.search_engine = engine,
                    cx,
                ),
            ))
            .child(row(
                "Open local sites from replies automatically",
                "Shows a local web page or HTML file from an agent's reply in the preview.",
                toggle("auto-open", settings.auto_open_preview, |s, on| s.auto_open_preview = on, cx),
            ))
            .child(section_label("IDE"))
            .child(row(
                "Editor",
                "The editor the IDE tab shows.",
                choice_group(
                    "ide-editor",
                    vec![(IdeKind::Builtin, "Built-in".to_string()), (IdeKind::VsCode, "VS Code".to_string())],
                    &settings.ide,
                    |s, ide| s.ide = ide,
                    cx,
                ),
            ))
            .when(settings.ide == IdeKind::VsCode, |d| {
                d.child(row(
                    "Start VS Code when ArchonUI opens",
                    "Starts code-server in the background, so the IDE tab is ready.",
                    toggle("ide-autostart", settings.ide_autostart, |s, on| s.ide_autostart = on, cx),
                ))
                .child(row(
                    "code-server program",
                    "The program that runs VS Code.",
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(path_text(server_text))
                        .child(choose_button("code-server-choose", true, |s, p| s.code_server_path = Some(p), cx))
                        .when(server.is_some(), |d| {
                            d.child(reset_button("code-server-reset", |s| s.code_server_path = None, cx))
                        }),
                ))
                .children((!status.is_empty()).then(|| muted_line(status)))
                .child(row(
                    "Restart VS Code",
                    "Restarts the VS Code server.",
                    button("ide-restart", "Restart VS Code")
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.restart_ide(cx))),
                ))
            })
            .child(section_label("Terminal"))
            .child(row(
                "Run terminals in tmux",
                "Shells keep running after ArchonUI closes. Applies to new terminals.",
                toggle("use-tmux", settings.use_tmux, |s, on| s.use_tmux = on, cx),
            ))
            .child(row(
                "Text size",
                "Terminal text size in pixels, from 10 to 24.",
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(button("font-smaller", "−").on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.settings.terminal_font_size = (this.settings.terminal_font_size - 1.).clamp(10., 24.);
                        this.settings_changed(cx);
                    })))
                    .child(div().w(px(28.)).flex().justify_center().child(format!("{terminal_size}")))
                    .child(button("font-larger", "+").on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.settings.terminal_font_size = (this.settings.terminal_font_size + 1.).clamp(10., 24.);
                        this.settings_changed(cx);
                    }))),
            ))
            .child(section_label("About"))
            .child(row(
                "ArchonUI data folder",
                "Chats, attachments and settings are kept here.",
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(path_text(data_dir.display().to_string()))
                    .child(
                        button("data-open", "Open")
                            .on_click(cx.listener(move |_, _: &ClickEvent, _, _| open_file(&data_dir))),
                    ),
            ));

        div().flex_1().min_h_0().flex().flex_col().child(header).child(page)
    }

    /// One detected agent: its name, then its default model and effort. Each opens an
    /// inline list of choices below its button.
    fn agent_block(&self, index: usize, agent: &DetectedAgent, cx: &mut Context<Self>) -> Div {
        let id = agent.harness.id();
        let block = div()
            .px_4()
            .py_2()
            .flex()
            .flex_col()
            .gap_2()
            .child(div().font_weight(FontWeight::BOLD).child(agent.harness.name()));
        let Some(Some(caps)) = self.caps.get(id) else {
            return block.child(muted_line("Loading…"));
        };

        let defaults = self.settings.agent_defaults.get(id).cloned().unwrap_or_default();
        let model_label = match defaults.model.as_deref() {
            None => "Agent default".to_string(),
            Some(m) => caps.models.iter().find(|x| x.id == m).map_or_else(|| m.to_string(), |x| x.label.clone()),
        };
        let levels = caps.efforts(defaults.model.as_deref());
        let effort_label = defaults.effort.clone().unwrap_or_else(|| "Agent default".to_string());
        let buttons = div()
            .flex()
            .flex_wrap()
            .gap_2()
            .child(
                button(("agent-model", index), format!("Model: {model_label} ▾")).on_click(cx.listener(
                    move |this, _: &ClickEvent, _, cx| this.toggle_agent_list(id, AgentSetting::Model, cx),
                )),
            )
            .when(!levels.is_empty(), |d| {
                d.child(
                    button(("agent-effort", index), format!("Effort: {effort_label} ▾")).on_click(cx.listener(
                        move |this, _: &ClickEvent, _, cx| this.toggle_agent_list(id, AgentSetting::Effort, cx),
                    )),
                )
            });

        let open = self.settings_expanded.filter(|(agent, _)| *agent == id).map(|(_, which)| which);
        let list = open.map(|which| match which {
            AgentSetting::Model => choice_list(
                id,
                index,
                which,
                model_choices(self.settings.pins(id), caps),
                defaults.model.clone(),
                cx,
            ),
            AgentSetting::Effort => {
                choice_list(id, index, which, effort_choices(levels), defaults.effort.clone(), cx)
            }
        });
        block.child(buttons).children(list)
    }
}

/// A setting: its name and a muted description on the left, its control on the right.
fn row(label: impl Into<SharedString>, description: impl Into<SharedString>, control: impl IntoElement) -> Div {
    div()
        .px_4()
        .py_2()
        .flex()
        .items_center()
        .justify_between()
        .gap_4()
        .child(
            div()
                .flex_1()
                .min_w_0()
                .child(label.into())
                .child(div().text_xs().text_color(rgb(0x7f849c)).child(description.into())),
        )
        .child(div().flex_none().max_w(px(340.)).child(control))
}

/// A line of muted small text, indented like a setting's row.
fn muted_line(text: impl Into<SharedString>) -> Div {
    div().px_4().py_1().text_xs().text_color(rgb(0x7f849c)).child(text.into())
}

/// A file or folder path, muted and truncated.
fn path_text(path: String) -> Div {
    div().max_w(px(200.)).min_w_0().truncate().text_xs().text_color(rgb(0x7f849c)).child(path)
}

/// A small button, highlighted while `selected`.
fn choice_button(id: impl Into<ElementId>, label: impl Into<SharedString>, selected: bool) -> Stateful<Div> {
    // GPUI allows one hover style per element, so pick colours before setting it.
    let (bg, hover) = if selected { (0x3b5bdb, 0x4c6ef5) } else { (0x313244, 0x45475a) };
    div()
        .id(id)
        .px_2()
        .py_1()
        .rounded_md()
        .text_xs()
        .cursor_pointer()
        .bg(rgb(bg))
        .hover(move |d| d.bg(rgb(hover)))
        .child(label.into())
}

/// "On" (blue) or "Off"; clicking it flips the setting with `apply`.
fn toggle(id: &'static str, on: bool, apply: fn(&mut Settings, bool), cx: &mut Context<ChatView>) -> Stateful<Div> {
    choice_button(id, if on { "On" } else { "Off" }, on).on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
        apply(&mut this.settings, !on);
        this.settings_changed(cx);
    }))
}

/// Buttons picking one of `options`, the current one highlighted.
fn choice_group<T: Clone + PartialEq + 'static>(
    id: &'static str,
    options: Vec<(T, String)>,
    current: &T,
    apply: fn(&mut Settings, T),
    cx: &mut Context<ChatView>,
) -> Div {
    div().flex().flex_wrap().justify_end().gap_1().children(options.into_iter().enumerate().map(
        |(n, (value, label))| {
            let selected = &value == current;
            choice_button((id, n), label, selected).on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                apply(&mut this.settings, value.clone());
                this.settings_changed(cx);
            }))
        },
    ))
}

/// "Choose…" for a file (`files`) or a folder, storing the pick with `apply`.
fn choose_button(
    id: &'static str,
    files: bool,
    apply: fn(&mut Settings, PathBuf),
    cx: &mut Context<ChatView>,
) -> Stateful<Div> {
    button(id, "Choose…").on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.choose_path(files, apply, cx)))
}

/// "Reset", clearing a path setting with `clear`.
fn reset_button(id: &'static str, clear: fn(&mut Settings), cx: &mut Context<ChatView>) -> Stateful<Div> {
    button(id, "Reset").on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
        clear(&mut this.settings);
        this.settings_changed(cx);
    }))
}

/// The model choices for an agent: its default, then the pinned models, then the rest.
fn model_choices(pins: &[String], caps: &Capabilities) -> Vec<Choice> {
    let model = |m: &ModelInfo, pinned: bool| Choice {
        value: Some(m.id.clone()),
        label: m.label.clone(),
        hint: if m.label == m.id { String::new() } else { m.id.clone() },
        pinned,
    };
    let default = Choice {
        value: None,
        label: "Agent default".into(),
        hint: caps.default_model.clone().unwrap_or_default(),
        pinned: false,
    };
    let pinned = pins.iter().filter_map(|id| caps.models.iter().find(|m| &m.id == id)).map(|m| model(m, true));
    let rest = caps.models.iter().filter(|m| !pins.contains(&m.id)).map(|m| model(m, false));
    std::iter::once(default).chain(pinned).chain(rest).collect()
}

/// The effort choices: the agent's default, then each of the model's `levels`.
fn effort_choices(levels: &[String]) -> Vec<Choice> {
    let default = Choice { value: None, label: "Agent default".into(), hint: String::new(), pinned: false };
    std::iter::once(default)
        .chain(levels.iter().map(|l| Choice {
            value: Some(l.clone()),
            label: l.clone(),
            hint: String::new(),
            pinned: false,
        }))
        .collect()
}

/// The inline list under an agent's model or effort button, with `current` highlighted.
fn choice_list(
    agent: &'static str,
    index: usize,
    which: AgentSetting,
    choices: Vec<Choice>,
    current: Option<String>,
    cx: &mut Context<ChatView>,
) -> Stateful<Div> {
    div()
        .id(("agent-list", index))
        .max_h(px(240.))
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .p_1()
        .rounded_md()
        .bg(rgb(0x181825))
        .border_1()
        .border_color(rgb(0x313244))
        .children(choices.into_iter().enumerate().map(|(n, choice)| {
            let Choice { value, label, hint, pinned } = choice;
            let selected = value == current;
            div()
                .id((agent, n))
                .px_2()
                .py_1()
                .rounded_sm()
                .flex()
                .items_center()
                .gap_2()
                .cursor_pointer()
                .when(selected, |d| d.bg(rgb(0x313244)))
                .hover(|d| d.bg(rgb(0x2a2b3c)))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.set_agent_default(agent, which, value.clone(), cx)
                }))
                .child(div().flex_none().child(label))
                .when(!hint.is_empty(), |d| {
                    d.child(div().min_w_0().truncate().text_xs().text_color(rgb(0x6c7086)).child(hint))
                })
                .when(pinned, |d| d.child(div().flex_none().text_xs().text_color(rgb(0x89b4fa)).child("pinned")))
        }))
}
