# Shared brief: ArchonUI UI port workers

Project: ArchonUI, a GPUI 0.2.2 desktop app (Rust) at /home/archonminipc/ArchonUI that drives AI coding-agent CLIs.
We are replacing the UI with the design in `design/ArchonUI Agent v3.dc.html`. Read `docs/UI_PORT_PLAN.md` first.
Exact design values, with prototype line refs, are in `docs/design/*.md` — use them verbatim (colours via theme
tokens, px sizes, radii, fonts, timings). The prototype is authored at 1 design px = 1 logical px; do NOT apply the
prototype's `fitScale`. Measured screenshots in `.qa/design/*.png` were taken at 0.8333 scale.

## The new UI's architecture (already in place — read these files)
- `src/ui/shell.rs`: `Shell`, the root view. Fields you use: `state: AppState` (sessions, projects, workspace,
  settings — `src/app/state.rs`, `src/app/settings.rs`, `src/app/workspace.rs`), `theme: Theme`
  (`src/ui/theme.rs`: tokens bg, fg, muted, faint, acc, acc2, deep, alt, on, panel, pop, tile, line, line2, soft,
  border, …, fonts `display_font`, `ui_font`, `code_font`, `band(i, focused)`), `agents`, `caps`, `toasts`, `focus`,
  and one UI-state struct per region (`sidebar: SidebarUi`, `tiles: TilesUi`, `panel: PanelUi`,
  `composers: ComposerUi`, `stream: StreamUi`). Helpers: `regions(window) -> Regions {window, left, right, centre,
  gap}` (animated sidebar/panel widths), `widths_changed(instant, cx)` (call after changing
  `state.workspace.left_open/right_open/right_width`), `save(cx)` (persists settings/workspace/projects, coalesced),
  `refresh_theme(cx)`, `toast(msg, cx)`, `effects()`, `default_folder()`, `rescan(cx)`.
  `render` draws: wallpaper → `render_sidebar` → `render_tiles` → `render_panel` → `render_overlays` → toasts.
  Keys: `on_key` asks `composer_key`, `tiles_key`, `panel_key`, `sidebar_key` (first that returns true wins), then
  `global_key`.
- Each region is `impl Shell` in its own file: `sidebar.rs`, `tiles.rs`, `rpanel.rs`, `composer.rs`, `stream.rs`.
  Stubs exist with fixed signatures — keep the signatures (others call them). You may add fields to your own
  `*Ui` struct and add new files under your own area (e.g. `src/ui/stream/…` → if you turn `stream.rs` into a
  directory module, keep `pub(super)` items reachable).
- `src/ui/glass.rs`: `Backdrop` (passed to region renders), `glass_surface(backdrop, rect, radius, fill, border,
  blur_px)` → rounded blurred-wallpaper surface aligned with the wallpaper (use it for sidebars, panel, tiles,
  popovers that sit over the wallpaper). Surface fills/blur per background style: `state.settings.background
  .surfaces(&theme, self.effects())` and `.tile(&theme, effects, TileState{focused, empty, tint_all})` in theme.rs.
- `src/ui/widgets/*`: Button, Toggle, Segmented, slider, Kbd, Section/SettingRow, Popover, Toasts, TextInput
  (`text_input(&entity, theme)`, events Changed/Submit/Blur, IME-capable), AttachmentCard/badge_for.
- `src/ui/sigil.rs`: `sigil_element(index, size, color)`, `status_dot`. `src/ui/tiling.rs`: `Layout` (in
  `state.workspace.layout`): compute rects/handles, split/close/open/focus/drag/reset. `src/ui/anim.rs`: Animator,
  Transition consts, `tick(window, running)`, `reduced_motion()`.
- Backend: `src/app/state.rs` `AppState` methods (new_session, open_session…, send/apply/stop, sections, projects),
  `src/engine` (harnesses, Mode, discovery `Capabilities` with models + `efforts(model)`), `src/store.rs` (Session,
  Message{role,text,attachments,tools,thought_secs}).
- The OLD UI (`src/chat.rs`, `src/chat/*`) stays untouched and working; the new UI runs with `--new-ui`.

