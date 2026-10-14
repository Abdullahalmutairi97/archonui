# ArchonUI theme system — extracted spec

Source of truth: `design/ArchonUI Agent v3.dc.html`. Every value below is lifted verbatim from that file or computed with
the prototype’s own formulas (reproduced in §2). `L###` = line in the prototype. Machine-readable twin:
`docs/design/themes.json`. JSON shape: `{ meta, themes[{ name, order, default, tokens, presetTokens, wallpaper, logo, render, bands[6],
  bandAlphas, fonts[3] }], backgroundStyles[6], fonts[77], tokenDerivation, editableColourTokens[12], runtimeDefaults, switching }`.

---

## 1. Theme list, order and default

`THEMES` is defined at L883–896 and `NAMES = Object.keys(THEMES)` at L897, so the canonical order is:

`ArchonUI` · `Blood Moon` · `Moonlight` · `Wreck` · `Citadel` · `Neon` · `Violet` · `Ink` · `Bonfire` · `Hunter` ·
`Shards` · `Vale`

The designer-prop declaration (L881) lists them in a different order — `Blood Moon` first — and sets
`"default":"Blood Moon"`:

> `theme` enum options (L881): Blood Moon, ArchonUI, Moonlight, Wreck, Citadel, Neon, Violet, Ink, Bonfire, Hunter,
> Shards, Vale — `default: "Blood Moon"`, `tsType: string`.

Resolution (L1265):

```js
themeName() { const n = this.state.theme || this.props.theme || 'Blood Moon'; return THEMES[n] ? n : 'Blood Moon'; }
```

* default theme = **Blood Moon** (state starts `theme: null`, L1190);
* an unknown/stale name silently falls back to Blood Moon;
* `Alt+T` cycles in **NAMES order** (L1248): ArchonUI → Blood Moon → … → Vale → ArchonUI.

---

## 2. Token model and derivation

### 2.1 Preset token set — the `T()` factory (L882)

```js
const T = (wall, render, bg, panel, pop, fg, muted, faint, line, line2, acc, acc2, deep, alt, on, soft, logo) => ({
  wall: 'assets/fhd/' + wall, render, bg, panel, pop, fg, muted, faint, line, line2, acc, acc2, deep, alt, on, soft,
  logo: 'assets/' + logo,
  border: `linear-gradient(135deg,${acc2},${acc} 50%,${deep})`
});
```

Argument order is the only positional contract in the file — mis-order a preset and every token shifts. The 18 keys and
where they are consumed:

| token | paints | example sites |
|---|---|---|
| `wall` | wallpaper URL (`assets/fhd/<file>`) | L21–23 backdrop layer, L368 sign-in hero, L641 preset thumb, L660 style preview |
| `render` | `image-rendering` for the logo in the pixel themes (`auto` \| `pixelated`) | L111, L133, L158, L368 |
| `bg` | app background; code surface at 0.72 alpha | L20 root, L660, `codeBg = rgba(bg,0.72)` L1617 |
| `panel` | sidebar / tools-panel fill (alpha-mixed by the background style) | L1476–1482 `side`; status-dot ring L36, L70 |
| `pop` | popovers, menus, composer card, modals, toasts | L235, L265, L307, L518, L571, L877 |
| `fg` | primary text, icon strokes; source of `line`/`line2` | L20 root colour + 69 sites |
| `muted` | secondary text, labels, section headers | 164 sites (highest usage) |
| `faint` | greyed text, code line numbers, inactive dots | 16 sites (L396, L445, L1510…) |
| `line` | hairline borders + faint fills (tab strip, chips, table rows) | 60 sites |
| `line2` | stronger borders, hover fills, dividers | 127 sites |
| `acc` | primary accent: CTAs, active rings, focus border, checkbox fill | 51 sites |
| `acc2` | highlight: focused icon, strings, “running” status, emphasis | 51 sites |
| `deep` | deepest accent — 3rd stop of the focus-ring gradient, share-avatar palette | L1468, L1614 |
| `alt` | secondary accent: `+`-line counts, branch chip, “done” status, gauges | 38 sites (L84, L472, L1563…) |
| `on` | text/icon colour placed **on** `acc` | 20 sites (L58, L113, L374, L482, L534, L790…) |
| `soft` | accent wash: user bubble, selected rows, focused rail pill | 14 sites (L152, L1408, L1545, L1563…) |
| `logo` | logo URL (`assets/<file>`) | L47, L111, L133, L158, L574, L618 |
| `border` | focus-ring gradient (2px, mask-composited) | produced L1468, used L1520 `ring` → painted L119 |

### 2.2 Derivation pipeline (`renderVals`, L1459–1469)

```js
const name  = this.themeName(), fonts = FONTS[name], fi = (s.fontIdx[name] || 0) % fonts.length;
const base  = THEMES[name], hx = c => [1, 3, 5].map(i => parseInt(c.slice(i, i + 2), 16));
const mix   = (a, b, k) => { const A = hx(a), B = hx(b); return '#' + A.map((v, i) => Math.round(v + (B[i] - v) * k).toString(16).padStart(2, '0')).join(''); };
const rgba  = (c, a) => 'rgba(' + hx(c).join(',') + ',' + a + ')';
const TUNE  = name === 'Blood Moon' ? { …flat overrides… } : { …mix overrides… };
const ov    = s.custom[name] || {};                 // stage 3: user per-token overrides
const t0    = { ...base, ...TUNE, ...ov };          // preset → tuning → user
const t     = { ...t0,
  logo: s.logo || base.logo, wall: s.wall || base.wall,
  display: s.dispFont || fonts[fi].d,
  ui:      s.uiFont   || fonts[fi].u,
  code:    /mono|code|monospace/i.test(s.uiFont || fonts[fi].u) ? (s.uiFont || fonts[fi].u) : "'Geist Mono', monospace",
  line:    rgba(t0.fg, 0.09),
  line2:   rgba(t0.fg, 0.22),
  soft:    ov.acc ? rgba(t0.acc, 0.28) : t0.soft,
  border:  'linear-gradient(135deg,' + t0.acc2 + ',' + t0.acc + ' 50%,' + t0.deep + ')'
};
```

* `mix(a,b,k)` = integer-rounded **linear RGB lerp**, lowercase 6-digit hex, zero-padded.
* `rgba(c,a)` = hex → `rgba(R,G,B,a)` with unrounded alpha, e.g. `rgba(243,234,216,0.22)`.
* There is **no `color-mix()`** in the prototype. The only other colour helper is `rgbaOf(hex, a)` (L983, identical maths)
  used for chart bars (L1354) and file-tag backgrounds (L1206).

### 2.3 TUNE — the second stage (L1463–1465)

```js
const TUNE = name === 'Blood Moon'
  ? { tile: '#16100d', panel: '#0f0b09', pop: '#1b1411', fg: '#f3ead8', muted: '#bfae94', faint: '#6a5a48',
      soft: 'rgba(176,20,30,0.32)', acc: '#c81e2a' }
  : { tile:  mix(base.bg, base.fg, 0.05),
      panel: mix(base.bg, base.fg, 0.02),
      pop:   mix(base.bg, base.fg, 0.08),
      muted: mix(base.muted, base.fg, 0.3),
      faint: mix(base.faint, base.fg, 0.12) };
```

