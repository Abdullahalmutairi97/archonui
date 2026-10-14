//! Tools-panel glyphs: the design's 16-unit SVG paths (`design/ArchonUI Agent v3.dc.html` L333–353), stroked on a
//! canvas in the current text colour so hover styles recolour them.

use gpui::{canvas, prelude::*, Bounds, IntoElement, PathBuilder, Pixels, Point, Window, App};
use crate::ui::scale::px;
use std::f32::consts::TAU;

#[derive(Clone, Copy)]
pub(super) enum Glyph {
    ChevronLeft,
    ChevronRight,
    Browser,
    Ide,
    Files,
    Git,
    Terminal,
    Minimize,
    Maximize,
    Close,
}

/// A `size` px square glyph. `stroke` is in 16-unit grid units, like the design's `stroke-width`.
pub(super) fn glyph(glyph: Glyph, size: f32, stroke: f32) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |bounds: Bounds<Pixels>, _, window: &mut Window, _: &mut App| {
            let scale = size / 16.;
            let color = window.text_style().color;
            let origin = bounds.origin;
            let at = |(x, y): (f32, f32)| Point::new(origin.x + px(x * scale), origin.y + px(y * scale));
            for line in strokes(glyph) {
                let mut builder = PathBuilder::stroke(px(stroke * scale));
                for (i, &point) in line.points.iter().enumerate() {
                    if i == 0 {
                        builder.move_to(at(point));
                    } else {
                        builder.line_to(at(point));
                    }
                }
                if line.closes {
                    builder.close();
                }
                if let Ok(path) = builder.build() {
                    window.paint_path(path, color);
                }
            }
        },
    )
    .size(px(size))
    .flex_none()
}

/// A polyline in 16-unit grid coordinates; curves are sampled into points.
struct Line {
    points: Vec<(f32, f32)>,
    closes: bool,
}

const CURVE_STEPS: usize = 6;
const CIRCLE_STEPS: usize = 24;

impl Line {
    fn start(x: f32, y: f32) -> Self {
        Self { points: vec![(x, y)], closes: false }
    }

    fn line_to(mut self, x: f32, y: f32) -> Self {
        self.points.push((x, y));
        self
    }

    /// A cubic Bézier from the last point, as in the SVG `c` command.
    fn curve_to(mut self, c1: (f32, f32), c2: (f32, f32), end: (f32, f32)) -> Self {
        let start = self.points.last().copied().unwrap_or_default();
        for step in 1..=CURVE_STEPS {
            let t = step as f32 / CURVE_STEPS as f32;
            let u = 1. - t;
            let (a, b, c, d) = (u * u * u, 3. * u * u * t, 3. * u * t * t, t * t * t);
            self.points.push((
                a * start.0 + b * c1.0 + c * c2.0 + d * end.0,
                a * start.1 + b * c1.1 + c * c2.1 + d * end.1,
            ));
        }
        self
    }

    fn close(mut self) -> Self {
        self.closes = true;
        self
    }
}

fn circle(cx: f32, cy: f32, r: f32) -> Line {
    let points = (0..CIRCLE_STEPS)
        .map(|k| {
            let a = k as f32 / CIRCLE_STEPS as f32 * TAU;
            (cx + r * a.cos(), cy + r * a.sin())
        })
        .collect();
    Line { points, closes: true }
}

fn rounded_rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Line {
    // Corner centres, each with the start angle (degrees) of its quarter arc; y grows downwards.
    let corners = [(x + w - r, y + r, -90.), (x + w - r, y + h - r, 0.), (x + r, y + h - r, 90.), (x + r, y + r, 180.)];
    let mut points = Vec::new();
    for (cx, cy, start) in corners {
        for step in 0..=CURVE_STEPS {
            let a = (start + 90. * step as f32 / CURVE_STEPS as f32).to_radians();
            points.push((cx + r * a.cos(), cy + r * a.sin()));
        }
    }
    Line { points, closes: true }
}

fn strokes(glyph: Glyph) -> Vec<Line> {
    match glyph {
        Glyph::ChevronLeft => vec![Line::start(10., 3.5).line_to(5.5, 8.).line_to(10., 12.5)],
        Glyph::ChevronRight => vec![Line::start(6., 3.5).line_to(10.5, 8.).line_to(6., 12.5)],
        Glyph::Browser => vec![
            circle(8., 8., 6.),
            Line::start(2., 8.).line_to(14., 8.),
            Line::start(8., 2.).curve_to((10.2, 4.2), (10.2, 11.8), (8., 14.)),
            Line::start(8., 2.).curve_to((5.8, 4.2), (5.8, 11.8), (8., 14.)),
        ],
        Glyph::Ide => vec![
            Line::start(5.5, 4.5).line_to(2., 8.).line_to(5.5, 11.5),
            Line::start(10.5, 4.5).line_to(14., 8.).line_to(10.5, 11.5),
        ],
        Glyph::Files => vec![Line::start(1.8, 4.2)
            .curve_to((1.8, 3.5), (2.3, 3.), (3., 3.))
            .line_to(6., 3.)
            .line_to(7.5, 4.5)
            .line_to(13., 4.5)
            .curve_to((13.7, 4.5), (14.2, 5.), (14.2, 5.7))
            .line_to(14.2, 11.8)
            .curve_to((14.2, 12.5), (13.7, 13.), (13., 13.))
            .line_to(3., 13.)
            .curve_to((2.3, 13.), (1.8, 12.5), (1.8, 11.8))
            .close()],
        Glyph::Git => vec![
            circle(4.5, 3.5, 1.6),
            circle(4.5, 12.5, 1.6),
            circle(11.5, 5.5, 1.6),
            Line::start(4.5, 5.1).line_to(4.5, 10.9),
            Line::start(11.5, 7.1).curve_to((11.5, 9.5), (9.3, 10.1), (5.5, 11.1)),
        ],
        Glyph::Terminal => vec![
            rounded_rect(1.8, 2.8, 12.4, 10.4, 1.8),
            Line::start(4.8, 6.3).line_to(7., 8.).line_to(4.8, 9.7),
            Line::start(8.6, 10.).line_to(11.2, 10.),
        ],
        Glyph::Minimize => vec![Line::start(4., 8.).line_to(12., 8.)],
        Glyph::Maximize => vec![rounded_rect(4., 4., 8., 8., 1.)],
        Glyph::Close => vec![Line::start(4.5, 4.5).line_to(11.5, 11.5), Line::start(11.5, 4.5).line_to(4.5, 11.5)],
    }
}
