# ArchonUI UI port plan

Port of `design/ArchonUI Agent v3.dc.html` (the visual and behavioural spec) into the native GPUI app.
Exact values are extracted, with prototype line references, in `docs/design/`:

| doc | covers |
|---|---|
| `layout.md` | window shell, sidebars, tiles, tiling algorithm, glass, motion, type/radii/spacing scales |
| `themes.md` / `themes.json` | 12 themes, token pipeline, 6 background styles, colour overrides, fonts |
| `icons.md` / `icons.json` | the 100 session sigils, generator, assignment, render sites, wordmark, identity icons |
| `chat.md` | message stream, tool-call rows, code/chart/image blocks, attachments, composer, model picker |
| `settings.md` | the settings modal and all 8 pages |
| `tools-and-behaviour.md` | right tools panel tabs, keyboard, tiling behaviour, the "no dead UI" control inventory (§C) |

Status keys: **Done** · **Done (disabled: <reason>)** (an honestly disabled control that says why) · **Partial: <what's missing>**. The matrix is updated as work lands. The module names in the tables are the plan's original paths; the landed code lives under `src/ui/tabs/`, `src/ui/composer/`, `src/ui/settings/`, `src/ui/stream/`, plus `src/ui/tiles.rs`, `src/ui/rpanel.rs`, `src/ui/sidebar.rs`, `src/ui/shell.rs` and `src/app/`.

## Framework decision

Keep **GPUI 0.2.2**. It reaches the design:

- Frameless window: `WindowDecorations::Client`, no titlebar, `start_window_move` / `start_window_resize`, and
  real minimise / maximise / close (the prototype only toasts for these).
- Transparency: `WindowBackgroundAppearance::{Transparent, Blurred}`. Blurred uses the compositor where it exists
  (macOS vibrancy, Windows, KDE's blur protocol on Wayland).
- Custom text: `StyledText` runs, embedded fonts (`App::text_system().add_fonts`), `TextLayout` hit-testing.
- Vector icons: `canvas` + `PathBuilder` for the sigils.
- Animation: per-frame `request_animation_frame` with our own easing, matching the prototype's durations/curves.

### Limitations and the native alternatives used

| effect in the prototype | limitation | what we do |
|---|---|---|
| `backdrop-filter: blur(...)` on sidebars and tiles | GPUI has no backdrop filter, and GNOME (the dev machine) has no compositor blur protocol | In this design the only thing behind a glass surface is the wallpaper. We pre-blur the wallpaper (image crate, background thread, cached per radius) and draw the blurred copy clipped to each glass surface, then the translucent fill on top. This is pixel-equivalent to a backdrop blur here, and works on every OS. Where the compositor offers blur we still draw our own, so the look is identical everywhere. |
| embedded web page (`wry` suggested) | wry/WebKit can't be embedded inside a GPUI window on Wayland | Keep the existing Browser: headless Chrome over the DevTools protocol, frames streamed into the panel, input forwarded. Works on Linux/macOS/Windows wherever Chrome/Chromium exists. |
| CSS `transition` on any property | no CSS engine | an `Animator` helper (value, target, duration, cubic-bezier) driven by `request_animation_frame`; the reduced-motion setting (OS on macOS/Windows, `gtk-enable-animations` / a settings toggle on Linux) sets durations to 0 |
| window icon at runtime (custom identity icon) | GPUI 0.2.2 exposes no runtime window-icon API on Linux; Wayland has no client icon protocol in GPUI | the identity icon is used everywhere inside the app (sidebar, wordmark, empty states) and written into the `.desktop` file icon on Linux (`~/.local/share/icons` + desktop entry) so the dock/taskbar shows it after relaunch; macOS/Windows dock icon is set where GPUI allows, else listed as a gap |

## Things the prototype doesn't have, which the brief requires (net-new, in the prototype's visual language)

1. **Persistence**: the prototype stores nothing. We persist everything in §6 of the brief (see "Persistence" below).
2. **Projects create / rename / delete** and a real folder picker ("+ Open folder" is a stub in the prototype).
3. **Context menu** on sessions (open in new split, rename, move to project, delete) and **modifier-click** (Alt+click
   opens a session in a new split). Styled like the prototype's popovers (`t.pop`, `t.line2` border, radius 10, shadow
   `0 14px 40px rgba(0,0,0,.45)`).
