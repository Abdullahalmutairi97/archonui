//! Composer glyphs. The prototype's SVG path data is parsed and painted with gpui's `PathBuilder`,
//! so the strings below are the design's own. Only the commands the prototype uses are parsed:
//! `M L H V C A Z`, absolute and relative. Arcs are never rotated in these paths, so the x-axis
//! rotation argument is read and ignored.

use gpui::{canvas, div, point, prelude::*, AnyElement, Bounds, Hsla, IntoElement, PathBuilder, PathStyle, Pixels, StrokeOptions, Window};
use crate::ui::scale::px;

/// One path segment in viewBox units, with absolute coordinates.
#[derive(Clone, Debug, PartialEq)]
pub enum Seg {
    Move([f32; 2]),
    Line([f32; 2]),
    Cubic([f32; 2], [f32; 2], [f32; 2]),
    Arc { radii: [f32; 2], large: bool, sweep: bool, to: [f32; 2] },
    Close,
}

/// Paperclip, 16×16, stroke 1.5.
pub const PAPERCLIP: &str = "M10.5 4.5l-5 5a1.6 1.6 0 002.3 2.3l5.5-5.5a3 3 0 00-4.2-4.2L3.6 7.6";
/// Send arrow, 16×16, stroke 1.8.
pub const SEND: &str = "M8 13V3M3.5 7.5L8 3l4.5 4.5";
/// Auto mode: double chevron.
pub const MODE_AUTO: &str = "M3 4l4 4-4 4M8.5 4l4 4-4 4";
/// Plan mode: three rules.
pub const MODE_PLAN: &str = "M3 4h10M3 8h7M3 12h9";
/// Ask mode: question mark.
pub const MODE_ASK: &str = "M5.8 6a2.2 2.2 0 1 1 3.3 1.9c-.7.4-1.1.9-1.1 1.6M8 12.2v.1";

/// Reads path data left to right. Numbers and command letters are separated by spaces, commas or
/// signs; arc flags are single `0`/`1` characters, which can run into the next number (`002.3`).
struct Scan<'a> {
    b: &'a [u8],
    i: usize,
}

impl Scan<'_> {
    fn skip_separators(&mut self) {
        while self.i < self.b.len() && matches!(self.b[self.i], b' ' | b',' | b'\t' | b'\n' | b'\r') {
            self.i += 1;
        }
    }

    fn at_end(&mut self) -> bool {
        self.skip_separators();
        self.i >= self.b.len()
    }

    /// The next command letter, consumed. `None` when the next token is a number.
    fn command(&mut self) -> Option<char> {
        self.skip_separators();
        let c = *self.b.get(self.i)? as char;
        if c.is_ascii_alphabetic() {
            self.i += 1;
            Some(c)
        } else {
            None
        }
    }

    fn number(&mut self) -> Result<f32, String> {
        self.skip_separators();
        let start = self.i;
        if matches!(self.b.get(self.i), Some(b'+' | b'-')) {
            self.i += 1;
        }
        let mut dot = false;
        while self.i < self.b.len() && (self.b[self.i].is_ascii_digit() || (self.b[self.i] == b'.' && !dot)) {
            dot |= self.b[self.i] == b'.';
            self.i += 1;
        }
        let text = std::str::from_utf8(&self.b[start..self.i]).map_err(|e| e.to_string())?;
        text.parse::<f32>().map_err(|_| format!("expected a number at byte {start}"))
    }

    fn flag(&mut self) -> Result<bool, String> {
        self.skip_separators();
        let flag = match self.b.get(self.i) {
            Some(b'0') => false,
            Some(b'1') => true,
            _ => return Err(format!("expected an arc flag at byte {}", self.i)),
        };
        self.i += 1;
        Ok(flag)
    }
}

