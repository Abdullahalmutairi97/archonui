//! Charts as shapes in a `w` × `h` box: the same list is painted on screen and written out as SVG,
//! so a copied chart is the chart on screen. The layout follows the prototype's viz.js: margins, grid,
//! band widths, slice radii and legend rows. Pure, so the geometry is tested without GPUI.

use super::spec::{fmt, nice_max, Cell, Pie, Scatter, Stats, Table, View, Xy};
use crate::ui::theme::{mix, Rgba, Theme};
use std::f32::consts::PI;

/// The colours a chart draws with, from the theme.
#[derive(Debug, Clone, PartialEq)]
pub struct Ink {
    pub bg: Rgba,
    pub fg: Rgba,
    pub muted: Rgba,
    pub line: Rgba,
    pub line2: Rgba,
    /// Series colours in order: accent 2, accent, alt, accent 2 toward the background, accent toward
    /// the text, muted.
    pub pal: [Rgba; 6],
    pub ui_font: String,
    pub display_font: String,
    pub code_font: String,
}

impl Ink {
    pub fn of(t: &Theme) -> Self {
        Self {
            bg: t.bg,
            fg: t.fg,
            muted: t.muted,
            line: t.line,
            line2: t.line2,
            pal: [t.acc2, t.acc, t.alt, mix(t.acc2, t.bg, 0.45), mix(t.acc, t.fg, 0.45), t.muted],
            ui_font: t.chat_font.clone(),
            display_font: t.display_font.clone(),
            code_font: t.code_font.clone(),
        }
    }

