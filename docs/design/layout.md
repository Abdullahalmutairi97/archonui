# ArchonUI UI — LAYOUT, WINDOW & MOTION spec

Extracted from `design/ArchonUI Agent v3.dc.html` (1689 lines; script starts L880) and the `support.js`
runtime render. Line refs are `L###` into that HTML file. Every value below is lifted from source or read
back out of the live DOM with Playwright (viewport 1600×1000) — see **§9 screenshots**.

> **Design canvas.** The whole app is one fixed **1920 × 1080** box: `position:fixed;left:50%;top:50%;
> width:1920px;height:1080px;transform:translate(-50%,-50%) scale(fitScale)` (L19). `fitScale =
> min(vw/1920, vh/1080)`, recomputed on resize (L1228). **All px values in this doc are design px**;
> at 1600×1000 everything is multiplied by 0.8333. Source: `fs` (root font-size) default **15 px**.
> Default theme is **Blood Moon** (`props.theme` default, script props L880); all concrete colours quoted
> here are Blood Moon and come with the derivation so any theme can be recomputed (§8).

---

## 1. Window

### 1.1 Frameless, no top bar
- `html,body{margin:0;height:100%;background:#000;overflow:hidden}` (L15). There is **no title bar,
  menu bar, toolbar or tabstrip**. The outermost chrome *is* the two sidebars; the wallpaper sits behind
  everything (L20–22).
- Root canvas (L19) is the only "window": `overflow:hidden`, `font-family:{{t.ui}}`, `font-size:{{fs}}px`,
  `color:{{t.fg}}`, `background:{{t.bg}}` (= `#0d0a08` for Blood Moon, `rgb(13,10,8)` confirmed),
  `user-select:none` (text selection is re-enabled only on messages/terminal/code, e.g. L130 `user-select:text`).

### 1.2 Where window controls live
Window controls exist in **exactly one place: the right tools sidebar** — never in the left sidebar and
never in the middle. All three call `winAct` which is a stub: it shows the toast *"Window controls are
handled by the OS"* (L1625). Titles are exactly `Minimize`, `Maximize`, `Close`.

**(a) Panel open — top-right of the tools panel header** (L351–353):

| # | element | geometry | icon | colour / hover |
|---|---------|----------|------|----------------|
| 1 | Minimize | 28×28, `border-radius:6px` | 14×14 line `M4 8h8` | `color:t.muted`; hover `background:t.line2; color:t.fg` |
| 2 | Maximize | 28×28, `border-radius:6px` | 14×14 square `x4 y4 w8 h8 rx1` | same |
| 3 | Close | 28×28, `border-radius:6px` | 14×14 cross | `color:t.muted`; hover **`background:t.acc; color:t.on`** (red hover) |

The header row order is: right-tab buttons → `flex:1` spacer → **collapse** button (34×34) → 1×16 px
divider (`background:t.line2; margin:0 4px`) → Minimize → Maximize → Close (L349–353). Header row:
`padding:10px 10px; gap:4px; border-bottom:1px solid t.line` (measured `10px`, `4px`).

**(b) Panel collapsed (52 px rail) — bottom of the rail** (L337–340). After the tab icons there is a
`<span style="flex:1">` (L336) so the three controls are pushed to the bottom edge, stacked vertically
with the rail's `gap:6px`: Minimize 28×28 → Maximize 28×28 → Close 28×28 (same icons/hover as above).

Collapsed-rail order top→bottom: collapse(expand) 34×34 · 5 tab icons 34×34 · flexible spacer ·
Minimize · Maximize · Close.

### 1.3 Outer padding / margins (geometry of the shell)
Computed in `renderVals` L1470–1473:

```js
g = gaps            // default 14   (settings range 0–32, step 1)
innerH = 1080 - 2*g // 1052
ct = g              // top of <main>
sx = 0; sh = 1080   // sidebars: left:0 / top:0, full canvas height
lw = leftOpen  ? 300 : 52 ;  lx = 0
rw = rightOpen ? rwOpen : 52 ; rx = 1920 - sx - rw      // rwOpen default 480, drag clamp 360–820
cx = sx + lw + g ; cw = rx - g - cx
```

| region | left | top | width | height |
|--------|------|-----|-------|--------|
| wallpaper | 0 | 0 | 1920 | 1080 (image layer `inset:-60px`, bleeds 60 px all round, L21) |
| left sidebar `<aside>` | 0 | 0 | `lw` 300 / 52 | 1080 |
| `<main>` (tiling) | 314 | **14** | 1112 | 1052 |
| right sidebar `<aside>` | 1440 (→ `1920 - rw`) | 0 | 480 / 52 | 1080 |
| panel resize handle | `rx - 5` = 1435 | 0 | 10 | 1080 |

So the shell insets are **only** the `g` gutter: sidebars are flush to `top/bottom/left/right`, and the
middle column is inset by `g` on all four sides (`left = lw+g`, `top = g`, `height = 1080-2g`).
Sidebar edges: left aside `border-right:1px solid t.line2`, right aside `border-left:1px solid t.line2`
(measured `1px solid rgba(243,234,216,0.22)`).

### 1.4 Overlays & window furniture
- Toast (L876): `left:50%; bottom:36px; transform:translateX(-50%); padding:11px 18px;
  border-radius:8px; background:t.pop; border:1px solid t.line2; font-size:13px;
  box-shadow:0 10px 30px rgba(0,0,0,.4)`; auto-dismiss **1800 ms** (L1258).
- Modals (share/settings) are in-canvas overlays, scrim `rgba(0,0,0,0.45)` for settings / `0.6` for
  share, `z-index:20`; lightbox scrim `rgba(0,0,0,0.85)`, `z-index:30`.
- z-index ladder: tiles ring `1`, resize handles `3`, tile popovers `5`, panel handle `6`, modals `20`,
  lightbox `30`.

---

## 2. Left sidebar

