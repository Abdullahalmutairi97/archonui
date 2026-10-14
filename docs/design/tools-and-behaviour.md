# ArchonUI — Tools Panel, Behaviour & Interactive-Control Extraction

Source of truth: `design/ArchonUI Agent v3.dc.html` (1689 lines) + `design/support.js` runtime.
All values below are lifted verbatim (line refs `L###`). Default props: `theme = "Blood Moon"`, `layout = "Dwindle"`, `rightTab = "IDE"` (`L8`, `data-dc-script` props).
Screenshots (1600×1000 viewport, `fitScale = min(1600/1920, 1000/1080) = 0.8333`, app drawn 1920×1080 centred → 50 px black bars top/bottom):
`.qa/design/tool-browser.png`, `tool-ide.png`, `tool-files.png`, `tool-git.png`, `tool-term.png` (each captured after clicking its tab; capture steps: `.qa/design/steps-tools.json`).

**Unit conversion:** every number in this doc is a *design pixel* (1920×1080 space) unless marked “measured CSS px”. To convert a measured CSS px value back to design px multiply by 1.2 (÷0.8333).

---

## 0. Tokens this panel uses (default theme Blood Moon)

`THEMES` (`L883`) then `TUNE` override for Blood Moon only + computed derived tokens (`renderVals`, `L1457-1485`):

| token | source | value (Blood Moon) |
|---|---|---|
| `bg` | `L883` | `#0d0a08` |
| `tile` | `TUNE` `L1463` | `#16100d` |
| `panel` (sidebar) | `TUNE` `L1463` | `#0f0b09` |
| `pop` | `TUNE` `L1463` | `#1b1411` |
| `fg` | `TUNE` `L1463` | `#f3ead8` (measured `rgb(243,234,216)`) |
| `muted` | `TUNE` `L1463` | `#bfae94` (measured `rgb(191,174,148)`) |
| `faint` | `TUNE` `L1463` | `#6a5a48` (measured `rgb(106,90,72)`) |
| `acc` | `TUNE` `L1463` | `#c81e2a` (measured `rgb(200,30,42)`) |
| `acc2` | base `L883` | `#d9b56a` (measured `rgb(217,181,106)`) |
| `alt` | base `L883` | `#c9a77a` (measured `rgb(201,167,122)`) |
| `on` | base `L883` | `#f6ecdc` (measured `rgb(246,236,220)`) |
| `deep` | base `L883` | `#5a0a10` |
| `soft` | `TUNE` `L1463` | `rgba(176,20,30,0.32)` — the **red band** (measured, §A2) |
| `line` | computed `L1468` | `rgba(fg,0.09)` = `rgba(243,234,216,0.09)` |
| `line2` | computed `L1468` | `rgba(fg,0.22)` = `rgba(243,234,216,0.22)` |
| `codeBg` | `L1615` | `rgba(bg,0.72)` = `rgba(13,10,8,0.72)` |
| `display` font | `FONTS['Blood Moon'][0]` `L902` | `'UnifrakturCook', serif` |
| `ui` font | `FONTS['Blood Moon'][0]` `L902` | `'JetBrains Mono', monospace` |
| `code` font | `L1468` | `ui` if it matches `/mono|code|monospace/` else `'JetBrains Mono', monospace` → here `'JetBrains Mono', monospace` |
| sidebar bg | `BGS.Canvas.side` `L1476` | `rgba(panel,0.72)` = `rgba(15,11,9,0.72)` |
| sidebar blur | `L1474` | `blur(24px) saturate(1.2)` (`blur` = state, default 24) |
| focus ring | `L1468` | `linear-gradient(135deg, #d9b56a, #c81e2a 50%, #5a0a10)` (measured, §B5) |
| scrollbars | global CSS `L21` | track transparent, thumb `rgba(128,128,128,.35)` 6px, radius 3px |

Other themes: `soft` comes from the 16th `T()` argument (e.g. ArchonUI `rgba(255,255,255,0.12)`, Moonlight `rgba(143,184,232,0.20)`) unless the user edits `acc`, in which case `soft = rgba(acc,0.28)` (`L1468`).

---

# A. The RIGHT TOOLS PANEL

## A0. Panel shell

`L329-331` — `<aside data-screen-label="Tools sidebar">`:

```
position:absolute; left: rx px; top: 0; width: rw px; height: 1080px;
box-sizing:border-box; border-left:1px solid t.line2; background: sideBg;
backdrop-filter: sideBlur; overflow:hidden; transition: width .3s,left .3s,background .3s
```

Geometry (`L1470`, `L1473`): `sx=0, sh=1080, g=14` (gaps), `rw = rightOpen ? rwOpen : 52`, `rx = 1920 - rw`, `lw = leftOpen ? 300 : 52`, `cx = lw + g`, `cw = rx - g - cx`.
Defaults: `rwOpen = 480` → `rx = 1440`, `cx = 314`, `cw = 1112`. Measured: x = 1200 CSS px, width = 400 CSS px, height = 900 CSS px.
Collapsed rail width **52**; expanded default **480**; drag range **360–820** (`L1226`), double-click reset **480** (`L1653`).

Header row (`L344-345`) — `display:flex; align-items:center; gap:4px; padding:10px 10px; border-bottom:1px solid t.line`:

| control | style | measured rect (CSS px) |
|---|---|---|
| tab button | `display:flex;align-items:center;padding:7px 8px;border-radius:6px;font-size:12.5px`, active `background:t.line2; color:t.fg`, inactive `transparent / t.muted` | Browser 57×26 @(1209,60); IDE 32×26; Files 45×26; Git 32×26; Terminal 63×26 |
| collapse chevron `›` (title “Collapse”) | 34×34, radius 6, colour `t.muted`, hover bg `t.line2` + colour `t.fg` | 28×28 @(1473,58) |
| separator | `width:1px;height:16px;background:t.line2;margin:0 4px` | — |
| window buttons Minimize / Maximize / Close | 28×28, radius 6, colour `t.muted`; Close hover `background:t.acc;color:t.on` | 23×23 each |

Tab order and glyphs (`L1612`): `[['Browser','◎'],['IDE','‹›'],['Files','▤'],['Git','⎇'],['Terminal','▸']]` — the header uses the **text label**, the collapsed rail uses these glyphs as SVG icons in 34×34 buttons (`L335`, svgs at `L337`: globe / `‹›` / folder / git-branch / terminal-window).

Collapsed rail (`L332-341`): `display:flex; flex-direction:column; align-items:center; gap:6px; padding:10px 0; height:100%`, then the expand/collapse chevron (34×34, `‹`, `d="M10 3.5L5.5 8l4.5 4.5"`, no `title`), the 5 tab icon buttons (34×34, `title` = tab name), a `flex:1` spacer, then Minimize/Maximize/Close (28×28).

**Defaults / content per tab**: `rightTab` default `'IDE'` (`L8`); `rp = { browser: rightTab==='Browser', ide:…, files:…, git:…, term:… }` (`L1657`). Clicking any tab does `setState({ rightTab: n, rightOpen: true })` (`L1656`) — it also un-collapses the panel.

---

## A1. Browser tab (`rp.browser`) — `L356-382`