* `tile` is **not** a preset value — it only exists here, and every use site wraps it in `rgba(tile, α)` (`TR()`, L1469).
* Blood Moon is hand-tuned (and gets a punchier accent `#c81e2a` than its preset `#b0141e`).
* The other 11 themes derive tile/sidebar/popover/text tiers by lightening the background toward `fg`.
* **Superseded preset values:** `panel`, `pop`, `muted`, `faint` are overwritten for *every* theme; for Blood Moon
  `fg`, `acc` and `soft` are overwritten too. The preset `line`/`line2` are overwritten for every theme (§2.5).

### 2.4 Per-token user overrides (stage 3)

`ov = s.custom[name]` (see §5) is spread **after** TUNE, so it always wins, and the derived tokens recompute from it:

* override `fg` ⇒ `line` = `rgba(fg,0.09)` and `line2` = `rgba(fg,0.22)` move with it;
* override `acc` ⇒ `soft` = `rgba(acc,0.28)` and the preset `soft` is discarded;
* override `acc2` / `acc` / `deep` ⇒ the focus-ring `border` gradient changes;
* overriding `tile` / `panel` / `pop` flows straight through `TR()` / `PR()` at the use sites.

### 2.5 Derived tokens, always recomputed (L1468)

| derived token | formula | note |
|---|---|---|
| `line` | `rgba(t0.fg, 0.09)` | preset value (`…0.06`) is dead |
| `line2` | `rgba(t0.fg, 0.22)` | preset value (`…0.15`/`…0.16`) is dead |
| `soft` | `t0.soft`, or `rgba(t0.acc, 0.28)` when `acc` is user-overridden | presets ship alpha 0.12–0.24; ArchonUI bases it on `acc2` (white), the rest on `acc` |
| `border` | `linear-gradient(135deg, acc2, acc 50%, deep)` | preset `border` is the same maths, recomputed anyway |
| `code` | `ui` when it matches `/mono\|code\|monospace/i`, else `'Geist Mono', monospace` | every preset `u` stack qualifies ⇒ `code === ui` unless the user picks a sans UI font |
| `display`, `ui` | `s.dispFont \|\| pair.d`, `s.uiFont \|\| pair.u` | §6 |
| `wall`, `logo` | `s.wall \|\| base.wall`, `s.logo \|\| base.logo` | user picks survive theme switches |
| `TR(a)` | `rgba(t.tile, a)` | session tiles |
| `PR(a)` | `rgba(t.panel, a)` | sidebars |
| `codeBg` | `rgba(t.bg, 0.72)` | L1617 |

### 2.6 Runtime variables the tokens interact with (state L1189–1207 / sliders L1393–1394)

| state | default | range | meaning |
|---|---|---|---|
| `gaps` | 14 px | 0–32, step 1 | gap between tiles and panels; `innerH = 1080 − 2·g`, `ct = g` |
| `radius` | 9 px | 0–20, step 1 | tile radius; inner radius `rad2 = max(0, rad − 2)` |
| `blur` | 24 px | 0–48, step 1 | `blurS = blur(Npx) saturate(1.2)` used by sidebars and glass tiles |
| `dim` | 0 % | 0–80, step 5 | appended to the wallpaper filter as `brightness(1 − dim/100)` |
| `fs` | 15 px | 13–19, step 1 | root font size (message text) |
| `bgStyle` | `null` → `Canvas` | 6 values | §4 |
| `fit` | 1 | `min(vw/1920, vh/1080)` | the whole 1920×1080 design is scaled with `transform: scale(fit)` (L20, L1245) |

---

## 3. Per-theme values (all 12, NAMES order)

### 3.1 Preset (raw `T()`) tokens, part 1 of 2

| theme | bg | panel | pop | fg | muted | faint | line | line2 |
|---|---|---|---|---|---|---|---|---|
| ArchonUI | #050505 | rgba(8,8,8,0.80) | rgba(14,14,14,0.97) | #ededed | #8a8a8a | #3a3a3a | rgba(237,237,237,0.06) | rgba(237,237,237,0.15) |
| Blood Moon | #0d0a08 | rgba(13,10,8,0.80) | rgba(18,13,10,0.97) | #eadfcc | #9a8c78 | #43382c | rgba(234,223,204,0.06) | rgba(234,223,204,0.15) |
| Moonlight | #050a14 | rgba(6,12,24,0.72) | rgba(8,14,28,0.97) | #e3ecf8 | #8392aa | #2c3a52 | rgba(227,236,248,0.06) | rgba(227,236,248,0.16) |
| Wreck | #1e140a | rgba(30,20,10,0.78) | rgba(34,23,12,0.97) | #f3e6cc | #a8916c | #5a4428 | rgba(243,230,204,0.06) | rgba(243,230,204,0.15) |
| Citadel | #0d0f18 | rgba(13,15,24,0.76) | rgba(16,18,28,0.97) | #f2e6d0 | #8f8ea4 | #383a52 | rgba(242,230,208,0.06) | rgba(242,230,208,0.15) |
| Neon | #0e0718 | rgba(14,7,24,0.74) | rgba(18,9,30,0.97) | #f1e6ff | #9d8bb8 | #3d2a5a | rgba(241,230,255,0.06) | rgba(241,230,255,0.15) |
| Violet | #0c0a20 | rgba(12,10,32,0.74) | rgba(15,13,38,0.97) | #ece8ff | #9590c0 | #36316a | rgba(236,232,255,0.06) | rgba(236,232,255,0.15) |
| Ink | #f2f0ea | rgba(246,244,238,0.86) | rgba(250,248,243,0.98) | #16130f | #6b665d | #bdb8ad | rgba(22,19,15,0.06) | rgba(22,19,15,0.15) |
| Bonfire | #141b20 | rgba(20,28,34,0.80) | rgba(24,32,38,0.97) | #dbe6ea | #8a9ba3 | #3a4a52 | rgba(219,230,234,0.06) | rgba(219,230,234,0.16) |
| Hunter | #0f1622 | rgba(15,22,34,0.80) | rgba(18,26,40,0.97) | #e4e8f0 | #8794ad | #323d55 | rgba(228,232,240,0.06) | rgba(228,232,240,0.15) |
| Shards | #0a0a0c | rgba(10,10,12,0.78) | rgba(14,14,17,0.97) | #eef0f4 | #8c92a0 | #353944 | rgba(238,240,244,0.06) | rgba(238,240,244,0.15) |
| Vale | #0d150c | rgba(13,21,12,0.76) | rgba(16,25,15,0.97) | #eef2e0 | #94a283 | #34442c | rgba(238,242,224,0.06) | rgba(238,242,224,0.15) |

### 3.2 Preset (raw `T()`) tokens, part 2 of 2