`<aside>` L27 — `position:absolute; left:0; top:0; width:{{lw}}px; height:1080px;
box-sizing:border-box; border-right:1px solid t.line2; background:{{sideBg}};
backdrop-filter:{{sideBlur}}; overflow:hidden; transition:width .3s, background .3s`.
**Expanded width 300 px, collapsed width 52 px** (L1473). No drag-resize.

### 2.1 Expanded (L44–105) — element order top→bottom

| # | element | geometry / typography | refs |
|---|---------|----------------------|------|
| 1 | **Header row** | `display:flex; gap:10px; padding:10px 14px 0 9px; width:300px; box-sizing:border-box` — measured `padding:10px 14px 0px 9px`, width 300 | L45 |
| 1a | ⇤ Collapse button | 34×34, radius 7, `color:t.muted`, hover `bg:t.line2; color:t.fg`; chevron svg 20×20, `stroke-width:1.6`, path `M10 3.5L5.5 8l4.5 4.5`; `title="Collapse sidebar"` | L46 |
| 1b | Logo | 22×22, `background:url(logo) center/contain no-repeat` | L47 |
| 1c | Brand name | `font-family:t.display` (Blood Moon = `'UnifrakturCook', serif` — blackletter), **24 px**, `line-height:1`; text = `appName` ("archonui") | L48 |
| 2 | **Segmented control** Sessions \| Projects | wrapper: `display:grid; grid-template-columns:repeat(2,1fr); gap:4px; margin:14px 14px 0; padding:4px; border-radius:9px; background:t.line`; buttons: `text-align:center; padding:8px 0; border-radius:6px; font-size:12px`; active `bg:t.line2, color:t.fg`; inactive `bg:transparent, color:t.muted`. Measured: cols `129.5px 129.5px`, gap 4, pad 4, radius 9, bg `rgba(243,234,216,0.09)` | L50–52 |
| 3 | **Scroll area** | `flex:1; overflow:auto; padding:16px 14px; display:flex; flex-direction:column; gap:10px; min-height:0` (measured `16px 14px`) | L56 |
| 3.1 | **"New session" button** | `display:flex; justify-content:space-between; padding:11px 14px; border-radius:8px; background:t.acc; color:t.on; font-weight:700`; label "New session"; shortcut hint right-aligned `font-size:11px; opacity:.75` text **`Alt ⏎`** (U+23CE). Measured bg `rgb(200,30,42)` = `t.acc`, `rgb(246,236,220)` = `t.on` | L58–60 |
| 3.2 | **Search box** | `display:flex; gap:8px; padding:9px 12px; border-radius:8px; border:1px solid t.line2`; leading `⌕` (U+2315) in `t.muted`; `<input placeholder="Search sessions" font-size:13px>`; filters on `session.name` and `session.project` (L1561) | L61–64 |
| 3.3 | **Groups** | one column per group, `display:flex; flex-direction:column; gap:3px`; group label `font-size:10.5px; letter-spacing:.16em; color:t.muted; padding:8px 4px 4px` = measured `letter-spacing:1.68px`, `padding:8px 4px 4px` | L66–67 |
| 3.4 | **Session row** | `display:flex; gap:11px; align-items:center; padding:8px 10px; border-radius:8px; background:si.bg; border:1px solid si.bd` | L69–74 |

**Groups:** exactly two, in order **`TODAY`** then **`EARLIER`** (L1564). Sessions with `when:'today'`
go to TODAY; everything else to EARLIER; empty groups are dropped.

**Session row internals (left→right):**
1. icon slot `20×20` (`position:relative`, `display:grid;place-items:center`, `color:si.icC`) with a
   16×16 stroke-1.5 sigil path + a status dot `8×8; border-radius:50%; border:2px solid t.tile` at
   `right:-2px; bottom:-2px` (L70–71).
   - icon colour: focused `t.acc2`; open-but-not-focused `t.fg`; otherwise `t.muted` (L1562).
   - dot colour (`dotFor`, L1510): `running → t.acc2`, `done → t.alt`, otherwise `t.faint`.
2. title `flex:1; min-width:0; font-size:13.5px; nowrap+ellipsis; color:t.fg`.
3. **harness badge** (L72): `height:18px; min-width:18px; padding:0 4px; box-sizing:border-box;
   border-radius:5px; display:grid; place-items:center; font-size:9.5px; font-weight:700;
   letter-spacing:.02em` (measured 18 px tall, `padding:0px 4px`, `letter-spacing:0.19px`).
   `title` = `"<harness name> · <project>"`.
   - Badge background/foreground: **focused** `bg:t.line2, color:t.acc2`; **not focused**
     `bg:t.line, color:t.alt` (L1562). Measured focused: bg `rgba(243,234,216,0.22)`, fg `rgb(217,181,106)`.
4. relative time `flex:none; width:30px; text-align:right; font-size:11px; color:t.muted` (L73).

**Harness badge labels (exact)** — `PROVIDERS[].i` (L934–941), keyed off the session's model provider:

| harness id | badge | full name | badge accent if unused |
|-----------|-------|-----------|------------------------|
| `claude` | **Cc** | Claude Code | fg `t.alt` (focused `t.acc2`) |
| `codex` | **Cx** | Codex CLI | " |
| `gemini` | **Gm** | Gemini CLI | " |
| `opencode` | **Oc** | OpenCode | " |
| `aider` | **Ai** | Aider | " |
| `goose` | **Gs** | Goose | " |

Badges use one shared colour pair (they are **not** per-harness coloured): `t.alt` `#c9a77a` on
`rgba(243,234,216,0.09)`, or `t.acc2` `#d9b56a` on `rgba(243,234,216,0.22)` when the row is focused.

**Relative-time format rules.** `ago` is a pre-formatted string; the only normalisation is
`'yesterday' → '1d'` (L1563). Observed/canonical set:
`now`, `Nm` (minutes, e.g. `4m`, `12m`, `40m`), `Nh` (`1h`, `3h`), `1d`, `2d`, `Nd`,
and **short weekday names** for older items (`Mon`, `Sun`). Same vocabulary is reused by the Git panel
log (`t`: `12m`/`40m`/`3h`/`1d`/`2d`, L952; new commits get `now`, L1676).

