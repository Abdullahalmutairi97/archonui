# Settings / Share / Switcher — visual diff vs prototype (set 2, re-review)

Re-review after the named changes: Harnesses cards draw a 42×42 line icon per harness (no monograms), Pi is a 7th
card, a new **Interface size** row is in Appearance › EFFECTS, the session-list rows changed, and the app draws at
85 % interface scale by default. Every image below was opened with the read tool.

## Capture provenance
- **Ours** (all `set2-*.png`): display `:130`, `ARCHONUI_DATA_DIR=/tmp/archonui-qa-130/data`,
  `ARCHONUI_CONFIG_DIR=/tmp/archonui-qa-130/cfg`, window 1600×1000 at (0,0), binary from this session's `cargo build`.
  `settings.json` was pre-seeded with `"ui_scale": 1.0` **before launch** (src/app/settings.rs:31 `default_ui_scale`
  = 0.85; src/ui/scale.rs), so every ours px = 1 design px. Seeds copied from `.qa/integration/seed/`.
- **Prototype** (`protofix-*.png`, `proto-share.png`): `.qa/review/` shots at fitScale 0.8333 (1600×1000).
  **Proto screenshot px ÷ 0.8333 = design px.** `protofix-keyboard-top`/`-bottom` are the same file (the whole page
  fits). All prototype "top" shots were re-taken with scrollTop forced to 0.
- Fix before/after shots: `set2-keyboard*.png` / `set2-extensions*.png` = before; `set2-after-keyboard*.png` /
  `set2-after-extensions*.png` = after.

---

## 1. General — `set2-general.png`, `set2-general-bottom.png` vs `protofix-general-top.png`, `protofix-general-bottom.png`
1. **Net-new-and-approved:** ours STARTUP has an extra row `Default folder for new sessions · Not set [Choose…]`
   (row height ~74 px at y≈365). Not in the prototype. `src/ui/settings/general.rs:108`.
2. **Net-new-and-approved:** ours DATA has an extra last row `Data folder · /tmp/archonui-qa-130/data [Open]`
   (y≈850). Not in the prototype. `src/ui/settings/general.rs:129`.
3. **LOW — `Run commands in sandbox` default.** ours off (track rgb(47,39,34)); prototype on (track
   rgb(200,30,42)). `src/app/settings.rs:145` (`sandbox: false`) vs the prototype's default `true`. Product default;
   the file is outside this task's edit set.
4. **LOW — Max-steps knob position.** both read `40`; ours knob at 31.6 % of the 240 px track (`SliderRange`
   10–100, `src/app/settings.rs:27 MAX_STEPS`), prototype at 40 % (0–100).
5. **LOW — footer version.** ours `archonui 0.1.0 · Stable`, prototype `archonui 0.9.0 · Stable`
   (`src/ui/settings/mod.rs:430`, `env!("CARGO_PKG_VERSION")`). Data.