| theme | acc | acc2 | deep | alt | on | soft | render |
|---|---|---|---|---|---|---|---|
| ArchonUI | #d8d8d8 | #ffffff | #4a4a4a | #b0b0b0 | #050505 | rgba(255,255,255,0.12) | auto |
| Blood Moon | #b0141e | #d9b56a | #5a0a10 | #c9a77a | #f6ecdc | rgba(176,20,30,0.24) | auto |
| Moonlight | #8fb8e8 | #dceaff | #33557f | #b4c6e0 | #06101f | rgba(143,184,232,0.20) | auto |
| Wreck | #c8893a | #f0c27a | #6b4420 | #e3cfa4 | #1e140a | rgba(200,137,58,0.22) | auto |
| Citadel | #e09a3a | #ffcc70 | #7a3d14 | #9fb0d8 | #140c04 | rgba(224,154,58,0.22) | auto |
| Neon | #b04cff | #ff6ad5 | #4b1d7a | #93a8ff | #ffffff | rgba(176,76,255,0.24) | auto |
| Violet | #7a6cff | #c9b8ff | #33287a | #e08ab8 | #ffffff | rgba(122,108,255,0.24) | auto |
| Ink | #c8201e | #e0322a | #16130f | #3d3a35 | #ffffff | rgba(200,32,30,0.12) | auto |
| Bonfire | #e8762c | #ffb24a | #8a3a1c | #9cc8d2 | #1a0f08 | rgba(232,118,44,0.22) | pixelated |
| Hunter | #c23a3a | #f0e2c0 | #6e1e24 | #8fa6cc | #0f1622 | rgba(194,58,58,0.24) | pixelated |
| Shards | #9aa4b8 | #ffffff | #3a3f4c | #c2c8d6 | #0a0a0c | rgba(154,164,184,0.20) | auto |
| Vale | #d0882e | #f2c36a | #5a3a14 | #8cc06a | #140c04 | rgba(208,136,46,0.22) | auto |

> `render` is `pixelated` only for **Bonfire** and **Hunter** (pixel-art wallpapers + pixel fonts); `auto` otherwise.

### 3.3 Effective tokens after TUNE (what actually renders) — part 1 of 2

| theme | bg | tile | panel | pop | fg | muted | faint |
|---|---|---|---|---|---|---|---|
| ArchonUI | #050505 | #111111 | #0a0a0a | #181818 | #ededed | #a8a8a8 | #4f4f4f |
| Blood Moon | #0d0a08 | #16100d | #0f0b09 | #1b1411 | #f3ead8 | #bfae94 | #6a5a48 |
| Moonlight | #050a14 | #10151f | #090f19 | #171c26 | #e3ecf8 | #a0adc1 | #424f66 |
| Wreck | #1e140a | #291f14 | #22180e | #2f251a | #f3e6cc | #bfab89 | #6c573c |
| Citadel | #0d0f18 | #181a21 | #12131c | #1f2027 | #f2e6d0 | #ada8b1 | #4e4f61 |
| Neon | #0e0718 | #191224 | #130b1d | #20192a | #f1e6ff | #b6a6cd | #53416e |
| Violet | #0c0a20 | #17152b | #100e24 | #1e1c32 | #ece8ff | #afaad3 | #4c477c |
| Ink | #f2f0ea | #e7e5df | #eeece6 | #e0ded8 | #16130f | #524d46 | #a9a49a |
| Bonfire | #141b20 | #1e252a | #181f24 | #242b30 | #dbe6ea | #a2b2b8 | #4d5d64 |
| Hunter | #0f1622 | #1a212c | #131a26 | #202732 | #e4e8f0 | #a3adc1 | #475268 |
| Shards | #0a0a0c | #151618 | #0f0f11 | #1c1c1f | #eef0f4 | #a9aeb9 | #4b4f59 |
| Vale | #0d150c | #182017 | #121910 | #1f271d | #eef2e0 | #afba9f | #4a5942 |

### 3.4 Effective tokens after TUNE — part 2 of 2

`line` / `line2` here are the derived values `rgba(fg,0.09)` and `rgba(fg,0.22)`, not the preset values in §3.1.

| theme | acc | acc2 | deep | alt | on | soft | line | line2 |
|---|---|---|---|---|---|---|---|---|
| ArchonUI | #d8d8d8 | #ffffff | #4a4a4a | #b0b0b0 | #050505 | rgba(255,255,255,0.12) | rgba(237,237,237,0.09) | rgba(237,237,237,0.22) |
| Blood Moon | #c81e2a | #d9b56a | #5a0a10 | #c9a77a | #f6ecdc | rgba(176,20,30,0.32) | rgba(243,234,216,0.09) | rgba(243,234,216,0.22) |
| Moonlight | #8fb8e8 | #dceaff | #33557f | #b4c6e0 | #06101f | rgba(143,184,232,0.20) | rgba(227,236,248,0.09) | rgba(227,236,248,0.22) |
| Wreck | #c8893a | #f0c27a | #6b4420 | #e3cfa4 | #1e140a | rgba(200,137,58,0.22) | rgba(243,230,204,0.09) | rgba(243,230,204,0.22) |
| Citadel | #e09a3a | #ffcc70 | #7a3d14 | #9fb0d8 | #140c04 | rgba(224,154,58,0.22) | rgba(242,230,208,0.09) | rgba(242,230,208,0.22) |
| Neon | #b04cff | #ff6ad5 | #4b1d7a | #93a8ff | #ffffff | rgba(176,76,255,0.24) | rgba(241,230,255,0.09) | rgba(241,230,255,0.22) |
| Violet | #7a6cff | #c9b8ff | #33287a | #e08ab8 | #ffffff | rgba(122,108,255,0.24) | rgba(236,232,255,0.09) | rgba(236,232,255,0.22) |
| Ink | #c8201e | #e0322a | #16130f | #3d3a35 | #ffffff | rgba(200,32,30,0.12) | rgba(22,19,15,0.09) | rgba(22,19,15,0.22) |
| Bonfire | #e8762c | #ffb24a | #8a3a1c | #9cc8d2 | #1a0f08 | rgba(232,118,44,0.22) | rgba(219,230,234,0.09) | rgba(219,230,234,0.22) |
| Hunter | #c23a3a | #f0e2c0 | #6e1e24 | #8fa6cc | #0f1622 | rgba(194,58,58,0.24) | rgba(228,232,240,0.09) | rgba(228,232,240,0.22) |
| Shards | #9aa4b8 | #ffffff | #3a3f4c | #c2c8d6 | #0a0a0c | rgba(154,164,184,0.20) | rgba(238,240,244,0.09) | rgba(238,240,244,0.22) |
| Vale | #d0882e | #f2c36a | #5a3a14 | #8cc06a | #140c04 | rgba(208,136,46,0.22) | rgba(238,242,224,0.09) | rgba(238,242,224,0.22) |

### 3.5 Focus-ring gradient (`border`, L1468)

| theme | border |
|---|---|
| ArchonUI | `linear-gradient(135deg,#ffffff,#d8d8d8 50%,#4a4a4a)` |
| Blood Moon | `linear-gradient(135deg,#d9b56a,#c81e2a 50%,#5a0a10)` |
| Moonlight | `linear-gradient(135deg,#dceaff,#8fb8e8 50%,#33557f)` |
| Wreck | `linear-gradient(135deg,#f0c27a,#c8893a 50%,#6b4420)` |
| Citadel | `linear-gradient(135deg,#ffcc70,#e09a3a 50%,#7a3d14)` |
| Neon | `linear-gradient(135deg,#ff6ad5,#b04cff 50%,#4b1d7a)` |
| Violet | `linear-gradient(135deg,#c9b8ff,#7a6cff 50%,#33287a)` |
| Ink | `linear-gradient(135deg,#e0322a,#c8201e 50%,#16130f)` |
| Bonfire | `linear-gradient(135deg,#ffb24a,#e8762c 50%,#8a3a1c)` |
| Hunter | `linear-gradient(135deg,#f0e2c0,#c23a3a 50%,#6e1e24)` |
| Shards | `linear-gradient(135deg,#ffffff,#9aa4b8 50%,#3a3f4c)` |
| Vale | `linear-gradient(135deg,#f2c36a,#d0882e 50%,#5a3a14)` |