**Row states:** selected/focused → `background:t.soft` (`rgba(176,20,30,0.32)`), `border:1px solid t.line2`;
unselected → transparent bg, transparent border. Hover (any row) → `background:t.line2` (instant, no
transition). *(Fields `meta`, `halo`, `tag`, `icBg` are computed at L1562–1563 but not consumed by this
markup — dead for porting.)*

### 2.2 Projects view (L80–96) — what it shows and its controls
Replaces the Sessions block inside the same scroll area when the segmented control is on *Projects*.
It renders one card per project (`PROJECTS`, L992–997) plus a footer button — **2 controls total**:

- **Card** (L82–92): `display:flex; flex-direction:column; gap:8px; padding:14px; border-radius:9px;
  border:1px solid pj.bd; background:pj.bg`. Active project → `border:t.acc`, `bg:t.soft`; others →
  `border:t.line2`, transparent. Row 1: name `font-family:t.display; font-size:20px; color:t.fg`
  (display font!) + session count `font-size:11px; color:t.muted` right-aligned
  (`"<n> sessions"`). Row 2: path `font-size:12px; color:t.muted`. Row 3: branch chip
  `padding:3px 8px; border-radius:5px; background:t.line2; color:t.alt` prefixed `⎇`, and language
  `font-size:11.5px` as `● <Language>` in the project's accent role
  (`archonui-core→alt #c9a77a`, `archonui-web→acc #c81e2a`, `archonui-cli→acc2 #d9b56a`, `ember-gl→muted #bfae94`).
- **Footer button "+ Open folder"** (L94): `text-align:center; padding:11px; border-radius:8px;
  border:1px dashed t.line2; color:t.muted; font-size:13px`.
- **Clicking a card selects that project** (`pick` → sets `project`, flashes *"Switched to …"*) — it
  becomes the cwd used by the terminal prompt, Git panel, Files panel and new sessions.
- **"+ Open folder"** is a stub: flashes *"Folder picker opens here"* (L1631).
- **Gap vs the brief:** the prototype has **no create / rename / delete project UI** — no context menu,
  no edit button, no overflow menu. Only *select* and *open folder*. Treat create/rename/delete as
  undesigned; raise it before porting them.

### 2.3 Footer / bottom bar (L99–105)
`display:flex; gap:6px; padding:12px 14px; border-top:1px solid t.line` (measured pad `12px 14px`,
border `rgba(243,234,216,0.09)`).
- **Settings** button: `flex:1; display:flex; gap:10px; padding:9px 12px; border-radius:8px;
  font-size:13px; color:t.fg`, gear svg 18×18, leading icon wrapped in `color:t.muted`; background
  `settingsBg` = `t.line2` while the Settings modal is open, else transparent; hover `bg:t.line2`.
- **Share** button: `width:38px`, `display:grid; place-items:center; border-radius:8px; color:t.muted`,
  share svg 18×18; background `shareBg` (`t.line2` when the share modal is open); hover
  `bg:t.line2; color:t.fg`; `title="Share"`.

### 2.4 Collapsed rail (L29–42) — icons on rotating colour bands
Structure: `height:100%; display:flex; flex-direction:column; align-items:center; gap:4px;
padding:10px 0` (L30). Top→bottom:

| # | element | geometry |
|---|---------|----------|
| 1 | Expand button `title="Expand sidebar"` | 34×34, radius 7, `color:t.muted`, chevron 20×20 `M6 3.5L10.5 8 6 12.5` |
| 2 | New session button | 34×34, radius 7, `color:t.fg`, plus svg 17×17 |
| 3 | Divider | `span 20×1; background:t.line2; margin:6px 0; flex:none` |
| 4 | Session icon list | `flex:1; overflow:auto; flex-direction:column; align-items:center; gap:6px; padding:2px 0` |
| 5 | Share | 34×34, radius 7, `color:t.fg` |
| 6 | Settings | 34×34, radius 7, `color:t.fg` |

**Rail session button** (L36): `36×36; flex:none; border-radius:9px; display:grid; place-items:center;
color:rs.icC; background:rs.bg`; 17×17 sigil svg; status dot `8×8; border-radius:50%;
border:2px solid t.panel` at `right:1px; bottom:1px`. Hover `color:t.fg`.
Icon colour: focused `t.acc2`, open `t.fg`, else `t.muted` (L1654).

**The rotating accent bands (exact).** `railSess` background is computed at L1658:

```js
bg = rgba( mix(t.acc, t.acc2, (i % 6) * 0.07),  focus === session[i] ? 0.42 : 0.2 )
// mix(a,b,k) = per-channel round(a + (b-a)*k)  in sRGB, hex→hex  (L1465)
```

So there are **6 bands in a fixed order**, cycling with the session list index `i`, interpolating
linearly from the theme accent `t.acc` to the highlight `t.acc2` in steps of **0.07** (k = 0, .07, .14,
.21, .28, .35), then flattened through a single alpha: **0.20** for normal, **0.42** for the focused
session. Bands repeat every 6 rows (band 7 = band 1). "Geometry" is the button box itself: 36×36,
radius 9, `gap:6px` between rows, `padding:2px 0`.

Blood Moon (`t.acc = #c81e2a`, `t.acc2 = #d9b56a`) — browser-confirmed, in order:

| i | i%6 | k | mix hex | rendered `background` |
|---|-----|---|---------|-----------------------|
| 0 | 0 | 0.00 | `#c81e2a` | `rgba(200, 30, 42, 0.42)` *(focused)* |
| 1 | 1 | 0.07 | `#c9292e` | `rgba(201, 41, 46, 0.20)` |
| 2 | 2 | 0.14 | `#ca3333` | `rgba(202, 51, 51, 0.20)` |
| 3 | 3 | 0.21 | `#cc3e37` | `rgba(204, 62, 55, 0.20)` |
| 4 | 4 | 0.28 | `#cd483c` | `rgba(205, 72, 60, 0.20)` |
| 5 | 5 | 0.35 | `#ce5340` | `rgba(206, 83, 64, 0.20)` |

