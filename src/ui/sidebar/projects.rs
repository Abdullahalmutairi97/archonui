//! The Projects view (layout.md §2.2): one card per project and "+ Open folder". Each card's
//! branch and language come from a background probe of its folder, cached until the app restarts.

use super::{Probe, Shell, Target};
use crate::{
    app::workspace::Project,
    git,
    ui::{
        composer::icons::stroke_icon,
        widgets::text_input::{text_input, TextInput},
    },
};

/// A cloud with an arrow down: a project to download.
const CLOUD_DOWN: &str = "M4.5 12a3 3 0 0 1-.4-6 4 4 0 0 1 7.8.6 2.7 2.7 0 0 1-.4 5.4M8 7.5v6M6 11.5l2 2 2-2";

/// How long ago `at` was, in words, for a project's last change.
fn changed_ago(at: u64, now: u64) -> String {
    match now.saturating_sub(at) / 60 {
        0 => "just now".to_string(),
        m if m < 60 => format!("{m} min ago"),
        m if m < 60 * 24 => format!("{} h ago", m / 60),
        m => format!("{} d ago", m / (60 * 24)),
    }
}
use gpui::{
    div, prelude::*, AnyElement, ClickEvent, Context, ElementId, Entity, MouseButton, MouseDownEvent,
    PathPromptOptions,
};
use crate::ui::scale::px;
use std::path::{Path as FsPath, PathBuf};

/// What a card shows that comes from the folder rather than from ArchonUI.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct ProjectInfo {
    branch: Option<String>,
    language: Option<&'static str>,
}

impl ProjectInfo {
    /// Reads the branch and language. Blocking, so run it on the background executor.
    fn probe(path: &FsPath) -> Self {
        let branch = git::status(path).ok().filter(|s| s.repo).and_then(|s| s.branch);
        ProjectInfo { branch, language: language(path) }
    }
}

/// The language of a folder, from its manifest. Best effort: a folder without one shows no dot.
fn language(path: &FsPath) -> Option<&'static str> {
    const MANIFESTS: [(&str, &str); 4] =
        [("Cargo.toml", "Rust"), ("go.mod", "Go"), ("pyproject.toml", "Python"), ("package.json", "JavaScript")];
    MANIFESTS.iter().find(|(file, _)| path.join(file).is_file()).map(|(_, lang)| *lang)
}

impl Shell {
    pub(super) fn projects_view(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let paths: Vec<PathBuf> = self.state.projects().iter().map(|p| p.path.clone()).collect();
        for path in &paths {
            self.ensure_probe(path, cx);
        }
        let current = self.state.workspace.current_project.clone();
        let rename = self.sidebar.rename.as_ref().map(|r| (r.target.clone(), r.input.clone()));
        let hidden = self.state.settings.hidden_projects.clone();
        let cards: Vec<AnyElement> = self
            .state
            .projects()
            .iter()
            .filter(|p| !hidden.contains(&p.id))
            .map(|project| {
                let renaming = rename.as_ref().and_then(|(target, input)| match target {
                    Target::Project(id) if *id == project.id => Some(input),
                    _ => None,
                });
                let active = current.as_deref() == Some(project.id.as_str());
                self.project_card(project, active, renaming, cx)
            })
            .collect();
        let remote = self.remote_project_cards(cx);
        div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .children(cards)
            .child(self.open_folder_button(cx))
            .children(remote)
            .into_any_element()
    }

