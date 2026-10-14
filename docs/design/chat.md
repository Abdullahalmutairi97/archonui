# SESSION / CHAT VIEW — extracted spec

Source of truth: `design/ArchonUI Agent v3.dc.html` (1689 lines). All line refs (`L###`) point at that file.
Every value below is copied verbatim from the prototype, or computed with the prototype's own formula (formula + computed values given).
Browser-verified values (Playwright, Chrome, 1600×1000 viewport) are marked **[measured]**.

Screenshots: `.qa/design/chat-*.png` (see §11 index).

---

## 0. Conventions you need before reading the numbers

### 0.1 Authoring scale
The prototype is a fixed **1920×1080** canvas scaled to the window:

```
fitScale = min(innerWidth / 1920, innerHeight / 1080)        // L1238 (fitFn; mounted L1237)
wrapper: position:fixed; left:50%; top:50%; width:1920px; height:1080px;
         transform: translate(-50%,-50%) scale(fitScale)      // L25
```

At the 1600×1000 test viewport `fitScale = 0.833333`, so **every screenshot is 0.8333× the layout px**.
All px values in this document are **layout px (scale 1)**, as authored in the HTML.

### 0.2 Theme tokens used in the chat view
`t` = theme object built at L1463–L1468 (`TUNE` L1463, `t0` L1467, `t` L1468). Default theme is **Blood Moon**
(prop default, L881), with a Blood-Moon-specific tuning override in `TUNE` (L1463–L1466).

| token | Blood Moon value | use in chat view | measured |
|---|---|---|---|
| `t.bg` | `#0d0a08` | app background, stop-button fg | `rgb(13,10,8)` |
| `t.pop` | `#1b1411` (TUNE) | composer wrapper bg, popovers, toast, moon buttons | `rgb(27,20,17)` |
| `t.fg` | `#f3ead8` (TUNE) | body text, stop button bg | `rgb(243,234,216)` |
| `t.muted` | `#bfae94` (TUNE) | meta lines, section labels, counts | `rgb(191,174,148)` |
| `t.faint` | `#6a5a48` (TUNE) | code line numbers, un-reached moons | `rgb(106,90,72)` |
| `t.acc` | `#c81e2a` (TUNE) | send button, keywords, selected mode bg, fill gradient start | `rgb(200,30,42)` |
| `t.acc2` | `#d9b56a` | selected model check, tool icon, effort fill/glow, lang badge | `rgb(217,181,106)` |
| `t.alt` | `#c9a77a` | diff counts/tool result column, number literals | `rgb(201,167,122)` |
| `t.on` | `#f6ecdc` | text on acc/acc2 filled surfaces | `rgb(246,236,220)` |
| `t.soft` | `rgba(176,20,30,0.32)` | user bubble, selected popover row | `rgba(176,20,30,0.32)` |
| `t.line` | `rgba(fg,0.09)` — **computed** | tool rows bg, attachment badge bg, dividers | `rgba(243,234,216,0.09)` |
| `t.line2` | `rgba(fg,0.22)` — **computed** | tool-group border, card borders, popover border | `rgba(243,234,216,0.22)` |
| `t.deep` | `#5a0a10` | tile ring gradient end | — |
| `codeBg` | `rgba(t.bg,0.72)` — L1617 | code / chart / image frame background | `rgba(13,10,8,0.72)` |

`line`/`line2`/`soft` are derived, not literal: `line = rgba(t0.fg, 0.09)`, `line2 = rgba(t0.fg, 0.22)`,
`soft` = `rgba(t0.acc,0.28)` when the user overrides the accent else the theme value — all three on **L1468**.

### 0.3 Fonts
```
t.ui      = user UI font || fonts[fi].u     // FONTS['Blood Moon'][0].u
t.display = user display font || fonts[fi].d
t.code    = /mono|code|monospace/i.test(t.ui) ? t.ui : "'Geist Mono', monospace"   // L1478
```
Default (Blood Moon, `fontIdx=0`; `FONTS` L900, Blood Moon entry L902): **`ui` = `'JetBrains Mono', monospace`**,
**`display` = `'UnifrakturCook', serif`**, **`code` = `'JetBrains Mono', monospace`** (ui font is mono → reused) **[measured on the code body, meta lines and headings]**.

Root font size is the user setting `fs`, default **15px** (L1199 `fs: 15`, setting label "Message text size" L1396).
Everything that does not set an explicit `font-size` inherits 15px — including `MODE`/`MODEL`/`EFFORT` popover
labels' parent buttons (their child spans set 12.5px/10.5px/11px explicitly).

### 0.4 Scrollbars
`::-webkit-scrollbar{width:6px;height:6px}`, thumb `rgba(128,128,128,.35)` radius 3px, track transparent (L22). Applies to the message list, the model list and the settings panes.

### 0.5 Tile frame (context for the chat view)
Session tiles are rendered at L116–L117; header at L120:
`display:flex;align-items:center;gap:10px;padding:12px 12px 12px 16px;border-bottom:1px solid t.line`.
Header children: 20×20 sigil glyph (colour `acc2` when focused) with an 8×8 status dot
(`running → acc2`, `done → alt`, `idle → faint`, `dotFor` L1510; rail copy L1654), a column with
`name` (`display` font **20px**, line-height 1.1, ellipsis) and `meta` (**11.5px**, `t.muted`, ellipsis),
then two 28×28 `t.muted` buttons: `+` (`title="Split new session"`) and `×` (`title="Close tile"`).

`meta` format (L1521):
```
${project} · ${model.s} · ${effort.toLowerCase()} effort · ${mode.toLowerCase()} mode · running
```
Clause rules: empty-effort models drop the effort clause; the "mode" clause appears only when mode ≠ Auto;
"· running" only while `status === 'running'`. E.g. `archonui-core · Opus 4.5 · high effort`
(measured), `ember-gl · Opus 4.5 · max effort`, `archonui-web · Sonnet 4.5 · medium effort · ask mode · running`,
`archonui-core · llama 70B` (effort hidden), `archonui-core · GPT-5 · high effort · plan mode` (measured after plan selection).

---

## 1. Message stream

### 1.1 Container — L130
```html
<div data-msgs="{{tl.id}}"
     style="flex:1;overflow:auto;min-height:0;padding:18px 18px 8px;
            display:flex;flex-direction:column;gap:16px;user-select:text">
```
| property | value |
|---|---|
| padding | `18px 18px 8px` |
| gap between messages | `16px` |
| axis | column, items stretch (user message self-aligns right) |
| overflow | `auto`, min-height 0 (the tile is the scroll owner) |
| text selection | enabled here (`user-select:text`; root has `user-select:none`, L25) |
| auto-scroll | `scrollAll()` (L1263) sets `scrollTop = scrollHeight` on every `[data-msgs]` in a rAF after a send |

### 1.2 Empty state (session with no messages) — L131–L137
Centred column, gap 12px, `text-align:center`:
* logo 44×44, `opacity:.55`
* `What should {{appName}} work on?` — `display` font **28px**, `text-wrap:balance`
* `in {{tl.project}} · {{tl.model}}` — **12px**, `t.muted` (model is the **full** name, `model.n`)

### 1.3 User message — L139–L153
```
wrapper:  align-self:flex-end; max-width:82%; display:flex; flex-direction:column;
          align-items:flex-end; gap:6px                        (L140)
  atts row (only when m.hasAtts): display:flex; flex-wrap:wrap; gap:6px; justify-content:flex-end (L142)
  bubble: padding:11px 14px; border-radius:10px 10px 3px 10px; background:t.soft;
          line-height:1.55; white-space:pre-wrap; text-wrap:pretty   (L152)
```
**[measured]** bubble: bg `rgba(176,20,30,0.32)`, colour `t.fg`, padding `11px 14px`,
radius `10px 10px 3px` (4th corner = 2nd), line-height 23.25px (1.55 × 15), white-space `pre-wrap`,
`text-wrap: pretty`, font 15px `JetBrains Mono`, max body width 82% of the message list.
The 3px corner is the bottom-right (asymmetric bubble tail toward the right edge).

### 1.4 Agent message — L155–L232
```
block: display:flex; flex-direction:column; gap:8px; max-width:94%       (L156)
```
Children in order:
1. **Header line** (L157–L160): `display:flex;align-items:center;gap:8px;font-size:11px;color:t.muted`
   * logo 16×16 (`assets/logo-bm.png` for Blood Moon), `image-rendering:auto` (pixelated themes use `pixelated`)
   * `{{appNameUp}}` = app name uppercased → **`ARCHONUI`** with `letter-spacing:.12em` (**1.32px** at 11px) **[measured]**
   * `{{m.meta}}` — plain 11px, `t.muted`, letter-spacing normal. Values in the seed data: `thought for 14s`, `thought for 6s`, `thought for 2s`, `thought for 31s`, `thought for 19s`, `thought for 24s`, or **`working`** while streaming. Non-Auto replies append `· plan mode` / `· ask mode` (L1314). Stopped replies show **`stopped`** (L1535).
