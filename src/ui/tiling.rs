//! Tiling model for the centre column: tile order, focus, split fractions and
//! divider geometry. Pure logic; the UI draws what `Layout::compute` returns.
//! Ported from `design/ArchonUI Agent v3.dc.html` (`renderVals`, `newSession`,
//! `openSession`, `closeTile`, the Alt+arrow handler). Spec: `docs/design/layout.md` §3.2.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Most tiles a layout holds; a split at the cap drops the focused tile first.
pub const MAX_TILES: usize = 6;
/// Narrowest tile `split_focused` and `drag` allow. Not in the prototype.
pub const MIN_TILE_W: f32 = 120.0;
/// Shortest tile `split_focused` and `drag` allow. Not in the prototype.
pub const MIN_TILE_H: f32 = 100.0;

const RATIO_MIN: f32 = 0.12;
const RATIO_MAX: f32 = 0.88;
const SPLIT_DEFAULT: f32 = 0.5;
const MASTER_DEFAULT: f32 = 0.56;
/// Bisection steps used to find the largest drag value that keeps tiles at minimum size.
const BISECT_STEPS: u32 = 30;
/// Minimum divider hit-box thickness, in px.
const HANDLE_MIN_THICK: f32 = 12.0;

/// Tile order, focus and sizing for the centre column.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Layout {
    /// Session ids in placement order.
    pub tiles: Vec<String>,
    /// Focused session id; `None` when nothing is focused.
    pub focus: Option<String>,
    pub mode: LayoutMode,
    /// Dwindle split fractions keyed by split index, not by tile. As in the prototype,
    /// inserting or closing a tile changes which tile a stored fraction applies to.
    pub ratios: BTreeMap<usize, f32>,
    /// Width fraction of the first tile in master mode.
    pub master_ratio: f32,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            tiles: Vec::new(),
            focus: None,
            mode: LayoutMode::default(),
            ratios: BTreeMap::new(),
            master_ratio: MASTER_DEFAULT,
        }
    }
}

/// `Dwindle` alternates split axis; `Master` keeps one large tile on the left.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LayoutMode {
    #[default]
    Dwindle,
    Master,
}

/// Divider orientation. `V` is a vertical divider between side-by-side tiles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    V,
    H,
}

/// Which divider a handle drives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandleKey {
    Split(usize),
    Master,
}

/// Alt+arrow direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arrow {
    Left,
    Right,
    Up,
    Down,
}

/// Axis-aligned rectangle in px.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// A draggable divider between tiles.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Handle {
    pub key: HandleKey,
    pub dir: Dir,
    /// Hit box, centred on the gutter.
    pub rect: Rect,
    /// Visible grip, centred in `rect`.
    pub grip: Rect,
    /// Rect being divided. Drag fractions are measured against it.
    pub parent: Rect,
}

impl Layout {
    /// Tile rects and dividers for `area` with `gap` px between tiles.
    ///
    /// Dwindle splits left/right while the remaining box is at least 0.9× as wide as
    /// tall, otherwise top/bottom. The last tile takes what is left. Master mode rounds
    /// the master width, as the prototype does.
    pub fn compute(&self, area: Rect, gap: f32) -> (Vec<(String, Rect)>, Vec<Handle>) {
        let n = self.tiles.len();
        let mut rects = Vec::with_capacity(n);
        let mut handles = Vec::new();
        if self.mode == LayoutMode::Master && n > 1 {
            // Sizes are floored at zero so an area smaller than the gaps never yields negative tiles.
            let mw = ((area.w - gap).max(0.0) * self.fraction(HandleKey::Master)).round();
            rects.push(Rect { x: area.x, y: area.y, w: mw, h: area.h });
            handles.push(handle(HandleKey::Master, Dir::V, area.x + mw, area, gap));
            let stack_h = (area.h - gap * (n as f32 - 2.0)).max(0.0) / (n as f32 - 1.0);
            for i in 1..n {
                let y = area.y + (i as f32 - 1.0) * (stack_h + gap);
                rects.push(Rect { x: area.x + mw + gap, y, w: (area.w - mw - gap).max(0.0), h: stack_h });
            }
        } else {
            let mut r = area;
            // Remaining-region estimate, used only to pick the split axis.
            let (mut er_w, mut er_h) = (area.w, area.h);
            for i in 0..n {
                if i == n - 1 {
                    rects.push(r);
                    break;
                }
                let k = self.fraction(HandleKey::Split(i));
                if er_w >= er_h * 0.9 {
                    er_w = (er_w - gap) / 2.0;
                    let avail = (r.w - gap).max(0.0);
                    let w = avail * k;
                    rects.push(Rect { w, ..r });
                    handles.push(handle(HandleKey::Split(i), Dir::V, r.x + w, r, gap));
                    r = Rect { x: r.x + w + gap, w: avail - w, ..r };
                } else {
                    er_h = (er_h - gap) / 2.0;
                    let avail = (r.h - gap).max(0.0);
                    let h = avail * k;
                    rects.push(Rect { h, ..r });
                    handles.push(handle(HandleKey::Split(i), Dir::H, r.y + h, r, gap));
                    r = Rect { y: r.y + h + gap, h: avail - h, ..r };
                }
            }
        }
        let tiles = self.tiles.iter().cloned().zip(rects).collect();
        (tiles, handles)
    }