    /// Projects on the user's other devices that aren't here yet, from the cloud's list. Clicking one
    /// downloads its latest copy into a new folder and opens it as the same project.
    fn remote_project_cards(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let projects = self.remote_projects();
        if projects.is_empty() {
            return None;
        }
        let theme = &self.theme;
        let (line2, muted, fg) = (theme.line2.to_hsla(), theme.muted.to_hsla(), theme.fg.to_hsla());
        let now = crate::store::now();
        let cards = projects.into_iter().map(|p| {
            let from = self.chat_device_name(&p.device);
            let busy = self.remote.downloading.contains(&p.project);
            let detail = if busy {
                "Downloading…".to_string()
            } else {
                format!("On {from} · changed {}", changed_ago(p.updated, now))
            };
            let (project, device) = (p.project.clone(), p.device.clone());
            div()
                .id(ElementId::Name(format!("remote-project-{}", p.project).into()))
                .flex()
                .items_center()
                .gap(px(10.))
                .px(px(14.))
                .py(px(10.))
                .rounded(px(9.))
                .border_1()
                .border_dashed()
                .border_color(line2)
                .cursor_pointer()
                .hover(move |s| s.bg(line2))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.open_from_cloud(&project, &device, cx)))
                .child(stroke_icon(CLOUD_DOWN, 16., 1.4, muted))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .child(div().overflow_hidden().whitespace_nowrap().text_ellipsis().text_size(px(14.)).text_color(fg).child(p.name.clone()))
                        .child(div().text_size(px(11.5)).text_color(muted).child(detail)),
                )
        });
        Some(
            div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .pt(px(8.))
                .child(div().text_size(px(11.)).text_color(muted).child("ON YOUR OTHER DEVICES"))
                .children(cards)
                .into_any_element(),
        )
    }

    /// Starts the probe for `path` once. The card redraws when it finishes.
    fn ensure_probe(&mut self, path: &PathBuf, cx: &mut Context<Self>) {
        if self.sidebar.probes.contains_key(path) {
            return;
        }
        self.sidebar.probes.insert(path.clone(), Probe::Running);
        let key = path.clone();
        let job = path.clone();
        cx.spawn(async move |this, cx| {
            let info = cx.background_executor().spawn(async move { ProjectInfo::probe(&job) }).await;
            this.update(cx, |this, cx| {
                this.sidebar.probes.insert(key, Probe::Done(info));
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn project_card(
        &self,
        project: &Project,
        active: bool,
        rename: Option<&Entity<TextInput>>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = &self.theme;
        let count = self.state.sessions().iter().filter(|s| s.visible() && s.project.as_deref() == Some(project.id.as_str())).count();
        let info = match self.sidebar.probes.get(&project.path) {
            Some(Probe::Done(info)) => Some(info),
            _ => None,
        };
        let name: AnyElement = match rename {
            Some(input) => text_input(input, theme).into_any_element(),
            None => div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .text_size(px(14.))
                .text_color(theme.fg.to_hsla())
                .child(project.name.clone())
                .into_any_element(),
        };
        let id = project.id.clone();
        let card = div()
            .id(ElementId::Name(format!("project-{}", project.id).into()))
            .flex()
            .flex_col()
            .gap(px(4.))
            .px(px(12.))
            .py(px(9.))
            .rounded(px(8.))
            .border_1()
            .border_color(if active { theme.acc.to_hsla() } else { theme.line2.to_hsla() })
            .when(active, |d| d.bg(theme.soft.to_hsla()))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener({
                    let id = id.clone();
                    move |this, ev: &MouseDownEvent, _, cx| this.open_menu(Target::Project(id.clone()), ev.position, cx)
                }),
            )
            .child(
                div()
                    .flex()
                    .items_baseline()
                    .gap(px(8.))
                    .child(name)
                    .child(
                        div()
                            .flex_none()
                            .text_size(px(11.))
                            .text_color(theme.muted.to_hsla())
                            .child(format!("{count} {}", if count == 1 { "session" } else { "sessions" })),
                    ),
            )
            .child(
                div()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .text_size(px(11.))
                    .text_color(theme.muted.to_hsla())
                    .child(project.path.display().to_string()),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .when_some(info.and_then(|i| i.branch.clone()), |row, branch| {
                        row.child(
                            div()
                                .px(px(6.))
                                .py(px(1.))
                                .rounded(px(4.))
                                .bg(theme.line2.to_hsla())
                                .text_size(px(10.5))
                                .text_color(theme.alt.to_hsla())
                                .child(format!("⎇ {branch}")),
                        )
                    })
                    .when_some(info.and_then(|i| i.language), |row, lang| {
                        row.child(
                            div()
                                .text_size(px(11.5))
                                .text_color(theme.alt.to_hsla())
                                .child(format!("● {lang}")),
                        )
                    }),
            );
        if rename.is_some() {
            return card.into_any_element();
        }
        card.cursor_pointer()
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.select_project(&id, cx)))
            .into_any_element()
    }

    /// The dashed "+ Open folder" button under the cards.
    fn open_folder_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = &self.theme;
        div()
            .id("open-folder")
            .flex()
            .justify_center()
            .py(px(11.))
            .rounded(px(8.))
            .border_1()
            .border_dashed()
            .border_color(theme.line2.to_hsla())
            .text_size(px(13.))
            .text_color(theme.muted.to_hsla())
            .cursor_pointer()
            .on_click(cx.listener(|_, _: &ClickEvent, _, cx| {
                pick_folder(cx, |this, path, cx| this.add_project_folder(path, cx));
            }))
            .child("+ Open folder")
    }

    /// Makes the project the current one, which new sessions and the panels then use.
    fn select_project(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(name) = self.state.project(id).map(|p| p.name.clone()) else { return };
        self.state.workspace.current_project = Some(id.to_string());
        self.toast(format!("Switched to {name}"), cx);
        self.save(cx);
    }

    /// Adds the folder as a project named after its last component.
    fn add_project_folder(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let name = path.file_name().map_or_else(|| "project".to_string(), |n| n.to_string_lossy().into_owned());
        self.state.add_project(&name, path);
        self.toast(format!("Added {name}"), cx);
        self.save(cx);
    }

    /// Points the project at a new folder, keeping its name.
    pub(super) fn change_project_folder(&mut self, id: String, cx: &mut Context<Self>) {
        pick_folder(cx, move |this, path, cx| {
            let Some(project) = this.state.project(&id) else { return };
            let (name, old) = (project.name.clone(), project.path.clone());
            this.state.update_project(&id, &name, path);
            this.sidebar.probes.remove(&old);
            this.save(cx);
        });
    }

    /// Deletes the project. Its sessions stay and lose their project.
    pub(super) fn delete_project(&mut self, id: &str, cx: &mut Context<Self>) {
        self.state.remove_project(id);
        self.persist_sidebar(cx);
    }
}

/// Asks for a folder, then runs `then` with it. A cancelled picker does nothing.
fn pick_folder(cx: &mut Context<Shell>, then: impl FnOnce(&mut Shell, PathBuf, &mut Context<Shell>) + 'static) {
    let picked = cx.prompt_for_paths(PathPromptOptions {
        files: false,
        directories: true,
        multiple: false,
        prompt: Some("Open".into()),
    });
    cx.spawn(async move |this, cx| {
        let Ok(Ok(Some(paths))) = picked.await else { return };
        let Some(path) = paths.into_iter().next() else { return };
        this.update(cx, |this, cx| then(this, path, cx)).ok();
    })
    .detach();
}