    pub fn series(&self, i: usize) -> Rgba {
        self.pal[i % self.pal.len()]
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Anchor {
    Start,
    Middle,
    End,
}

/// One thing to draw. Colours carry their own alpha; `alpha` fades a whole shape.
#[derive(Debug, Clone, PartialEq)]
pub enum Shape {
    Line { a: (f32, f32), b: (f32, f32), color: Rgba, width: f32, dashed: bool },
    Rect { x: f32, y: f32, w: f32, h: f32, radius: f32, color: Rgba, alpha: f32 },
    /// A polygon or polyline, filled, stroked or both.
    Poly { points: Vec<(f32, f32)>, closed: bool, fill: Option<Rgba>, stroke: Option<(Rgba, f32)>, alpha: f32 },
    /// A closed polygon filled with `color` fading from `top` alpha at `y0` to `bottom` at `y1`.
    Fade { points: Vec<(f32, f32)>, color: Rgba, top: f32, bottom: f32, y0: f32, y1: f32 },
    Circle { c: (f32, f32), r: f32, fill: Rgba, stroke: Option<(Rgba, f32)> },
    /// Text whose baseline sits at `y`.
    Text { x: f32, y: f32, text: String, size: f32, color: Rgba, anchor: Anchor, display: bool, upright: bool },
}

/// The XY plot's margins: left, right, top, bottom.
const XY_PAD: (f32, f32, f32, f32) = (52., 18., 14., 34.);
const SCATTER_PAD: (f32, f32, f32, f32) = (52., 18., 14., 40.);

fn text(x: f32, y: f32, s: impl Into<String>, size: f32, color: Rgba, anchor: Anchor) -> Shape {
    Shape::Text { x, y, text: s.into(), size, color, anchor, display: false, upright: true }
}

/// The value at the top of an XY plot's axis.
fn xy_max(xy: &Xy, stacked: bool) -> f64 {
    let peak = (0..xy.x.len())
        .map(|i| {
            let at = |s: &super::spec::Series| s.data.get(i).copied().unwrap_or(0.);
            if stacked { xy.series.iter().map(at).sum() } else { xy.series.iter().map(at).fold(f64::MIN, f64::max) }
        })
        .fold(f64::MIN, f64::max);
    nice_max(peak)
}

/// Bars, lines or areas. `hi` is the hovered x position.
pub fn xy(xy: &Xy, view: View, w: f32, h: f32, ink: &Ink, hi: Option<usize>) -> Vec<Shape> {
    let (l, r, t, b) = XY_PAD;
    let (iw, ih) = ((w - l - r).max(1.), (h - t - b).max(1.));
    let n = xy.x.len().max(1);
    let stacked = xy.stacked && view == View::Bar;
    let max = xy_max(xy, stacked);
    let hi = hi.filter(|&i| i < xy.x.len());
    let yv = |v: f64| t + ih - (v / max) as f32 * ih;
    let band = iw / n as f32;
    let cx = |i: usize| l + band * i as f32 + band / 2.;
    let mut out = Vec::new();
    for k in [0., 0.25, 0.5, 0.75, 1.] {
        let y = t + ih - k * ih;
        out.push(Shape::Line { a: (l, y), b: (w - r, y), color: ink.line2, width: 1., dashed: k > 0. });
        out.push(text(l - 10., y + 4., fmt(max * k as f64), 11., ink.muted, Anchor::End));
    }
    let step = (n as f32 / (iw / 64.).floor().max(1.)).ceil().max(1.) as usize;
    for (i, label) in xy.x.iter().enumerate() {
        if i % step == 0 {
            out.push(text(cx(i), h - 12., label.clone(), 11., if hi == Some(i) { ink.fg } else { ink.muted }, Anchor::Middle));
        }
    }
    if let Some(i) = hi {
        out.push(Shape::Rect { x: l + band * i as f32, y: t, w: band, h: ih, radius: 4., color: ink.line, alpha: 1. });
    }
    if view == View::Bar {
        let gw = band * 0.66;
        let bw = if stacked { gw } else { gw / xy.series.len().max(1) as f32 };
        for i in 0..xy.x.len() {
            let mut acc = 0.;
            for (j, s) in xy.series.iter().enumerate() {
                let v = s.data.get(i).copied().unwrap_or(0.).max(0.);
                let x = if stacked { cx(i) - gw / 2. } else { cx(i) - gw / 2. + j as f32 * bw };
                let (y0, hh) = if stacked { (yv(acc + v), yv(acc) - yv(acc + v)) } else { (yv(v), t + ih - yv(v)) };
                acc += v;
                out.push(Shape::Rect {
                    x: x + if stacked { 0. } else { 1. },
                    y: y0,
                    w: (bw - if stacked { 0. } else { 2. }).max(1.),
                    h: hh.max(0.),
                    radius: if stacked { 0. } else { 3. },
                    color: ink.series(j),
                    alpha: if hi.is_none() || hi == Some(i) { 1. } else { 0.45 },
                });
            }
        }
    } else {
        for (j, s) in xy.series.iter().enumerate() {
            let pts: Vec<(f32, f32)> = (0..xy.x.len()).map(|i| (cx(i), yv(s.data.get(i).copied().unwrap_or(0.)))).collect();
            let c = ink.series(j);
            if view == View::Area && pts.len() > 1 {
                let mut area = pts.clone();
                area.push((pts[pts.len() - 1].0, t + ih));
                area.push((pts[0].0, t + ih));
                out.push(Shape::Fade { points: area, color: c, top: 0.38, bottom: 0., y0: t, y1: t + ih });
            }
            out.push(Shape::Poly { points: pts.clone(), closed: false, fill: None, stroke: Some((c, 2.2)), alpha: 1. });
            if let Some(&p) = hi.and_then(|i| pts.get(i)) {
                out.push(Shape::Circle { c: p, r: 4.5, fill: ink.bg, stroke: Some((c, 2.2)) });
            }
        }
    }
    out
}

/// The x position under `x` on an XY plot `w` wide with `n` positions.
pub fn xy_hit(n: usize, w: f32, x: f32) -> Option<usize> {
    let (l, r, _, _) = XY_PAD;
    let iw = w - l - r;
    if n == 0 || x < l || x >= w - r || iw <= 0. {
        return None;
    }
    Some((((x - l) / (iw / n as f32)) as usize).min(n - 1))
}

/// The x of position `i`'s centre, for placing its tooltip.
pub fn xy_centre(n: usize, w: f32, i: usize) -> f32 {
    let (l, r, _, _) = XY_PAD;
    let band = (w - l - r) / n.max(1) as f32;
    l + band * i as f32 + band / 2.
}

/// The pie's centre, outer and inner radius in a `w` × `h` box.
pub fn pie_geometry(donut: bool, w: f32, h: f32) -> ((f32, f32), f32, f32) {
    let r = (h / 2. - 16.).min(130.).max(10.);
    let cx = (w * 0.32).min(r + 30.);
    ((cx, h / 2.), r, if donut { r * 0.62 } else { 0. })
}

/// The arc from `a0` to `a1` at radius `r` about `c`, as points.
fn arc(c: (f32, f32), r: f32, a0: f32, a1: f32) -> Vec<(f32, f32)> {
    let steps = (((a1 - a0).abs() / (PI / 90.)).ceil() as usize).max(2);
    (0..=steps).map(|k| {
        let a = a0 + (a1 - a0) * k as f32 / steps as f32;
        (c.0 + a.cos() * r, c.1 + a.sin() * r)
    }).collect()
}

/// Pie or donut slices, the donut's centre figure, and the legend. `hi` is the hovered slice.
pub fn pie(p: &Pie, donut: bool, w: f32, h: f32, ink: &Ink, hi: Option<usize>) -> Vec<Shape> {
    let total: f64 = p.data.iter().map(|d| d.1).sum::<f64>().max(f64::MIN_POSITIVE);
    let (c, r, ir) = pie_geometry(donut, w, h);
    let hi = hi.filter(|&i| i < p.data.len());
    let mut out = Vec::new();
    let mut a0 = -PI / 2.;
    for (i, (_, v)) in p.data.iter().enumerate() {
        let a1 = a0 + (*v / total) as f32 * PI * 2.;
        let rr = r + if hi == Some(i) { 6. } else { 0. };
        let mut pts = arc(c, rr, a0, a1);
        if ir > 0. {
            let mut inner = arc(c, ir, a0, a1);
            inner.reverse();
            pts.extend(inner);
        } else {
            pts.push(c);
        }
        out.push(Shape::Poly {
            points: pts,
            closed: true,
            fill: Some(ink.series(i)),
            stroke: Some((ink.bg, 2.)),
            alpha: if hi.is_none() || hi == Some(i) { 1. } else { 0.5 },
        });
        a0 = a1;
    }
    if donut {
        let (value, label) = match hi.and_then(|i| p.data.get(i)) {
            Some((l, v)) => (format!("{}{}", fmt(*v), p.unit), format!("{}% · {l}", (v / total * 100.).round())),
            None => (format!("{}{}", fmt(total), p.unit), "Total".to_string()),
        };
        out.push(Shape::Text { x: c.0, y: c.1 + 2., text: value, size: 24., color: ink.fg, anchor: Anchor::Middle, display: true, upright: true });
        out.push(text(c.0, c.1 + 22., label, 11., ink.muted, Anchor::Middle));
    }
    let lx = c.0 + r + 40.;
    let top = c.1 - p.data.len() as f32 * 13.;
    for (i, (label, v)) in p.data.iter().enumerate() {
        let y = top + i as f32 * 26.;
        out.push(Shape::Rect { x: lx, y: y - 5., w: 10., h: 10., radius: 2., color: ink.series(i), alpha: 1. });
        out.push(text(lx + 18., y + 4., label.clone(), 12.5, if hi == Some(i) { ink.fg } else { ink.muted }, Anchor::Start));
        out.push(text(w - 16., y + 4., format!("{}%", (v / total * 100.).round()), 12.5, ink.fg, Anchor::End));
    }
    out
}

/// The slice under (`x`, `y`): on the ring itself, or on its legend row.
pub fn pie_hit(p: &Pie, donut: bool, w: f32, h: f32, x: f32, y: f32) -> Option<usize> {
    let (c, r, ir) = pie_geometry(donut, w, h);
    let lx = c.0 + r + 40.;
    if x >= lx - 4. {
        let top = c.1 - p.data.len() as f32 * 13.;
        let i = ((y - top + 13.) / 26.).floor();
        return (i >= 0. && (i as usize) < p.data.len()).then_some(i as usize);
    }
    let (dx, dy) = (x - c.0, y - c.1);
    let d = (dx * dx + dy * dy).sqrt();
    if d > r + 6. || d < ir {
        return None;
    }
    let total: f64 = p.data.iter().map(|d| d.1).sum();
    // Clockwise from twelve o'clock, as the slices are laid.
    let mut a = dy.atan2(dx) + PI / 2.;
    if a < 0. {
        a += PI * 2.;
    }
    let mut acc = 0.;
    for (i, (_, v)) in p.data.iter().enumerate() {
        acc += (*v / total) as f32 * PI * 2.;
        if a <= acc {
            return Some(i);
        }
    }
    p.data.len().checked_sub(1)
}

fn scatter_axes(s: &Scatter) -> (f64, f64) {
    let all = s.series.iter().flat_map(|x| &x.points);
    let xm = nice_max(all.clone().map(|p| p.0).fold(f64::MIN, f64::max));
    let ym = nice_max(all.map(|p| p.1).fold(f64::MIN, f64::max));
    (xm, ym)
}

/// Where point `p` sits on a scatter plot.
pub fn scatter_at(s: &Scatter, w: f32, h: f32, p: (f64, f64)) -> (f32, f32) {
    let (l, r, t, b) = SCATTER_PAD;
    let (iw, ih) = (w - l - r, h - t - b);
    let (xm, ym) = scatter_axes(s);
    (l + (p.0 / xm) as f32 * iw, t + ih - (p.1 / ym) as f32 * ih)
}

/// A scatter plot. `hi` is the hovered series and point.
pub fn scatter(s: &Scatter, w: f32, h: f32, ink: &Ink, hi: Option<(usize, usize)>) -> Vec<Shape> {
    let (l, r, t, b) = SCATTER_PAD;
    let (iw, ih) = (w - l - r, h - t - b);
    let (xm, ym) = scatter_axes(s);
    let mut out = Vec::new();
    for k in [0., 0.25, 0.5, 0.75, 1.] {
        let y = t + ih - k * ih;
        out.push(Shape::Line { a: (l, y), b: (w - r, y), color: ink.line2, width: 1., dashed: k > 0. });
        out.push(text(l - 10., y + 4., fmt(ym * k as f64), 11., ink.muted, Anchor::End));
        out.push(text(l + k * iw, t + ih + 16., fmt(xm * k as f64), 11., ink.muted, Anchor::Middle));
    }
    if !s.x_label.is_empty() {
        out.push(text(l + iw / 2., h - 4., s.x_label.clone(), 11., ink.muted, Anchor::Middle));
    }
    if !s.y_label.is_empty() {
        out.push(Shape::Text { x: 12., y: t + ih / 2., text: s.y_label.clone(), size: 11., color: ink.muted, anchor: Anchor::Middle, display: false, upright: false });
    }
    for (j, ser) in s.series.iter().enumerate() {
        for (i, p) in ser.points.iter().enumerate() {
            let on = hi == Some((j, i));
            let colour = ink.series(j);
            out.push(Shape::Circle {
                c: scatter_at(s, w, h, (p.0, p.1)),
                r: if on { 7. } else { 5. },
                fill: if on { colour } else { colour.alpha(colour.a * 0.75) },
                stroke: on.then_some((ink.fg, 1.5)),
            });
        }
    }
    out
}

/// The point under (`x`, `y`), within 9 px.
pub fn scatter_hit(s: &Scatter, w: f32, h: f32, x: f32, y: f32) -> Option<(usize, usize)> {
    let mut best: Option<((usize, usize), f32)> = None;
    for (j, ser) in s.series.iter().enumerate() {
        for (i, p) in ser.points.iter().enumerate() {
            let (px, py) = scatter_at(s, w, h, (p.0, p.1));
            let d = ((px - x).powi(2) + (py - y).powi(2)).sqrt();
            if d <= 9. && best.is_none_or(|(_, bd)| d < bd) {
                best = Some(((j, i), d));
            }
        }
    }
    best.map(|b| b.0)
}

/// A table as shapes, for copying it as an image: a header row and rows 34 px apart.
pub fn table(t: &Table, w: f32, ink: &Ink) -> (Vec<Shape>, f32) {
    let row_h = 34.;
    let cols = t.columns.len().max(1);
    let cw = (w - 24.) / cols as f32;
    let mut out = Vec::new();
    let x_of = |c: usize, numeric: bool| if numeric { 12. + cw * (c + 1) as f32 - 12. } else { 12. + cw * c as f32 + 12. };
    for (c, name) in t.columns.iter().enumerate() {
        let n = t.numeric(c);
        out.push(text(x_of(c, n), 22., name.clone(), 12., ink.muted, if n { Anchor::End } else { Anchor::Start }));
    }
    out.push(Shape::Line { a: (12., row_h), b: (w - 12., row_h), color: ink.line2, width: 1., dashed: false });
    for (r, row) in t.rows.iter().enumerate() {
        let y = row_h * (r + 1) as f32;
        for c in 0..cols {
            let n = t.numeric(c);
            let s = super::spec::cell_text(row.get(c));
            out.push(text(x_of(c, n), y + 22., s, if n { 12.5 } else { 13. }, ink.fg, if n { Anchor::End } else { Anchor::Start }));
        }
        out.push(Shape::Line { a: (12., y + row_h), b: (w - 12., y + row_h), color: ink.line, width: 1., dashed: false });
    }
    (out, row_h * (t.rows.len() + 1) as f32 + 8.)
}

/// The sparkline of a stat, in a `w` × 28 box whose top is `y`.
pub fn sparkline(trend: &[f64], x: f32, y: f32, w: f32) -> Vec<(f32, f32)> {
    if trend.len() < 2 {
        return Vec::new();
    }
    let (mn, mx) = trend.iter().fold((f64::MAX, f64::MIN), |(a, b), v| (a.min(*v), b.max(*v)));
    let span = if mx - mn == 0. { 1. } else { mx - mn };
    trend
        .iter()
        .enumerate()
        .map(|(k, v)| (x + k as f32 / (trend.len() - 1) as f32 * w, y + 28. - ((v - mn) / span) as f32 * 24.))
        .collect()
}

/// Stats as shapes, for copying them as an image: tiles 160 px wide in rows.
pub fn stats(s: &Stats, w: f32, ink: &Ink) -> (Vec<Shape>, f32) {
    let (tile_w, tile_h, gap) = (160_f32, 104_f32, 10_f32);
    let per_row = (((w - 24. + gap) / (tile_w + gap)).floor() as usize).max(1);
    let mut out = Vec::new();
    for (i, item) in s.items.iter().enumerate() {
        let (x, y) = (12. + (i % per_row) as f32 * (tile_w + gap), 12. + (i / per_row) as f32 * (tile_h + gap));
        out.push(Shape::Rect { x, y, w: tile_w, h: tile_h, radius: 8., color: ink.line, alpha: 1. });
        out.push(text(x + 14., y + 24., item.label.clone(), 12., ink.muted, Anchor::Start));
        out.push(Shape::Text { x: x + 14., y: y + 56., text: item.value.clone(), size: 28., color: ink.fg, anchor: Anchor::Start, display: true, upright: true });
        let good = item.good();
        if !item.delta.is_empty() {
            out.push(text(x + tile_w - 14., y + 56., item.delta.clone(), 12., if good { crate::ui::theme::GREEN } else { crate::ui::theme::RED }, Anchor::End));
        }
        let line = sparkline(&item.trend, x + 14., y + 66., tile_w - 28.);
        if !line.is_empty() {
            out.push(Shape::Poly { points: line, closed: false, fill: None, stroke: Some((if good { ink.pal[0] } else { crate::ui::theme::RED }, 1.6)), alpha: 1. });
        }
    }
    let rows = s.items.len().div_ceil(per_row).max(1);
    (out, 12. + rows as f32 * (tile_h + gap) + 2.)
}

fn css(c: Rgba) -> String {
    format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b)
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// The shapes as an SVG document `w` × `h`, on the theme's background.
pub fn to_svg(shapes: &[Shape], w: f32, h: f32, ink: &Ink) -> String {
    let mut s = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}" font-family="{}"><rect width="{w}" height="{h}" fill="{}"/>"#,
        esc(&ink.ui_font),
        css(ink.bg)
    );
    let mut gradients = 0;
    for shape in shapes {
        match shape {
            Shape::Line { a, b, color, width, dashed } => s += &format!(
                r#"<line x1="{:.1}" y1="{:.1}" x2="{:.1}" y2="{:.1}" stroke="{}" stroke-opacity="{}" stroke-width="{width}"{}/>"#,
                a.0, a.1, b.0, b.1, css(*color), color.a, if *dashed { r#" stroke-dasharray="2 4""# } else { "" }
            ),
            Shape::Rect { x, y, w, h, radius, color, alpha } => s += &format!(
                r#"<rect x="{x:.1}" y="{y:.1}" width="{w:.1}" height="{h:.1}" rx="{radius}" fill="{}" fill-opacity="{}"/>"#,
                css(*color), color.a * alpha
            ),
            Shape::Poly { points, closed, fill, stroke, alpha } => {
                let pts: Vec<String> = points.iter().map(|p| format!("{:.1},{:.1}", p.0, p.1)).collect();
                let tag = if *closed { "polygon" } else { "polyline" };
                let fill = fill.map_or("fill=\"none\"".to_string(), |f| format!(r#"fill="{}" fill-opacity="{}""#, css(f), f.a));
                let stroke = stroke.map_or(String::new(), |(c, w)| format!(r#" stroke="{}" stroke-opacity="{}" stroke-width="{w}" stroke-linejoin="round" stroke-linecap="round""#, css(c), c.a));
                s += &format!(r#"<{tag} points="{}" {fill}{stroke} opacity="{alpha}"/>"#, pts.join(" "));
            }
            Shape::Fade { points, color, top, bottom, y0, y1 } => {
                gradients += 1;
                let pts: Vec<String> = points.iter().map(|p| format!("{:.1},{:.1}", p.0, p.1)).collect();
                s += &format!(
                    r#"<defs><linearGradient id="f{gradients}" gradientUnits="userSpaceOnUse" x1="0" y1="{y0}" x2="0" y2="{y1}"><stop offset="0" stop-color="{c}" stop-opacity="{top}"/><stop offset="1" stop-color="{c}" stop-opacity="{bottom}"/></linearGradient></defs><polygon points="{}" fill="url(#f{gradients})"/>"#,
                    pts.join(" "),
                    c = css(*color)
                );
            }
            Shape::Circle { c, r, fill, stroke } => {
                let stroke = stroke.map_or(String::new(), |(sc, w)| format!(r#" stroke="{}" stroke-width="{w}""#, css(sc)));
                s += &format!(r#"<circle cx="{:.1}" cy="{:.1}" r="{r}" fill="{}" fill-opacity="{}"{stroke}/>"#, c.0, c.1, css(*fill), fill.a);
            }
            Shape::Text { x, y, text, size, color, anchor, display, upright } => {
                let anchor = match anchor {
                    Anchor::Start => "start",
                    Anchor::Middle => "middle",
                    Anchor::End => "end",
                };
                let font = if *display { format!(r#" font-family="{}""#, esc(&ink.display_font)) } else { String::new() };
                let turn = if *upright { String::new() } else { format!(r#" transform="rotate(-90 {x} {y})""#) };
                s += &format!(
                    r#"<text x="{x:.1}" y="{y:.1}" font-size="{size}" fill="{}" text-anchor="{anchor}"{font}{turn}>{}</text>"#,
                    css(*color),
                    esc(text)
                );
            }
        }
    }
    s + "</svg>"
}

/// `svg` drawn at twice its size as PNG bytes, on its own background.
pub fn svg_to_png(svg: &str) -> Result<Vec<u8>, String> {
    use std::sync::LazyLock;
    static FONTS: LazyLock<std::sync::Arc<resvg::usvg::fontdb::Database>> = LazyLock::new(|| {
        let mut db = resvg::usvg::fontdb::Database::new();
        db.load_system_fonts();
        crate::ui::fonts::load_bundled_into(&mut db);
        std::sync::Arc::new(db)
    });
    let options = resvg::usvg::Options { fontdb: FONTS.clone(), ..Default::default() };
    let tree = resvg::usvg::Tree::from_str(svg, &options).map_err(|e| e.to_string())?;
    let size = tree.size().to_int_size().scale_by(2.).ok_or("Too small to draw")?;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(size.width(), size.height()).ok_or("Too large to draw")?;
    resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(2., 2.), &mut pixmap.as_mut());
    pixmap.encode_png().map_err(|e| e.to_string())
}

/// The number shown in a table cell, as text.
pub fn cell(c: &Cell) -> String {
    super::spec::cell_text(Some(c))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::stream::viz::spec::{Series, Spec};

    fn ink() -> Ink {
        let w = Rgba::hex(0xffffff);
        Ink { bg: Rgba::hex(0), fg: w, muted: w, line: w, line2: w, pal: [w; 6], ui_font: "Geist".into(), display_font: "Cinzel".into(), code_font: "Geist Mono".into() }
    }

    fn sample() -> Xy {
        Xy { kind: View::Bar, title: "t".into(), unit: String::new(), x_label: String::new(), x: vec!["a".into(), "b".into()], series: vec![Series { name: "s".into(), data: vec![40., 98.] }], stacked: false }
    }

    #[test]
    fn bars_reach_the_value_on_a_nice_axis() {
        let shapes = xy(&sample(), View::Bar, 720., 260., &ink(), None);
        let bars: Vec<_> = shapes.iter().filter_map(|s| match s { Shape::Rect { h, .. } => Some(*h), _ => None }).collect();
        // The axis tops out at 100 over a 212 px plot: 98 is 207.76 px, 40 is 84.8.
        assert_eq!(bars.len(), 2);
        assert!((bars[1] - 207.76).abs() < 0.01 && (bars[0] - 84.8).abs() < 0.01, "{bars:?}");
        assert_eq!(xy_hit(2, 720., 60.), Some(0));
        assert_eq!(xy_hit(2, 720., 700.), Some(1));
        assert_eq!(xy_hit(2, 720., 10.), None);
    }

    #[test]
    fn stacked_bars_add_up() {
        let mut s = sample();
        s.stacked = true;
        s.series.push(Series { name: "u".into(), data: vec![10., 2.] });
        let shapes = xy(&s, View::Bar, 720., 260., &ink(), Some(1));
        assert_eq!(xy(&s, View::Bar, 720., 260., &ink(), Some(9)), xy(&s, View::Bar, 720., 260., &ink(), None), "a stale hover draws nothing");
        let total: f32 = shapes.iter().filter_map(|x| match x { Shape::Rect { h, radius, .. } if *radius == 0. => Some(*h), _ => None }).skip(2).sum();
        assert!((total - 212.).abs() < 0.01, "100 of 100 fills the plot: {total}");
    }

    #[test]
    fn slices_are_found_where_they_are_drawn() {
        let Spec::Pie(p) = Spec::parse(r#"{"type":"donut","data":[{"label":"a","value":3},{"label":"b","value":1}]}"#).unwrap() else { panic!() };
        let ((cx, cy), r, ir) = pie_geometry(true, 720., 240.);
        let mid = (r + ir) / 2.;
        // a fills the first three quarters clockwise from the top; b the last, at the upper left.
        assert_eq!(pie_hit(&p, true, 720., 240., cx + mid, cy), Some(0));
        assert_eq!(pie_hit(&p, true, 720., 240., cx - mid * 0.7, cy - mid * 0.7), Some(1));
        assert_eq!(pie_hit(&p, true, 720., 240., cx, cy), None, "the hole is empty");
        assert_eq!(pie_hit(&p, false, 720., 240., cx, cy + 1.), Some(0));
        // The legend rows answer too.
        let lx = cx + r + 40.;
        assert_eq!(pie_hit(&p, true, 720., 240., lx + 20., cy + 2.), Some(1));
        assert_eq!(pie_hit(&p, true, 720., 240., lx + 20., cy - 24.), Some(0));
    }

    #[test]
    fn scatter_points_are_hit_within_reach() {
        let Spec::Scatter(s) = Spec::parse(r#"{"type":"scatter","series":[{"name":"a","points":[[42,18],[9,41]]}]}"#).unwrap() else { panic!() };
        let (x, y) = scatter_at(&s, 720., 280., (9., 41.));
        assert_eq!(scatter_hit(&s, 720., 280., x + 3., y - 3.), Some((0, 1)));
        assert_eq!(scatter_hit(&s, 720., 280., x + 30., y), None);
    }

    #[test]
    fn svg_escapes_text_and_draws_on_the_background() {
        let shapes = vec![text(1., 2., "a < b & \"c\"", 11., Rgba::hex(0xffffff), Anchor::End)];
        let svg = to_svg(&shapes, 10., 20., &ink());
        assert!(svg.contains("a &lt; b &amp; &quot;c&quot;") && svg.contains(r##"fill="#000000""##) && svg.contains("text-anchor=\"end\""));
        let png = svg_to_png(&svg).unwrap();
        assert_eq!(&png[1..4], b"PNG");
    }
}