    /// Whether `id` is tiled.
    pub fn contains(&self, id: &str) -> bool {
        self.tiles.iter().any(|t| t == id)
    }

    /// Index of the focused tile, if it is tiled.
    fn focus_index(&self) -> Option<usize> {
        self.focus.as_ref().and_then(|f| self.tiles.iter().position(|t| t == f))
    }

    /// Focuses `id` if it is tiled. Returns whether it was.
    pub fn focus(&mut self, id: &str) -> bool {
        if !self.contains(id) {
            return false;
        }
        self.focus = Some(id.to_owned());
        true
    }

    /// Alt+arrow focus move: Right/Down go to the next tile and Left/Up to the previous,
    /// wrapping. Follows tile order, not geometry, as the prototype does.
    pub fn focus_arrow(&mut self, arrow: Arrow) {
        let n = self.tiles.len() as isize;
        if n == 0 {
            return;
        }
        let step = match arrow {
            Arrow::Right | Arrow::Down => 1,
            Arrow::Left | Arrow::Up => -1,
        };
        // With nothing focused, forward starts at the first tile and back at the last.
        let i = self.focus_index().map_or(if step > 0 { -1 } else { n }, |i| i as isize);
        self.focus = Some(self.tiles[(i + step).rem_euclid(n) as usize].clone());
    }

    /// Adds `id` after the focused tile and focuses it (`newSession`).
    ///
    /// At the `MAX_TILES` cap the focused tile is dropped first. The prototype then looks up
    /// the focus index after the drop, so the new tile is appended at the end, not put in
    /// the dropped tile's slot. This port keeps that behaviour.
    ///
    /// Returns false and leaves the layout unchanged if `id` is already tiled or any tile
    /// in `area` would fall below `MIN_TILE_W`×`MIN_TILE_H`.
    pub fn split_focused(&mut self, id: &str, area: Rect, gap: f32) -> bool {
        if self.contains(id) {
            return false;
        }
        let mut next = self.clone();
        if next.tiles.len() >= MAX_TILES {
            // Drop the focused tile, or the last one when nothing tiled is focused, so the cap holds.
            let drop = next.focus_index().unwrap_or(next.tiles.len() - 1);
            next.tiles.remove(drop);
            next.tiles.truncate(MAX_TILES - 1);
        }
        let at = next.focus_index().map_or(next.tiles.len(), |i| i + 1);
        next.tiles.insert(at, id.to_owned());
        next.focus = Some(id.to_owned());
        if !next.fits(area, gap) {
            return false;
        }
        *self = next;
        true
    }

    /// Focuses `id` if it is already tiled, otherwise splits it in after the focused tile.
    pub fn open_in_new_split(&mut self, id: &str, area: Rect, gap: f32) -> bool {
        if self.contains(id) {
            self.focus = Some(id.to_owned());
            return true;
        }
        self.split_focused(id, area, gap)
    }

    /// Sidebar open (`openSession`). A tiled `id` is just focused. Otherwise `id` replaces
    /// the focused tile, or the last tile when nothing is focused. In an empty layout it
    /// becomes the only tile.
    pub fn open_in_focused(&mut self, id: &str) {
        if self.contains(id) {
            self.focus = Some(id.to_owned());
            return;
        }
        match self.focus_index().or(self.tiles.len().checked_sub(1)) {
            Some(i) => self.tiles[i] = id.to_owned(),
            None => self.tiles.push(id.to_owned()),
        }
        self.focus = Some(id.to_owned());
    }