## Rules
- THE PROTOTYPE IS THE DESIGN — DO NOT REDESIGN. The user wants the UI exactly as in `design/ArchonUI Agent v3.dc.html`:
  same layout, sizes, colours, fonts, copy, controls and motion. Never add, remove, move, restyle or reword visible
  UI on your own judgement. Fixes may only bring our UI CLOSER to the prototype. If a needed feature has no place in
  the prototype (e.g. an error message), use the prototype's existing patterns (toast, inline muted text) and keep it
  minimal. Net-new UI that the prototype lacks but a feature truly needs is allowed (user-approved), but
  it must be built from the prototype's existing components, theme tokens, fonts, sizes and spacing so it looks native
  and doesn't break the style. Never restyle or rearrange anything the prototype does show.
- Edit only the files your task names. If you truly need a change elsewhere (e.g. a new AppState method), make the
  smallest additive change and say so in your report. Never edit `src/ui/shell.rs` beyond what your task allows.
- Concise `///` docs, no comment noise. No new warnings in your files. Don't run `cargo update`.
- Other developers build in parallel; if the build fails only in files you don't own, wait a minute and retry.
- Test visually on your OWN virtual display (pick an unused number `:N` from your task) with isolated data:
  ```
  export DISPLAY=:N; (setsid Xvfb :N -screen 0 1600x1000x24 >/dev/null 2>&1 &); sleep 2
  mkdir -p /tmp/archonui-qa-N/{cfg,data}
  (setsid env -u WAYLAND_DISPLAY DISPLAY=:N ARCHONUI_DATA_DIR=/tmp/archonui-qa-N/data ARCHONUI_CONFIG_DIR=/tmp/archonui-qa-N/cfg ARCHONUI_TMUX_SOCKET=archonui-qa-N VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json /home/archonminipc/ArchonUI/target/debug/archonui --new-ui >/tmp/archonui-qa-N/log 2>&1 &); sleep 10
  xdotool mousemove 500 400; sleep 0.3; xdotool mousemove 505 405; sleep 0.5; import -window root /home/archonminipc/ArchonUI/.qa/<name>.png
  ```
  Always wiggle the mouse before a screenshot (the virtual display may skip a repaint). Compare against the
  prototype: render it with `cd /home/archonminipc/ArchonUI/.archonui-dev && NODE_PATH=/home/archonminipc/.local/lib/node_modules node shot.js /home/archonminipc/ArchonUI/.qa/<prefix> steps.json`
  (steps: `["click",css]`, `["clickText",text]`, `["clickXY",x,y]`, `["key",k]`, `["type",t]`, `["wait",ms]`,
  `["shot",name]`, `["eval",js]`; viewport 1600×1000 at fitScale 0.8333, so prototype px = screenshot px / 0.8333).
  Before relaunching after a rebuild, kill only YOUR app (find its PID via `/proc/<pid>/environ` containing
  `ARCHONUI_DATA_DIR=/tmp/archonui-qa-N`). At the end kill only your app and your Xvfb, and `tmux -L archonui-qa-N kill-server`.
  NEVER kill other archonui processes (the user's real app runs without ARCHONUI_DATA_DIR) or other Xvfb displays.
  NEVER use `pkill -f` (it can match your own shell); use the PID checks above.
- To get sessions to look at, copy the seeds: `cp /home/archonminipc/ArchonUI/.qa/integration/seed/*.json /tmp/archonui-qa-N/data/`
  (ARCHONUI_DATA_DIR IS the sessions folder — no `sessions/` subfolder). Never read or modify the user's real
  `~/.local/share/archonui` files.
- If your model can't view images: still save screenshots (the supervisor reviews them), and verify behaviour by
  other means — file contents on disk, the app's stderr log, `xdotool getwindowgeometry`, pixel colours via
  `convert shot.png -crop 1x1+X+Y txt:-`, and unit tests. Say in your report which checks were visual-only.
- Report (under 30 lines): what you built, files changed, screenshots compared, anything not matching the prototype
  and why, open issues.