4. **Keyboard rebinding with conflict detection** (the prototype's Keyboard page is static keycaps).
5. **Git diff view** (the prototype has none; ArchonUI already has one): kept, restyled with theme tokens.
6. **Session switcher** (Ctrl+K style palette over all sessions).
7. Prototype stubs made real: window controls, browser back/forward/reload, "+ Open folder", Export settings,
   Markdown/JSON export, "+ Add harness", "+ Add server", "Import folder", "+ New skill".

## Prototype mock data replaced by real data

The prototype's lists are sample data. Copying them would ship dead UI, so each page keeps the prototype's layout and
shows the machine's real state:

| page | prototype mock | real source |
|---|---|---|
| Harnesses | 6 cards with fake versions/paths | `engine::harness` registry: detection, versions, auth status, test turn |
| Models | 10-row sample table | discovered models per installed harness (`engine::discover`), with real effort levels and mode support |
| Tools › MCP servers | GitHub, Linear, Postgres, … | ArchonUI-managed MCP server list (stored in ArchonUI's config) plus servers imported from each harness's own config; passed to harnesses at launch |
| Tools › built-in tools | 7 Allow/Ask/Off rows | mapped to each harness's real tool allow/deny flags (`docs/HARNESS_MAPPING.md`); "Ask" uses the approval prompt (see below) |
| Skills | 10 sample skills | `SKILL.md` folders found on disk (`~/.claude/skills`, `~/.pi/agent/skills`, `~/.agents/skills`, project `.claude/skills`) |
| Extensions | Vim mode, WakaTime, … | real installable add-ons: Claude Code plugins, Pi packages, OpenCode plugins (list / install / remove / enable via each CLI) |
| Projects | archonui-core, archonui-web, … | user-created projects bound to folders |
| Share | team members, public links | Markdown / JSON export are real; links, invites and visibility need a server ArchonUI doesn't have, so they're shown disabled with a tooltip explaining why |

## Module structure

```
src/
  main.rs            entry, headless test commands, window creation (frameless, transparent)
  app/               state store + event bus (UI talks to this, never to processes)
    state.rs         AppState: sessions, projects, layout, settings, live turns
    events.rs        AppEvent enum; harness events in, UI intents out
    persist.rs       load/save of state + settings (atomic writes), migrations from the old format
  engine/            harnesses (trait), detection, discovery, turns, attachments (existing, extended)
  ui/
    theme.rs fonts.rs sigil.rs tiling.rs     (done: pure logic + drawing helpers)
    anim.rs          Animator + easing + reduced motion
    glass.rs         wallpaper loading, pre-blur cache, glass surface element
    widgets/         buttons, toggle, segmented, slider, popover, toast, text input (single/multi-line), scroll
    shell.rs         root view: wallpaper, left sidebar, tile area, right panel, overlays, keyboard map
    sidebar.rs       sessions list, projects, collapsed rail with bands
    tile.rs          one session tile: header, message stream, composer
    stream/          message rendering: markdown, tool rows, code/chart/image blocks, lightbox
    composer.rs      multiline input, attachments, model/mode/effort picker
    panel/           right tools panel: browser.rs ide.rs files.rs git.rs terminal.rs
    settings/        modal shell + 8 pages
    share.rs         share/export modal
  (existing backend kept: editor/, workspace.rs, terminal.rs, preview.rs, ide.rs, git.rs, store.rs, thumbs.rs)
```

The old `chat.rs` UI is replaced. Its backend logic (sending turns, applying events, attachments, discovery) moves
into `app/` so the new UI reuses it.

## Persistence

- Settings: `$XDG_CONFIG_HOME/archonui/settings.json` (macOS `~/Library/Application Support/archonui`, Windows
  `%APPDATA%\archonui`), migrated from the old `~/.local/share/archonui/settings.json`.
- Sessions (messages, harness, model, mode, effort, icon, project, timestamps): one JSON file each under the data
  dir, as today, extended.
- Projects, tile layout per workspace, sidebar/panel open state and widths, active tool tab, theme, custom colours,
  font choice, wallpaper/background, layout sliders, identity (name + icon), keybindings, harness/model/tool/skill/
  extension config: in settings.json.

## Feature matrix

Legend: Exists in Rust? = before this port. Status = now.

### Window and global look

| feature | exists? | Rust module | work | status |
|---|---|---|---|---|
| frameless window, no top bar | no | main.rs, ui/shell.rs | client decorations, drag to move, edge resize | Done |
| window controls in the right panel header / rail bottom | no | ui/panel | real minimise / maximise / close | Done |
| wallpaper canvas, `inset:-60px` bleed | no | ui/glass.rs | load per theme or user file, cover-fit | Done |
| glass surfaces (sidebars, tiles, popovers) with blur | no | ui/glass.rs | pre-blurred wallpaper under translucent fills | Done |
| focused tile nearly transparent, unfocused dimmer | no | ui/tile.rs | Canvas style values (themes.md §4.1, layout.md §3.3) | Done |
| 12 themes, Blood Moon default, live switch | no (one dark palette) | ui/theme.rs | tokens, pipeline, bands | Done |
| 6 background styles | no | ui/theme.rs, ui/glass.rs | per-surface values | Done |
| fonts embedded and live-switchable | no | ui/fonts.rs, assets/fonts | 29 families, 86 faces | Done |
| motion: durations/easing per transition, reduced motion | no | ui/anim.rs | Animator + reduced-motion | Done |
| toasts (1800 ms) | no | ui/widgets | toast host | Done |

### Left sidebar

| feature | exists? | Rust module | work | status |
|---|---|---|---|---|
| expanded 300 px / collapsed 52 px, animated, persisted | partial (fixed sidebar) | ui/sidebar.rs | rebuild to spec | Done |
| brand: logo + app name in display font | no | ui/sidebar.rs | identity | Done |
| Sessions / Projects segmented control | no | ui/sidebar.rs | | Done |
| New session button (Alt ⏎ hint) | partial ("New chat with") | ui/sidebar.rs | uses default harness/project | Done |
| search sessions | no | ui/sidebar.rs | filter by title/content | Done |
| TODAY / EARLIER grouping, relative time auto-updating | partial (ago) | ui/sidebar.rs | prototype rules | Done |
| session row: sigil, title, harness badge, time, status dot | partial | ui/sidebar.rs | | Done |
| click swaps into focused tile; Alt+click / menu opens in new split | no | ui/sidebar.rs, ui/tiling.rs | | Done |
| session context menu (open in split, rename, move, delete) | partial (delete ×) | ui/sidebar.rs | net-new | Done |
| projects: list, create, rename, delete, bound to folders | no | app/state.rs, ui/sidebar.rs | net-new + folder picker | Done |
| collapsed rail: sigils on rotating 6-colour bands | no | ui/sidebar.rs | band formula | Done |
| footer: Settings, Share | partial (settings) | ui/sidebar.rs | | Done |

### Tiles

| feature | exists? | Rust module | work | status |
|---|---|---|---|---|
| dwindle / master tiling, ratios, 6-tile cap, min size | no | ui/tiling.rs | | Done |
| tile chrome: sigil, display-font title, subtitle, + / × | no | ui/tile.rs | | Done |
| drag gaps to resize, double-click reset, panel edge drag | partial (panel edge) | ui/shell.rs | handles + grips | Done |
| one focused tile; click / Alt+arrows / composer focus | no | ui/shell.rs | | Done |
| layout persisted per workspace | no | app/persist.rs | | Done |
| session switcher | no | ui/shell.rs | net-new palette | Done |

### Chat view

| feature | exists? | Rust module | work | status |
|---|---|---|---|---|
| user / agent message styles | partial (bubbles) | ui/stream | restyle | Done |
| live token streaming | yes | engine | keep | Done |
| "ARCHONUI · thought for Ns / working" header | no | ui/stream, engine | timing per reply | Done |
| tool-call rows (read/grep/edit/run, path, +/- counts, results) | partial (status text) | engine/parse, ui/stream | structured tool events per harness | Done |
| markdown rendering | no | ui/stream | pulldown-cmark | Done |
| code blocks with language badge, path, Copy, line numbers, highlighting | no | ui/stream | reuse editor highlighter | Done |
| chart block (```chart JSON) + Copy CSV | no | ui/stream | bar chart per prototype | Done |
| image block (paths/URLs/markdown images) + Open / Attach + lightbox | partial (attachment thumbnails) | ui/stream | | Done |
| attachments: drag-drop, picker, paste, type-badge cards, remove | partial (chips) | ui/composer.rs | badge colours per type | Done |
| attach popover (repo files, browser screenshot, terminal output, upload) | no | ui/composer.rs | real sources | Done |
| multiline composer, Enter / Shift+Enter / Shift+Tab, per-mode placeholder | partial (single line) | ui/widgets, ui/composer.rs | real text input | Done |
| send / stop | yes | ui/composer.rs | restyle | Done |
| model picker: model list, Auto/Plan/Ask, effort moons (2–6) | partial (separate palettes) | ui/composer.rs | single popover | Partial: the rows show the model's own description (or the agent's name) where the prototype shows a `harness · ctx` sub-line |
| modes mapped to real harness flags | yes | engine/harness | `docs/HARNESS_MAPPING.md` | Done |

### Session icons

| feature | exists? | Rust module | work | status |
|---|---|---|---|---|
| 100 sigils, exact generator | no | ui/sigil.rs | | Done |
| deterministic, collision-free assignment, stored with the session | no | ui/sigil.rs, store.rs | | Done |
| tinted by state, in sidebar / rail / tile header | no | ui/sidebar.rs, ui/tile.rs | | Done |

### Right tools panel

| feature | exists? | Rust module | work | status |
|---|---|---|---|---|
| 480 px (360–820) / 52 px rail, tabs, collapse, persisted | partial | ui/panel | rebuild | Done |
| Browser: URL bar, back/forward/reload, agent can open URLs | yes (headless Chrome) | preview.rs, ui/panel/browser.rs | restyle | Done |
| IDE: tabs, breadcrumb, highlighting, change bands, status bar, save | yes (built-in editor) | editor/, ui/panel/ide.rs | restyle | Done |
| Files: tree, open, rename, create, delete, live changes, filter, footer actions | partial (tree, live) | workspace.rs, ui/panel/files.rs | rename/create/delete, filter, footer | Done |
| Git: branch, changes, stage/unstage, commit, push, log, diff | yes | git.rs, ui/panel/git.rs | restyle, prototype layout | Done |
| Terminal: PTY, themed, resize, scrollback | yes (tmux) | terminal.rs, ui/panel/terminal.rs | theme colours | Partial: the surface follows the theme, but the ANSI colours come from xterm's palette, not the theme's |

### Settings (modal 1320×880, nav + 8 pages)

| feature | exists? | Rust module | work | status |
|---|---|---|---|---|
| modal shell, nav with counts, close on backdrop / Esc | partial (page) | ui/settings | rebuild | Done |
| General: restore, default project, default mode, sandbox, net ask, show tools, max steps, notify, sound, update channel, analytics, export, clear history | partial | ui/settings/general.rs | wire each to real behaviour (see notes) | Done (disabled: the update channel and analytics rows have no backend in this build and each says so) |
| Appearance › Identity: app name, preset icons, upload | no | ui/settings/appearance.rs | used everywhere | Done |
| Appearance › presets, background styles, wallpaper picker | no | | | Done |
| Appearance › per-colour customisation saved as custom theme | no | | | Done |
| Appearance › typography (pairings, minimal, display, interface) | no | | | Done |
| Appearance › layout & effects sliders (layout mode, gaps, radius, reset sizes, blur, dim, tint all, text size) | no | | | Done |
| Keyboard: all shortcuts, rebindable, conflict detection | no | ui/settings/keyboard.rs, ui/keymap.rs | net-new rebinding | Done |
| Harnesses: installed, path, auth, default model, enable, test, add | partial (detection) | ui/settings/harnesses.rs, engine/harness | UI | Done (disabled: "+ Add harness" only toasts; only the seven built-in harnesses run) |
| Models: per-harness list, capabilities, defaults, enable | partial (discovery) | ui/settings/models.rs | UI + enable flags | Done |
| Tools: MCP servers (add/remove/toggle/reconnect), built-in tool permissions | no | ui/settings/tools.rs, engine/mcp.rs | MCP config passed to harnesses | Done (disabled: "+ Add server" only toasts; servers are added in `settings.json`) |
| Skills: list, enable, import folder, new skill | no | ui/settings/skills.rs, engine/skills.rs | disk-backed | Done |
| Extensions: search, installed, browse/install, remove, enable | no | ui/settings/extensions.rs, engine/plugins.rs | harness plugins | Done |

### Other

| feature | exists? | Rust module | work | status |
|---|---|---|---|---|
| Share modal: Markdown / JSON export | no | ui/share.rs | real export to a file | Done |
| Share modal: link, visibility, invites | no | ui/share.rs | no server: disabled with tooltip | Done (disabled: no server, so the controls say there is no link) |
| notifications when a session finishes (window inactive) + sound | no | app/notify.rs | notify-send / OS APIs | Done |
| Harness trait + Gemini CLI, Aider, Goose, Pi | partial | engine/harness | | Done |
| approval prompt for "Ask" tool permission (Claude `--permission-prompt-tool`) | no | engine/approve.rs | small MCP server inside ArchonUI | Done |

## Existing features kept and where they go

| existing feature | new home |
|---|---|
| slash-command palette (`/` in the composer, agent commands discovered per harness) | composer popover, styled like the model picker |
| model pins (3 per harness) | model picker: pinned models first |
| Details tab (chat details, files, activity log) | Settings is not right for it; it becomes a "Session info" popover from the tile header subtitle |
| VS Code (code-server) IDE option | Settings › Tools › IDE engine (Built-in / VS Code) |
| tmux-backed terminals, attach command | Terminal tab + Settings › Tools |
| agent change following in the editor ("Follow agent", Changes list) | IDE tab toolbar |
| `/preview`, `/edit`, `/terminal`, `/mode`, `/effort`, `/model` commands | still in the slash palette |
| headless test commands (`--detect`, `--discover`, `--ask`, `--term-test`, `--tmux-test`, `--preview-shot`) | unchanged |

## Phases

1. Foundation, pure logic: themes, fonts, sigils, tiling, harness trait + modes. **Done.**
2. App state store and persistence (`app/`), migrating sessions/settings; widgets (text input, toggle, segmented,
   slider, popover, toast); animator; glass/wallpaper. **Done.**
3. Shell: frameless window, wallpaper, left sidebar (expanded + rail), tile area with tiling + handles, right panel
   frame, keyboard map. The old ChatView retires here. **Done.**
4. Tile content: message stream (markdown, tool rows, code/chart/image, lightbox), composer, model picker. **Done.**
5. Right panel tabs restyled onto the existing backends; Files CRUD. **Done.**
6. Settings modal and all pages, with their backends (MCP, skills, plugins, keymap, identity, notifications). **Done.**
7. Share/export, session switcher, context menus, projects CRUD. **Done.**
8. Polish: motion everywhere, reduced motion, visual comparison against the prototype screenshots, clippy, tests.
   **Partial:** motion and reduced motion are wired and the theme sweep ran; the remaining prototype pixel deltas are
   listed under Known issues in `CHANGELOG.md`.