2. **Tool-call group** (L161–L173) — only when the setting `opts.showTools` is on (Settings → General "Show tool calls in chat", L1397; toggle `showTools` L1620) and `m.tools.length > 0`.
3. **Thinking label** (L175–L177) — only while `m.thinking`:
   `<span style="font-size:13px;color:t.muted;font-style:italic">{{m.thinkLbl}}</span>`
   `thinkLbl = 'Working · ' + tools.length + ' tool calls'` (L1523) — e.g. `Working · 2 tool calls`
   (**[measured]** `Working · 1 tool calls` — the label is not pluralised in the source; keep it verbatim).
4. **Parts** (L178–L231): array built by `parts()` (L1342) from the reply text; `txt`, `chart`, `img`, `code` variants. Text part (L179):
   `line-height:1.6; white-space:pre-wrap; text-wrap:pretty` (**[measured]** 24px at 15px).

### 1.5 Tool-call rows — L161–L174
```
group: display:flex; flex-direction:column; gap:1px; border-radius:8px;
       overflow:hidden; border:1px solid t.line2                        (L163)
row:   display:flex; align-items:center; gap:10px; padding:7px 11px;
       font-size:12px; background:t.line                                  (L165)
```
Row columns, in DOM order:

| # | source | style | example |
|---|---|---|---|
| 1 | `{{tc.ic}}` | `width:14px;text-align:center;color:t.acc2` | `◇` |
| 2 | `{{tc.k}}` | `color:t.muted;width:38px` | `read` |
| 3 | `{{tc.x}}` | `flex:1;min-width:0;white-space:nowrap;overflow:hidden;text-overflow:ellipsis` | `src/server/session.ts` |
| 4 | `{{tc.d}}` | `color:t.alt` | `+24 −8` |

Icon map `TOOL_IC` (L1125) — verbatim glyphs:
`read: '◇'`, `grep: '⌕'`, `edit: '✎'`, `run: '▸'`.
(The count column uses U+2212 MINUS SIGN `−`, reproduced from the source diff strings.)

**[measured]** group border `1px solid rgba(243,234,216,0.22)`, radius 8px, column gap 1px;
row height **30px** (25px on the 0.8333-scaled screenshot), padding `7px 11px`, font 12px,
bg `rgba(243,234,216,0.09)`, gap 10px;
icon colour `rgb(217,181,106)`, kind `rgb(191,174,148)` in a fixed 38px column, path `t.fg`,
result `rgb(201,167,122)` (no explicit alignment — it is pushed to the right edge by the
`flex:1` path column).

Seed examples (L1128–L1154) — the whole vocabulary used by the prototype:
```
◇ read  src/server/session.ts
⌕ grep  "verify(" in src/
✎ edit  src/server/session.ts      +24 −8
✎ edit  src/server/cache.ts        +41
▸ run   pnpm test                  128 passed
▸ run   pnpm e2e upload.spec.ts --repeat 20   3 failed
▸ run   git log v0.8..HEAD --oneline           23 commits
▸ run   cargo run -- --help-all
▸ run   node --heap-prof dist/server.js
✎ edit  tests/session.test.ts      +18 −2
```
The 4th column is empty for rows without a result (`d` absent → `''`).
There is **no collapse/expand control and no chevron**; the "collapsed" appearance is simply the
bordered 1px-gap group. The only "collapsed summary" in the prototype is the
`Working · N tool calls` italic line, which is the *placeholder while streaming*, not a collapsed group.

### 1.6 Code block (fence type `` ```<lang> <path> ``) — L215–L227
```
frame:  border-radius:8px; border:1px solid t.line2; background:codeBg; overflow:hidden   (L216)
header: display:flex; align-items:center; gap:10px; padding:5px 6px 5px 12px;
        border-bottom:1px solid t.line; font-size:11px; color:t.muted                     (L217)
   lang {L218a}: letter-spacing:.12em; text-transform:uppercase; color:t.acc2
   path {L218b}: flex:1; min-width:0; nowrap; ellipsis
   Copy {L219}: padding:4px 9px; border-radius:5px; hover → background:t.line2; color:t.fg
body:  padding:10px 0; overflow:auto; font-family:t.code; font-size:12.5px; line-height:1.65  (L221)
line {L223}: display:flex; gap:12px; white-space:pre; padding-right:12px
   number: <span style="width:26px;text-align:right;color:t.faint;flex:none">   (inside L223)
   segments: one <span> per token-coloured run
```
**[measured]** frame radius 8px, border `rgba(243,234,216,0.22)`, bg `rgba(13,10,8,0.72)`;
header padding `5px 6px 5px 12px`, 11px, `t.muted`, bottom border `t.line`;
lang `TS`/`ts` → letter-spacing 1.32px, uppercase, `rgb(217,181,106)`;
Copy button padding `4px 9px`, radius 5px, 11px, `t.muted`;
body font `JetBrains Mono` 12.5px, line-height 20.625px (1.65), padding `10px 0`, 5 lines for the sample;
line gap 12px, white-space `pre`, number width 26px right-aligned `rgb(106,90,72)`.

**Syntax colours** come from `tokenize(line, t)` (L1110–L1124), a single regex over the line:

| token class | regex branch | colour | example **[measured]** |
|---|---|---|---|
| comments `//…` / line-start `#…` | `/\/\/.*$` or `^#.*$` | `t.muted` | — |
| strings `"…" '…' \`…\`` | `"[^"]*"`, `'[^']*'`, `` \`[^\`]*\` `` | `t.acc2` | `"…/cache"` |
| numbers | `\b\d+\b` | `t.alt` | `300`, `128` |
| keyword | in `KW` set (L1108) | `t.acc` | `const` → `rgb(200,30,42)` |
| literal | in `LIT` set (L1109): `null undefined true false this` | `t.alt` | `this` → `rgb(201,167,122)` |
| identifier starting uppercase | `/^[A-Z]/` | `t.acc2` | `TokenCache`, `Map` |
| other identifier | — | `t.fg` | `key`, `get` |
| punctuation `{ } ( ) [ ] ; , . : = < >` | `[{}()\[\];,.:=<>]` | `t.muted` | `(` `)` `;` |
| whitespace | `\s+` | `t.fg` (inherits) | — |

`KW` (verbatim, L1108): `const let var function return export import from async await if else for while new type interface class extends of in try catch throw default private constructor describe it expect`.
Adjacent runs with the same colour are merged (L1123); an empty line becomes one `' '` span coloured `t.fg` (L1124).

### 1.7 Chart block (fence type `` ```chart <title> ``) — markup L180–L204
The prototype's representation of chart data is a **fenced block whose body is whitespace-separated rows**:
````
```chart Cache hit rate (%) · staging
09:00 38
10:00 61
11:00 74
12:00 82
13:00 87
14:00 91
```
````
Parser (L1349–L1356):
* `title = rest` (everything after the word `chart`, trimmed) — falls back to `'Chart'`
* each body line → `trim().split(/\s+/)`, kept if `length > 1`
* `label = all tokens except the last, joined by ' '`; `value = +lastToken` (numeric cast)
* `peak = max(values, 1)`
* `sum = rows.length + ' points · peak ' + peak` → **`6 points · peak 91`** **[measured]**
* grid lines at `y = 25 | 50 | 75` (%)
* bar height `h = Math.max(2, Math.round(v / peak * 118))` px inside a 150px plot
* peak bar: `vc = t.acc2`, `bg = linear-gradient(180deg, t.acc2, t.acc)`
  → `linear-gradient(rgb(217,181,106), rgb(200,30,42))` **[measured]**
* other bars: `vc = t.muted`, `bg = linear-gradient(180deg, rgba(acc,0.85), rgba(acc,0.25))`
  → `linear-gradient(rgba(200,30,42,0.85), rgba(200,30,42,0.25))` **[measured]**
* `Copy CSV` (L1355) → clipboard `label,value` per line; toast `Copied CSV`