| part | exact style (design px) |
|---|---|
| controls row `L357` | `display:flex; align-items:center; gap:6px; padding:10px 12px` |
| “‹ › ↻” | **one plain `<span>`**, `color:t.muted; padding:0 4px` — static text, *not* three buttons; back/forward/reload do nothing |
| URL bar `L359-362` | `flex:1; display:flex; align-items:center; gap:8px; padding:7px 12px; border-radius:7px; background:t.line` (measured `rgba(243,234,216,0.09)`, radius 7px, padding 7px 12px) |
| URL dot | `<span>` `●` `color:t.alt; font-size:11px` (measured `rgb(201,167,122)`) |
| URL input | `flex:1; min-width:0; font-size:12.5px`, value `http://localhost:5173/login`, `onChange → state.url` (nothing else reads `url`) |
| page frame `L363` | `flex:1; margin:0 12px 12px; border-radius:8px; overflow:hidden; background:t.bg; border:1px solid t.line2; display:flex; flex-direction:column; min-height:0` |
| site nav `L365` | `display:flex; align-items:center; gap:16px; padding:12px 16px; border-bottom:1px solid t.line; font-size:12px; color:t.muted`; “archonui” in display font 18px `t.fg`; then `flex:1` spacer, `Docs`, `Sessions`, `Login` (`color:t.fg`) — all static |
| hero `L367` | `height:200px; background:url(t.wall) center/cover; image-rendering:t.render; display:flex; align-items:flex-end; padding:18px`; caption “Sign in to continue”, display font 36px, `text-shadow:0 2px 18px rgba(0,0,0,.7)` |
| form `L369-373` | `padding:18px 16px; display:flex; flex-direction:column; gap:10px`; “email” and “password” are **static divs** (`padding:10px 12px; border-radius:6px; border:1px solid t.line2; font-size:12.5px; color:t.muted`); “Sign in” is a **static div** (`background:t.acc; color:t.on; font-weight:700; text-align:center`) |
| console `L374-378` | `margin-top:auto; padding:10px 14px; border-top:1px solid t.line; column; gap:4px; font-size:11.5px; font-family:t.ui`; lines: `CONSOLE` (`t.muted`), `[vite] hmr update /src/server/session.ts` (`t.alt`), `GET /api/session 200 · 4ms (cache hit)` (`t.muted`) |

Whole tab is a static mock-up: the only live control is the URL input. See `tool-browser.png`.

---

## A2. IDE tab (`rp.ide`) — `L384-403` + `L1659-1661`

State: `openFiles = ['src/server/session.ts','src/server/cache.ts']`, active `file = 'src/server/session.ts'` (`L1192`).

### File tab strip `L386-390`
`display:flex; gap:2px; padding:8px 10px 0; overflow:hidden;` then per file:
`cursor:pointer; display:flex; align-items:center; gap:8px; padding:7px 8px 7px 12px; border-radius:6px 6px 0 0; font-size:12px; white-space:nowrap;`
active → `background:t.line2 (rgba(243,234,216,0.22)); color:t.fg`; inactive → `background:transparent; color:t.muted`.
Label = **basename only** (“session.ts”, “cache.ts”). Close control = `×` `<button>` `all:unset; cursor:pointer; color:t.muted; padding:0 3px` (measured 11×13 @ (1286,108) and (1370,108)).
Click tab → `setState({file})`. Click `×` → `stopPropagation` + remove from `openFiles`; if it was the only file the list keeps `[file]`, and if it was active the new active file is `openFiles[0]` (`L1659`).

### Breadcrumb `L392`
`padding:8px 14px; font-size:11.5px; color:t.muted; background:t.line; border-bottom:1px solid t.line` — value `crumb = project + ' / ' + file.split('/').join(' / ')` (`L1661`) → `archonui-core / src / server / session.ts`. Measured rect: 399×27 @ (1201,128).

### Code area `L393-398`
Container: `flex:1; overflow:auto; min-height:0; padding:10px 0; font-family:t.code; font-size:12.5px; line-height:1.7; user-select:text` (measured: JetBrains Mono, 12.5px, line-height 21.25px, padding 10px 0).
Row: `display:flex; gap:14px; padding:0 14px 0 0; background:<changed? t.soft : transparent>; white-space:pre`.
Gutter number: `width:34px; text-align:right; color:t.faint; flex:none`.

**Changed-line band (the red band):** `bg = t.soft` when `file === 'src/server/session.ts' && i >= 8 && i <= 15` (**0-based**, `L1570`) → 1-based **lines 9–16** (8 rows). Measured backgrounds: rows 1–8 and 17–23 `transparent`, rows 9–16 `rgba(176,20,30,0.32)`. It is a hard-coded demo diff, *not* derived from the Git state; it disappears when another file is active (`cache.ts` has no band).

`session.ts` content (23 lines, `L1025-1047`) with the band on 9–16:

```
 1  import { TokenCache } from "./cache";
 2
 3  const cache = new TokenCache({ ttl: 300 });
 4
 5  export async function sessionMiddleware(req, res, next) {
 6    const conn = req.socket.id;
 7    let token = cache.get(conn);
 8
 9    if (!token) {                              ← band start
10      token = await verify(req.headers.authorization);
11      cache.set(conn, token);
12      metrics.inc("session.cache.miss");
13    } else {
14      metrics.inc("session.cache.hit");
15    }
16                                             ← band end
17    req.session = token;
18    next();
19  }
20
21  export function onLogout(conn) {
22    cache.delete(conn);
23  }
```

Second tab `cache.ts` (17 lines / 408 B, `L1048-1064`, `TokenCache` class) → breadcrumb `archonui-core / src / server / cache.ts`, status bar `17 lines`.

File sizes shown by the tree (`fsize`, `L1574`) are the raw byte length of the template string: loop.ts 602 B, tools.ts 251 B, session.ts 486 B, cache.ts 408 B, session.test.ts 268 B, package.json 157 B, tsconfig.json 119 B, .gitignore 22 B, architecture.md 88 B, README.md 80 B.

