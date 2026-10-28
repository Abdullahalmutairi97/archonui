//! The Appearance page: theme, background, fonts, colours, layout, window and identity. Spec: settings.md §3.

use super::{
    colour::{token_colour, HSL_CHANNELS},
    controls::{caption, listen, section, segmented_row, toggle_row},
    FontSlot, Slide,
};
use crate::{
    app::{
        persist::copy_asset,
        settings::{theme_key, AppIcon, BackgroundFilter, GAPS, PANEL_BLUR, RADIUS, TEXT_SIZE, WALLPAPER_DIM},
    },
    ui::{
        anim,
        shell::Shell,
        theme::{self, presets, BackgroundStyle},
        widgets::{
            section::SettingRow,
            slider::{slider, SliderRange},
            text_input::{text_input, TextInput, TextInputConfig, TextInputEvent},
        },
    },
};
use gpui::{
    div, img, prelude::*, AnyElement, Context, ElementId, Image, ImageFormat, ObjectFit, PathPromptOptions, SharedString,
    Stateful, Styled,
};
use crate::ui::scale::px;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

/// The background styles in segment order, with the description the row shows for each.
const BACKGROUNDS: [(BackgroundStyle, &str, &str); 6] = [
    (BackgroundStyle::Canvas, "Canvas", "Wallpaper behind chats"),
    (BackgroundStyle::Glass, "Glass", "Frosted tiles"),
    (BackgroundStyle::Solid, "Solid", "No wallpaper"),
    (BackgroundStyle::Gaps, "Gaps", "Wallpaper in the gaps"),
    (BackgroundStyle::Tinted, "Tinted", "Dimmed, light glass"),
    (BackgroundStyle::Ambient, "Ambient", "Blurred and dark"),
];

/// The background filters in picker order.
const FILTERS: [(BackgroundFilter, &str); 4] = [
    (BackgroundFilter::Off, "Original"),
    (BackgroundFilter::Duotone, "Duotone"),
    (BackgroundFilter::DuotoneGrey, "Duotone B&W"),
    (BackgroundFilter::Tritone, "Tritone"),
];

/// The bundled families by kind, in the order the font list shows them. Display faces come last: they
/// suit titles more than running text.
const FONT_GROUPS: [(&str, &[&str]); 4] = [
    ("Sans", &["Geist", "IBM Plex Sans", "DM Sans", "Instrument Sans", "Manrope", "Space Grotesk"]),
    ("Serif", &["Young Serif", "Cormorant Garamond", "Bodoni Moda", "Gloock", "Zen Old Mincho", "Italiana", "Philosopher"]),
    ("Mono", &["JetBrains Mono", "IBM Plex Mono", "Geist Mono", "DM Mono", "Fira Code", "Space Mono"]),
    ("Display", &["Cinzel", "Bebas Neue", "Orbitron", "UnifrakturCook", "Pirata One", "Grenze Gotisch", "Metamorphous", "Pixelify Sans", "Silkscreen", "Press Start 2P"]),
];

/// Image files the backgrounds folder offers.
const IMAGE_EXTENSIONS: [&str; 5] = ["jpg", "jpeg", "png", "webp", "gif"];

/// A theme's background without an image, as a thumbnail: its three gradient colours.
fn gradient_thumb(stops: [crate::ui::theme::Rgba; 3]) -> gpui::Div {
    // GPUI gradients take two stops, so the middle colour fills the box under a blend of the ends.
    div().size_full().bg(stops[1].to_hsla()).child(div().size_full().opacity(0.7).bg(gpui::linear_gradient(
        135.,
        gpui::linear_color_stop(stops[0].to_hsla(), 0.),
        gpui::linear_color_stop(stops[2].to_hsla(), 1.),
    )))
}

/// The image format for a file name, from its extension. Unknown extensions decode as JPEG.
pub(super) fn image_format(name: &str) -> ImageFormat {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".png") {
        ImageFormat::Png
    } else if lower.ends_with(".webp") {
        ImageFormat::Webp
    } else if lower.ends_with(".gif") {
        ImageFormat::Gif
    } else {
        ImageFormat::Jpeg
    }
}

/// A slider range from a `(min, max)` settings bound.
fn range(bounds: (f32, f32), step: f32) -> SliderRange {
    SliderRange { min: bounds.0, max: bounds.1, step }
}

impl Shell {
    /// The thumbnail for a background image file, decoded once.
    pub(super) fn wall_thumb(&mut self, path: &Path) -> Option<Arc<Image>> {
        let name = path.file_name()?.to_string_lossy().into_owned();
        self.cached_image(format!("file:{}", path.display()), || {
            let bytes = std::fs::read(path).ok()?;
            Some(Arc::new(Image::from_bytes(image_format(&name), bytes)))
        })
    }

    /// The backgrounds folder in the config dir. Uploads are copied here, and every image in it is
    /// offered for any theme.
    fn backgrounds_dir(&self) -> PathBuf {
        self.paths.config.join("backgrounds")
    }

