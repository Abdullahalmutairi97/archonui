//! The Appearance page: identity, presets, background, colours, typography, layout and effects. Spec: settings.md §3.

use super::{
    colour::{hex, token_colour, HSL_CHANNELS},
    controls::{button_row, caption, heading, section, segmented_row, toggle_row},
    Slide,
};
use crate::{
    app::{
        persist::copy_asset,
        settings::{theme_key, AppIcon, GAPS, PANEL_BLUR, RADIUS, TEXT_SIZE, WALLPAPER_DIM},
    },
    ui::{
        anim,
        fonts::{self, FontChoice},
        glass::wallpaper_bytes,
        shell::Shell,
        theme::{BackgroundStyle, ThemeId, EDITABLE_TOKENS, PRESETS},
        tiling::LayoutMode,
        widgets::{
            button::ButtonStyle,
            section::SettingRow,
            slider::{slider, SliderRange},
            text_input::text_input,
        },
    },
};
use gpui::{
    div, img, prelude::*, AnyElement, Context, ElementId, Image, ImageFormat, ObjectFit, PathPromptOptions, SharedString,
    Stateful, Styled,
};
use crate::ui::scale::px;
use std::{path::PathBuf, sync::Arc};

/// The six background styles in card order, with the description each card shows.
const BACKGROUNDS: [(BackgroundStyle, &str, &str); 6] = [
    (BackgroundStyle::Canvas, "Canvas", "Wallpaper behind chats"),
    (BackgroundStyle::Glass, "Glass", "Frosted tiles"),
    (BackgroundStyle::Solid, "Solid", "No wallpaper"),
    (BackgroundStyle::Gaps, "Gaps", "Wallpaper in the gaps"),
    (BackgroundStyle::Tinted, "Tinted", "Dimmed, light glass"),
    (BackgroundStyle::Ambient, "Ambient", "Blurred and dark"),
];

/// The minimal font pairs' labels, in the order the Typography page shows them.
/// The presets with their own wallpaper and logo; the rest reuse theirs.
const ORIGINAL_PRESETS: &[crate::ui::theme::Preset] = PRESETS.split_at(12).0;

/// Captions over the colour cards with how many of `EDITABLE_TOKENS`, in order, each holds.
const COLOUR_GROUPS: [(&str, usize); 4] = [("Surfaces", 4), ("Text", 4), ("Accents", 4), ("Lines and highlights", 5)];

/// The file name a theme wallpaper path ends in, e.g. `bloodmoon2.jpg` for `fhd/bloodmoon2.jpg`.
fn wall_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
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

/// Sets the width-to-height ratio of a box. GPUI's `Styled` has no method for it, so this sets the style field.
trait AspectRatio: Styled + Sized {
    fn aspect_ratio(mut self, ratio: f32) -> Self {
        self.style().aspect_ratio = Some(ratio);
        self
    }
}
impl<T: Styled> AspectRatio for T {}

/// The display name of a theme preset, as the Appearance cards show it.
fn theme_name(id: ThemeId) -> &'static str {
    match id {
        ThemeId::ArchonUI => "ArchonUI",
        ThemeId::BloodMoon => "Blood Moon",
        ThemeId::Moonlight => "Moonlight",
        ThemeId::Wreck => "Wreck",
        ThemeId::Citadel => "Citadel",
        ThemeId::Neon => "Neon",
        ThemeId::Violet => "Violet",
        ThemeId::Ink => "Ink",
        ThemeId::Bonfire => "Bonfire",
        ThemeId::Hunter => "Hunter",
        ThemeId::Shards => "Shards",
        ThemeId::Vale => "Vale",
        ThemeId::Codex => "Codex",
        ThemeId::ClaudeCode => "Claude Code",
        ThemeId::Gemini => "Gemini",
        ThemeId::Hermes => "Hermes",
    }
}

/// A slider range from a `(min, max)` settings bound.
fn range(bounds: (f32, f32), step: f32) -> SliderRange {
    SliderRange { min: bounds.0, max: bounds.1, step }
}

