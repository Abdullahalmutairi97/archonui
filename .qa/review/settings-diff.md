# Settings / Share / Switcher — visual diff vs prototype

Reviewer: visual only, **no source edits**. Every image below was opened with the read tool.

## Capture provenance (read this first)
- `cargo build` run 11:39; binary `target/debug/archonui` mtime 11:39. All `q-ours-*.png` were captured 11:44–12:06
  on display `:120`, `ARCHONUI_DATA_DIR=/tmp/archonui-qa-120/data` (seeds copied from `.qa/integration/seed/`).
  The earlier `ours-*.png` (11:41–11:42) are the same build; `q-ours-*` are the clean re-capture (scroll to top, then
  wheel to the real bottom) used for the pairs below.
- The previous `proto-*.png` "top" shots for pages **after** General are wrong: the prototype scroll container keeps
  its scrollTop when the page changes, so `proto-appearance.png` starts mid-presets and `proto-keyboard.png` is
  byte-identical to `proto-keyboard-bottom.png`. I re-captured the prototype with scrollTop forced to 0 before every
  top shot → **`protofix-*.png`** (11:43–11:44). Pairs below use `protofix-*`.
  `proto-share.png` (08:29) is valid and was reused.
- Proto screenshots are at fitScale 0.8333 (1600×1000); ours are 1:1 logical px. All size comparisons below are
  converted (proto screenshot px ÷ 0.8333) or made ratio-wise. Supporting crops: `cmp-slider-gaps.png`,
  `cmp-seg3.png`, `cmp-tog-restore.png`, `cmp-colors1/2.png`, `cmp-skillcard.png`, `cmp-icons.png`.

## 1. General (`q-ours-general-top.png`, `q-ours-general-bottom.png` / `protofix-general-top.png`, `protofix-general-bottom.png`)
1. **HIGH — two rows exist that the prototype does not have.** Ours: `Default folder for new sessions · Not set [Choose…]`
   (STARTUP) and `Data folder · /tmp/archonui-qa-120/data [Open]` (DATA, last row). Prototype `GEN` (design/ArchonUI Agent
   v3.dc.html L1383–1390) STARTUP has only Restore + Default project, and DATA ends at `Clear session history`.
   Likely: `src/ui/settings/general.rs:107` (`folder_row`), `src/ui/settings/general.rs:128` (`data_folder_row`).
2. **HIGH — every slider's unfilled track.** Prototype: a visible **light** bar to the right of the knob
   (measured rgb(239,239,239), ~7px tall) for the full 240px width; ours: a **dark 1px hairline** (theme.line = rgb(47,39,34))
   with no visible unfilled track (pixel scans in `cmp-slider-gaps.png`; see §3.5 below).
   Likely: `src/ui/widgets/slider.rs` (track painted with `theme.line`).
3. **LOW — `Run commands in sandbox` default.** Prototype/docs default **on** (toggle red in `protofix-general-top.png`;
   `docs/design/settings.md:207`), ours default **off** (grey). Source of truth: `src/app/settings.rs:133` (`sandbox: false`).
   May be an intentional product choice (no container in this build); flagging because the doc says on.
4. **Not comparable (data, not UI):** `Default project` — with zero projects ours correctly renders the fallback text
   `No projects yet`; `src/ui/settings/general.rs:85` renders the same segmented control as the prototype once a
   project exists. Proto shows `archonui-core | archonui-web | archonui-cli | ember-gl`. Also the nav counts (ours Models 113 /
   Tools 0 vs proto 6/6) are discovery data.
5. Verified matching: row order/copy of the 13 shared rows, section headers (STARTUP/AGENT/NOTIFICATIONS/UPDATES AND
   PRIVACY/DATA), segmented `Default mode` and `Update channel`, toggles (`cmp-tog-restore.png`), kbd badges, buttons
   (`Export`, `Clear history`), values (`Max steps` 40).

## 2. Appearance (`q-ours-appearance-top.png`, `q-ours-attr-7.png`, `q-ours-appearance-bottom.png` / `protofix-appearance-top.png`, `protofix-appearance-mid.png`, `protofix-appearance-bottom.png`)
1. **MEDIUM — extra row not in the prototype.** Ours EFFECTS ends `Tint all sessions` → **`Reduce motion`**
   ("Collapse the sidebar and panel and reflow tiles instantly.") → `Message text size`. Prototype `APB` EFFECTS
   (L1392) is `Panel blur`, `Wallpaper dim`, `Tint all sessions`, `Message text size` — `grep "Reduce motion"` in the
   prototype = 0. Likely: `src/ui/settings/appearance.rs:846`.