    /// The images in the backgrounds folder, in name order, then wallpapers added before the folder
    /// existed that are still on disk.
    fn backgrounds(&self) -> Vec<PathBuf> {
        let mut found: Vec<PathBuf> = std::fs::read_dir(self.backgrounds_dir())
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.extension().is_some_and(|e| IMAGE_EXTENSIONS.contains(&e.to_string_lossy().to_ascii_lowercase().as_str()))
            })
            .collect();
        found.sort();
        for old in &self.state.settings.user_wallpapers {
            if old.is_file() && !found.contains(old) {
                found.push(old.clone());
            }
        }
        found
    }

    /// The image for `key`, made by `load` the first time only.
    pub(super) fn cached_image(&mut self, key: String, load: impl FnOnce() -> Option<Arc<Image>>) -> Option<Arc<Image>> {
        if let Some(image) = self.settings.images.get(&key) {
            return Some(image.clone());
        }
        let image = load()?;
        self.settings.images.insert(key, image.clone());
        Some(image)
    }

    pub(super) fn appearance_page(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        vec![
            self.identity_block(cx),
            self.presets_block(cx),
            self.background_block(cx),
            self.typography_block(cx),
            self.colours_block(cx),
            self.layout_block(cx),
            self.window_block(cx),
        ]
    }

    /// The app's name, and whether its icon follows the theme or is an image of your own.
    fn identity_block(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = self.theme.clone();
        let weak = cx.entity().downgrade();
        let name = self.settings.name.as_ref().map(|(input, _)| input.clone());
        let name_row = SettingRow::new("App name", "", &theme).when_some(name, |row, input| {
            row.control(div().w(px(200.)).px(px(11.)).py(px(7.)).rounded(px(7.)).bg(theme.line.to_hsla()).child(text_input(&input, &theme)))
        });
        let custom = matches!(self.state.settings.icon, AppIcon::Custom(_) | AppIcon::Preset(_));
        let icon_row = segmented_row(
            "app-icon",
            "App icon",
            "",
            &["Theme icon", "Custom…"],
            usize::from(custom),
            &weak,
            &theme,
            |this, i, cx| {
                if i == 0 {
                    this.state.settings.icon = AppIcon::Theme;
                    this.refresh_theme(cx);
                } else {
                    this.pick_icon(cx);
                }
            },
            None,
        );
        let current_logo = self.theme.logo.clone();

        let mut tiles = div().flex().flex_wrap().gap(px(6.));
        let theme_logos: Vec<(String, String)> = presets().iter().map(|p| (p.id.to_string(), p.logo.to_string())).collect();
        let logo_files = image_files(&self.paths.config.join("logos"));
        let uploads = image_files(&self.icons_dir());
        for (i, tile) in icon_tiles(&theme_logos, &logo_files, &uploads).into_iter().enumerate() {
            let active = same_logo(&tile.logo, &current_logo);
            let logo_image = self.logo_image(&tile.logo);
            let group: SharedString = format!("icon-tile-{i}").into();
            let pick = tile.pick.clone();
            let mut el = div()
                .id(("icon", i))
                .group(group.clone())
                .relative()
                .w(px(36.))
                .h(px(36.))
                .rounded(px(8.))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .bg(if active { theme.soft.to_hsla() } else { theme.line.to_hsla() })
                .border_2()
                .border_color(if active { theme.acc.to_hsla() } else { gpui::transparent_black() })
                .hover(|s| s.bg(theme.line2.to_hsla()))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.state.settings.icon = pick.clone();
                    this.refresh_theme(cx);
                }))
                .when_some(logo_image, |d, src| {
                    d.child(img(src).w(gpui::relative(0.6)).h(gpui::relative(0.6)).object_fit(ObjectFit::Contain))
                });
            if let (true, AppIcon::Custom(path)) = (tile.removable, &tile.pick) {
                let path = path.clone();
                el = el.child(
                    div()
                        .id(("icon-remove", i))
                        .absolute()
                        .top(px(-5.))
                        .right(px(-5.))
                        .w(px(16.))
                        .h(px(16.))
                        .rounded(px(8.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .bg(theme.pop.to_hsla())
                        .border_1()
                        .border_color(theme.line2.to_hsla())
                        .text_size(px(10.))
                        .text_color(theme.fg.to_hsla())
                        .opacity(0.)
                        .group_hover(group, |s| s.opacity(1.))
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.remove_icon(&path, cx);
                        }))
                        .child("×"),
                );
            }
            tiles = tiles.child(el);
        }
        tiles = tiles.child(
            div()
                .id("icon-upload")
                .w(px(36.))
                .h(px(36.))
                .rounded(px(8.))
                .border_1()
                .border_dashed()
                .border_color(theme.line2.to_hsla())
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(18.))
                .text_color(theme.muted.to_hsla())
                .cursor_pointer()
                .hover(|s| s.text_color(theme.fg.to_hsla()))
                .on_click(cx.listener(|this, _, _, cx| this.pick_icon(cx)))
                .child("+"),
        );
        let tiles_row = SettingRow::new("Icons", "Any theme's logo, a file in the logos folder, or + to add one", &theme).below(tiles);
        section("Identity", vec![name_row, icon_row, tiles_row], &theme)
    }

    /// The live preview over the Appearance page: a small copy of the window, with the sidebar, two
    /// sessions and the message box, drawn with what the page sets now. `width` is the body's width.
    pub(super) fn appearance_preview(&mut self, width: f32) -> AnyElement {
        let t = self.settings.app_theme.clone().unwrap_or_else(|| self.theme.clone());
        let dialog = self.theme.clone();
        let s = self.state.settings.clone();
        let wall = self.preview_wallpaper();
        let edge = s.flush;
        let rad = if edge { 0. } else { s.radius };
        let gap = if edge { 0. } else { s.gaps };
        let bw = !edge && s.borders;
        let oc = s.window_opacity.clamp(0., 1.);
        let side_bg = t.bg.alpha(oc);
        let tile_bg = if s.tint_all { theme::mix(t.bg, t.acc, 0.10).alpha(oc) } else { t.bg.alpha(oc) };
        let projects: Vec<(String, String)> = self
            .state
            .projects()
            .iter()
            .filter(|p| !s.hidden_projects.contains(&p.id))
            .take(3)
            .map(|p| (p.name.clone(), super::general::tilde(&p.path)))
            .collect();
        let zoom = crate::ui::scale::zoom() * 0.5;
        let inner_w = width * 2.;
        let solid = s.background == BackgroundStyle::Solid;
        let app_name = s.app_name.clone();
        let msg = s.text_size;
        let session_ring = s.session_ring;
        let build = || {
            let line = t.line.to_hsla();
            let line2 = t.line2.to_hsla();
            let muted = t.muted.to_hsla();
            let card_border = |d: gpui::Div| if bw { d.border_1().border_color(line) } else { d };
            let mut side = card_border(
                div()
                    .w(px(270.))
                    .flex_none()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .px(px(14.))
                    .py(px(18.))
                    .rounded(px(rad))
                    .bg(side_bg.to_hsla()),
            )
            .child(
                div()
                    .font_family(t.display_font.clone())
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_size(px(26.))
                    .line_height(px(26.))
                    .pt(px(2.))
                    .px(px(6.))
                    .pb(px(12.))
                    .child(app_name.clone()),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .px(px(12.))
                    .py(px(10.))
                    .rounded(px(rad))
                    .bg(t.tile.to_hsla())
                    .border_1()
                    .border_color(line2)
                    .child(div().text_color(t.acc.to_hsla()).text_size(px(18.)).line_height(px(18.)).child("+"))
                    .child(div().flex_1().child("New session"))
                    .child(
                        div()
                            .px(px(6.))
                            .py(px(1.))
                            .rounded(px(5.))
                            .border_1()
                            .border_color(line2)
                            .font_family(t.code_font.clone())
                            .text_size(px(11.))
                            .text_color(muted)
                            .child("Ctrl N"),
                    ),
            )
            .child(div().text_size(px(12.)).text_color(muted).pt(px(14.)).px(px(6.)).pb(px(2.)).child("Projects"));
            for (name, path) in &projects {
                side = side.child(
                    card_border(div().flex().flex_col().gap(px(6.)).px(px(12.)).py(px(10.)).rounded(px(rad)).bg(t.tile.to_hsla()))
                        .child(div().child(name.clone()))
                        .child(div().text_size(px(12.)).text_color(muted).whitespace_nowrap().overflow_hidden().text_ellipsis().child(path.clone())),
                );
            }
            let tiles = [
                ("archon", "Claude Code · Sonnet 4.5", "Make the sidebar resizable", "Done. The sidebar now resizes between 220 and 520 px, and a double-click resets it to 300.", session_ring),
                ("website", "Codex · GPT-5 Codex", "Why is the build slow?", "Most of the time goes into image optimisation. Caching its output takes the build from 48 s to 9 s.", false),
            ];
            let mut row = div().absolute().top_0().left_0().size_full().flex().gap(px(gap)).p(px(gap)).child(side);
            for (name, meta, q, a, focused) in tiles {
                row = row.child(
                    card_border(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .flex()
                            .flex_col()
                            .rounded(px(rad))
                            .bg(tile_bg.to_hsla())
                            .overflow_hidden(),
                    )
                    .when(focused, |d| d.border(px(1.5)).border_color(t.acc.to_hsla()))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(12.))
                            .px(px(18.))
                            .py(px(14.))
                            .border_b_1()
                            .border_color(line)
                            .child(div().size(px(16.)).rounded(px(4.)).bg(t.acc2.to_hsla()))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(2.))
                                    .child(div().font_family(t.display_font.clone()).font_weight(gpui::FontWeight::SEMIBOLD).text_size(px(20.)).line_height(px(22.)).child(name))
                                    .child(div().text_size(px(12.)).text_color(muted).child(meta)),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_h(px(0.))
                            .flex()
                            .flex_col()
                            .gap(px(14.))
                            .px(px(20.))
                            .py(px(18.))
                            .text_size(px(msg))
                            .line_height(px(msg * 1.65))
                            .overflow_hidden()
                            .child(div().flex().justify_end().child(div().max_w(gpui::relative(0.8)).px(px(14.)).py(px(9.)).rounded(px(rad)).bg(line2).child(q)))
                            .child(div().child(a)),
                    )
                    .child(
                        div()
                            .mx(px(14.))
                            .mb(px(14.))
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .pl(px(14.))
                            .pr(px(10.))
                            .py(px(10.))
                            .rounded(px(rad))
                            .border_1()
                            .border_color(line2)
                            .bg(line)
                            .child(div().px(px(9.)).py(px(3.)).rounded(px(6.)).bg(line2).text_size(px(12.)).child("Auto"))
                            .child(div().flex_1().text_size(px(14.)).text_color(muted).child(format!("Ask {app_name}…")))
                            .child(div().size(px(28.)).rounded_full().bg(t.acc.to_hsla())),
                    ),
                );
            }
            let dim = s.wallpaper_dim.clamp(0., 100.) / 100.;
            div()
                .relative()
                .w(px(inner_w))
                .h(px(440.))
                .overflow_hidden()
                .font_family(t.ui_font.clone())
                .text_size(px(13.5))
                .text_color(t.fg.to_hsla())
                .bg(t.bg.to_hsla())
                .when(!solid, |d| {
                    d.child(match wall.clone() {
                        Some(wall) => img(wall).absolute().top_0().left_0().size_full().object_fit(ObjectFit::Cover).into_any_element(),
                        None => gradient_thumb(t.gradient).absolute().top_0().left_0().into_any_element(),
                    })
                })
                .child(div().absolute().top_0().left_0().size_full().bg(t.bg.alpha(dim).to_hsla()))
                .when(s.wallpaper_fade, |d| {
                    d.child(div().absolute().left_0().bottom_0().w_full().h(gpui::relative(0.45)).bg(gpui::linear_gradient(
                        180.,
                        gpui::linear_color_stop(t.bg.alpha(0.).to_hsla(), 0.),
                        gpui::linear_color_stop(t.bg.to_hsla(), 1.),
                    )))
                })
                .child(row)
        };
        let inner = crate::ui::scale::zoomed(zoom, crate::ui::scale::scoped(zoom, build));
        div()
            .flex_none()
            .px(px(super::BODY_PAD))
            .pt(px(16.))
            .pb(px(8.))
            .bg(dialog.panel.to_hsla())
            .child(
                div()
                    .relative()
                    .h(px(220.))
                    .rounded(px(12.))
                    .overflow_hidden()
                    .border_1()
                    .border_color(dialog.line2.to_hsla())
                    .bg(t.bg.to_hsla())
                    .shadow(vec![gpui::BoxShadow {
                        color: gpui::hsla(0., 0., 0., 0.35),
                        offset: gpui::point(px(0.), px(12.)),
                        blur_radius: px(32.),
                        spread_radius: px(0.),
                    }])
                    .child(inner),
            )
            .into_any_element()
    }

    /// The wallpaper as the preview shows it: a small copy through the chosen filter, made once.
    fn preview_wallpaper(&mut self) -> Option<Arc<Image>> {
        let small = small_wallpaper(&self.filtered_wallpaper(BackgroundFilter::Off))?;
        let source = self.with_filter(crate::ui::glass::WallpaperSource::File(small.clone()), self.state.settings.filter);
        self.cached_image(format!("preview:{}:{source:?}", small.display()), || {
            Some(Arc::new(Image::from_bytes(ImageFormat::Png, source.preview_png(640)?)))
        })
    }

    /// Opens a picker for an icon image, copies it into the data dir and makes it the app icon.
    fn pick_icon(&mut self, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions { files: true, directories: false, multiple: false, prompt: None });
        let dir = self.icons_dir();
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else { return };
            let Some(src) = paths.into_iter().next() else { return };
            let copied = cx.background_executor().spawn(async move { copy_asset(&dir, &src) }).await;
            let _ = this.update(cx, |this, cx| match copied {
                Ok(path) if crate::ui::glass::logo_image(&path.display().to_string(), 44, false).is_none() => {
                    let _ = std::fs::remove_file(&path);
                    this.toast("Icon not added: that file isn't an image ArchonUI can read".to_string(), cx);
                }
                Ok(path) => {
                    this.state.settings.icon = AppIcon::Custom(path);
                    this.refresh_theme(cx);
                }
                Err(e) => this.toast(format!("Icon not added: {e}"), cx),
            });
        })
        .detach();
    }

    /// Where uploaded icons are copied.
    fn icons_dir(&self) -> PathBuf {
        self.paths.data.join("icons")
    }

    /// Deletes an uploaded icon (ArchonUI's own copy in `icons/`); the app goes back to the theme's
    /// icon if it was the one in use.
    fn remove_icon(&mut self, path: &Path, cx: &mut Context<Self>) {
        if !path.starts_with(self.icons_dir()) {
            return;
        }
        if let Err(e) = std::fs::remove_file(path) {
            self.toast(format!("Icon not removed: {e}"), cx);
            return;
        }
        if self.state.settings.icon == AppIcon::Custom(path.to_path_buf()) {
            self.state.settings.icon = AppIcon::Theme;
        }
        self.refresh_theme(cx);
    }

    /// The theme cards. Picking one resets its fonts and wallpaper, as the prototype does; a theme of
    /// your own can be renamed and removed.
    fn presets_block(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = self.theme.clone();
        let current = self.state.settings.theme.clone();
        let cols = super::columns(self.settings.body_w, 150., 14., 12);
        let mut grid = div().grid().grid_cols(cols).gap_x(px(14.)).gap_y(px(16.));
        for (i, preset) in presets().iter().enumerate() {
            let id = preset.id.to_string();
            let active = id == current;
            let look = theme::resolve(preset, &Default::default(), Default::default());
            let bar = || div().h(px(4.)).rounded(px(2.)).bg(theme::mix(look.bg, look.fg, 0.22).to_hsla());
            let own = !theme::is_builtin(&id);
            let renaming = self.settings.theme_rename.as_ref().filter(|(r, _)| *r == id).map(|(_, (input, _))| input.clone());
            let thumb = ringed(div(), active, &theme)
                .relative()
                .h(px(84.))
                .rounded(px(9.))
                .overflow_hidden()
                .bg(look.bg.to_hsla())
                .child(div().absolute().top_0().bottom_0().left_0().w(gpui::relative(0.3)).bg(theme::mix(look.bg, look.fg, 0.07).to_hsla()))
                .child(
                    div()
                        .absolute()
                        .top(px(14.))
                        .left(gpui::relative(0.4))
                        .right(px(12.))
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .child(bar().w(gpui::relative(0.8)))
                        .child(bar().w(gpui::relative(0.55)))
                        .child(bar().w(gpui::relative(0.68))),
                )
                .child(div().absolute().bottom(px(10.)).right(px(12.)).w(px(30.)).h(px(10.)).rounded(px(5.)).bg(look.acc.to_hsla()))
                .when(own, |t| {
                    let (rid, did) = (id.clone(), id.clone());
                    t.child(
                        div()
                            .absolute()
                            .top(px(5.))
                            .right(px(5.))
                            .flex()
                            .gap(px(4.))
                            .child(corner_button(("theme-rename", i), super::controls::icon::EDIT, 20., 11., 0.65).on_click(cx.listener(
                                move |this, _: &gpui::ClickEvent, window, cx| {
                                    cx.stop_propagation();
                                    this.start_theme_rename(&rid, window, cx);
                                },
                            )))
                            .child(corner_button(("theme-remove", i), super::controls::icon::X, 20., 10., 0.65).on_click(cx.listener(
                                move |this, _: &gpui::ClickEvent, _, cx| {
                                    cx.stop_propagation();
                                    this.remove_theme(&did, cx);
                                },
                            ))),
                    )
                });
            let label = match renaming {
                Some(input) => div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(div().px(px(8.)).py(px(4.)).rounded(px(6.)).bg(theme.line.to_hsla()).border_1().border_color(theme.acc.to_hsla()).child(text_input(&input, &theme)))
                    .child(div().text_size(px(12.)).text_color(theme.muted.to_hsla()).child("Enter saves"))
                    .into_any_element(),
                None => div()
                    .px(px(2.))
                    .text_size(px(12.))
                    .text_color(if active { theme.fg.to_hsla() } else { theme.muted.to_hsla() })
                    .overflow_hidden()
                    .text_ellipsis()
                    .whitespace_nowrap()
                    .child(preset.name.to_string())
                    .into_any_element(),
            };
            grid = grid.child(
                div()
                    .id(("preset", i))
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .min_w(px(0.))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.state.settings.theme = id.clone();
                        // The open colour editor belongs to the theme it was opened on.
                        this.settings.picker = None;
                        this.state.settings.fonts.remove(theme_key(&id));
                        // A theme saved with "Save as theme" brings its whole look back.
                        if let Some(look) = theme::saved_appearance(&this.paths.config, &id) {
                            apply_appearance(&mut this.state.settings, &look);
                        }
                        this.refresh_theme(cx);
                    }))
                    .child(thumb)
                    .child(label),
            );
        }
        let save = super::controls::act_button("theme-save", "Save as theme", Some(super::controls::icon::PLUS), &theme, listen(cx, |this, cx| this.save_as_theme(cx)));
        div()
            .flex()
            .flex_col()
            .child(crate::ui::widgets::section::section_head("Theme", None, vec![save], &theme))
            .child(grid)
            .into_any_element()
    }

    /// The background style, the image, the filter, then the dim, blur and window opacity sliders.
    fn background_block(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = self.theme.clone();
        let weak = cx.entity().downgrade();
        let current = self.state.settings.background;
        let style_index = BACKGROUNDS.iter().position(|(s, _, _)| *s == current).unwrap_or(0);
        let style_names: Vec<&str> = BACKGROUNDS.iter().map(|(_, name, _)| *name).collect();
        let dim = self.state.settings.wallpaper_dim;
        let blur = self.state.settings.panel_blur;
        let opacity = self.state.settings.window_opacity;

        let style_row = segmented_row(
            "bg-style",
            "Style",
            "",
            &style_names,
            style_index,
            &weak,
            &theme,
            |this, i, cx| {
                this.state.settings.background = BACKGROUNDS[i].0;
                this.refresh_theme(cx);
            },
            None,
        );
        let images = self.wallpaper_grid(cx);
        let image_row = SettingRow::new("Image", "", &theme).below(images);
        let filters = self.filter_grid(cx);
        let filter_row = SettingRow::new("Filter", "", &theme).below(filters);
        let fade_row = super::controls::toggle_row(
            "bg-fade",
            "Fade at the bottom",
            "",
            self.state.settings.wallpaper_fade,
            &weak,
            &theme,
            |this, on, cx| {
                this.state.settings.wallpaper_fade = on;
                this.refresh_theme(cx);
            },
            None,
        );
        let mut rows = vec![style_row, image_row, filter_row, fade_row];
        let dim_slider = self.slide(Slide::Dim, dim, range(WALLPAPER_DIM, 5.), "%", cx);
        let blur_slider = self.slide(Slide::Blur, blur, range(PANEL_BLUR, 1.), "px", cx);
        let opacity_slider = self.slide(Slide::Opacity, opacity, range(crate::app::settings::WINDOW_OPACITY, 5.), "%", cx);
        rows.push(SettingRow::new("Wallpaper dim", "", &theme).control(dim_slider));
        rows.push(SettingRow::new("Panel blur", "", &theme).control(blur_slider));
        rows.push(
            SettingRow::new("Window opacity", "Text stays solid", &theme).control(opacity_slider),
        );
        section("Background", rows, &theme)
    }

    /// Removes a background. One in the backgrounds folder is removed on every device it is shared to;
    /// an older one kept elsewhere is only taken off the list. A picked one goes back to the theme's own.
    fn remove_background(&mut self, path: &Path, cx: &mut Context<Self>) {
        let dir = self.backgrounds_dir();
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let result = if path.parent() == Some(dir.as_path()) {
            crate::remote::assets::remove(&self.paths.config, "backgrounds", &name)
        } else {
            self.state.settings.user_wallpapers.retain(|p| p != path);
            Ok(())
        };
        if let Err(e) = result {
            self.toast(format!("Could not remove {name}: {e}"), cx);
            return;
        }
        if self.state.settings.wallpaper.as_deref() == Some(path) {
            self.state.settings.wallpaper = None;
        }
        self.settings.images.clear();
        self.refresh_theme(cx);
    }

    /// Removes a theme file, on every device it is shared to. If it was the theme in use, the default
    /// theme takes over.
    fn remove_theme(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(file) = theme::theme_file(&self.paths.config, id) else { return };
        let name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if let Err(e) = crate::remote::assets::remove(&self.paths.config, "themes", &name) {
            self.toast(format!("Could not remove the theme: {e}"), cx);
            return;
        }
        theme::reload_presets();
        if self.state.settings.theme == id {
            self.state.settings.theme = theme::DEFAULT_THEME.to_string();
        }
        self.settings.theme_rename = None;
        self.refresh_theme(cx);
    }

    /// Turns a custom theme's name into a field. Enter keeps the new name; leaving the field drops it.
    fn start_theme_rename(&mut self, id: &str, window: &mut gpui::Window, cx: &mut Context<Self>) {
        let current = presets().iter().find(|p| p.id == id).map(|p| p.name.to_string()).unwrap_or_default();
        let config = TextInputConfig { placeholder: "Theme name".into(), max_len: Some(40), font_size: px(12.), line_height: px(16.), ..Default::default() };
        let input = cx.new(|cx| TextInput::new(config, window, cx));
        input.update(cx, |input, cx| input.set_text(&current, cx));
        input.read(cx).focus(window);
        let theme_id = id.to_string();
        let subscription = cx.subscribe(&input, move |this, input, event: &TextInputEvent, cx| match event {
            TextInputEvent::Submit => {
                let name = input.read(cx).text().trim().to_string();
                this.settings.theme_rename = None;
                if !name.is_empty() {
                    this.rename_theme(&theme_id, &name, cx);
                }
                cx.notify();
            }
            TextInputEvent::Blur => {
                this.settings.theme_rename = None;
                cx.notify();
            }
            TextInputEvent::Changed => {}
        });
        self.settings.theme_rename = Some((id.to_string(), (input, subscription)));
        cx.notify();
    }

    /// Writes a new name into the theme's file; the change reaches the other devices like any edit.
    fn rename_theme(&mut self, id: &str, name: &str, cx: &mut Context<Self>) {
        let Some(path) = theme::theme_file(&self.paths.config, id) else { return };
        let renamed = std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).map_err(|e| e.to_string()))
            .and_then(|mut file| {
                file["name"] = serde_json::Value::from(name);
                let text = serde_json::to_string_pretty(&file).map_err(|e| e.to_string())?;
                std::fs::write(&path, text).map_err(|e| e.to_string())
            });
        match renamed {
            Ok(()) => {
                theme::reload_presets();
                self.refresh_theme(cx);
            }
            Err(e) => self.toast(format!("Could not rename the theme: {e}"), cx),
        }
    }

    /// Saves what the window looks like now as a new theme file, then switches to it: the colours on
    /// screen (edits included), wallpaper, logo, fonts, and the other appearance settings.
    fn save_as_theme(&mut self, cx: &mut Context<Self>) {
        let t = self.theme.clone();
        let dir = self.paths.config.join("themes");
        // The id is unique to this save, so themes saved on different devices never replace each other
        // when they are shared. The name counts the custom themes already here.
        let id = format!("custom-{:x}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis()));
        let n = presets().iter().filter(|p| p.id.starts_with("custom-")).count() + 1;
        let name = format!("Custom {n}");
        let wallpaper = match &self.state.settings.wallpaper {
            Some(path) if path.is_file() => path.display().to_string(),
            _ => t.wallpaper.clone(),
        };
        // Files in the shared folders are named by file name alone, so the theme finds them on every
        // device they are shared to; anything else keeps its full path.
        let wallpaper = in_shared(&self.paths.config, "backgrounds", &wallpaper);
        let logo = in_shared(&self.paths.config, "logos", &t.logo);
        let mut look = serde_json::Map::new();
        if let Ok(serde_json::Value::Object(all)) = serde_json::to_value(&self.state.settings) {
            for key in APPEARANCE_KEYS {
                if let Some(v) = all.get(key) {
                    look.insert(key.to_string(), v.clone());
                }
            }
        }
        // The file names the wallpaper itself, so picking the theme shows it rather than a separate pick.
        look.insert("wallpaper".into(), serde_json::Value::Null);
        let file = serde_json::json!({
            "id": id,
            "name": name,
            "colors": {
                "bg": css(t.bg), "fg": css(t.fg), "muted": css(t.muted), "faint": css(t.faint), "acc": css(t.acc),
                "acc2": css(t.acc2), "deep": css(t.deep), "alt": css(t.alt), "on": css(t.on), "soft": css(t.soft),
            },
            "tune": { "tile": css(t.tile), "panel": css(t.panel), "pop": css(t.pop) },
            "gradient": t.gradient.map(css),
            "wallpaper": wallpaper,
            "logo": logo,
            "pixelated": t.render == crate::ui::theme::Render::Pixelated,
            "fonts": [{ "label": "Saved", "display": t.display_font, "ui": t.ui_font }],
            "appearance": look,
        });
        let written = std::fs::create_dir_all(&dir).and_then(|()| {
            let text = serde_json::to_string_pretty(&file).map_err(std::io::Error::other)?;
            std::fs::write(dir.join(format!("{id}.json")), text)
        });
        match written {
            Ok(()) => {
                theme::reload_presets();
                self.state.settings.theme = id;
                self.settings.picker = None;
                self.refresh_theme(cx);
                self.toast(format!("Saved as {name}"), cx);
            }
            Err(e) => self.toast(format!("Could not save the theme: {e}"), cx),
        }
    }

    /// One tile per filter, each showing the current wallpaper through it. The previews are made from a
    /// small copy of the wallpaper, written once to the temp folder, so opening the page stays quick.
    fn filter_grid(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = self.theme.clone();
        let current = self.state.settings.filter;
        let base = self.filtered_wallpaper(BackgroundFilter::Off);
        let small = small_wallpaper(&base);
        let mut grid = div().flex().flex_wrap().gap(px(12.));
        for (i, (filter, name)) in FILTERS.into_iter().enumerate() {
            let thumb = small.as_ref().and_then(|small| {
                let source = self.with_filter(crate::ui::glass::WallpaperSource::File(small.clone()), filter);
                self.cached_image(format!("filter:{}:{source:?}", small.display()), || {
                    Some(Arc::new(Image::from_bytes(ImageFormat::Png, source.preview_png(208)?)))
                })
            });
            let picture = match thumb {
                Some(t) => img(t).size_full().object_fit(ObjectFit::Cover).into_any_element(),
                None => gradient_thumb(theme.gradient).into_any_element(),
            };
            grid = grid.child(
                pic_tile(("bg-filter", i), filter == current, Some(name), &theme, picture, None)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.state.settings.filter = filter;
                        this.refresh_theme(cx);
                    })),
            );
        }
        grid.into_any_element()
    }

    /// The image tiles: the theme's own background, every image in the backgrounds folder, and an
    /// upload tile.
    fn wallpaper_grid(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = self.settings.app_theme.clone().unwrap_or_else(|| self.theme.clone());
        let dialog = self.theme.clone();
        let picked = self.state.settings.wallpaper.clone().filter(|p| p.is_file());
        let backgrounds = self.backgrounds();
        let mut grid = div().flex().flex_wrap().gap(px(12.));
        // The theme's own background: its image, or its gradient.
        let own = Some(PathBuf::from(&theme.wallpaper)).filter(|p| p.is_file()).and_then(|p| self.wall_thumb(&p));
        let picture = match own {
            Some(thumb) => img(thumb).size_full().object_fit(ObjectFit::Cover).into_any_element(),
            None => gradient_thumb(theme.gradient).into_any_element(),
        };
        grid = grid.child(pic_tile("wall-theme", picked.is_none(), Some("Theme"), &dialog, picture, None).on_click(cx.listener(|this, _, _, cx| {
            this.state.settings.wallpaper = None;
            this.refresh_theme(cx);
        })));
        for (i, path) in backgrounds.into_iter().enumerate() {
            let Some(thumb) = self.wall_thumb(&path) else { continue };
            let active = picked.as_ref() == Some(&path);
            let gone = path.clone();
            let remove = corner_button(("wall-remove", i), super::controls::icon::X, 18., 10., 0.62).on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                cx.stop_propagation();
                this.remove_background(&gone, cx);
            }));
            let label = format!("Image {}", i + 1);
            grid = grid.child(
                pic_tile(("wall", i), active, Some(&label), &dialog, img(thumb).size_full().object_fit(ObjectFit::Cover).into_any_element(), Some(remove))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.state.settings.wallpaper = Some(path.clone());
                        this.refresh_theme(cx);
                    })),
            );
        }
        let muted = dialog.muted.to_hsla();
        grid = grid.child(
            div()
                .id("wall-upload")
                .w(px(104.))
                .h(px(66.))
                .rounded(px(8.))
                .border_1()
                .border_dashed()
                .border_color(dialog.line2.to_hsla())
                .flex()
                .items_center()
                .justify_center()
                .gap(px(6.))
                .text_size(px(12.))
                .text_color(muted)
                .cursor_pointer()
                .hover(|s| s.bg(dialog.line.to_hsla()))
                .on_click(cx.listener(|this, _, _, cx| this.pick_wall_file(cx)))
                .child(crate::ui::composer::icons::stroke_icon(super::controls::icon::PLUS, 14., 1.4, muted))
                .child("Upload"),
        );
        grid.into_any_element()
    }

    /// Copies an uploaded image into the backgrounds folder and makes it the background for every theme.
    fn pick_wall_file(&mut self, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions { files: true, directories: false, multiple: false, prompt: None });
        let dir = self.backgrounds_dir();
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else { return };
            let Some(src) = paths.into_iter().next() else { return };
            let copied = cx.background_executor().spawn(async move { copy_asset(&dir, &src) }).await;
            let _ = this.update(cx, |this, cx| match copied {
                Ok(path) => {
                    this.state.settings.wallpaper = Some(path);
                    this.refresh_theme(cx);
                }
                Err(e) => this.toast(format!("Wallpaper not added: {e}"), cx),
            });
        })
        .detach();
    }

    /// The Titles or Text slot, then a chip per font.
    fn typography_block(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = self.theme.clone();
        let key = theme_key(&self.state.settings.theme);
        let choice = self.state.settings.fonts.get(key).cloned().unwrap_or_default();
        let preset = theme::preset(&self.state.settings.theme);
        let pairing = preset.pairings[choice.pairing % 3];
        let titles = choice.display.clone().unwrap_or_else(|| pairing.display.to_string());
        let text = choice.ui.clone().unwrap_or_else(|| pairing.ui.to_string());
        let open = self.settings.font_open;
        let rows = [
            (FontSlot::Titles, "Titles font", "", titles.clone()),
            (FontSlot::Text, "Text font", "", text.clone()),
        ]
        .map(|(slot, name, desc, family)| {
            let is_open = open == Some(slot);
            let current = div()
                .flex()
                .items_center()
                .gap(px(10.))
                .child(div().px(px(6.)).text_size(px(22.)).line_height(px(22.)).font_family(family.clone()).child("Aa"))
                .child(div().text_size(px(12.)).text_color(theme.muted.to_hsla()).child(family.clone()))
                .child(
                    div()
                        .id(SharedString::from(format!("font-change-{name}")))
                        .px(px(12.))
                        .py(px(6.))
                        .rounded(px(7.))
                        .border_1()
                        .border_color(if is_open { theme.acc.to_hsla() } else { theme.line2.to_hsla() })
                        .text_size(px(12.))
                        .cursor_pointer()
                        .hover(|s| s.bg(theme.line.to_hsla()))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.settings.font_open = if this.settings.font_open == Some(slot) { None } else { Some(slot) };
                            cx.notify();
                        }))
                        .child(if is_open { "Done" } else { "Change" }),
                );
            SettingRow::new(name, desc, &theme).control(current)
        });
        let [titles_row, text_row] = rows;

        let list = open.map(|slot| {
            let in_use = match slot {
                FontSlot::Titles => titles.clone(),
                FontSlot::Text => text.clone(),
            };
            let mut groups = div().flex().flex_col().gap(px(12.));
            for (group, families) in FONT_GROUPS {
                let mut grid = div().grid().grid_cols(super::columns(self.settings.body_w, 110., 6., 5)).gap(px(6.));
                for family in families.iter().copied() {
                    let active = in_use == family;
                    grid = grid.child(
                        div()
                            .id(SharedString::from(format!("font-{family}")))
                            .h(px(54.))
                            .px(px(10.))
                            .flex()
                            .flex_col()
                            .justify_center()
                            .gap(px(2.))
                            .rounded(px(7.))
                            .border_1()
                            .border_color(if active { theme.acc.to_hsla() } else { theme.line.to_hsla() })
                            .when(active, |d| d.bg(theme.soft.to_hsla()))
                            .cursor_pointer()
                            .hover(|s| s.bg(theme.line.to_hsla()))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let key = theme_key(&this.state.settings.theme).to_string();
                                let entry = this.state.settings.fonts.entry(key).or_default();
                                match slot {
                                    FontSlot::Titles => entry.display = Some(family.to_string()),
                                    FontSlot::Text => entry.ui = Some(family.to_string()),
                                }
                                this.refresh_theme(cx);
                            }))
                            // Every sample is the same size and clipped to one line, so no face takes over.
                            .child(div().h(px(22.)).overflow_hidden().whitespace_nowrap().text_size(px(17.)).line_height(px(22.)).font_family(family).child("Aa Bb"))
                            .child(div().text_size(px(10.5)).text_color(theme.muted.to_hsla()).truncate().child(family)),
                    );
                }
                groups = groups.child(div().flex().flex_col().gap(px(6.)).child(caption(group, &theme)).child(grid));
            }
            groups
        });

        div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(section("Fonts", vec![titles_row, text_row], &theme))
            .when_some(list, |d, list| d.child(div().pt(px(6.)).child(list)))
            .into_any_element()
    }

    /// Every colour of the theme as a swatch in its group; a click opens its editor under the grid.
    fn colours_block(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = self.theme.clone();
        let live = self.settings.app_theme.clone().unwrap_or_else(|| self.theme.clone());
        let mut groups = div().grid().grid_cols(super::columns(self.settings.body_w - 36., 200., 28., 4)).gap_x(px(28.)).gap_y(px(20.));
        for (group, tokens) in SWATCHES {
            let mut col = div().flex().flex_col().gap(px(10.)).child(div().text_size(px(12.)).text_color(theme.muted.to_hsla()).child(group));
            for (token, label) in tokens.iter().copied() {
                let Some(colour) = token_colour(&live, token) else { continue };
                let open = self.settings.picker.as_ref().is_some_and(|p| p.token == token);
                col = col.child(
                    div()
                        .id(SharedString::from(format!("colour-{token}")))
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _, window, cx| {
                            if this.settings.picker.as_ref().is_some_and(|p| p.token == token) {
                                this.settings.picker = None;
                                cx.notify();
                            } else {
                                this.open_picker(token, window, cx);
                            }
                        }))
                        .child(
                            div()
                                .flex_none()
                                .size(px(22.))
                                .rounded(px(6.))
                                .bg(colour.to_hsla())
                                .border_1()
                                .border_color(if open { theme.acc.to_hsla() } else { theme.line2.to_hsla() }),
                        )
                        .child(div().flex_1().child(label))
                        .child(div().font_family(theme.code_font.clone()).text_size(px(12.)).text_color(theme.muted.to_hsla()).child(super::colour::hex(colour))),
                );
            }
            groups = groups.child(col);
        }
        let (radius, border) = crate::ui::widgets::section::card_style();
        let editor = self.settings.picker.as_ref().map(|p| {
            let label = SWATCHES.iter().flat_map(|(_, t)| t.iter()).find(|(t, _)| *t == p.token).map_or(p.token, |(_, l)| *l);
            let current = token_colour(&live, p.token).unwrap_or(live.fg);
            let channels = HSL_CHANNELS.iter().zip(p.hsl.iter()).map(|((name, _, _), channel)| {
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(div().w(px(120.)).text_size(px(12.)).text_color(theme.muted.to_hsla()).child(name.to_string()))
                    .child(slider(channel, &theme))
            });
            div()
                .flex()
                .flex_col()
                .gap(px(10.))
                .mt(px(4.))
                .pt(px(16.))
                .border_t_1()
                .border_color(theme.line.to_hsla())
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(12.))
                        .child(div().w(px(120.)).child(label.to_string()))
                        .child(div().w(px(110.)).px(px(10.)).py(px(6.)).rounded(px(7.)).bg(theme.line.to_hsla()).child(text_input(&p.input, &theme)))
                        .child(div().size(px(30.)).rounded(px(7.)).border_1().border_color(theme.line2.to_hsla()).bg(current.to_hsla()))
                        .child(div().text_size(px(12.)).text_color(theme.muted.to_hsla()).child("Enter closes")),
                )
                .children(channels)
        });
        let reset = super::controls::act_button("colours-reset", "Reset", None, &theme, listen(cx, |this, cx| {
            let key = theme_key(&this.state.settings.theme);
            this.state.settings.custom_colours.remove(key);
            this.settings.picker = None;
            this.refresh_theme(cx);
        }));
        div()
            .flex()
            .flex_col()
            .child(crate::ui::widgets::section::section_head("Custom colours", None, vec![reset], &theme))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(16.))
                    .p(px(18.))
                    .rounded(px(radius))
                    .bg(theme.tile.to_hsla())
                    .when(border, |d| d.border_1().border_color(theme.line.to_hsla()))
                    .child(groups)
                    .when_some(editor, |d, editor| d.child(editor)),
            )
            .into_any_element()
    }

    /// Tiling mode, gap and radius sliders, and the reset for dragged sizes.
    fn layout_block(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = self.theme.clone();
        let weak = cx.entity().downgrade();
        let gaps = self.state.settings.gaps;
        let radius = self.state.settings.radius;
        let flush = self.state.settings.flush;
        let gaps_slider = self.slide(Slide::Gaps, gaps, range(GAPS, 1.), "px", cx);
        let radius_slider = self.slide(Slide::Radius, radius, range(RADIUS, 1.), "px", cx);
        let rows = vec![
            toggle_row(
                "flush",
                "Edge to edge",
                "No gaps, corners or borders",
                flush,
                &weak,
                &theme,
                |this, on, cx| {
                    this.state.settings.flush = on;
                    this.save(cx);
                },
                None,
            ),
            SettingRow::new("Gaps", "", &theme).control(gaps_slider),
            SettingRow::new("Corner radius", "", &theme).control(radius_slider),
        ];
        section("Layout", rows, &theme)
    }

    /// Interface and text size, borders, session highlight, tint and reduce motion.
    fn window_block(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = self.theme.clone();
        let weak = cx.entity().downgrade();
        let text = self.state.settings.text_size;
        let tint = self.state.settings.tint_all;
        let ring = self.state.settings.session_ring;
        let borders = self.state.settings.borders;
        let reduce_motion = self.state.settings.reduce_motion.unwrap_or_else(anim::reduced_motion);
        let text_slider = self.slide(Slide::Text, text, range(TEXT_SIZE, 1.), "px", cx);
        let scale = (self.state.settings.ui_scale * 100.).round();
        let scale_range = (crate::app::settings::UI_SCALE.0 * 100., crate::app::settings::UI_SCALE.1 * 100.);
        let scale_slider = self.slide(Slide::Scale, scale, range(scale_range, 5.), "%", cx);
        let rows = vec![
            SettingRow::new("Interface size", "", &theme).control(scale_slider),
            SettingRow::new("Message text size", "", &theme).control(text_slider),
            toggle_row(
                "borders",
                "Borders",
                "",
                borders,
                &weak,
                &theme,
                |this, on, cx| {
                    this.state.settings.borders = on;
                    this.save(cx);
                },
                None,
            ),
            toggle_row(
                "session-ring",
                "Session highlight",
                "Ring around the focused session",
                ring,
                &weak,
                &theme,
                |this, on, cx| {
                    this.state.settings.session_ring = on;
                    this.save(cx);
                },
                None,
            ),
            toggle_row(
                "tint-all",
                "Tint all sessions",
                "",
                tint,
                &weak,
                &theme,
                |this, on, cx| {
                    this.state.settings.tint_all = on;
                    this.save(cx);
                },
                None,
            ),
            toggle_row(
                "reduce-motion",
                "Reduce motion",
                "",
                reduce_motion,
                &weak,
                &theme,
                |this, on, cx| {
                    this.state.settings.reduce_motion = Some(on);
                    anim::set_reduced_motion(Some(on));
                    this.save(cx);
                },
                None,
            ),
        ];
        section("Window", rows, &theme)
    }
}