For any other theme substitute its `acc`/`acc2` in the same formula. *(Note: `hueOf` at L1509 builds a
6-tone palette `[acc, acc2, alt, deep, mix(acc,alt,.5), mix(acc2,fg,.4)]` — it is **never called**;
the rail bands above are the real implementation.)* No ring/border is drawn in the rail (`rs.ring` is
also unused).

---

## 3. Session tiles (centre tiling)

`<main>` L108: `position:absolute; left:{{cx}}px; top:{{ct}}px; width:{{cw}}px; height:{{innerH}}px`.
Empty state (L109–114): centred column `gap:18px` with logo 72×72 (`opacity:.8`), brand
`font-size:56px` display + `text-shadow:0 2px 24px rgba(0,0,0,.6)`, and a "New session" button
`padding:12px 22px; border-radius:8px; background:t.acc; color:t.on; font-weight:700`.

### 3.1 Tile chrome
Each tile (L117) is an absolutely-positioned `<section>`:
`left/top/width/height` from the layout (±1 px rounding), `padding:2px; box-sizing:border-box;
border-radius:{{rad}}px; transition:{{tileTr}}`. `rad` = **radius setting, default 9** (range 0–20);
inner radius `rad2 = max(0, rad-2)` = **7**.

- **Ring (focus border)**, L118: full-bleed `position:absolute; inset:0; padding:2px; border-radius:rad;
  background:{{tl.ring}}` masked into a 2 px frame via
  `-webkit-mask:linear-gradient(#000 0 0) content-box, linear-gradient(#000 0 0);
  -webkit-mask-composite:xor; mask-composite:exclude; pointer-events:none; z-index:1`.
  Measured `-webkit-mask-composite: exclude, exclude`.
- **Glass body**, L119: `width:100%; height:100%; border-radius:rad2; background:{{tl.bg}};
  backdrop-filter:{{tl.blur}}; display:flex; flex-direction:column; overflow:hidden`.
- **Header** (L120): `display:flex; align-items:center; gap:10px; padding:12px 12px 12px 16px;
  border-bottom:1px solid t.line` (measured exactly; border `rgba(243,234,216,0.09)`).

Header contents:
1. icon slot `20×20` + status dot `8×8`, `border:2px solid t.tile`, at `right:-2px; bottom:-2px`.
2. title block `display:flex; flex-direction:column; gap:2px; min-width:0; flex:1`:
   - **title**: `font-family:t.display` — for Blood Moon that is **`'UnifrakturCook', serif`**, a
     **blackletter display face** (theme-driven; L902). `font-size:20px; line-height:1.1`
     (measured **20 px / 22 px**), `white-space:nowrap; overflow:hidden; text-overflow:ellipsis`,
     colour `t.fg`.
   - **subtitle / meta**: `font-size:11.5px; color:t.muted`, same ellipsis. Built at L1521 as
     `` `${project} · ${model.s}${effName ? ' · '+effName.toLowerCase()+' effort' : ''}${mode!=='Auto' ? ' · '+mode.toLowerCase()+' mode' : ''}${running ? ' · running' : ''}` ``
     → e.g. `archonui-core · Opus 4.5 · high effort` (browser-confirmed), plus `· plan mode` / `· ask mode`
     when the mode isn't Auto, plus `· running` while streaming.
3. `+` split button (L126): 28×28, radius 6, `font-size:17px`, `color:t.muted`, hover `bg:t.line2`,
   `title="Split new session"`.
4. `×` close button (L127): 28×28, radius 6, `font-size:17px`, `color:t.muted`, hover `bg:t.line2`,
   `title="Close tile"`.

Gaps between tiles come from the layout maths, not CSS margins: **`g` px** (default 14) between rects,
plus 2 px inner padding on each side ⇒ ~18 px visual gutter at default.
Messages area `data-msgs` (L130): `flex:1; overflow:auto; min-height:0; padding:18px 18px 8px;
display:flex; flex-direction:column; gap:16px; user-select:text`.
Composer (L233–321): wrapper `border-radius:10px; border:1px solid tl.compBd; background:t.pop;
position:relative` — focused tile `compBd = t.line2`, unfocused `t.line` (L1547). textarea
`padding:12px 14px 6px; line-height:1.5; font-size:15px` (measured 22.5 px LH). Button row
`padding:6px 8px 8px; gap:6px`; attach 32×32 radius 7; model button 32 px tall radius 7 `font-size:13px`;
send button **34×34, `border-radius:50%`**, background `running?t.fg : draft?t.acc : t.line2`.
Outer wrapper `padding:8px 12px 12px`.

**Popovers** (both inside the tile, `bottom:calc(100% + 6px)`, `padding:6px; border-radius:10px;
background:t.pop; border:1px solid t.line2; box-shadow:0 14px 40px rgba(0,0,0,.45); z-index:5`):
model/effort picker `right:8px; width:360px` (L265, browser-confirmed 360 px), attach picker
`left:8px; width:300px` (L307, confirmed 300 px). Both fly up from the composer row.

### 3.2 Splitting, closing, resizing

**Layout algorithm** (`renderVals` L1487–1505). Two modes (`layout` setting, default **Dwindle**):

*Dwindle* — walk tiles left→right over a shrinking rect `r`; for tile `i` (all but the last):
```js
k    = ratios[i] ?? 0.5                      // per-split fraction, clamp 0.12–0.88 while dragging
vert = er.w >= er.h * 0.9                    // ← alternating by aspect ratio
if (vert) split vertically  : rect = {...r, w:(r.w-g)*k}; r = {x:r.x+w+g, w:r.w-g-w, ...}
else      split horizontally: rect = {...r, h:(r.h-g)*k}; r = {y:r.y+h+g, h:r.h-g-h, ...}
```
`er` is the remaining-area estimate used only to choose the axis: **split vertically while the
remaining box is at least 0.9× as wide as it is tall, horizontally once it is not** — the classic
dwindle alternation. **The last tile takes the leftover rect unchanged.** Fractions default to 0.5
(even split) and are per-split-index (`ratios[i]`), reset by the "Reset sizes" button (L1395 targets
`ratios:{}`), or per-handle by double-click (`reset` → `ratios[i] = 0.5`, L1507).

