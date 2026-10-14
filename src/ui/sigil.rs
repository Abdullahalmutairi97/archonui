//! Session sigils: 100 lattice-traced icons plus the status dot.
//! Spec: `docs/design/icons.md`; machine-readable twin `docs/design/icons.json`.

use std::collections::HashSet;
use std::sync::OnceLock;

use gpui::{
    Bounds, Hsla, IntoElement, PathBuilder, Pixels, Point, Window, canvas, div, point, prelude::*,
    };
use crate::ui::scale::px;

/// Lattice coordinate per axis; point `row * 3 + col` is `(LATTICE[col], LATTICE[row])`.
const LATTICE: [f64; 3] = [3.0, 8.0, 13.0];
/// Side of the square `viewBox` the sigils are drawn in.
const VIEW_BOX: f64 = 16.0;
/// Stroke width in `viewBox` units (`stroke-width="1.5"`).
const STROKE: f64 = 1.5;

/// End-marker shape. Circles and squares are stroked outlines, never filled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Marker {
    /// Circle, r = 0.9.
    Dot,
    /// Circle, r = 1.9.
    Ring,
    /// Square, 2.8 × 2.8.
    Square,
}

/// One session icon: a stroked path through the 3×3 lattice with end markers.
#[derive(Clone, Debug, PartialEq)]
pub struct Sigil {
    /// Lattice point indices in draw order (`row * 3 + col`, 0..9).
    pub path: Vec<u8>,
    /// Path ends with `Z` (variant 1 with more than three points).
    pub closed: bool,
    /// Generation index `% 5`; selects the markers.
    pub variant: u8,
    /// Extra r = 1.6 circle at `path[1]` (variant 0 only, coin flip).
    pub mid_ring: bool,
}

impl Sigil {
    /// Marker drawn at `path[0]`.
    pub fn start_marker(&self) -> Marker {
        match self.variant {
            2 => Marker::Ring,
            3 => Marker::Square,
            _ => Marker::Dot,
        }
    }

    /// SVG path `d` in the prototype's format; equals the `d` in `icons.json`.
    pub fn svg_d(&self) -> String {
        let body = self
            .path
            .iter()
            .map(|&i| {
                let (x, y) = lattice_point(i);
                format!("{x} {y}")
            })
            .collect::<Vec<_>>()
            .join("L");
        let mut d = format!("M{body}");
        if self.closed {
            d.push('Z');
        }
        let (sx, sy) = lattice_point(self.path[0]);
        d += &marker_d(self.start_marker(), sx, sy);
        if self.variant == 4 {
            let (ex, ey) = lattice_point(self.path[self.path.len() - 1]);
            d += &marker_d(Marker::Dot, ex, ey);
        }
        if self.mid_ring {
            let (mx, my) = lattice_point(self.path[1]);
            d += &circle_d(mx, my, 1.6);
        }
        d
    }
}

/// Viewbox coordinates of lattice point `i`.
fn lattice_point(i: u8) -> (f64, f64) {
    (LATTICE[usize::from(i % 3)], LATTICE[usize::from(i / 3)])
}

/// SVG subpath for a marker centred at `(x, y)`.
fn marker_d(marker: Marker, x: f64, y: f64) -> String {
    match marker {
        Marker::Dot => circle_d(x, y, 0.9),
        Marker::Ring => circle_d(x, y, 1.9),
        Marker::Square => format!("M{} {}h2.8v2.8h-2.8z", x - 1.4, y - 1.4),
    }
}

/// Full-circle SVG subpath built from two arcs, matching the prototype's string.
fn circle_d(x: f64, y: f64, r: f64) -> String {
    format!(
        "M{} {}a{r} {r} 0 1 0 {} 0a{r} {r} 0 1 0 {} 0",
        x - r,
        y,
        2.0 * r,
        -2.0 * r
    )
}

/// Park-Miller minimal standard generator, the prototype's `rnd()`.
struct Rng(u64);

impl Rng {
    /// Next draw in `[0, 1)`.
    fn next(&mut self) -> f64 {
        self.0 = self.0 * 16807 % 2_147_483_647;
        self.0 as f64 / 2_147_483_647.0
    }