Layout:
```
frame:  border-radius:10px; border:1px solid t.line2; background:codeBg; overflow:hidden   (L181)
header (L182): display:flex; align-items:center; gap:10px; padding:6px 6px 6px 14px;
               border-bottom:1px solid t.line; font-size:11px; color:t.muted
   bar-chart icon 13×13, stroke t.acc2, path "M3 13V9M8 13V4M13 13V7"
   title: flex:1; font-size:12.5px; color:t.fg; ellipsis
   sum:   (inherits 11px muted)
   Copy CSV button: same 4px 9px / radius 5px treatment as Copy
plot (L188): position:relative; height:150px; display:flex; align-items:flex-end; gap:10px;
             border-bottom:1px solid t.line2
   grid (L190): position:absolute; left:0; right:0; bottom:{25|50|75}%;
                border-top:1px dashed t.line            → rgba(243,234,216,0.09) dashed
   bar col (L191): flex:1; min-width:0; display:flex; flex-direction:column;
                   align-items:center; justify-content:flex-end; gap:5px
      value label (L192): font-size:11px; color:br.vc
      bar (L193): width:100%; max-width:46px; height:br.h px; border-radius:4px 4px 0 0
x labels (L198): display:flex; gap:10px; padding-top:7px; each flex:1; text-align:center;
                 11px; t.muted; nowrap; ellipsis
body:  padding:16px 16px 10px                                                              (L187)
```
**[measured]** x-labels padding `7px 0` (i.e. 7px top), 11px `t.muted`; bars 46px wide in the tile
(38px on the 0.8333-scaled screenshot, i.e. the `max-width:46px` clamp is active),
radius `4px 4px 0 0`; grid lines at 25/50/75%.

### 1.8 Image block (fence type `` ```image <path> <name> ``) — markup L204–L214; lightbox L870–L874
````
```image assets/fhd/bonfire.jpg bonfire-frame-240.png
bonfire.wgsl · frame 240 · 1920×1080
```
````
Parser (L1358–L1363): `[path, name] = rest.split(/\s+/)`, `name = name || basename(path)`,
`caption = body` (the whole body, newlines preserved).
```
frame: max-width:560px; border-radius:10px; border:1px solid t.line2;
       background:codeBg; overflow:hidden                                       (L205)
thumb: cursor:zoom-in; aspect-ratio:16/9; background:url(src) center/cover       (L206)  → opens lightbox
caption row (L207): display:flex; align-items:center; gap:6px; padding:7px 6px 7px 14px;
                    border-top:1px solid t.line
   image icon 13×13, stroke t.acc2, path "M2 3h12v10H2z"-shape:
     <rect x="2" y="3" width="12" height="10" rx="1.5"/> + <path d="M2.5 11l3.5-3.5 3 3 2-2 2.5 2.5"/>
   name: 12.5px, t.fg, ellipsis            caption: 11px, t.muted, ellipsis
   Open button:   padding:5px 10px; border-radius:5px; font-size:11.5px; color:t.muted; hover bg line2 + fg
   Attach button: identical styling; adds the file to the composer attachment list (L1362) + toast `Attached <name>`
```
**[measured]** frame max-width 560px, radius 10px, border `rgba(243,234,216,0.22)`, bg `rgba(13,10,8,0.72)`;
thumb 397×223 at 0.8333 scale (= 476×268 layout) with `aspect-ratio:16/9`, `cursor:zoom-in`;
caption row padding `7px 6px 7px 14px`, gap 6px; name 12.5px `t.fg`; caption 11px `t.muted`;
both buttons `padding:5px 10px; radius:5px; font-size:11.5px; color:t.muted`.

**Full-size viewer (lightbox)** — L870–L874:
```html
<div onClick=close  style="position:absolute;inset:0;z-index:30;background:rgba(0,0,0,0.85);
     display:flex;flex-direction:column;align-items:center;justify-content:center;gap:14px;cursor:zoom-out">
  <div style="width:1440px;height:810px;border-radius:10px;background:url(src) center/contain no-repeat"></div>
  <span style="font-size:13px">{{name}} <span style="color:t.muted">· {{caption}}</span></span>
</div>
```
**[measured]** overlay `rgba(0,0,0,0.85)`, image box 1440×810 radius 10px `background-size:contain`,
caption 13px (`name` in `t.fg`, ` · caption` in `t.muted`) →
`bonfire-frame-240.png · bonfire.wgsl · frame 240 · 1920×1080`.
Whole overlay is click-to-close; **Escape** also closes it (global key handler L1243, only when a modal or
lightbox is open).

### 1.9 Inline file chips (attachments rendered inside messages)
A user message's `atts` are rendered as cards **above the bubble**, inside the right-aligned wrapper
(L142–L151). Sizes differ from the composer cards — see §3.2 for the full table.
Example from the seed (`s1`, L1128): attachment `src/server/session.ts` renders
**badge `TS` + `session.ts` (11.5px) + `src/server · 23 lines` (10px, `t.muted`)** — the exact
"session.ts / src/server · 23 lines" chip in the brief. `23 lines` = line count of the file's source string (L1216).

---

## 2. Rich reply blocks — quick reference

| fence | header line | body | actions |
|---|---|---|---|
| ` ```<lang> <path>` | uppercase lang badge (acc2, ls .12em) + path (muted) | line-numbered tokenised code, `t.code` 12.5px/1.65 | `Copy` → clipboard of body, toast `Copied <file>` or `Copied code` |
| ` ```chart <title>` | bar-chart icon + title (12.5px fg) + `N points · peak P` | 150px bar plot + x labels | `Copy CSV` → `label,value` lines, toast `Copied CSV` |
| ` ```image <path> <name>` | 16:9 thumbnail (zoom-in) | image + caption row | `Open` → lightbox, `Attach` → composer attachment |
| anything else / no fence | — | plain text part (15px, 1.6, pre-wrap) | — |

Fence regex (L980): `` const FENCE = new RegExp('\x60{3}([^\n]*)\n([\s\S]*?)\x60{3}', 'g') `` —
the info string is `m[1]`, first word = language, the rest = `title`/`path`/`file`; body is `m[2]` with one
trailing newline stripped (**L1348**). Text outside fences is emitted as `txt` parts with leading/trailing
newlines trimmed (**L1344**). Branch refs in `parts()` (L1342): chart L1349, image L1358, code L1365;
copy handlers L1355 (CSV) / L1366 (code).

---

## 3. Attachments

### 3.1 Type badges — every type the prototype knows
`attInfo(full, t)` (L1211–L1219) derives the badge, the thumbnail/`isImg` flag and the meta line.

```js
const ext = (name.split('.').pop() || '').toLowerCase();
const img = ['png','jpg','jpeg','gif','webp'].includes(ext);
const TAG = { ts:['TS',t.acc2], tsx:['TSX',t.acc2], json:['{ }',t.alt], md:['MD',t.fg],
              pdf:['PDF',t.acc], log:['LOG',t.alt], wgsl:['WGSL',t.acc2] }[ext]
            || [ext.slice(0,4).toUpperCase() || 'FILE', t.muted];
tagBg = rgba(TAG.colour, 0.14);
```

| ext | badge label | badge colour (Blood Moon) | badge bg **[measured]** |
|---|---|---|---|
| `ts` | `TS` | `t.acc2` `#d9b56a` | `rgba(217,181,106,0.14)` |
| `tsx` | `TSX` | `t.acc2` | `rgba(217,181,106,0.14)` |
| `json` | `{ }` | `t.alt` `#c9a77a` | `rgba(201,167,122,0.14)` |
| `md` | `MD` | `t.fg` `#f3ead8` | `rgba(243,234,216,0.14)` |
| `pdf` | `PDF` | `t.acc` `#c81e2a` | `rgba(200,30,42,0.14)` |
| `log` | `LOG` | `t.alt` | `rgba(201,167,122,0.14)` |
| `wgsl` | `WGSL` | `t.acc2` | `rgba(217,181,106,0.14)` |
| `png/jpg/jpeg/gif/webp` | **image thumbnail** instead of a badge | `isImg:true`, `src = ATT_SRC[full] \|\| t.wall` | — |
| anything else | `ext.slice(0,4).toUpperCase()` (e.g. `TXT`, `GIT`) or `FILE` | `t.muted` `#bfae94` | `rgba(191,174,148,0.14)` |

`ATT_SRC` (L983) = `{ 'bonfire-frame-240.png': 'assets/fhd/bonfire.jpg' }`; any other image name falls back
to the current wallpaper. (There is a *separate* tag map for the IDE file tree, L1575:
`ts→TS acc2, json→'{ }' alt, md→MD muted, gitignore→GIT muted, default TXT muted` — not used in chat.)

**Meta line** (L1216):
| case | value | example **[measured]** |
|---|---|---|
| image | `full === 'browser.png' ? 'Browser · 1440×900' : 'Image · 1920×1080'` | `Browser · 1440×900` |
| `log` | `'Terminal · 42 lines'` | — |
| `pdf` | `'PDF · 4 pages'` | — |
| otherwise | `(dir \|\| 'root') + ' · ' + lineCount + ' lines'` where `dir = full.split('/').slice(0,-1).join('/')` | `src/server · 23 lines`, `src/agent · 20 lines` |