*Master* (L1490–1495), when `n > 1`: first tile `w = round((cw-g) * masterR)` full height
(`masterR` default **0.56**, clamp 0.12–0.88); remaining `n-1` tiles stack vertically with
`h = (innerH - g*(n-2)) / (n-1)`.

**Split.** `+` on a tile sets focus to that tile then runs `newSession` (L1527): a new session is
inserted **immediately after the focused tile's index**, and it becomes focused. Max **6 tiles** — when
`tiles.length >= 6` the focused tile is dropped first (L1279), so the split still lands.
Working example (default 3 tiles `[s1,s2,s4]`, dwindle, g=14, cw=1112, innerH=1052): rects
`(0,0,549,1052)`, `(563,0,549,519)`, `(563,533,549,519)`; after `+` on the first tile → 4 tiles
`(0,0,549,1052)`, `(563,0,549,519)`, `(563,533,268,519)`, `(845,533,268,519)` — all browser-confirmed.

**Close / merge.** `×` removes the tile (L1284); focus moves to the tile that was before it
(`tiles[max(0,i-1)]`, else the first) and the remaining tiles re-flow into the freed space with the
0.32 s transition (no explicit "merge animation" — the rect change *is* the animation). Closing the last
tile shows the empty state. Opening a session from the sidebar while it is **not** already tiled
**replaces the focused tile** (`openSession`, L1282) rather than adding one; if it *is* tiled it just
gets focus.

**Resize handles** (L323–325, built at L1504–1508): one per split. Hit box
`display:grid; place-items:center`, `cursor:col-resize` (vertical split) / `row-resize` (horizontal),
`z-index:3`; size = `hw = max(g, 12)` thick (**14 px** at default) by the full split length; centered on
the gutter (`x = splitLine + g/2 - hw/2`). Grip: `span` **3×44** (vertical) / **44×3** (horizontal),
`border-radius:2px; background:t.line2`, `transition:background .15s`, turns `t.acc2` while dragging.
Drag updates `ratios[i]` (or `masterR`) with clamp 0.12–0.88 and shows `row-resize`/`col-resize` on
`body`; double-click resets that split to 0.5 (master → 0.56).
Panel handle (L514): 10 px wide strip at `left:rx-5`, full height, `z-index:6`, grip 3×44 radius 2;
drag clamps `rwOpen` to **360–820**; double-click resets to **480**.

**Min sizes.** There is no explicit min-width/height — only the fraction clamps (0.12–0.88 of the
gutter-adjusted space) and the 6-tile cap. At the default 1112×1052 centre, 2–3 tiles bottom out at
≈132 px wide (the last split of the full width, `(1112-14)*0.12`); with 4+ tiles the clamp applies
inside an already-split box (≈64 px), so a real port should add a minimum tile size.

### 3.3 Focus vs unfocused styles
Focus = the tile whose session id is `focus`; it changes on `mousedown` anywhere in the tile, on
composer focus, on `+`, and via Alt+arrows. With the default background style **Canvas** (L1519–1521):

| property | focused | unfocused |
|----------|---------|-----------|
| ring (2 px, masked) | `linear-gradient(135deg, t.acc2, t.acc 50%, t.deep)` → `linear-gradient(135deg, rgb(217,181,106), rgb(200,30,42) 50%, rgb(90,10,16))` | `t.line2` solid `rgba(243,234,216,0.22)` |
| body background | **`rgba(t.tile, 0.82)`** → `rgba(22,16,13,0.82)` | **`linear-gradient(180deg, rgba(22,16,13,0.25), rgba(22,16,13,0.6) 45%, rgba(22,16,13,0.96) 85%)`** |
| body backdrop-filter | **`blur(8px)`** | **`none`** |
| header icon colour | `t.acc2` | `t.fg` |
| composer border | `t.line2` | `t.line` |
| ring alpha / blur | 100 % gradient, blur 8 px | 0.22 border, **no** blur (fully opaque-ish fade) |

Two special cases (same table): **focused but empty** → background `rgba(t.tile,0.12)`, blur `blur(2px)`;
and the *settings* option **"Tint all sessions"** (`tintAll`) makes every tile use the focused style.
Under any non-Canvas background style the focus distinction disappears — every tile uses `BG.tile`/`BG.tb`
(§5). Focus changes are **instant** (no transition on background/blur/ring).

---

## 4. Right tools panel

`<aside>` L329: `position:absolute; left:{{rx}}px; top:0; width:{{rw}}px; height:1080px; box-sizing:border-box;
border-left:1px solid t.line2; background:{{sideBg}}; backdrop-filter:{{sideBlur}}; overflow:hidden;
transition:{{sideTr}}`. **Expanded width 480 px (default), collapsed 52 px**; expanded range 360–820 via
the drag handle (§3.2).

**Expanded header** (L343–353, browser-confirmed): `display:flex; align-items:center; gap:4px;
padding:10px 10px; border-bottom:1px solid t.line`, containing in order:
1. five tab buttons — `padding:7px 8px; border-radius:6px; font-size:12.5px`; active
   `background:t.line2; color:t.fg`, inactive transparent/`t.muted`. Tabs, in order (L1584):
   **Browser · IDE · Files · Git · Terminal**; default `rightTab = IDE` (props default).
2. `flex:1` spacer.
3. **Collapse** button 34×34, radius 6, `title="Collapse"` (chevron pointing right, 20×20).
4. divider `1×16px`, `background:t.line2`, `margin:0 4px`.
5. Minimize / Maximize / Close 28×28 (§1.2a).

Panel bodies (unchanged chrome, listed for completeness): Browser (`‹ › ↻` + URL pill + faux page,
L363+), IDE (file tabs `border-radius:6px 6px 0 0`, breadcrumb bar `background:t.line`, code
`font-family:t.code; font-size:12.5px; line-height:1.7`, status bar L404–407), Files (tree rows
`height:30px; padding:0 10px 0 {{pad}}px; border-radius:6px; font-size:13px`, indent step 16 px,
`pad = 10 + depth*16`, guide lines at `x = 15 + k*16`), Git (changes, checkbox 14×14 radius 3,
2-row commit box, log grid `14px 64px 1fr auto`), Terminal (`font-family:t.code; font-size:12.5px;
line-height:1.6; padding:14px 16px`).