    /// Integer in `0..n`, as `Math.floor(rnd() * n)`.
    fn below(&mut self, n: usize) -> usize {
        (self.next() * n as f64).floor() as usize
    }
}

/// Runs the prototype's `SIGILS` generator (seed 7). Draw order is part of the output.
fn generate() -> [Sigil; 100] {
    let mut rng = Rng(7);
    let mut out: Vec<Sigil> = Vec::with_capacity(100);
    let mut seen: HashSet<Vec<u8>> = HashSet::new();
    while out.len() < 100 {
        let len = 3 + rng.below(3);
        let mut path = vec![rng.below(9) as u8];
        while path.len() < len {
            let n = rng.below(9) as u8;
            if !path.contains(&n) {
                path.push(n);
            }
        }
        let mirror: Vec<u8> = path.iter().rev().copied().collect();
        if seen.contains(&path) || seen.contains(&mirror) {
            continue;
        }
        let variant = (out.len() % 5) as u8;
        let mid_ring = variant == 0 && rng.next() > 0.5;
        seen.insert(path.clone());
        out.push(Sigil {
            closed: len > 3 && variant == 1,
            path,
            variant,
            mid_ring,
        });
    }
    out.try_into().expect("generator yields exactly 100 sigils")
}

static SIGILS: OnceLock<[Sigil; 100]> = OnceLock::new();

/// All 100 sigils; a session's icon is an index into this array.
pub fn sigils() -> &'static [Sigil; 100] {
    SIGILS.get_or_init(generate)
}

/// Icon for a new session: the lowest unused index, else `taken.len() % 100`.
pub fn assign(taken: &[u16]) -> u16 {
    (0..100)
        .find(|k| !taken.contains(k))
        .unwrap_or(taken.len() as u16 % 100)
}

/// Stable icon for session `id`: FNV-1a hash mod 100, probing forward past `taken`.
/// Falls back to the hash index when all 100 are taken.
pub fn seed_icon_for(id: &str, taken: &[u16]) -> u16 {
    let start = (fnv1a(id.as_bytes()) % 100) as u16;
    (0..100)
        .map(|step| (start + step) % 100)
        .find(|k| !taken.contains(k))
        .unwrap_or(start)
}