2. **HIGH — same slider track issue** on Gaps, Corner radius, Panel blur, Wallpaper dim, Message text size (see §1.2).
3. **LOW — copy: heading spelling.** Ours `COLOURS · Blood Moon`; prototype `COLORS · {{ themeTitle }}` (L680).
   Likely: `src/ui/settings/appearance.rs:664`.
4. Verified matching: IDENTITY (Name + preview + Icon grid of 13 sigils + dashed `+`, `Use theme icon`), PRESETS
   (12 presets, same names/case, same swatches, red ring + `Active` tag on Blood Moon — `cmp-preset-card` inspected),
   BACKGROUND (6 option tiles **with** preview images, Canvas selected; wallpaper thumbs 7/row + `+ Upload`),
   COLOURS values (Background `#0D0A08`, Session tile `#16100D`, Text `#F3EAD8`, Muted `#BFAE94`, Accent `#C81E2A`, …),
   TYPOGRAPHY (`Minimal` 3 pairs, `Display font` 17, `Interface font` 5), LAYOUT (Tiling, Gaps, Corner radius,
   Tile and panel sizes).
5. Rejected as **not** a difference (checked in `cmp-icons.png`): icon-tile size/ratio — ours 74px tile matches the
   computed 74.1px spec; the apparent glyph-size difference is measurement noise from the 0.8333 downscale.

## 3. Keyboard (`q-ours-keyboard-top.png`, `q-ours-keyboard-bottom.png` / `protofix-keyboard-top.png` (whole page fits: scrollH 772))
1. **HIGH — TILING: one row became two.** Prototype: `Move focus between tiles` `[Alt ← →]`. Ours: `Focus next tile`
   `[Alt →]` and `Focus previous tile` `[Alt ←]`. Likely: `src/ui/settings/keyboard.rs`.
2. **HIGH — section order.** Prototype TILING → COMPOSER → APP. Ours TILING → APP → COMPOSER. Likely: `keyboard.rs`.
3. **MEDIUM — APP has four extra bindings** not in the prototype's `KEYS` (L1389): `Toggle sidebar Ctrl B`,
   `Toggle tools panel Ctrl J`, `New session Ctrl N`, `Session switcher Ctrl K` (all real app shortcuts). `keyboard.rs`.
4. **MEDIUM — extra hint line under the header:** `Click a shortcut to change it. Escape closes this window.`
   Prototype has no such line; its kbd badges are explicitly non-interactive (settings §1.6). `keyboard.rs`.
5. **LOW — `Send / new line` `[⏎ / ⇧⏎]` split into `Send` and `New Line` rows.** `keyboard.rs`.
6. Verified matching: badge styling (border 1px line2, alt text, 12px) and `Cycle agent mode: Auto, Plan, Ask` `[⇧ Tab]`,
   `Close dialog` `[Esc]`.

## 4. Harnesses (`q-ours-harnesses-top.png`, `q-ours-harnesses-bottom.png` / `protofix-harnesses-top.png`, `protofix-harnesses-bottom.png`)
1. **No layout difference found.** Card grid (2 cols), sigil tile, name + subtitle, Ready/Not-installed dot, `Path`/`Auth`
   value boxes with in-box labels, `N models · N enabled`, `Test`, `Disable`, accent `Install` for not-installed, and the
   dashed `+ Add harness (any CLI that speaks ACP)` row all match. Verified matching: 1.
2. **Not comparable (data):** ours has a 7th card (`Pi · Terminal coding agent`) the prototype's 6 harnesses lack; path
   versions, auth strings and model counts differ; proto's Gemini CLI shows `API key from GEMINI_API_KEY` vs ours
   `signed in`.

## 5. Models (`q-ours-models-top.png`, `q-ours-models-bottom.png` / `protofix-models-top.png`, `protofix-models-bottom.png`)
1. **HIGH — the `CONTEXT` column is missing.** Prototype table header is `MODEL | HARNESS | CONTEXT | ON` with values
   200k / 1M / 400k / 128k. Ours is `MODEL | HARNESS | ON`. Likely: `src/ui/settings/models.rs:242` (`table_header`
   has MODEL / HARNESS 200px / ON 64px only) and the row builder below it.
