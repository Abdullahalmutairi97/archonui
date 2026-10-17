//! The right-click menu on session rows and project cards (brief §B7, §B8). It opens at the
//! pointer, outside the sidebar's clip, and closes on a click elsewhere or on a choice.

use super::{Menu, Shell, Target};
use crate::ui::theme::Theme;
use gpui::{
    anchored, deferred, div, prelude::*, AnyElement, App, ClickEvent, Context, ElementId, Pixels, Point,
    SharedString, Window,
};
use crate::ui::scale::px;

/// What the menu shows: its choices, the project list under "Move to project", or a confirmation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum MenuView {
    Actions,
    MoveTo,
    ConfirmDelete,
}

impl Shell {
    pub(super) fn open_menu(&mut self, target: Target, at: Point<Pixels>, cx: &mut Context<Self>) {
        self.sidebar.menu = Some(Menu { target, at, view: MenuView::Actions });
        cx.notify();
    }

    fn set_menu_view(&mut self, view: MenuView, cx: &mut Context<Self>) {
        if let Some(menu) = self.sidebar.menu.as_mut() {
            menu.view = view;
        }
        cx.notify();
    }

    /// The open menu as an overlay at its pointer position, or `None` when it is closed.
    pub(super) fn menu(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let menu = self.sidebar.menu.as_ref()?;
        let (target, at, view) = (menu.target.clone(), menu.at, menu.view);
        let items = match &target {
            Target::Session(id) => self.session_menu(id, view, cx),
            Target::Project(id) => self.project_menu(id, view, cx),
        };
        let theme = &self.theme;
        let panel = div()
            .id("sidebar-menu")
            .occlude()
            .w(px(220.))
            .p(px(4.))
            .flex()
            .flex_col()
            .gap(px(1.))
            .rounded(px(9.))
            .border_1()
            .border_color(theme.line2.to_hsla())
            .bg(theme.pop.to_hsla())
            .text_color(theme.fg.to_hsla())
            .font_family(theme.ui_font.clone())
            .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                this.sidebar.menu = None;
                cx.notify();
            }))
            .children(items);
        Some(deferred(anchored().position(at).snap_to_window_with_margin(px(8.)).child(panel)).with_priority(1).into_any_element())
    }

    fn session_menu(&mut self, id: &str, view: MenuView, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = self.theme.clone();
        let id = id.to_string();
        match view {
            MenuView::Actions => vec![
                item("menu-split", "Open in new split", &theme, cx.listener({
                    let id = id.clone();
                    move |this, _: &ClickEvent, window, cx| {
                        this.sidebar.menu = None;
                        this.open_in_split(&id, window);
                        this.save(cx);
                    }
                })),
                item("menu-rename", "Rename", &theme, cx.listener({
                    let id = id.clone();
                    move |this, _: &ClickEvent, window, cx| {
                        this.sidebar.menu = None;
                        this.start_rename(Target::Session(id.clone()), window, cx);
                    }
                })),
                item("menu-move", "Move to project  ▸", &theme, cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.set_menu_view(MenuView::MoveTo, cx);
                })),
                item("menu-share", "Share…", &theme, cx.listener({
                    let id = id.clone();
                    move |this, _: &ClickEvent, _, cx| {
                        this.sidebar.menu = None;
                        this.share_open_for(&id, cx);
                    }
                })),
                divider(&theme),
                danger("menu-delete", "Delete", &theme, cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.set_menu_view(MenuView::ConfirmDelete, cx);
                })),
            ],
            MenuView::MoveTo => {
                let current = self.state.session(&id).and_then(|s| s.project.clone());
                let mut rows = vec![item("menu-back", "‹ Back", &theme, cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.set_menu_view(MenuView::Actions, cx);
                }))];
                let picked = |project: Option<&str>| if current.as_deref() == project { "✓ " } else { "" };
                let none = format!("{}No project", picked(None));
                rows.push(item("menu-move-none", none, &theme, cx.listener({
                    let id = id.clone();
                    move |this, _: &ClickEvent, _, cx| this.move_session(&id, None, cx)
                })));
                for project in self.state.projects() {
                    let label = format!("{}{}", picked(Some(&project.id)), project.name);
                    let project_id = project.id.clone();
                    rows.push(item(ElementId::Name(format!("menu-move-{}", project.id).into()), label, &theme, cx.listener({
                        let id = id.clone();
                        move |this, _: &ClickEvent, _, cx| this.move_session(&id, Some(project_id.clone()), cx)
                    })));
                }
                rows
            }
            MenuView::ConfirmDelete => {
                let title = self.state.session(&id).map(|s| s.title.clone()).unwrap_or_default();
                confirm_rows(
                    format!("Delete “{title}”? Its messages are removed."),
                    &theme,
                    cx.listener({
                        let id = id.clone();
                        move |this, _: &ClickEvent, _, cx| {
                            this.sidebar.menu = None;
                            // A chat on another device goes when that device has deleted it.
                            if !this.delete_remote(&id, cx) {
                                this.state.delete_session(&id);
                                this.terminal_forget(&id);
                                this.share_deleted(&id);
                                this.persist_sidebar(cx);
                            }
                        }
                    }),
                    cx.listener(|this, _: &ClickEvent, _, cx| this.set_menu_view(MenuView::Actions, cx)),
                )
            }
        }
    }

    fn project_menu(&mut self, id: &str, view: MenuView, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = self.theme.clone();
        let id = id.to_string();
        match view {
            MenuView::Actions | MenuView::MoveTo => vec![
                item("menu-rename", "Rename", &theme, cx.listener({
                    let id = id.clone();
                    move |this, _: &ClickEvent, window, cx| {
                        this.sidebar.menu = None;
                        this.start_rename(Target::Project(id.clone()), window, cx);
                    }
                })),
                item("menu-folder", "Change folder", &theme, cx.listener({
                    let id = id.clone();
                    move |this, _: &ClickEvent, _, cx| {
                        this.sidebar.menu = None;
                        this.change_project_folder(id.clone(), cx);
                    }
                })),
                divider(&theme),
                danger("menu-delete", "Delete", &theme, cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.set_menu_view(MenuView::ConfirmDelete, cx);
                })),
            ],
            MenuView::ConfirmDelete => confirm_rows(
                "Delete this project? Its sessions stay and lose their project.".into(),
                &theme,
                cx.listener({
                    let id = id.clone();
                    move |this, _: &ClickEvent, _, cx| {
                        this.sidebar.menu = None;
                        this.delete_project(&id, cx);
                    }
                }),
                cx.listener(|this, _: &ClickEvent, _, cx| this.set_menu_view(MenuView::Actions, cx)),
            ),
        }
    }

    /// Puts the session in `project`, or in no project, and saves.
    fn move_session(&mut self, id: &str, project: Option<String>, cx: &mut Context<Self>) {
        self.sidebar.menu = None;
        self.state.move_to_project(id, project);
        self.persist_sidebar(cx);
    }
}