6. Data only: nav counts (Models 107 / Tools 0 / Skills 95 / Extensions 22 vs 6/6/7/4) and `Default project`
   (`No projects yet` vs the prototype's 4 projects). Everything else matches, including the previously broken
   slider unfilled track (now the light bar) and all toggle/segmented states.

## 2. Appearance — `set2-appearance.png`, `-presets`, `-background`, `-colors`, `-typography`, `-bottom`
   vs `protofix-appearance-top.png`, `protofix-appearance-mid.png`, `protofix-appearance-bottom.png`
1. **Net-new-and-approved:** ours EFFECTS has an extra first row `Interface size · 100%` (slider, y≈547).
   `src/ui/settings/appearance.rs:833`.
2. **Net-new-and-approved:** ours EFFECTS has an extra `Reduce motion` toggle row between `Tint all sessions`
   and `Message text size` (y≈797). `src/ui/settings/appearance.rs:851`.
3. **LOW — DEFAULTS copy is not visible here but the page now matches**: IDENTITY (name + Sidebar preview + 12-sigil
   icon grid + dashed `+`), PRESETS (same 12 names/case/swatches, red ring + `Active` on Blood Moon), BACKGROUND
   (6 preview tiles, Canvas selected), wallpaper grid (7/row + `+ Upload`), `COLORS · BLOOD MOON` (US spelling now
   matches — `appearance.rs:665`; same 12 swatch values and hex strings), TYPOGRAPHY (3 script pairs + `Minimal`
   6 pairs + `Display font` grid) and LAYOUT (Tiling Dwindle, Gaps 14px, Corner radius 9px, `Reset sizes`) all match.

## 3. Keyboard — `set2-keyboard.png`, `set2-keyboard-bottom.png` vs `protofix-keyboard-top.png`; after fix `set2-after-keyboard.png`
1. **HIGH — section order (FIXED).** Before: ours TILING → APP → COMPOSER; prototype TILING → COMPOSER → APP
   (docs/design/settings.md §4, L1391). `src/ui/settings/keyboard.rs:47`. After: TILING → COMPOSER → APP.
2. **MEDIUM — `Send / new line` split (FIXED).** Before: two rows `Send [⏎]` and `New line [⇧ ⏎]`; prototype one
   row `Send / new line` `[⏎ / ⇧⏎]` under APP. `keyboard.rs:34`. After: one row in APP.
3. **MEDIUM — `Move focus between tiles`.** ours two rebindable rows `Focus next tile [Alt →]` +
   `Focus previous tile [Alt ←]`; prototype one row `Move focus between tiles [Alt ← →]` (3 TILING rows vs our 4).
   `keyboard.rs:12`. Kept: the two rows are separate rebindable actions.
4. **MEDIUM — four extra APP bindings** (`Toggle sidebar Ctrl B`, `Toggle tools panel Ctrl J`, `New session Ctrl N`,
   `Session switcher Ctrl K`) are not in the prototype's 7-row KEYS. `keyboard.rs:21`. Net-new (real shortcuts).
5. **MEDIUM — extra hint line** `Click a shortcut to change it. Escape closes this window.` (y≈190); the prototype
   page has no such line and its badges have no click handler. `keyboard.rs:50`. Net-new (explains rebinding).
6. Badge styling matches the prototype (1px `line2` border, 12px, alt text); `Cycle agent mode: Auto, Plan, Ask`
   `[⇧ Tab]` and `Close dialog [Esc]` match verbatim.

## 4. Harnesses — `set2-harnesses.png`, `set2-harnesses-bottom.png` vs `protofix-harnesses-top.png`, `protofix-harnesses-bottom.png`
1. **MEDIUM — card leading tile content.** both tiles measure 42×42 px (ours y 218–260, x 428–470), but ours draws a
   **line icon** (icon 20 px stroke 1.5) where the prototype draws the **2-letter monogram** (`Cc`, `Cx`, `Gm`, `Oc`,
   `Ai`, `Gs`; L735 `{{pv.i}}`; icons.md has no harness-icon set). `src/ui/settings/harnesses.rs:97,104` +
   `src/ui/composer/icons.rs:222`. Deliberate build change — not reverted.
2. **Net-new-and-approved:** ours has a 7th card `Pi · Minimal terminal agent, any provider` / `Ready` /
   `…/.local/bin/pi · 1.2.0` / `signed in` / `62 models · 62 enabled` / `Test` / `Disable`
   (bottom shot, y≈595–815); the prototype's 6 cards end at Aider/Goose. `src/ui/settings/mod.rs:36 HARNESS_IDS`.
3. No layout difference otherwise: 2-col grid, Path/Auth value boxes with in-box labels, `N models · N enabled` /
   `Uses its own model config`, `Test` + `Disable`/`Install`, dashed `+ Add harness (any CLI that speaks ACP)`.
4. Data: path versions, auth strings, model counts differ (Gemini `Not installed` vs prototype `Ready`).

## 5. Models — `set2-models.png`, `set2-models-bottom.png` vs `protofix-models-top.png`, `protofix-models-bottom.png`
1. **HIGH — the `CONTEXT` column is missing (NOT fixed).** ours header `MODEL | HARNESS | ON`; prototype
   `MODEL | HARNESS | CONTEXT | ON` with 200k/1M/400k/128k. `src/ui/settings/models.rs:245–257`. Blocked: the model
   catalogue (`ModelInfo`, `src/engine/discover.rs:30`) has no context field and the harness parsers don't read one
   (`src/engine/harness/pi.rs:124`, `claude.rs:130`); engine files are outside this task's allowed edit set. Needs a
   decision (add `ModelInfo.context` + per-harness parsing, or drop the column).
2. **LOW — DEFAULTS sub-copy.** ours `…/ Names new sessions with the cheapest model of their harness, such as Haiku
   for Claude Code.` vs prototype `…/ Session names and share summaries use Haiku 4.5.` (`models.rs:53`);
   ours `Prefer long-context variants / New sessions start on the 1M-token version…` vs prototype
   `Switch to 1M-token versions when a session grows past 150k tokens.` (`models.rs:68`).
3. Row pitch is now uniform (the old ratchet — 46 px rows with no description — is gone).
4. Data: model names/descriptions/counts differ; DEFAULTS radios and both toggles match.

## 6. Tools — `set2-tools.png`, `set2-tools-bottom.png` vs `protofix-tools-top.png`, `protofix-tools-bottom.png`
1. **MEDIUM — extra section `TERMINAL AND PREVIEW`** with `Open previews automatically` (toggle on) after
   BUILT-IN TOOLS; the prototype ends at the 7 PERM rows. `src/ui/settings/tools.rs:216`. Net-new (the real
   `auto_open_preview` setting); kept.
2. **LOW — extra explanation line** `Ask: Claude Code in Auto mode asks in ArchonUI before each use…` under the PERM
   rows. `src/ui/settings/tools.rs:32`. Net-new; kept.
3. Otherwise matches: `MCP SERVERS` + `+ Add server`, 7 `Allow | Ask | Off` rows with the same selections, same row
   pitch. Data: ours has no MCP servers → `No MCP servers yet.`, so the prototype's MCP table can't be compared.

## 7. Skills — `set2-skills.png`, `set2-skills-bottom.png` vs `protofix-skills-top.png`
1. **No layout difference found**: `SKILLS · N OF M ON` + `Import folder` + accent `+ New skill`, 2-col cards with
   source badge, toggle, muted description and `/command`. Data: `95 OF 95 ON` vs `7 OF 10 ON`, different skill set.

## 8. Extensions — `set2-extensions.png`, `set2-extensions-bottom.png` vs `protofix-extensions-top.png`, `protofix-extensions-bottom.png`; after fix `set2-after-extensions-bottom.png`
1. **MEDIUM — BROWSE `Install` full-width (FIXED).** Before: 295 px wide inside a ~330 px card (measured x 430–724),
   a red bar across every card. Prototype: inline auto-width (measured 59–63 screenshot px ≈ 71–76 design px).
   `src/ui/settings/extensions.rs:126`. After: inline, 81 px (x 430–510).
2. **LOW — a raw filesystem path** renders as an extension name and wraps to two lines
   (`/home/…/runtime/node_modules/pi-mcp-adapter`); presentation is fine, the value is data (`name_meta`).
3. Data: ours 22 installed (design, pi-*, …) vs the prototype's 4; our BROWSE fixtures have no author/version so the
   `author · version` credit line is absent. Search field, INSTALLED container/separators, sigil tiles, `Remove`
   + toggle all match.

## 9. Share — `set2-share.png` vs `proto-share.png`
1. **MEDIUM — visibility track selected pill.** ours selects `Private` with a neutral `line2` fill (measured
   rgb(60,53,47)); the prototype selects `Team` with the accent fill (rgb(200,30,42)). `src/ui/share.rs:594`
   (`visibility_track` hard-codes `ix == 0` → `line2`, inside `unavailable`). Deliberate (links need a server).
2. **LOW/MEDIUM — extra third EXPORT button `Copy as Markdown`** (full-width below the `Markdown | JSON` pair);
   the prototype has only the two-button row. `src/ui/share.rs:520`. Net-new feature; kept.
3. Geometry matches: 520 design px wide (ours measured 518 px at 1.0; prototype 428 screenshot ÷ 0.8333 = 514
   design), `pop` fill + `line2` border + radius 12, anchored 120 px from the top (`share.rs:552`), 26 px display
   header + ✕, 22 px display title, `Include file diffs` on / `Include terminal output` off.
4. Feature/data-gated: `No link` + `Only you can open this session.` (prototype shows the link + workspace note) and
   an empty PEOPLE list (prototype shows You / Mara Reyes / Jun Lee).

## 10. Ctrl+K switcher — `set2-switcher.png` (no prototype state)
1. **No prototype equivalent.** `grep -i "switcher|palette|ctrl+k|⌘k"` in `design/ArchonUI Agent v3.dc.html` = 0 matches,
   so this is net-new UI; compared against prototype primitives: panel `pop` fill rgb(27,20,17) + `line2` border,
   selected row `line2` rgb(74,67,61), bordered search field, rows reuse the session row (sigil + status dot, title,
   harness indicator, project, relative time), and a footer hint bar (`switcher.rs:393`, net-new, LOW).
2. **MEDIUM — harness indicator inconsistent with the changed session rows (NOT changed).** the switcher draws the
   2-letter badge (`badge_label`, 18 px tile, `line` fill — `src/ui/switcher.rs:254`, matching the prototype's
   row badge, layout.md §2.1), while the sidebar session row now draws the harness **line icon** in a 20 px tile
   (`src/ui/sidebar/sessions.rs:270`). The switcher's comment "as the sidebar draws it" (`switcher.rs:393`) is now
   stale. Left alone: the 2-letter badge is what the prototype specifies while the sidebar's icon change is outside
   this task's edit set, so choosing a direction is a product decision.

---

## Fixed in this pass (before → after)
- **Keyboard section order** (HIGH, diff 3.1–3.2): `src/ui/settings/keyboard.rs` — sections are now
  TILING → COMPOSER → APP and `Send`/`New line` are one `Send / new line [⏎ / ⇧⏎]` row under APP, matching
  settings.md §4. `set2-keyboard.png` → `set2-after-keyboard.png`.
- **Extensions BROWSE `Install` width** (MEDIUM, diff 8.1): `src/ui/settings/extensions.rs` — the button is inline
  (81 px) instead of stretched across the card (295 px). `set2-extensions-bottom.png` → `set2-after-extensions-bottom.png`.

## Not fixed (needs a decision)
- **Models `CONTEXT` column** (HIGH, diff 5.1): needs a context field on `ModelInfo` (`src/engine/discover.rs`) and
  per-harness parsing — outside this task's allowed edit set.
- **Harness identity as line icons** instead of monograms (diff 4.1 + 10.2): app-wide deliberate change in the
  harness cards and the sidebar session rows; reverting would contradict the shipped change, so it is reported, not
  reverted. The switcher still uses the old 2-letter badge.