/// FNV-1a, 64-bit.
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &b| {
        (hash ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// Session sigil `index % 100` at `size` × `size` in `color`, like the prototype's 16×16 SVG.
pub fn sigil_element(index: u16, size: Pixels, color: Hsla) -> impl IntoElement {
    let sigil: &'static Sigil = &sigils()[usize::from(index % 100)];
    canvas(
        |_, _, _| {},
        move |bounds, _, window, _| paint_sigil(sigil, bounds, color, window),
    )
    .w(size)
    .h(size)
}

/// Maps viewBox units onto a canvas.
struct ViewBox {
    origin: Point<Pixels>,
    scale: f64,
}

impl ViewBox {
    fn at(&self, x: f64, y: f64) -> Point<Pixels> {
        point(
            self.origin.x + gpui::px((x * self.scale) as f32),
            self.origin.y + gpui::px((y * self.scale) as f32),
        )
    }

    fn len(&self, v: f64) -> Pixels {
        gpui::px((v * self.scale) as f32)
    }
}

fn paint_sigil(sigil: &Sigil, bounds: Bounds<Pixels>, color: Hsla, window: &mut Window) {
    let vp = window.viewport_size();
    if bounds.origin.x + bounds.size.width <= px(0.)
        || bounds.origin.x >= vp.width
        || bounds.origin.y + bounds.size.height <= px(0.)
        || bounds.origin.y >= vp.height
    {
        return;
    }
    let vb = ViewBox {
        origin: bounds.origin,
        scale: f64::from(bounds.size.width / gpui::px(1.0)) / VIEW_BOX,
    };
    let width = vb.len(STROKE);

    let points: Vec<Point<Pixels>> = sigil
        .path
        .iter()
        .map(|&i| {
            let (x, y) = lattice_point(i);
            vb.at(x, y)
        })
        .collect();
    stroke_polyline(window, &points, sigil.closed, width, color);

    let (sx, sy) = lattice_point(sigil.path[0]);
    match sigil.start_marker() {
        Marker::Dot => stroke_circle(window, vb.at(sx, sy), vb.len(0.9), width, color),
        Marker::Ring => stroke_circle(window, vb.at(sx, sy), vb.len(1.9), width, color),
        Marker::Square => {
            let h = 1.4;
            let corners =
                [(-h, -h), (h, -h), (h, h), (-h, h)].map(|(dx, dy)| vb.at(sx + dx, sy + dy));
            stroke_polyline(window, &corners, true, width, color);
        }
    }
    if sigil.variant == 4 {
        let (ex, ey) = lattice_point(sigil.path[sigil.path.len() - 1]);
        stroke_circle(window, vb.at(ex, ey), vb.len(0.9), width, color);
    }
    if sigil.mid_ring {
        let (mx, my) = lattice_point(sigil.path[1]);
        stroke_circle(window, vb.at(mx, my), vb.len(1.6), width, color);
    }
}

/// Strokes a polyline with round joins and caps. gpui's `PathBuilder` only offers miter/butt
/// (lyon's cap/join enums are not exported), so each segment is butt-capped and a disc sits at
/// every vertex; that union is exactly a round-joined stroke, with no miter spikes.
fn stroke_polyline(
    window: &mut Window,
    points: &[Point<Pixels>],
    closed: bool,
    width: Pixels,
    color: Hsla,
) {
    if points.is_empty() {
        return;
    }
    let closing = if closed {
        points.last().copied().zip(points.first().copied())
    } else {
        None
    };
    let mut segments = PathBuilder::stroke(width);
    for (a, b) in points.windows(2).map(|w| (w[0], w[1])).chain(closing) {
        segments.move_to(a);
        segments.line_to(b);
    }
    if let Ok(path) = segments.build() {
        window.paint_path(path, color);
    }
    let mut disc = PathBuilder::fill();
    for &p in points {
        add_circle(&mut disc, p, width * 0.5);
    }
    if let Ok(path) = disc.build() {
        window.paint_path(path, color);
    }
}

/// Stroked circle outline (ring), never filled.
fn stroke_circle(
    window: &mut Window,
    centre: Point<Pixels>,
    r: Pixels,
    width: Pixels,
    color: Hsla,
) {
    let mut ring = PathBuilder::stroke(width);
    add_circle(&mut ring, centre, r);
    if let Ok(path) = ring.build() {
        window.paint_path(path, color);
    }
}

/// Adds a closed circle subpath made of two half-arcs.
fn add_circle(builder: &mut PathBuilder, centre: Point<Pixels>, r: Pixels) {
    let west = point(centre.x - r, centre.y);
    let east = point(centre.x + r, centre.y);
    builder.move_to(west);
    builder.arc_to(point(r, r), px(0.), false, true, east);
    builder.arc_to(point(r, r), px(0.), false, true, west);
    builder.close();
}

/// Session run state shown by the status dot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionState {
    /// Session is running (`acc2` in the prototype).
    Running,
    /// Session finished (`alt`).
    Done,
    /// Neither running nor done (`faint`).
    Idle,
}

/// Status-dot colours. `surface` is the 2px ring that separates the dot from its background.
#[derive(Clone, Copy, Debug)]
pub struct StatusDotColours {
    pub running: Hsla,
    pub done: Hsla,
    pub idle: Hsla,
    pub surface: Hsla,
}

/// Status dot: 8px fill with a 2px `surface` ring (12px outer, as the prototype's content-box
/// sizing). The caller positions it at the sigil's bottom-right corner.
pub fn status_dot(state: SessionState, colours: &StatusDotColours) -> impl IntoElement {
    status_dot_sized(state, colours, 8., 2.)
}

/// A status dot whose fill is `inner` px across inside a `border` px ring of `colours.surface`.
pub fn status_dot_sized(state: SessionState, colours: &StatusDotColours, inner: f32, border: f32) -> impl IntoElement {
    let fill = match state {
        SessionState::Running => colours.running,
        SessionState::Done => colours.done,
        SessionState::Idle => colours.idle,
    };
    div()
        .size(px(inner + 2. * border))
        .rounded_full()
        .border(px(border))
        .border_color(colours.surface)
        .bg(fill)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn sigil_generator_matches_icons_json() {
        let text = fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/docs/design/icons.json"
        ))
        .unwrap();
        let json: serde_json::Value = serde_json::from_str(&text).unwrap();
        let patterns = json["patterns"].as_array().unwrap();
        assert_eq!(patterns.len(), 100);
        for (i, (sigil, pattern)) in sigils().iter().zip(patterns).enumerate() {
            assert_eq!(pattern["index"].as_u64(), Some(i as u64));
            let path: Vec<u64> = pattern["path"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_u64().unwrap())
                .collect();
            let ours: Vec<u64> = sigil.path.iter().map(|&p| u64::from(p)).collect();
            assert_eq!(ours, path, "path of sigil {i}");
            assert_eq!(sigil.closed, pattern["closed"].as_bool().unwrap(), "closed {i}");
            let variant = pattern["variant"].as_u64().unwrap();
            assert_eq!(u64::from(sigil.variant), variant, "variant {i}");
            assert_eq!(sigil.svg_d(), pattern["d"].as_str().unwrap(), "d of sigil {i}");
        }
    }

    #[test]
    fn sigil_paths_are_unique_up_to_reversal() {
        let mut seen = HashSet::new();
        for sigil in sigils() {
            let key: String = sigil.path.iter().map(|p| p.to_string()).collect();
            let mirror: String = key.chars().rev().collect();
            assert!(!seen.contains(&mirror) && seen.insert(key));
        }
    }

    #[test]
    fn sigil_assign_takes_first_unused_then_round_robin() {
        assert_eq!(assign(&[]), 0);
        assert_eq!(assign(&[0, 1, 3]), 2);
        let all: Vec<u16> = (0..100).collect();
        assert_eq!(assign(&all), 0);
        let overflow: Vec<u16> = (0..100).chain([5]).collect();
        assert_eq!(assign(&overflow), 1);
    }

    #[test]
    fn sigil_seed_icon_is_hash_based_and_probes_past_taken() {
        assert_eq!(fnv1a(b"a"), 0xaf63_dc4c_8601_ec8c);
        let start = (fnv1a(b"sess-1") % 100) as u16;
        assert_eq!(seed_icon_for("sess-1", &[]), start);
        assert_eq!(seed_icon_for("sess-1", &[start]), (start + 1) % 100);
        assert_eq!(seed_icon_for("sess-1", &[start, (start + 1) % 100]), (start + 2) % 100);
        let taken_tail: Vec<u16> = (start..100).collect();
        assert_eq!(seed_icon_for("sess-1", &taken_tail), 0);
    }

    #[test]
    fn sigil_seed_icon_is_collision_free_below_100_sessions() {
        let mut taken: Vec<u16> = Vec::new();
        for n in 0..100 {
            let icon = seed_icon_for(&format!("session-{n}"), &taken);
            assert!(!taken.contains(&icon), "session {n} collided");
            taken.push(icon);
        }
    }

    /// Writes all 100 sigils for side-by-side review with the prototype.
    /// Run with `cargo test sigil_sheet -- --ignored`.
    #[test]
    #[ignore = "writes .qa/design/sigils-port.svg for visual review"]
    fn sigil_sheet_svg() {
        const CELL: usize = 80;
        let mut svg = format!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="{0}" height="{0}" viewBox="0 0 {0} {0}"><rect width="{0}" height="{0}" fill="#000"/>"##,
            CELL * 10
        );
        for (i, sigil) in sigils().iter().enumerate() {
            let (col, row) = (i % 10, i / 10);
            svg += &format!(
                r##"<g transform="translate({} {}) scale(3)"><path d="{}" fill="none" stroke="#f3ead8" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/></g><text x="{}" y="{}" font-family="sans-serif" font-size="9" fill="#bfae94">{i}</text>"##,
                col * CELL + 16,
                row * CELL + 12,
                sigil.svg_d(),
                col * CELL + 4,
                row * CELL + 74
            );
        }
        svg += "</svg>\n";
        let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".qa/design/sigils-port.svg");
        fs::create_dir_all(out.parent().unwrap()).unwrap();
        fs::write(&out, svg).unwrap();
    }
}