**Collapsed rail (52 px, L331–340):** centred column, `gap:6px; padding:10px 0`, top→bottom:
collapse/expand 34×34 (radius 6) → the 5 tab icons 34×34 (radius 7, active `bg:t.line2`, hover
`color:t.fg`) → `flex:1` spacer → Minimize/Maximize/Close 28×28 (radius 6) pinned to the bottom.
Tab icons are 18×18 `stroke-width:1.5`.

---

## 5. Glass

Two knobs: `blur` = "Panel blur", **default 24 px**, range 0–48 (L1394); `dim` = "Wallpaper dim",
default 0 %, range 0–80 in 5 % steps. Derived strings:

```js
blurS = 'blur(24px) saturate(1.2)'                    // sidebars (L1474)
TR(a) = rgba(t.tile,  a)      PR(a) = rgba(t.panel, a) // L1469
codeBg = rgba(t.bg, 0.72)                              // code/chart/image block fill (L1617)
wallFilter = (BG.wf || '') + (dim ? ' brightness(1-dim/100)' : '')  // L1619
```

Background styles `BGS` (L1475–1482), default **Canvas** — verbatim:

| style | wallpaper opacity | sidebar fill `sideBg` | sidebar blur | tile fill | tile blur |
|-------|------------------|----------------------|--------------|-----------|-----------|
| **Canvas** *(default)* | 1 | `PR(0.72)` → `rgba(15,11,9,0.72)` | `blur(24px) saturate(1.2)` | focused `TR(0.82)`, unfocused `linear-gradient(180deg,TR(0.25),TR(0.6) 45%,TR(0.96) 85%)` | focused `blur(8px)`, unfocused `none` |
| Glass | 1 | `PR(0.6)` | `blurS` | `TR(0.55)` | `blur(24px)` |
| Solid | 0 | `t.panel` (opaque `#0f0b09`) | `none` | `t.tile` (`#16100d`) | `none` |
| Gaps | 1 | `PR(0.96)` | `none` | `TR(1)` | `none` |
| Tinted | 0.5 | `PR(0.55)` | `blurS` | `TR(0.35)` | `blur(12px)` (= blur/2) |
| Ambient | 1 (`filter: blur(28px) brightness(0.55)`) | `PR(0.5)` | `blurS` | `TR(0.45)` | `blur(6px)` |

`PR(0.72)` for Blood Moon = `rgba(15,11,9,0.72)` — browser-confirmed on both sidebars, together with
`backdrop-filter: blur(24px) saturate(1.2)`. Wallpaper image layer: `position:absolute; inset:-60px;
background:url(t.wall) center/cover no-repeat` (L21, confirmed) — the 60 px bleed is what stops the
blur from exposing edges.

Other glass surfaces: popovers/menus `t.pop` opaque (`#1b1411`) + `backdrop-filter` inherited from the
tile; code/chart/image cards `codeBg = rgba(13,10,8,0.72)` with `border:1px solid t.line2`; modal
dialogs `t.pop` opaque; preset-palette strip inside Appearance uses `rgba(0,0,0,0.5); backdrop-filter:blur(6px)`.

---

## 6. MOTION

Everything is CSS transitions — **there is no `@keyframes`, no `@media`, no `animation-*` anywhere in
the file** (grep-confirmed), and **no `prefers-reduced-motion` handling** (see §6.3).

| # | animated thing | property | duration | easing | source |
|---|----------------|----------|----------|--------|--------|
| 1 | wallpaper wrapper | `opacity` | **0.4 s** | `ease` | L20 |
| 1 | wallpaper wrapper | `left`, `width` | **0.3 s** each | `ease` | L20 |
| 2 | **left sidebar collapse/expand** | `width`, `background` | **0.3 s** each | `ease` | L27 |
| 3 | **right panel collapse/expand** | `width`, `left`, `background` | **0.3 s** each | `ease` | L329 (`sideTr`) |
| 3b | right panel *while dragging* | `background` only | 0.3 s | `ease` | L1652 (`sideTr = 'background .3s'` when `drag`) |
| 4 | **tile split / merge / reflow** | `left`, `top`, `width`, `height` | **0.32 s** each | **`cubic-bezier(.2,.8,.2,1)`** | L117 (`tileTr`) |
| 4b | tile while dragging a split | *none* | — | — | L1651 (`tileTr = 'none'` when `drag`) |
| 5 | resize-handle grip | `background` | **0.15 s** | `ease` | L325 |
| 6 | effort slider fill (gradient bar) | `width` | **0.25 s** | `ease` | L292 |
| 7 | effort level moon buttons | `filter`, `color` | **0.2 s** each | `ease` | L294 |
| 8 | every toggle switch: track | `background` | **0.2 s** | `ease` | L600/856 + share L537 |
| 8 | every toggle switch: knob | `left` | **0.2 s** | `ease` | L600/856 |
| 9 | hover states (`style-hover`) — session rows, buttons, tree rows, chips | `background` / `color` / `border-color` | **instant (0 s)** | — | `style-hover` attrs |
| 10 | popovers / pickers appearance | **none** | — | — | model & attach popovers mount with no transition (checked computed styles) |

Confirmed by scanning every element's computed transition in the live DOM: the only non-zero entries are
the four groups above (wallpaper `opacity 0.4s, left 0.3s, width 0.3s`; both `<aside>`s
`width/left/background 0.3s`; every tile `<section>` the 0.32 s cubic-bezier; the handle grips
`background 0.15s`). Toggle track/knob transitions were confirmed inside the Settings modal
(`background 0.2s` / `left 0.2s`).

**Focus changes are not animated** — ring colour and tile background/blur swap instantly.