/// The settings a saved theme brings back with it, as `AppSettings` names them.
const APPEARANCE_KEYS: [&str; 14] = [
    "background", "filter", "filter_pixel", "wallpaper_fade", "gaps", "radius", "panel_blur", "wallpaper_dim", "window_opacity",
    "tint_all", "session_ring", "borders", "flush", "text_size",
];

/// Sets the appearance settings `look` names; anything else in it, and a value that doesn't fit, is ignored.
fn apply_appearance(settings: &mut crate::app::settings::AppSettings, look: &serde_json::Value) {
    let (Ok(serde_json::Value::Object(mut all)), Some(look)) = (serde_json::to_value(&*settings), look.as_object()) else { return };
    for key in APPEARANCE_KEYS.iter().copied().chain(["wallpaper"]) {
        if let Some(v) = look.get(key) {
            all.insert(key.to_string(), v.clone());
        }
    }
    if let Ok(applied) = serde_json::from_value(serde_json::Value::Object(all)) {
        *settings = applied;
    }
}

/// `path` as a theme file names it: just the file name when it is in `config/<dir>` (a shared
/// folder), else unchanged.
fn in_shared(config: &Path, dir: &str, path: &str) -> String {
    let shared = config.join(dir);
    match Path::new(path).strip_prefix(&shared) {
        Ok(name) if name.components().count() == 1 => name.display().to_string(),
        _ => path.to_string(),
    }
}

