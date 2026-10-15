//! Attachment cards from chat.md §3: a file-type badge (or image thumbnail), a name and a meta line.
//! Two sizes: the composer's removable card and the card in a sent message.
//! [`badge_for`] and [`display_name`] are pure; the card is the GPUI element.

use std::rc::Rc;

use gpui::{
    div, img, prelude::*, App, ElementId, FontWeight, Hsla, ImageSource, IntoElement,
    ObjectFit, RenderOnce, SharedString, Window,
};
use crate::ui::scale::px;

use crate::ui::theme::{Rgba, Theme};

/// Which theme colour a file-type badge uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Acc,
    Acc2,
    Alt,
    Fg,
    Muted,
}

impl Tone {
    /// The theme colour for this tone.
    pub fn colour(self, theme: &Theme) -> Rgba {
        match self {
            Tone::Acc => theme.acc,
            Tone::Acc2 => theme.acc2,
            Tone::Alt => theme.alt,
            Tone::Fg => theme.fg,
            Tone::Muted => theme.muted,
        }
    }
}

/// What a card shows in its thumbnail slot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Badge {
    /// An image file: the card shows its thumbnail.
    Image,
    /// A file: a short label in the given tone.
    File { label: String, tone: Tone },
}

const IMAGE_EXTENSIONS: [&str; 5] = ["png", "jpg", "jpeg", "gif", "webp"];

/// The badge for `name`, following chat.md §3.1. Known extensions get their own label and tone;
/// others get their first four letters (or FILE) in `muted`.
pub fn badge_for(name: &str) -> Badge {
    // The extension is read from the file name only, so a dot in a folder name (`v1.2/Makefile`) doesn't leak into it.
    let base = name.rsplit('/').next().unwrap_or(name);
    let ext = base.rsplit('.').next().unwrap_or("").to_lowercase();
    if IMAGE_EXTENSIONS.contains(&ext.as_str()) {
        return Badge::Image;
    }
    let (label, tone) = match ext.as_str() {
        "ts" => ("TS", Tone::Acc2),
        "tsx" => ("TSX", Tone::Acc2),
        "json" => ("{ }", Tone::Alt),
        "md" => ("MD", Tone::Fg),
        "pdf" => ("PDF", Tone::Acc),
        "log" => ("LOG", Tone::Alt),
        "wgsl" => ("WGSL", Tone::Acc2),
        _ => {
            let label: String = ext.chars().take(4).collect::<String>().to_uppercase();
            let label = if label.is_empty() { "FILE".to_string() } else { label };
            return Badge::File { label, tone: Tone::Muted };
        }
    };
    Badge::File { label: label.to_string(), tone }
}

/// The name shown on a card: the basename of `full`, with two built-in names renamed.
pub fn display_name(full: &str) -> String {
    match full {
        "browser.png" => "browser screenshot.png".to_string(),
        "terminal.log" => "terminal output.log".to_string(),
        _ => full.rsplit('/').next().unwrap_or(full).to_string(),
    }
}

/// Which size of card to draw.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CardVariant {
    /// In the composer: 36px thumbnail, a remove button, up to 230px wide.
    Composer,
    /// In a sent message: 30px thumbnail, no remove button, up to 240px wide.
    Sent,
}

#[derive(Clone, Copy)]
struct Colors {
    fg: Hsla,
    muted: Hsla,
    line: Hsla,
    line2: Hsla,
    pop: Hsla,
    acc: Hsla,
    tile: Hsla,
}

/// An attachment card. `full` is the file path; the card derives its name and badge from it.
#[derive(IntoElement)]
pub struct AttachmentCard {
    id: SharedString,
    full: SharedString,
    meta: SharedString,
    thumbnail: Option<ImageSource>,
    variant: CardVariant,
    colors: Colors,
    tone_colour: Hsla,
    on_remove: Option<Rc<dyn Fn(&mut Window, &mut App)>>,
}