### 6.1 Timer-driven (not CSS) motion
- Toast lifetime **1800 ms** (L1258); "Copied" label resets after **1500 ms** (L1638).
- Simulated agent stream: tool rows appear at `500 + i*450` ms, the reply lands at
  `700 + n*450` ms (L1312–1321); `n` = number of tools derived from effort fraction.
- Message lists are force-scrolled to the bottom on the next `requestAnimationFrame` after each change
  (`scrollAll`, L1263).
- Effort → thinking-time: `think = round(2 + frac*29)` s, displayed as `thought for Ns`.

### 6.2 Interaction timings worth porting
- Split fraction clamp `0.12…0.88`; master fraction `0.12…0.88`; panel width `360…820`.
- Double-click a split handle → 0.5; double-click the master handle → 0.56; double-click the panel
  handle → 480.
- Body cursor is forced to `row-resize`/`col-resize` for the whole drag (L1236).
- Tiling shortcuts (listed in Settings ▸ Keyboard, L1391): **Alt ⏎** split new session, **Alt Q** close
  tile, **Alt ←/→/↑/↓** move focus between tiles, **Alt T** next theme, **⇧ Tab** cycle agent mode,
  **⏎ / ⇧⏎** send / newline, **Esc** close dialog. Implemented in the global `keydown` handler
  (L1244–1250); Alt+arrows wrap around the tile list.

### 6.3 Reduced motion
**Not implemented.** No `prefers-reduced-motion` query, no reduced-motion variants of any transition, no
alternative instant path. (There is also no `@media` block at all, and no focus-visible styling — the
prototype uses `style-hover` only.) If the port must honour reduced motion, that is a new decision:
nothing in the spec says which transitions to drop.

---

## 7. Global metrics

### 7.1 Type scale (design px; families from the theme pairing)
Families: `t.display` / `t.ui` / `t.code`. Blood Moon pairing index 0 (L902) =
**display `'UnifrakturCook', serif` (blackletter)**, **ui `'JetBrains Mono', monospace`**;
`t.code` = the ui font when it looks mono, else `'Geist Mono', monospace` (L1468). Root
`font-family:t.ui; font-size:15px`.

| role | size | line-height | letter-spacing | family | ref |
|------|------|-------------|----------------|--------|-----|
| root / inherited body text | **15 px** (setting 13–19) | normal | — | ui | L19 / L1394 |
| brand in sidebar header | 24 px | 1 | — | display | L48 |
| no-tiles hero brand | 56 px | — | — | display | L112 |
| settings nav brand / dialog title | 26 px / 36 px | 1 / 1 | — | display | L575 / L590 |
| project card name (sidebar) | 20 px | — | — | display | L84 |
| **tile title** | **20 px** | **1.1** (22 px) | — | display | L123 |
| tile empty-state question | 28 px | — | — | display | L134 |
| popover effort value / share name | 22 px | 1 / 1.15 | — | display | L290 / L525 |
| section labels (TODAY, CHANGES, MODE, ATTACH, EFFORT, …) | 10.5–11 px | — | **.16em** (≈1.68 px @10.5) | ui | L67, L267 |
| capability/model table header | 10.5 px | — | **.14em** | ui | L698 |
| agent reply byline (`ARCHONUI`) & code lang | 11 px | — | **.12em** | ui | L159 / L217 |
| segmented control labels | 12 px | — | — | ui | L52 |
| settings nav items | 13.5 px | — | — | ui | L578 |
| session row title | 13.5 px | — | — | ui | L71 |
| session row time | 11 px | — | — | ui | L73 |
| **harness badge** | 9.5 px, 700 | — | **.02em** | ui | L72 |
| file-type tag | 9.5 px, 700 | — | .04em | ui | L425 |
| tile meta subtitle | 11.5 px | — | — | ui | L124 |
| group label | 10.5 px | — | .16em | ui | L67 |
| search input | 13 px | — | — | ui | L63 |
| composer textarea | 15 px | 1.5 | — | ui | L245 |
| user bubble | inherits 15 px | **1.55** | — | ui | L155 |
| agent prose | inherits | **1.6** | — | ui | L173 |
| code block | 12.5 px | **1.65** | — | code | L220 |
| chart values / bars | 11 px | — | — | ui | L187 |
| right-panel tab label | 12.5 px | — | — | ui | L348 |
| tools-panel IDE code / terminal | 12.5 px | 1.7 / 1.6 | — | code | L398 / L497 |

### 7.2 Radii scale
`2` (resize grip) · `3` (checkbox, guide chips) · `5` (badges, tags, chips) · `6` (small buttons, tree rows,
segmented items, icon buttons in the right panel) · `7` (rail buttons, 32 px composer buttons, attach
popover rows) · `8` (session rows, search, New-session, footer buttons, code chips, settings nav) ·
`9` (segmented control wrapper, rail session button, project card, toast chips) · `10` (composer
wrapper, popovers, cards) · `12` (share dialog) · `14` (settings dialog) · `50%` (send button, effort
nodes, status dots, avatars). Tiles: outer `rad` (setting, default **9**), inner `max(0, rad-2)` = **7**.
Off-scale values: `border-radius:10px 10px 3px 10px` (user bubble), `6px 6px 0 0` (IDE tabs).

### 7.3 Spacing scale
Base gutter `g` = **14 px** (setting 0–32). Canonical paddings: `2` (tile frame) · `4` (segmented
track padding, small gaps) · `6` (toolbars, popover padding, right-panel icon gaps) · `8` (buttons,
composer bar) · `9–10` (search/rows/footer/panel header) · `11` (New session, "+ Open folder") ·
`12` (composer sides, card rows) · `14` (sidebar padding, project cards, modal gutters) · `16` (sidebar
scroll, tile messages) · `18` (message area, empty state) · `22–26–36` (settings dialog). Common gaps:
`2,3,4,6,8,10,11,16`; session-row gap **11 px**; sidebar section gap **10 px**; tile message gap **16 px**;
rail gaps **4 px / 6 px**. Fixed heights: header 34 px rows, session rows ≈40 px, tree rows 30 px,
composer bar 32 px, toggles 18 px, badges 18 px, hit targets 28/32/34/36/38 px.