/// A colour as a theme file writes it: `#rrggbb`, or `rgba(...)` when it is see-through.
fn css(c: crate::ui::theme::Rgba) -> String {
    if c.a >= 1.0 {
        format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b)
    } else {
        format!("rgba({},{},{},{:.2})", c.r, c.g, c.b, c.a)
    }
}

/// A small dark button for a tile's corner, such as × to remove it: `size` square, with a white icon.
fn corner_button(id: impl Into<ElementId>, glyph: &'static str, size: f32, icon: f32, alpha: f32) -> Stateful<gpui::Div> {
    div()
        .id(id)
        .size(px(size))
        .rounded(px(5.))
        .flex()
        .items_center()
        .justify_center()
        .bg(gpui::hsla(0., 0., 0., alpha))
        .cursor_pointer()
        .hover(|s| s.bg(gpui::hsla(0., 0., 0., 0.85)))
        .child(crate::ui::composer::icons::stroke_icon(glyph, icon, 1.4, gpui::white()))
}

/// The ring around a card: the accent when picked, a hairline otherwise.
fn ringed<E: Styled>(el: E, active: bool, theme: &crate::ui::theme::Theme) -> E {
    let (colour, width) = if active { (theme.acc, 1.5) } else { (theme.line2, 1.) };
    el.border(px(width)).border_color(colour.to_hsla())
}