impl AttachmentCard {
    /// `id` must be unique per window: the remove button's id is derived from it.
    /// `meta` is the second line, such as `src/server · 23 lines` or `Image · 1920×1080`.
    pub fn new(
        id: impl Into<SharedString>,
        full: impl Into<SharedString>,
        meta: impl Into<SharedString>,
        variant: CardVariant,
        theme: &Theme,
    ) -> Self {
        let full: SharedString = full.into();
        let tone_colour = match badge_for(&full) {
            Badge::Image => theme.muted,
            Badge::File { tone, .. } => tone.colour(theme),
        }
        .to_hsla();
        Self {
            id: id.into(),
            full,
            meta: meta.into(),
            thumbnail: None,
            variant,
            colors: Colors {
                fg: theme.fg.to_hsla(),
                muted: theme.muted.to_hsla(),
                line: theme.line.to_hsla(),
                line2: theme.line2.to_hsla(),
                pop: theme.pop.to_hsla(),
                acc: theme.acc.to_hsla(),
                tile: theme.tile.to_hsla(),
            },
            tone_colour,
            on_remove: None,
        }
    }

    /// The thumbnail for an image card. Without one, an image card shows a plain `tile` square.
    pub fn thumbnail(mut self, source: impl Into<ImageSource>) -> Self {
        self.thumbnail = Some(source.into());
        self
    }