---

## 8. Default theme token table (Blood Moon) — needed to compute §5/§6 values

`t.acc`/`t.acc2` are the two colours the rail bands, rings and accents derive from.
Base row = `THEMES['Blood Moon']` (L884); `TUNE` row = override applied **only** to Blood Moon (L1463);
`final` = `{...base, ...TUNE}` then `line = rgba(fg,.09)`, `line2 = rgba(fg,.22)`,
`soft = acc-overridden ? rgba(acc,.28) : soft`, `border = linear-gradient(135deg, acc2, acc 50%, deep)`.

| token | base | TUNE | final (Blood Moon) | browser-confirmed as |
|-------|------|------|--------------------|----------------------|
| bg | `#0d0a08` | — | `#0d0a08` | `rgb(13,10,8)` |
| tile | — | `#16100d` | `#16100d` | `rgba(22,16,13,0.82)` @0.82 |
| panel | `rgba(13,10,8,0.80)` | `#0f0b09` | `#0f0b09` | `rgba(15,11,9,0.72)` |
| pop | `rgba(18,13,10,0.97)` | `#1b1411` | `#1b1411` | `rgb(27,20,17)` |
| fg | `#eadfcc` | `#f3ead8` | `#f3ead8` | `rgb(243,234,216)` |
| muted | `#9a8c78` | `#bfae94` | `#bfae94` | `rgb(191,174,148)` |
| faint | `#43382c` | `#6a5a48` | `#6a5a48` | `rgb(106,90,72)` (idle dot) |
| line | `rgba(234,223,204,0.06)` | — | `rgba(243,234,216,0.09)` | `rgba(243,234,216,0.09)` |
| line2 | `rgba(234,223,204,0.15)` | — | `rgba(243,234,216,0.22)` | `rgba(243,234,216,0.22)` |
| acc | `#b0141e` | `#c81e2a` | `#c81e2a` | `rgb(200,30,42)` |
| acc2 | `#d9b56a` | — | `#d9b56a` | `rgb(217,181,106)` |
| deep | `#5a0a10` | — | `#5a0a10` | in ring gradient |
| alt | `#c9a77a` | — | `#c9a77a` | `rgb(201,167,122)` |
| on | `#f6ecdc` | — | `#f6ecdc` | `rgb(246,236,220)` (text on accent) |
| soft | `rgba(176,20,30,0.24)` | `rgba(176,20,30,0.32)` | `rgba(176,20,30,0.32)` | selected row bg |
| border | gradient(acc2, acc 50%, deep) | — | `linear-gradient(135deg,#d9b56a,#c81e2a 50%,#5a0a10)` | focused tile ring |
| logo / wall | `logo-bm.png` / `assets/fhd/bloodmoon2.jpg` | — | same | wallpaper url in DOM |

Non-Blood-Moon themes skip `TUNE` entirely and instead derive
`tile = mix(bg,fg,.05)`, `panel = mix(bg,fg,.02)`, `pop = mix(bg,fg,.08)`,
`muted = mix(muted,fg,.3)`, `faint = mix(faint,fg,.12)` (L1467). Full base palettes: L883–896;
font pairings: L900–913 (Blood Moon at L902) — per-theme index switchable in Settings → Appearance.

---

## 9. Browser verification & screenshots (`.qa/design/`)

Playwright steps: `layout-steps-1..5.json`. Screenshots (1600×1000, scale 0.8333):

| file | state |
|------|-------|
| `layout-default.png` | default: 3 tiles, both sidebars expanded |
| `layout-left-collapsed.png` | left rail 52 px — shows the 6 accent bands |
| `layout-projects.png` | left sidebar in Projects view |
| `layout-split.png` | after `+` — 4 tiles, dwindle alternation visible |
| `layout-right-collapsed.png` | right rail 52 px with bottom window controls |
| `layout-model-popover.png` | tile model/effort popover (360 px) |
| `layout-settings.png` | settings modal (1320×880) |

Confirmed readings (live computed styles): sidebar width 300→52 and master rect left 314→66; right
480→52; tile rects before/after split (see §3.2); ring gradients; tile body bg/blur for focused vs
unfocused; handle geometry 14 px / 3×44; `.05` bands list; header padding `12px 12px 12px 16px`;
title 20 px/22 px `UnifrakturCook`; meta 11.5 px; badge `Cc` 18 px; brand 24 px; project card name
20 px; right-panel header `10px`+`4px` gap+1 px divider; composer `radius:10px`, textarea 15 px/22.5 px,
send 34×34 circle; toggle track 34×18 with `background .2s` + knob 14×14 with `left .2s`; effort node
38×38 `border-radius:50%` with `filter .2s, color .2s` and `drop-shadow(0 0 8px rgb(200,30,42))` when
selected; settings dialog 1320×880 radius 14 with nav 236 px; all transition lists (§6).

---

## 10. Ambiguities / gaps found

1. **Project create / rename / delete do not exist** in the prototype (§2.2). Only *select* and a stub
   *"+ Open folder"*. Needs a product decision before porting.
2. **Window controls are a stub** — `winAct` only toasts *"Window controls are handled by the OS"*. The
   layout (right-panel header / rail bottom) is specified, but the OS integration is not.
3. **No reduced-motion support** anywhere (§6.3).
4. **Hover styling is instant** (no transition) and there is no `:focus-visible` styling — keyboard focus
   affordances for tiles/rows are undesigned.
5. **Left sidebar width is fixed** (300/52, not draggable) while the right panel is (360–820). Only the
   right panel has a gutter handle.
6. Dead code that looks like spec but is unused: `hueOf` 6-tone palette (L1509), `rs.ring` in the rail,
   and the session-row `meta`/`halo`/`tag`/`icBg` fields — do **not** port them as behaviour.
7. The rail "6-colour accent bands" are really a **6-step `acc→acc2` sRGB interpolation** at alphas
   0.20/0.42 (§2.4); "6 colours" only holds for the 6 seeded sessions — band index is `i % 6`.