Consumed at L1520 (`ring: foc ? t.border : t.line2`) and painted at L119–120: a `padding:2px` box filled with the
gradient and mask-composited to a 2px ring — `mask: linear-gradient(#000 0 0) content-box, linear-gradient(#000 0 0)`
with `-webkit-mask-composite: xor; mask-composite: exclude`. An unfocused tile draws the same 2px ring in flat `line2`.

### 3.6 The 6 collapsed-sidebar bands (L1655)

Each collapsed-rail session button is tinted with a 6-step ramp that rotates with the session index `i`, more opaque
when that session is focused:

```js
railSess: s.sessions.map(…)
  .map((r, i) => ({ ...r, bg: rgba(mix(t.acc, t.acc2, (i % 6) * 0.07),
                                      s.focus === s.sessions[i].id ? 0.42 : 0.2) }))
```

* ramp factors between `acc` and `acc2`: **0, 0.07, 0.14, 0.21, 0.28, 0.35**;
* alpha: **0.42** when focused, **0.20** otherwise;
* button: 36×36, `border-radius: 9px`, icon colour `foc ? acc2 : open ? fg : muted` (L1654–1655 → L36).

| theme | band 1 (0) | band 2 (0.07) | band 3 (0.14) | band 4 (0.21) | band 5 (0.28) | band 6 (0.35) | alpha |
|---|---|---|---|---|---|---|---|
| ArchonUI | #d8d8d8 | #dbdbdb | #dddddd | #e0e0e0 | #e3e3e3 | #e6e6e6 | 0.42 / 0.20 |
| Blood Moon | #c81e2a | #c9292e | #ca3333 | #cc3e37 | #cd483c | #ce5340 | 0.42 / 0.20 |
| Moonlight | #8fb8e8 | #94bcea | #9abfeb | #9fc3ed | #a5c6ee | #aacaf0 | 0.42 / 0.20 |
| Wreck | #c8893a | #cb8d3e | #ce9143 | #d09547 | #d3994c | #d69d50 | 0.42 / 0.20 |
| Citadel | #e09a3a | #e29e3e | #e4a142 | #e7a545 | #e9a849 | #ebac4d | 0.42 / 0.20 |
| Neon | #b04cff | #b64efc | #bb50f9 | #c152f6 | #c654f3 | #cc57f0 | 0.42 / 0.20 |
| Violet | #7a6cff | #8071ff | #8577ff | #8b7cff | #9081ff | #9687ff | 0.42 / 0.20 |
| Ink | #c8201e | #ca211f | #cb2320 | #cd2421 | #cf2521 | #d02622 | 0.42 / 0.20 |
| Bonfire | #e8762c | #ea7a2e | #eb7e30 | #ed8332 | #ee8734 | #f08b37 | 0.42 / 0.20 |
| Hunter | #c23a3a | #c54643 | #c8524d | #cc5d56 | #cf6960 | #d27569 | 0.42 / 0.20 |
| Shards | #9aa4b8 | #a1aabd | #a8b1c2 | #afb7c7 | #b6bdcc | #bdc4d1 | 0.42 / 0.20 |
| Vale | #d0882e | #d28c32 | #d59036 | #d7943b | #da993f | #dc9d43 | 0.42 / 0.20 |

Browser-verified example (Blood Moon): `rgba(200,30,42,0.42)`, `rgba(201,41,46,0.2)`, `rgba(202,51,51,0.2)`,
`rgba(204,62,55,0.2)`, `rgba(205,72,60,0.2)`, `rgba(206,83,64,0.2)`.

> Dead code nearby: `hueOf()` (L1509) defines a *different* 6-tone rotation
> `[acc, acc2, alt, deep, mix(acc,alt,0.5), mix(acc2,fg,0.4)]` indexed by `icon % 6`, but nothing calls it. The live rail
> tint is the `acc → acc2` ramp above.

### 3.7 Wallpaper, logo and render per theme (L884–895)

| theme | wallpaper | logo | render |
|---|---|---|---|
| ArchonUI | `assets/fhd/archonui4.jpg` | `assets/sword-logo-diag.png` | auto |
| Blood Moon | `assets/fhd/bloodmoon2.jpg` | `assets/logo-bm.png` | auto |
| Moonlight | `assets/fhd/moonlight2.jpg` | `assets/logo-moon.png` | auto |
| Wreck | `assets/fhd/wreck2.jpg` | `assets/logo-sand.png` | auto |
| Citadel | `assets/fhd/citadel2.jpg` | `assets/logo-drg-black.png` | auto |
| Neon | `assets/fhd/neon2.jpg` | `assets/logo-cat.png` | auto |
| Violet | `assets/fhd/violet2.jpg` | `assets/logo-portal.png` | auto |
| Ink | `assets/fhd/ink2.jpg` | `assets/logo-tree.png` | auto |
| Bonfire | `assets/fhd/bonfire.jpg` | `assets/logo-bonfire.png` | pixelated |
| Hunter | `assets/fhd/hunter2.jpg` | `assets/logo-helm.png` | pixelated |
| Shards | `assets/fhd/shards2.jpg` | `assets/logo-snow.png` | auto |
| Vale | `assets/fhd/vale2.jpg` | `assets/logo-galaxy.png` | auto |

Wallpaper file names keep their numbered suffixes (`archonui4`, `bloodmoon2`, `moonlight2`, `wreck2`, `citadel2`, `neon2`,
`violet2`, `ink2`, `hunter2`, `shards2`, `vale2`; `bonfire.jpg` is the only unnumbered one). All 12 wallpapers exist in
`design/assets/fhd/`, all 12 logos in `design/assets/`.

Wallpaper layer (L21–23, values from L1617):

```html
<div style="position:absolute;left:{{ wx }}px;top:0;width:{{ ww }}px;height:1080px;overflow:hidden;
            opacity:{{ wallOp }};transition:opacity .4s,left .3s,width .3s">
  <div style="position:absolute;inset:-60px;background:url('{{ t.wall }}') center / cover no-repeat;
              filter:{{ wallFilter }}"></div>
</div>
```

* `wx = 0` and `ww = 1920` are constants (L1617) — the layer always spans the full canvas;
* the image itself is inset **−60px** on every side and painted `center / cover no-repeat`;
* `wallOp = BG.wall` (0 … 1, per background style), animated with `opacity .4s`;
* `wallFilter = ((BG.wf || '') + (s.dim ? ' brightness(' + (1 − s.dim/100) + ')' : '')).trim() || 'none'`.

---

## 4. The 6 background styles (`BGS`, L1475–1483; default `Canvas`)

