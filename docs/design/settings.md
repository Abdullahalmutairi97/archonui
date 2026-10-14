# ArchonUI — Settings modal: extracted spec

Source of truth: `design/ArchonUI Agent v3.dc.html` (1689 lines). `L###` = line in that file. Every label, colour, px value,
option list and default below is lifted verbatim from the source; values that are computed by the prototype's own JS are
given as the formula plus the computed value. Live effects were verified in Chrome (Playwright) at a 1600×1000 viewport;
measurements are quoted where they were taken.

Companion docs (do not duplicate): `docs/design/themes.md` (tokens, 12 presets, background styles, fonts, colour
overrides), `docs/design/layout.md` (app shell, tiling), `docs/design/icons.md`.

Machine-checked facts used throughout: the whole app is a fixed **1920×1080** canvas
(`L20`) scaled by `fitScale = min(vw/1920, vh/1080)` (`L1238`) — at the reference viewport that is **0.8333**, so the
canvas occupies 1600×900 centred at (0,50). All px below are **unscaled canvas px**; rendered px = value × 0.8333.

---

## 1. Entry points and modal shell

Two buttons call `openSettings()` (`L1622`):

| # | Where | L | Style |
|---|---|---|---|
| 1 | Session header, top-right gear (next to the close/`+` cluster) | L41 | 34×34, `border-radius:7px`, icon 18×18 stroke 1.5, `color:t.fg`, hover `background:t.line2` |
| 2 | Left sidebar, bottom-left **"Settings"** row (under the search field) | L100 | full-width row, `padding:9px 12px`, `border-radius:8px`, `font-size:13px`, gap 10, 18×18 gear, `background: settingsBg` where `settingsBg = modal==='settings' ? t.line2 : 'transparent'` (L1624) |

The modal is closed by: the overlay click, the header ✕ button, and `Esc` (global handler `L1243`). Clicks inside the
modal call `stop` (`e.stopPropagation()`, L1623) so they never close it.

### 1.1 Overlay + dialog (`L569–571`)

```html
<div onClick=closeModal style="position:absolute;inset:0;z-index:20;background:rgba(0,0,0,0.45);display:grid;place-items:center">   <!-- L570 -->
  <div onClick=stop style="width:1320px;height:880px;display:flex;border-radius:14px;background:{{t.pop}};
       border:1px solid {{t.line2}};box-shadow:0 40px 120px rgba(0,0,0,0.65);overflow:hidden">                  <!-- L571 -->
```

* overlay: `z-index:20`, `rgba(0,0,0,0.45)`, `display:grid;place-items:center` → dialog is centred in the 1920×1080 canvas.
* dialog: **1320×880**, `border-radius:14px`, `background: t.pop`, `border:1px solid t.line2`,
  shadow `0 40px 120px rgba(0,0,0,0.65)`, `display:flex` (nav column | content column).
* measured at 1600×1000: box at x=249.2, y=132.5, w=1101.7 (=1322×0.8333 incl. 1px borders), h=735 (=882×0.8333).

`data-screen-label="Settings"` is on the dialog (L571).

### 1.2 Nav column (`L572–584`)

```html
<nav style="width:236px;flex:none;display:flex;flex-direction:column;gap:2px;padding:22px 12px 18px;
     border-right:1px solid {{t.line}};background:{{t.line}}">
```

* width **236** (box-sizing is `content-box` in this file — measured 261 px visual: 236 + 24 padding + 1 border);
  fixed (`flex:none`), `padding:22px 12px 18px`, `gap:2px`, `border-right:1px solid t.line`, **`background:t.line`**
  (i.e. `rgba(fg,0.09)` — the nav is a faint fill, not `t.panel`).
* Nav header (`L573–576`): row, `gap:10px`, `padding:0 10px 10px`; logo `22×22`, `background:url(t.logo) center/contain`;
  title **"Settings"**, `font-family:t.display`, `font-size:26px`, `line-height:1`.
* Section head (`L578`): `font-size:10.5px`, `letter-spacing:.16em`, `color:t.muted`, `padding:16px 12px 6px`,
  `text-transform` **none** (the heads are already uppercase literals).
* Item (`L579`): `<button>` `padding:9px 12px`, `border-radius:7px`, `font-size:13.5px`,
  `background: active ? t.line2 : transparent`, `color: active ? t.fg : t.muted`, hover `color:t.fg`;
  layout `display:flex;align-items:center;justify-content:space-between` → label left, count right
  (`font-size:11px;color:t.muted`, empty string when the page has no count).
* Spacer `flex:1`, then footer (`L583`): `font-size:11px`, `color:t.muted`, `padding:0 12px`,
  text `"{appName} 0.9.0 · {channel}"` → rendered `archonui 0.9.0 · Stable` (appName default `archonui`, channel `Stable`).

### 1.3 Nav structure and counts (`L1400–1412`)

```js
const NAV = [['APP'],['General'],['Appearance'],['Keyboard'],['AI'],
             ['Harnesses', PROVIDERS.filter(x=>s.provOn[x.id]).length],
             ['Models', MODELS.filter(provOk).length],           // provOk = modelOn[id] && provOn[provider]
             ['CAPABILITIES'],['Tools', MCP.filter(x=>s.mcp[x.id]).length],
             ['Skills', skOn],                                   // skOn = skills toggled on
             ['Extensions', s.installed.length]];
const HEADS = ['APP','AI','CAPABILITIES'];
```

| order | kind | label | count rendered | count rule | default |
|---|---|---|---|---|---|
| 1 | head | `APP` | — | — | — |
| 2 | item | `General` | none | — | selected on open (`setTab:'General'`, L1199) |
| 3 | item | `Appearance` | none | — | |
| 4 | item | `Keyboard` | none | — | |
| 5 | head | `AI` | — | — | — |
| 6 | item | `Harnesses` | `4` | # harnesses enabled | claude, codex, gemini, opencode |
| 7 | item | `Models` | `6` | # models on **and** provider on | opus, sonnet, haiku, gpt, gemini, local |
| 8 | head | `CAPABILITIES` | — | — | — |
| 9 | item | `Tools` | `6` | # MCP servers on | github, linear, postgres, playwright, sentry, fs |
| 10 | item | `Skills` | `7` | # skills on | 7 of 10 |
| 11 | item | `Extensions` | `4` | `s.installed.length` (installed list, **not** enabled) | vim, prettier, wakatime, discord |

Clicking an item sets `setTab` (`L1412`). Counts update live (verified: enabling Aider moved Harnesses 4 → 5).

### 1.4 Content column (`L585–593`)

```html
<div style="flex:1;min-width:0;display:flex;flex-direction:column">
  <header style="display:flex;align-items:flex-start;gap:16px;padding:26px 22px 20px 36px;border-bottom:1px solid {{t.line}}">
    <div style="flex:1;display:flex;flex-direction:column;gap:8px">
      <span style="font-family:{{t.display}};font-size:36px;line-height:1">{{setTitle}}</span>   <!-- = tab name -->
      <span style="font-size:13px;color:{{t.muted}}">{{setDesc}}</span>
    </div>
    <button title="Close" …32×32, radius 7, color t.muted, hover bg t.line2 + color t.fg, 17×17 ✕ stroke 1.4 />
  </header>
  <div style="flex:1;overflow:auto;min-height:0;padding:8px 36px 48px;display:flex;flex-direction:column;gap:28px">
```

Page title = the nav label verbatim; page subtitle = `DESC[tab]` (`L1403`):

| page | `DESC` (verbatim) |
|---|---|
| General | `Startup, agent behaviour, notifications and data.` |
| Appearance | `Presets, colours, fonts, wallpaper and layout. Changes apply live.` |
| Keyboard | `Shortcuts for tiling and navigation.` |
| Harnesses | `Agent CLIs that this app drives. Each one brings its own models, tools and sign-in.` |
| Models | `Which models appear in the composer, and the defaults for new sessions.` |
| Tools | `MCP servers and built-in tools the agent can call.` |
| Skills | `Reusable instructions the agent loads when a task matches.` |
| Extensions | `Plugins that add panels, commands and integrations.` |

Body: scroll container, `padding:8px 36px 48px`, **`gap:28px`** between top-level blocks. Appearance measured
`scrollHeight 3073 / clientHeight 772` at 1600×1000.