**Display name**: `full === 'browser.png' ? 'browser screenshot.png' : full === 'terminal.log' ? 'terminal output.log' : basename` (L1214).

### 3.2 Cards
| context | source | thumbnail/badge | card | text |
|---|---|---|---|---|
| **composer** | L239–L244 | image: 36×36 radius 6px cover; file: `<span>` 36×36 radius 6px, font **9.5px/700**, letter-spacing `.04em`, `display:grid;place-items:center` | `position:relative; display:flex; align-items:center; gap:10px; padding:5px 12px 5px 5px; border-radius:9px; background:t.line; border:1px solid t.line2; max-width:230px` | name **12px** (`t.fg`, ellipsis); meta **10.5px** `t.muted` (ellipsis); column gap 2px |
| **sent message** | L144–L150 | image: 30×30 radius 5px; file: `<span>` 30×30 radius 5px, font **9px/700**, ls `.04em` | `display:flex; align-items:center; gap:9px; padding:4px 11px 4px 4px; border-radius:8px; border:1px solid t.line2; background:t.line; max-width:240px` | name **11.5px**; meta **10px** `t.muted`; column gap 1px, no remove button |
| composer attachment row | L236–L237 | — | `display:flex; flex-wrap:wrap; gap:8px; padding:12px 12px 0` (only rendered when `hasAtts`) | — |
| sent-message attachment row | L142 | — | `display:flex; flex-wrap:wrap; gap:6px; justify-content:flex-end` | — |

**Remove button (composer only)** — L243:
```
position:absolute; top:-7px; right:-7px; width:18px; height:18px; border-radius:50%;
display:grid; place-items:center; font-size:11px; line-height:1;
background:t.pop; border:1px solid t.line2; color:t.muted; title="Remove"
hover: color:t.fg; border-color:t.acc
```
**[measured]** content `×`, absolute, top/right `-7px`, 18×18, radius 50%, bg `rgb(27,20,17)`,
border `1px solid rgba(243,234,216,0.22)`, 11px/11px `t.muted`.

### 3.3 Attach popover (opened by the paperclip) — L306–L318 (panel L307)
```
panel: position:absolute; left:8px; bottom:calc(100% + 6px); width:300px; padding:6px;
       border-radius:10px; background:t.pop; border:1px solid t.line2;
       box-shadow:0 14px 40px rgba(0,0,0,.45); z-index:5;
       display:flex; flex-direction:column; gap:2px
header: 'ATTACH' — font-size:10.5px; letter-spacing:.16em (1.68px); color:t.muted; padding:6px 8px
option: display:flex; align-items:center; gap:10px; padding:8px 10px; border-radius:6px; cursor:pointer
        hover → background:t.line2
   image option: 28×28 radius 5px cover
   file option:  28×28 radius 5px tag span, font 8.5px/700, bg rgba(tag,0.14)
   name: font-size:12.5px; flex:1; nowrap; ellipsis
   kind: font-size:11px; color:t.muted
```
Option list (`attachOpts`, L1553–L1558) is **fixed**, in this order:

| # | name shown | kind column | underlying file |
|---|---|---|---|
| 1–4 | first four keys of `FILES`: `loop.ts`, `tools.ts`, `session.ts`, `cache.ts` | `src/agent`, `src/agent`, `src/server`, `src/server` (dir) | `src/agent/loop.ts`, `src/agent/tools.ts`, `src/server/session.ts`, `src/server/cache.ts` |
| 5 | `browser screenshot.png` | `Browser` | `browser.png` (image → thumbnail) |
| 6 | `terminal output.log` | `Terminal` | `terminal.log` (LOG badge) |
| 7 | `Upload from disk…` | `''` | `design-spec.pdf` (PDF badge) |

Selecting an option appends to `atts[focus]` (no duplicates) and closes the popover (L1558).
**[measured]** panel 300px wide, left 8px, bottom `calc(100% + 6px)`, padding 6px, radius 10px,
bg `rgb(27,20,17)`, border `rgba(243,234,216,0.22)`, shadow `rgba(0,0,0,0.45) 0 14px 40px`, gap 2px;
header `ATTACH` 10.5px ls 1.68px; options padding `8px 10px`, radius 6px, gap 10px;
badges 28×28 radius 5px, 8.5px/700; names 12.5px; kinds 11px muted.

---

## 4. Composer

### 4.1 Structure — L234–L263 (composer); popovers L264–L318
```html
<div style="padding:8px 12px 12px">                                    <!-- outer, L234 -->
  <div style="position:relative; border-radius:10px;
              border:1px solid {{tl.compBd}}; background:{{t.pop}}">   <!-- wrapper, L235 -->
    [attachments row]                                                  <!-- L236-L247 -->
    <textarea …></textarea>                                            <!-- L248 -->
    <div style="display:flex; align-items:center; gap:6px; padding:6px 8px 8px; flex-wrap:wrap">  <!-- toolbar, L249 -->
      [paperclip] [spacer flex:1] [model trigger] [send/stop]
    </div>
    [model popover]  [attach popover]                                  <!-- L264 / L306, absolute, above -->
  </div>
</div>
```
| element | values |
|---|---|
| outer padding | `8px 12px 12px` **[measured]** |
| wrapper | `border-radius:10px`, `background:t.pop` (`#1b1411`), `border:1px solid compBd` where `compBd = focused ? t.line2 : t.line` (L1537) — **the focused tile has the brighter border**; `position:relative` |
| textarea | `rows="2"`, `display:block; width:100%; box-sizing:border-box; padding:12px 14px 6px; line-height:1.5; font-size:15px`, `resize:none` (global `textarea{resize:none}`, L21) |
| toolbar | `display:flex; align-items:center; gap:6px; padding:6px 8px 8px; flex-wrap:wrap` |

**[measured]** wrapper 521px wide in the 1st tile (434px on the 0.8333-scaled screenshot),
border `rgba(243,234,216,0.22)`, bg `rgb(27,20,17)`;
textarea padding `12px 14px 6px`, 15px, line-height 22.5px; toolbar height 40px.
Multiline: 2 lines → textarea height **63px** (= 2 × 22.5 + 12 + 6) **[measured]**; beyond 2 rows the
textarea grows with content and is visually bounded by the tile, not by a max-height.

Per-tile composer state is stored per session id: `drafts[sid]`, `atts[sid]`, `pop{k:'model'|'att', id}`
(L1191; `atts`/`attOpen`/`modelOpen` L1538–L1539, toggles L1541–L1542) — drafts and attachments survive tile
focus changes but are cleared on send (L1305).

### 4.2 Placeholders — L1532 (`ph`)
| mode | placeholder (verbatim) | measured |
|---|---|---|
| Auto | `'Message ' + AN + '… (⏎ to send, ⇧⏎ newline, ⇧Tab mode)'` | `Message archonui… (⏎ to send, ⇧⏎ newline, ⇧Tab mode)` |
| Plan | `'Describe the change. ' + AN + ' will plan before touching files…'` | `Describe the change. archonui will plan before touching files…` |
| Ask | `'Ask about the code. Nothing will be changed…'` | same |

`AN = appName.trim() || 'archonui'` (L1516) — the user-configurable app name (default `archonui`, lowercase in
placeholders, `ARCHONUI` in the agent header).

### 4.3 Buttons
| # | control | markup |
|---|---|---|
| 1 | **Attach** `title="Attach files"` (L250) | `width:32px; height:32px; display:grid; place-items:center; border-radius:7px; color:t.fg; background:attBtnBg` (`t.line`, `t.line2` while its popover is open); hover `t.line2`; icon = paperclip `16×16` `viewBox 0 0 16 16`, `stroke-width:1.5`, path `M10.5 4.5l-5 5a1.6 1.6 0 002.3 2.3l5.5-5.5a3 3 0 00-4.2-4.2L3.6 7.6` |
| 2 | spacer | `<span style="flex:1">` |
| 3 | **Model trigger** (L252–L257) | `display:flex; align-items:center; gap:8px; height:32px; padding:0 12px; border-radius:7px; font-size:13px; background:modelBtnBg` (`t.line`, `t.line2` when open) — contents in §5.6 |
| 4 | **Send / Stop** (L258–L263) | `width:34px; height:34px; display:grid; place-items:center; border-radius:50%`, `title = {{tl.sendLbl}}` |

Send/Stop states (L1535–L1536, **[measured]**):