/// Parses path data into absolute segments.
pub fn parse_path(d: &str) -> Result<Vec<Seg>, String> {
    let mut s = Scan { b: d.as_bytes(), i: 0 };
    let mut segs = Vec::new();
    let (mut cur, mut start) = ([0.0f32; 2], [0.0f32; 2]);
    let mut cmd = ' ';
    while !s.at_end() {
        if let Some(c) = s.command() {
            if matches!(c, 'Z' | 'z') {
                segs.push(Seg::Close);
                cur = start;
            } else {
                cmd = c;
            }
            continue;
        }
        let rel = cmd.is_ascii_lowercase();
        let base = if rel { cur } else { [0.0, 0.0] };
        match cmd.to_ascii_uppercase() {
            'M' | 'L' => {
                let to = [base[0] + s.number()?, base[1] + s.number()?];
                if cmd.eq_ignore_ascii_case(&'M') {
                    start = to;
                    segs.push(Seg::Move(to));
                    // Pairs after a move are line-tos.
                    cmd = if rel { 'l' } else { 'L' };
                } else {
                    segs.push(Seg::Line(to));
                }
                cur = to;
            }
            'H' => {
                cur = [base[0] + s.number()?, cur[1]];
                segs.push(Seg::Line(cur));
            }
            'V' => {
                cur = [cur[0], base[1] + s.number()?];
                segs.push(Seg::Line(cur));
            }
            'C' => {
                let c1 = [base[0] + s.number()?, base[1] + s.number()?];
                let c2 = [base[0] + s.number()?, base[1] + s.number()?];
                let to = [base[0] + s.number()?, base[1] + s.number()?];
                segs.push(Seg::Cubic(c1, c2, to));
                cur = to;
            }
            'A' => {
                let radii = [s.number()?, s.number()?];
                // The x-axis rotation is read and ignored; no prototype path uses it.
                s.number()?;
                let large = s.flag()?;
                let sweep = s.flag()?;
                let to = [base[0] + s.number()?, base[1] + s.number()?];
                segs.push(Seg::Arc { radii, large, sweep, to });
                cur = to;
            }
            other => return Err(format!("unsupported path command {other:?} in {d:?}")),
        }
    }
    Ok(segs)
}

/// The moon for an effort fraction (chat.md §5.4 `MOON`): the lit half of the disc, with the
/// terminator's ellipse set by the fraction. Empty at zero.
pub fn moon_segs(fraction: f32) -> Vec<Seg> {
    if fraction <= 0.0 {
        return Vec::new();
    }
    let rx = ((1.0 - 2.0 * fraction).abs() * 5.5).max(0.01);
    vec![
        Seg::Move([8.0, 2.5]),
        Seg::Arc { radii: [5.5, 5.5], large: false, sweep: true, to: [8.0, 13.5] },
        Seg::Arc { radii: [rx, 5.5], large: false, sweep: fraction > 0.5, to: [8.0, 2.5] },
        Seg::Close,
    ]
}

/// The ring around a moon: a circle of radius 5.5 centred in the viewBox.
fn ring_segs() -> Vec<Seg> {
    vec![
        Seg::Move([2.5, 8.0]),
        Seg::Arc { radii: [5.5, 5.5], large: false, sweep: true, to: [13.5, 8.0] },
        Seg::Arc { radii: [5.5, 5.5], large: false, sweep: true, to: [2.5, 8.0] },
        Seg::Close,
    ]
}

/// Paints segments into `bounds`, which spans the 16-unit viewBox. `stroke` is the stroke width
/// in viewBox units; `None` fills the path.
fn paint(window: &mut Window, segs: &[Seg], bounds: Bounds<Pixels>, stroke: Option<f32>, colour: Hsla) {
    paint_with(window, segs, bounds, stroke, false, colour);
}

/// [`paint`], with round caps and joins when `round` is set (`stroke-linecap`/`linejoin: round`).
fn paint_with(window: &mut Window, segs: &[Seg], bounds: Bounds<Pixels>, stroke: Option<f32>, round: bool, colour: Hsla) {
    let scale = bounds.size.width / 16.0;
    let map = |p: [f32; 2]| bounds.origin + point(scale * p[0], scale * p[1]);
    let mut builder = match stroke {
        Some(w) if round => PathBuilder::fill().with_style(PathStyle::Stroke(
            StrokeOptions::default()
                .with_line_width(f32::from(scale * w))
                .with_line_cap(lyon::tessellation::LineCap::Round)
                .with_line_join(lyon::tessellation::LineJoin::Round),
        )),
        Some(w) => PathBuilder::stroke(scale * w),
        None => PathBuilder::fill(),
    };
    for seg in segs {
        match seg {
            Seg::Move(p) => builder.move_to(map(*p)),
            Seg::Line(p) => builder.line_to(map(*p)),
            Seg::Cubic(c1, c2, to) => builder.cubic_bezier_to(map(*c1), map(*c2), map(*to)),
            Seg::Arc { radii, large, sweep, to } => {
                builder.arc_to(point(scale * radii[0], scale * radii[1]), px(0.), *large, *sweep, map(*to))
            }
            Seg::Close => builder.close(),
        }
    }
    if let Ok(path) = builder.build() {
        window.paint_path(path, colour);
    }
}