Top-level blocks are rendered in a fixed order inside the body (`L594–873`):

1. `genTop` — `{General: GEN, Keyboard: KEYS, Models: MDT}[tab]` (`L1414`)
2. `pg.appearance` (IDENTITY, PRESETS, BACKGROUND, COLORS, TYPOGRAPHY)
3. `pg.providers` (Harnesses)
4. `pg.models` (MODELS table)
5. `pg.tools` (MCP SERVERS)
6. `pg.skills` (SKILLS)
7. `pg.extensions` (search, INSTALLED, BROWSE)
8. `genBot` — `{Appearance: APB, Tools: TLB}[tab]` (`L1414`)

So: **Appearance** = IDENTITY, PRESETS, BACKGROUND, COLORS, TYPOGRAPHY, LAYOUT, EFFECTS; **Tools** = MCP SERVERS,
BUILT-IN TOOLS; **Models** = DEFAULTS, MODELS; **General/Keyboard** = their sections only;
**Harnesses/Skills/Extensions** = the `pg.*` block only.

### 1.5 Section + row system (`L594–608`, mirrored at `L849–863`)

```html
<div style="display:flex;flex-direction:column">                 <!-- section -->
  <span style="font-size:11px;letter-spacing:.16em;color:{{t.muted}};padding:14px 0 2px">{{sec.title}}</span>
  <div style="display:flex;align-items:center;gap:24px;padding:14px 0;border-bottom:1px solid {{t.line}}">   <!-- row -->
    <div style="flex:1;min-width:0;display:flex;flex-direction:column;gap:3px">
      <span style="font-size:14px">{{r.n}}</span>
      <span style="font-size:12px;color:{{t.muted}};text-wrap:pretty">{{r.d}}</span>       <!-- always rendered; empty text when d = '' -->
    </div>
    … one control slot (toggle | segmented | slider | button | kbd) …
  </div>
</div>
```

Row pitch ≈ **62 px** in canvas units (≈52 px on screen at `fitScale` 0.8333). Every row is a "label + description left,
single control right" pair; controls are right-aligned by the flex row (`gap:24px`).

### 1.6 Shared control primitives

Five builders in `settingsVals` (`L1374–1381`); all rows start from `B = {tog:false,seg:false,rng:false,btn:false,kbd:false,d:''}`.

| kind | builder | options | current | action |
|---|---|---|---|---|
| toggle | `RT(k,n,d)` | `tog(k,n)` on `s.opts[k]` | — | `flip()` |
| segmented | `RS(n,d,opts,cur,fn)` | `opts: string[]` | `cur` | `fn(opt)` |
| slider | `RR(n,d,v,min,max,step,unit,fn)` | min/max/step/unit | `v` (`vl = v+unit`) | `fn(+value)` |
| button | `RB(n,d,label,fn,danger)` | — | — | `fn()` |
| kbd badge | `RK(k,n)` | — | — | none (not interactive) |

**Toggle** (`L600–602`): track `34×18`, `border-radius:9px`, `background: on ? t.acc : t.faint`,
`transition:background .2s`; knob `14×14`, `border-radius:50%`, `background: on ? t.on : t.fg`,
`position:absolute;top:2px;left: on ? 18px : 2px`, `transition:left .2s`. (`TG(on)` at L1375.)

**Segmented** (`L603`): container `display:flex;gap:2px;padding:3px;border-radius:8px;background:t.line;flex:none`;
each option `<button>` `padding:6px 12px;border-radius:6px;font-size:12px`,
`background: selected ? t.acc : transparent`, `color: selected ? t.on : t.muted`. Selected pill is the **only** fill.

**Slider** (`L605`): wrapper `display:flex;align-items:center;gap:12px;flex:none`;
`<input type=range min max step value>` **`width:240px`**, `accent-color:t.acc`, `cursor:pointer`;
value read-out `<span style="width:48px;text-align:right;font-size:12px;color:t.alt">{{v}}{{unit}}</span>`.
Rendered (Blood Moon): track light `#f3ead8`-on-`rgba(243,234,216,0.09)` with the filled part in `#c81e2a`; verified
`accent-color = rgb(200,30,42)`. Updates fire only on change (`onChange`), not while typing.

**Outline button** (`L606`): `padding:8px 14px;border-radius:7px;font-size:12.5px`, `border:1px solid bc`,
`color:bf`, hover `background:t.line2`; `bc/bf = danger ? t.acc : (t.line2 / t.fg)` → the destructive
"Clear history" button is accent-red text/border, "Export" and "Reset sizes" are neutral.

**Kbd badge** (`L607`): `padding:4px 10px;border-radius:5px;border:1px solid t.line2;color:t.alt;font-size:12px`,
**not clickable** (no handler; see §7).

**Toast** (used by most actions, `L1268`, `L877`): `position:absolute;left:50%;bottom:36px;transform:translateX(-50%)`,
`padding:11px 18px;border-radius:8px;background:t.pop;border:1px solid t.line2;font-size:13px`,
`box-shadow:0 10px 30px rgba(0,0,0,.4)`, auto-dismiss after **1800 ms**.

---

## 2. General (`GEN`, `L1383–1390`) — `settings-general.png`, `settings-general-bottom.png`

Four sections, 10 rows. All toggles write `s.opts[key]` (`L1567`).

### STARTUP
| row | control | label | description | default | options / range |
|---|---|---|---|---|---|
| 1 | toggle | `Restore sessions on launch` | `Reopen the tiles you had open when archonui last quit.` | **on** (`opts.restore:true`) | — |
| 2 | segmented | `Default project` | `Where new sessions start when no session is focused.` | **`archonui-core`** | `archonui-core` · `archonui-web` · `archonui-cli` · `ember-gl` (`PROJECTS` order, `L992`) |

### AGENT
| row | control | label | description | default | options / range |
|---|---|---|---|---|---|
| 3 | segmented | `Default mode` | `Auto edits and runs commands. Plan proposes steps first. Ask only answers. ⇧Tab cycles in the composer.` | **`Auto`** | `Auto` · `Plan` · `Ask` |
| 4 | toggle | `Run commands in sandbox` | `Shell commands run in an isolated container with no network by default.` | **on** | — |
| 5 | toggle | `Ask before network access` | `Pause and confirm when a tool wants to reach the internet.` | **on** | — |
| 6 | toggle | `Show tool calls in chat` | `Display reads, edits and commands above each reply.` | **on** | — |
| 7 | slider | `Max steps per turn` | `The agent stops and asks for input after this many tool calls.` | **40** | min **10**, max **100**, step **5**, unit `''` |

### NOTIFICATIONS
| row | control | label | description | default |
|---|---|---|---|---|
| 8 | toggle | `Notify when a session finishes` | `Only when archonui is not the active window.` | **on** |
| 9 | toggle | `Play a sound` | *(none — `d:''`, so the description span renders with empty text)* | **off** |

### UPDATES AND PRIVACY
| row | control | label | description | default | options |
|---|---|---|---|---|---|
| 10 | segmented | `Update channel` | `Beta and Nightly get features earlier and may be unstable.` | **`Stable`** | `Stable` · `Beta` · `Nightly` |
| 11 | toggle | `Share anonymous usage data` | `Crash reports and feature counts. Never prompts or code.` | **off** | — |

### DATA
| row | control | button label | label | description | effect |
|---|---|---|---|---|---|
| 12 | button (neutral) | `Export` | `Export settings` | `Themes, providers, tools and keybindings as JSON. API keys are left out.` | toast `Exported archonui-settings.json` (no file is actually written in the prototype) |
| 13 | button (danger) | `Clear history` | `Clear session history` | `Removes every session that is not currently tiled.` | deletes every session not in `s.tiles`, toast `Cleared N sessions` |

Live effects: `restore`, `sandbox`, `netAsk`, `showTools`, `notify`, `sound`, `analytics` only move prototype state
(no visible change except the toggle itself). `Default project` changes the terminal prompt `~/code/<project> ❯` and the
project chip in every tile header (`L1338`, `L1561`). `Default mode` sets `defMode`, used for sessions created after the
change (`L1274`). `Max steps per turn` sets `maxSteps` (used only in the new-session record; no UI feedback).
`Update channel` sets `channel`, which is rendered in the nav footer (`archonui 0.9.0 · Stable` → `· Beta`).