```js
const blurS = 'blur(' + s.blur + 'px) saturate(1.2)';
const BGS = {
  Canvas:  { wall: 1,   side: PR(0.72), sb: blurS, tile: 'linear-gradient(180deg,' + TR(0.25) + ',' + TR(0.6) + ' 45%,' + TR(0.96) + ' 85%)', tb: 'none' },
  Glass:   { wall: 1,   side: PR(0.6),  sb: blurS, tile: TR(0.55), tb: 'blur(' + s.blur + 'px)' },
  Solid:   { wall: 0,   side: t.panel,  sb: 'none', tile: t.tile,  tb: 'none' },
  Gaps:    { wall: 1,   side: PR(0.96), sb: 'none', tile: TR(1),    tb: 'none' },
  Tinted:  { wall: 0.5, side: PR(0.55), sb: blurS, tile: TR(0.35),  tb: 'blur(' + Math.round(s.blur / 2) + 'px)' },
  Ambient: { wall: 1,   wf: 'blur(28px) brightness(0.55)', side: PR(0.5), sb: blurS, tile: TR(0.45), tb: 'blur(6px)' },
};
const BG = BGS[bgs] || BGS.Canvas;    // bgs = s.bgStyle || 'Canvas'
```

`wall` → wallpaper-layer opacity · `side` → both sidebars’ `background` · `sb` → their `backdrop-filter` ·
`tile` → session-tile `background` · `tb` → tile `backdrop-filter` · `wf` → wallpaper CSS filter.
`PR(α) = rgba(t.panel, α)`, `TR(α) = rgba(t.tile, α)`. Table below = **Blood Moon with defaults**
(`blur 24`, `gaps 14`, `radius 9`, so `t.panel = #0f0b09`, `t.tile = #16100d`, `t.bg = #0d0a08`):

| style | settings label (L1404/L1484) | wallpaper opacity | wallpaper filter | sidebar background | sidebar backdrop-filter | tile background | tile backdrop-filter |
|---|---|---|---|---|---|---|---|
| Canvas (default) | Wallpaper behind chats | 1 | none | rgba(15,11,9,0.72) | blur(24px) saturate(1.2) | linear-gradient(180deg,rgba(22,16,13,0.25),rgba(22,16,13,0.6) 45%,rgba(22,16,13,0.96) 85%) | none |
| Glass | Frosted tiles | 1 | none | rgba(15,11,9,0.6) | blur(24px) saturate(1.2) | rgba(22,16,13,0.55) | blur(24px) |
| Solid | No wallpaper | 0 | none | #0f0b09 (opaque `t.panel`) | none | #16100d (opaque `t.tile`) | none |
| Gaps | Wallpaper in the gaps | 1 | none | rgba(15,11,9,0.96) | none | rgba(22,16,13,1) | none |
| Tinted | Dimmed, light glass | 0.5 | none | rgba(15,11,9,0.55) | blur(24px) saturate(1.2) | rgba(22,16,13,0.35) | blur(12px) |
| Ambient | Blurred, dark wallpaper | 1 | blur(28px) brightness(0.55) | rgba(15,11,9,0.5) | blur(24px) saturate(1.2) | rgba(22,16,13,0.45) | blur(6px) |

Two `BGD` label maps exist with slightly different wording (L1404 in `settingsVals`, L1484 in `renderVals`):

| style | L1404 | L1484 |
|---|---|---|
| Canvas | Wallpaper behind chats | Wallpaper behind chats |
| Glass | Frosted tiles | Frosted session tiles |
| Solid | No wallpaper | No wallpaper |
| Gaps | Wallpaper in the gaps | Solid tiles, wallpaper in gaps |
| Tinted | Dimmed, light glass | Dimmed wallpaper, light glass |
| Ambient | Blurred and dark | Blurred, dark wallpaper |

### 4.1 The Canvas per-tile override (L1520) — only Canvas varies per tile

```js
bg:   bgs !== 'Canvas' ? BG.tile
    : foc && !se.msgs.length ? TR(0.12)          // focused, empty session
    : (foc || s.opts.tintAll) ? TR(0.82)         // focused, or “tint all sessions” enabled
    : BG.tile,                                   // unfocused: the 3-stop gradient above
blur: bgs !== 'Canvas' ? BG.tb
    : foc && !se.msgs.length ? 'blur(2px)'
    : (foc || s.opts.tintAll) ? 'blur(8px)'
    : BG.tb,
```

So in Canvas: focused+empty = `rgba(tile,0.12)` + `blur(2px)`; focused with messages (or `tintAll`) =
`rgba(tile,0.82)` + `blur(8px)`; unfocused = the vertical gradient, no blur. The other five styles use `BG.tile`/
`BG.tb` for every tile. Sidebars and the panel keep `sideBg`/`sideBlur` in all six styles.

### 4.2 There are no generated patterns

The file contains `linear-gradient` only — L118 (ring mask), L292 (effort track `linear-gradient(90deg, acc, acc2)` +
`box-shadow: 0 0 10px acc`), L882/L1468 (ring + preset border), L1354 (chart bars), L1475–1482 (Canvas tile).
No `radial-gradient`, `conic-gradient`, `repeating-*`, SVG noise or procedural pattern anywhere. The “generated”
surfaces are the Canvas tile gradient, the focus-ring gradient and the effort-track gradient.

### 4.3 Background-style picker cards (L657–669)

Six cards in a `repeat(6, minmax(0,1fr))` grid. Each preview is a 74px-tall box built from the live tokens: the wallpaper
layer (`inset:-6px`, `opacity: bc.wo`, `filter: bc.wf`), an 18%-wide left sidebar strip (`bc.side`) and two tile
rectangles (`bc.tile`) at left 24% and left 62%. Selected card: `1px solid t.acc` border, `t.soft` fill (L1408 `pick()`).

---

## 5. Per-colour customisation on top of a preset

### 5.1 The 12 editable tokens (`TOK`, L1406)

| key | label | what it repaints |
|---|---|---|
| `bg` | Background | app background, code surface (`rgba(bg,0.72)`) |
| `tile` | Session tile | session tiles (`rgba(tile,α)`) and the Canvas gradient stops |
| `panel` | Sidebar | sidebar + tools panel (`rgba(panel,α)`) and dot rings |
| `pop` | Popover | popovers, composer card, modals, toast |
| `fg` | Text | text, icons, and `line`/`line2`/`border` inputs |
| `muted` | Muted text | secondary text and labels |
| `faint` | Faint | greyed text, code gutters, idle dots |
| `on` | Text on accent | text/icon on `acc` fills |
| `acc` | Accent | primary accent + `soft` (recomputed `rgba(acc,0.28)`) + `border` mid stop |
| `acc2` | Highlight | highlight + `border` first stop |
| `alt` | Secondary | secondary accent |
| `deep` | Deep accent | `border` last stop + share-avatar palette |

### 5.2 How it works

* UI: Settings → **Appearance** → `COLORS · <THEME NAME>` (L680–690) with a “Reset to preset” button. Each token is a
  label containing a 34×34 swatch, the token name, the current value in uppercase, an `edited` marker, and a full-bleed
  `<input type="color" value="{{ ck.v }}" onChange="{{ ck.on }}">` with `opacity:0` (L687) — clicking anywhere on the card
  opens the OS colour picker.
* Storage: `state.custom = { [themeName]: { [tokenKey]: "#rrggbb" } }` (declared L1199, written L1421). Overrides are
  **per theme** and persist while you switch themes; the preset card for that theme then shows the tag **“Edited”** (active theme:
  **“Active · edited”**, L1415).
* Merge order: `t0 = { ...base, ...TUNE, ...ov }` — user overrides are applied last, then `line`, `line2`, `soft`, `border`
  are recomputed from the overridden values (§2.4).