/// A card's outline and fill for the active and inactive states.
fn card(id: impl Into<ElementId>, active: bool, theme: &crate::ui::theme::Theme) -> Stateful<gpui::Div> {
    div()
        .id(id)
        .flex()
        .flex_col()
        .gap(px(8.))
        .p(px(8.))
        .rounded(px(10.))
        .border_1()
        .cursor_pointer()
        .border_color(if active { theme.acc.to_hsla() } else { theme.line.to_hsla() })
        .when(active, |d| d.bg(theme.soft.to_hsla()))
        .hover(|s| s.bg(theme.line.to_hsla()))
}

impl Shell {
    /// The wallpaper thumbnail for a built-in wall, decoded once.
    pub(super) fn wall_thumb(&mut self, name: &str) -> Option<Arc<Image>> {
        let bytes = wallpaper_bytes(name)?;
        self.cached_image(format!("wall:{name}"), || Some(Arc::new(Image::from_bytes(image_format(name), bytes.to_vec()))))
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

    /// The wallpaper in effect for the current theme: the user's pick, else the theme's own.
    fn current_wall(&self) -> String {
        let s = &self.state.settings;
        match s.wallpapers.get(theme_key(s.theme)) {
            Some(path) => path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            None => wall_name(s.theme.preset().wallpaper).to_string(),
        }
    }

    pub(super) fn appearance_page(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        vec![
            self.identity_block(cx),
            self.presets_block(cx),
            self.background_block(cx),
            self.colours_block(cx),
            self.typography_block(cx),
            self.layout_block(cx),
            self.effects_block(cx),
        ]
    }

    /// Name and icon, with a live sidebar preview.
    fn identity_block(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = self.theme.clone();
        let name = self.settings.name.as_ref().map(|(input, _)| input.clone());
        let app_name = self.state.settings.app_name.clone();
        let logo = self.icon_logo();
        let reset_op = if matches!(self.state.settings.icon, AppIcon::Theme) { 0.4 } else { 1.0 };
        let current = self.state.settings.icon.clone();
        let theme_choice = self.state.settings.theme;

        let mut grid = div().grid().grid_cols(7).gap(px(8.));
        // The look-alike presets reuse the first twelve's logos, so only those twelve are offered; the
        // one showing a look-alike's logo is marked when that look-alike is chosen.
        for preset in ORIGINAL_PRESETS.iter() {
            let active = match &current {
                AppIcon::Preset(id) => id.preset().logo == preset.logo,
                AppIcon::Theme => theme_choice.preset().logo == preset.logo,
                AppIcon::Custom(_) => false,
            };
            let logo_image = self.logo_image(preset.logo);
            let id = preset.id;
            let tile = div()
                .id(("icon", id as usize))
                .aspect_ratio(1.)
                .rounded(px(8.))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .bg(if active { theme.soft.to_hsla() } else { theme.line.to_hsla() })
                .border_2()
                .border_color(if active { theme.acc.to_hsla() } else { gpui::transparent_black() })
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.state.settings.icon = AppIcon::Preset(id);
                    this.save(cx);
                }))
                .when_some(logo_image, |d, src| {
                    d.child(img(src).w(gpui::relative(0.58)).h(gpui::relative(0.58)).object_fit(ObjectFit::Contain))
                });
            grid = grid.child(tile);
        }
        if let AppIcon::Custom(_) = &current {
            if let Some(src) = logo.clone() {
                grid = grid.child(
                    div()
                        .aspect_ratio(1.)
                        .rounded(px(8.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .bg(theme.soft.to_hsla())
                        .border_2()
                        .border_color(theme.acc.to_hsla())
                        .child(img(src).w(gpui::relative(0.58)).h(gpui::relative(0.58)).object_fit(ObjectFit::Contain)),
                );
            }
        }
        grid = grid.child(
            div()
                .id("icon-upload")
                .aspect_ratio(1.)
                .rounded(px(8.))
                .border_1()
                .border_dashed()
                .border_color(theme.line2.to_hsla())
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(20.))
                .text_color(theme.muted.to_hsla())
                .cursor_pointer()
                .hover(|s| s.text_color(theme.fg.to_hsla()))
                .on_click(cx.listener(|this, _, _, cx| this.pick_icon(cx)))
                .child("+"),
        );

        let left = card_plain(&theme)
            .child(caption("Name", &theme))
            .when_some(name, |d, input| {
                d.child(div().px(px(12.)).py(px(8.)).rounded(px(8.)).bg(theme.line.to_hsla()).child(text_input(&input, &theme).family(theme.display_font.clone())))
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .px(px(12.))
                    .py(px(10.))
                    .rounded(px(8.))
                    .border_1()
                    .border_dashed()
                    .border_color(theme.line2.to_hsla())
                    .when_some(self.icon_logo(), |d, logo| {
                        d.child(img(logo).w(px(22.)).h(px(22.)).object_fit(ObjectFit::Contain))
                    })
                    .child(div().font_family(theme.display_font.clone()).text_size(px(22.)).child(app_name))
                    .child(div().flex_1())
                    .child(div().text_size(px(11.)).text_color(theme.muted.to_hsla()).child("Sidebar preview")),
            );

        let right = card_plain(&theme)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(div().text_size(px(12.)).text_color(theme.muted.to_hsla()).child("Icon"))
                    .child(
                        div()
                            .id("icon-reset")
                            .px(px(10.))
                            .py(px(5.))
                            .rounded(px(6.))
                            .border_1()
                            .border_color(theme.line2.to_hsla())
                            .text_size(px(12.))
                            .opacity(reset_op)
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.state.settings.icon = AppIcon::Theme;
                                this.save(cx);
                            }))
                            .child("Use theme icon"),
                    ),
            )
            .child(grid);

        div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .pt(px(14.))
            .child(heading("IDENTITY", None, &theme))
            .child(div().grid().grid_cols(5).gap(px(14.)).child(div().col_span(2).child(left)).child(div().col_span(3).child(right)))
            .into_any_element()
    }

    /// Opens a picker for an icon image, copies it into the data dir and makes it the app icon.
    fn pick_icon(&mut self, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions { files: true, directories: false, multiple: false, prompt: None });
        let dir = self.paths.data.join("icons");
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else { return };
            let Some(src) = paths.into_iter().next() else { return };
            let copied = cx.background_executor().spawn(async move { copy_asset(&dir, &src) }).await;
            let _ = this.update(cx, |this, cx| match copied {
                Ok(path) => {
                    this.state.settings.icon = AppIcon::Custom(path);
                    this.save(cx);
                }
                Err(e) => this.toast(format!("Icon not added: {e}"), cx),
            });
        })
        .detach();
    }

    /// The preset cards. Picking one resets its fonts and wallpaper, as the prototype does.
    fn presets_block(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = self.theme.clone();
        let current = self.state.settings.theme;
        let mut grid = div().grid().grid_cols(4).gap(px(14.));
        for preset in PRESETS.iter() {
            let id = preset.id;
            let active = id == current;
            let edited = self.state.settings.custom_colours.get(theme_key(id)).is_some_and(|t| !t.is_empty());
            let tag = match (active, edited) {
                (true, true) => "Active · edited",
                (true, false) => "Active",
                (false, true) => "Edited",
                (false, false) => "",
            };
            let wall = self.wall_thumb(wall_name(preset.wallpaper));
            let swatches = [preset.bg, preset.fg, preset.muted, preset.acc, preset.acc2, preset.alt];
            let thumb = div()
                .relative()
                .h(px(132.))
                .rounded(px(6.))
                .overflow_hidden()
                .bg(preset.bg.to_hsla())
                .when_some(wall, |d, wall| d.child(img(wall).absolute().size_full().object_fit(ObjectFit::Cover)))
                .child(
                    div()
                        .absolute()
                        .left(px(8.))
                        .right(px(8.))
                        .bottom(px(8.))
                        .flex()
                        .gap(px(4.))
                        .p(px(5.))
                        .rounded(px(6.))
                        .bg(gpui::hsla(0., 0., 0., 0.5))
                        .children(swatches.map(|c| div().flex_1().h(px(10.)).rounded(px(3.)).bg(c.to_hsla()))),
                );
            grid = grid.child(
                card(("preset", id as usize), active, &theme)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.state.settings.theme = id;
                        // The open colour editor belongs to the theme it was opened on.
                        this.settings.picker = None;
                        this.state.settings.fonts.remove(theme_key(id));
                        this.state.settings.wallpapers.remove(theme_key(id));
                        this.refresh_theme(cx);
                    }))
                    .child(thumb)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(8.))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .font_family(preset.pairings[0].display)
                                    .text_size(px(19.))
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .whitespace_nowrap()
                                    .child(theme_name(preset.id)),
                            )
                            .child(div().flex_none().text_size(px(11.)).text_color(theme.acc2.to_hsla()).child(tag)),
                    ),
            );
        }
        div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(heading("PRESETS", Some("Picking a preset also resets fonts and wallpaper."), &theme))
            .child(grid)
            .into_any_element()
    }

    /// Six background styles with a live preview, then the wallpaper picker.
    fn background_block(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = self.theme.clone();
        let current = self.state.settings.background;
        let wall = self.wall_thumb(&self.current_wall());
        let mut styles = div().grid().grid_cols(6).gap(px(10.));
        for (i, (style, name, desc)) in BACKGROUNDS.iter().copied().enumerate() {
            let active = style == current;
            let (wall_alpha, side_alpha, tile_alpha) = match style {
                BackgroundStyle::Canvas => (1.0, 0.72, 0.6),
                BackgroundStyle::Glass => (1.0, 0.60, 0.55),
                BackgroundStyle::Solid => (0.0, 1.0, 1.0),
                BackgroundStyle::Gaps => (1.0, 0.96, 1.0),
                BackgroundStyle::Tinted => (0.5, 0.55, 0.35),
                BackgroundStyle::Ambient => (0.55, 0.50, 0.45),
            };
            let mut preview = div().relative().h(px(74.)).rounded(px(6.)).overflow_hidden().bg(theme.bg.to_hsla());
            if let Some(wall) = wall.clone().filter(|_| wall_alpha > 0.) {
                preview = preview.child(img(wall).absolute().inset_0().size_full().object_fit(ObjectFit::Cover).opacity(wall_alpha));
            }
            preview = preview
                .child(div().absolute().left_0().top_0().bottom_0().w(gpui::relative(0.18)).bg(theme.panel.alpha(side_alpha).to_hsla()))
                .child(
                    div()
                        .absolute()
                        .left(gpui::relative(0.24))
                        .top(px(8.))
                        .bottom(px(8.))
                        .w(gpui::relative(0.34))
                        .rounded(px(3.))
                        .bg(theme.tile.alpha(tile_alpha).to_hsla()),
                )
                .child(
                    div()
                        .absolute()
                        .left(gpui::relative(0.62))
                        .top(px(8.))
                        .bottom(px(8.))
                        .right(px(8.))
                        .rounded(px(3.))
                        .bg(theme.tile.alpha(tile_alpha).to_hsla()),
                );
            styles = styles.child(
                card(("bg-style", i), active, &theme)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.state.settings.background = style;
                        this.refresh_theme(cx);
                    }))
                    .child(preview)
                    .child(div().text_size(px(13.)).child(name))
                    .child(div().text_size(px(11.)).text_color(theme.muted.to_hsla()).child(desc)),
            );
        }

        let walls = self.wallpaper_grid(cx);
        div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(heading("BACKGROUND", None, &theme))
            .child(styles)
            .child(caption("Wallpaper", &theme))
            .child(walls)
            .into_any_element()
    }

    /// The wallpaper grid: the twelve theme walls, any uploaded walls, and an upload tile.
    fn wallpaper_grid(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = self.theme.clone();
        let current = self.current_wall();
        let mut grid = div().grid().grid_cols(7).gap(px(10.));
        let mut names: Vec<(String, Option<PathBuf>)> = ORIGINAL_PRESETS
            .iter()
            .map(|p| (wall_name(p.wallpaper).to_string(), None))
            .collect();
        for path in self.state.settings.user_wallpapers.clone() {
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            names.push((name, Some(path)));
        }
        for (i, (name, user)) in names.into_iter().enumerate() {
            let active = name == current;
            let Some(thumb) = (match &user {
                Some(path) => self.cached_image(format!("file:{}", path.display()), || {
                    let bytes = std::fs::read(path).ok()?;
                    Some(Arc::new(Image::from_bytes(image_format(&name), bytes)))
                }),
                None => self.wall_thumb(&name),
            }) else {
                continue;
            };
            let pick_name = name.clone();
            let pick_user = user.clone();
            let cell = div()
                .id(("wall", i))
                .h(px(80.))
                .rounded(px(6.))
                .overflow_hidden()
                .cursor_pointer()
                .border_2()
                .border_color(if active { theme.acc.to_hsla() } else { gpui::transparent_black() })
                .on_click(cx.listener(move |this, _, _, cx| {
                    let own = own_for(this);
                    this.pick_wall(&pick_name, pick_user.clone(), &own, cx);
                }))
                .child(img(thumb).size_full().object_fit(ObjectFit::Cover));
            grid = grid.child(cell);
        }
        grid = grid.child(
            div()
                .id("wall-upload")
                .h(px(80.))
                .rounded(px(6.))
                .border_1()
                .border_dashed()
                .border_color(theme.line2.to_hsla())
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(12.))
                .text_color(theme.muted.to_hsla())
                .cursor_pointer()
                .hover(|s| s.text_color(theme.fg.to_hsla()))
                .on_click(cx.listener(|this, _, _, cx| this.pick_wall_file(cx)))
                .child("+ Upload"),
        );
        grid.into_any_element()
    }

    /// Makes `name` the wallpaper for the current theme. A built-in wall is copied into the data dir
    /// first, since the shell reads wallpapers from files. The theme's own wall clears the override.
    fn pick_wall(&mut self, name: &str, user: Option<PathBuf>, own: &str, cx: &mut Context<Self>) {
        let key = theme_key(self.state.settings.theme).to_string();
        if let Some(path) = user {
            self.state.settings.wallpapers.insert(key, path);
            self.refresh_theme(cx);
            return;
        }
        if name == own {
            self.state.settings.wallpapers.remove(&key);
            self.refresh_theme(cx);
            return;
        }
        let Some(bytes) = wallpaper_bytes(name) else { return };
        let dir = self.paths.data.join("wallpapers");
        let path = dir.join(name);
        if let Err(e) = std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&path, bytes)) {
            self.toast(format!("Wallpaper not set: {e}"), cx);
            return;
        }
        self.state.settings.wallpapers.insert(key, path);
        self.refresh_theme(cx);
    }

    /// Adds an uploaded wallpaper to the data dir and makes it this theme's wall.
    fn pick_wall_file(&mut self, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions { files: true, directories: false, multiple: false, prompt: None });
        let dir = self.paths.data.join("wallpapers");
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else { return };
            let Some(src) = paths.into_iter().next() else { return };
            let copied = cx.background_executor().spawn(async move { copy_asset(&dir, &src) }).await;
            let _ = this.update(cx, |this, cx| match copied {
                Ok(path) => {
                    let key = theme_key(this.state.settings.theme).to_string();
                    this.state.settings.user_wallpapers.push(path.clone());
                    this.state.settings.wallpapers.insert(key, path);
                    this.refresh_theme(cx);
                }
                Err(e) => this.toast(format!("Wallpaper not added: {e}"), cx),
            });
        })
        .detach();
    }

    /// The twelve token overrides for the current theme. A cell opens the hex editor under the grid.
    fn colours_block(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = self.theme.clone();
        let key = theme_key(self.state.settings.theme);
        let overrides = self.state.settings.custom_colours.get(key).cloned().unwrap_or_default();
        let has_overrides = !overrides.is_empty();
        let reset_op = if has_overrides { 1.0 } else { 0.4 };
        let name = theme_name(self.state.settings.theme).to_uppercase();

        let mut groups: Vec<(&str, gpui::Div)> =
            COLOUR_GROUPS.iter().map(|(name, _)| (*name, div().grid().grid_cols(4).gap(px(10.)))).collect();
        for (n, (token, label)) in EDITABLE_TOKENS.iter().copied().enumerate() {
            let Some(colour) = token_colour(&theme, token) else { continue };
            let edited = overrides.contains_key(token);
            let open = self.settings.picker.as_ref().is_some_and(|p| p.token == token);
            let static_token: &'static str = token;
            let group = COLOUR_GROUPS.iter().scan(0, |end, (_, len)| { *end += len; Some(*end) }).position(|end| n < end).unwrap_or(0);
            let grid = &mut groups[group].1;
            *grid = std::mem::replace(grid, div()).child(
                div()
                    .id(SharedString::from(format!("colour-{token}")))
                    .relative()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .p(px(10.))
                    .rounded(px(9.))
                    .border_1()
                    .border_color(if open { theme.acc.to_hsla() } else { theme.line2.to_hsla() })
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.line.to_hsla()))
                    .on_click(cx.listener(move |this, _, window, cx| this.open_picker(static_token, window, cx)))
                    .child(
                        div()
                            .w(px(34.))
                            .h(px(34.))
                            .rounded(px(7.))
                            .bg(colour.to_hsla())
                            .border_1()
                            .border_color(theme.line2.to_hsla()),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(div().text_size(px(13.)).child(label))
                            .child(div().text_size(px(11.)).text_color(theme.muted.to_hsla()).child(hex(colour).to_uppercase())),
                    )
                    .when(edited, |d| d.child(div().text_size(px(10.5)).text_color(theme.acc2.to_hsla()).child("edited"))),
            );
        }

        let editor = self.settings.picker.as_ref().map(|p| {
            let label = EDITABLE_TOKENS.iter().find(|(t, _)| *t == p.token).map_or(p.token, |(_, l)| *l);
            let current = token_colour(&theme, p.token).unwrap_or(theme.fg);
            let channels = HSL_CHANNELS.iter().zip(p.hsl.iter()).map(|((name, _, _), channel)| {
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(div().w(px(180.)).text_size(px(12.)).text_color(theme.muted.to_hsla()).child(name.to_string()))
                    .child(slider(channel, &theme))
            });
            div()
                .flex()
                .flex_col()
                .gap(px(10.))
                .p(px(12.))
                .rounded(px(9.))
                .border_1()
                .border_color(theme.acc.to_hsla())
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(12.))
                        .child(div().w(px(180.)).text_size(px(13.)).child(label.to_string()))
                        .child(div().w(px(120.)).px(px(10.)).py(px(6.)).rounded(px(8.)).bg(theme.line.to_hsla()).child(text_input(&p.input, &theme)))
                        .child(div().size(px(34.)).rounded(px(7.)).border_1().border_color(theme.line2.to_hsla()).bg(current.to_hsla()))
                        .child(div().text_size(px(11.)).text_color(theme.muted.to_hsla()).child("Enter to close")),
                )
                .children(channels)
        });

        let reset = div()
            .id("colour-reset")
            .px(px(12.))
            .py(px(6.))
            .rounded(px(6.))
            .border_1()
            .border_color(theme.line2.to_hsla())
            .text_size(px(12.))
            .opacity(reset_op)
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, _, cx| {
                let key = theme_key(this.state.settings.theme);
                this.state.settings.custom_colours.remove(key);
                this.settings.picker = None;
                this.refresh_theme(cx);
            }))
            .child("Reset to preset");

        div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(heading(&format!("COLORS · {name}"), None, &theme))
                    .child(reset),
            )
            .children(groups.into_iter().map(|(name, grid)| div().flex().flex_col().gap(px(6.)).child(caption(name, &theme)).child(grid)))
            .when_some(editor, |d, editor| d.child(editor))
            .into_any_element()
    }

    /// The theme's pairings, then every font with a Titles and a Text button.
    fn typography_block(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = self.theme.clone();
        let key = theme_key(self.state.settings.theme);
        let choice = self.state.settings.fonts.get(key).cloned().unwrap_or_default();
        let preset = self.state.settings.theme.preset();
        let name_text = self.state.settings.app_name.clone();

        let mut pairings = div().grid().grid_cols(3).gap(px(10.));
        for (i, pair) in preset.pairings.iter().enumerate() {
            let active = choice.pairing == i && choice.display.is_none() && choice.ui.is_none();
            pairings = pairings.child(
                card(("pairing", i), active, &theme)
                    .p(px(14.))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let key = theme_key(this.state.settings.theme).to_string();
                        this.state.settings.fonts.insert(key, FontChoice { pairing: i, display: None, ui: None });
                        this.refresh_theme(cx);
                    }))
                    .child(div().font_family(pair.display).text_size(px(30.)).line_height(px(30.)).child(name_text.clone()))
                    .child(
                        div()
                            .text_size(px(11.5))
                            .text_color(theme.muted.to_hsla())
                            .child(format!("{} · {}", pair.label, pair.ui)),
                    ),
            );
        }

        let pairing = preset.pairings[choice.pairing % 3];
        let titles = choice.display.clone().unwrap_or_else(|| pairing.display.to_string());
        let text = choice.ui.clone().unwrap_or_else(|| pairing.ui.to_string());
        let mut all = div().grid().grid_cols(5).gap(px(8.));
        for (i, (label, family)) in fonts::all_fonts().into_iter().enumerate() {
            let (is_titles, is_text) = (titles == family, text == family);
            all = all.child(
                card_plain(&theme)
                    .p(px(10.))
                    .px(px(12.))
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .when(is_titles || is_text, |d| d.border_color(theme.acc.to_hsla()))
                    .child(div().font_family(family).text_size(px(22.)).line_height(px(26.)).child("Aa Bb"))
                    .child(div().text_size(px(10.5)).text_color(theme.muted.to_hsla()).child(label))
                    .child(
                        div()
                            .flex()
                            .gap(px(4.))
                            .child(slot_button(("font-titles", i), "Titles", is_titles, &theme).on_click(cx.listener(
                                move |this, _, _, cx| {
                                    let key = theme_key(this.state.settings.theme).to_string();
                                    this.state.settings.fonts.entry(key).or_default().display = Some(family.to_string());
                                    this.refresh_theme(cx);
                                },
                            )))
                            .child(slot_button(("font-text", i), "Text", is_text, &theme).on_click(cx.listener(
                                move |this, _, _, cx| {
                                    let key = theme_key(this.state.settings.theme).to_string();
                                    this.state.settings.fonts.entry(key).or_default().ui = Some(family.to_string());
                                    this.refresh_theme(cx);
                                },
                            ))),
                    ),
            );
        }

        div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(heading("TYPOGRAPHY", None, &theme))
            .child(caption("Theme pairings", &theme))
            .child(pairings)
            .child(caption(&format!("All fonts · Titles: {titles} · Text (chat and message box): {text}"), &theme))
            .child(all)
            .into_any_element()
    }

    /// Tiling mode, gap and radius sliders, and the reset for dragged sizes.
    fn layout_block(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = self.theme.clone();
        let weak = cx.entity().downgrade();
        let master = matches!(self.state.workspace.layout.mode, LayoutMode::Master);
        let gaps = self.state.settings.gaps;
        let radius = self.state.settings.radius;
        let flush = self.state.settings.flush;
        let gaps_slider = self.slide(Slide::Gaps, gaps, range(GAPS, 1.), "px", cx);
        let radius_slider = self.slide(Slide::Radius, radius, range(RADIUS, 1.), "px", cx);
        let rows = vec![
            segmented_row(
                "tiling",
                "Tiling",
                "Dwindle splits the focused tile. Master keeps one large tile on the left.",
                &["Dwindle", "Master"],
                usize::from(master),
                &weak,
                &theme,
                |this, i, cx| {
                    this.state.workspace.layout.mode = if i == 1 { LayoutMode::Master } else { LayoutMode::Dwindle };
                    this.save(cx);
                },
                None,
            ),
            toggle_row(
                "flush",
                "Edge to edge",
                "No gaps anywhere, square corners and no borders: sessions fill the window. Gaps, corner radius and Borders keep their values for when this is off.",
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
            button_row(
                "reset-sizes",
                "Tile and panel sizes",
                "Drag the gaps between tiles, or the tools panel edge, to resize. Double-click a gap to even it out.",
                "Reset sizes",
                ButtonStyle::Outline,
                &weak,
                &theme,
                |this, cx| {
                    this.state.workspace.layout.ratios.clear();
                    this.state.workspace.layout.master_ratio = 0.56;
                    this.state.workspace.right_width = 480.;
                    this.widths_changed(false, cx);
                },
            ),
        ];
        section("LAYOUT", rows, &theme)
    }

    /// Panel blur, wallpaper dim, tint and message size.
    fn effects_block(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = self.theme.clone();
        let weak = cx.entity().downgrade();
        let blur = self.state.settings.panel_blur;
        let dim = self.state.settings.wallpaper_dim;
        let text = self.state.settings.text_size;
        let tint = self.state.settings.tint_all;
        let ring = self.state.settings.session_ring;
        let borders = self.state.settings.borders;
        let reduce_motion = self.state.settings.reduce_motion.unwrap_or_else(anim::reduced_motion);
        let blur_slider = self.slide(Slide::Blur, blur, range(PANEL_BLUR, 1.), "px", cx);
        let dim_slider = self.slide(Slide::Dim, dim, range(WALLPAPER_DIM, 5.), "%", cx);
        let text_slider = self.slide(Slide::Text, text, range(TEXT_SIZE, 1.), "px", cx);
        let scale = (self.state.settings.ui_scale * 100.).round();
        let scale_range = (crate::app::settings::UI_SCALE.0 * 100., crate::app::settings::UI_SCALE.1 * 100.);
        let scale_slider = self.slide(Slide::Scale, scale, range(scale_range, 5.), "%", cx);
        let rows = vec![
            SettingRow::new("Interface size", "Scales the whole window, like zooming a page.", &theme).control(scale_slider),
            SettingRow::new("Panel blur", "Blur behind the sidebars and glass tiles.", &theme).control(blur_slider),
            SettingRow::new("Wallpaper dim", "", &theme).control(dim_slider),
            toggle_row(
                "tint-all",
                "Tint all sessions",
                "Give unfocused sessions the same solid tint as the focused one.",
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
                "borders",
                "Borders",
                "Lines around sessions and along the tools panel's edge. Off, sessions have no border.",
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
                "Draw the coloured ring around the focused session. Off, it gets the same plain border as the others.",
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
                "reduce-motion",
                "Reduce motion",
                "Collapse the sidebar and panel and reflow tiles instantly.",
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
            SettingRow::new("Message text size", "", &theme).control(text_slider),
        ];
        section("EFFECTS", rows, &theme)
    }
}

/// A small toggle on a font card that puts the font in one slot; filled when it is in that slot.
fn slot_button(id: impl Into<ElementId>, label: &'static str, active: bool, theme: &crate::ui::theme::Theme) -> Stateful<gpui::Div> {
    div()
        .id(id)
        .flex_1()
        .py(px(3.))
        .rounded(px(5.))
        .border_1()
        .border_color(if active { theme.acc.to_hsla() } else { theme.line2.to_hsla() })
        .when(active, |d| d.bg(theme.acc.to_hsla()).text_color(theme.on.to_hsla()))
        .text_size(px(10.5))
        .text_center()
        .cursor_pointer()
        .hover(|s| s.bg(theme.line.to_hsla()))
        .child(label)
}

/// The plain bordered card used by Identity.
fn card_plain(theme: &crate::ui::theme::Theme) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .gap(px(12.))
        .p(px(16.))
        .rounded(px(10.))
        .border_1()
        .border_color(theme.line2.to_hsla())
}

/// The theme's own wall name, for the handler that compares picks against it.
fn own_for(this: &Shell) -> String {
    wall_name(this.state.settings.theme.preset().wallpaper).to_string()
}
