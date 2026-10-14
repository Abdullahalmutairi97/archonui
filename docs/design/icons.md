# ArchonUI — Session Icons, Brand Mark & Identity Icons

Source: `design/ArchonUI Agent v3.dc.html` (prototype). Everything below is lifted verbatim from that file; line refs are `Lnnn`.
Machine-readable twin with all 100 pattern objects (path indices, 3x3 bitmaps, cells, SVG `d`): **`docs/design/icons.json`**.

> **Important correction to the brief.** The "100 dot-grid patterns" are **not** on/off dot matrices. They are 100
> **sigils**: each is a single SVG **path** traced through a **3x3 lattice of 9 points** [L1164–L1184], optionally
> closed, with **stroked** circle / square end-markers appended. There is no per-cell on/off state and no per-dot opacity.

---

## 1. Generator (verbatim, L1164–L1184)

```js
const SIGILS = (() => {
  const G = [3, 8, 13], pts = []; G.forEach(y => G.forEach(x => pts.push([x, y])));
  let seed = 7; const rnd = () => (seed = (seed * 16807) % 2147483647) / 2147483647;
  const out = [], seen = new Set();
  while (out.length < 100) {
    const len = 3 + Math.floor(rnd() * 3), path = [Math.floor(rnd() * 9)];
    while (path.length < len) { const n = Math.floor(rnd() * 9); if (!path.includes(n)) path.push(n); }
    const k = path.join('');
    if (seen.has(k) || seen.has([...path].reverse().join(''))) continue;
    seen.add(k);
    const [sx, sy] = pts[path[0]], [ex, ey] = pts[path[path.length - 1]];
    const dot = (x, y, r) => 'M' + (x - r) + ' ' + y + 'a' + r + ' ' + r + ' 0 1 0 ' + (2 * r) + ' 0a' + r + ' ' + r + ' 0 1 0 ' + (-2 * r) + ' 0';
    const sq = (x, y) => 'M' + (x - 1.4) + ' ' + (y - 1.4) + 'h2.8v2.8h-2.8z';
    const v = out.length % 5, closed = len > 3 && v === 1;
    let d = 'M' + path.map(i => pts[i].join(' ')).join('L') + (closed ? 'Z' : '');
    d += v === 2 ? dot(sx, sy, 1.9) : v === 3 ? sq(sx, sy) : dot(sx, sy, 0.9);
    if (v === 4) d += dot(ex, ey, 0.9);
    if (v === 0 && rnd() > 0.5) { const m = pts[path[1]]; d += dot(m[0], m[1], 1.6); }
    out.push(d);
  }
  return out;
})();
```

### Lattice (L1165)

| Property | Value |
|---|---|
| Values | `[3, 8, 13]` |
| Points | 9 — the cartesian product `{3,8,13} x {3,8,13}` |
| Point index | `idx = row*3 + col` (row = y index, col = x index) |
| Coordinates | `x = 3 + 5*col`, `y = 3 + 5*row` |
| Spacing | 5 (uniform, both axes) |
| viewBox | `0 0 16 16` (lattice occupies 3..13) |

Index → point: 0=(3,3) 1=(8,3) 2=(13,3) 3=(3,8) 4=(8,8) 5=(13,8) 6=(3,13) 7=(8,13) 8=(13,13).

### PRNG (L1166)

Lehmer / Park-Miller minimal standard. `seed = 7`; on each draw `seed = (seed*16807) % 2147483647`, return `seed/2147483647`.

Draw order / consumption (must be reproduced exactly, it shifts every later pattern):
1. one draw per candidate: `len` (→ 3, 4 or 5),
2. one draw for `path[0]`,
3. one draw per rejection *and* per accepted point until `path.length === len`,
4. only for accepted **variant-0** candidates: one extra draw for the middle-dot coin flip (`rnd() > 0.5`).
   Rejected candidates (`continue`) have already consumed draws 1–3 but never reach 4.

### Algorithm, step by step

1. Build the 9 lattice points.
2. Loop until 100 sigils are produced.
3. `len = 3 + floor(rnd()*3)` → 3, 4 or 5.
4. `path = [floor(rnd()*9)]`; append random distinct indices until `path.length === len` (order matters, no repeats).
5. Dedupe: skip if `path.join('')` **or its reverse** was already seen (a sigil and its mirror count as one).
6. `v = out.length % 5`; `closed = len > 3 && v === 1`.
7. Path body = `"M" + path.map(i => pts[i].join(" ")).join("L")` + `"Z"` only if `closed`.
8. Append marker(s) per `v` (see §2).
9. Push `d`. Index in `out` (= `v` cycle 0..4) is the pattern's identity.

### Marker rules (by variant)

| `v = idx % 5` | Start marker | End marker | Extra |
|---|---|---|---|
| 0 | circle r=0.9 at `path[0]` | — | if `rnd() > 0.5`: circle r=1.6 at `path[1]` |
| 1 | circle r=0.9 at `path[0]` | — | path is closed (`Z`) when `len > 3` |
| 2 | circle **r=1.9** at `path[0]` | — | — |
| 3 | **square 2.8 x 2.8** at `path[0]` | — | — |
| 4 | circle r=0.9 at `path[0]` | circle r=0.9 at last point | — |