* Reset: “Reset to preset” deletes `custom[themeName]` entirely (`delete c[name]`, L1422); its button is dimmed to
  `opacity: 0.4` when the theme has no edits (`resetOp = nOv ? 1 : 0.4`, L1422).
* Picking a preset (`presets[].pick`, L1415) resets `theme`, `dispFont`, `uiFont` and `wall` to `null` (so fonts and
  wallpaper follow the new theme) but **does not** clear colour overrides and **does not** clear a user-chosen logo.
* Values come straight from `<input type="color">`, i.e. always 6-digit hex (`#0d0a08`, `#c81e2a`, …), which is why the
  swatch text is rendered `text-transform:uppercase`.

### 5.3 Sidebar “Identity” and logo overrides (L611–633)

* **Name**: text input, `maxlength="18"`, placeholder `archonui`, rendered in `t.display` at 34px (L616). Default `archonui`.
* **Icon**: 7-column grid of every unique theme logo (`[...new Set(NAMES.map(nm => THEMES[nm].logo))]`, L1428 = 12 logos)
  plus an upload tile (`URL.createObjectURL`). Selected = `outline: 2px solid t.acc; outline-offset: 1px; background:
  t.soft`; unselected = `background: t.line`. “Use theme icon” resets `logo` to `null` (→ the theme default) and is
  dimmed to `opacity: 0.4` when already default (L625, L1430).
* **Wallpaper**: 7-column grid of the 12 theme wallpapers plus “+ Upload” (L670–679). Selecting sets `state.wall`
  (replacing the theme default); the selected tile gets `outline: 2px solid t.acc; outline-offset: 2px` (L1419).
  Picking a theme preset clears the override (`wall: null`).

---

## 6. Fonts

### 6.1 Per-theme pairings (`FONTS`, L900–913) — 3 pairs per theme, default index 0

`F(n, d, u)`: `n` = label shown on the card, `d` = **display** stack, `u` = **UI/code** stack.
`fi = (s.fontIdx[name] || 0) % fonts.length` (L1459) — the chosen pair is remembered **per theme name**

| theme | # | label | display stack (`t.display`) | UI stack (`t.ui`, `t.code`) |
|---|---|---|---|---|
| ArchonUI | 0 | Grenze | `'Grenze Gotisch', serif` | `'IBM Plex Mono', monospace` |
| ArchonUI | 1 | Bodoni | `'Bodoni Moda', serif` | `'JetBrains Mono', monospace` |
| ArchonUI | 2 | Italiana | `'Italiana', serif` | `'DM Mono', monospace` |
| Blood Moon | 0 | Fraktur | `'UnifrakturCook', serif` | `'JetBrains Mono', monospace` |
| Blood Moon | 1 | Pirata | `'Pirata One', serif` | `'Space Mono', monospace` |
| Blood Moon | 2 | Grenze | `'Grenze Gotisch', serif` | `'IBM Plex Mono', monospace` |
| Moonlight | 0 | Cormorant | `'Cormorant Garamond', serif` | `'JetBrains Mono', monospace` |
| Moonlight | 1 | Italiana | `'Italiana', serif` | `'DM Mono', monospace` |
| Moonlight | 2 | Bodoni | `'Bodoni Moda', serif` | `'IBM Plex Mono', monospace` |
| Wreck | 0 | Bebas | `'Bebas Neue', sans-serif` | `'Space Mono', monospace` |
| Wreck | 1 | Orbitron | `'Orbitron', sans-serif` | `'JetBrains Mono', monospace` |
| Wreck | 2 | Gloock | `'Gloock', serif` | `'DM Mono', monospace` |
| Citadel | 0 | Grenze | `'Grenze Gotisch', serif` | `'JetBrains Mono', monospace` |
| Citadel | 1 | Cinzel | `Cinzel, serif` | `'IBM Plex Mono', monospace` |
| Citadel | 2 | Fraktur | `'UnifrakturCook', serif` | `'Space Mono', monospace` |
| Neon | 0 | Orbitron | `'Orbitron', sans-serif` | `'JetBrains Mono', monospace` |
| Neon | 1 | Mincho | `'Zen Old Mincho', serif` | `'DM Mono', monospace` |
| Neon | 2 | Bebas | `'Bebas Neue', sans-serif` | `'Fira Code', monospace` |
| Violet | 0 | Cormorant | `'Cormorant Garamond', serif` | `'JetBrains Mono', monospace` |
| Violet | 1 | Metamorphous | `'Metamorphous', serif` | `'DM Mono', monospace` |
| Violet | 2 | Cinzel | `Cinzel, serif` | `'IBM Plex Mono', monospace` |
| Ink | 0 | Mincho | `'Zen Old Mincho', serif` | `'DM Mono', monospace` |
| Ink | 1 | Cormorant | `'Cormorant Garamond', serif` | `'JetBrains Mono', monospace` |
| Ink | 2 | Bodoni | `'Bodoni Moda', serif` | `'IBM Plex Mono', monospace` |
| Bonfire | 0 | Pixelify | `'Pixelify Sans', monospace` | `'JetBrains Mono', monospace` |
| Bonfire | 1 | Silkscreen | `'Silkscreen', monospace` | `'Space Mono', monospace` |
| Bonfire | 2 | Press Start | `'Press Start 2P', monospace` | `'IBM Plex Mono', monospace` |
| Hunter | 0 | Silkscreen | `'Silkscreen', monospace` | `'JetBrains Mono', monospace` |
| Hunter | 1 | Pixelify | `'Pixelify Sans', monospace` | `'Space Mono', monospace` |
| Hunter | 2 | Press Start | `'Press Start 2P', monospace` | `'IBM Plex Mono', monospace` |
| Shards | 0 | Orbitron | `'Orbitron', sans-serif` | `'Space Mono', monospace` |
| Shards | 1 | Bebas | `'Bebas Neue', sans-serif` | `'JetBrains Mono', monospace` |
| Shards | 2 | Italiana | `'Italiana', serif` | `'DM Mono', monospace` |
| Vale | 0 | Metamorphous | `'Metamorphous', serif` | `'JetBrains Mono', monospace` |
| Vale | 1 | Young Serif | `'Young Serif', serif` | `'DM Mono', monospace` |
| Vale | 2 | Philosopher | `'Philosopher', sans-serif` | `'Fira Code', monospace` |

| FONTS shorthand | stack |
|---|---|
| `JB` | `'JetBrains Mono', monospace` |
| `PLEX` | `'IBM Plex Mono', monospace` |
| `SPACE` | `'Space Mono', monospace` |
| `DMM` | `'DM Mono', monospace` |
| `FIRA` | `'Fira Code', monospace` |

The pairings card (L694–700) previews the app name in `d` at 30px with the label `"<label> · <family>"` in `u` at 11.5px.
Selecting one sets `dispFont: null, uiFont: null, fontIdx[name] = k` — i.e. it clears any manual font overrides (L1423).

### 6.2 The other font lists

**Minimal** pairs (`MIN_PAIRS`, L979 — 6 cards, L703–710, preview 28px / `font-weight:500` / `letter-spacing:-0.02em`):