**Token colours** (`tokenize`, `L1110-1127`; regex `(\/\/.*$|^#.*$)|("[^"]*"|'[^']*'|`[^`]*`)|(\b\d+\b)|([A-Za-z_$][\w$]*)|(\s+)|(.)`):

| match | colour | measured (Blood Moon) |
|---|---|---|
| comment (`//…` or `#…`) | `t.muted` | `rgb(191,174,148)` |
| string literal | `t.acc2` | `rgb(217,181,106)` |
| number | `t.alt` | `rgb(201,167,122)` |
| keyword (`KW` set `L1108`) | `t.acc` | `rgb(200,30,42)` |
| literal (`null undefined true false this`) | `t.alt` | `rgb(201,167,122)` |
| identifier starting uppercase | `t.acc2` | `rgb(217,181,106)` |
| other identifier | `t.fg` | `rgb(243,234,216)` |
| punctuation `{}()[];,.:=<>` | `t.muted` | `rgb(191,174,148)` |
| whitespace | `t.fg` | — |

### Status bar `L400-402`
`display:flex; gap:14px; padding:7px 14px; font-size:11px; color:t.muted; border-top:1px solid t.line`.
Children in order: `⎇ {{branch}}` (`color:t.alt`), `{{lang}}`, `<span style="flex:1">`, `{{lineCount}} lines`, `UTF-8`.
Values (`L1661`): `branch` = current project branch → `feat/session-cache`; `lang = LANG[ext]` (`L1107`: ts→`TypeScript`, json→`JSON`, md→`Markdown`, else `Text`); `lineCount = code.length`.
Rendered: `⎇ feat/session-cache   TypeScript            23 lines   UTF-8` — **separated by 14 px gaps only, no `·` characters**. Measured: bar 399×24 @ (1201,926); branch `rgb(201,167,122)`, rest `rgb(191,174,148)`, all 11px.

See `tool-ide.png`.

---

## A3. Files tab (`rp.files`) — `L405-455`

State: `project = 'archonui-core'`, `projPath = '~/code/archonui-core'`, `fq = ''`, `selFile = 'src/server/session.ts'`, `openDirs = { src:true, 'src/server':true, 'src/agent':false, tests:true }` (`L1192`).

**Header `L406-414`** — `display:flex; align-items:center; gap:8px; padding:12px 10px 8px 16px`
* left column `flex:1`: project name (display font **20px**, line-height 1.1, measured `UnifrakturCook`) + path (`11.5px`, `t.muted`);
* three icon buttons 28×28, radius 6, `color:t.muted`, hover `background:t.line2; color:t.fg`, titles **“New file”**, **“New folder”**, **“Collapse all”** (measured 23×23 at y=110).

**Filter row `L416-419`** — `display:flex; align-items:center; gap:8px; margin:0 12px 8px; padding:7px 10px; border-radius:7px; background:t.line; border:1px solid t.line`; 13×13 magnifier in `t.muted`; `<input placeholder="Filter files">` 12.5px; count `11px t.muted` = `paths.length + ' files' + (changes ? ' · ' + changes.length + ' changed' : '')` → `10 files · 3 changed` (measured row 379×29 @ (1211,145)).

**Tree `L421-443`** — container `flex:1; overflow:auto; min-height:0; padding:0 8px 8px`.
Row (`row()`, `L1580-1592` + `L422-435`):
```
position:relative; cursor:pointer; display:flex; align-items:center; gap:8px;
height:30px; padding:0 10px 0 (10 + depth*16)px; border-radius:6px; font-size:13px;
background: t.soft if it is the selected file else transparent;  hover: t.line2;  title = full path
```
children, left→right:
1. **indent guides**: one absolutely positioned span per ancestor level — `left:(15 + k*16)px; top:0; bottom:0; width:1px; background:t.line2` (`L423`);
2. chevron span `width:10px; font-size:9px; color:t.muted` — `▼` open dir, `▶` closed dir, empty for files;
3. dir: folder SVG 15×15, colour `t.acc2` when open else `t.alt`; file: extension badge `width:30px; text-align:center; padding:2px 0; border-radius:3px; font-size:9.5px; font-weight:700; letter-spacing:.04em; color:<tagC>; background:t.line`;
   tags (`TAGS`, `L1575`): `ts`→`TS` (`t.acc2`), `json`→`{ }` (`t.alt`), `md`→`MD` (`t.muted`), `gitignore`→`GIT` (`t.muted`), else `TXT` (`t.muted`);
4. name span `flex:1; min-width:0; ellipsis; color:t.acc2 if selected else t.fg` containing a nested sub span `color:t.muted; font-size:11px; margin-left:8px` (parent dir, only populated in filter mode);
5. change indicator span `font-size:11px; font-weight:700; width:12px; text-align:center` — file with a git entry → its letter (`A`→`t.alt`, `M`→`t.acc2`, other→`t.muted`), directory containing changes → `•` (`t.acc2`), otherwise empty;
6. meta span `font-size:11px; width:48px; text-align:right; color:t.muted` — dirs `"<n> items"`, files `"N B"` / `"N.N KB"` (byte length of the file body).

Default tree order (`walk()`, `L1595-1601`; sort = directories first, then `localeCompare`):
```
▶ docs            1 items        ▼ src             • 4 items
   (collapsed)                    ▶ agent             2 items
                                  ▼ server          • 2 items
                                     TS cache.ts    A   408 B
                                     TS session.ts  M   486 B   ← selected ⇒ t.soft row bg
                                  ▼ tests           • 1 items
                                     TS session.test.ts M 268 B
GIT .gitignore                     22 B
{ } package.json                  157 B
MD  README.md                      80 B
{ } tsconfig.json                 119 B
```
(measured rows: x=1208, width=386, height=25 CSS px = 30 design px; the selected `session.ts` row measured background `rgba(176,20,30,0.32)`.)
Filter mode (`fq` non-empty) flattens to a sorted list of full paths at depth 0 with the parent dir in the `sub` span; no match → `<span>No files match.</span>` `12.5px t.muted; padding:14px 10px` (`L444-446`).

**Selection footer `L447-454`** (only when a file is selected) —
`border-top:1px solid t.line2; padding:12px 14px 14px; column; gap:10px; background:t.line`;
* title row: file name `14px; font-weight:700; ellipsis` + status label `11px`, colour `stC` → `modified · +28 −8` (`added`/`modified` + `+a` + optional `−d`, from the git change entry; `unchanged` + `t.muted` otherwise);
* info line `11.5px t.muted`: `src/server/session.ts · 486 B · 23 lines · 4 min ago` (`MOD` timestamps `L1606`: session.ts/cache.ts “4 min ago”, session.test.ts “12 min ago”, else “2 days ago”, new files “just now”);
* preview box `height:118px; border-radius:6px; background:t.bg; border:1px solid t.line; padding:8px 0; font-size:11.5px; line-height:1.6; overflow:hidden` — first **6** lines, gutter `width:22px; text-align:right; color:t.faint`, syntax colours as §A2;
* 3-button row `display:grid; grid-template-columns:1fr 1fr 1fr; gap:6px` — **Open**, **Attach**, **Copy path**: `display:flex; align-items:center; justify-content:center; gap:6px; padding:8px 0; border-radius:6px; font-size:12px; border:1px solid t.line2`, hover `background:t.line2`, each with a 13×13 icon. Measured 122×28 CSS px each at y=910.

See `tool-files.png`.

---

## A4. Git tab (`rp.git`) — `L457-494` + `L1670-1678`

State (`L1193-1195`): `changes = [{f:'src/server/session.ts',st:'M',a:28,d:8,on:true},{f:'src/server/cache.ts',st:'A',a:41,d:0,on:true},{f:'tests/session.test.ts',st:'M',a:18,d:2,on:false}]`, `commitMsg='feat(session): cache tokens per connection'`, `ahead=2`, `log` = 5 entries (below).

| block | exact layout / style |
|---|---|
| **header** `L458-463` | `padding:14px 16px 8px; display:flex; align-items:center; gap:10px` (measured rect 399×43 @ (1201,96)) · branch chip `<span>`: `font-size:13px; padding:5px 10px; border-radius:6px; background:t.line2; color:t.alt` → `⎇ feat/session-cache` · `↑2 ↓0 · origin`: `12px; color:t.muted` · `flex:1` spacer · **Push** `<button>`: `font-size:12px; padding:6px 10px; border-radius:6px; border:1px solid t.line2` (no hover style) |
| **changes label** `L464-465` | `<span>` `font-size:10.5px; letter-spacing:.16em; color:t.muted; padding:10px 16px 6px` → `CHANGES · 3` (measured letter-spacing 1.68px) |
| **change list** `L466` | wrapper `display:flex; flex-direction:column; padding:0 8px` |
| **change row** `L467-474` | `cursor:pointer; display:flex; align-items:center; gap:10px; padding:7px 8px; border-radius:6px; font-size:12.5px`, hover `background:t.line2` — measured 386×26 @ y=164/190/216. Children: **checkbox** `width:14px;height:14px;border-radius:3px;border:1.5px solid <boxBd>;background:<boxBg>;display:grid;place-items:center;font-size:10px;color:t.on` (staged: `bg=t.acc`, `bd=t.acc`, glyph `✓`; unstaged: `bg=transparent`, `bd=t.faint`, empty — measured `1px solid rgb(200,30,42)` / `rgb(200,30,42)` vs `1px solid rgb(106,90,72)`) · **status letter** span `width:12px; color:t.acc2` (`M`/`A`) · **path** `flex:1; ellipsis` · **`+N`** span `color:t.alt` (measured `rgb(201,167,122)`) · **`−N`** span `color:t.muted` (empty string for `d === 0`) |
| **empty state** `L471-473` | `<span>Working tree clean.</span> padding:10px 8px; 12.5px; t.muted` |
| **commit box** `L475-484` | `margin:12px 16px; border-radius:8px; border:1px solid t.line2; background:t.line` (measured 373×86 @ (1214,252)) · `<textarea rows="2" placeholder="Commit message">` `width:100%; padding:10px 12px; font-size:12.5px; line-height:1.5` · footer `display:flex; justify-content:space-between; align-items:center; padding:6px 8px 8px 12px`: left `<span>2 of 3 staged</span>` (`11px; t.muted`; = `staged + ' of ' + changes.length + ' staged'`) · right **Commit** button `padding:7px 14px; border-radius:6px; font-size:12px; font-weight:700; background:t.acc; color:t.on; opacity:<1 or 0.4>` (`commitOp = staged && message ? 1 : 0.4`) |
| **history label** `L486` | `<span>HISTORY</span>` `10.5px; letter-spacing:.16em; t.muted; padding:6px 16px` (measured @ y=348) |
| **log** `L487-493` | container `flex:1; overflow:auto; min-height:0; padding:0 16px 12px` · row `display:grid; grid-template-columns:14px 64px 1fr auto; gap:8px; align-items:center; padding:7px 0; font-size:12px; border-bottom:1px solid t.line` (measured computed cols `14px 64px 325.19px 19.81px`) · children: **dot** `width:8px;height:8px;border-radius:50%;border:2px solid <c>` (`c = t.acc2` for row 0 else `t.faint`) · **hash** `color:t.alt` · **message** (ellipsis) · **time** `color:t.muted; font-size:11px` |

Default log (`L1195`): `a41c9e2 test: cover token expiry 12m` / `7d03b1f refactor: extract verify() 40m` / `19ce88a chore: bump vitest 3h` / `e2b7710 feat: tiled sessions 1d` / `0f9a3c4 fix: stream backpressure 2d`.

**There is NO diff view in this panel.** No per-file diff, no hunk colours, no split/unified toggle, no “stage all”, no branch switcher, no fetch/pull. Staging is only the per-row checkbox, and the only “diff colour” signals are: the `+N` (`t.alt`) / `−N` (`t.muted`) counters here, the `+a −d` status label in Files, and the `t.soft` band in the IDE. If the Rust port needs a real diff view it is net-new (see §D).

See `tool-git.png`.

---

## A5. Terminal tab (`rp.term`) — `L498-507` + `L1321-1341`, `L1679-1681`

| part | style |
|---|---|
| container `L499-504` | `onClick=tl.focusTerm`; `flex:1; overflow:auto; min-height:0; padding:14px 16px; font-family:t.code; font-size:12.5px; line-height:1.6; display:flex; flex-direction:column; cursor:text; user-select:text;` `data-term="1"` (measured: JetBrains Mono 12.5px, line-height 20px, padding 14px 16px) |
| lines `L502-503` | one `<span>` per line, `white-space:pre-wrap; color:<line colour>` |
| prompt row `L504-506` | `display:flex; gap:8px; align-items:center`; prompt `<span>` `color:t.acc2` = `~/code/archonui-core ❯` (from `prompt() = '~/code/' + project + ' ❯'`, `L1340`); `<input>` `flex:1; min-width:0; caret-color:t.acc2; spellcheck="false"` (measured font 12.5px mono, caret `rgb(217,181,106)`), `onChange → termIn`, `Enter → runCmd` |

Font: `t.code` (JetBrains Mono by default). No font-size control, no tabs, no real shell.

Seed output (`termSeed`, `L1337`), colour per line:

| text | colour |
|---|---|
| `~/code/archonui-core ❯ pnpm test` | `t.fg` (prompt+command are **one** span here) |
| ` ✓ tests/session.test.ts (6)` | `t.alt` |
| ` Tests  128 passed (3.2s)` | `t.acc2` |
| `~/code/archonui-core ❯ git status -s` | `t.fg` |
| ` M src/server/session.ts` | `t.acc` |
| ` A src/server/cache.ts` | `t.acc` |
| `type 'help' for commands` | `t.muted` |

Command handling (`runCmd`, `L1321-1335`) — echoes `<prompt> <cmd>` in `t.fg` first, then:
`clear` clears the buffer · `help` → `help · ls · git status · git log · pnpm test · archonui ls · theme <name> · clear` · `ls` → `README.md  package.json  src/  tests/  node_modules/` · `git status` → `On branch <branch>` + one line per change (`staged  <f>` in `t.alt` / `modified  <f>` in `t.acc`) or `nothing to commit, working tree clean` · `git log` → first 5 log entries `<hash>  <msg>` in `t.alt` · `pnpm test` → 3 lines (RUN in `t.muted`, `✓ tests/…` in `t.alt`, `Tests  128 passed (3.2s)` in `t.acc2`) · `archonui ls` → `<status padded to 8> <name>` per session (`t.acc2` when running else `t.muted`) · `theme <name>` → switches theme (`theme → X` in `t.acc2`) or lists `themes: …` · anything else → `archonui: command not found: <first word>` in `t.acc`. All other output lines default to `t.muted`.
Switching project resets `term: null` (`L1630`) → the seed is replayed with the new prompt path.

See `tool-term.png`.

---

# B. BEHAVIOUR

## B1. Keyboard — *every* key handler in the source

`window.addEventListener('keydown', this.keyFn)` (`L1255`, handler `L1242-1254`):

| key | guard | action | source |
|---|---|---|---|
| `Escape` | `modal \|\| lightbox` open | close the lightbox if open, else the modal (share/settings) | `L1243` |
| `Alt+Enter` | — | `newSession()` | `L1246` |
| `Alt+q` / `Alt+Q` | — | `closeTile(focus)` | `L1247` |
| `Alt+t` / `Alt+T` | — | next theme in `NAMES` order (wraps) | `L1248` |
| `Alt+ArrowRight` / `Alt+ArrowDown` | — | focus := next tile in `tiles` (wraps) | `L1249-1252` |
| `Alt+ArrowLeft` / `Alt+ArrowUp` | — | focus := previous tile (wraps) | `L1249-1252` |
| any other key | `!e.altKey` → ignored | — | `L1245` |

Per-element handlers:
| element | key | action |
|---|---|---|
| composer `<textarea>` (`tl.onKey`, `L1530`) | `Shift+Tab` | cycle agent mode Auto → Plan → Ask (preventDefault) |
| composer `<textarea>` | `Enter` (no Shift, no Alt) | send (`send(id)`), preventDefault |
| composer `<textarea>` | `Shift+Enter` | newline (native) |
| composer `<textarea>` | `Alt+Enter` | bubbles to the window handler → new session |
| terminal `<input>` (`termKey`, `L1680`) | `Enter` | `runCmd(termIn)` |
| share-modal invite `<input>` (`inviteKey`, `L1648`) | `Enter` | add the typed person |

Settings → Keyboard page documents (static text, not implemented as handlers): **Alt ⏎** Split new session, **Alt Q** Close tile, **Alt ← →** Move focus between tiles, **⇧ Tab** Cycle agent mode, **Alt T** Next theme, **⏎ / ⇧⏎** Send / new line, **Esc** Close dialog (`KEYS`, `L1392`).

## B2. Tiling — exact placement rule (Dwindle, default)

`renderVals` `L1487-1503`:

```
n = tiles.length ;  g = 14 ;  rad = 9 ;  innerH = 1080 - 2g = 1052
cw  = rx - g - cx  = 1112   (rx = 1920 - rw = 1440, cx = lw + g = 314)
```
Dwindle (`L1497-1503`), when `layout !== 'Master'` or `n === 1`:

```
r  = {x:0, y:0, w:cw, h:innerH}         // remaining rectangle (includes gaps)
er = {w:cw, h:innerH}                   // remaining "region" used only for the aspect decision
for i in 0..n-1:
  if i == n-1: rects.push(r); break
  k = ratios[i] ?? 0.5                  // per-split ratio, default 0.5
  vert = er.w >= er.h * 0.9             // ← the rule: split left/right if remaining width ≥ 90% of remaining height
  if vert:  er = {(er.w-g)/2, er.h}; w = (r.w-g)*k
            rects.push({...r, w});      handle at x = r.x + w, vertical (col-resize)
            r = {x: r.x+w+g, y: r.y, w: r.w-g-w, h: r.h}
  else:     er = {er.w, (er.h-g)/2}; h = (r.h-g)*k
            rects.push({...r, h});      handle at y = r.y + h, horizontal (row-resize)
            r = {x: r.x, y: r.y+h+g, w: r.w, h: r.h-g-h}
```
Because `er` halves one axis (minus gaps) per iteration, the direction **alternates**: first split vertical, second horizontal, third vertical, … as long as the geometry keeps the test flipping (`w >= 0.9h`). Concretely, with the default 1112×1052 area:
* `er = 1112×1052` → `1112 ≥ 946.8` → **vertical** (`k = ratios[0] ?? 0.5`);
* `er = 549×1052` → `549 < 946.8` → **horizontal**;
* `er = 549×519` → `549 ≥ 467.1` → **vertical**;
* `er = 267×519` → `267 < 467.1` → **horizontal**;
* …

**Measured results** (page coords = CSS px; divide by `fitScale` 0.8333 to get design px — given in brackets):
* 3 tiles (default `tiles = ['s1','s2','s4']`): left `457×877` @(262,62) → design `548×1052`; top-right `457×433` @(731,62) → `548×519`; bottom-right `457×433` @(731,506) → `548×519`.
* 4 tiles (after `Alt+Enter`): left `548×1052`; right column top `548×519`; then the remaining bottom region splits **vertically** into two `268/223×519` tiles → `831×433` … measured `223×433` @(731,506) and @(966,506) → design `268×519` each.

Master layout (`layout === 'Master' && n > 1`, `L1489-1495`):
```
mw = round((cw - g) * masterR)        // masterR default 0.56  → round((1112-14)*0.56) = 615
rects[0] = {0, 0, mw, innerH}         // one big tile on the left
handle 'm' vertical at x = mw, draggable, reset → masterR = 0.56
sh2 = (innerH - g*(n-2)) / (n-1)      // right column tiles
rects[i] = {mw+g, (i-1)*(sh2+g), cw-mw-g, sh2}  for i = 1..n-1
```
Master is reachable only from Settings → Appearance → LAYOUT → Tiling (`Dwindle` | `Master`, `L1394`).

**New split placement:** `newSession()` (`L1271-1281`) does **not** split in-place; it appends the new id to `tiles` immediately *after the focused tile*:
```
if (tiles.length >= 6) tiles = tiles.filter(t => t !== focus)   // drop the focused tile at 6 tiles
fi = tiles.indexOf(focus); tiles.splice(fi < 0 ? tiles.length : fi + 1, 0, id); focus = id
```
The whole layout is then recomputed from `ratios`/`gaps` — there is no persistent split tree, so the aspect rule (not the tile’s own shape) decides whether the new tile lands left/right or above/below. Max 6 tiles.

**How closing collapses:** `closeTile(id)` (`L1292-1298`): remove the id from `tiles`; `focus = tiles[max(0, index-1)]` when the closed tile was focused (else focus unchanged); `pop = null`. The freed space is simply re-divided by the algorithm above — no “promote sibling” step. Closing the last tile sets `focus = null` and shows the empty state (`noTiles`, `L109-115`): 72×72 logo at `opacity:.8`, app name in the display font **56px** with `text-shadow:0 2px 24px rgba(0,0,0,.6)`, and a **New session** button (`padding:12px 22px; border-radius:8px; background:t.acc; color:t.on; font-weight:700`).

## B3. Focus rules

* `<section onMouseDown={tl.focus}>` (`L117`) — any mousedown inside a tile focuses it (runs even when clicking a button; the header `+`/`×` call `stopPropagation` before acting).
* Composer `<textarea onFocus={tl.focus}>` (`L248`) and both popovers (`toggleAtt`/`toggleModel` also set `focus: id`, `L1541-1542`).
* Focused tile visual: outer ring `background: t.border = linear-gradient(135deg,#d9b56a,#c81e2a 50%,#5a0a10)` (measured on the focused tile only); unfocused ring = `t.line2`.
* Focused tile background: `bg = bgs !== 'Canvas' ? BG.tile : (focused && empty ? rgba(tile,0.12) : (focused || tintAll) ? rgba(tile,0.82) : BG.tile)` and `blur = … 'blur(8px)'` when focused (`L1520`). Unfocused tiles are the wallpaper gradient (`linear-gradient(180deg, rgba(tile,0.25), rgba(tile,0.6) 45%, rgba(tile,0.96) 85%)`).
* Setting `tintAll` (“Tint all sessions”, Settings → Appearance → EFFECTS) makes every tile use the focused style.
* Tile status dot: `running → t.acc2`, `done → t.alt`, `idle → t.faint` (`dotFor`, `L1510`), 8×8 with a 2 px border in `t.tile` (`L121`).

## B4. Opening sessions

* **Left sidebar list row** → `openSession(id)` (`L1283-1291`):
  ```
  if (tiles.includes(id)) → focus = id  (no layout change)
  else → replace the focused tile's id with id  (or the last tile, or push when tiles is empty); focus = id
  ```
  Verified: with `tiles = ['s1','s2','s4']` and focus `s1`, clicking the sidebar entry “Release notes v0.9” (s3) produced tiles `['s3','s2','s4']` — *swapped into the focused tile*, layout untouched.
* **Collapsed-rail session button** (36×36, `title = name`, `L36`) → the same `openSession`.
* **Tile `+`** (title “Split new session”, 28×28) → `setState({focus:id}, this.newSession)` — a new tile next to this one (`L1527`).
* **No modifier-click, no double-click, no right-click / context menu anywhere**: `grep` finds `0` occurrences of `onContextMenu`, `contextmenu`, `metaKey`, `ctrlKey`; the only `shiftKey` is the composer’s `Shift+Tab`/`Enter` handler at `L1530`, and the only other `onDoubleClick`s are the resize handles (`L324`, `L514`) and tree rows (`L422`). A synthetic `contextmenu` + meta-`mousedown` on a sidebar row changed nothing (verified).
* **No session switcher / command palette** (no ⌘K, no tab-list UI). The only cross-tile navigation is `Alt+←/→/↑/↓`.
* Sidebar rows show: provider badge (`hi` = 2-letter initials of the harness, `title = "<harness> · <project>"`), relative time (`ago`; `yesterday` is displayed as `1d`), and, when the session is in a tile, are highlighted (`bg = t.soft` while focused, `bd = t.line2`); running sessions get a `halo` value that is **computed but never rendered** (`L1562-1563`).

## B5. Resize / drag interactions

`startDrag(e, h)` (`L1220-1235`):
```
e.preventDefault(); e.stopPropagation()
rect = mainRef.getBoundingClientRect(); fit = state.fit; g = state.gaps
px = (clientX - rect.left) / fit ;  py = (clientY - rect.top) / fit      // design-space coords
kind 'panel' → rwOpen = clamp(round(h.right - px), 360, 820); return
k = h.dir==='v' ? (px - h.p.x - g/2) / (h.p.w - g) : (py - h.p.y - g/2) / (h.p.h - g)
k = clamp(k, 0.12, 0.88)                       // hard limits on every split ratio
kind 'master' → masterR = k ; else ratios[h.i] = k
on mouseup: remove listeners, body cursor '', drag = null
body cursor = 'row-resize' for horizontal splits else 'col-resize'
```
Handle geometry (`L1504-1509`): `hw = max(g,12) = 14` at 14 px gaps; vertical handle `w=14, h=<split length>` with grip `3×44`; horizontal handle `w=<split length>, h=14` with grip `44×3`; grip colour `t.line2`, `t.acc2` while dragging (`transition: background .15s`); `title="Drag to resize · double-click to reset"`; `z-index:3`.
Double-click a handle → `ratios[i] = 0.5` (split) or `masterR = 0.56` (master) (`L1508`).
While `drag !== null` the tile transition is disabled: `tileTr = 'none'` instead of `left .32s cubic-bezier(.2,.8,.2,1),top …,width …,height …` (`L1651`).

**Panel edge** (`L513-515`): an absolutely positioned strip `left: rx-5; top:0; width:10px; height:1080px; cursor:col-resize; z-index:6; display:grid; place-items:center` with a `3×44` grip (`background:t.line2`, `t.acc2` while dragging); rendered only when the panel is open. Drag → `rwOpen = clamp(360..820)`; double-click → `rwOpen = 480` (`L1653`). The aside itself animates `width .3s,left .3s,background .3s` (`sideTr`, `L1652`).

## B6. New-session flow

1. Trigger: the big “New session” button (expanded sidebar, with the `Alt ⏎` hint), the collapsed-rail `+`, the empty-state “New session”, the tile-header `+`, or `Alt+Enter`.
2. `newSession()` creates `{ id: 's'+Date.now(), icon: <first unused sigil index>, name: 'New session', project: state.project, model: state.defModel ('opus'), effort: lv[effIdx(lv, state.defEffort)] ('Medium'), mode: state.defMode ('Auto'), status: 'idle', when: 'today', ago: 'now', msgs: [] }` and prepends it to `sessions`.
3. Tile placement as §B2 (max 6); `leftTab` unchanged; `pop = null`.
4. On first send, the session is renamed to the first 38 characters of the message (`text.slice(0,38)`, `L1304`) if it had no messages.
5. Sending (`send`, `L1299-1320`) appends the user message plus a placeholder agent message (`meta:'working'`, `thinking:true`), clears the draft/attachments, then streams 1–4 fake tool calls (`POOL.slice(0, max(1, round(fr*4)))`) at `500 + i*450 ms`, then completes at `700 + n*450 ms` with `meta: 'thought for <think>s'` (`think = round(2 + fr*29)`), a canned `REPLIES[]`/`PLAN_REPLY`/`ASK_REPLY` text and `status:'idle'`; an `edit` tool call also appends `src/agent/loop.ts` (`M`, +12 −3, unstaged) to the Git changes list. `Plan`/`Ask` modes use read-only tools and prefix `meta` with ` · plan mode` / ` · ask mode`.
6. Send button while `status === 'running'` becomes **Stop** (`sendLbl`, `L1536`): it stops the run and rewrites the last thinking message to `meta:'stopped'`, text `'Stopped.'`.

## B7. Search

`state.search` (`L1191`) ← the sidebar `<input placeholder="Search sessions">` (`L63`); filtering (`L1560-1567`):
```
q = search.toLowerCase()
list = sessions.filter(x => !q || x.name.toLowerCase().includes(q) || x.project.includes(q))
```
Sessions are then grouped `TODAY` (`when === 'today'`) / `EARLIER`, and empty groups are dropped. Consequences to preserve: matching is by **session name or project name** only (never message text); the filter is case-insensitive on the name and effectively case-insensitive on the project (project names are lower-case); group labels use `font-size:10.5px; letter-spacing:.16em; color:t.muted; padding:8px 4px 4px`. **There is no “no results” state** — if nothing matches, both groups vanish and the list is empty.

## B8. Projects

* Only the **Projects** left tab renders them (`L80-95`); the tab bar is the 2-column segmented control `Sessions | Projects` (`L50-55`, active `background:t.line2; color:t.fg`).
* Card (`L82-92`): `cursor:pointer; display:flex; flex-direction:column; gap:8px; padding:14px; border-radius:9px; border:1px solid <bd>; background:<bg>` — `bd = t.acc` and `bg = t.soft` for the current project, else `t.line2`/transparent. Contents: name (display font 20px) + session count (`"<n> sessions"`, 11px `t.muted`); path (12px `t.muted`); row of `⎇ <branch>` chip (`padding:3px 8px; border-radius:5px; background:t.line2; color:t.alt`) and `● <lang>` (`color` per project: archonui-core `t.alt`, archonui-web `t.acc`, archonui-cli `t.acc2`, ember-gl `t.muted`).
* **Click a card** → `setState({project: name, term: null})` + toast `Switched to <name>` (`L1630`). This changes the Files header/path, the Git branch chip + log branch, the terminal prompt, and the default project for new sessions.
* **`+ Open folder`** (`L94`, dashed border, 13px `t.muted`, `padding:11px`) → `flash('Folder picker opens here')` — a stub.
* **Create / rename / delete project: none exists.** No project rename, no delete, no add-project form, no folder picker. The only destructive data action in the whole prototype is Settings → General → DATA → **“Clear session history”** (`RB(...)`, `L1389`) which removes every session not currently tiled.

## B9. Context menus

**None.** There are no `onContextMenu` handlers and no custom menu component anywhere in the file; right-click is completely unhandled (browser default is suppressed only by `user-select:none` on the app root). The only popovers are the composer’s attach menu (`tl.attOpen`) and the model/effort menu (`tl.modelOpen`), both anchored `bottom: calc(100% + 6px)` with `width:360px` (model) / `300px` (attach), `background:t.pop`, `border:1px solid t.line2`, `box-shadow:0 14px 40px rgba(0,0,0,.45)`, `z-index:5`, `padding:6px`.

## B10. Persistence

**Nothing is persisted.** `grep` for `localStorage` / `sessionStorage` / `indexedDB` / cookies returns 0 hits; there is no persistence layer at all. Everything lives in the component’s `state` (`L1189-1207`) and is reset on reload: theme, layout mode, gaps, radius, left/right tab + open state, right width (`rwOpen`), blur, dim, font size, custom colours, uploaded wallpapers/icons, app name, providers/harness install flags, enabled models, default model/effort/mode, max steps, permissions, MCP/skills/extensions toggles, project, open files, tree `openDirs`, selected file, session list, drafts, attachments, changes/staging, commit message, `ahead`, log, url, share visibility/people, every settings toggle. `fitScale` is recomputed on `resize` and via a `ResizeObserver` on `<html>` (`L1238-1241`).

Note the *only* durable-looking thing the UI hints at is `shareLink = 'archonui.dev/s/' + id.slice(-4) + '-' + name-slug` (`L1642`), which is derived, not stored.

## B11. Other behaviour worth porting

* **Auto-scroll**: `componentDidUpdate` watches a signature of tile message counts/lengths + terminal line count and, when it changes, `scrollAll()` sets `scrollTop = scrollHeight` on every `[data-msgs]` and `[data-term]` element on the next frame (`L1259-1265`).
* **Toasts**: `flash(msg)` shows one toast for **1800 ms** (`L1268`); markup at `L876-878`: `position:absolute; left:50%; bottom:36px; transform:translateX(-50%); padding:11px 18px; border-radius:8px; background:t.pop; border:1px solid t.line2; font-size:13px; box-shadow:0 10px 30px rgba(0,0,0,.4)`. Non-interactive.
* **Lightbox**: overlay `rgba(0,0,0,0.85)`, `z-index:30`, `cursor:zoom-out`, `1440×810` image box `border-radius:10px`, caption `13px` + `t.muted`; **any click closes** (`L870-874`).
* **Window controls** are fake: `winAct: () => this.flash('Window controls are handled by the OS')` (`L1625`) for Minimize/Maximize/Close on both rails.
* Modal shells: share modal `width:520px; max-height:820px; border-radius:12px; background:t.pop; border:1px solid t.line2; box-shadow:0 30px 80px rgba(0,0,0,0.6)`, backdrop `rgba(0,0,0,0.6)`, `align-items:flex-start; padding-top:120px`; settings modal `1320×880; border-radius:14px; box-shadow:0 40px 120px rgba(0,0,0,0.65)`, backdrop `rgba(0,0,0,0.45)`, `place-items:center`, left nav `236px` with `background:t.line`. Backdrop click closes (`closeModal`), inner click calls `stop` (`e.stopPropagation()`).
* **Computed-but-never-rendered** values (do **not** build UI for them): `themeGrid`, `fontOpts`, `layoutOpts`, `gapOpts`, `looks`, `toggles`, `bgOpts`, `tileCount`, `effLbl`, `effBars`, the sidebar session `tag`/`halo`/`meta`, and `rs.ring` (`L1562`, `L1618`, `L1625`, `L1633-1637`).

---

# C. Interactive-control inventory (“no dead UI” checklist)

Format: **where — label — effect**. `[LIVE]` = changes state visibly; `[DEAD]` = painted as a control but does nothing (static, or a stub that only toasts); `[n/a]` = not a control at all (label / info row).

### Left sidebar — collapsed rail (52 px, `L30-42`)
1. rail — Expand sidebar (`›`, title "Expand sidebar") — `leftOpen = true`. `[LIVE]`
2. rail — New session (`+`, title "New session") — `newSession()`. `[LIVE]`
3. rail — session buttons 36×36 (one per session, `title` = session name, sigil icon + 8 px status dot) — `openSession(id)`. `[LIVE]`
4. rail — Share (title "Share") — opens the share modal. `[LIVE]`
5. rail — Settings (title "Settings") — opens the settings modal. `[LIVE]`

### Left sidebar — expanded (300 px, `L45-103`)
6. header — Collapse sidebar (`‹`) — `leftOpen = false`. `[LIVE]`
7. header — logo + app name — label only, not a control. `[n/a]`
8. segmented — **Sessions** — `leftTab = 'Sessions'`. `[LIVE]`
9. segmented — **Projects** — `leftTab = 'Projects'`. `[LIVE]`
10. Sessions tab — **New session** button (hint “Alt ⏎”) — `newSession()`. `[LIVE]`
11. Sessions tab — **Search sessions** input — filters the list (§B7). `[LIVE]`
12. Sessions tab — session rows (icon+badge+name+harness badge+time) — `openSession`. `[LIVE]`
13. Projects tab — project cards (name, count, path, branch chip, language dot) — switch project + toast. `[LIVE]`
14. Projects tab — **+ Open folder** — toast “Folder picker opens here”. `[DEAD]`
15. footer — **Settings** (icon + label, bg highlights while the modal is open) — opens settings. `[LIVE]`
16. footer — Share icon (title "Share") — opens share. `[LIVE]`

### Tiles (`L117-312`)
17. tile body — (no label) mousedown anywhere — focuses the tile. `[LIVE]`
18. tile header — **+** (title "Split new session") — focus this tile + new session in a new split. `[LIVE]`
19. tile header — **×** (title "Close tile") — `closeTile`. `[LIVE]`
20. agent tool-call rows — info rows (icon, kind, target, diff/result), not controls. `[n/a]`
21. chart card — **Copy CSV** — clipboard `label,value` lines + toast “Copied CSV”. `[LIVE]`
22. code block — **Copy** — clipboard body + toast `Copied <file>`. `[LIVE]`
23. image figure — image area (cursor `zoom-in`) — opens the lightbox. `[LIVE]`
24. image figure — **Open** — opens the lightbox. `[LIVE]`
25. image figure — **Attach** — adds the image to the composer attachments + toast. `[LIVE]`
26. user message — attachment chips — info only (no remove affordance). `[n/a]`
27. composer — attachment chip **×** (title "Remove") — removes the attachment. `[LIVE]`
28. composer — textarea (placeholder depends on mode: `Message archonui… (⏎ to send, ⇧⏎ newline, ⇧Tab mode)` / `Describe the change. archonui will plan before touching files…` / `Ask about the code. Nothing will be changed…`) — draft, send, mode cycle, focus. `[LIVE]`
29. composer — **attach** button (title "Attach files", paperclip, 32×32) — toggles the attach popover. `[LIVE]`
30. attach popover — 7 rows (first 4 repo files, `browser screenshot.png`, `terminal output.log`, `Upload from disk…`) — adds an attachment, closes the popover. `[LIVE]`
31. attach popover — header `ATTACH` — label only. `[n/a]`
32. composer — **model chip** (`▾`, shows mode icon + model name + effort moon + effort label) — toggles the model popover. `[LIVE]`
33. model popover — `MODE` header — label only. `[n/a]`
34. model popover — mode cards **Auto / Plan / Ask** (each with `sub`: “Edits and runs”, “Steps first”, “Read-only”; title tooltips add “⇧Tab to cycle”) — sets the mode. `[LIVE]`
35. model popover — `MODEL` header — label only. `[n/a]`
36. model popover — model rows (name + `harness · ctx`, ✓ on the active one; only models whose provider + model toggles are on, plus the session’s current model) — sets the model and keeps the nearest effort level. `[LIVE]`
37. model popover — effort track + moon buttons (one per level, `title` = level name) — sets the effort. `[LIVE]`
38. model popover — `EFFORT · N LEVELS` header + the level-name row under the track — labels only. `[n/a]`
39. composer — **send** button (title "Send"/"Stop") — sends, or stops when running. `[LIVE]`

### Tiling (`L324`, `L513-515`)
40. split handles (between tiles) — drag to resize (clamped 0.12–0.88), double-click resets to 0.5 / master 0.56. `[LIVE]`
41. right-panel edge (10 px strip at `rx-5`) — drag to resize 360–820, double-click resets 480. `[LIVE]`

### Right tools panel (`L329-510`)
42. header — Collapse chevron (title "Collapse") — `rightOpen = false`. `[LIVE]`
43. header — tabs **Browser / IDE / Files / Git / Terminal** — `rightTab` + force-open. `[LIVE]`
44. rail — 5 tab icon buttons (same as #43, with `title` = tab name). `[LIVE]`
45. header/rail — Minimize, Maximize, Close (title "Minimize"/"Maximize"/"Close") — toast “Window controls are handled by the OS”. `[DEAD]`
46. Browser — back/forward/reload glyph string `‹ › ↻` — static text. `[DEAD]`
47. Browser — URL input — editable, updates `state.url` only (nothing consumes it). `[LIVE (state only)]`
48. Browser — mock page: nav links `Docs`/`Sessions`/`Login`, the `email` and `password` boxes, the `Sign in` bar (all look interactive) — static. `[DEAD]`
49. IDE — file tabs (basename) — switch the active file (breadcrumb, code, language, line count all follow). `[LIVE]`
50. IDE — file tab **×** — closes that file tab (keeps ≥1 tab). `[LIVE]`
51. IDE — code area — text viewer (selectable), not a control. `[n/a]`
52. Files — **New file** (title "New file") — creates `src/untitled-N.ts` (empty, added to the tree, selected, staged as `A` in Git). `[LIVE]`
53. Files — **New folder** (title "New folder") — creates `new-folder-N/` (empty, expanded) + toast `Created new-folder-N/`. `[LIVE]`
54. Files — **Collapse all** (title "Collapse all") — `openDirs = {}`. `[LIVE]`
55. Files — **Filter files** input — filters the tree to a flat matching list (or “No files match.”). `[LIVE]`
56. Files — file-count label — label only. `[n/a]`
57. Files — tree row (single click) — directory: expand/collapse; file: select (drives the footer + `t.soft` row highlight). `[LIVE]`
58. Files — tree row (double click) — file: open in the IDE (adds a tab, switches `rightTab='IDE'`). `[LIVE]`
59. Files — selection footer **Open** — open the file in the IDE. `[LIVE]`
60. Files — selection footer **Attach** — attach the file path to the focused session + toast. `[LIVE]`
61. Files — selection footer **Copy path** — clipboard `<projPath>/<file>` + toast “Copied path”. `[LIVE]`
62. Git — **Push** — `ahead = 0` + toast `Pushed to origin/<branch>` (no real push, no-op when already 0). `[LIVE (cosmetic)]`
63. Git — change row (whole row is the hit area) — toggles staged/unstaged (checkbox, `stagedLbl`, commit-button opacity). `[LIVE]`
64. Git — commit textarea — commit message. `[LIVE]`
65. Git — **Commit** — when ≥1 staged **and** the message is non-empty: prepends `{h: randomHex7, m: message, t:'now'}` to HISTORY, removes the staged changes, clears the message, `ahead += 1`, toast `Committed <hash>`. Otherwise it silently does nothing (only dimmed to `opacity:.4`). `[LIVE]`
66. Git — history rows — info rows; no click, no commit detail. `[n/a]`
67. Terminal — output area click — focuses the prompt input. `[LIVE]`
68. Terminal — prompt input — Enter runs a command (§A5). `[LIVE]`

### Modals / overlays (`L516-878`)
69. Share modal — backdrop click — close. `[LIVE]`
70. Share modal — **×** (title "Close") — close. `[LIVE]`
71. Share modal — visibility segmented **Private / Team / Public** — sets `vis` + the note text. `[LIVE]`
72. Share modal — **Copy**/**Copied** — clipboard share link, label flips for 1500 ms. `[LIVE]`
73. Share modal — toggles **Include file diffs** / **Include terminal output** — flip state (not reflected anywhere else). `[LIVE (state only)]`
74. Share modal — **Invite** + invite input (`name@team.dev`, Enter also submits) — appends a row to PEOPLE (“Can view”), clears the input, toast `Invite sent to …`; a blank value no-ops. `[LIVE]`
75. Share modal — **Markdown** / **JSON** — toast `Exported <session>.md` / `Exported session.json`. `[DEAD]`
76. Share modal — PEOPLE rows (avatar initials, name, email, role) — info rows. `[n/a]`
77. Settings modal — backdrop click / **×** (title "Close") — close. `[LIVE]`
78. Settings modal — nav items **General, Appearance, Keyboard, Harnesses, Models, Tools, Skills, Extensions** (plus the static heads APP / AI / CAPABILITIES and the footer “archonui 0.9.0 · Stable”) — switch page. `[LIVE]`
79. Settings · General — toggles “Restore sessions on launch”, “Run commands in sandbox”, “Ask before network access”, “Show tool calls in chat”, “Notify when a session finishes”, “Play a sound”, “Share anonymous usage data”; segment “Default project” (4 projects), “Default mode” (Auto/Plan/Ask), “Update channel” (Stable/Beta/Nightly); slider “Max steps per turn” (10–100 step 5); buttons **Export** (toast) `[DEAD]` and **Clear history** (really removes non-tiled sessions). `[LIVE]`
80. Settings · Appearance — app-name input (max 18 chars, live in the sidebar/empty state); **Use theme icon**; 12+N icon swatches + `+` upload; 12 preset cards (also reset fonts + wallpaper); 6 background cards; wallpaper grid + `+ Upload`; 12 colour pickers each with an invisible `<input type="color">`; **Reset to preset** (`opacity:.4` when nothing is edited); 3 font pairings; 6 “Minimal” pairings; 17 display fonts; 12 interface fonts; (bottom, `genBot`) tiling segment, **Gaps** 0–32 px, **Corner radius** 0–20 px, **Reset sizes**, **Panel blur** 0–48 px, **Wallpaper dim** 0–80 %, “Tint all sessions”, “Message text size” 13–19 px. `[LIVE]`
81. Settings · Keyboard — 7 keycap rows — documentation only, not controls. `[n/a]`
82. Settings · Harnesses — per provider **Test** (toast `"<name> <ver> · handshake OK"`), **Enable/Disable/Install** (flips `provOn`, marks installed, toast); **+ Add harness (any CLI that speaks ACP)** (toast only) `[DEAD]`. `[LIVE]`
83. Settings · Models — radio “Set as default”, model on/off toggles (blocked with a toast when the provider is off); “Default effort” segment; “Use a fast model for titles”, “Prefer long-context variants”. `[LIVE]`
84. Settings · Tools — 7 MCP rows with on/off toggles, **Reconnect** (Sentry, appears only when errored), **+ Add server** (toast only) `[DEAD]`; 7 built-in permission segments (Allow/Ask/Off). `[LIVE]`
85. Settings · Skills — 10 skill cards with toggles, **Import folder** (toast) `[DEAD]`, **+ New skill** (toast `Opens a new SKILL.md in the IDE panel`) `[DEAD]`. `[LIVE]`
86. Settings · Extensions — search input (filters installed + browse), installed rows with a toggle + **Remove**, featured cards with **Install** (moves into installed, enables it). `[LIVE]`
87. Lightbox — click anywhere — close. `[LIVE]`
88. Toast — text only, non-interactive, auto-hides after 1800 ms. `[n/a]`

*(Items 69–86 belong to the Share/Settings worker’s detail doc; they are listed here only so the checklist is complete.)*

Keyboard controls (all `[LIVE]`): `Alt+Enter`, `Alt+Q`, `Alt+T`, `Alt+←/→/↑/↓`, `Shift+Tab` (composer), `Enter`/`Shift+Enter` (composer), `Enter` (terminal), `Enter` (invite), `Escape` (modal/lightbox).

---

# D. Ambiguities / gaps to resolve before porting

1. **Git diff view does not exist** in the prototype although the brief mentions “diff view colours”. The only diff-ish visuals are the IDE `t.soft` line band (hard-coded to lines 9–16 of `session.ts`), the `+N/−N` counters, and the Files `modified · +28 −8` label. Decide whether the Rust port invents a diff view or copies the prototype exactly.
2. **No project create/rename/delete** and **no context menus at all** — if the port needs them they are net-new; there is no prototype reference.
3. **No persistence** — nothing survives reload; if the Rust app persists (theme, layout, sizes…), the key/value map must be designed from scratch (the prototype has no key names).
4. **Dead-but-painted controls** (the ones marked `[DEAD]` in §C): #14 `+ Open folder`, #45 Minimize/Maximize/Close, #46 the browser back/forward/reload glyphs, #48 the whole mock web page (nav links, email/password fields, Sign in), #75 Markdown/JSON export, and the settings stubs — Export settings, `+ Add harness`, `+ Add server`, `Import folder`, `+ New skill`. They are styled as buttons/inputs but only toast (or do nothing). Either implement or drop; do not ship inert.
5. The tile **status dot colours** and the **sidebar running “halo”** are computed; only the dot is rendered (the halo is not).
6. `alt` on the terminal seed prompt vs `acc2` on the live prompt: the first line’s prompt is `t.fg` while the interactive prompt is `t.acc2` — intentional in the source, easy to “fix” wrongly.
7. Measured screenshots are the implementation check: re-render at 1600×1000 and compare against `.qa/design/tool-*.png` after dividing by `fitScale`.

---

### Report

Extracted the full right tools panel (5 tabs) with exact inline styles + measured values (`rgba(176,20,30,0.32)` red band on IDE lines 9–16, status bar `⎇ feat/session-cache / TypeScript / 23 lines / UTF-8` with 14 px gaps and no literal separators, Git row/checkbox colours, terminal seed colours and command list), screenshotted every tab to `.qa/design/tool-<tab>.png`, and documented all behaviour: 6 global key rows + 3 local key handlers, the exact Dwindle placement rule (`vert = er.w >= er.h*0.9`, alternating, new tile inserted after the focused tile, max 6), Master layout, close/focus/swap rules, resize clamps (0.12–0.88, panel 360–820), new-session flow, search, and the fact that there is **no** context menu, modifier-click, session switcher, project rename/delete, diff view, or persistence. Ambiguities are listed in §D — the main one is that the brief asks for a Git “diff view” that the prototype does not have.