Marker path syntax (verbatim):

```
circle(x, y, r) = M{x-r} {y} a{r} {r} 0 1 0 {2r} 0 a{r} {r} 0 1 0 {-2r} 0      // full circle subpath
square(x, y)    = M{x-1.4} {y-1.4} h2.8 v2.8 h-2.8 z                            // 2.8 x 2.8 square
```

Because the rendered `<svg>` uses `fill="none"`, both markers draw as **stroked outlines** (a ring / a square
outline), not filled shapes. Stroke width 1.5 in viewBox units.

---

## 2. Rendering spec

Every render site uses the same SVG shape:

```html
<svg width="W" height="W" viewBox="0 0 16 16" fill="none" stroke="currentColor"
     stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
  <path d="{sigil}"></path>
</svg>
```

- `fill="none"`, `stroke="currentColor"`, `stroke-width="1.5"`, round cap + round join.
- **No per-dot colour or opacity** — the whole sigil is a single `currentColor`.
- Colour is chosen by *state* at each site (below). Verified in-browser with `getComputedStyle` on the default
  (Blood Moon) theme: focused `rgb(217,181,106)` (= `acc2 #d9b56a`), open `rgb(243,234,216)` (= `fg #f3ead8`),
  unfocused `rgb(191,174,148)` (= `muted #bfae94`).
- Companion **status dot** (not part of the sigil): `8px x 8px`, `border-radius:50%`,
  `background = running ? acc2 : done ? alt : faint` (L1510), `border: 2px solid` the local surface colour.

### Render sites and rendered pixel sizes

| # | Where | Line | Container | SVG rendered size | Sigil colour | Status dot |
|---|---|---|---|---|---|---|
| 1 | Sidebar, expanded, per session row | L70 | `20x20` grid span, `position:relative` | **16 x 16** | `foc ? acc2 : open ? fg : muted` (L1562) | `right:-2;bottom:-2`, border `2px panel` |
| 2 | Sidebar, collapsed rail (accent bands) | L36 | `36x36` button, `border-radius:9px` | **17 x 17** | `foc ? acc2 : open ? fg : muted` (L1654) | `right:1;bottom:1`, border `2px panel` |
| 3 | Tile header, per tile | L121 | `20x20` grid span | **16 x 16** | `foc ? acc2 : fg` (L1520) | `right:-2;bottom:-2`, border `2px tile` |

There are exactly these three sites — `SIGILS[...]` is referenced only at L1520 (tile header), L1562 (sidebar row) and
L1654 (collapsed rail).

**Collapsed-rail accent band** (L1654): each 36x36 button's background is
`rgba(mix(acc, acc2, (i % 6) * 0.07), focused ? 0.42 : 0.20)`, where `i` is the session's position in the list.
Rendered proof (Blood Moon, `acc #c81e2a`, `acc2 #d9b56a`):
focused = `rgba(200,30,42,0.42)`; then `rgba(201,41,46,0.2)`, `rgba(202,51,51,0.2)`, `rgba(204,62,55,0.2)`,
`rgba(205,72,60,0.2)`, `rgba(206,83,64,0.2)`.

---

## 3. How a session is assigned a pattern

| Rule | Source | Code |
|---|---|---|
| Initial seed (6 sessions) spreads indices | L1191 | `sessions: seed().map((x, i) => ({ ...x, icon: [0, 17, 34, 51, 68, 85][i] }))` |
| New session | L1273 | `const used = new Set(sessions.map(x => x.icon)); let icon = SIGILS.findIndex((_, k) => !used.has(k)); if (icon < 0) icon = sessions.length % SIGILS.length;` |

So a new session takes the **lowest sigil index not currently used**; if all 100 are in use, it falls back to
`sessionCount % 100`. Session's own field is `session.icon` (a number 0–99); all render sites read
`SIGILS[session.icon ?? 0]`.

---

## 4. App logo / brand mark ("archonui" wordmark)

The wordmark is the editable app name (`appName`, default **"archonui"**, `maxlength=18`, L616), set in the active
theme's **display font**.

- Display font resolution (L1468): `display = s.dispFont || FONTS[theme][fontIdx].d`.
- Per-theme default pairing (L900–L914), and the first entry is the default:
  ArchonUI → `'Grenze Gotisch', serif`; **Blood Moon (default theme) → `'UnifrakturCook', serif`**;
  Moonlight → `'Cormorant Garamond', serif`; Wreck → `'Bebas Neue', sans-serif`; Citadel → `'Grenze Gotisch', serif`;
  Neon → `'Orbitron', sans-serif`; Violet → `'Cormorant Garamond', serif`; Ink → `'Zen Old Mincho', serif`;
  Bonfire → `'Pixelify Sans', monospace`; Hunter → `'Silkscreen', monospace`; Shards → `'Orbitron', sans-serif`;
  Vale → `'Metamorphous', serif`.
