//! Sidebar glyphs, drawn from the prototype's 16-unit SVG paths (layout.md §2.1–2.4).

use gpui::{canvas, div, point, prelude::*, Bounds, Hsla, IntoElement, PathBuilder, Pixels, Point, Window};
use crate::ui::scale::px;

/// A glyph from the sidebar chrome.
#[derive(Clone, Copy)]
pub enum Glyph {
    /// Collapse (‹) in the header.
    ChevronLeft,
    /// Expand (›) in the rail.
    ChevronRight,
    Plus,
    Gear,
    Share,
}

enum Shape {
    /// A polyline, closed when the flag is set.
    Line(&'static [(f32, f32)], bool),
    /// A ring of radius `r` around `(x, y)`.
    Ring(f32, f32, f32),
}

const GEAR_TEETH: &[(f32, f32)] = &[
    (12.75, 6.78), (14.31, 6.96), (14.31, 9.04), (12.75, 9.22), (12.22, 10.49), (13.20, 11.73),
    (11.73, 13.20), (10.49, 12.22), (9.22, 12.75), (9.04, 14.31), (6.96, 14.31), (6.78, 12.75),
    (5.51, 12.22), (4.27, 13.20), (2.80, 11.73), (3.78, 10.49), (3.25, 9.22), (1.69, 9.04),
    (1.69, 6.96), (3.25, 6.78), (3.78, 5.51), (2.80, 4.27), (4.27, 2.80), (5.51, 3.78),
    (6.78, 3.25), (6.96, 1.69), (9.04, 1.69), (9.22, 3.25), (10.49, 3.78), (11.73, 2.80),
    (13.20, 4.27), (12.22, 5.51),
];

fn shapes(glyph: Glyph) -> &'static [Shape] {
    match glyph {
        Glyph::ChevronLeft => &[Shape::Line(&[(10., 3.5), (5.5, 8.), (10., 12.5)], false)],
        Glyph::ChevronRight => &[Shape::Line(&[(6., 3.5), (10.5, 8.), (6., 12.5)], false)],
        Glyph::Plus => &[Shape::Line(&[(8., 3.), (8., 13.)], false), Shape::Line(&[(3., 8.), (13., 8.)], false)],
        Glyph::Gear => &[Shape::Line(GEAR_TEETH, true), Shape::Ring(8., 8., 2.1)],
        Glyph::Share => &[
            Shape::Ring(12., 3.8, 1.8),
            Shape::Ring(4., 8., 1.8),
            Shape::Ring(12., 12.2, 1.8),
            Shape::Line(&[(5.6, 7.1), (10.4, 4.7)], false),
            Shape::Line(&[(5.6, 8.9), (10.4, 11.3)], false),
        ],
    }
}

/// A glyph in a `size` × `size` box, stroked in `colour`. `stroke` is the prototype's stroke width
/// in its 16-unit viewBox, so it scales with the box.
pub fn glyph(glyph: Glyph, size: f32, stroke: f32, colour: Hsla) -> impl IntoElement {
    let paint_glyph = canvas(
        |_, _, _| {},
        move |bounds, _, window, _| paint(shapes(glyph), bounds, stroke, colour, window),
    );
    div().w(px(size)).h(px(size)).flex_none().child(paint_glyph.size_full())
}

fn paint(shapes: &[Shape], bounds: Bounds<Pixels>, stroke: f32, colour: Hsla, window: &mut Window) {
    let scale = f32::from(bounds.size.width) / 16.;
    let at = |x: f32, y: f32| -> Point<Pixels> {
        point(bounds.origin.x + gpui::px(x * scale), bounds.origin.y + gpui::px(y * scale))
    };
    let width = gpui::px(stroke * scale);
    for shape in shapes {
        let mut path = PathBuilder::stroke(width);
        match shape {
            Shape::Line(points, closed) => {
                let (first, rest) = points.split_first().expect("a line has points");
                path.move_to(at(first.0, first.1));
                for &(x, y) in rest {
                    path.line_to(at(x, y));
                }
                if *closed {
                    path.close();
                }
            }
            Shape::Ring(x, y, r) => {
                let centre = at(*x, *y);
                let radius = gpui::px(r * scale);
                let west = point(centre.x - radius, centre.y);
                let east = point(centre.x + radius, centre.y);
                path.move_to(west);
                path.arc_to(point(radius, radius), px(0.), false, true, east);
                path.arc_to(point(radius, radius), px(0.), false, true, west);
                path.close();
            }
        }
        if let Ok(built) = path.build() {
            window.paint_path(built, colour);
        }
    }
}
