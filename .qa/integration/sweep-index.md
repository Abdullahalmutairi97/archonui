# Theme sweep — our app vs the prototype

Generated on Xvfb `:116` with isolated dirs (`/tmp/archonui-qa-116/{cfg,data}`), seeds copied from
`.qa/integration/seed/` into `ARCHONUI_DATA_DIR`. Sources:

* **ours** = `target/debug/archonui --new-ui`, 3 tiles opened by **alt-clicking 3 sidebar rows**
  (`archonui-test-s1`, `archonui-test-s2`, `archonui-test-s3`; verified in `/tmp/archonui-qa-116/cfg/workspace.json`),
  focus on `archonui-test-s1`, sidebar expanded (Sessions), right panel open on the **Git** tab (480 px),
  background style `canvas`, settings written to `ARCHONUI_CONFIG_DIR/settings.json` + app restart per theme.
* **prototype** = `design/ArchonUI Agent v3.dc.html` via `.archonui-dev/shot.js`, 1600×1000 at fitScale 0.8333,
  its default state (tiles `s1,s2,s4`, focus `s1`, right tab IDE, canvas background); themes driven with
  `Alt+T` (prototype `NAMES` order) — verified non-visually by the per-theme average colours below.

Each `sweep-<id>.png` is a `montage` of the two 1600×1000 captures scaled to 50 % (left = ours,
right = prototype). The prototype capture is letterboxed: its 1920×1080 stage renders 1600×900 at `+0+50`,
so the right pane has 50 px black bands top/bottom that our pane does not have. Deltas below therefore
compare our full frame with the prototype's **content crop** (`1600x900+0+50`).

## Contact sheets (one line per theme)

| theme | ours avg RGB | proto content avg RGB | Δ ours−proto (R/G/B) | Δ tiles band | Δ panel band | read |
|---|---|---|---|---|---|---|
| `archonui` | ( 31.6, 31.4, 31.3) | ( 32.1, 33.2, 33.6) | -0.5 / -1.8 / -2.3 | +3.8 / +1.8 / +0.9 | -6.5 / -6.6 / -6.6 | match |
| `blood-moon` | ( 45.2, 33.9, 31.8) | ( 49.3, 36.3, 35.3) | -4.1 / -2.4 / -3.5 | -4.2 / -2.7 / -4.3 | -7.2 / +0.2 / -0.5 | close |
| `moonlight` | ( 35.2, 44.1, 56.9) | ( 36.7, 46.8, 60.3) | -1.5 / -2.7 / -3.4 | +0.7 / -0.9 / -1.3 | -2.9 / -4.6 / -7.0 | close |
| `wreck` | ( 77.3, 59.3, 40.6) | ( 80.8, 62.6, 43.3) | -3.5 / -3.3 / -2.7 | -0.6 / -2.7 / -3.8 | -10.5 / -4.9 / +0.4 | close |
| `citadel` | ( 44.9, 40.5, 42.5) | ( 47.9, 44.6, 46.9) | -3.0 / -4.1 / -4.4 | -0.1 / -3.9 / -6.5 | -8.1 / -4.5 / -0.2 | close |
| `neon` | ( 42.1, 31.5, 59.2) | ( 44.2, 34.1, 64.6) | -2.1 / -2.6 / -5.4 | +1.4 / -1.1 / -1.7 | -5.5 / -1.1 / -9.4 | close |
| `violet` | ( 39.9, 34.5, 62.1) | ( 40.6, 36.6, 65.5) | -0.7 / -2.1 / -3.4 | +1.0 / -1.2 / -2.5 | -3.5 / -2.9 / -8.9 | close |
| `ink` | (189.9,185.1,181.1) | (197.9,193.0,189.5) | -8.0 / -7.9 / -8.4 | -18.9 / -20.2 / -20.8 | +1.3 / +6.1 / +6.0 | **check** |
| `bonfire` | ( 52.2, 59.8, 64.2) | ( 54.7, 62.6, 67.0) | -2.5 / -2.8 / -2.8 | -2.0 / -3.2 / -3.8 | -6.0 / -2.6 / -0.2 | match |
| `hunter` | ( 43.4, 50.2, 63.3) | ( 46.7, 52.8, 66.1) | -3.3 / -2.6 / -2.8 | -4.4 / -4.0 / -3.9 | -4.5 / +0.6 / +0.4 | close |
| `shards` | ( 44.2, 44.6, 48.7) | ( 43.7, 45.6, 50.3) | +0.5 / -1.0 / -1.6 | +3.7 / +1.7 / +1.1 | -3.2 / -3.5 / -4.6 | match |
| `vale` | ( 49.2, 55.2, 46.3) | ( 50.1, 56.2, 47.0) | -0.9 / -1.0 / -0.7 | +1.7 / +0.9 / -0.3 | -6.6 / -4.2 / -0.4 | match |