- The mark is a **logo image** (`t.logo`) inline with the wordmark, never a single combined asset.

| Where | Line | Logo image | Wordmark |
|---|---|---|---|
| Sidebar header (expanded) | L47–L48 | `22x22`, `background:url(t.logo) center/contain` | `font-size:24px; line-height:1` |
| Empty workspace (no tiles) | L111–L112 | `72x72`, `opacity:.8`, `image-rendering: t.render` | `font-size:56px`, `text-shadow:0 2px 24px rgba(0,0,0,.6)` |
| Tile empty state | L133–L134 | `44x44`, `opacity:.55` | `font-size:28px` — "What should {appName} work on?" |
| Login screen | L366 / L369 | — | nav "archonui" `18px`; heading "Sign in to continue" `36px`, `text-shadow:0 2px 18px rgba(0,0,0,.7)` |
| Settings > Appearance, IDENTITY "Name" | L616 / L619 | preview `22x22` | input `34px`; "Sidebar preview" `22px` |

---

## 5. Preset identity icons (Settings > IDENTITY > Icon)

> The brief says *Settings > General > Identity*; in the source the Identity block is gated on
> `pg.appearance` (the **Appearance** tab), L611. Behaviour is otherwise identical.

- List (L1428): `icons: [...new Set(NAMES.map(nm => THEMES[nm].logo)), ...s.upIcons]` — the 12 per-theme logo files
  (deduped) plus any user uploads.
- Grid (L626–L631): `display:grid; grid-template-columns:repeat(7,minmax(0,1fr)); gap:8px`; each cell
  `aspect-ratio:1; border-radius:8px`, inner image at `width:58%; height:58%`, `center / contain`.
- Selected cell: `outline:2px solid theme.acc; outline-offset:1px`, `background: theme.soft`; unselected:
  `outline: transparent`, `background: theme.line`.
- Buttons: "Use theme icon" → `resetIcon` (`opacity` `1` when a custom logo is set, else `0.4`, L1430).
  `+` cell is a `<label>` with `<input type="file" accept="image/*">`; the chosen file becomes an object URL,
  is appended to `upIcons` and set as the logo (L1429).

| # | Theme | Asset file |
|---|---|---|
| 1 | ArchonUI | `assets/sword-logo-diag.png` |
| 2 | Blood Moon | `assets/logo-bm.png` |
| 3 | Moonlight | `assets/logo-moon.png` |
| 4 | Wreck | `assets/logo-sand.png` |
| 5 | Citadel | `assets/logo-drg-black.png` |
| 6 | Neon | `assets/logo-cat.png` |
| 7 | Violet | `assets/logo-portal.png` |
| 8 | Ink | `assets/logo-tree.png` |
| 9 | Bonfire | `assets/logo-bonfire.png` |
| 10 | Hunter | `assets/logo-helm.png` |
| 11 | Shards | `assets/logo-snow.png` |
| 12 | Vale | `assets/logo-galaxy.png` |

(Order = `NAMES` = `Object.keys(THEMES)`, L897.)

---

## 6. All 100 sigils

Dumped by running the generator above in the browser (Playwright) and cross-checked against the six sigils
actually rendered in the DOM (seed indices 0, 17, 34, 51, 68, 85 all matched).

- `path` = lattice point indices in draw order; `v` = `index % 5`; `closed` = trailing `Z`.
- Full per-pattern detail (visited 3x3 bitmap as three "0/1" row strings, plus `cells`) is in `icons.json`.