/// A stroked path glyph in a `size` × `size` box. `stroke` is in the 16-unit viewBox.
pub fn stroke_icon(d: &'static str, size: f32, stroke: f32, colour: Hsla) -> impl IntoElement {
    let segs = parse_path(d).expect("composer icon path is a constant");
    canvas(|_, _, _| {}, move |bounds, _, window, _| paint(window, &segs, bounds, Some(stroke), colour))
        .w(px(size))
        .h(px(size))
        .flex_none()
}

/// A harness's line icon in a `size` × `size` box: round caps and joins, `stroke` in the 16-unit
/// viewBox. Harnesses without a glyph get their two-letter monogram instead.
pub fn harness_icon(harness: &str, size: f32, stroke: f32, colour: Hsla) -> AnyElement {
    let segs = crate::engine::harness(harness).and_then(|h| h.glyph()).and_then(|d| parse_path(d).ok());
    match segs {
        Some(segs) => canvas(|_, _, _| {}, move |bounds, _, window, _| paint_with(window, &segs, bounds, Some(stroke), true, colour))
            .w(px(size))
            .h(px(size))
            .flex_none()
            .into_any_element(),
        None => div()
            .text_size(px(size * 0.75))
            .font_weight(gpui::FontWeight::BOLD)
            .text_color(colour)
            .child(crate::ui::sidebar::sessions::badge_label(harness))
            .into_any_element(),
    }
}

/// A moon glyph in a `size` × `size` box: the lit part in `colour`, over a thin `ring` circle.
pub fn moon(fraction: f32, size: f32, colour: Hsla, ring: Hsla, ring_width: f32) -> impl IntoElement {
    let lit = moon_segs(fraction);
    let outline = ring_segs();
    canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            paint(window, &outline, bounds, Some(ring_width), ring);
            paint(window, &lit, bounds, None, colour);
        },
    )
    .w(px(size))
    .h(px(size))
    .flex_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn design_paths_parse() {
        for d in [PAPERCLIP, SEND, MODE_AUTO, MODE_PLAN, MODE_ASK] {
            assert!(parse_path(d).is_ok(), "{d}");
        }
    }

    #[test]
    fn every_harness_glyph_parses() {
        for id in ["claude", "codex", "gemini", "opencode", "pi"] {
            let h = crate::engine::harness(id).unwrap();
            if let Some(d) = h.glyph() {
                assert!(parse_path(d).is_ok(), "{id}: {d}");
            }
        }
    }

    #[test]
    fn claude_has_a_glyph_so_the_sidebar_badge_is_not_text() {
        let claude = crate::engine::harness("claude").expect("claude is a known harness");
        assert!(claude.glyph().is_some(), "claude should render an icon, not its two-letter monogram");
    }

    #[test]
    fn relative_and_implicit_commands_resolve_to_absolute_points() {
        // `M3 4l4 4-4 4`: the second pair after `l` is another relative line.
        let segs = parse_path("M3 4l4 4-4 4").unwrap();
        assert_eq!(
            segs,
            vec![Seg::Move([3.0, 4.0]), Seg::Line([7.0, 8.0]), Seg::Line([3.0, 12.0])]
        );
    }

    #[test]
    fn split_decimals_read_as_separate_numbers() {
        let segs = parse_path("M5.8 6c-.7.4-1.1.9-1.1 1.6").unwrap();
        match &segs[1] {
            Seg::Cubic(c1, c2, to) => {
                assert!((c1[0] - 5.1).abs() < 1e-4 && (c1[1] - 6.4).abs() < 1e-4);
                assert!((c2[0] - 4.7).abs() < 1e-4 && (c2[1] - 6.9).abs() < 1e-4);
                assert!((to[0] - 4.7).abs() < 1e-4 && (to[1] - 7.6).abs() < 1e-4);
            }
            other => panic!("expected a cubic, got {other:?}"),
        }
    }

    #[test]
    fn moon_is_empty_at_zero_and_sweeps_by_fraction() {
        assert!(moon_segs(0.0).is_empty());
        let half = moon_segs(0.5);
        assert!(matches!(half[2], Seg::Arc { sweep: false, .. }));
        let gibbous = moon_segs(0.75);
        assert!(matches!(gibbous[2], Seg::Arc { sweep: true, .. }));
    }
}