| state | condition | bg | fg | icon |
|---|---|---|---|---|
| idle, empty draft | `!running && !draft.trim()` | `t.line2` → `rgba(243,234,216,0.22)` | `t.muted` `rgb(191,174,148)` | arrow-up svg `16×16`, path `M8 13V3M3.5 7.5L8 3l4.5 4.5`, stroke-width 1.8, title `Send` |
| idle, draft text | `!running && draft.trim()` | `t.acc` → `rgb(200,30,42)` | `t.on` → `rgb(246,236,220)` | same arrow, title `Send` |
| running | `status === 'running'` | `t.fg` → `rgb(243,234,216)` | `t.bg` → `rgb(13,10,8)` | `<span style="width:11px;height:11px;border-radius:2px;background:currentColor">` (rounded square), title `Stop` |

Clicking while running **stops**: `status → 'idle'` and the last agent message becomes
`{ thinking:false, meta:'stopped', text: m.text || 'Stopped.' }` (L1535).
**[measured]** after stop: header reads `ARCHONUI stopped`, body `Stopped.`, tile meta loses `· running`,
send button returns to the idle-empty state. Clicking while idle calls `send()` (no-op if the draft is empty
or the session is already running, L1300).

### 4.4 Keyboard — `onKey` L1530 (bound on the textarea at L248) + global handler L1242–L1253
| key | behaviour |
|---|---|
| `Enter` (no Shift, no Alt) | `preventDefault()` → send (draft is `.trim()`-checked; empty draft does nothing) |
| `Shift+Enter` | newline (default browser behaviour, not intercepted) |
| `Shift+Tab` | `preventDefault()` → cycle mode `Auto → Plan → Ask → Auto` (`cycle()`, L1518) — placeholder, trigger icon and tile meta update live |
| `Alt+Enter` | new session (global, L1245) |
| `Alt+Q` | close focused tile (global) |
| `Alt+T` | next theme (L1248) |
| `Alt+←/→/↑/↓` | move focus between tiles (global) |
| `Escape` | only closes an open modal or the image lightbox (L1243) — it does **not** clear the draft or close the pickers |

There is **no click-outside dismissal** for the two popovers: they close when their trigger is clicked again,
when a model/mode/effort/attachment is chosen, on send, and when a session/modal action resets `pop` (L1280, L1296, L1305, L1622).

---

## 5. MODEL PICKER popover

Opened by clicking the model trigger (`toggleModel`, L1542); rendered at L264–L304, anchored
`right:8px; bottom:calc(100% + 6px)`.

### 5.1 Panel
```
width:360px                (content box; +6px padding ×2 +1px border ×2 → 374px outer)
padding:6px; border-radius:10px; background:t.pop; border:1px solid t.line2;
box-shadow:0 14px 40px rgba(0,0,0,.45); z-index:5;
display:flex; flex-direction:column; gap:2px
```
**[measured]** outer 374×545 (at scale 1), bg `rgb(27,20,17)`, radius 10px, shadow
`rgba(0,0,0,0.45) 0 14px 40px`, gap 2px, display flex column.

### 5.2 `MODE` section — L266–L274
```
label:  'MODE'  font-size:11px; letter-spacing:.16em (1.76px); color:t.muted; padding:6px 8px
grid:   display:grid; grid-template-columns:repeat(3,minmax(0,1fr)); gap:4px; padding:0 4px 8px
        → measured columns 114.66px each, gap 4px
button: display:flex; flex-direction:column; gap:5px; padding:9px 10px; border-radius:7px;
        border:1px solid {{am.bd}}; background:{{am.bg}}; hover → background:t.line2   (measured 115×56)
   title span: display:flex; align-items:center; gap:7px; font-size:12.5px; color:{{am.fg}}
               + svg 13×13, viewBox 0 0 16 16, stroke-width 1.6, path {{am.d}}
   sub span:   font-size:10.5px; color:t.muted
```

| mode | `n` | `sub` | icon path `d` | `title` (tooltip) | selected colours |
|---|---|---|---|---|---|
| Auto | `Auto` | `Edits and runs` | `M3 4l4 4-4 4M8.5 4l4 4-4 4` (double chevron ») | `Auto: edits files and runs commands (⇧Tab to cycle)` | bg `t.line2`, border `t.acc2`, text `t.acc2` |
| Plan | `Plan` | `Steps first` | `M3 4h10M3 8h7M3 12h9` (three rules ≡) | `Plan: proposes steps, changes nothing (⇧Tab to cycle)` | bg `t.line2`, border `t.acc2`, text `t.alt` |
| Ask | `Ask` | `Read-only` | `M5.8 6a2.2 2.2 0 1 1 3.3 1.9c-.7.4-1.1.9-1.1 1.6M8 12.2v.1` (question mark) | `Ask: answers questions, read-only (⇧Tab to cycle)` | bg `t.line2`, border `t.acc2`, text `t.fg` |

Unselected: bg transparent, border `t.line`, title+sub `t.muted`.
Selection formula (L1531): `fg = on ? (Auto ? t.acc2 : Plan ? t.alt : t.fg) : t.muted`, plus
`bd = on ? t.acc2 : t.line`, `bg = on ? t.line2 : 'transparent'`, `px = on ? 10 : 7`.
**[measured]** Auto selected → bg `rgba(243,234,216,0.22)`, border `rgb(217,181,106)`, title `rgb(217,181,106)`;
Plan selected → title `rgb(201,167,122)` (alt); Ask selected → title `rgb(243,234,216)` (fg).
Mode definitions: `MODES` (L984–L988).

### 5.3 `MODEL` section — L275–L286
```
label:  'MODEL'  same 11px / .16em / t.muted with border-top:1px solid t.line and padding:6px 8px
list:   max-height:232px; overflow:auto; display:flex; flex-direction:column; gap:2px   (measured maxH 232px)
row:    cursor:pointer; display:flex; align-items:center; gap:10px;
        padding:8px 10px; border-radius:6px; background:{{mo.bg}}; hover → t.line2  (measured 40px tall)
   name: font-size:13px
   desc: font-size:11px; color:t.muted      → "{{harness full name}} · {{ctx}}"
   check: color:t.acc2, content '✓'         (only for the current model)
   selected row bg: t.soft → rgba(176,20,30,0.32)
```
List construction (L1545): `MODELS.filter(m => (modelOn[m.id] && provOn[m.p]) || m.id === se.model)` —
i.e. **enabled models only, plus the session's current model even if disabled**.
`d = PROVIDERS.find(h => h.id === m.p).n + ' · ' + m.ctx`; `check = m.id === se.model ? '✓' : ''`.

All models the prototype knows (`MODELS` L914–L925, `LV_EXTRA` L926; `p` = harness id):

| id | name `n` | short `s` | desc `d` | ctx | harness (`p`) | effort levels `lv` |
|---|---|---|---|---|---|---|
| `opus` | Claude Opus 4.5 | Opus 4.5 | Most capable | 200k | claude (Claude Code) | Low, Medium, High, Max (4) |
| `sonnet` | Claude Sonnet 4.5 | Sonnet 4.5 | Balanced | 1M | claude | Off, Low, Medium, High, Max (5) |
| `haiku` | Claude Haiku 4.5 | Haiku 4.5 | Fast | 200k | claude | Off, On (2) |
| `gpt` | GPT-5 | GPT-5 | Reasoning | 400k | codex (Codex CLI) | Minimal, Low, Medium, High (4) |
| `gptmini` | GPT-5 mini | GPT-5 mini | Fast | 400k | codex | Low, Medium, High (3) |
| `gemini` | Gemini 2.5 Pro | Gemini 2.5 | Long context | 1M | gemini (Gemini CLI) | Auto, 1k, 4k, 8k, 16k, 32k (6) |
| `flash` | Gemini 2.5 Flash | Flash 2.5 | Fast | 1M | gemini | Off, Low, High (3) |
| `deepseek` | DeepSeek V3.1 | DeepSeek | Open weights | 128k | opencode (OpenCode) | Off, On (2) |
| `local` | qwen3-coder 32B | qwen3 32B | Local | 128k | opencode | Off, Low, High (3) |
| `llama` | llama3.3 70B | llama 70B | Local | 128k | opencode | **(none)** |

`LV_EXTRA` supplies `lv` only when a model has none declared: `gpt`, `gptmini`, `gemini`, `flash`.
Models **enabled by default** (`modelOn`, L1199): `opus, sonnet, haiku, gpt, gemini, local`.
Default harnesses on (`provOn`): `claude, codex, gemini, opencode` (aider/goose off, both flagged `missing: true`).
→ **The default picker list is exactly 6 rows**: `Claude Opus 4.5 (Claude Code · 200k) ✓`,
`Claude Sonnet 4.5 (Claude Code · 1M)`, `Claude Haiku 4.5 (Claude Code · 200k)`,
`GPT-5 (Codex CLI · 400k)`, `Gemini 2.5 Pro (Gemini CLI · 1M)`, `qwen3-coder 32B (OpenCode · 128k)`
**[measured, in that order]** — plus `llama3.3 70B (OpenCode · 128k)` once it is enabled in Settings → Models.