| name | display stack | UI stack |
|---|---|---|
| Geist | `'Geist', sans-serif` | `'Geist', sans-serif` |
| IBM Plex | `'IBM Plex Sans', sans-serif` | `'IBM Plex Sans', sans-serif` |
| DM Sans | `'DM Sans', sans-serif` | `'DM Sans', sans-serif` |
| Instrument | `'Instrument Sans', sans-serif` | `'Geist Mono', monospace` |
| Manrope | `'Manrope', sans-serif` | `'JetBrains Mono', monospace` |
| Grotesk | `'Space Grotesk', sans-serif` | `'Space Mono', monospace` |

`pick: () => set({ dispFont: d, uiFont: u })` — writes overrides for both slots (L1424).

**Display font** list (`DISPLAY`, L975; `DISPLAY.unshift(...SANS_D)` at L978 — 23 cards, L712–720, preview 22px):

| # | name | stack | weights requested (L12) | weight actually used |
|---|---|---|---|---|
| 0 | Geist | `'Geist', sans-serif` | 400;500;600 | 400 (no weight set on the cards) |
| 1 | Plex Sans | `'IBM Plex Sans', sans-serif` | 400;500;600 | 400 (no weight set on the cards) |
| 2 | DM Sans | `'DM Sans', sans-serif` | 400;500;600 | 400 (no weight set on the cards) |
| 3 | Instrument | `'Instrument Sans', sans-serif` | 400;500;600 | 400 (no weight set on the cards) |
| 4 | Manrope | `'Manrope', sans-serif` | 400;500;600 | 400 (no weight set on the cards) |
| 5 | Grotesk | `'Space Grotesk', sans-serif` | 400;500;600 | 400 (no weight set on the cards) |
| 6 | Fraktur | `'UnifrakturCook', serif` | 700 | 400 (no weight set on the cards) |
| 7 | Pirata | `'Pirata One', serif` | 400 | 400 (no weight set on the cards) |
| 8 | Grenze | `'Grenze Gotisch', serif` | 600;700 | 400 (no weight set on the cards) |
| 9 | Cinzel | `Cinzel, serif` | 500;700 | 400 (no weight set on the cards) |
| 10 | Cormorant | `'Cormorant Garamond', serif` | 600;700 | 400 (no weight set on the cards) |
| 11 | Italiana | `'Italiana', serif` | 400 | 400 (no weight set on the cards) |
| 12 | Bodoni | `'Bodoni Moda', serif` | 600;700 | 400 (no weight set on the cards) |
| 13 | Gloock | `'Gloock', serif` | 400 | 400 (no weight set on the cards) |
| 14 | Mincho | `'Zen Old Mincho', serif` | 600;700 | 400 (no weight set on the cards) |
| 15 | Metamorphous | `'Metamorphous', serif` | 400 | 400 (no weight set on the cards) |
| 16 | Young Serif | `'Young Serif', serif` | 400 | 400 (no weight set on the cards) |
| 17 | Philosopher | `'Philosopher', sans-serif` | 400;700 | 400 (no weight set on the cards) |
| 18 | Bebas | `'Bebas Neue', sans-serif` | 400 | 400 (no weight set on the cards) |
| 19 | Orbitron | `'Orbitron', sans-serif` | 500;700 | 400 (no weight set on the cards) |
| 20 | Pixelify | `'Pixelify Sans', monospace` | 500;700 | 400 (no weight set on the cards) |
| 21 | Silkscreen | `'Silkscreen', monospace` | 400;700 | 400 (no weight set on the cards) |
| 22 | Press Start | `'Press Start 2P', monospace` | 400 | 400 (no weight set on the cards) |

`pick: () => set({ dispFont: f })` — overrides the display slot only, dimming the pairing card (L1425).

**Interface font** list (`MONO`, L976 — 12 cards, L721–731, preview `"Aa 0O 1lI"` 14px, L724):

| # | name | stack | weights requested (L12) |
|---|---|---|---|
| 0 | JetBrains Mono | `'JetBrains Mono', monospace` | 400;500;700 |
| 1 | IBM Plex Mono | `'IBM Plex Mono', monospace` | 400;500;700 |
| 2 | Space Mono | `'Space Mono', monospace` | 400;700 |
| 3 | DM Mono | `'DM Mono', monospace` | 400;500 |
| 4 | Fira Code | `'Fira Code', monospace` | 400;500;700 |
| 5 | Geist Mono | `'Geist Mono', monospace` | 400;500 |
| 6 | Geist | `'Geist', sans-serif` | 400;500;600 |
| 7 | IBM Plex Sans | `'IBM Plex Sans', sans-serif` | 400;500;600 |
| 8 | DM Sans | `'DM Sans', sans-serif` | 400;500;600 |
| 9 | Instrument Sans | `'Instrument Sans', sans-serif` | 400;500;600 |
| 10 | Manrope | `'Manrope', sans-serif` | 400;500;600 |
| 11 | Space Grotesk | `'Space Grotesk', sans-serif` | 400;500;600 |

`pick: () => set({ uiFont: f })` (L1426). Note the list is named `MONO` but mixes true monospace with sans faces
(Geist, IBM Plex Sans, DM Sans, Instrument Sans, Manrope, Space Grotesk). Picking one of those also flips `t.code` to
`'Geist Mono', monospace`, because the `/mono|code|monospace/i` test only sees the override (§2.5).

### 6.3 Weights

The `<link>` in the helmet (L12) requests exactly these weights from Google Fonts; nothing else is loaded, so a port must
ship/lazy-load the same cuts. UI text uses **400** (default, root `font-size: 15px`), **500** only on the Minimal-pair
preview (L707), and **700** on CTAs/chips/badges (L58, L113, L482, L534, L790, L800, L841…). Display type is always
weight 400 unless a family only ships a heavier cut (e.g. UnifrakturCook 700, Cormorant Garamond 600/700,
Grenze Gotisch 600/700, Cinzel 500/700, Zen Old Mincho 600/700, Orbitron 500/700).

| family | weights in the GF request |
|---|---|
| Cinzel | 500;700 |
| Cormorant Garamond | 600;700 |
| Pixelify Sans | 500;700 |
| Young Serif | 400 |
| UnifrakturCook | 700 |
| Pirata One | 400 |
| Italiana | 400 |
| Bodoni Moda | 600;700 |
| Silkscreen | 400;700 |
| Press Start 2P | 400 |
| Gloock | 400 |
| Philosopher | 400;700 |
| IBM Plex Mono | 400;500;700 |
| Space Mono | 400;700 |
| DM Mono | 400;500 |
| Fira Code | 400;500;700 |
| JetBrains Mono | 400;500;700 |
| Orbitron | 500;700 |
| Bebas Neue | 400 |
| Zen Old Mincho | 600;700 |
| Metamorphous | 400 |
| Grenze Gotisch | 600;700 |
| Geist | 400;500;600 |
| Geist Mono | 400;500 |
| IBM Plex Sans | 400;500;600 |
| DM Sans | 400;500;600 |
| Instrument Sans | 400;500;600 |
| Manrope | 400;500;600 |
| Space Grotesk | 400;500;600 |

### 6.4 Where each font applies

**`t.ui` — global UI font.** Set once on the root 1920×1080 wrapper (L20: `font-family:{{ t.ui }}; font-size:{{ fs }}px;
color:{{ t.fg }}`), so every element without an explicit family inherits it. Also applied explicitly at L376 (browser-mock
terminal footer, 11.5px) and L724 (interface-font card preview).

**`t.display` — headline/identity font** (17 static uses):