    /// Removes `id` (`closeTile`). If it was focused, focus moves to the tile before it, or
    /// to the first tile. Closing an untiled id does nothing.
    pub fn close(&mut self, id: &str) {
        let Some(i) = self.tiles.iter().position(|t| t == id) else {
            return;
        };
        self.tiles.remove(i);
        if self.focus.as_deref() == Some(id) {
            self.focus = self.tiles.get(i.saturating_sub(1)).cloned();
        }
    }

    /// Moves a divider to `pointer`, in the same px space as `area`.
    ///
    /// The fraction is measured from `handle.parent`, as in the prototype's `startDrag`,
    /// and clamped to 0.12–0.88. If the result would leave a tile below the minimum size,
    /// it is moved back to the nearest value that fits. A layout already below the minimum
    /// can only move to a value that fits.
    pub fn drag(&mut self, handle: &Handle, pointer: (f32, f32), area: Rect, gap: f32) {
        let p = handle.parent;
        let raw = match handle.dir {
            Dir::V => (pointer.0 - p.x - gap / 2.0) / (p.w - gap),
            Dir::H => (pointer.1 - p.y - gap / 2.0) / (p.h - gap),
        };
        // A zero-length parent or a NaN pointer gives no position to move to.
        if raw.is_nan() {
            return;
        }
        let requested = raw.clamp(RATIO_MIN, RATIO_MAX);
        let current = self.fraction(handle.key);
        // A value is allowed when it leaves tiles no further below the minimum than they are now: in a
        // layout that fits, that means it still fits; in one already too small (the window shrank), a
        // divider can still move as long as it doesn't make things worse.
        let shortfall_at = |k: f32| {
            let mut next = self.clone();
            next.set_fraction(handle.key, k);
            next.shortfall(area, gap)
        };
        let base = self.shortfall(area, gap);
        let fits = |k: f32| shortfall_at(k) <= base + 1e-3;
        let k = if fits(requested) {
            requested
        } else {
            let (mut ok, mut bad) = (current, requested);
            for _ in 0..BISECT_STEPS {
                let mid = (ok + bad) / 2.0;
                if fits(mid) {
                    ok = mid;
                } else {
                    bad = mid;
                }
            }
            ok
        };
        if k != current {
            self.set_fraction(handle.key, k);
        }
    }

    /// Double-click reset: a split goes to 0.5, the master tile to 0.56.
    pub fn reset(&mut self, handle: &Handle) {
        match handle.key {
            HandleKey::Split(i) => {
                self.ratios.insert(i, SPLIT_DEFAULT);
            }
            HandleKey::Master => self.master_ratio = MASTER_DEFAULT,
        }
    }

    /// "Reset sizes" in settings: clears every split fraction and the master fraction.
    pub fn reset_all(&mut self) {
        self.ratios.clear();
        self.master_ratio = MASTER_DEFAULT;
    }

    /// Repairs a layout loaded from disk: duplicate tiles are dropped, at most `MAX_TILES` are kept, and
    /// fractions outside 0.12–0.88 (or not numbers) are clamped or reset.
    pub fn normalize(&mut self) {
        let mut seen = std::collections::HashSet::new();
        self.tiles.retain(|t| seen.insert(t.clone()));
        self.tiles.truncate(MAX_TILES);
        self.ratios.retain(|_, k| k.is_finite());
        for k in self.ratios.values_mut() {
            *k = k.clamp(RATIO_MIN, RATIO_MAX);
        }
        self.master_ratio = self.fraction(HandleKey::Master);
    }

    /// Whether every tile in `area` meets the minimum size.
    fn fits(&self, area: Rect, gap: f32) -> bool {
        self.compute(area, gap).0.iter().all(|(_, r)| r.w >= MIN_TILE_W && r.h >= MIN_TILE_H)
    }

    /// How far, in px summed over tiles and axes, the tiles in `area` fall below the minimum size.
    fn shortfall(&self, area: Rect, gap: f32) -> f32 {
        self.compute(area, gap)
            .0
            .iter()
            .map(|(_, r)| (MIN_TILE_W - r.w).max(0.0) + (MIN_TILE_H - r.h).max(0.0))
            .sum()
    }