/// A menu row that runs `on_click`.
fn item(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    theme: &Theme,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let line2 = theme.line2.to_hsla();
    div()
        .id(id)
        .px(px(10.))
        .py(px(7.))
        .rounded(px(6.))
        .text_size(px(13.))
        .cursor_pointer()
        .hover(move |s| s.bg(line2))
        .on_click(on_click)
        .child(label.into())
        .into_any_element()
}

/// A menu row in the accent colour, for Delete.
fn danger(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    theme: &Theme,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let line2 = theme.line2.to_hsla();
    div()
        .id(id)
        .px(px(10.))
        .py(px(7.))
        .rounded(px(6.))
        .text_size(px(13.))
        .text_color(theme.acc.to_hsla())
        .cursor_pointer()
        .hover(move |s| s.bg(line2))
        .on_click(on_click)
        .child(label.into())
        .into_any_element()
}

/// The confirmation rows: the question, then Delete and Cancel.
fn confirm_rows(
    question: String,
    theme: &Theme,
    on_delete: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    on_cancel: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Vec<AnyElement> {
    vec![
        div()
            .px(px(10.))
            .py(px(8.))
            .text_size(px(12.5))
            .text_color(theme.muted.to_hsla())
            .child(question)
            .into_any_element(),
        danger("menu-confirm-delete", "Delete", theme, on_delete),
        item("menu-cancel", "Cancel", theme, on_cancel),
    ]
}

fn divider(theme: &Theme) -> AnyElement {
    div().h(px(1.)).my(px(3.)).bg(theme.line.to_hsla()).into_any_element()
}