    /// Shows the remove button. Only the composer variant should set this.
    pub fn on_remove(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_remove = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for AttachmentCard {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let c = self.colors;
        let composer = self.variant == CardVariant::Composer;
        let (thumb, name_size, meta_size, gap) = if composer {
            (px(36.), px(12.), px(10.5), px(2.))
        } else {
            (px(30.), px(11.5), px(10.), px(1.))
        };
        let badge = badge_for(&self.full);
        let name = display_name(&self.full);

        let thumbnail = match (&badge, self.thumbnail) {
            (Badge::Image, Some(source)) => img(source)
                .flex_none()
                .size(thumb)
                .rounded(px(if composer { 6. } else { 5. }))
                .object_fit(ObjectFit::Cover)
                .into_any_element(),
            (Badge::Image, None) => div()
                .flex_none()
                .size(thumb)
                .rounded(px(if composer { 6. } else { 5. }))
                .bg(c.tile)
                .into_any_element(),
            (Badge::File { label, .. }, _) => div()
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .size(thumb)
                .rounded(px(if composer { 6. } else { 5. }))
                .bg(tinted(self.tone_colour, 0.14))
                .text_color(self.tone_colour)
                .text_size(if composer { px(9.5) } else { px(9.) })
                .font_weight(FontWeight::BOLD)
                .child(label.clone())
                .into_any_element(),
        };

        let text = div()
            .min_w_0()
            .flex()
            .flex_col()
            .gap(gap)
            .child(
                div()
                    .overflow_hidden()
                    .text_ellipsis()
                    .text_size(name_size)
                    .text_color(c.fg)
                    .child(name),
            )
            .child(
                div()
                    .overflow_hidden()
                    .text_ellipsis()
                    .text_size(meta_size)
                    .text_color(c.muted)
                    .child(self.meta),
            );

        let card = div()
            .id(ElementId::Name(self.id.clone()))
            .relative()
            .flex()
            .items_center()
            .border_1()
            .border_color(c.line2)
            .bg(c.line)
            .when(composer, |d| d.gap(px(10.)).pl(px(5.)).pr(px(12.)).py(px(5.)).rounded(px(9.)).max_w(px(230.)))
            .when(!composer, |d| d.gap(px(9.)).pl(px(4.)).pr(px(11.)).py(px(4.)).rounded(px(8.)).max_w(px(240.)))
            .child(thumbnail)
            .child(text);

        match self.on_remove {
            Some(handler) => card.child(
                div()
                    .id(ElementId::NamedInteger(self.id, 1))
                    .absolute()
                    .top(px(-7.))
                    .right(px(-7.))
                    .size(px(18.))
                    .rounded(px(9.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(11.))
                    .bg(c.pop)
                    .border_1()
                    .border_color(c.line2)
                    .text_color(c.muted)
                    .cursor_pointer()
                    .hover(move |style| style.text_color(c.fg).border_color(c.acc))
                    .on_click(move |_, window, cx| handler(window, cx))
                    .child("×"),
            ),
            None => card,
        }
    }
}

/// `colour` at `alpha`, for the tinted fill behind a badge label (chat.md §3.1 uses 0.14).
fn tinted(colour: Hsla, alpha: f32) -> Hsla {
    Hsla { a: alpha, ..colour }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::fonts::FontChoice;
    use crate::ui::theme::resolve;
    use std::collections::HashMap;

    fn file(label: &str, tone: Tone) -> Badge {
        Badge::File { label: label.to_string(), tone }
    }

    #[test]
    fn known_extensions_get_their_label_and_tone() {
        assert_eq!(badge_for("src/app.ts"), file("TS", Tone::Acc2));
        assert_eq!(badge_for("App.tsx"), file("TSX", Tone::Acc2));
        assert_eq!(badge_for("package.json"), file("{ }", Tone::Alt));
        assert_eq!(badge_for("README.md"), file("MD", Tone::Fg));
        assert_eq!(badge_for("spec.pdf"), file("PDF", Tone::Acc));
        assert_eq!(badge_for("run.log"), file("LOG", Tone::Alt));
        assert_eq!(badge_for("shader.wgsl"), file("WGSL", Tone::Acc2));
    }

    #[test]
    fn extensions_match_case_insensitively() {
        assert_eq!(badge_for("DATA.JSON"), file("{ }", Tone::Alt));
        assert_eq!(badge_for("photo.PNG"), Badge::Image);
    }

    #[test]
    fn image_extensions_show_a_thumbnail() {
        for name in ["a.png", "a.jpg", "a.jpeg", "a.gif", "a.webp"] {
            assert_eq!(badge_for(name), Badge::Image, "{name}");
        }
        assert_eq!(badge_for("a.svg"), file("SVG", Tone::Muted));
    }

    #[test]
    fn unknown_extensions_use_four_letters_in_muted() {
        assert_eq!(badge_for("notes.txt"), file("TXT", Tone::Muted));
        assert_eq!(badge_for("config.toml"), file("TOML", Tone::Muted));
        assert_eq!(badge_for("Makefile"), file("MAKE", Tone::Muted));
        assert_eq!(badge_for("trailing."), file("FILE", Tone::Muted));
        assert_eq!(badge_for("v1.2/Makefile"), file("MAKE", Tone::Muted), "a dot in a folder name is not an extension");
    }

    #[test]
    fn display_name_is_the_basename_with_two_renamed_builtins() {
        assert_eq!(display_name("src/server/main.rs"), "main.rs");
        assert_eq!(display_name("browser.png"), "browser screenshot.png");
        assert_eq!(display_name("terminal.log"), "terminal output.log");
    }

    #[test]
    fn badge_colours_come_from_the_blood_moon_tokens() {
        let theme = resolve(crate::ui::theme::preset(crate::ui::theme::DEFAULT_THEME), &HashMap::new(), FontChoice::default());
        assert_eq!(Tone::Acc2.colour(&theme), theme.acc2);
        assert_eq!(Tone::Acc.colour(&theme), theme.acc);
        assert_eq!(Tone::Alt.colour(&theme), theme.alt);
        assert_eq!(Tone::Muted.colour(&theme), theme.muted);
        assert_eq!(Tone::Fg.colour(&theme), theme.fg);
    }

    #[test]
    fn tint_keeps_the_hue_and_sets_alpha() {
        let tint = tinted(gpui::hsla(0.5, 0.5, 0.5, 1.), 0.14);
        assert_eq!((tint.h, tint.s, tint.l), (0.5, 0.5, 0.5));
        assert!((tint.a - 0.14).abs() < 1e-6);
    }
}