    /// The fraction a divider uses, kept in 0.12–0.88 so a bad saved value can't produce
    /// negative or zero-size tiles.
    fn fraction(&self, key: HandleKey) -> f32 {
        let (k, default) = match key {
            HandleKey::Split(i) => (self.ratios.get(&i).copied().unwrap_or(SPLIT_DEFAULT), SPLIT_DEFAULT),
            HandleKey::Master => (self.master_ratio, MASTER_DEFAULT),
        };
        if k.is_finite() {
            k.clamp(RATIO_MIN, RATIO_MAX)
        } else {
            default
        }
    }

    fn set_fraction(&mut self, key: HandleKey, k: f32) {
        match key {
            HandleKey::Split(i) => {
                self.ratios.insert(i, k);
            }
            HandleKey::Master => self.master_ratio = k,
        }
    }
}

/// Divider for the gutter starting at `gutter` (x for `V`, y for `H`) inside `parent`.
fn handle(key: HandleKey, dir: Dir, gutter: f32, parent: Rect, gap: f32) -> Handle {
    let thick = gap.max(HANDLE_MIN_THICK);
    let centre = gutter + gap / 2.0;
    let (rect, grip) = match dir {
        Dir::V => {
            let rect = Rect { x: centre - thick / 2.0, y: parent.y, w: thick, h: parent.h };
            (rect, centred(rect, 3.0, 44.0))
        }
        Dir::H => {
            let rect = Rect { x: parent.x, y: centre - thick / 2.0, w: parent.w, h: thick };
            (rect, centred(rect, 44.0, 3.0))
        }
    };
    Handle { key, dir, rect, grip, parent }
}