> The brief asked for "General > Identity (rename ArchonUI, preset icons + upload)". In this prototype **IDENTITY lives at
> the top of the Appearance page** (§3.1) — the General page has no identity/icon controls.

---

## 3. Appearance (`pg.appearance` `L610–732` + `APB` `L1392–1395`)

Five blocks from `pg.appearance`, then LAYOUT and EFFECTS from `genBot`.

### 3.1 IDENTITY (`L611–634`) — `settings-appearance.png`

Header span `IDENTITY` at `font-size:11px;letter-spacing:.16em;color:t.muted` (block has `gap:12px;padding-top:14px`).
Grid `grid-template-columns:minmax(0,1fr) minmax(0,1.5fr);gap:14px`.

**Left card** — `padding:16px;border-radius:10px;border:1px solid t.line2`, column `gap:12px`:
* label `Name` (`12px, t.muted`).
* text field: `<input value={{appNameV}} onChange={{onAppName}} maxlength="18" spellcheck="false" placeholder="archonui">`,
  `font-family:t.display;font-size:34px;line-height:1.2;padding:8px 12px;border-radius:8px;background:t.line`,
  no border. Default value `archonui`; live-edits `appName` (and the uppercase `appNameUp` used elsewhere).
* preview strip: `display:flex;align-items:center;gap:10px;padding:10px 12px;border-radius:8px;
  border:1px dashed t.line2` → 22×22 `t.logo` + `appName` at `font-family:t.display;font-size:22px`,
  spacer, right-hand caption `Sidebar preview` (`11px, t.muted`).

**Right card** — same frame; header row `Icon` (12px muted) + button `Use theme icon`
(`padding:5px 10px;border-radius:6px;font-size:12px;border:1px solid t.line2;opacity: resetIconOp` where
`resetIconOp = s.logo ? 1 : 0.4`, L1432) → `set({logo:null})`.

Below: icon grid `grid-template-columns:repeat(7,minmax(0,1fr));gap:8px`:
* one tile per **unique** theme logo (`[...new Set(NAMES.map(nm=>THEMES[nm].logo))]` = **12** entries) plus uploaded
  icons: `aspect-ratio:1;border-radius:8px`, `background: active ? t.soft : t.line`,
  `outline:2px solid (active ? t.acc : transparent);outline-offset:1px`, inner image `58%×58%`, `background:url(src) center/contain`.
* last tile is an upload affordance: `<label>` `aspect-ratio:1;border-radius:8px;border:1px dashed t.line2`,
  centred `+` at `font-size:20px;color:t.muted` (hover `color:t.fg`), wrapping
  `<input type="file" accept="image/*" onChange={{onIconFile}} style="display:none">` → `URL.createObjectURL` is pushed
  to `upIcons` and becomes the active `logo` (L1430).
* Selecting an icon sets `s.logo`; `t.logo = s.logo || base.logo` (L1467), so it repaints the nav logo, sidebar, tiles.

### 3.2 PRESETS (`L636–650`) — 12 theme cards, 4-column grid

Header row: span `PRESETS` + right-hand note `Picking a preset also resets fonts and wallpaper.` (`12px, t.muted`).
Grid `repeat(4,minmax(0,1fr));gap:14px`. Card: `padding:8px;border-radius:10px`,
`border:1px solid (active ? t.acc : t.line)`, `background: (active ? t.soft : transparent)`, column `gap:8px`.

