//! The Appearance page: theme, background, fonts, colours, layout, window and identity. Spec: settings.md §3.

use super::{
    colour::{token_colour, HSL_CHANNELS},
    controls::{caption, heading, listen, section, segmented_row, toggle_row},
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
        theme::{self, presets, BackgroundStyle, EDITABLE_TOKENS},
        widgets::{
            button::{Button, ButtonStyle},
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

/// Captions over the colour swatches with how many of `EDITABLE_TOKENS`, in order, each holds.
const COLOUR_GROUPS: [(&str, usize); 4] = [("Surfaces", 4), ("Text", 4), ("Accents", 4), ("Lines and highlights", 5)];

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

/// A card's outline and fill for the active and inactive states.
fn card(id: impl Into<ElementId>, active: bool, theme: &crate::ui::theme::Theme) -> Stateful<gpui::Div> {
    div()
        .id(id)
        .flex()
        .flex_col()
        .gap(px(6.))
        .p(px(6.))
        .rounded(px(8.))
        .border_1()
        .cursor_pointer()
        .border_color(if active { theme.acc.to_hsla() } else { theme.line.to_hsla() })
        .when(active, |d| d.bg(theme.soft.to_hsla()))
        .hover(|s| s.bg(theme.line.to_hsla()))
}

/// A small tag in a font chip: `T` for Titles, `Aa` for Text.
fn badge(text: &'static str, theme: &crate::ui::theme::Theme) -> gpui::Div {
    div()
        .px(px(4.))
        .rounded(px(3.))
        .bg(theme.line.to_hsla())
        .text_size(px(9.5))
        .text_color(theme.muted.to_hsla())
        .child(text)
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
            self.presets_block(cx),
            self.background_block(cx),
            self.typography_block(cx),
            self.colours_block(cx),
            self.layout_block(cx),
            self.window_block(cx),
            self.identity_block(cx),
        ]
    }

    /// Name and icon: the name field and a sidebar preview on the left, the icon tiles on the right.
    fn identity_block(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = self.theme.clone();
        let name = self.settings.name.as_ref().map(|(input, _)| input.clone());
        let app_name = self.state.settings.app_name.clone();
        let logo = self.icon_logo();
        let reset_op = if matches!(self.state.settings.icon, AppIcon::Theme) { 0.4 } else { 1.0 };
        let current = self.state.settings.icon.clone();
        let theme_logo = self.theme.logo.clone();

        let mut tiles = div().flex().flex_wrap().justify_end().gap(px(6.)).max_w(px(300.));
        // One tile per distinct logo; themes that share a logo share its tile.
        let mut seen = std::collections::HashSet::new();
        let logos: Vec<_> = presets().iter().filter(|p| seen.insert(p.logo.clone())).collect();
        for (i, preset) in logos.into_iter().enumerate() {
            let active = match &current {
                AppIcon::Preset(id) => theme::preset(id).logo == preset.logo,
                AppIcon::Theme => theme_logo == preset.logo,
                AppIcon::Custom(_) => false,
            };
            let logo_image = self.logo_image(&preset.logo.clone());
            let id = preset.id.to_string();
            let tile = div()
                .id(("icon", i))
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
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.state.settings.icon = AppIcon::Preset(id.clone());
                    this.save(cx);
                }))
                .when_some(logo_image, |d, src| {
                    d.child(img(src).w(gpui::relative(0.6)).h(gpui::relative(0.6)).object_fit(ObjectFit::Contain))
                });
            tiles = tiles.child(tile);
        }
        if let AppIcon::Custom(_) = &current {
            if let Some(src) = logo.clone() {
                tiles = tiles.child(
                    div()
                        .w(px(36.))
                        .h(px(36.))
                        .rounded(px(8.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .bg(theme.soft.to_hsla())
                        .border_2()
                        .border_color(theme.acc.to_hsla())
                        .child(img(src).w(gpui::relative(0.6)).h(gpui::relative(0.6)).object_fit(ObjectFit::Contain)),
                );
            }
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

        let left = div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(caption("Name", &theme))
            .when_some(name, |d, input| {
                d.child(
                    div()
                        .max_w(px(320.))
                        .px(px(10.))
                        .py(px(6.))
                        .rounded(px(8.))
                        .bg(theme.line.to_hsla())
                        .child(text_input(&input, &theme).family(theme.display_font.clone())),
                )
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .pt(px(4.))
                    .when_some(logo, |d, logo| d.child(img(logo).w(px(18.)).h(px(18.)).object_fit(ObjectFit::Contain)))
                    .child(div().font_family(theme.display_font.clone()).text_size(px(16.)).child(app_name)),
            );

        let right = div()
            .flex()
            .flex_col()
            .items_end()
            .gap(px(8.))
            .child(
                div()
                    .id("icon-reset")
                    .px(px(10.))
                    .py(px(4.))
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
            )
            .child(tiles);

        div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(heading("IDENTITY", None, &theme))
            .child(div().flex().items_start().gap(px(24.)).child(left).child(right))
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

    /// The theme cards, six to a row. Picking one resets its fonts and wallpaper, as the prototype does.
    fn presets_block(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = self.theme.clone();
        let current = self.state.settings.theme.clone();
        let mut grid = div().grid().grid_cols(super::columns(self.settings.body_w, 112., 10., 6)).gap(px(10.));
        for (i, preset) in presets().iter().enumerate() {
            let id = preset.id.to_string();
            let active = id == current;
            let edited = self.state.settings.custom_colours.get(theme_key(&id)).is_some_and(|t| !t.is_empty());
            let wall = Some(Path::new(preset.wallpaper.as_ref())).filter(|p| p.is_file()).and_then(|p| self.wall_thumb(p));
            let gradient = preset.gradient;
            // A small picture of the theme: its background, a sidebar, a session with lines of text and
            // an accent button, so themes with similar backgrounds still look different.
            let look = theme::resolve(preset, &Default::default(), Default::default());
            let line = |w: f32, c: crate::ui::theme::Rgba| div().h(px(3.)).w(gpui::relative(w)).rounded(px(2.)).bg(c.to_hsla());
            let thumb = div()
                .relative()
                .h(px(58.))
                .rounded(px(6.))
                .overflow_hidden()
                .bg(preset.bg.to_hsla())
                .child(match wall {
                    Some(wall) => img(wall).absolute().size_full().object_fit(ObjectFit::Cover).into_any_element(),
                    None => gradient_thumb(gradient).absolute().into_any_element(),
                })
                .child(div().absolute().left_0().top_0().bottom_0().w(gpui::relative(0.22)).bg(look.panel.alpha(0.9).to_hsla()).p(px(5.)).flex().flex_col().gap(px(4.)).child(line(0.8, look.acc)).child(line(0.6, look.muted)).child(line(0.7, look.muted)))
                .child(
                    div()
                        .absolute()
                        .left(gpui::relative(0.28))
                        .right(px(6.))
                        .top(px(6.))
                        .bottom(px(6.))
                        .rounded(px(4.))
                        .bg(look.tile.alpha(0.92).to_hsla())
                        .p(px(6.))
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .child(line(0.55, look.fg))
                        .child(line(0.85, look.muted))
                        .child(line(0.7, look.muted))
                        .child(div().absolute().right(px(5.)).bottom(px(5.)).w(px(14.)).h(px(6.)).rounded(px(3.)).bg(look.acc.to_hsla())),
                );
            grid = grid.child(
                card(("preset", i), active, &theme)
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
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .min_w_0()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_size(px(12.))
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .whitespace_nowrap()
                                    .child(preset.name.to_string()),
                            )
                            .when(edited, |d| d.child(div().flex_none().w(px(6.)).h(px(6.)).rounded_full().bg(theme.acc2.to_hsla()))),
                    ),
            );
        }
        div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(div().flex_1().child(heading("THEME", None, &theme)))
                    .child(
                        Button::new("theme-save", "Save as theme", &theme)
                            .style(ButtonStyle::Outline)
                            .on_click(super::controls::listen(cx, |this, cx| this.save_as_theme(cx))),
                    ),
            )
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
            BACKGROUNDS[style_index].2,
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
        section("BACKGROUND", rows, &theme)
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
        let mut grid = div().flex().flex_wrap().gap(px(8.));
        for (i, (filter, name)) in FILTERS.into_iter().enumerate() {
            let thumb = small.as_ref().and_then(|small| {
                let source = self.with_filter(crate::ui::glass::WallpaperSource::File(small.clone()), filter);
                self.cached_image(format!("filter:{}:{source:?}", small.display()), || {
                    Some(Arc::new(Image::from_bytes(ImageFormat::Png, source.preview_png(192)?)))
                })
            });
            let active = filter == current;
            grid = grid.child(
                div()
                    .id(("bg-filter", i))
                    .relative()
                    .w(px(96.))
                    .h(px(58.))
                    .rounded(px(6.))
                    .overflow_hidden()
                    .cursor_pointer()
                    .border_2()
                    .border_color(if active { theme.acc.to_hsla() } else { gpui::transparent_black() })
                    .bg(theme.line2.to_hsla())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.state.settings.filter = filter;
                        this.refresh_theme(cx);
                    }))
                    .children(thumb.map(|t| img(t).size_full().object_fit(ObjectFit::Cover)))
                    .child(
                        div()
                            .absolute()
                            .left(px(4.))
                            .bottom(px(3.))
                            .px(px(4.))
                            .rounded(px(3.))
                            .bg(gpui::hsla(0., 0., 0., 0.55))
                            .text_size(px(9.))
                            .text_color(gpui::white())
                            .child(name),
                    ),
            );
        }
        grid.into_any_element()
    }

    /// The image tiles: the theme's own background, every image in the backgrounds folder, and an
    /// upload tile.
    fn wallpaper_grid(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = self.theme.clone();
        let picked = self.state.settings.wallpaper.clone().filter(|p| p.is_file());
        let backgrounds = self.backgrounds();
        let mut grid = div().flex().flex_wrap().gap(px(8.));
        let cell = |id: ElementId, active: bool| {
            div()
                .id(id)
                .relative()
                .w(px(96.))
                .h(px(58.))
                .rounded(px(6.))
                .overflow_hidden()
                .cursor_pointer()
                .border_2()
                .border_color(if active { theme.acc.to_hsla() } else { gpui::transparent_black() })
        };
        // The theme's own background: its image, or its gradient.
        let own = Some(PathBuf::from(&theme.wallpaper)).filter(|p| p.is_file()).and_then(|p| self.wall_thumb(&p));
        grid = grid.child(
            cell("wall-theme".into(), picked.is_none())
                .on_click(cx.listener(|this, _, _, cx| {
                    this.state.settings.wallpaper = None;
                    this.refresh_theme(cx);
                }))
                .child(match own {
                    Some(thumb) => img(thumb).size_full().object_fit(ObjectFit::Cover).into_any_element(),
                    None => gradient_thumb(theme.gradient).into_any_element(),
                })
                .child(
                    div()
                        .absolute()
                        .left(px(4.))
                        .bottom(px(3.))
                        .px(px(4.))
                        .rounded(px(3.))
                        .bg(gpui::hsla(0., 0., 0., 0.55))
                        .text_size(px(9.))
                        .text_color(gpui::white())
                        .child("Theme"),
                ),
        );
        for (i, path) in backgrounds.into_iter().enumerate() {
            let Some(thumb) = self.wall_thumb(&path) else { continue };
            let active = picked.as_ref() == Some(&path);
            grid = grid.child(
                cell(("wall", i).into(), active)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.state.settings.wallpaper = Some(path.clone());
                        this.refresh_theme(cx);
                    }))
                    .child(img(thumb).size_full().object_fit(ObjectFit::Cover)),
            );
        }
        grid = grid.child(
            div()
                .id("wall-upload")
                .w(px(96.))
                .h(px(58.))
                .rounded(px(6.))
                .border_1()
                .border_dashed()
                .border_color(theme.line2.to_hsla())
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(10.5))
                .text_color(theme.muted.to_hsla())
                .cursor_pointer()
                .hover(|s| s.text_color(theme.fg.to_hsla()))
                .on_click(cx.listener(|this, _, _, cx| this.pick_wall_file(cx)))
                .child("+ Upload"),
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
            (FontSlot::Titles, "Titles", "Headings, the app name and chat titles.", titles.clone()),
            (FontSlot::Text, "Text", "The chat, the message box and the interface.", text.clone()),
        ]
        .map(|(slot, name, desc, family)| {
            let is_open = open == Some(slot);
            let current = div()
                .flex()
                .items_center()
                .gap(px(10.))
                .child(div().text_size(px(15.)).font_family(family.clone()).child("Aa"))
                .child(div().text_size(px(12.5)).text_color(theme.muted.to_hsla()).child(family.clone()))
                .child(
                    div()
                        .id(SharedString::from(format!("font-change-{name}")))
                        .px(px(10.))
                        .py(px(4.))
                        .rounded(px(6.))
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
            .child(section("FONTS", vec![titles_row, text_row], &theme))
            .when_some(list, |d, list| d.child(list))
            .into_any_element()
    }

    /// The custom colour toggle. When open, every token is a compact swatch, and a click opens its editor.
    fn colours_block(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = self.theme.clone();
        let key = theme_key(&self.state.settings.theme);
        let overrides = self.state.settings.custom_colours.get(key).cloned().unwrap_or_default();
        let has_overrides = !overrides.is_empty();
        let expanded = self.settings.colours_open;

        let mut actions = div().flex().items_center().gap(px(8.));
        if has_overrides {
            actions = actions.child(Button::new("colours-reset", "Reset", &theme).style(ButtonStyle::Outline).on_click(listen(
                cx,
                |this, cx| {
                    let key = theme_key(&this.state.settings.theme);
                    this.state.settings.custom_colours.remove(key);
                    this.settings.picker = None;
                    this.refresh_theme(cx);
                },
            )));
        }
        let toggle_label = if expanded { "Hide" } else { "Edit" };
        actions = actions.child(Button::new("colours-edit", toggle_label, &theme).style(ButtonStyle::Outline).on_click(listen(
            cx,
            |this, cx| {
                this.settings.colours_open = !this.settings.colours_open;
                this.settings.picker = None;
                cx.notify();
            },
        )));
        let row = SettingRow::new("Custom colours", "", &theme).control(actions);
        let mut out = div().flex().flex_col().gap(px(10.)).child(section("COLOURS", vec![row], &theme));
        if !expanded {
            return out.into_any_element();
        }

        let mut groups: Vec<(&str, gpui::Div)> =
            COLOUR_GROUPS.iter().map(|(name, _)| (*name, div().flex().flex_wrap().gap(px(6.)))).collect();
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
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .pl(px(6.))
                    .pr(px(10.))
                    .py(px(4.))
                    .rounded(px(6.))
                    .border_1()
                    .border_color(if open { theme.acc.to_hsla() } else { theme.line2.to_hsla() })
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.line.to_hsla()))
                    .on_click(cx.listener(move |this, _, window, cx| this.open_picker(static_token, window, cx)))
                    .child(div().w(px(22.)).h(px(22.)).rounded_full().bg(colour.to_hsla()).border_1().border_color(theme.line2.to_hsla()))
                    .child(div().text_size(px(12.)).child(label))
                    .when(edited, |d| d.child(div().w(px(6.)).h(px(6.)).rounded_full().bg(theme.acc2.to_hsla()))),
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

        out = out.child(
            div()
                .flex()
                .flex_col()
                .gap(px(4.))
                .children(groups.into_iter().map(|(name, grid)| div().flex().flex_col().gap(px(6.)).child(caption(name, &theme)).child(grid)))
                .when_some(editor, |d, editor| d.child(div().pt(px(8.)).child(editor))),
        );
        out.into_any_element()
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
        section("LAYOUT", rows, &theme)
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
        section("WINDOW", rows, &theme)
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
}