Picking a model (L1545) keeps the *relative* effort position:
`effort = nl.length ? nearestLv(nl, NL ? efr : 0.5) : se.effort` where `NL` = the current model's level count and
`efr` the current fraction (`nearestLv` picks the level whose `effFrac` is closest, L930).

### 5.4 `EFFORT` section — L288–L302 (rendered only when `tl.hasEff`, i.e. `lv.length > 0`)
```
block:  display:flex; flex-direction:column; gap:10px; padding:12px 10px 14px;
        margin-top:4px; border-top:1px solid t.line
header: display:flex; align-items:baseline; justify-content:space-between
   label: 'EFFORT · {{effN}} LEVELS'  font-size:11px; letter-spacing:.16em; color:t.muted
   name:  {{effName}}  font-family:t.display (UnifrakturCook); font-size:22px; line-height:1;
          color:t.acc2                                    (measured "High" / "Max" in UnifrakturCook)
track:  position:relative; display:grid; grid-template-columns:repeat({{effN}},minmax(0,1fr));
        align-items:center; height:44px               (measured 85px per column for N=4, 68px for N=5)
   rail (L291): position:absolute; left:{{effInset}}%; right:{{effInset}}%; top:50%; height:2px;
                margin-top:-1px; border-radius:1px; background:t.line2
   fill (L292): position:absolute; left:{{effInset}}%; top:50%; height:2px; margin-top:-1px; border-radius:1px;
                width:{{effFill}}%; background:linear-gradient(90deg,t.acc,t.acc2);
                box-shadow:0 0 10px t.acc; transition:width .25s
   moon (L294): button, all:unset, cursor:pointer, position:relative; justify-self:center;
                width:38px; height:38px; display:grid; place-items:center; border-radius:50%;
                background:t.pop; border:1px solid {{ef.bd}}; color:{{ef.c}}; filter:{{ef.glow}};
                transition:filter .2s,color .2s; title={{ef.n}}
                inner svg width=24 height=24 viewBox="0 0 16 16":
                  <circle cx="8" cy="8" r="5.5" fill="none" stroke="currentColor" stroke-width=".8" opacity=".5"/>
                  <path d="{{ef.moon}}" fill="currentColor"/>
labels (L299): display:grid; grid-template-columns:repeat({{effN}},minmax(0,1fr)); text-align:center;
               font-size:11.5px; each span color {{ef.lc}}; white-space:nowrap
desc (L301):   font-size:12px; color:t.muted; text-align:center; text-wrap:pretty
```

**MOONS** — the effort control is drawn as moon phases, one button per level.
`MOON(f)` (L982) builds an SVG path from the level's **fraction** `f`:

```js
const MOON = f => { if (f <= 0) return '';                       // f == 0 → empty path (nothing drawn)
  const rx = Math.max(0.01, Math.abs(1 - 2 * f) * 5.5);
  return 'M8 2.5A5.5 5.5 0 0 1 8 13.5A' + rx.toFixed(2) + ' 5.5 0 0 ' + (f > 0.5 ? 1 : 0) + ' 8 2.5Z'; };
```
Computed per fraction (all paths **[measured]** in the browser):

| f | rx | sweep | path | glyph |
|---|---|---|---|---|
| 0 | — | — | `''` (path omitted) | nothing lit |
| 0.1667 (1/6) | 3.67 | 0 | `M8 2.5A5.5 5.5 0 0 1 8 13.5A3.67 5.5 0 0 0 8 2.5Z` | thin crescent (right) |
| 0.25 | 2.75 | 0 | `…A2.75 5.5 0 0 0 8 2.5Z` | crescent (right) |
| 0.3333 | 1.83 | 0 | `…A1.83 5.5 0 0 0 8 2.5Z` | thin crescent |
| 0.5 | 0.01 | 0 | `…A0.01 5.5 0 0 0 8 2.5Z` | new moon (dark sliver) |
| 0.6667 | 1.83 | 1 | `…A1.83 5.5 0 0 1 8 2.5Z` | gibbous |
| 0.75 | 2.75 | 1 | `…A2.75 5.5 0 0 1 8 2.5Z` | gibbous |
| 0.8333 | 3.67 | 1 | `…A3.67 5.5 0 0 1 8 2.5Z` | near-full |
| 1 | 5.50 | 1 | `…A5.50 5.5 0 0 1 8 2.5Z` | full moon |

**Fraction formula** (L929):
```
effFrac(lv, k) = lv.length < 2 ? 0
               : lv[0] === 'Off' ? k / (lv.length - 1)      // Off-first lists
               : (k + 1) / lv.length                        // lists without Off
```
So: a 4-level list → 0.25 / 0.5 / 0.75 / 1.0; a 5-level `Off…` list → 0 / 0.25 / 0.5 / 0.75 / 1.0;
a 6-level list → 0.1667 … 1.0.

**Moon colours / glow** (L1547):
```js
c     = k <= ei ? t.acc2 : t.faint            // reached levels are gold, later ones faint
bd    = selected ? t.acc2 : (k <= ei ? t.line2 : t.line)
glow  = selected ? 'drop-shadow(0 0 8px ' + t.acc + ')' : 'none'
lc    = selected ? t.fg : t.muted             // label under the moon
```
**[measured]** Low/Medium unselected-reached: colour `rgb(217,181,106)`, border `rgba(243,234,216,0.22)`, no glow;
selected High: border `rgb(217,181,106)`, filter `drop-shadow(rgb(200,30,42) 0px 0px 8px)`;
future Max: colour `rgb(106,90,72)`, border `rgba(243,234,216,0.09)`;
labels: reached/selected `rgb(243,234,216)` when selected else `rgb(191,174,148)`.
Moon buttons: 38×38 + 1px border = 40px outer, radius 50%, bg `rgb(27,20,17)`.