* thumb: `aspect-ratio:16/9;border-radius:6px;background:url(theme.wall) center/cover`, with an overlaid palette strip
  `position:absolute;left:8px;right:8px;bottom:8px;display:flex;gap:4px;padding:5px;border-radius:6px;
  background:rgba(0,0,0,0.5);backdrop-filter:blur(6px)` holding **6** swatches (`flex:1;height:10px;border-radius:3px`)
  in the order **`[bg, fg, muted, acc, acc2, alt]`** (after applying that theme's custom overrides).
* name: `font-family:FONTS[theme][0].d;font-size:19px` (single line, ellipsised).
* tag: `font-size:11px;color:t.acc2;white-space:nowrap` — `Active` / `Edited` / `Active · edited`, computed at `L1417`:
  `on = (name === current)`, `ed = custom[name] has keys` → `on ? (ed?'Active · edited':'Active') : (ed?'Edited':'')`.

Order (NAMES, L883): **ArchonUI · Blood Moon (default, active) · Moonlight · Wreck · Citadel · Neon · Violet · Ink ·
Bonfire · Hunter · Shards · Vale.** Picking a preset = `set({theme:nm, dispFont:null, uiFont:null, wall:null})` — i.e. it
resets fonts and wallpaper. Verified live: picking Moonlight changed the canvas background to `rgb(5,10,20)`, the page
title font to `Cormorant Garamond, serif` and the slider accent to `rgb(143,184,232)`; the whole app repaints
(`themes.md` §3–§4 lists all preset token values).

### 3.3 BACKGROUND (`L652–677`) — 6 style cards + wallpaper picker

Header span `BACKGROUND`.

**Style cards**, `repeat(6,minmax(0,1fr));gap:10px`, same active/normal border+fill as presets. Preview
`height:74px;border-radius:6px;overflow:hidden;background:t.bg` containing, in order:
1. wallpaper layer `position:absolute;inset:-6px;background:url(t.wall) center/cover;opacity:{{bc.wo}};filter:{{bc.wf}}`
2. sidebar band `left:0;top:0;bottom:0;width:18%;background:{{bc.side}}`
3. two tiles `left:24%;top:8px;bottom:8px;width:34%` and `left:62%;…;right:8px`, `border-radius:3px;background:{{bc.tile}}`

Card body: name `13px`, description `11px;color:t.muted;line-height:1.35`.

| # | name | description (verbatim) | preview/actual values (`L1475–1482`) |
|---|---|---|---|
| 1 | `Canvas` (default; `bgStyle:null` resolves to it, L1483) | `Wallpaper behind chats` | wall=`1`, side=`rgba(panel,0.72)`, sideBlur=`blur(24px) saturate(1.2)`, tile=`linear-gradient(180deg, rgba(tile,0.25), rgba(tile,0.6) 45%, rgba(tile,0.96) 85%)`, tileBlur=`none` |
| 2 | `Glass` | `Frosted tiles` | wall=`1`, side=`rgba(panel,0.60)`, sideBlur=`blur(24px) saturate(1.2)`, tile=`rgba(tile,0.55)`, tileBlur=`blur(24px)` |
| 3 | `Solid` | `No wallpaper` | wall=`0`, side=`panel` (opaque), sideBlur=`none`, tile=`tile`, tileBlur=`none` |
| 4 | `Gaps` | `Wallpaper in the gaps` | wall=`1`, side=`rgba(panel,0.96)`, sideBlur=`none`, tile=`rgba(tile,1)`, tileBlur=`none` |
| 5 | `Tinted` | `Dimmed, light glass` | wall=`0.5`, side=`rgba(panel,0.55)`, sideBlur=`blur(24px) saturate(1.2)`, tile=`rgba(tile,0.35)`, tileBlur=`blur(12px)` (= `round(blur/2)`) |
| 6 | `Ambient` | `Blurred and dark` | wall=`1`, `wf=blur(28px) brightness(0.55)`, side=`rgba(panel,0.50)`, sideBlur=`blur(24px) saturate(1.2)`, tile=`rgba(tile,0.45)`, tileBlur=`blur(6px)` |

(`blur(24px)` = the Panel-blur slider default; `rgba(tile,…)` / `rgba(panel,…)` use the **effective** Blood Moon
`tile #16100d` / `panel #0f0b09`, and `PR(a)`/`TR(a)` are `rgba(t.panel,a)`/`rgba(t.tile,a)`.) The description table
used by the cards is the one inside `settingsVals` (`BGD`, L1405); the identically keyed `BGD` in `renderVals` (L1484)
is not the one the cards read.

Live effect: picking a style rewrites both sidebars (`background: sideBg`, `backdrop-filter: sideBlur` at `L27`/`L329`)
and every tile's `background`/`backdrop-filter` (`L119`). Verified: Canvas → sidebars `rgba(15,11,9,0.72)`;
picking Glass → `rgba(15,11,9,0.6)`; picking Canvas again → back to 0.72.

**Wallpaper picker.** Caption `Wallpaper` (`12px, t.muted;padding-top:6px`). Grid
`repeat(7,minmax(0,1fr));gap:10px`; each thumb `aspect-ratio:16/10;border-radius:6px;background:url(w) center/cover`,
`outline:2px solid (active ? t.acc : transparent);outline-offset:2px`. Items = the **12 theme walls** in NAMES order
plus any uploaded walls (`s.upWalls`). Last cell is `<label>` `+ Upload` (`aspect-ratio:16/10;border-radius:6px;
border:1px dashed t.line2`, centred `12px, t.muted`, hover `t.fg`) wrapping a hidden `image/*` file input →
`URL.createObjectURL`, pushed to `upWalls` and set as `s.wall` (L1429). Picking sets `s.wall`;
`t.wall = s.wall || base.wall` (L1467) and repaints the backdrop (`L22`) plus every preset/style preview.
Default active thumb = the current theme's wall (`bloodmoon2.jpg`).

### 3.4 COLORS · &lt;THEME&gt; (`L679–692`) — 12 token overrides

Header row: `COLORS · BLOOD MOON` (theme name uppercased) + `Reset to preset` button
(`padding:6px 12px;border-radius:6px;font-size:12px;border:1px solid t.line2;opacity: resetOp` where
`resetOp = overrides ? 1 : 0.4`, L1428) → deletes `custom[name]`.

Grid `repeat(4,minmax(0,1fr));gap:10px`. Each cell is a `<label>` `position:relative;display:flex;align-items:center;
gap:12px;padding:10px;border-radius:9px;border:1px solid t.line2` (hover `background:t.line`) with:
swatch `34×34;border-radius:7px;background:{{value}};border:1px solid t.line2` → token name `13px` → value
`11px;color:t.muted;text-transform:uppercase` (hex) → marker `10.5px;color:t.acc2` reading `edited` (only when that
token is overridden) → `<input type="color" value onChange>` stretched over the whole cell
(`position:absolute;inset:0;width:100%;height:100%;opacity:0;cursor:pointer`).

The 12 editable tokens, in grid order (`TOK`, L1406) — with the effective values for the default Blood Moon theme:

| key | label | Blood Moon (effective) | key | label | Blood Moon (effective) |
|---|---|---|---|---|---|
| `bg` | `Background` | `#0d0a08` | `on` | `Text on accent` | `#f6ecdc` |
| `tile` | `Session tile` | `#16100d` | `acc` | `Accent` | `#c81e2a` |
| `panel` | `Sidebar` | `#0f0b09` | `acc2` | `Highlight` | `#d9b56a` |
| `pop` | `Popover` | `#1b1411` | `alt` | `Secondary` | `#c9a77a` |
| `fg` | `Text` | `#f3ead8` | `deep` | `Deep accent` | `#5a0a10` |
| `muted` | `Muted text` | `#bfae94` | | | |
| `faint` | `Faint` | `#6a5a48` | | | |

(Values read from the rendered swatches; for the raw preset values of all 12 themes plus how `TUNE`, `mix()` and the
override layer compose, see `themes.md` §2.3–§2.5 and §5.) Overrides are stored per theme name
(`custom[themeName][token]`) and are **not** cleared by picking another preset (the card instead gains the `Edited` tag).

### 3.5 TYPOGRAPHY (`L694–731`)

Four sub-groups, each preceded by a small caption (`12px, t.muted;padding-top:6px`, except the first which is flush).

1. **Theme pairings** (`pairings`, the 3 `FONTS[theme]` entries, `L1425`) — grid `repeat(3,minmax(0,1fr));gap:10px`;
   card `padding:14px 16px;border-radius:10px;border:1px solid (active?t.acc:t.line2);background:(active?t.soft:transparent)`;
   preview shows the app name in the display font at `30px/1`; caption `"{name} · {uiFamily}"` at
   `font-size:11.5px;color:t.muted`. Blood Moon: `Fraktur · JetBrains Mono` (active), `Pirata · Space Mono`,
   `Grenze · IBM Plex Mono`. Selecting = `set({dispFont:null, uiFont:null, fontIdx:{[theme]:k}})`.
2. Caption `Minimal` (`minPairs` = `MIN_PAIRS`, 6 fixed pairs) — same 3-col grid; preview `28px;font-weight:500;
   letter-spacing:-0.02em`. Order: `Geist · Geist` · `IBM Plex · IBM Plex Sans` · `DM Sans · DM Sans` ·
   `Instrument · Geist Mono` · `Manrope · JetBrains Mono` · `Grotesk · Space Mono`. Selecting sets both
   `dispFont` and `uiFont`.
3. Caption `Display font` (`dispFonts`, `L1427`) — grid `repeat(6,minmax(0,1fr));gap:8px`; card
   `padding:10px 12px;border-radius:8px`, preview app name at `22px/1.1` in that font, label
   `font-size:10.5px;color:t.muted`. **23 options** (6 sans-serif entries unshifted in front of the 17 display faces,
   `L978` + `L975`): Geist, Plex Sans, DM Sans, Instrument, Manrope, Grotesk, Fraktur, Pirata, Grenze, Cinzel,
   Cormorant, Italiana, Bodoni, Gloock, Mincho, Metamorphous, Young Serif, Philosopher, Bebas, Orbitron, Pixelify,
   Silkscreen, Press Start. Selecting sets `s.dispFont` only (clears neither).
4. Caption `Interface font` (`monoFonts`, `L1428`) — same 6-col grid, preview text **`Aa 0O 1lI`** at `14px` in that
   font, label `10.5px;color:t.muted`. **12 options** (`MONO`, L976): JetBrains Mono, IBM Plex Mono, Space Mono,
   DM Mono, Fira Code, Geist Mono, Geist, IBM Plex Sans, DM Sans, Instrument Sans, Manrope, Space Grotesk.
   Selecting sets `s.uiFont` → `t.ui` (and `t.code` when it looks monospace).

Live effect: any font choice repaints instantly — the root div uses `font-family:{{t.ui}}` (`L20`), the page titles use
`t.display` (`L587`), code panes use `t.code` (`L1468`). Font stacks (exact strings) are in `themes.md` §6.

### 3.6 LAYOUT (`APB` §1, `L1393`) — `settings-appearance-bottom.png`

| row | control | label | description | default | range / options |
|---|---|---|---|---|---|
| 1 | segmented | `Tiling` | `Dwindle splits the focused tile. Master keeps one large tile on the left.` | `Dwindle` | `Dwindle` · `Master` |
| 2 | slider | `Gaps` | *(empty)* | **14 px** | min **0**, max **32**, step **1**, unit `px` |
| 3 | slider | `Corner radius` | *(empty)* | **9 px** | min **0**, max **20**, step **1**, unit `px` |
| 4 | button | `Tile and panel sizes` | `Drag the gaps between tiles, or the tools panel edge, to resize. Double-click a gap to even it out.` | button label `Reset sizes` | neutral outline button → `set({ratios:{}, masterR:0.56, rwOpen:480})` |

### 3.7 EFFECTS (`APB` §2, `L1394`)

| row | control | label | description | default | range |
|---|---|---|---|---|---|
| 5 | slider | `Panel blur` | `Blur behind the sidebars and glass tiles.` | **24 px** | min **0**, max **48**, step **1**, unit `px` |
| 6 | slider | `Wallpaper dim` | *(empty)* | **0 %** | min **0**, max **80**, step **5**, unit `%` |
| 7 | toggle | `Tint all sessions` | `Give unfocused sessions the same solid tint as the focused one.` | **off** | — |
| 8 | slider | `Message text size` | *(empty)* | **15 px** | min **13**, max **19**, step **1**, unit `px` |

`Panel blur` is only visible when the active background style has a non-`none` sidebar blur (`Canvas`, `Glass`,
`Tinted`, `Ambient`); `Solid` and `Gaps` set `sideBlur:'none'` (`L1478`, `L1479`), so the slider appears inert there.
`Wallpaper dim` is likewise inert while the style is `Solid` (wallpaper opacity 0).

### 3.8 What each Appearance slider/segmented value does (verified live)

There are **no CSS custom properties anywhere in the prototype** — every style is an inline value computed in JS from
the state. The table gives the state key, the exact consumption expression and the DOM property it lands on
(this is the plumbing the Rust port must reproduce):

| control | state key | consumed at | lands on | verified measurement |
|---|---|---|---|---|
| Tiling | `s.layout` (`this.layout()`, L1266) | L1489–1502 | tile rects + drag handles (`Master` puts one large tile left, `masterR` 0.56) | scroll/view behaviour only; rect list changes shape |
| Gaps | `s.gaps` | `g`, `innerH = 1080 - 2g`, `ct = g` (L1470), tile gutters + handle widths (L1489–1509) | `main` `left/top/width/height`, tile x/y/w/h, gutter hit-areas `max(g,12)` | 14 → 32 changed focused tile from x262,y62,457×877 to x277,y77,435×847 and tile 2/3 y from 62 to 77 |
| Corner radius | `s.radius` | `rad` on each tile `<section>` (L117) and focus ring (L118), `rad2 = max(0, rad-2)` on the inner surface (L119) | `border-radius` of the tile section (rad) and the tile surface (rad−2) | 9 → 20 changed the section `border-radius` to `20px` |
| Panel blur | `s.blur` | `blurS = 'blur('+blur+'px) saturate(1.2)'` (L1474) → `BG.sb`; also `BG.tb` for Glass/Tinted/Ambient | `backdrop-filter` of both `<aside>`s (L27, L329) and of tiles (L119) | 24 → 48 changed both sidebars to `blur(48px) saturate(1.2)` |
| Wallpaper dim | `s.dim` | `wallFilter = (BG.wf ?? '') + ' brightness(' + (1 - dim/100) + ')'` (L1617) | `filter` on the wallpaper layer `div[inset:-60px]` (L22) | 0 → 40 gave `filter: brightness(0.6)` |
| Tint all sessions | `s.opts.tintAll` | tile `bg`/`blur` ternary (L1520) | unfocused tile `background: rgba(tile,0.82)` + `backdrop-filter:blur(8px)` matching the focused tile | tile surfaces only, no state-independent measure |
| Message text size | `s.fs` | `font-size:{{fs}}px` on the **root 1920×1080 div** (L20), exported as `fs` (L1617) | the app-wide base font size (all `em`-less text inherits) | 15 → 19 changed the root computed `font-size` to `19px` |

---

## 4. Keyboard (`KEYS`, `L1391`) — `settings-keyboard.png`

Three sections, **7 rows**, each row is `action name` + right-aligned kbd badge (`RK(k,n)` → `n` = name, `k` = binding).
The badge is a plain `<span>` — **there is no click handler, no rebinding affordance and no conflict detection
anywhere in the prototype** (grep for `conflict|rebind|keybind` finds only the "Export settings" description string at
L1388; the row template at L604 has no `onClick`). Rebind/conflict UX therefore does **not** exist in this spec and must
be designed separately.

| section | action name (verbatim) | default binding (verbatim) |
|---|---|---|
| TILING | `Split new session` | `Ctrl ⇧ ⏎` (U+21E7 U+23CE) |
| TILING | `Close tile` | `Ctrl ⇧ W` |
| TILING | `Move focus between tiles` | `Ctrl ⇧ ← →` (U+2190 U+2192) |
| COMPOSER | `Cycle agent mode: Auto, Plan, Ask` | `⇧ Tab` (U+21E7) |
| APP | `Next theme` | `Ctrl ⇧ T` |
| APP | `Send / new line` | `⏎ / ⇧⏎` (U+23CE ×2) |
| APP | `Close dialog` | `Esc` |

Bindings actually implemented in the prototype (`keyFn`, `L1242–1254`, plus per-input handlers). The app moved its tiling and theme defaults from Alt to Ctrl+Shift because Hyprland grabs Alt+key globally, so the Alt rows below describe the prototype, not the app's defaults:

| binding | code | matches the page? |
|---|---|---|
| `Escape` | closes `modal` or `lightbox` (L1243) | yes (`Close dialog`) |
| `Alt+Enter` | `newSession()` (L1246) → new tile | yes (`Split new session`) |
| `Alt+Q` / `Alt+q` | `closeTile(focus)` (L1247) | yes |
| `Alt+T` / `Alt+t` | next theme, cycling `NAMES` (L1248) | yes |
| `Alt+ArrowRight/Down` | focus next tile (wraps) (L1250) | **page shows `Alt ← →` only** — Up/Down/Left also work |
| `Alt+ArrowLeft/Up` | focus previous tile (wraps) | (same row) |
| `Shift+Tab` (composer textarea) | cycles `Auto → Plan → Ask` (L1530) | yes |
| `Enter` (composer, no Shift/Alt) | send (L1530) | yes (`Send`) |
| `Shift+Enter` | newline (send is skipped) | yes (`new line`) |
| `Enter` (terminal input) | run command (L1680) | not listed on the page |
| `Enter` (share-invite input) | add person (L1648) | not listed on the page |

Handler notes for the port: the global handler early-returns unless `e.altKey`, and `Alt+Enter`/`Alt+Q`/`Alt+T`/arrows
all call `preventDefault()`; the composer `Enter` check is `!e.shiftKey && !e.altKey` so `Alt+Enter` bubbles to the
global new-session handler.

---

## 5. Harnesses (`pg.providers`, `L733–759`; data `PROVIDERS` `L934–941`) — `settings-harnesses.png`

Page body: `display:grid;grid-template-columns:repeat(2,minmax(0,1fr));gap:14px;padding-top:18px`.

**Card** (`L735–756`): `display:flex;flex-direction:column;gap:14px;padding:18px;border-radius:12px`,
`border:1px solid (on ? t.line2 : t.line)`, `background:(on ? t.line : transparent)`.

1. **Head row** `display:flex;align-items:center;gap:14px`:
   * icon tile `42×42;border-radius:10px;font-size:15px;font-weight:700;background:t.line2;color:(on?t.acc2:t.muted)`
     with the 2-character sigil;
   * name `font-size:16px` + description `12px, t.muted`;
   * status `display:flex;align-items:center;gap:7px;font-size:12px;color:stC` with a `7×7` round dot in `stC`.
2. **Path row** `display:flex;align-items:center;gap:12px;padding:9px 12px;border-radius:8px;
   border:1px solid t.line;background:t.line` → label `Path` (`11px, t.muted, width:62px, flex:none`) + value
   (`12.5px`, colour `installed ? t.alt : t.faint`, ellipsised). Value text is
   `"{path} · {version}"` when installed, else **`Not found on PATH`**. This is a **read-only text line, not an input**.
3. **Auth row** `display:flex;align-items:center;gap:12px;padding:0 12px;margin-top:-6px` → label `Auth` (same 62px) +
   `{auth}` (`12.5px`, `t.fg` when installed else `t.faint`); `—` when not installed.
4. **Footer row** `display:flex;align-items:center;gap:8px`: model summary text `12px, t.muted` (flex 1), optional
   `Test` button (only when `on`), then the primary action button.

Control behaviour (`L1431–1434`):

* model summary: `"{n} models · {k} enabled"` when the harness owns models, else the literal
  `Uses its own model config` (`n`/`k` come from `MODELS.filter(m=>m.p===id)` and `s.modelOn`).
* `Test` button: `padding:7px 12px;border-radius:6px;font-size:12px;color:t.muted`, hover
  `background:t.line2;color:t.fg` → toast **`"{name} {version} · handshake OK"`** (verified: `Aider v0.86.1 · handshake OK`).
* action button: `padding:7px 14px;border-radius:6px;font-size:12px;font-weight:700`,
  `background:(on?transparent:t.acc)`, `color:(on?t.fg:t.on)`, `border:1px solid (on?t.line2:t.acc)`;
  label **`Disable`** when on, **`Enable`** when installed-but-off, **`Install`** when not installed.
  Clicking toggles `provOn[id]` **and** sets `harnInst[id]=true`; toast `Disabled {name}` / `Enabled {name}` /
  `Installed {name}`. Verified: Aider went `Not installed`/`Install` → `Ready`/`Disable`, gained a `Test` button,
  the Path row changed from `Not found on PATH` to `~/.local/bin/aider · v0.86.1`, Auth from `—` to
  `Uses OpenCode keys`, and the nav count rose 4 → 5.
* status text: `Ready` (on) / `Disabled` (installed, off) / `Not installed`; dot colour `on ? t.alt : t.faint`.
* last grid cell spans both columns: `+ Add harness (any CLI that speaks ACP)` —
  `grid-column:1/-1;padding:16px;border-radius:12px;border:1px dashed t.line2;font-size:13px;color:t.muted`
  → toast `Point the app at any CLI that speaks the Agent Client Protocol`.

Harness data (`PROVIDERS`, `L934–941`) with **default** enabled state (`provOn`, `L1201`: claude/codex/gemini/opencode
`true`, aider/goose `false`):

| # | name | sigil | description | binary path | version | auth label | installed by default | enabled by default | models owned |
|---|---|---|---|---|---|---|---|---|---|
| 1 | `Claude Code` | `Cc` | `Anthropic · Claude models` | `~/.local/bin/claude` | `v2.0.14` | `Signed in · Max plan` | yes | **yes** → `Ready` | 3 (opus, sonnet, haiku) * |
| 2 | `Codex CLI` | `Cx` | `OpenAI · GPT models` | `/opt/homebrew/bin/codex` | `v0.46.0` | `Signed in · ChatGPT Pro` | yes | **yes** → `Ready` | 2 (gpt, gptmini) |
| 3 | `Gemini CLI` | `Gm` | `Google · Gemini models` | `~/.npm-global/bin/gemini` | `v0.9.2` | `API key from GEMINI_API_KEY` | yes | **yes** → `Ready` | 2 (gemini, flash) |
| 4 | `OpenCode` | `Oc` | `Any provider, including local Ollama` | `~/.opencode/bin/opencode` | `v0.15.3` | `OpenRouter key · Ollama on :11434` | yes | **yes** → `Ready` | 3 (deepseek, local, llama) |
| 5 | `Aider` | `Ai` | `Pair programming inside git` | `~/.local/bin/aider` | `v0.86.1` | `Uses OpenCode keys` | **no** (`missing:true`) | no → `Not installed` | 0 → `Uses its own model config` |
| 6 | `Goose` | `Gs` | `Extensible agent with MCP built in` | `/usr/local/bin/goose` | `v1.9.0` | `Configured on first run` | **no** (`missing:true`) | no → `Not installed` | 0 → `Uses its own model config` |

\* the `3 models · 3 enabled` / `2 models · 1 enabled` counts on the cards come from live `s.modelOn`, which starts
`{opus:true, sonnet:true, haiku:true, gpt:true, gemini:true, local:true}` → Claude Code `3 · 3`, Codex `2 · 1`,
Gemini `2 · 1`, OpenCode `3 · 1`.

> Brief expectation vs prototype: there is **no per-harness "default model" control** and **no editable binary-path
> input**. The default model is global (`Models` page, `s.defModel`); the path is a read-only line. The header text for
> Aider/Goose flips to `Ready` only after the user clicks Install.

---

## 6. Models (`genTop MDT` `L1396` + `pg.models` `L761–777`) — `settings-models.png`, `settings-models-bottom.png`

### 6.1 DEFAULTS section (rendered above the table)

| row | control | label | description | default | options |
|---|---|---|---|---|---|
| 1 | segmented | `Default effort` | `Used for new sessions. Each session can change it in the composer.` | **`Medium`** | `Low` · `Medium` · `High` · `Max` (`EFFORTS`, L933) |
| 2 | toggle | `Use a fast model for titles and summaries` | `Session names and share summaries use Haiku 4.5.` | **on** (`opts.fastTitles`) | — |
| 3 | toggle | `Prefer long-context variants` | `Switch to 1M-token versions when a session grows past 150k tokens.` | **off** (`opts.longCtx`) | — |

### 6.2 MODELS table

Header row (`L762–766`): `the section header` = span `MODELS` + right note
`The radio sets the default for new sessions. Only enabled models appear in the composer.` (`12px, t.muted`).
Container `border-radius:10px;border:1px solid t.line2;overflow:hidden`.

Column template (identical on header and rows, `L765`/`L767`):
`grid-template-columns: 48px minmax(0,1fr) 200px 100px 64px` (measured `48px 557px 200px 100px 64px` at 1600×1000).

| col | header label | header style | row cell |
|---|---|---|---|
| 1 | *(empty)* | — | default radio: `14×14;border-radius:50%;border:1.5px solid (def?t.acc:t.faint)`, inner dot `6×6` `background:(def?t.acc:transparent)`, click = `setDef` |
| 2 | `MODEL` | `10.5px;letter-spacing:.14em;color:t.muted` | name `13.5px` + sub-label `11.5px;color:t.muted` |
| 3 | `HARNESS` | same | `12.5px;color:t.alt`, provider name (plus " · off" when the provider is disabled) |
| 4 | `CONTEXT` | same | `12.5px;color:t.muted` |
| 5 | `ON` (`text-align:right`) | same | toggle `34×18` / knob `14×14`, `left: on?18:2px` (knob `transition:left .2s`; this variant has no track transition) |

Header row: `padding:10px 16px 10px 0`, `background:t.line`. Data rows: `padding:11px 16px 11px 0`,
`border-top:1px solid t.line`, **`opacity: available ? 1 : 0.45`** (provider disabled). Row toggle = `flip()`;
radio = `setDef()`. Both are **guarded**: when the provider is off they only toast `Enable {provider} first` and change
nothing (`L1441`); setting a default also force-enables that model.

### 6.3 Full model data table (`MODELS`, `L914–927`)

`lv` = the effort-level list offered by the composer for that model (`LV_EXTRA` fills in the models declared without
`lv`; note `llama` is declared as `lv: []`, which is truthy, so it is **not** filled in and ends with **0 levels**).

| # | name (`n`) | short (`s`) | provider (`p`) | sub-label (`d`) | context | `lv` array (verbatim) | effort levels | enabled default (`modelOn`) |
|---|---|---|---|---|---|---|---|---|
| 1 | `Claude Opus 4.5` | `Opus 4.5` | claude (Claude Code) | `Most capable` | `200k` | `['Low','Medium','High','Max']` | **4** | **on**, and **default** (`defModel:'opus'`) |
| 2 | `Claude Sonnet 4.5` | `Sonnet 4.5` | claude | `Balanced` | `1M` | `['Off','Low','Medium','High','Max']` | **5** | on |
| 3 | `Claude Haiku 4.5` | `Haiku 4.5` | claude | `Fast` | `200k` | `['Off','On']` | **2** | on |
| 4 | `GPT-5` | `GPT-5` | codex (Codex CLI) | `Reasoning` | `400k` | `['Minimal','Low','Medium','High']` (LV_EXTRA) | **4** | on |
| 5 | `GPT-5 mini` | `GPT-5 mini` | codex | `Fast` | `400k` | `['Low','Medium','High']` (LV_EXTRA) | **3** | off |
| 6 | `Gemini 2.5 Pro` | `Gemini 2.5` | gemini (Gemini CLI) | `Long context` | `1M` | `['Auto','1k','4k','8k','16k','32k']` (LV_EXTRA) | **6** | on |
| 7 | `Gemini 2.5 Flash` | `Flash 2.5` | gemini | `Fast` | `1M` | `['Off','Low','High']` (LV_EXTRA) | **3** | off |
| 8 | `DeepSeek V3.1` | `DeepSeek` | opencode (OpenCode) | `Open weights` | `128k` | `['Off','On']` | **2** | off |
| 9 | `qwen3-coder 32B` | `qwen3 32B` | opencode | `Local` | `128k` | `['Off','Low','High']` | **3** | on |
| 10 | `llama3.3 70B` | `llama 70B` | opencode | `Local` | `128k` | `[]` | **0** | off |

Effort-level maths (used by the Models default-effort segmented control and the composer, `L928–932`):

```js
EFFORTS_BASE = ['Low','Medium','High','Max'];
effIdx(lv, eff)  = lv.indexOf(eff) >= 0 ? that index
                 : Math.round((EFFORTS_BASE.indexOf(eff) >= 0 ? EFFORTS_BASE.indexOf(eff)/3 : 0.5) * (lv.length - 1));
effFrac(lv, k)   = lv.length < 2 ? 0 : lv[0] === 'Off' ? k/(lv.length-1) : (k+1)/lv.length;
nearestLv(lv, f) = argmin_k |effFrac(lv,k) - f|;
nearestLv(lv, 0.5)  // what a model switch selects for a normal list
EDESC(name, frac) = /^\d+k$/  ? 'Thinking budget of '+name+' tokens.'
                  : name==='Auto' ? 'The model picks a thinking budget each turn.'
                  : frac===0 ? 'No extended thinking. Fastest replies.'
                  : frac<=0.34 ? 'Light thinking for small, contained changes.'
                  : frac<=0.6 ? 'Reads around the change before editing.'
                  : frac<1 ? 'Weighs alternatives and runs tests.'
                  : 'Thinks as long as the task needs.';
```

* New sessions get `effort = lv[effIdx(lv, defEffort)]` (`L1274`); if `lv` is empty the raw `defEffort` string is kept.
* Switching model in the composer re-picks `nearestLv(lv, 0.5)` (or keeps the current effort when `lv.length === 0`)
  and the composer shows the empty state `"{shortName} has no effort levels."` (`L303`) — reachable only with
  `llama3.3 70B`.
* **Mode support is not per-model.** `Auto`/`Plan`/`Ask` (`MODES`, `L984`) are per-session and available for every
  model. There is no capability column for it on the Models table; the only columns are MODEL / HARNESS / CONTEXT / ON.

Live effect of the table: toggling a model sets `s.modelOn[id]` → the composer's model menu is
`MODELS.filter(m => (s.modelOn[m.id] && s.provOn[m.p]) || m.id === se.model)` (`L1545`), so disabled models disappear
from the picker; the `Models` nav count (`modelOn && provOn`) updates; the radio sets `s.defModel`.

---

## 7. Tools (`pg.tools` `L779–796` + `genBot TLB` `L1398`) — `settings-tools.png`, `settings-tools-bottom.png`

### 7.1 MCP SERVERS

Header row: span `MCP SERVERS` + `+ Add server` button (`padding:7px 12px;border-radius:6px;font-size:12px;
border:1px solid t.line2`, hover `background:t.line2`) → toast `Paste a server command or URL`.
List container `border-radius:10px;border:1px solid t.line2;overflow:hidden`.

Row (`L783–791`): `display:flex;align-items:center;gap:14px;padding:13px 16px;border-bottom:1px solid t.line`.

| cell | style |
|---|---|
| sigil | `34×34;border-radius:8px;font-size:12.5px;font-weight:700;background:t.line2;color:(on?t.acc2:t.muted)` |
| name / description | `14px` / `12px;color:t.muted` (single line, ellipsised) |
| transport badge | `font-size:11px;padding:3px 8px;border-radius:5px;background:t.line;color:t.alt` (`http` / `stdio`) |
| tool count | `width:64px;font-size:12px;color:t.muted;text-align:right` (`"{n} tools"`) |
| status | `width:120px`, right-aligned, `gap:7px;font-size:12px;color:stC` + `7×7` dot in `stC` |
| Reconnect | only on error (`on && m.err && !mcpFixed`): `padding:6px 10px;border-radius:6px;font-size:12px;font-weight:700;background:t.acc;color:t.on` → marks fixed + toast `Reconnected {name}` |
| toggle | `34×18` / `14×14`, `left:on?18:2px` → `s.mcp[id]` |

Status logic (`L1443`): `on ? (err ? m.err : 'Connected') : 'Off'`; dot/colour `on ? (err ? t.acc : t.alt) : t.faint`.

Data (`MCP`, `L942–950`) — default on-state in bold (`s.mcp`, `L1202`):

| # | name | sigil | description | transport | tools | sentry error | default |
|---|---|---|---|---|---|---|---|
| 1 | `GitHub` | `Gh` | `Issues, pull requests and code search across 4 repos` | `http` | 24 | — | **on** → `Connected` |
| 2 | `Linear` | `Li` | `Read and update issues in the ASH team` | `http` | 12 | — | **on** → `Connected` |
| 3 | `Postgres` | `Pg` | `postgres://localhost:5432/archonui · read-only` | `stdio` | 6 | — | **on** → `Connected` |
| 4 | `Playwright` | `Pw` | `Drives the Browser panel for end-to-end checks` | `stdio` | 21 | — | **on** → `Connected` |
| 5 | `Sentry` | `Se` | `Errors and traces for archonui-web` | `http` | 9 | `Token expired` | **on** → `Token expired` (accent dot) + **Reconnect** button |
| 6 | `Figma` | `Fi` | `Frames and variables from design files` | `http` | 8 | — | off → `Off` |
| 7 | `Filesystem` | `Fs` | `~/code · access outside the current project` | `stdio` | 11 | — | **on** → `Connected` |

### 7.2 BUILT-IN TOOLS (`TLB`, `L1398`)

One section, one row per permission (`PERM`, `L1397`), each a segmented control
`['Allow','Ask','Off']` bound to `s.perm[key]`:

| row label (verbatim) | key | default |
|---|---|---|
| `Read files` | `read` | **`Allow`** |
| `Edit files` | `edit` | **`Ask`** |
| `Search code` | `grep` | **`Allow`** |
| `Run shell commands` | `run` | **`Ask`** |
| `Control the browser panel` | `browser` | **`Ask`** |
| `Web search` | `web` | **`Allow`** |
| `Fetch URLs` | `fetch` | **`Allow`** |

All rows have an **empty description** (`RS(n,'',…)`), so the row is just the label + segmented control.

---

## 8. Skills (`pg.skills` `L798–815`; data `SKILLS` `L951–962`) — `settings-skills.png`

Header row (`L799`): span **`SKILLS · {n} OF {total} ON`** → rendered `SKILLS · 7 OF 10 ON` (flex 1), then
`Import folder` (`padding:7px 12px;border-radius:6px;font-size:12px;border:1px solid t.line2` → toast
`Folder picker opens here`) and `+ New skill`
(`padding:7px 12px;border-radius:6px;font-size:12px;font-weight:700;background:t.acc;color:t.on` → toast
`Opens a new SKILL.md in the IDE panel`).

Grid `repeat(2,minmax(0,1fr));gap:12px`. Card (`L802–812`): `padding:16px;border-radius:10px`,
`border:1px solid (on ? t.line2 : t.line)`, column `gap:8px`:

* head row: name `14px` with `opacity:(on?1:0.55)`, `flex:1` → source badge `11px;padding:3px 8px;border-radius:5px;
  background:t.line;color:t.muted` → toggle `34×18`/`14×14`;
* description `12.5px;color:t.muted;line-height:1.5;text-wrap:pretty;opacity:(on?1:0.55)`;
* command `12px;color:t.acc2;opacity:(on?1:0.55)`;
* toggling flips `s.skillOn[id]` and updates the count in the header and the nav.

Data (`SKILLS`) with default on-state (`skillOn`, `L1203`):

| # | name | command | source | description (verbatim) | default |
|---|---|---|---|---|---|
| 1 | `Code review` | `/review` | `Built-in` | `Reviews staged changes for bugs, risky patterns and missing tests.` | **on** |
| 2 | `Write tests` | `/tests` | `Built-in` | `Adds tests for the files in context using the project test runner.` | **on** |
| 3 | `Release notes` | `/notes` | `Built-in` | `Summarises commits since the last tag into user-facing notes.` | **on** |
| 4 | `Commit messages` | `/commit` | `Built-in` | `Writes conventional commit messages from the staged diff.` | **on** |
| 5 | `Debugging` | `/debug` | `Built-in` | `Reproduces and isolates a failure before changing any code.` | **on** |
| 6 | `Refactor` | `/refactor` | `Personal` | `Small, behaviour-preserving steps with tests run after each one.` | **on** |
| 7 | `Docs writer` | `/docs` | `archonui-core` | `Follows docs/STYLE.md and updates the architecture page.` | **on** |
| 8 | `Security audit` | `/audit` | `Built-in` | `Checks auth, input handling and secrets in the changed files.` | off |
| 9 | `DB migrations` | `/migrate` | `archonui-core` | `Generates reversible migrations and a rollback plan.` | off |
| 10 | `Frontend design` | `/ui` | `Personal` | `Builds components against the archonui theme tokens.` | off |

There is no per-skill text editing in the prototype (the two header buttons only toast).

---

## 9. Extensions (`pg.extensions` `L817–847`; data `EXTS` `L963–974`) — `settings-extensions.png`

### 9.1 Search

`display:flex;align-items:center;gap:8px;padding:9px 12px;border-radius:8px;border:1px solid t.line2`;
14×14 magnifier `<svg>` (`circle cx=7 cy=7 r=4.5`, `path M10.5 10.5 L14 14`, stroke 1.4, `color:t.muted`) +
`<input value={{extQ}} onChange={{onExtQ}} placeholder="Search plugins and extensions" style="flex:1;min-width:0;font-size:13px">`.
Filter (`L1408`): `q = extQ.trim().toLowerCase()`, `match = !q || (name + ' ' + desc + ' ' + author).toLowerCase().includes(q)`.

### 9.2 INSTALLED

Header span `INSTALLED`. List container `border-radius:10px;border:1px solid t.line2;overflow:hidden`.
Row `display:flex;align-items:center;gap:14px;padding:13px 16px;border-bottom:1px solid t.line`:

| cell | style / content |
|---|---|
| sigil | `34×34;border-radius:8px;font-size:12.5px;font-weight:700;background:t.line2;color:t.acc2` — **first two characters of the name** (`n.slice(0,2)`, so `Vi`, `Pr`, `Wa`, `Di`) |
| name + meta | `14px` name, then inline `11.5px;color:t.muted` = `"{author} · {version}"` |
| description | `12px;color:t.muted` |
| `Remove` | `padding:6px 10px;border-radius:6px;font-size:12px;color:t.muted`, hover `background:t.line2;color:t.fg` → drops it from `s.installed` + toast `Removed {name}` |
| toggle | `34×18`/`14×14`, bound to `s.extOn[id]` |

Empty state (only when the filtered list is empty): `padding:14px 16px;font-size:12.5px;color:t.muted`,
text **`No installed extensions match.`**

Default installed (`s.installed`, `L1204`) = `vim, prettier, wakatime, discord`; default enabled
(`s.extOn`, `L1204`) = `vim:true, prettier:true, wakatime:true, discord:false`.

### 9.3 BROWSE

Header span `BROWSE`; grid `repeat(3,minmax(0,1fr));gap:12px`. Card `padding:16px;border-radius:10px;
border:1px solid t.line2`, column `gap:10px`: head row (sigil `34×34`, `background:t.line2`, `color:t.muted` + name
`14px` + `11.5px` meta `"{author} · {version}"`), description `flex:1;font-size:12.5px;color:t.muted;line-height:1.5;
text-wrap:pretty`, and `Install` (`align-self:flex-start;padding:7px 14px;border-radius:6px;font-size:12px;
font-weight:700;background:t.acc;color:t.on` → pushes to `installed`, sets `extOn[id]=true`, toast `Installed {name}`).
Empty state: `Nothing else to install.`

Data (`EXTS`) — all ten, with default state:

| # | name | author | version | description (verbatim) | default |
|---|---|---|---|---|---|
| 1 | `Vim mode` | `archonui` | `1.4.0` | `Modal editing in the composer and IDE panel.` | installed + **on** |
| 2 | `Prettier on save` | `prettier` | `3.3.2` | `Formats files the agent edits before they are staged.` | installed + **on** |
| 3 | `WakaTime` | `wakatime` | `0.8.1` | `Tracks time per project and session.` | installed + **on** |
| 4 | `Discord presence` | `community` | `0.3.0` | `Shows the active project in your Discord status.` | installed + off |
| 5 | `Linear sync` | `linear` | `1.1.0` | `Links sessions to issues and posts a summary when they finish.` | browse |
| 6 | `Pomodoro` | `community` | `0.5.2` | `Focus timer at the bottom of the left sidebar.` | browse |
| 7 | `Docker` | `docker` | `2.0.0` | `Containers panel, and sandboxed runs in your own images.` | browse |
| 8 | `Tailwind IntelliSense` | `tailwindlabs` | `0.12.4` | `Class completion and colour previews in the IDE panel.` | browse |
| 9 | `Now playing` | `community` | `0.2.1` | `Music controls in the tools panel.` | browse |
| 10 | `Sentry insights` | `sentry` | `1.0.3` | `Opens a session from any Sentry issue with the stack trace attached.` | browse |

Note the INSTALLED count in the nav is `s.installed.length` (4), so it counts installed-but-disabled extensions too.

---

## 10. Screenshot index (all `.qa/design/`)

| file | page / state |
|---|---|
| `settings-general.png` | General top (STARTUP, AGENT, NOTIFICATIONS) |
| `settings-general-bottom.png` | General bottom (UPDATES AND PRIVACY, DATA) |
| `settings-appearance.png` | Appearance top (IDENTITY name+icon, PRESETS first rows) |
| `settings-appearance-mid.png` | Appearance mid (PRESETS bottom, all 6 BACKGROUND cards, Wallpaper grid, COLORS header) |
| `settings-appearance-fonts.png` | Appearance (COLORS · BLOOD MOON 12 swatches, TYPOGRAPHY pairings + Minimal) |
| `settings-appearance-bottom.png` | Appearance bottom (Interface font 12 tiles, LAYOUT, EFFECTS with all 5 sliders) |
| `settings-keyboard.png` | Keyboard (all 7 shortcuts) |
| `settings-harnesses.png` | Harnesses default (4 Ready, 2 Not installed) |
| `settings-harnesses-installed.png` | Harnesses after installing Aider (nav 4→5, toast `Installed Aider`) |
| `settings-harnesses-toast.png` | `Test` result toast `Aider v0.86.1 · handshake OK` |
| `settings-models.png` | Models (DEFAULTS + table rows 1–8) |
| `settings-models-bottom.png` | Models bottom (all 10 rows; `qwen3-coder 32B` and `llama3.3 70B`) |
| `settings-tools.png` | Tools (MCP SERVERS incl. Sentry error + Reconnect, BUILT-IN TOOLS start) |
| `settings-tools-bottom.png` | Tools bottom (remaining built-in tool permissions) |
| `settings-skills.png` | Skills (7 OF 10 ON, all 10 cards) |
| `settings-extensions.png` | Extensions (search, INSTALLED 4, BROWSE 6) |
| `settings-extensions-bottom.png` | Extensions bottom (browse cards) |

Step scripts used to produce them: `settings-steps.json`, `settings-steps3.json` (live-effect measurements),
`settings-steps4.json` (screenshots of interactions).

---

## 11. Gaps / ambiguities to resolve before porting

1. **Keyboard rebinding + conflict detection do not exist in the prototype.** The Keyboard page is a static list of 7
   shortcuts rendered as inert badges (`RK` → `kbd` flag, `L604`); there is no keymap store, no capture mode, no
   conflict state, and no reset-to-default. The bindings themselves are hard-coded in `keyFn` (`L1242–1254`) and in two
   input handlers. The port needs a separate design decision (or the page should ship read-only).
2. **Identity lives on Appearance, not General** — contradicts the brief's page map. Please confirm the intended page.
3. **`llama3.3 70B` has `lv: []`** and therefore 0 effort levels (`L923` guarded by `if (!m.lv)`). The composer then
   shows "llama3.3 70B has no effort levels." Decide whether that model should get a level list or be hidden.
4. **Harnesses have no editable path field and no per-harness default model** — the brief expected both. Path/version
   is a read-only line, and the default model is global (`Models`).
5. **`Alt + Arrow` is documented as `← →` only** on the Keyboard page, but all four arrows move focus in code.
6. **`Uses its own model config`** is the literal string shown for harnesses with no `MODELS` entries (Aider, Goose).
7. Prototype buttons that only `flash()` a toast (Export, Add server, Add harness, Import folder, New skill, Reconnect)
   have no defined behaviour beyond the toast text quoted above.