2. **MEDIUM — uneven row heights.** Prototype rows are uniform (measured separator pitch 48/48/49 screenshot px, i.e.
   every row keeps its second line even when the qualifier is empty). Ours: rows whose model has no description are
   ~46px tall while described rows are ~63px (separator deltas 46,64,63,64,… in `q-ours-models-bottom.png`), so the
   table visibly ratchets. Likely: `src/ui/settings/models.rs:196–201` (the subline div collapses when `description` is empty).
3. **LOW — second-line content.** Prototype shows a short qualifier (`Most capable`, `Balanced`, `Fast`, `Open weights`);
   ours shows a full sentence and, for the Pi harness rows, sometimes nothing (data-driven: model catalogue fields).
4. Verified matching: `DEFAULTS` rows (`Default effort` Low/Medium/High/Max with Medium selected), `Use a fast model for
   titles` on, `Prefer long-context variants` off (pixel-verified both), the right-aligned note, radio column, per-row
   harness name and toggle.

## 6. Tools (`q-ours-tools-top.png`, `q-ours-tools-bottom.png` / `protofix-tools-top.png`, `protofix-tools-bottom.png`)
1. **MEDIUM — extra section `TERMINAL AND PREVIEW`** (`Open previews automatically` / "Show a preview when the agent
   writes a file that has one.", toggle on) after BUILT-IN TOOLS. Prototype `TLB` (L1398) is only `BUILT-IN TOOLS`;
   `grep "TERMINAL AND PREVIEW"` = 0. Likely: `src/ui/settings/tools.rs`.
2. **LOW — extra explanatory line** after the built-in rows: "Ask: Claude Code in Auto mode asks in ArchonUI before each
   use. Other harnesses have no per-tool prompt, so they run Ask tools as Allow." Not in the prototype. `tools.rs`.
3. Verified matching: `MCP SERVERS` heading + `+ Add server`, both sections' row pitch, the 7 `PERM` rows with
   `Allow | Ask | Off` segmented (same selected options: Read Allow, Edit Ask, Search Allow, Run Ask, Browser Ask,
   Web Allow, Fetch Allow), `Import`-style outline buttons.
4. **Not comparable (data):** no MCP servers configured → ours shows the empty state `No MCP servers yet.`, so the
   prototype's MCP table (transport chip, `N tools`, status dot, `Reconnect`, toggle) could not be compared.

## 7. Skills (`q-ours-skills-top.png`, `q-ours-skills-bottom.png` / `protofix-skills-top.png`, `protofix-skills-bottom.png`)
1. **No layout difference found.** Header `SKILLS · N OF M ON` + `Import folder` + `+ New skill` (accent), 2-column card
   grid with title, source badge, toggle, muted description, acc2 command, red border when enabled — all match.
2. Rejected as **not** a difference: card-title brightness. A first eyeball pass suggested ours was dimmer; a
   brightest-pixel histogram gives **243 for both** (`cmp-skillcard.png`) — the earlier crop was misaligned, and
   `skills.rs:64` uses `dim = 1.0` when enabled, matching the prototype's `op = on ? 1 : 0.55`.
3. **Not comparable (data):** `95 OF 95 ON` vs `7 OF 10 ON`, and the card set (Personal/Cloudflare skills vs the
   prototype's ten fixtures).

## 8. Extensions (`q-ours-extensions-top.png`, `q-ours-extensions-bottom.png` / `protofix-extensions-top.png`, `protofix-extensions-bottom.png`)
1. **MEDIUM — BROWSE `Install` buttons are full-width in ours.** Prototype: inline auto-width button (measured 64
   screenshot px ≈ 77 design px) at the card's bottom-left. Ours: 295px wide inside a 328px card (full inner width),
   red bar across every card. Likely: `src/ui/settings/extensions.rs:123` (the `Button` is a direct child of the
   flex-column card, so it stretches; the prototype button is inline).
2. **LOW — very long extension name wraps.** The second INSTALLED row shows the raw path
   `/home/…/runtime/node_modules/pi-mcp-adapter` as the name across two lines. Presentation is fine, the value is
   data-driven (`name_meta`, `extensions.rs:190`), but the prototype never shows a path as a name.
3. Verified matching: search field (bordered, radius 8, ⌕ icon), INSTALLED container border + row separators, sigil tile,
   `Remove` + toggle, the `author · version` credit path exists in code (`extensions.rs:191`) — the BROWSE fixtures in
   our test data simply have empty author/version, so no credit line renders (data, not UI).
4. Rejected as not a difference: search-box border colour (ours rgb(74,67,61) vs proto rgb(62,54,49)) — both use
   `line2`; the 12/74 delta is consistent with the downscale of the prototype shot.

## 9. Share (`q-ours-share.png` / `proto-share.png`)
1. **MEDIUM — the visibility track's selected pill is not accent.** Prototype `Private | Team | Public` with the
   selected pill `background: t.acc` (pixel scan: rgb(200,30,42) on the `Team` pill). Ours selects `Private` with a
   neutral `line2` fill rgb(60,53,47) and it is not interactive. Likely: `src/ui/share.rs:594` (`visibility_track`,
   hard-codes `ix == 0` → `theme.line2`). This is deliberate (`unavailable(...)`, module doc L1 says links/visibility
   need a backend) but it is still a visible deviation.
2. **LOW/MEDIUM — extra button `Copy as Markdown`** as a third, full-width EXPORT button; the prototype has only the
   `Markdown | JSON` pair (L559). Likely: `src/ui/share.rs:519`.
3. **Not comparable (data/feature-gated):** `No link` (proto shows `archonui.dev/s/s1-…`), "Only you can open this
   session." (scope = Private), empty PEOPLE list (proto shows You / Mara Reyes / Jun Lee), `Include terminal output`
   off. `unavailable()` is applied to the whole track/link/invite block.
4. Verified matching: 520px dialog, `pop` fill + `line2` border + radius, `Share session` display-font header + ✕,
   title in display font 22px, `Copy` accent-filled button, `Include file diffs` toggle on, `EXPORT` two-button row,
   button radii/padding.

## 10. Ctrl+K switcher (`q-ours-switcher.png`)
1. **No prototype pair exists — the prototype has no command palette.** `grep -i "switcher|palette|ctrl+k|⌘k"` in
   `design/ArchonUI Agent v3.dc.html` returns nothing, so this is net-new UI. Compared instead against prototype primitives:
   panel uses `pop` fill + `line2` border + radius (same family as the settings modal), rows reuse the sidebar session
   row content (sigil, title, harness badge, project, relative time) and the selected row uses the documented `line2`
   fill (`src/ui/switcher.rs:175`). Surface colours pixel-checked: selected row rgb(74,67,61) = `line2`, panel bg
   rgb(27,20,17) = `pop`.
2. **LOW — footer hint bar** (`↑↓ move ⏎ open alt ⏎ new split esc close`) has no prototype counterpart. Prototype
   keyboard hints are plain muted text in the composer, so the style is consistent; only the presence is net-new.
   `src/ui/switcher.rs`.
3. Rejected as not a difference: no accent-red selection bar — the red I first saw at the panel edge is the wallpaper
   peeking past the panel, not a row border (pixel scan at y=241).

## High items (act on these first)
- **H1** Keyboard page: TILING row split, section order (APP before COMPOSER), 4 extra APP bindings, extra hint line,
  `Send / new line` split — `src/ui/settings/keyboard.rs`.
- **H2** General: `Default folder for new sessions` and `Data folder` rows are not in the prototype —
  `src/ui/settings/general.rs:107,128`.
- **H3** Slider unfilled track is invisible/dark instead of the prototype's light bar (6 sliders) —
  `src/ui/widgets/slider.rs`; `docs/design/settings.md:179` describes a light track.
- **H4** Models table is missing the `CONTEXT` column — `src/ui/settings/models.rs:242`.
- **H5** Appearance: `Reduce motion` row is not in the prototype — `src/ui/settings/appearance.rs:846`.

## Notes for follow-up (not high, but real)
- Extensions `Install` buttons stretch full-width (`extensions.rs:123`).
- Models rows with an empty description collapse to ~46px (`models.rs:196`), breaking the prototype's uniform pitch.
- Tools adds `TERMINAL AND PREVIEW` + an Ask note (`tools.rs`).
- Share: neutral selected pill and the extra `Copy as Markdown` (`share.rs:594,519`).
- Copy: `COLOURS` → `COLORS` (`appearance.rs:664`).
- Defaults: `sandbox` off vs the documented on (`src/app/settings.rs:133` vs `docs/design/settings.md:207`).
- Cleanup: my app (ARCHONUI_DATA_DIR=/tmp/archonui-qa-120/data) and Xvfb `:120` were killed via /proc environ checks;
  `tmux -L archonui-qa-120 kill-server` (no server present). Other workers' archonui instances/Xvfb were not touched.
- Evidence files added: `q-ours-*.png` (12:06 latest), `protofix-*.png`, `cmp-*.png`. The earlier `ours-*.png` from the
  same build were left in place; the mis-scrolled `proto-*.png` top shots were left in place but are not cited above.
