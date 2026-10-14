# Motion report — new UI vs `design/ArchonUI Agent v3.dc.html`

Sources: `docs/design/layout.md` §6 (grep `transition:`/`@keyframes`/`animation:` in the prototype),
`docs/design/themes.md` §3.7, `docs/design/settings.md` §1.6, `docs/design/chat.md` §5.
The prototype has **no `@keyframes`, no `animation-*`, no `@media`, no `prefers-reduced-motion`** — every
effect is a CSS transition. Infra: `src/ui/anim.rs` (`Transition`/`Easing`/`Animator`/`tick`/`reduced_motion`,
plus the new `animated()` element that keeps an `Animator` in GPUI element state).

## 1. Table — prototype transition → our code → verdict

| # | animated thing | property | proto dur/easing | our code | verdict |
|---|---|---|---|---|---|
| 1 | wallpaper layer | `opacity` | 0.4 s `ease` | `anim.rs` `WALLPAPER_OPACITY`; `glass.rs` `wallpaper_layer` via `anim::animated` | matches |
| 1 | wallpaper layer | `left`, `width` | 0.3 s `ease` | not used | n/a — dead in the prototype: `wx:0, ww:1920` are constants (themes.md §3.7 note 7) and our layer is always the full canvas |
| 2 | left sidebar collapse/expand | `width` | 0.3 s `ease` | `SIDEBAR_COLLAPSE`; `shell.rs` `left_w` (`widths_changed`, `regions`) | matches |
| 2 | left sidebar | `background` | 0.3 s `ease` | `SIDEBAR_COLLAPSE`; `sidebar.rs` `SidebarUi::fill` | matches |
| 3 | right panel collapse/expand | `width`, `left` | 0.3 s `ease` | `PANEL_COLLAPSE`; `shell.rs` `right_w` (`x = ww - rw`, so `left` follows) | matches |
| 3 | right panel | `background` | 0.3 s `ease` | `PANEL_COLLAPSE`; `rpanel.rs` `PanelUi::fill` | matches |
| 3b | right panel **while dragging** its edge | `background` only (`sideTr = 'background .3s'`) | 0.3 s `ease` | `widths_changed(instant=true)` snaps `width`/`left` (no transition) — the prototype's intent | matches (the fill value cannot change during a drag, so this is a no-op in both) |
| 4 | tile split/merge/reflow | `left`, `top`, `width`, `height` | 0.32 s `cubic-bezier(.2,.8,.2,1)` | `TILE_REFLOW`; `tiles.rs` `TilesUi::rects` (`Lerp for Rect`) | matches |
| 4b | tile while dragging a split | none | — | `INSTANT` re-seeds the rect each frame in `render_tiles` | matches |
| 5 | resize-handle grip | `background` | 0.15 s `ease` | `GRIP`; `tiles.rs` handle grips, `rpanel.rs` panel-edge grip | matches |
| 6 | effort slider fill (gradient bar) | `width` | 0.25 s `ease` | `EFFORT_FILL`; `composer/picker.rs` `effort_block` fill bar | matches |
| 7 | effort level moon buttons | `filter`, `color` | 0.2 s `ease` | `EFFORT_NODE`; `composer/picker.rs` moons (colour `faint↔acc2`, glow `acc` alpha) | matches |
| 8 | toggle switch — track | `background` | 0.2 s `ease` | `TOGGLE`; `widgets/toggle.rs` (`Hsla::lerp(faint, acc)`) | matches |
| 8 | toggle switch — knob | `left` | 0.2 s `ease` | `TOGGLE`; `widgets/toggle.rs` (`2 + 16t` px, i.e. 2↔18) | matches |
| 9 | hover states (rows, buttons, chips) | `background`/`color`/`border-color` | instant (0 s) | plain `.hover(...)` / `.group_hover(...)`, no animator | matches |
| 10 | popovers/pickers appearance | none | — | `POPOVER` = `INSTANT`; they mount with no transition | matches |
| — | focus ring / tile blur on focus | instant | — | ring colour picked from `theme.border`, no animator | matches |
| — | settings modal / share modal open | none | — | mounted with no transition | matches |

### Timer-driven (layout.md §6.1)

| thing | prototype | ours | verdict |
|---|---|---|---|
| toast lifetime | 1800 ms | `widgets/toast.rs` `TOAST_MS = 1800`, `background_executor().timer` | matches |
| "Copied" label resets | 1500 ms on the Share modal's Copy button | our Share link row's Copy button is inert (no link to copy); the real copy paths ("Copy as Markdown", code-block copy) use the 1800 ms toast instead | not applicable today — see §3 |
| message list force-scroll | next `requestAnimationFrame` | `stream` scroll-to-bottom on change | matches |
| simulated stream timings, thought-time formula | 500+450i / 700+450n ms, `round(2+frac*29)` s | `--stream-demo` pacing in `stream/demo.rs`; `thought_secs` from the harness | matches (demo-only in the prototype) |

### Reduced motion

The prototype has none; per the decision in `UI_PORT_PLAN.md` we honour the OS/env (`ARCHONUI_REDUCED_MOTION=1`,
GNOME `enable-animations`) and the Settings ▸ Appearance toggle (`settings.reduce_motion` →
`anim::set_reduced_motion`, `shell.rs:87`). Every path above goes through `Animator`, whose
`set_target` zeroes the duration when reduced motion is on — including the new `animated()` element,
which is built on the same `Animator`. `settings/appearance.rs` shows the resolved value.

## 2. What changed in this task

Only the properties the prototype animates; no static look was touched (a settled `Animator` is exactly the
target value, and every animated element starts settled).

- `src/ui/anim.rs` — new `animated(id, transition, target, build) -> AnyElement`: an `Element` that keeps
  an `Animator<T>` in GPUI element state (`window.with_element_state`, the same pattern gpui's own
  `with_animation` uses), retargets it every frame, runs the builder with the value on screen and asks for
  the next frame while it is still moving. Lets `RenderOnce` widgets animate a property without owning state.
- `src/ui/glass.rs` — wallpaper layer opacity (0.4 s) now eases on a background-style change.
- `src/ui/sidebar.rs` — `SidebarUi::fill` eases the sidebar surface fill (0.3 s).
- `src/ui/rpanel.rs` — `PanelUi::fill` eases the panel surface fill (0.3 s); the panel-edge grip fades
  `line2 → acc2` over 0.15 s.
- `src/ui/tiles.rs` — the split-handle grips fade `line2 → acc2` over 0.15 s.
- `src/ui/widgets/toggle.rs` — track `background` and knob `left` over 0.2 s (the knob's colour still snaps,
  as the prototype's `tg.knob` has no transition).
- `src/ui/composer/picker.rs` — the effort fill bar's `width` over 0.25 s and the effort moons' colour +
  glow over 0.2 s.

## 3. Not done / open

- The Share modal's Copy button showing "Copied" for 1500 ms is unreachable in our port: the share link row
  is a placeholder ("No link") with no handler, and the working copy actions toast instead. Wiring that
  button is a Share-dialog feature gap, not motion; left to the Share/settings workers.
- `TILE_REFLOW` is still `Transition::new(0.32, CubicBezier(0.2, 0.8, 0.2, 1.0))`, matching
  `cubic-bezier(.2,.8,.2,1)`.
- GPUI has no per-property transitions on one element; where the prototype animates two properties of the
  same element with the same duration/easing (toggle track+knob, moon colour+filter) we drive both from
  one `Animator`, which is visually identical.