/// A 104 × 66 picture tile with its name under it, as the image and filter rows show.
fn pic_tile(
    id: impl Into<ElementId>,
    active: bool,
    name: Option<&str>,
    theme: &crate::ui::theme::Theme,
    picture: AnyElement,
    corner: Option<Stateful<gpui::Div>>,
) -> Stateful<gpui::Div> {
    div()
        .id(id)
        .flex()
        .flex_col()
        .gap(px(6.))
        .w(px(104.))
        .cursor_pointer()
        .child(
            ringed(div(), active, theme)
                .relative()
                .h(px(66.))
                .rounded(px(8.))
                .overflow_hidden()
                .child(div().absolute().top_0().left_0().size_full().child(picture))
                .when_some(corner, |d, b| d.child(div().absolute().top(px(4.)).right(px(4.)).child(b))),
        )
        .when_some(name, |d, name| {
            d.child(
                div()
                    .text_size(px(12.))
                    .text_color(if active { theme.fg.to_hsla() } else { theme.muted.to_hsla() })
                    .child(name.to_string()),
            )
        })
}

/// The colour swatches by group, with the names the page shows. Every one opens the colour editor.
const SWATCHES: [(&str, &[(&str, &str)]); 4] = [
    ("Surfaces", &[("bg", "Background"), ("tile", "Tile"), ("panel", "Panel"), ("pop", "Popover")]),
    ("Text", &[("fg", "Text"), ("muted", "Muted"), ("faint", "Faint"), ("on", "Text on accent")]),
    ("Accents", &[("acc", "Accent"), ("acc2", "Accent 2"), ("alt", "Secondary"), ("deep", "Deep accent")]),
    ("Lines and highlights", &[("line", "Line"), ("line2", "Line 2"), ("soft", "Selection"), ("ring1", "Highlight start"), ("ring2", "Highlight end")]),
];