`tiles band` = x 320–1080, `panel band` = x 1140–1600, both y 60–940 (identical window rows in both
panes). Every theme's global average is within 5/255 of the prototype except `ink` (8/255).

### What the numbers say

* All 12 themes carry the right wallpaper and the right overall tint — no theme fell back to Blood Moon
  and no capture is blank (all 1600×1000, means between 0.10 and 0.75).
* `ink` is the one theme worth an eyeball: our tile band is ~20/255 darker than the prototype's (our
  tiles look like they apply less of the light `tile` wash) while our panel is ~6/255 lighter. Global
  delta is a uniform −8, so it is not a hue rotation.
* The panel band reads consistently darker in ours (up to −10 for `wreck`). That is expected, not a
  token bug: our pane shows the **Git** tab (dark change list) while the prototype pane shows its
  **IDE** tab (code on a lighter surface). Theme tokens should be judged from the sidebar/tiles bands.
* Content is not identical by design: our tiles are s1/s2/s3, the prototype's are s1/s2/s4, and our Git
  panel content is the real `/tmp/iq/proj` repo, so text-level pixels never line up.

## Files

* Contact sheets: `.qa/integration/sweep-<id>.png` (1600×500, ours | prototype, 50 %).
* Ours: `.qa/integration/theme-<id>.png` (12 themes, canvas).
* Ours, Blood Moon background styles: `.qa/integration/bg-<style>.png` (`canvas`, `glass`, `solid`,
  `gaps`, `tinted`, `ambient`).
* Prototype: `.qa/design/sweep-theme-<id>.png` (12).
* Scripts: `.qa/integration/theme-sweep/` (helpers + prototype step list, not source).

## Background styles (Blood Moon, whole-frame averages)

| style | avg RGB | sidebar | tiles |
|---|---|---|---|
| `canvas` | (45.2, 33.9, 31.8) | (42.1, 26.9, 25.7) | (48.6, 39.4, 36.7) |
| `glass` | (47.5, 34.4, 32.8) | (46.9, 31.3, 30.3) | (46.1, 35.2, 33.1) |
| `solid` | (27.8, 20.2, 17.4) | (30.5, 17.6, 15.8) | (31.5, 23.5, 20.3) |
| `gaps` | (31.2, 22.8, 20.2) | (31.8, 19.3, 17.6) | (32.9, 24.8, 21.7) |
| `tinted` | (51.9, 37.1, 34.8) | (43.9, 28.7, 28.4) | (58.8, 43.5, 39.9) |
| `ambient` | (37.1, 27.0, 25.4) | (38.0, 24.6, 23.5) | (37.6, 28.4, 26.7) |

`solid` and `gaps` (wallpaper hidden / heavily covered) are the darkest, `tinted` the brightest; the
ordering matches the formulas in `docs/design/themes.md` §4. No prototype counterpart exists for these
(the prototype is only captured in its default `canvas` style).

## Reproduce

Scripts kept in `.qa/integration/theme-sweep/` (`env.sh`, `sweep-themes.sh`, `proto-themes.json`,
`workspace-3tiles.json` — the workspace state produced by the three alt-clicks).

```
export DISPLAY=:116; Xvfb :116 -screen 0 1600x1000x24 &
mkdir -p /tmp/archonui-qa-116/{cfg,data}; cp .qa/integration/seed/*.json /tmp/archonui-qa-116/data/
# settings.json: {"theme":"<id>","background":"<style>"}; workspace.json: 3 tiles + right_tab git
ARCHONUI_DATA_DIR=/tmp/archonui-qa-116/data ARCHONUI_CONFIG_DIR=/tmp/archonui-qa-116/cfg \
  VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json target/debug/archonui --new-ui
cd .archonui-dev && NODE_PATH=/home/archonminipc/.local/lib/node_modules node shot.js \
  /home/archonminipc/ArchonUI/.qa/design/sweep .qa/integration/theme-sweep/proto-themes.json
montage -geometry 800x500+0+0 -tile 2x1 .qa/integration/theme-<id>.png .qa/design/sweep-theme-<id>.png \
  .qa/integration/sweep-<id>.png
```