**Track geometry** (L1533; the chosen level is set by the moon's `pick: () => set({ effort: ef })`, L1546):
```
effInset = 50 / N                      // %    N=4 → 12.5,  N=5 → 10,  N=6 → 8.33, N=2 → 25, N=3 → 16.67
effFill  = N > 1 ? ei / (N - 1) * (100 - 100 / N) : 0     // % width of the lit segment
```
N=4 (`opus`, `gpt`): inset 12.5%, widths **0 / 25 / 50 / 75%** at ei = 0…3
(ei = 2 "High" → `left:12.5%; width:50%` **[measured]**).
N=5 (`sonnet`): inset 10%, widths 0 / 20 / 40 / 60 / 80% **[measured rail/`left:10%`, fill `80%` at Max]**.
N=6 (`gemini`): inset 8.33%, widths 0 / 16.67 / 33.33 / 50 / 66.67 / 83.33%.
N=3 (`gptmini`, `flash`, `local`): inset 16.67%, widths 0 / 33.33 / 66.67%. N=2: inset 25%, widths 0 / 50%.

**Labels**: the tail of the track shows `{{ef.n}}`, the level name verbatim:
`Low`, `Medium`, `High`, `Max`, `Minimal`, `Off`, `On`, `Auto`, `1k`, `4k`, `8k`, `16k`, `32k`.
They are not derived — `n: ef` = the raw level string (L1546). The narrow-tile variant `lbl: ef[0]` exists in the
view model but is **not used** by the popover markup (the `· {{effLbl}}` trigger variant is unused too).

**Description line** (L1533; `EDESC` L931):
`effDesc = EDESC(effName, effFrac) + ' ' + model.s + ' has ' + N + ' levels.'`

`EDESC(name, f)` logic (L931) — first match wins:
| condition | text |
|---|---|
| `/^\d+k$/` on name | `Thinking budget of <name> tokens.` |
| name === `Auto` | `The model picks a thinking budget each turn.` |
| `f === 0` | `No extended thinking. Fastest replies.` |
| `f <= 0.34` | `Light thinking for small, contained changes.` |
| `f <= 0.6` | `Reads around the change before editing.` |
| `f < 1` | `Weighs alternatives and runs tests.` |
| else | `Thinks as long as the task needs.` |

Measured: High on Opus → `Weighs alternatives and runs tests. Opus 4.5 has 4 levels.`;
Max on Sonnet → `Thinks as long as the task needs. Sonnet 4.5 has 5 levels.`
(A different, unused map `EFF_DESC` exists at L989 with "About 2s/6s/15s" timings — it is only referenced
by `settingsVals`, not by the composer.)

**No-effort state** (`tl.noEff`, L303 — rendered instead of the whole EFFORT block):
```html
<span style="display:block;margin-top:4px;padding:12px 10px 10px;border-top:1px solid t.line;
             font-size:12px;color:t.muted">{{tl.modelS}} has no effort levels.</span>
```
**[measured]** with `llama3.3 70B` selected: `llama 70B has no effort levels.`, 12px `t.muted`,
padding `12px 10px 10px`, margin-top 4px, top border `t.line`; no `EFFORT · …` header, no track, no moons.

### 5.5 Per-message "thought for Ns" / tool-call volume (what effort means in the prototype)
`send()` (L1299; the effort-derived values are computed on L1306) uses:
```
fr   = lv.length ? effFrac(lv, effIdx(lv, effort)) : 0.3
n    = Math.max(1, Math.round(fr * 4))          // number of tool calls
think= Math.round(2 + fr * 29)                  // "thought for Ns"
```
| level (4/5-level list) | fr | tool calls `n` | `thought for` | completion delay `700 + n*450` |
|---|---|---|---|---|
| Off | 0 | 1 | 2s | 1150 ms |
| ¼ (Low) | 0.25 | 1 | 9s | 1150 ms |
| ½ (Medium) | 0.5 | 2 | 17s | 1600 ms |
| ¾ (High) | 0.75 | 3 | 24s | 2050 ms |
| Max | 1.0 | 4 | 31s | 2500 ms |
| gemini 1/6…1 | 0.167…1 | 1,1,2,3,3,4 | 7,12,17,21,26,31s | — |

Tool rows are appended one at a time every `450 ms` starting at `500 ms` (L1309–L1311), from
`POOL = [read src/agent/loop.ts, grep "retry" in src/, edit src/agent/loop.ts +12 −3, run pnpm test 131 passed]`
(L1163) — sliced to `n` in Auto mode, or filtered to `read`/`grep` (max 2) in Plan/Ask mode.
`const THINK = { Low:2, Medium:6, High:14, Max:31 }` (L1186) exists but is **never used** (dead constant).
Replies: `PLAN_REPLY` (L990) in Plan mode, `ASK_REPLY` (L991) in Ask mode, else `REPLIES[(msgs.length/2|0) % 4]` (L1157).

### 5.6 Trigger label (`» Opus 4.5 · High ▾`)
Markup L252–L257, left to right:
1. **Mode icon** — 13×13 svg, `viewBox 0 0 16 16`, `stroke-width:1.6`, `color:{{tl.modeC}}`, path `{{tl.modeD}}`.
   `modeD` = the selected `MODES[].d` (so `»` chevrons for Auto, `≡` for Plan, `?` for Ask).
   `modeC` = `Auto → t.acc2`, `Plan → t.alt`, else `t.fg` (L1534).
2. **Model name** — `modelLbl = wide ? model.n : model.s` where **`wide = tile width > 620px`** (L1513, L1543):
   wide tiles show `Claude Opus 4.5`, narrow tiles show `Opus 4.5`. Font 13px (inherited by the button).
3. **Effort group** (only when `hasEff`, i.e. `lv.length > 0`):
   `<span style="display:flex;align-items:center;gap:5px;color:t.muted">` containing
   * moon svg 14×14, `color:t.acc2`, `<circle cx=8 cy=8 r=5.5 fill=none stroke=currentColor stroke-width=1 opacity=".45">` + `<path d={{tl.moon}} fill=currentColor>` (note: thinner/cooler than the picker's 24px moons),
   * `{{tl.effName}}` — the raw level name (`High`, `Max`, …) in the span's `t.muted`.
   There is **no `·` separator** in the prototype; the brief's `» Opus 4.5 · High ▾` renders as
   `» Opus 4.5 <moon> High ▾` (mode icon, name, moon, level, caret) — **[measured]** text `Opus 4.5 High ▾`
   with 2 svgs (13px mode icon + 14px moon).
4. **Caret** — `<span style="color:t.muted;font-size:10px">▾</span>`.

When effort is hidden (`lv.length === 0`), the whole effort span is dropped →
trigger text is **`llama 70B ▾`** with a single svg (mode icon only) **[measured]**, and the tile
meta becomes `archonui-core · llama 70B` (no `· … effort`).

---

## 6. Running / streaming indicators, stop, copy

| indicator | where | spec |
|---|---|---|
| tile status dot | tile header, 8×8 with `2px solid t.tile` ring | `running → t.acc2`, `done → t.alt`, `idle → t.faint` (L1510) |
| tile meta | header line 2 | appends `· running` while running (L1521) |
| session list/rail dot | left sidebar | same `dotFor` mapping (L1654) |
| agent header | message | `ARCHONUI working` while `thinking` (meta `'working'`, L1303) |
| thinking label | below the tool group | italic 13px `t.muted`: `Working · N tool calls` (L1523) |
| tool rows | appear progressively, one every 450 ms (L1309) | see §1.5 |
| send button | toolbar | becomes the Stop button (white circle + 11×11 dark square, §4.3) |
| completion | — | `meta → 'thought for Ns'` (+ ` · plan mode` / ` · ask mode` for non-Auto), `status → 'idle'`, tools stop growing (L1312–L1318) |
| stop | click Stop | `meta → 'stopped'`, `text → m.text \|\| 'Stopped.'`, `status → 'idle'` (L1535) |
| edit side-effect | Git panel | if any tool call was an `edit`, `src/agent/loop.ts  M +12 −3` is appended to the changes list (L1317) |

**Copy actions**
| control | clipboard payload | toast |
|---|---|---|
| code block `Copy` | the fence body verbatim (`navigator.clipboard.writeText(body)`) | `Copied <basename>` or `Copied code` when the fence has no path (L1366) |
| chart `Copy CSV` | `rows.map(r => r.l + ',' + r.v).join('\n')` | `Copied CSV` (L1355) |
| image `Attach` | — | `Attached <name>` (L1362) |

**Toast** (L875–L877): `position:absolute; left:50%; bottom:36px; transform:translateX(-50%);
padding:11px 18px; border-radius:8px; background:t.pop; border:1px solid t.line2; font-size:13px;
box-shadow:0 10px 30px rgba(0,0,0,.4)`. Auto-dismiss after **1800 ms** (L1268 `flash()`).
**[measured]** `Copied CSV`, `Copied cache.ts` — bottom 36px, padding `11px 18px`, radius 8px,
bg `rgb(27,20,17)`, border `rgba(243,234,216,0.22)`.

---

## 7. Value summary table (chat view, scale-1 px)

| element | padding | radius | font | colour | bg |
|---|---|---|---|---|---|
| message list | `18px 18px 8px`, gap 16px | — | 15px | `t.fg` | — |
| user wrapper | gap 6px, max-w 82% | — | — | — | — |
| user bubble | `11px 14px` | `10px 10px 3px 10px` | 15px / 1.55 / pre-wrap / pretty | `t.fg` | `t.soft` |
| agent block | gap 8px, max-w 94% | — | — | — | — |
| agent header | gap 8px | — | 11px (app name ls 1.32px) | `t.muted` | — |
| agent text part | — | — | 15px / 1.6 / pre-wrap / pretty | `t.fg` | — |
| tool group | gap 1px | 8px | — | — | — |
| tool row | `7px 11px`, gap 10px | — | 12px | icon `t.acc2`, kind `t.muted` (38px), path `t.fg`, count `t.alt` | `t.line` |
| thinking label | — | — | 13px italic | `t.muted` | — |
| code frame | — | 8px | — | — | `codeBg` |
| code header | `5px 6px 5px 12px`, gap 10px | — | 11px | lang `t.acc2` (ls 1.32px, uppercase), path `t.muted` | — |
| code body | `10px 0` | — | `t.code` 12.5px / 1.65 | per-token (§1.6) | `codeBg` |
| code line | `padding-right:12px`, gap 12px | — | — | number `t.faint` (26px, right) | — |
| chart frame | — | 10px | — | — | `codeBg` |
| chart header | `6px 6px 6px 14px`, gap 10px | — | 11px; title 12.5px | title `t.fg`, rest `t.muted` | — |
| chart plot | — | bottom border `t.line2` | 150px tall, gap 10px | — | — |
| chart bar | — | `4px 4px 0 0` | 11px value | peak `t.acc2`, else `t.muted` | peak `acc2→acc`, else `acc` 85%→25% |
| image frame | — | 10px, max-w 560px | — | — | `codeBg` |
| image caption | `7px 6px 7px 14px`, gap 6px | — | name 12.5px / caption 11px | name `t.fg`, caption `t.muted` | — |
| composer outer | `8px 12px 12px` | — | — | — | — |
| composer wrapper | — | 10px, border `line2`(focused)/`line` | — | — | `t.pop` |
| textarea | `12px 14px 6px` | — | 15px / 1.5 | `t.fg` | transparent |
| toolbar | `6px 8px 8px`, gap 6px | — | — | — | — |
| attach button | 32×32 | 7px | — | `t.fg` | `t.line` |
| model trigger | `0 12px`, h 32, gap 8 | 7px | 13px | — | `t.line` |
| send button | 34×34 | 50% | — | `t.muted`/`t.on`/`t.bg` | `t.line2`/`t.acc`/`t.fg` |
| composer attachment card | `5px 12px 5px 5px`, gap 10px | 9px, max-w 230px | name 12px, meta 10.5px | name `t.fg`, meta `t.muted` | `t.line` |
| message attachment card | `4px 11px 4px 4px`, gap 9px | 8px, max-w 240px | name 11.5px, meta 10px | name `t.fg`, meta `t.muted` | `t.line` |
| picker panel | 6px | 10px, w 360 | — | — | `t.pop` |
| picker section label | `6px 8px` | — | 11px, ls 1.76px | `t.muted` | — |
| picker mode button | `9px 10px`, gap 5px | 7px | 12.5px / sub 10.5px | selected `acc2`/`alt`/`fg` | selected `t.line2` |
| picker model row | `8px 10px`, gap 10px | 6px | 13px / sub 11px | sub `t.muted`, check `t.acc2` | selected `t.soft` |
| picker effort block | `12px 10px 14px`, gap 10px | — | label 11px ls 1.76px, name `display` 22px | name `t.acc2` | — |
| moon button | 38×38 | 50%, border 1px | — | reached `t.acc2`, future `t.faint` | `t.pop` |
| attach popover | 6px | 10px, w 300 | header 10.5px ls 1.68px | header `t.muted` | `t.pop` |
| attach option | `8px 10px`, gap 10px | 6px | name 12.5px, kind 11px | kind `t.muted` | transparent |
| toast | `11px 18px`, bottom 36px | 8px | 13px | `t.fg` | `t.pop` |

---

## 8. Interactions to reproduce (verified in the browser)

1. `Shift+Tab` in the composer — mode cycles Auto → Plan → Ask → Auto; placeholder, trigger mode icon/colour,
   message-meta suffix and (on send) reply text all change. **Verified.**
2. `Enter` sends (draft cleared, attachments cleared, user message appended, `status: running`);
   `Shift+Enter` inserts a newline and the textarea grows (63px for 2 lines). **Verified.**
3. Clicking the model trigger opens the picker; clicking it again closes it. Mode selection inside the picker
   keeps it open. Picking a model updates the trigger/meta immediately. **Verified.**
4. Clicking a moon sets the effort: label under it turns `t.fg`, border `t.acc2`, glow added, fill widens. **Verified.**
5. Selecting `llama3.3 70B` (after enabling it in Settings → Models) removes the EFFORT block, shows the
   "has no effort levels." note, and drops the moon + level from the trigger. **Verified.**
6. Paperclip → ATTACH popover with 7 entries; choosing one closes it and adds a card above the textarea;
   `×` removes it. **Verified.**
7. `Open` on an image block opens the 1440×810 lightbox; clicking anywhere (or Esc) closes it. **Verified.**
8. Send while running → Stop; the message header becomes `stopped` and the body `Stopped.`. **Verified.**
9. `Copy` / `Copy CSV` show the bottom-centre toast for 1.8 s. **Verified** (`Copied cache.ts`, `Copied CSV`).

---

## 9. Prototype mock data for the chat view (seed, L1127–L1156)

Three tiles are open by default: `s1` (focused, `archonui-core · Opus 4.5 · High`), `s2`
(`archonui-web · Sonnet 4.5 · Medium · Ask mode · running`), `s4` (`ember-gl · Opus 4.5 · Max`).
`s1` contains: a user bubble with one attachment, an agent reply with 5 tool rows
(`read / grep / edit +24 −8 / edit +41 / run 128 passed`) and a `ts src/server/cache.ts` code fence;
a second user bubble, an agent reply with 2 edit rows and a code fence **plus a `chart` fence** (6 points).
`s4` contains an agent reply with an `image` fence (`assets/fhd/bonfire.jpg bonfire-frame-240.png`).
`s2` is the streaming example: `meta: 'working'`, `thinking: true`, 2 tool rows, partial text.
Use these as fixtures for porting tests.

---

## 10. Ambiguities / notes for the Rust port

1. **`»` is the Auto mode icon**, not a glyph in the model name; the trigger is
   `[mode icon] [model short|full] [moon + level] [▾]` with no `·` separators. The brief's
   `» Opus 4.5 · High ▾` is a loose rendering of that.
2. **Narrow vs wide tiles**: the model label switches between the short name and the full name at
   `tile width > 620px`. Same for `effLbl` (`· {{effort}}`) which the markup does *not* use.
3. The thinking label is **not pluralised** (`Working · 1 tool calls`).
4. `EFFORTS`/`EFFORTS_BASE` (`['Low','Medium','High','Max']`, L932–L933) are only used for the
   *settings* default-effort control and the `effIdx` fallback; the composer/renderer always uses the
   per-model `lv` array. `EFF_DESC` (L989) and `THINK` (L1186) are unused in the chat view.
5. Effort `Off`/`On` lists are *not* treated as booleans anywhere in the chat view — they render as ordinary
   levels with fractions 0 and 1 (Off → empty moon path, On → full moon).
6. The prototype has no real streaming: text appears complete at the end (`700 + n*450` ms); only the tool
   rows stream in. There is no per-token cursor and no "stop mid-token" state.
7. Popovers are not dismissed by clicking elsewhere and not by Escape; they are absolute-positioned inside
   the composer wrapper (`right:8px` for the model picker, `left:8px` for the attach picker), so they overlay
   the last messages rather than being flipped into the viewport.
8. The chart fence format has no axis labels, no y-axis values and no series concept: a single bar series,
   dashed 25/50/75% gridlines, values printed above each bar (peak in `acc2`).

---

## 11. Screenshot index (`.qa/design/`)

| file | shows |
|---|---|
| `chat-01-default.png` | full chat view, Blood Moon default (3 tiles) |
| `chat-02-picker.png` | model picker open (Auto, Opus 4.5, 4 moons) |
| `chat-03-picker-zoom.png` | picker at 1.45× (moon phases legible) |
| `chat-04-picker-plan.png` | Plan selected in the picker + Plan placeholder |
| `chat-05-picker-effort-max.png` | Max selected (fill widened, glow) |
| `chat-06-picker-no-effort.png` | GPT-5 selected |
| `chat-07-picker-5-levels.png` | Sonnet 4.5 → `EFFORT · 5 LEVELS` (Off…Max) |
| `chat-09-attach-popover.png`, `chat-09b-attach-popover-zoom.png` | ATTACH popover (7 options) |
| `chat-10-composer-attachment.png`, `chat-11b-composer-attachment-zoom.png` | attachment card above the textarea + `×` |
| `chat-11-attachment-removed.png` | after `×` |
| `chat-12-toast-copy.png` | `Copied CSV` toast |
| `chat-13-lightbox.png`, `chat-14-lightbox-closed.png` | 1440×810 lightbox + caption |
| `chat-20-toast-copy-code.png` | `Copied cache.ts` toast (code `Copy`) |
| `chat-21-composer-draft.png`, `chat-22-composer-multiline.png` | draft (acc send button), Shift+Enter growth |
| `chat-23-mode-plan.png`, `chat-24-mode-ask.png` | Shift+Tab mode cycle + per-mode placeholders |
| `chat-25-running-thinking.png`, `chat-26-running-toolcalls.png`, `chat-36-running-mid.png` | `ARCHONUI working`, tool rows streaming, `Working · N tool calls`, Stop button |
| `chat-27-stopped.png`, `chat-37-stopped.png` | after Stop (`stopped` / `Stopped.`) |
| `chat-28-settings-models.png`, `chat-29-settings-models-tab.png`, `chat-32-models-llama-on.png` | Settings → Models (used to enable llama for the no-effort case) |
| `chat-30-composer-image-attachment.png` | image attachment card (thumbnail, `Browser · 1440×900`) |
| `chat-31-picker-no-effort.png`, `chat-33-picker-no-effort.png` | llama 70B → no EFFORT block, "has no effort levels." |
| `chat-34-toast-copy-code.png` | toast again (duplicate of 20) |
| `chat-35-sent-with-attachments.png`, `chat-38-composer-two-attachments.png`, `chat-39-sent-with-attachments.png` | composer with two cards / sent message with attachment chips |

Step files used: `.qa/design/chat-steps1-17.json` (step lists) and the generators that produced them in
`.qa/design/chat-gen/mk*.py`. Steps 1–2 are ad-hoc probes; 3–17 hold the evals quoted as **[measured]**.
(The picker/moon geometry was cross-checked numerically with `getComputedStyle` evals rather than by eye;
the two `-zoom.png` shots re-scale the app wrapper to 1.45–1.7× purely for legibility — they are the same DOM as the 1× shots.)
