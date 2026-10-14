//! The colour editor for one Appearance token. A `#rrggbb` value typed in the field, or a hue, saturation or lightness
//! slider, applies live. The sliders and the field stay in step with each other.

use super::controls::edit;
use crate::{
    app::settings::theme_key,
    ui::{
        shell::Shell,
        theme::{Rgba, Theme},
        widgets::{
            slider::{Slider, SliderRange},
            text_input::{TextInput, TextInputConfig, TextInputEvent},
        },
    },
};
use gpui::{Context, Entity, Subscription, Window, prelude::*};
use crate::ui::scale::px;

/// The hue, saturation and lightness sliders, in that order: label, range and unit.
pub(super) const HSL_CHANNELS: [(&str, SliderRange, &str); 3] = [
    ("Hue", SliderRange { min: 0., max: 360., step: 1. }, "°"),
    ("Saturation", SliderRange { min: 0., max: 100., step: 1. }, "%"),
    ("Lightness", SliderRange { min: 0., max: 100., step: 1. }, "%"),
];

/// The open editor: which token it changes, the hex field, and the hue, saturation and lightness sliders.
pub struct Picker {
    pub(super) token: &'static str,
    pub(super) input: Entity<TextInput>,
    pub(super) hsl: [Entity<Slider>; 3],
    pub(super) _subscription: Subscription,
}

/// The resolved colour of `token` in the current theme, or `None` for a token that is not editable.
pub(super) fn token_colour(theme: &Theme, token: &str) -> Option<Rgba> {
    Some(match token {
        "bg" => theme.bg,
        "tile" => theme.tile,
        "panel" => theme.panel,
        "pop" => theme.pop,
        "fg" => theme.fg,
        "muted" => theme.muted,
        "faint" => theme.faint,
        "on" => theme.on,
        "acc" => theme.acc,
        "acc2" => theme.acc2,
        "alt" => theme.alt,
        "deep" => theme.deep,
        _ => return None,
    })
}

/// `#rrggbb` for an opaque colour.
pub(super) fn hex(colour: Rgba) -> String {
    format!("#{:02x}{:02x}{:02x}", colour.r, colour.g, colour.b)
}

impl Shell {
    /// Opens the editor for `token`, replacing any editor already open.
    pub(super) fn open_picker(&mut self, token: &'static str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(colour) = token_colour(&self.theme, token) else { return };
        let config = TextInputConfig {
            placeholder: "#rrggbb".into(),
            max_len: Some(7),
            font_size: px(13.),
            line_height: px(18.),
            ..Default::default()
        };
        let input = cx.new(|cx| TextInput::new(config, window, cx));
        input.update(cx, |input, cx| input.set_text(&hex(colour), cx));
        input.read(cx).focus(window);
        let subscription = cx.subscribe(&input, move |this, input, event: &TextInputEvent, cx| match event {
            TextInputEvent::Changed => {
                if let Some(colour) = parse_hex(&input.read(cx).text()) {
                    this.hex_changed(token, colour, cx);
                }
            }
            TextInputEvent::Submit => {
                this.settings.picker = None;
                cx.notify();
            }
            TextInputEvent::Blur => {}
        });
        let shell = cx.entity().downgrade();
        let values = to_hsl(colour);
        let hsl = std::array::from_fn(|channel| {
            let (_, range, unit) = HSL_CHANNELS[channel];
            let on_change = edit(&shell, move |this: &mut Shell, _: f32, cx| this.hsl_changed(token, cx));
            cx.new(|_| Slider::new(values[channel], range, unit, on_change))
        });
        self.settings.picker = Some(Picker { token, input, hsl, _subscription: subscription });
        cx.notify();
    }

    /// Stores a custom colour for the current theme and repaints.
    pub(super) fn set_colour(&mut self, token: &str, value: String, cx: &mut Context<Self>) {
        let key = theme_key(self.state.settings.theme).to_string();
        self.state.settings.custom_colours.entry(key).or_default().insert(token.to_string(), value);
        self.refresh_theme(cx);
    }

    /// Drops the custom colour for `token` in the current theme, back to the preset.
    pub(super) fn reset_colour(&mut self, token: &str, cx: &mut Context<Self>) {
        let key = theme_key(self.state.settings.theme);
        if let Some(tokens) = self.state.settings.custom_colours.get_mut(key) {
            tokens.remove(token);
            if tokens.is_empty() {
                self.state.settings.custom_colours.remove(key);
            }
        }
        if self.settings.picker.as_ref().is_some_and(|p| p.token == token) {
            self.settings.picker = None;
        }
        self.refresh_theme(cx);
    }