| idx | len | v | closed | path | d |
|---:|---:|---:|:--:|---|---|
| 0 | 3 | 0 | no | [8, 2, 1] | `M13 13L13 3L8 3M12.1 13a0.9 0.9 0 1 0 1.8 0a0.9 0.9 0 1 0 -1.8 0M11.4 3a1.6 1.6 0 1 0 3.2 0a1.6 1.6 0 1 0 -3.2 0` |
| 1 | 4 | 1 | yes | [2, 6, 4, 5] | `M13 3L3 13L8 8L13 8ZM12.1 3a0.9 0.9 0 1 0 1.8 0a0.9 0.9 0 1 0 -1.8 0` |
| 2 | 5 | 2 | no | [2, 3, 6, 0, 4] | `M13 3L3 8L3 13L3 3L8 8M11.1 3a1.9 1.9 0 1 0 3.8 0a1.9 1.9 0 1 0 -3.8 0` |
| 3 | 5 | 3 | no | [7, 1, 4, 8, 6] | `M8 13L8 3L8 8L13 13L3 13M6.6 11.6h2.8v2.8h-2.8z` |
| 4 | 4 | 4 | no | [5, 8, 3, 7] | `M13 8L13 13L3 8L8 13M12.1 8a0.9 0.9 0 1 0 1.8 0a0.9 0.9 0 1 0 -1.8 0M7.1 13a0.9 0.9 0 1 0 1.8 0a0.9 0.9 0 1 0 -1.8 0` |
| 5 | 3 | 0 | no | [1, 2, 3] | `M8 3L13 3L3 8M7.1 3a0.9 0.9 0 1 0 1.8 0a0.9 0.9 0 1 0 -1.8 0` |
| 6 | 5 | 1 | yes | [5, 6, 7, 0, 2] | `M13 8L3 13L8 13L3 3L13 3ZM12.1 8a0.9 0.9 0 1 0 1.8 0a0.9 0.9 0 1 0 -1.8 0` |
| 7 | 4 | 2 | no | [4, 3, 1, 8] | `M8 8L3 8L8 3L13 13M6.1 8a1.9 1.9 0 1 0 3.8 0a1.9 1.9 0 1 0 -3.8 0` |
| 8 | 3 | 3 | no | [3, 5, 8] | `M3 8L13 8L13 13M1.6 6.6h2.8v2.8h-2.8z` |
| 9 | 4 | 4 | no | [1, 3, 2, 4] | `M8 3L3 8L13 3L8 8M7.1 3a0.9 0.9 0 1 0 1.8 0a0.9 0.9 0 1 0 -1.8 0M7.1 8a0.9 0.9 0 1 0 1.8 0a0.9 0.9 0 1 0 -1.8 0` |
| 10 | 4 | 0 | no | [2, 8, 4, 7] | `M13 3L13 13L8 8L8 13M12.1 3a0.9 0.9 0 1 0 1.8 0a0.9 0.9 0 1 0 -1.8 0M11.4 13a1.6 1.6 0 1 0 3.2 0a1.6 1.6 0 1 0 -3.2 0` |
| 11 | 4 | 1 | yes | [4, 6, 8, 3] | `M8 8L3 13L13 13L3 8ZM7.1 8a0.9 0.9 0 1 0 1.8 0a0.9 0.9 0 1 0 -1.8 0` |
| 12 | 5 | 2 | no | [2, 5, 3, 7, 0] | `M13 3L13 8L3 8L8 13L3 3M11.1 3a1.9 1.9 0 1 0 3.8 0a1.9 1.9 0 1 0 -3.8 0` |
| 13 | 5 | 3 | no | [0, 3, 1, 8, 5] | `M3 3L3 8L8 3L13 13L13 8M1.6 1.6h2.8v2.8h-2.8z` |
| 14 | 3 | 4 | no | [4, 5, 1] | `M8 8L13 8L8 3M7.1 8a0.9 0.9 0 1 0 1.8 0a0.9 0.9 0 1 0 -1.8 0M7.1 3a0.9 0.9 0 1 0 1.8 0a0.9 0.9 0 1 0 -1.8 0` |
| 15 | 5 | 0 | no | [7, 8, 6, 2, 0] | `M8 13L13 13L3 13L13 3L3 3M7.1 13a0.9 0.9 0 1 0 1.8 0a0.9 0.9 0 1 0 -1.8 0` |
| 16 | 3 | 1 | no | [0, 5, 2] | `M3 3L13 8L13 3M2.1 3a0.9 0.9 0 1 0 1.8 0a0.9 0.9 0 1 0 -1.8 0` |
| 17 | 5 | 2 | no | [4, 6, 2, 7, 3] | `M8 8L3 13L13 3L8 13L3 8M6.1 8a1.9 1.9 0 1 0 3.8 0a1.9 1.9 0 1 0 -3.8 0` |
| 18 | 5 | 3 | no | [1, 4, 0, 8, 5] | `M8 3L8 8L3 3L13 13L13 8M6.6 1.6h2.8v2.8h-2.8z` |
| 19 | 5 | 4 | no | [0, 8, 1, 5, 6] | `M3 3L13 13L8 3L13 8L3 13M2.1 3a0.9 0.9 0 1 0 1.8 0a0.9 0.9 0 1 0 -1.8 0M2.1 13a0.9 0.9 0 1 0 1.8 0a0.9 0.9 0 1 0 -1.8 0` |
| 20 | 3 | 0 | no | [2, 1, 5] | `M13 3L8 3L13 8M12.1 3a0.9 0.9 0 1 0 1.8 0a0.9 0.9 0 1 0 -1.8 0` |
| 21 | 5 | 1 | yes | [6, 4, 0, 8, 1] | `M3 13L8 8L3 3L13 13L8 3ZM2.1 13a0.9 0.9 0 1 0 1.8 0a0.9 0.9 0 1 0 -1.8 0` |
| 22 | 5 | 2 | no | [8, 0, 7, 6, 3] | `M13 13L3 3L8 13L3 13L3 8M11.1 13a1.9 1.9 0 1 0 3.8 0a1.9 1.9 0 1 0 -3.8 0` |
| 23 | 3 | 3 | no | [6, 7, 3] | `M3 13L8 13L3 8M1.6 11.6h2.8v2.8h-2.8z` |
| 24 | 3 | 4 | no | [3, 7, 5] | `M3 8L8 13L13 8M2.1 8a0.9 0.9 0 1 0 1.8 0a0.9 0.9 0 1 0 -1.8 0M12.1 8a0.9 0.9 0 1 0 1.8 0a0.9 0.9 0 1 0 -1.8 0` |
| 25 | 4 | 0 | no | [5, 6, 3, 1] | `M13 8L3 13L3 8L8 3M12.1 8a0.9 0.9 0 1 0 1.8 0a0.9 0.9 0 1 0 -1.8 0` |
| 26 | 5 | 1 | yes | [8, 5, 7, 1, 4] | `M13 13L13 8L8 13L8 3L8 8ZM12.1 13a0.9 0.9 0 1 0 1.8 0a0.9 0.9 0 1 0 -1.8 0` |
| 27 | 4 | 2 | no | [6, 8, 4, 1] | `M3 13L13 13L8 8L8 3M1.1 13a1.9 1.9 0 1 0 3.8 0a1.9 1.9 0 1 0 -3.8 0` |
| 28 | 4 | 3 | no | [1, 5, 4, 6] | `M8 3L13 8L8 8L3 13M6.6 1.6h2.8v2.8h-2.8z` |
| 29 | 4 | 4 | no | [6, 0, 2, 4] | `M3 13L3 3L13 3L8 8M2.1 13a0.9 0.9 0 1 0 1.8 0a0.9 0.9 0 1 0 -1.8 0M7.1 8a0.9 0.9 0 1 0 1.8 0a0.9 0.9 0 1 0 -1.8 0` |
| 30 | 3 | 0 | no | [6, 2, 5] | `M3 13L13 3L13 8M2.1 13a0.9 0.9 0 1 0 1.8 0a0.9 0.9 0 1 0 -1.8 0` |
| 31 | 3 | 1 | no | [6, 7, 8] | `M3 13L8 13L13 13M2.1 13a0.9 0.9 0 1 0 1.8 0a0.9 0.9 0 1 0 -1.8 0` |
| 32 | 5 | 2 | no | [6, 8, 0, 5, 1] | `M3 13L13 13L3 3L13 8L8 3M1.1 13a1.9 1.9 0 1 0 3.8 0a1.9 1.9 0 1 0 -3.8 0` |
| 33 | 3 | 3 | no | [0, 5, 8] | `M3 3L13 8L13 13M1.6 1.6h2.8v2.8h-2.8z` |
| 34 | 3 | 4 | no | [1, 6, 2] | `M8 3L3 13L13 3M7.1 3a0.9 0.9 0 1 0 1.8 0a0.9 0.9 0 1 0 -1.8 0M12.1 3a0.9 0.9 0 1 0 1.8 0a0.9 0.9 0 1 0 -1.8 0` |
| 35 | 4 | 0 | no | [8, 3, 6, 0] | `M13 13L3 8L3 13L3 3M12.1 13a0.9 0.9 0 1 0 1.8 0a0.9 0.9 0 1 0 -1.8 0` |
| 36 | 5 | 1 | yes | [8, 1, 4, 5, 0] | `M13 13L8 3L8 8L13 8L3 3ZM12.1 13a0.9 0.9 0 1 0 1.8 0a0.9 0.9 0 1 0 -1.8 0` |
| 37 | 3 | 2 | no | [5, 4, 8] | `M13 8L8 8L13 13M11.1 8a1.9 1.9 0 1 0 3.8 0a1.9 1.9 0 1 0 -3.8 0` |
| 38 | 3 | 3 | no | [3, 2, 0] | `M3 8L13 3L3 3M1.6 6.6h2.8v2.8h-2.8z` |
| 39 | 3 | 4 | no | [3, 0, 2] | `M3 8L3 3L13 3M2.1 8a0.9 0.9 0 1 0 1.8 0a0.9 0.9 0 1 0 -1.8 0M12.1 3a0.9 0.9 0 1 0 1.8 0a0.9 0.9 0 1 0 -1.8 0` |
| 40 | 5 | 0 | no | [6, 3, 7, 8, 5] | `M3 13L3 8L8 13L13 13L13 8M2.1 13a0.9 0.9 0 1 0 1.8 0a0.9 0.9 0 1 0 -1.8 0M1.4 8a1.6 1.6 0 1 0 3.2 0a1.6 1.6 0 1 0 -3.2 0` |
| 41 | 4 | 1 | yes | [3, 4, 5, 6] | `M3 8L8 8L13 8L3 13ZM2.1 8a0.9 0.9 0 1 0 1.8 0a0.9 0.9 0 1 0 -1.8 0` |
| 42 | 4 | 2 | no | [5, 6, 7, 8] | `M13 8L3 13L8 13L13 13M11.1 8a1.9 1.9 0 1 0 3.8 0a1.9 1.9 0 1 0 -3.8 0` |
| 43 | 4 | 3 | no | [4, 8, 3, 0] | `M8 8L13 13L3 8L3 3M6.6 6.6h2.8v2.8h-2.8z` |
| 44 | 4 | 4 | no | [7, 6, 8, 1] | `M8 13L3 13L13 13L8 3M7.1 13a0.9 0.9 0 1 0 1.8 0a0.9 0.9 0 1 0 -1.8 0M7.1 3a0.9 0.9 0 1 0 1.8 0a0.9 0.9 0 1 0 -1.8 0` |
| 45 | 3 | 0 | no | [7, 0, 8] | `M8 13L3 3L13 13M7.1 13a0.9 0.9 0 1 0 1.8 0a0.9 0.9 0 1 0 -1.8 0M1.4 3a1.6 1.6 0 1 0 3.2 0a1.6 1.6 0 1 0 -3.2 0` |
| 46 | 4 | 1 | yes | [6, 4, 3, 7] | `M3 13L8 8L3 8L8 13ZM2.1 13a0.9 0.9 0 1 0 1.8 0a0.9 0.9 0 1 0 -1.8 0` |
| 47 | 4 | 2 | no | [7, 2, 6, 3] | `M8 13L13 3L3 13L3 8M6.1 13a1.9 1.9 0 1 0 3.8 0a1.9 1.9 0 1 0 -3.8 0` |
| 48 | 4 | 3 | no | [1, 5, 2, 8] | `M8 3L13 8L13 3L13 13M6.6 1.6h2.8v2.8h-2.8z` |
| 49 | 5 | 4 | no | [3, 6, 7, 1, 2] | `M3 8L3 13L8 13L8 3L13 3M2.1 8a0.9 0.9 0 1 0 1.8 0a0.9 0.9 0 1 0 -1.8 0M12.1 3a0.9 0.9 0 1 0 1.8 0a0.9 0.9 0 1 0 -1.8 0` |
| 50 | 5 | 0 | no | [0, 1, 3, 6, 8] | `M3 3L8 3L3 8L3 13L13 13M2.1 3a0.9 0.9 0 1 0 1.8 0a0.9 0.9 0 1 0 -1.8 0` |
| 51 | 3 | 1 | no | [8, 6, 1] | `M13 13L3 13L8 3M12.1 13a0.9 0.9 0 1 0 1.8 0a0.9 0.9 0 1 0 -1.8 0` |
| 52 | 3 | 2 | no | [5, 0, 7] | `M13 8L3 3L8 13M11.1 8a1.9 1.9 0 1 0 3.8 0a1.9 1.9 0 1 0 -3.8 0` |
| 53 | 4 | 3 | no | [6, 1, 2, 8] | `M3 13L8 3L13 3L13 13M1.6 11.6h2.8v2.8h-2.8z` |
| 54 | 4 | 4 | no | [5, 6, 0, 2] | `M13 8L3 13L3 3L13 3M12.1 8a0.9 0.9 0 1 0 1.8 0a0.9 0.9 0 1 0 -1.8 0M12.1 3a0.9 0.9 0 1 0 1.8 0a0.9 0.9 0 1 0 -1.8 0` |
| 55 | 4 | 0 | no | [2, 5, 7, 8] | `M13 3L13 8L8 13L13 13M12.1 3a0.9 0.9 0 1 0 1.8 0a0.9 0.9 0 1 0 -1.8 0` |
| 56 | 5 | 1 | yes | [3, 7, 5, 0, 8] | `M3 8L8 13L13 8L3 3L13 13ZM2.1 8a0.9 0.9 0 1 0 1.8 0a0.9 0.9 0 1 0 -1.8 0` |
| 57 | 4 | 2 | no | [7, 3, 6, 8] | `M8 13L3 8L3 13L13 13M6.1 13a1.9 1.9 0 1 0 3.8 0a1.9 1.9 0 1 0 -3.8 0` |
| 58 | 5 | 3 | no | [1, 6, 8, 0, 2] | `M8 3L3 13L13 13L3 3L13 3M6.6 1.6h2.8v2.8h-2.8z` |
| 59 | 5 | 4 | no | [7, 8, 2, 1, 0] | `M8 13L13 13L13 3L8 3L3 3M7.1 13a0.9 0.9 0 1 0 1.8 0a0.9 0.9 0 1 0 -1.8 0M2.1 3a0.9 0.9 0 1 0 1.8 0a0.9 0.9 0 1 0 -1.8 0` |
| 60 | 4 | 0 | no | [5, 1, 2, 4] | `M13 8L8 3L13 3L8 8M12.1 8a0.9 0.9 0 1 0 1.8 0a0.9 0.9 0 1 0 -1.8 0` |
| 61 | 3 | 1 | no | [0, 5, 4] | `M3 3L13 8L8 8M2.1 3a0.9 0.9 0 1 0 1.8 0a0.9 0.9 0 1 0 -1.8 0` |
| 62 | 5 | 2 | no | [6, 4, 3, 5, 8] | `M3 13L8 8L3 8L13 8L13 13M1.1 13a1.9 1.9 0 1 0 3.8 0a1.9 1.9 0 1 0 -3.8 0` |
| 63 | 5 | 3 | no | [8, 6, 5, 7, 1] | `M13 13L3 13L13 8L8 13L8 3M11.6 11.6h2.8v2.8h-2.8z` |
| 64 | 3 | 4 | no | [6, 5, 3] | `M3 13L13 8L3 8M2.1 13a0.9 0.9 0 1 0 1.8 0a0.9 0.9 0 1 0 -1.8 0M2.1 8a0.9 0.9 0 1 0 1.8 0a0.9 0.9 0 1 0 -1.8 0` |
| 65 | 4 | 0 | no | [3, 4, 1, 0] | `M3 8L8 8L8 3L3 3M2.1 8a0.9 0.9 0 1 0 1.8 0a0.9 0.9 0 1 0 -1.8 0` |
| 66 | 4 | 1 | yes | [3, 1, 5, 0] | `M3 8L8 3L13 8L3 3ZM2.1 8a0.9 0.9 0 1 0 1.8 0a0.9 0.9 0 1 0 -1.8 0` |
| 67 | 3 | 2 | no | [7, 0, 6] | `M8 13L3 3L3 13M6.1 13a1.9 1.9 0 1 0 3.8 0a1.9 1.9 0 1 0 -3.8 0` |
| 68 | 3 | 3 | no | [3, 8, 7] | `M3 8L13 13L8 13M1.6 6.6h2.8v2.8h-2.8z` |
| 69 | 3 | 4 | no | [8, 1, 5] | `M13 13L8 3L13 8M12.1 13a0.9 0.9 0 1 0 1.8 0a0.9 0.9 0 1 0 -1.8 0M12.1 8a0.9 0.9 0 1 0 1.8 0a0.9 0.9 0 1 0 -1.8 0` |
| 70 | 4 | 0 | no | [0, 1, 6, 2] | `M3 3L8 3L3 13L13 3M2.1 3a0.9 0.9 0 1 0 1.8 0a0.9 0.9 0 1 0 -1.8 0M6.4 3a1.6 1.6 0 1 0 3.2 0a1.6 1.6 0 1 0 -3.2 0` |
| 71 | 5 | 1 | yes | [2, 4, 7, 6, 8] | `M13 3L8 8L8 13L3 13L13 13ZM12.1 3a0.9 0.9 0 1 0 1.8 0a0.9 0.9 0 1 0 -1.8 0` |
| 72 | 5 | 2 | no | [2, 6, 7, 1, 5] | `M13 3L3 13L8 13L8 3L13 8M11.1 3a1.9 1.9 0 1 0 3.8 0a1.9 1.9 0 1 0 -3.8 0` |
| 73 | 5 | 3 | no | [4, 7, 0, 2, 1] | `M8 8L8 13L3 3L13 3L8 3M6.6 6.6h2.8v2.8h-2.8z` |
| 74 | 3 | 4 | no | [5, 7, 1] | `M13 8L8 13L8 3M12.1 8a0.9 0.9 0 1 0 1.8 0a0.9 0.9 0 1 0 -1.8 0M7.1 3a0.9 0.9 0 1 0 1.8 0a0.9 0.9 0 1 0 -1.8 0` |
| 75 | 4 | 0 | no | [4, 3, 6, 2] | `M8 8L3 8L3 13L13 3M7.1 8a0.9 0.9 0 1 0 1.8 0a0.9 0.9 0 1 0 -1.8 0M1.4 8a1.6 1.6 0 1 0 3.2 0a1.6 1.6 0 1 0 -3.2 0` |
| 76 | 5 | 1 | yes | [2, 5, 4, 8, 1] | `M13 3L13 8L8 8L13 13L8 3ZM12.1 3a0.9 0.9 0 1 0 1.8 0a0.9 0.9 0 1 0 -1.8 0` |
| 77 | 3 | 2 | no | [1, 3, 5] | `M8 3L3 8L13 8M6.1 3a1.9 1.9 0 1 0 3.8 0a1.9 1.9 0 1 0 -3.8 0` |
| 78 | 5 | 3 | no | [5, 3, 1, 0, 4] | `M13 8L3 8L8 3L3 3L8 8M11.6 6.6h2.8v2.8h-2.8z` |
| 79 | 4 | 4 | no | [0, 5, 3, 4] | `M3 3L13 8L3 8L8 8M2.1 3a0.9 0.9 0 1 0 1.8 0a0.9 0.9 0 1 0 -1.8 0M7.1 8a0.9 0.9 0 1 0 1.8 0a0.9 0.9 0 1 0 -1.8 0` |
| 80 | 3 | 0 | no | [7, 2, 1] | `M8 13L13 3L8 3M7.1 13a0.9 0.9 0 1 0 1.8 0a0.9 0.9 0 1 0 -1.8 0` |
| 81 | 4 | 1 | yes | [1, 5, 6, 7] | `M8 3L13 8L3 13L8 13ZM7.1 3a0.9 0.9 0 1 0 1.8 0a0.9 0.9 0 1 0 -1.8 0` |
| 82 | 4 | 2 | no | [7, 6, 1, 5] | `M8 13L3 13L8 3L13 8M6.1 13a1.9 1.9 0 1 0 3.8 0a1.9 1.9 0 1 0 -3.8 0` |
| 83 | 5 | 3 | no | [8, 5, 2, 0, 7] | `M13 13L13 8L13 3L3 3L8 13M11.6 11.6h2.8v2.8h-2.8z` |
| 84 | 5 | 4 | no | [6, 4, 2, 1, 7] | `M3 13L8 8L13 3L8 3L8 13M2.1 13a0.9 0.9 0 1 0 1.8 0a0.9 0.9 0 1 0 -1.8 0M7.1 13a0.9 0.9 0 1 0 1.8 0a0.9 0.9 0 1 0 -1.8 0` |
| 85 | 5 | 0 | no | [5, 8, 2, 0, 6] | `M13 8L13 13L13 3L3 3L3 13M12.1 8a0.9 0.9 0 1 0 1.8 0a0.9 0.9 0 1 0 -1.8 0M11.4 13a1.6 1.6 0 1 0 3.2 0a1.6 1.6 0 1 0 -3.2 0` |
| 86 | 4 | 1 | yes | [6, 0, 8, 7] | `M3 13L3 3L13 13L8 13ZM2.1 13a0.9 0.9 0 1 0 1.8 0a0.9 0.9 0 1 0 -1.8 0` |
| 87 | 5 | 2 | no | [5, 1, 3, 0, 7] | `M13 8L8 3L3 8L3 3L8 13M11.1 8a1.9 1.9 0 1 0 3.8 0a1.9 1.9 0 1 0 -3.8 0` |
| 88 | 4 | 3 | no | [8, 2, 6, 5] | `M13 13L13 3L3 13L13 8M11.6 11.6h2.8v2.8h-2.8z` |
| 89 | 4 | 4 | no | [1, 2, 4, 3] | `M8 3L13 3L8 8L3 8M7.1 3a0.9 0.9 0 1 0 1.8 0a0.9 0.9 0 1 0 -1.8 0M2.1 8a0.9 0.9 0 1 0 1.8 0a0.9 0.9 0 1 0 -1.8 0` |
| 90 | 3 | 0 | no | [5, 7, 0] | `M13 8L8 13L3 3M12.1 8a0.9 0.9 0 1 0 1.8 0a0.9 0.9 0 1 0 -1.8 0` |
| 91 | 4 | 1 | yes | [3, 2, 0, 5] | `M3 8L13 3L3 3L13 8ZM2.1 8a0.9 0.9 0 1 0 1.8 0a0.9 0.9 0 1 0 -1.8 0` |
| 92 | 4 | 2 | no | [4, 3, 5, 8] | `M8 8L3 8L13 8L13 13M6.1 8a1.9 1.9 0 1 0 3.8 0a1.9 1.9 0 1 0 -3.8 0` |
| 93 | 5 | 3 | no | [4, 7, 8, 5, 3] | `M8 8L8 13L13 13L13 8L3 8M6.6 6.6h2.8v2.8h-2.8z` |
| 94 | 4 | 4 | no | [0, 2, 7, 1] | `M3 3L13 3L8 13L8 3M2.1 3a0.9 0.9 0 1 0 1.8 0a0.9 0.9 0 1 0 -1.8 0M7.1 3a0.9 0.9 0 1 0 1.8 0a0.9 0.9 0 1 0 -1.8 0` |
| 95 | 5 | 0 | no | [0, 7, 8, 6, 1] | `M3 3L8 13L13 13L3 13L8 3M2.1 3a0.9 0.9 0 1 0 1.8 0a0.9 0.9 0 1 0 -1.8 0` |
| 96 | 5 | 1 | yes | [4, 2, 7, 6, 0] | `M8 8L13 3L8 13L3 13L3 3ZM7.1 8a0.9 0.9 0 1 0 1.8 0a0.9 0.9 0 1 0 -1.8 0` |
| 97 | 3 | 2 | no | [5, 6, 0] | `M13 8L3 13L3 3M11.1 8a1.9 1.9 0 1 0 3.8 0a1.9 1.9 0 1 0 -3.8 0` |
| 98 | 3 | 3 | no | [3, 6, 5] | `M3 8L3 13L13 8M1.6 6.6h2.8v2.8h-2.8z` |
| 99 | 3 | 4 | no | [8, 5, 6] | `M13 13L13 8L3 13M12.1 13a0.9 0.9 0 1 0 1.8 0a0.9 0.9 0 1 0 -1.8 0M2.1 13a0.9 0.9 0 1 0 1.8 0a0.9 0.9 0 1 0 -1.8 0` |

---

## 7. Ambiguities / notes

- The brief calls these "dot-grid patterns"; they are lattice-traced sigils with stroked end markers. Port the
  generator, not a bitmap table, if you want all 100 to match byte-for-byte.
- The marker "dots" are stroked circles (rings) because `fill="none"` on the shared `<svg>`; do not fill them.
- Identity icons live under **Appearance**, not General (source L611).
- Only 3 render sites read `SIGILS`; no other component draws a session sigil.