| element | size | line |
|---|---|---|
| Sidebar app name + logo | 24px | L48 |
| Wallpaper preview strip (projects card title) | 20px | L84 |
| Empty-state app name (72px logo, centred) | 56px | L112 |
| Session tile title | 20px | L123 |
| Empty-tile prompt “What should <app> work on?” | 28px | L134 |
| Effort level name (colour `acc2`) | 22px | L289 |
| Browser-mock header wordmark / title | 18px / 36px | L366, L369 |
| Files-panel project name | 20px | L408 |
| Share modal title / session name | 26px / 22px | L520, L525 |
| Settings sidebar “Settings” / page title | 26px / 36px | L575, L587 |
| Identity name input / sidebar preview | 34px / 22px | L616, L619 |
| Preset card name (`FONTS[nm][0].d`) | 19px | L647 |
| Pairing card preview | 30px | L698 |
| Minimal card preview (weight 500, −0.02em) | 28px | L707 |
| Display-font card preview | 22px | L716 |

**`t.code` — monospace font** (3 uses, all 12.5px): chat code blocks (line-height 1.65, L221), IDE editor
(line-height 1.7, L393), terminal panel (line-height 1.6, L499). By default `t.code === t.ui`.

### 6.5 Dead data

`fontOpts` (L1634) builds a combo-card list from `fonts` but is never referenced in the markup — the live pairing UI is
`pairings` (L1423).

---

## 7. What triggers a theme / font change

| trigger | effect | line |
|---|---|---|
| initial mount | `state.theme = null` ⇒ `props.theme` ⇒ Blood Moon fallback | L1190, L1265 |
| `Alt+T` / `Alt+t` (global keydown, `altKey` required) | next theme in NAMES order, wrapping | L1248 |
| terminal command `/theme <name>` | exact case-insensitive match in NAMES; sets the theme and echoes `theme → <name>` in `acc2`; no match ⇒ lists all names | L1327, L1333 |
| Settings → Appearance → PRESETS card | `set({ theme: nm, dispFont: null, uiFont: null, wall: null })` | L1415 |
| Settings → Appearance → BACKGROUND card | `set({ bgStyle: n })` (live, no reload) | L1418 |
| Settings → Appearance → Wallpaper tile | `set({ wall: src })` (theme wallpapers or an uploaded blob URL) | L1419, L1420 |
| Settings → Appearance → Icon tile / “Use theme icon” | `set({ logo: src })` / `set({ logo: null })` | L1428, L1430 |
| colour swatch (`<input type=color>`) | writes `custom[themeName][token]` | L1421 |
| “Reset to preset” | deletes `custom[themeName]` | L1422 |
| pairing card | `set({ dispFont: null, uiFont: null, fontIdx[name] = k })` | L1423 |
| Minimal pair card | `set({ dispFont: d, uiFont: u })` | L1424 |
| Display-font card | `set({ dispFont: f })` | L1425 |
| Interface-font card | `set({ uiFont: f })` | L1426 |

There is **no** keyboard shortcut for fonts, and no shortcut for background style — only `Alt+T` for the theme.
Everything else reacts live (the Appearance description literally says “Changes apply live”, L1403).

---

## 8. Status colours (all theme-derived)

```js
const dotFor = st => st === 'running' ? t.acc2 : st === 'done' ? t.alt : t.faint;   // L1510
```

| state | colour | where |
|---|---|---|
| session `running` | `acc2` (+ `halo` = `t.soft` in the sessions list) | L1510 (definition), L1520, L1562, L1654 |
| session `done` | `alt` | same |
| session `idle` / anything else | `faint` | same |
| MCP connected / error / off | `alt` / `acc` / `faint` | L1443 (`mcpRows`) |
| harness ready / disabled / not installed | `alt` / `faint` / `faint` | L1431–1432 (`providers`) |
| changed file: `+N` / `−N` / status letter / staged box | `alt` / `muted` / `acc2` / `acc` fill | L470–472, L1672 |
| project language dot | `t[x.c]` — `alt` (TypeScript), `acc` (Svelte) | L1630 |
| share avatar | `[t.acc, t.alt, t.deep, t.acc2]` cycling `k % 4`; index 2 gets `t.fg` text, others `t.on` | L1614, L1645 |
| git-log hash | newest `acc2`, older `faint` | L1678 |
| chart bars | peak bar `linear-gradient(180deg, acc2, acc)`; others `rgba(acc,0.85) → rgba(acc,0.25)` | L1354 |

---

## 9. Code / syntax colours — themed, no separate syntax palette

`tokenize(line, t)` (L1110–1123) is the only highlighter, so every syntax colour **is** a theme token:

| token class | regex group | colour |
|---|---|---|
| comment (`//…` or `#…` line start) | 1 | `t.muted` |
| string (`"…"`, `'…'`, `` `…` ``) | 2 | `t.acc2` |
| number (`\b\d+\b`) | 3 | `t.alt` |
| identifier | 4 | keyword (set `KW`, L1108) ⇒ `t.acc`; literal (`null undefined true false this`, `LIT` L1109) ⇒ `t.alt`; starts with a capital ⇒ `t.acc2`; else `t.fg` |
| punctuation `{}()[];,.:=<>` | 6 | `t.muted` |
| whitespace / anything else | — | `t.fg` (default) |

Code surface: `codeBg = rgba(t.bg, 0.72)` (L1617) on a `1px solid t.line2` card; line numbers `t.faint` (L396, L445);
diff/selection highlight rows use `t.soft` (L1570); code-block header language label is `t.acc2`, uppercase,
`letter-spacing:.12em` (L218).

---

## 10. Notes, discrepancies and dead code

1. **Two theme orders exist** — NAMES/`Object.keys` (ArchonUI first) drives `Alt+T`, the preset grid and `/theme`; the DC prop
   enum lists Blood Moon first. The default is Blood Moon either way.
2. **Preset `line`/`line2`/`border` are never rendered** as authored; `renderVals` always recomputes them (§2.5).
3. **Preset `panel`/`pop`/`muted`/`faint` are always overwritten by TUNE**; the raw values only matter if a port wants the
   original hand-picked palette instead of the computed one. Both sets are in `themes.json` (`presetTokens` vs `tokens`).
4. `hueOf` (L1509) and `fontOpts` (L1634) are dead code — do not port them.
5. `ATT_SRC` (L983) maps the attachment image `bonfire-frame-240.png` back to `assets/fhd/bonfire.jpg`, and image
   attachments default to the **active theme wallpaper** as their thumbnail (`src: img ? (ATT_SRC[full] || t.wall) : ''`,
   L1217) — i.e. the wallpaper is also used as a placeholder attachment image.
6. Only `Bonfire` and `Hunter` use `render: pixelated`, applied to the logo (L111/133/158), the sign-in hero (L368) and the
   tiles’ logo — never to the wallpaper layer itself.
7. The wallpaper layer is always 1920 wide at x=0 (`wx`/`ww` constants), even though the layout supports gaps/margins —
   no per-style wallpaper repositioning exists.

---

*Generated by `.qa/design/gen-docs.js` from `.qa/design/derived.json` (produced by `.qa/design/derive.js`, which evaluates
the prototype’s own `T`/`THEMES`/`FONTS`/`mix`/`rgba` code). Spot-checked in Chrome via `.archonui-dev/shot.js`.*