    /// Applies a colour typed in the hex field, and moves the sliders to match it.
    fn hex_changed(&mut self, token: &'static str, colour: Rgba, cx: &mut Context<Self>) {
        self.set_colour(token, hex(colour), cx);
        let sliders = self.settings.picker.as_ref().map(|p| p.hsl.clone());
        for (slider, value) in sliders.iter().flatten().zip(to_hsl(colour)) {
            slider.update(cx, |slider, cx| slider.set_value(value, cx));
        }
    }

    /// Applies the colour the three sliders describe, and writes it back to the hex field.
    fn hsl_changed(&mut self, token: &'static str, cx: &mut Context<Self>) {
        let Some(picker) = self.settings.picker.as_ref() else { return };
        let [h, s, l] = picker.hsl.each_ref().map(|slider| slider.read(cx).value());
        let input = picker.input.clone();
        let colour = from_hsl(h, s, l);
        self.set_colour(token, hex(colour), cx);
        input.update(cx, |input, cx| input.set_text(&hex(colour), cx));
    }
}

/// Parses a `#rrggbb` colour; `None` for anything else.
pub(super) fn parse_hex(text: &str) -> Option<Rgba> {
    let digits = text.strip_prefix('#')?;
    if digits.len() != 6 || !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let channel = |i: usize| u8::from_str_radix(&digits[i..i + 2], 16).ok();
    Some(Rgba::new(channel(0)?, channel(2)?, channel(4)?, 1.))
}

/// Hue in degrees, then saturation and lightness in percent, for an opaque colour.
fn to_hsl(colour: Rgba) -> [f32; 3] {
    let [r, g, b] = [colour.r, colour.g, colour.b].map(|c| f32::from(c) / 255.);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let lightness = (max + min) / 2.;
    let delta = max - min;
    if delta == 0. {
        return [0., 0., lightness * 100.];
    }
    let saturation = if lightness > 0.5 { delta / (2. - max - min) } else { delta / (max + min) };
    let hue = if max == r {
        (g - b) / delta + if g < b { 6. } else { 0. }
    } else if max == g {
        (b - r) / delta + 2.
    } else {
        (r - g) / delta + 4.
    };
    [hue * 60., saturation * 100., lightness * 100.]
}

/// The colour for a hue in degrees, and a saturation and lightness in percent.
fn from_hsl(hue: f32, saturation: f32, lightness: f32) -> Rgba {
    let s = (saturation / 100.).clamp(0., 1.);
    let l = (lightness / 100.).clamp(0., 1.);
    let q = if l < 0.5 { l * (1. + s) } else { l + s - l * s };
    let p = 2. * l - q;
    let h = hue.rem_euclid(360.) / 360.;
    let channel = |t: f32| {
        let t = t.rem_euclid(1.);
        let v = if t < 1. / 6. {
            p + (q - p) * 6. * t
        } else if t < 0.5 {
            q
        } else if t < 2. / 3. {
            p + (q - p) * (2. / 3. - t) * 6.
        } else {
            p
        };
        (v * 255.).round() as u8
    };
    Rgba::new(channel(h + 1. / 3.), channel(h), channel(h - 1. / 3.), 1.)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hex_and_rejects_anything_else() {
        assert_eq!(parse_hex("#b04cff"), Some(Rgba::new(0xb0, 0x4c, 0xff, 1.)));
        assert_eq!(parse_hex("b04cff"), None);
        assert_eq!(parse_hex("#b04cf"), None);
        assert_eq!(parse_hex("#b04cgg"), None);
    }

    #[test]
    fn hsl_matches_known_values_and_round_trips() {
        let accent = parse_hex("#b04cff").unwrap();
        let [h, s, l] = to_hsl(accent);
        assert!((h - 274.).abs() < 1., "hue {h}");
        assert!((s - 100.).abs() < 1., "saturation {s}");
        assert!((l - 65.).abs() < 1., "lightness {l}");
        assert_eq!(hex(from_hsl(h, s, l)), "#b04cff");
        assert_eq!(hex(from_hsl(274., 100., 25.)), "#480080");
        assert_eq!(to_hsl(Rgba::new(128, 128, 128, 1.))[1], 0.);
    }
}
