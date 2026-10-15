//! The Appearance page: theme, background, fonts, colours, layout, window and identity. Spec: settings.md §3.

use super::{
    colour::{token_colour, HSL_CHANNELS},
    controls::{button_row, caption, heading, listen, section, segmented_row, toggle_row},
    FontSlot, Slide,
};
use crate::{
    app::{
        persist::copy_asset,
        settings::{theme_key, AppIcon, BackgroundFilter, FILTER_PIXEL, GAPS, PANEL_BLUR, RADIUS, TEXT_SIZE, WALLPAPER_DIM},
    },
    ui::{
        anim,
        fonts::{self, FontChoice},
        shell::Shell,
        theme::{self, presets, BackgroundStyle, EDITABLE_TOKENS},
        tiling::LayoutMode,
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

/// The background filters in segment order.
const FILTERS: [(BackgroundFilter, &str); 3] =
    [(BackgroundFilter::Off, "Off"), (BackgroundFilter::Duotone, "Duotone"), (BackgroundFilter::Tritone, "Tritone")];

/// Captions over the colour swatches with how many of `EDITABLE_TOKENS`, in order, each holds.
const COLOUR_GROUPS: [(&str, usize); 4] = [("Surfaces", 4), ("Text", 4), ("Accents", 4), ("Lines and highlights", 5)];

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
        let mut grid = div().grid().grid_cols(6).gap(px(10.));
        for (i, preset) in presets().iter().enumerate() {
            let id = preset.id.to_string();
            let active = id == current;
            let edited = self.state.settings.custom_colours.get(theme_key(&id)).is_some_and(|t| !t.is_empty());
            let wall = Some(Path::new(preset.wallpaper.as_ref())).filter(|p| p.is_file()).and_then(|p| self.wall_thumb(p));
            let gradient = preset.gradient;
            let thumb = div()
                .relative()
                .h(px(46.))
                .rounded(px(6.))
                .overflow_hidden()
                .bg(preset.bg.to_hsla())
                .child(match wall {
                    Some(wall) => img(wall).absolute().size_full().object_fit(ObjectFit::Cover).into_any_element(),
                    None => gradient_thumb(gradient).absolute().into_any_element(),
                });
            grid = grid.child(
                card(("preset", i), active, &theme)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.state.settings.theme = id.clone();
                        // The open colour editor belongs to the theme it was opened on.
                        this.settings.picker = None;
                        this.state.settings.fonts.remove(theme_key(&id));
                        this.state.settings.wallpapers.remove(theme_key(&id));
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
            .child(heading("THEME", Some("Picking a theme also resets fonts and wallpaper."), &theme))
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
        let filter = self.state.settings.filter;
        let filter_index = FILTERS.iter().position(|(f, _)| *f == filter).unwrap_or(0);
        let filter_names: Vec<&str> = FILTERS.iter().map(|(_, name)| *name).collect();
        let pixel = self.state.settings.filter_pixel;
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
        let image_row = SettingRow::new("Image", "Uploads are kept in the backgrounds folder.", &theme).control(images);
        let filter_row = segmented_row(
            "bg-filter",
            "Filter",
            "Dithers the background into a few colours.",
            &filter_names,
            filter_index,
            &weak,
            &theme,
            |this, i, cx| {
                this.state.settings.filter = FILTERS[i].0;
                this.refresh_theme(cx);
            },
            None,
        );

        let mut rows = vec![style_row, image_row, filter_row];
        if filter != BackgroundFilter::Off {
            let pixel_slider = self.slide(Slide::FilterPixel, pixel, range(FILTER_PIXEL, 1.), "px", cx);
            rows.push(SettingRow::new("Dot size", "Size of each dot in the filter.", &theme).control(pixel_slider));
        }
        let dim_slider = self.slide(Slide::Dim, dim, range(WALLPAPER_DIM, 5.), "%", cx);
        let blur_slider = self.slide(Slide::Blur, blur, range(PANEL_BLUR, 1.), "px", cx);
        let opacity_slider = self.slide(Slide::Opacity, opacity, range(crate::app::settings::WINDOW_OPACITY, 5.), "%", cx);
        rows.push(SettingRow::new("Wallpaper dim", "", &theme).control(dim_slider));
        rows.push(SettingRow::new("Panel blur", "Blur behind the sidebars and glass tiles.", &theme).control(blur_slider));
        rows.push(
            SettingRow::new("Window opacity", "Below 100% the desktop shows through. Text stays solid.", &theme).control(opacity_slider),
        );
        section("BACKGROUND", rows, &theme)
    }

    /// The image tiles: the theme's own background, every image in the backgrounds folder, and an
    /// upload tile.
    fn wallpaper_grid(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = self.theme.clone();
        let picked = self.state.settings.wallpapers.get(theme_key(&self.state.settings.theme)).filter(|p| p.is_file()).cloned();
        let mut grid = div().flex().flex_wrap().justify_end().gap(px(6.)).max_w(px(420.));
        let cell = |id: ElementId, active: bool| {
            div()
                .id(id)
                .relative()
                .w(px(64.))
                .h(px(40.))
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
                    let key = theme_key(&this.state.settings.theme).to_string();
                    this.state.settings.wallpapers.remove(&key);
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
        for (i, path) in self.backgrounds().into_iter().enumerate() {
            let Some(thumb) = self.wall_thumb(&path) else { continue };
            let active = picked.as_ref() == Some(&path);
            grid = grid.child(
                cell(("wall", i).into(), active)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let key = theme_key(&this.state.settings.theme).to_string();
                        this.state.settings.wallpapers.insert(key, path.clone());
                        this.refresh_theme(cx);
                    }))
                    .child(img(thumb).size_full().object_fit(ObjectFit::Cover)),
            );
        }
        grid = grid.child(
            div()
                .id("wall-upload")
                .w(px(64.))
                .h(px(40.))
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

    /// Copies an uploaded image into the backgrounds folder, where it stays for every theme, and makes it
    /// this theme's background.
    fn pick_wall_file(&mut self, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions { files: true, directories: false, multiple: false, prompt: None });
        let dir = self.backgrounds_dir();
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else { return };
            let Some(src) = paths.into_iter().next() else { return };
            let copied = cx.background_executor().spawn(async move { copy_asset(&dir, &src) }).await;
            let _ = this.update(cx, |this, cx| match copied {
                Ok(path) => {
                    let key = theme_key(&this.state.settings.theme).to_string();
                    this.state.settings.wallpapers.insert(key, path);
                    this.refresh_theme(cx);
                }
                Err(e) => this.toast(format!("Wallpaper not added: {e}"), cx),
            });
        })
        .detach();
    }

    /// The theme's pairings as a segmented control, the Titles or Text slot, then a chip per font.
    fn typography_block(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = self.theme.clone();
        let weak = cx.entity().downgrade();
        let key = theme_key(&self.state.settings.theme);
        let choice = self.state.settings.fonts.get(key).cloned().unwrap_or_default();
        let preset = theme::preset(&self.state.settings.theme);
        let custom = choice.display.is_some() || choice.ui.is_some();
        let pairing = preset.pairings[choice.pairing % 3];
        let pairing_names: Vec<&str> = preset.pairings.iter().map(|p| p.label).collect();
        // A custom font clears the pairing highlight, as the pairing cards did.
        let pairing_index = if custom { pairing_names.len() } else { choice.pairing % 3 };
        let slot = self.settings.font_slot;
        let slot_index = usize::from(slot == FontSlot::Text);

        let pairing_row = segmented_row(
            "font-pairing",
            "Pairing",
            "Sets the Titles and Text fonts together.",
            &pairing_names,
            pairing_index,
            &weak,
            &theme,
            |this, i, cx| {
                let key = theme_key(&this.state.settings.theme).to_string();
                this.state.settings.fonts.insert(key, FontChoice { pairing: i, display: None, ui: None });
                this.refresh_theme(cx);
            },
            None,
        );
        let slot_row = segmented_row(
            "font-slot",
            "Apply to",
            "Choose a font below for the titles, or for the chat and message text.",
            &["Titles", "Text"],
            slot_index,
            &weak,
            &theme,
            |this, i, cx| {
                this.settings.font_slot = if i == 1 { FontSlot::Text } else { FontSlot::Titles };
                cx.notify();
            },
            None,
        );

        let titles = choice.display.clone().unwrap_or_else(|| pairing.display.to_string());
        let text = choice.ui.clone().unwrap_or_else(|| pairing.ui.to_string());
        let mut chips = div().flex().flex_wrap().gap(px(6.));
        for (i, (label, family)) in fonts::all_fonts().into_iter().enumerate() {
            let (is_titles, is_text) = (titles == family, text == family);
            let editing = match slot {
                FontSlot::Titles => is_titles,
                FontSlot::Text => is_text,
            };
            let mut chip = div()
                .id(("font", i))
                .flex()
                .items_center()
                .gap(px(6.))
                .px(px(10.))
                .py(px(6.))
                .rounded(px(6.))
                .border_1()
                .border_color(if editing { theme.acc.to_hsla() } else { theme.line2.to_hsla() })
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
                .child(div().text_size(px(13.)).font_family(family).child(label));
            if is_titles {
                chip = chip.child(badge("T", &theme));
            }
            if is_text {
                chip = chip.child(badge("Aa", &theme));
            }
            chips = chips.child(chip);
        }

        div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(section("FONTS", vec![pairing_row, slot_row], &theme))
            .child(caption(&format!("Titles: {titles} · Text (chat and message box): {text}"), &theme))
            .child(chips)
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
        let row = SettingRow::new("Custom colours", "Fine-tune this theme's colours.", &theme).control(actions);
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
            SettingRow::new("Interface size", "Scales the whole window, like zooming a page.", &theme).control(scale_slider),
            SettingRow::new("Message text size", "", &theme).control(text_slider),
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
        ];
        section("WINDOW", rows, &theme)
    }
}