/// A 320 px copy of `source` in the temp folder, named by what it shows, made once. `None` if the
/// wallpaper can't be read.
fn small_wallpaper(source: &crate::ui::glass::WallpaperSource) -> Option<PathBuf> {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    source.hash(&mut hasher);
    if let crate::ui::glass::WallpaperSource::File(path) = source {
        std::fs::metadata(path).and_then(|m| m.modified()).ok().hash(&mut hasher);
    }
    let path = std::env::temp_dir().join(format!("archonui-wallpaper-{:016x}.png", hasher.finish()));
    if !path.is_file() {
        std::fs::write(&path, source.preview_png(320)?).ok()?;
    }
    Some(path)
}

/// One tile in Settings → Appearance → Identity: the logo it draws and the icon picking it sets.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct IconTile {
    pub logo: String,
    pub pick: AppIcon,
    /// An upload ArchonUI copied into its data dir, which the tile can delete.
    pub removable: bool,
}

/// The icon tiles: one per distinct theme logo, then every image in the config `logos` folder that no
/// theme uses, then the uploaded icons. Before, logo files without a theme and earlier uploads had no
/// tile, so once another icon was picked they could never be picked again.
pub(super) fn icon_tiles(themes: &[(String, String)], logo_files: &[PathBuf], uploads: &[PathBuf]) -> Vec<IconTile> {
    let mut tiles: Vec<IconTile> = Vec::new();
    for (id, logo) in themes {
        if !tiles.iter().any(|t| t.logo == *logo) {
            tiles.push(IconTile { logo: logo.clone(), pick: AppIcon::Preset(id.clone()), removable: false });
        }
    }
    for (files, removable) in [(logo_files, false), (uploads, true)] {
        for path in files {
            let logo = path.display().to_string();
            if !tiles.iter().any(|t| t.logo == logo) {
                tiles.push(IconTile { logo, pick: AppIcon::Custom(path.clone()), removable });
            }
        }
    }
    tiles
}