/// A `w`×`h` rect centred in `outer`.
fn centred(outer: Rect, w: f32, h: f32) -> Rect {
    Rect { x: outer.x + (outer.w - w) / 2.0, y: outer.y + (outer.h - h) / 2.0, w, h }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CENTRE: Rect = Rect { x: 0.0, y: 0.0, w: 1112.0, h: 1052.0 };
    const GAP: f32 = 14.0;

    fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect { x, y, w, h }
    }

    fn layout(ids: &[&str], focus: &str) -> Layout {
        Layout {
            tiles: ids.iter().map(|s| s.to_string()).collect(),
            focus: Some(focus.to_string()),
            ..Layout::default()
        }
    }

    fn assert_rect(actual: Rect, expected: Rect, tol: f32) {
        assert!(
            (actual.x - expected.x).abs() <= tol
                && (actual.y - expected.y).abs() <= tol
                && (actual.w - expected.w).abs() <= tol
                && (actual.h - expected.h).abs() <= tol,
            "rect {actual:?} != {expected:?}"
        );
    }

    fn rects(l: &Layout) -> Vec<Rect> {
        l.compute(CENTRE, GAP).0.into_iter().map(|(_, r)| r).collect()
    }

    #[test]
    fn default_three_tiles_match_prototype() {
        let l = layout(&["s1", "s2", "s4"], "s1");
        let (tiles, handles) = l.compute(CENTRE, GAP);
        assert_eq!(tiles[0].0, "s1");
        assert_rect(tiles[0].1, rect(0.0, 0.0, 549.0, 1052.0), 0.0);
        assert_rect(tiles[1].1, rect(563.0, 0.0, 549.0, 519.0), 0.0);
        assert_rect(tiles[2].1, rect(563.0, 533.0, 549.0, 519.0), 0.0);
        assert_eq!(handles.len(), 2);
        assert_eq!(handles[0].key, HandleKey::Split(0));
        assert_eq!(handles[0].dir, Dir::V);
        assert_rect(handles[0].rect, rect(549.0, 0.0, 14.0, 1052.0), 0.0);
        assert_rect(handles[0].grip, rect(554.5, 504.0, 3.0, 44.0), 0.0);
        assert_eq!(handles[1].dir, Dir::H);
        assert_rect(handles[1].rect, rect(563.0, 519.0, 549.0, 14.0), 0.0);
        assert_rect(handles[1].grip, rect(815.5, 524.5, 44.0, 3.0), 0.0);
    }

    #[test]
    fn split_focused_after_first_tile_matches_prototype() {
        let mut l = layout(&["s1", "s2", "s4"], "s1");
        assert!(l.split_focused("s5", CENTRE, GAP));
        assert_eq!(l.tiles, ["s1", "s5", "s2", "s4"]);
        assert_eq!(l.focus.as_deref(), Some("s5"));
        let r = rects(&l);
        // Prototype values are integers; the model keeps the exact 267.5 split.
        assert_rect(r[0], rect(0.0, 0.0, 549.0, 1052.0), 1.0);
        assert_rect(r[1], rect(563.0, 0.0, 549.0, 519.0), 1.0);
        assert_rect(r[2], rect(563.0, 533.0, 268.0, 519.0), 1.0);
        assert_rect(r[3], rect(845.0, 533.0, 268.0, 519.0), 1.0);
    }

    #[test]
    fn master_three_tiles() {
        let mut l = layout(&["a", "b", "c"], "a");
        l.mode = LayoutMode::Master;
        let (tiles, handles) = l.compute(CENTRE, GAP);
        // round((1112 - 14) * 0.56) = 615
        assert_rect(tiles[0].1, rect(0.0, 0.0, 615.0, 1052.0), 0.0);
        assert_rect(tiles[1].1, rect(629.0, 0.0, 483.0, 519.0), 0.0);
        assert_rect(tiles[2].1, rect(629.0, 533.0, 483.0, 519.0), 0.0);
        assert_eq!(handles.len(), 1);
        assert_eq!(handles[0].key, HandleKey::Master);
        assert_rect(handles[0].rect, rect(615.0, 0.0, 14.0, 1052.0), 0.0);
    }

    #[test]
    fn drag_clamps_to_ratio_range() {
        let area = rect(0.0, 0.0, 4000.0, 1000.0);
        let mut l = layout(&["a", "b"], "a");
        let (_, handles) = l.compute(area, GAP);
        l.drag(&handles[0], (-1000.0, 0.0), area, GAP);
        assert_eq!(l.ratios[&0], 0.12);
        l.drag(&handles[0], (10000.0, 0.0), area, GAP);
        assert_eq!(l.ratios[&0], 0.88);
    }

    #[test]
    fn drag_respects_min_tile_width() {
        let mut l = layout(&["a", "b"], "a");
        let (_, handles) = l.compute(CENTRE, GAP);
        // The tighter of the 0.12 clamp and the minimum tile width wins.
        l.drag(&handles[0], (0.0, 0.0), CENTRE, GAP);
        let min_k = (MIN_TILE_W / 1098.0).max(0.12);
        assert!((l.ratios[&0] - min_k).abs() < 1e-3, "got {}", l.ratios[&0]);
        assert!(rects(&l)[0].w >= MIN_TILE_W);
        // Mirror: the right tile keeps its minimum as well.
        l.drag(&handles[0], (2000.0, 0.0), CENTRE, GAP);
        assert!((l.ratios[&0] - (1.0 - min_k)).abs() < 1e-3, "got {}", l.ratios[&0]);
        assert!(rects(&l)[1].w >= MIN_TILE_W);
    }

    #[test]
    fn drag_master_respects_min_tile_width() {
        let mut l = layout(&["a", "b"], "a");
        l.mode = LayoutMode::Master;
        let (_, handles) = l.compute(CENTRE, GAP);
        l.drag(&handles[0], (0.0, 0.0), CENTRE, GAP);
        let min_k = (MIN_TILE_W / 1098.0).max(0.12);
        assert!((l.master_ratio - min_k).abs() < 1e-3, "got {}", l.master_ratio);
        let r = l.compute(CENTRE, GAP).0;
        assert!(r[0].1.w >= MIN_TILE_W && r[1].1.w >= MIN_TILE_W);
    }

    #[test]
    fn double_click_resets_split_and_master() {
        let mut l = layout(&["a", "b"], "a");
        l.ratios.insert(0, 0.3);
        l.master_ratio = 0.3;
        let (_, handles) = l.compute(CENTRE, GAP);
        l.reset(&handles[0]);
        assert_eq!(l.ratios[&0], 0.5);
        l.mode = LayoutMode::Master;
        let (_, master) = l.compute(CENTRE, GAP);
        l.reset(&master[0]);
        assert_eq!(l.master_ratio, 0.56);
    }

    #[test]
    fn reset_all_clears_sizes() {
        let mut l = layout(&["a", "b"], "a");
        l.ratios.insert(0, 0.3);
        l.master_ratio = 0.7;
        l.reset_all();
        assert!(l.ratios.is_empty());
        assert_eq!(l.master_ratio, 0.56);
    }

    #[test]
    fn split_refuses_when_tiles_too_small() {
        // Six tiles fit the default centre (the smallest is 126.75 px wide), but not a
        // narrow window, where the sixth would fall below the minimum.
        let mut l = layout(&["a", "b", "c", "d", "e"], "a");
        assert!(l.clone().split_focused("f", CENTRE, GAP));
        let narrow = Rect { x: 0.0, y: 0.0, w: 700.0, h: 700.0 };
        let before = l.clone();
        assert!(!l.split_focused("f", narrow, GAP));
        assert_eq!(l, before);
    }

    #[test]
    fn split_refuses_already_tiled_id() {
        let mut l = layout(&["a", "b"], "a");
        assert!(!l.split_focused("b", CENTRE, GAP));
        assert_eq!(l.tiles, ["a", "b"]);
    }

    #[test]
    fn six_tile_cap_drops_focused_and_appends() {
        let area = rect(0.0, 0.0, 4000.0, 3000.0);
        let mut l = layout(&["t0", "t1", "t2", "t3", "t4", "t5"], "t2");
        assert!(l.split_focused("new", area, GAP));
        assert_eq!(l.tiles, ["t0", "t1", "t3", "t4", "t5", "new"]);
        assert_eq!(l.focus.as_deref(), Some("new"));
    }

    #[test]
    fn close_moves_focus_to_previous_or_first() {
        let mut l = layout(&["a", "b", "c"], "b");
        l.close("b");
        assert_eq!(l.tiles, ["a", "c"]);
        assert_eq!(l.focus.as_deref(), Some("a"));

        let mut l = layout(&["a", "b", "c"], "a");
        l.close("a");
        assert_eq!(l.focus.as_deref(), Some("b"));

        let mut l = layout(&["a", "b", "c"], "a");
        l.close("c");
        assert_eq!(l.focus.as_deref(), Some("a"));

        let mut l = layout(&["a"], "a");
        l.close("a");
        assert!(l.tiles.is_empty());
        assert_eq!(l.focus, None);

        let mut l = layout(&["a"], "a");
        l.close("zzz");
        assert_eq!(l.tiles, ["a"]);
    }

    #[test]
    fn open_in_focused_replaces_or_focuses() {
        let mut l = layout(&["a", "b"], "a");
        l.open_in_focused("c");
        assert_eq!(l.tiles, ["c", "b"]);
        assert_eq!(l.focus.as_deref(), Some("c"));

        l.open_in_focused("b");
        assert_eq!(l.tiles, ["c", "b"]);
        assert_eq!(l.focus.as_deref(), Some("b"));

        let mut l = Layout::default();
        l.open_in_focused("x");
        assert_eq!(l.tiles, ["x"]);
        assert_eq!(l.focus.as_deref(), Some("x"));

        let mut l = layout(&["a", "b"], "zzz");
        l.open_in_focused("c");
        assert_eq!(l.tiles, ["a", "c"]);
    }

    #[test]
    fn focus_arrow_wraps_in_tile_order() {
        let mut l = layout(&["a", "b", "c"], "a");
        l.focus_arrow(Arrow::Right);
        assert_eq!(l.focus.as_deref(), Some("b"));
        l.focus_arrow(Arrow::Down);
        assert_eq!(l.focus.as_deref(), Some("c"));
        l.focus_arrow(Arrow::Right);
        assert_eq!(l.focus.as_deref(), Some("a"));
        l.focus_arrow(Arrow::Left);
        assert_eq!(l.focus.as_deref(), Some("c"));
        l.focus_arrow(Arrow::Up);
        assert_eq!(l.focus.as_deref(), Some("b"));
    }

    #[test]
    fn six_tile_cap_holds_without_focus() {
        let area = rect(0.0, 0.0, 4000.0, 3000.0);
        let mut l = layout(&["t0", "t1", "t2", "t3", "t4", "t5"], "t2");
        l.focus = None;
        assert!(l.split_focused("new", area, GAP));
        assert_eq!(l.tiles.len(), MAX_TILES);
        assert_eq!(l.tiles, ["t0", "t1", "t2", "t3", "t4", "new"]);
        // A focus that names an untiled session counts as no focus.
        l.focus = Some("gone".into());
        assert!(l.split_focused("newer", area, GAP));
        assert_eq!(l.tiles.len(), MAX_TILES);
    }

    #[test]
    fn focus_arrow_without_focus_starts_at_the_ends() {
        let mut l = layout(&["a", "b", "c"], "a");
        l.focus = None;
        l.focus_arrow(Arrow::Left);
        assert_eq!(l.focus.as_deref(), Some("c"));
        l.focus = None;
        l.focus_arrow(Arrow::Right);
        assert_eq!(l.focus.as_deref(), Some("a"));
    }

    #[test]
    fn bad_saved_fractions_are_clamped() {
        let mut l = layout(&["a", "b", "c"], "a");
        l.ratios.insert(0, 5.0);
        l.ratios.insert(1, -3.0);
        let r = rects(&l);
        assert!(r.iter().all(|r| r.w > 0.0 && r.h > 0.0), "{r:?}");
        assert_rect(r[0], rect(0.0, 0.0, (1112.0 - GAP) * 0.88, 1052.0), 1e-3);

        let mut m = layout(&["a", "b"], "a");
        m.mode = LayoutMode::Master;
        m.master_ratio = f32::NAN;
        assert_rect(m.compute(CENTRE, GAP).0[0].1, rect(0.0, 0.0, 615.0, 1052.0), 0.0);

        let mut n = layout(&["a", "b", "a", "c", "d", "e", "f", "g"], "a");
        n.ratios.insert(0, f32::INFINITY);
        n.ratios.insert(1, 0.01);
        n.master_ratio = 2.0;
        n.normalize();
        assert_eq!(n.tiles, ["a", "b", "c", "d", "e", "f"]);
        assert_eq!(n.ratios.get(&0), None);
        assert_eq!(n.ratios[&1], RATIO_MIN);
        assert_eq!(n.master_ratio, RATIO_MAX);
    }

    #[test]
    fn tiny_area_never_gives_negative_sizes() {
        for mode in [LayoutMode::Dwindle, LayoutMode::Master] {
            let mut l = layout(&["a", "b", "c", "d"], "a");
            l.mode = mode;
            for area in [rect(0.0, 0.0, 0.0, 0.0), rect(0.0, 0.0, 10.0, 5.0)] {
                for (_, r) in l.compute(area, GAP).0 {
                    assert!(r.w >= 0.0 && r.h >= 0.0, "{mode:?} {r:?}");
                }
            }
        }
    }

    #[test]
    fn drag_with_zero_length_parent_does_nothing() {
        let mut l = layout(&["a", "b"], "a");
        let area = rect(0.0, 0.0, GAP, 500.0);
        let (_, handles) = l.compute(area, GAP);
        l.drag(&handles[0], (GAP / 2.0, 0.0), area, GAP);
        assert!(l.ratios.get(&0).is_none_or(|k| k.is_finite()));
    }

    #[test]
    fn drag_still_works_when_another_tile_is_already_too_small() {
        // Four tiles side by side in a short window are all below the minimum height, which no
        // vertical divider can fix. The first divider must still move.
        let area = rect(0.0, 0.0, 1100.0, 90.0);
        let mut l = layout(&["a", "b", "c", "d"], "a");
        let (_, handles) = l.compute(area, GAP);
        assert!(!l.fits(area, GAP));
        l.drag(&handles[0], (400.0, 0.0), area, GAP);
        let k = l.ratios[&0];
        assert!((k - (400.0 - GAP / 2.0) / (1100.0 - GAP)).abs() < 1e-3, "got {k}");
    }

    #[test]
    fn serde_round_trip() {
        let mut l = layout(&["a", "b", "c"], "b");
        l.mode = LayoutMode::Master;
        l.ratios.insert(0, 0.3);
        l.ratios.insert(2, 0.7);
        l.master_ratio = 0.61;
        let json = serde_json::to_string(&l).unwrap();
        let back: Layout = serde_json::from_str(&json).unwrap();
        assert_eq!(back, l);
    }
}