/// Whether two logo keys draw the same logo, ignoring the mark's tint (`archonui-mark.png#rrggbb`).
pub(super) fn same_logo(a: &str, b: &str) -> bool {
    a.split('#').next() == b.split('#').next()
}

/// The image files in `dir`, by name.
fn image_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .filter(|p| {
            p.extension().and_then(|e| e.to_str()).is_some_and(|e| {
                matches!(e.to_ascii_lowercase().as_str(), "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp")
            })
        })
        .collect();
    files.sort();
    files
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_saved_theme_names_shared_files_by_name() {
        let config = Path::new("/home/me/.config/archonui");
        assert_eq!(in_shared(config, "backgrounds", "/home/me/.config/archonui/backgrounds/sea.jpg"), "sea.jpg");
        assert_eq!(in_shared(config, "logos", "/home/me/.config/archonui/logos/mark.png"), "mark.png");
        assert_eq!(in_shared(config, "backgrounds", "/srv/pics/sea.jpg"), "/srv/pics/sea.jpg");
        assert_eq!(in_shared(config, "logos", "archonui-mark.png"), "archonui-mark.png", "an embedded logo stays as it is");
    }

    use super::*;

    #[test]
    fn a_saved_theme_brings_its_look_back() {
        use crate::{app::settings::{AppSettings, BackgroundFilter}, ui::theme::BackgroundStyle};
        let config = std::env::temp_dir().join(format!("archonui-theme-save-{}", crate::store::new_id()));
        std::fs::create_dir_all(config.join("themes")).unwrap();
        let soft = crate::ui::theme::Rgba::new(10, 20, 30, 0.25);
        assert_eq!(css(soft), "rgba(10,20,30,0.25)");
        assert_eq!(css(crate::ui::theme::Rgba::hex(0x0a141e)), "#0a141e");
        let file = serde_json::json!({
            "id": "custom-1", "name": "Custom 1",
            "colors": { "bg": "#101010", "fg": "#f0f0f0", "acc": "#c81e2a", "soft": css(soft) },
            "tune": { "tile": "#181818" },
            "appearance": { "background": "glass", "filter": "warm", "gaps": 12.0, "wallpaper": null, "app_name": "nope" },
        });
        std::fs::write(config.join("themes/custom-1.json"), file.to_string()).unwrap();
        let themes = crate::ui::theme::user_themes(&config);
        assert_eq!(themes.len(), 1);
        assert_eq!(themes[0].name, "Custom 1");

        let look = crate::ui::theme::saved_appearance(&config, "custom-1").expect("the appearance block");
        let mut settings = AppSettings { wallpaper: Some("/old.png".into()), app_name: "mine".into(), ..Default::default() };
        apply_appearance(&mut settings, &look);
        assert_eq!(settings.background, BackgroundStyle::Glass);
        assert_eq!(settings.filter, BackgroundFilter::Warm);
        assert_eq!(settings.gaps, 12.0);
        assert_eq!(settings.wallpaper, None, "the theme's own wallpaper shows");
        assert_eq!(settings.app_name, "mine", "only appearance settings change");
        assert!(crate::ui::theme::saved_appearance(&config, "other").is_none());
        std::fs::remove_dir_all(config).ok();
    }

    #[test]
    fn every_logo_and_upload_gets_one_tile() {
        let themes = [
            ("archonui".to_string(), "archonui-mark.png".to_string()),
            ("dracula".to_string(), "archonui-mark.png".to_string()),
            ("blood-moon".to_string(), "/cfg/logos/logo-bm.png".to_string()),
        ];
        let files = [PathBuf::from("/cfg/logos/logo-bm.png"), PathBuf::from("/cfg/logos/logo-cat.png")];
        let uploads = [PathBuf::from("/data/icons/mine.png")];
        let tiles = icon_tiles(&themes, &files, &uploads);
        let logos: Vec<&str> = tiles.iter().map(|t| t.logo.as_str()).collect();
        assert_eq!(logos, ["archonui-mark.png", "/cfg/logos/logo-bm.png", "/cfg/logos/logo-cat.png", "/data/icons/mine.png"]);
        assert_eq!(tiles[1].pick, AppIcon::Preset("blood-moon".into()), "a theme's logo picks that theme's icon");
        assert_eq!(tiles[2].pick, AppIcon::Custom("/cfg/logos/logo-cat.png".into()), "a logo no theme uses is still offered");
        assert!(!tiles[2].removable, "files in the logos folder are the user's; only uploads can be removed");
        assert!(tiles[3].removable);
    }

    #[test]
    fn the_tinted_mark_is_the_same_logo() {
        assert!(same_logo("archonui-mark.png", "archonui-mark.png#8fb8e8"));
        assert!(same_logo("archonui-mark.png#0969da#1f2328", "archonui-mark.png"));
        assert!(!same_logo("archonui-mark.png", "/cfg/logos/logo-bm.png"));
    }

    #[test]
    fn only_images_are_listed() {
        let dir = std::env::temp_dir().join(format!("archonui-icons-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for name in ["b.PNG", "a.jpg", "notes.txt"] {
            std::fs::write(dir.join(name), b"x").unwrap();
        }
        let names: Vec<String> = image_files(&dir).iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect();
        assert_eq!(names, ["a.jpg", "b.PNG"]);
        std::fs::remove_dir_all(dir).unwrap();
        assert!(image_files(Path::new("/no/such/dir")).is_empty());
    }
}
