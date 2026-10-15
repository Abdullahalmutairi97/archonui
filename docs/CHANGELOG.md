# Changelog

## 0.1.7

- An empty session (nothing sent, typed or attached) is deleted, tile and all, as soon as you move to
  another session, not only when its tile is closed.

## 0.1.6

- Tile shortcuts are back on Alt: Alt+Enter (split new session), Alt+Q (close tile), Alt+←/→ (focus),
  Alt+T (next theme). Settings still on the Ctrl+Shift defaults move back; keys you rebound stay.
- An empty session (nothing sent or typed) is deleted when its tile is closed or replaced.

## 0.1.5

- A new session stays out of the sessions list until its first message is sent.
- Text typed but not sent is kept as a draft. Leave the session (focus another tile, close the tile or
  close the window) and it joins the list, titled by the draft, and stays there until you delete it.
  The draft is restored in the composer when you reopen it, even after a restart.

## 0.1.4 — first public beta

- Bug fixes across the engine, chat, editor, tabs and widgets.
- MIT license, `install.sh`, a prebuilt-binary release workflow, CI, and an Arch `PKGBUILD`.
- `--version` and `--help`; smoke tests that run the binary against a fake agent CLI.

## Unreleased — new UI

*This build's default interface is the new one: plain `archonui` (or `archonui`) opens it. The previous UI
is kept for now behind `archonui --old-ui`; `--new-ui` is still accepted and does nothing. Everything below
describes the new UI; the shared backend (harnesses, turns, discovery, git, editor, terminal, preview) is
unchanged at the protocol level except where noted.*

### Highlights

- A tiled workspace: up to six sessions on screen at once, dwindle or master tiling, drag the gaps to
  resize, and the layout is saved and restored.
- A frameless glass window over a wallpaper, with 12 themes, 6 background styles, per-colour overrides,
  switchable font pairings and a wallpaper picker — all applying live.
- New agent CLIs: Gemini CLI, Aider and Goose join Claude Code, Codex CLI, OpenCode and Pi behind one
  harness trait, each detected, sign-in-checked and testable from Settings.
- Auto / Plan / Ask modes, mapped to each CLI's own flags per turn, so a session can be told to plan or
  answer without editing files.
- A real chat view: markdown, tool-call rows with results, code blocks with Copy, chart blocks, image
  lightbox, live thinking timing, and a multiline composer with attachments and a single model/mode/effort
  picker.
- ArchonUI-managed MCP servers, skills and harness extensions, imported from disk and passed to the
  harnesses that support them at launch.
- Claude Code can ask ArchonUI for approval inside the app before each tool use, and finished turns raise a
  desktop notification and an optional sound.

### New features

#### Sessions & tiles

- Session sidebar, expanded (300 px) or collapsed to a sigil rail; a Sessions / Projects switch, a
  search field, and TODAY / EARLIER grouping with relative times that update on their own.
- Click a session to show it in the focused tile; Alt+click or the right-click menu's "Open in new split"
  opens it alongside. The menu also renames, moves to a project and deletes.
- Projects: add one by opening a folder, rename it, change its folder, or delete it (its sessions stay and
  lose the project). A project card shows session count, git branch and detected language.
- Session switcher (Ctrl+K): a palette over every session, searched by title and message text; Enter opens
  in the focused tile, Alt+Enter in a new split.
- Session info popover from a tile's header subtitle: harness and version, model, effort, mode, folder and
  creation time, with the agent's session id copyable.
- 100 generated sigils, assigned uniquely per session and stored with it, shown in the sidebar, the rail
  and the tile header.
- Frameless window with real minimise, maximise and close buttons, drag-to-move and edge resize.

#### Chat

- Message stream with user, agent and error rows; the agent header shows "ARCHONUI · thought for Ns" or
  "working" from the harness's own thinking events.
- Tool-call rows (read, grep, glob, edit, write, run, web, fetch, task) with the path or command and a
  result summary such as `+24 −8` or `128 passed`, including failed calls.
- Markdown rendering, code blocks with language badge, file path, Copy, line numbers and syntax colours,
  chart blocks written as ` ```chart ` with a Copy CSV button, and image blocks with a thumbnail lightbox
  and Open / Attach.
- Text selection across messages, and attachments shown as cards.

#### Composer

- Multiline input: Enter sends, Shift+Enter starts a new line, Shift+Tab cycles Auto / Plan / Ask, and the
  placeholder follows the mode.
- Attachments by drag-and-drop, file picker or paste, plus an attach popover for repository files, a
  browser screenshot, terminal output or an upload; typed badge cards can be removed before sending.
- One picker for model, mode and effort, with up to three pinned models per harness first in the list.
- Slash palette with local commands (`/model`, `/mode`, `/effort`, `/stop`) and each harness's own
  discovered commands.

#### Right panel tabs

- Browser: address bar, back / forward / reload, and the agent opening local sites or URLs into it; pages
  are rendered by headless Chrome streamed into the panel.
- IDE: the built-in editor (tabs, breadcrumb, highlighting, change bands, status bar, save, Follow agent)
  or VS Code through code-server. Opening a file from the Files tab opens it in the already-running
  code-server window instead of starting a second one.
- Files: the project tree with create file / folder, rename, delete (to the trash, with permanent delete
  when the trash is unavailable), a right-click menu, a filter field, live updates as files change, and a
  guard that blocks deleting a file with unsaved IDE edits.
- Git: branch, changed files with added / removed line counts, stage and unstage (selected or all),
  commit, push, pull, history log, diff view and init.
- Terminal: a real shell with theme colours, resize and scrollback, optionally inside tmux so shells
  survive closing ArchonUI; the attach command is shown with the session.
- The panel collapses to a 52 px rail; its width (360–820 px) and active tab are persisted.

#### Settings

- A 1320×880 modal with a nav column and 8 pages (General, Appearance, Keyboard, Harnesses, Models, Tools,
  Skills, Extensions), closable on backdrop click or Esc; nav rows show counts.
- General: restore sessions on launch, default project and folder, default mode, sandbox, ask-network,
  show tool calls, max steps per turn, notify, sound, update channel, analytics, export settings and clear
  history.
- Keyboard: 13 actions rebindable in place, with conflict detection and a Swap button; composer keys are
  listed read-only.
- Harnesses: installed state, path, version and auth status per CLI, default model, enable / disable and a
  Test button that runs one "Reply with exactly: ok" turn.
- Models: each harness's discovered models with per-model enable, per-harness default model and effort,
  fast titles and a long-context preference.
- Tools: the ArchonUI-managed MCP server list (add, enable, check, reconnect), an import of servers already
  configured in the installed harnesses, per-category built-in tool permissions (Read, Edit, Search, Run,
  Browser, Web, Fetch as Allow / Ask / Off), preview auto-open, tmux and terminal size.
- Skills: SKILL.md folders found on disk (personal, project, per-harness and plugin), each enabled or
  disabled, plus Import folder and New skill.
- Extensions: installed and available harness add-ons (Claude Code plugins, Pi packages, OpenCode plugins)
  with install, remove and enable / disable.
- Appearance: theme, background style, per-colour customisation, wallpaper, typography, layout sliders
  (gaps, radius, blur, dim, text size) and identity (app name and icon, preset or uploaded, used across
  the app).

#### Harnesses & engine

- A `Harness` trait with detection, model discovery, auth check and a test turn; the built-in harnesses are
  Claude Code, Codex CLI, OpenCode, Pi, Gemini CLI, Aider and Goose.
- Auto / Plan / Ask mapped to each CLI's flags, with the UI offering only the modes a CLI can enforce
  (`docs/HARNESS_MAPPING.md`).
- Per-turn settings become launch flags where the CLI has one: max steps (default 40), sandbox,
  ask-network and tool permissions.
- Claude Code approval prompts: in Auto mode with a tool set to Ask, Claude asks inside ArchonUI before using
  it. Allow (Enter) runs it, Deny (Esc) refuses it, and "Always allow <tool>" holds for the rest of the app
  run. Requests queue per session and time out after 10 minutes.
- Fast titles: a new session is named from its first exchange by the harness's cheapest model, off in
  Settings.
- Long-context variants: a model that lists a `[1m]` version is shown as that version, labelled "· 1M".
- Notifications and sound: a desktop notification when a turn ends while ArchonUI is in the background, with
  an optional completion sound; a stopped turn alerts nobody.
- MCP: servers imported from Claude, Codex, OpenCode, Pi, Gemini and Goose configs, checked against the
  live server, and passed to the harnesses that accept MCP config; server files are written 0600.
- Skills and extensions wired to the harnesses that support them (Claude Code and Pi).
- App state and persistence: `settings.json`, `workspace.json` and `projects.json` under the platform
  config dir, written atomically and debounced; a one-time migration from the old settings file; a corrupt
  file is set aside instead of blocking startup; sessions stay one JSON file each.

#### Appearance

- 12 themes (Blood Moon default) and 6 background styles, switchable live.
- Per-theme custom colours saved as overrides, and per-theme font choices from the bundled families.
- Layout and effects sliders for tile gaps, corner radius, panel blur, wallpaper dim and text size, plus a
  "tint all tiles" option.
- Identity: app name and icon (theme icon, a preset, or an uploaded file), used in the sidebar, wordmark
  and empty states, and written into the Linux desktop entry so the dock shows it after relaunch.

#### Added since the first draft

- Git panel: **Pull**, **Stage all**, **Unstage all**, and an inline diff pane for the selected file (changed
  lines coloured, added / removed counts, and a "show all" toggle for a long diff).
- IDE: a "changed on disk" bar with **Reload** / **Keep mine**; a Save / Don't save / Cancel prompt when
  closing a dirty tab; an **Open file…** picker over the tool folder; and an AGENT CHANGES list of the files
  the agent touched, with Clear.
- Share: **Include file diffs** and **Include terminal output** toggles (diffs on, terminal off, as the
  prototype has them) that add real sections to the Markdown and JSON exports.
- Composer: **Pin / Unpin** on each model row in the picker (up to three per harness, pinned rows first);
  Escape closes an open popover or palette first and otherwise stops a running reply; and dropping files
  anywhere on a tile attaches them, not just over the composer.
- Session info popover: the **Files** list (attachments plus the files the agent read or wrote) and the
  session's recent **activity log**.
- Settings: **IDE engine** (Built-in / VS Code), **code-server path** (auto-detected, with Choose… and Reset),
  **search engine**, **default folder for new sessions** (Choose… / Reset) and a **Data folder** "Open"
  button, and a **reduce motion** override next to the layout and effects sliders.
- Security: the Claude approval socket now lives in an owner-only 0700 directory, requires this run's random
  token on every call, checks that the peer runs as the current user, drops stale sockets on startup, and
  bounds each read. Settings export redacts MCP secrets, Codex no longer receives MCP secrets on its command
  line, ArchonUI's data files are written 0600 (directories 0700), the Chrome profile moved out of a predictable
  `/tmp` path, extension specs cannot start with `-`, and a notification cannot smuggle a flag through the app
  name (`docs/SECURITY_REVIEW.md` §Fixed).
- Claude Code's default effort is read from `~/.claude/settings.json` (`effortLevel`) when the CLI does not
  report one.
- code-server starts with dark, uncluttered defaults (`Default Dark Modern`, hidden secondary sidebar, no
  chat command centre, no startup editor); only keys that are absent are written, so a user's own settings win.

### Changed behaviour

- **Interface size.** The whole window is drawn at 85 % of the design size by default (like the prototype fitted to a
  1600 px window). Settings › Appearance › *Interface size* sets 60–150 %, live.
- **One-line composer.** Attach, the message box, the model button and Send sit on one line, like a chat app; the box
  grows upward with Shift+Enter (up to 10 lines). The placeholder is now just "Message archonui…".
- **Compact sidebar.** Smaller session titles and rows; the New session button is a thin bar.
- **Arabic.** Noto Sans Arabic / Noto Naskh Arabic are bundled, so Arabic renders joined and right-to-left without
  system fonts; paragraphs that start in Arabic (or Hebrew) are right-aligned.
- **New sessions use your defaults.** The sidebar's New session button and `archonui <files>` now apply the
  default model, effort and mode from Settings, like the tile's + button.

- The new UI is the default; the previous UI is behind `archonui --old-ui`, and `--new-ui` is ignored.
- Sandbox is off by default, so each harness's own config applies unless the setting is turned on. Ask
  network defaults on, and Edit and Run permissions default to Ask.
- Other defaults: max steps 40, mode Auto, theme Blood Moon, restore sessions on, notifications on, sound
  off, fast titles on, tmux on, built-in editor chosen over VS Code.
- The old right panel's Details tab is gone: that information is now the session info popover on the tile
  header. Preview is now the Browser tab.
- The composer is multiline instead of single-line, and model, mode and effort moved into one picker
  rather than separate palettes.
- Settings are a modal with pages rather than a single panel; the preview, IDE, terminal and tmux options
  live on the Tools page.
- Sessions are named automatically from their first exchange instead of their first prompt.
- Tool activity is shown as structured rows rather than a status line.

### Removed / not supported

- Share links, visibility and invites need a server ArchonUI does not have; those controls are disabled with
  a tooltip. Markdown and JSON export to a file work.
- The update channel can be set to Stable / Beta / Nightly, but update channels are not available in this
  build.
- The analytics toggle exists, but no usage data is sent in this build.
- "+ Add harness (any CLI that speaks ACP)" does not add one yet; only the seven built-in harnesses run.
- "+ Add server" in Tools does not open a form; MCP servers are added in `settings.json`.
- No window icon on Wayland through GPUI; the identity icon appears inside the app and in the Linux
  desktop entry.
- Per-harness gaps, from `docs/HARNESS_MAPPING.md`: tool Ask runs as Allow on Codex, OpenCode, Pi, Gemini,
  Aider and Goose (only Off turns a tool off there); switching a skill off is unsupported on Codex,
  OpenCode, Gemini, Aider and Goose; max steps is not sent to Codex or Pi; the sandbox is unsupported on
  OpenCode, Pi, Gemini, Aider and Goose; MCP is not passed to Gemini, Aider or Goose; OpenCode plugin
  install, remove and toggle are refused; Pi packages have no off switch.
- The Gemini CLI, Aider and Goose flag mappings come from their documentation and were not verified with a
  live run.

### Known issues

Open items from the parity diff review (`.qa/review/main-diff.md`, `.qa/review/settings-diff.md`) and the
motion report (`.qa/integration/motion-report.md`); the keyboard report (`.qa/integration/keys-report.md`)
and the robustness report (`.qa/integration/robustness-report.md`) have no open items.

- The diff review reports these prototype differences, high or medium: the tool-call row's collapse / expand
  icon alignment and row padding; tile header action-button padding (6 px against the design's 8 px); the
  split handle's visible stroke (4 px against 2 px); sidebar brand-header spacing (~2 px); browser
  address-bar corner radius (6 px against 8 px); the General page shows button state where the prototype
  shows default-project pills; the Appearance page's identity section is ordered differently from the
  prototype; wallpaper preview arrangement / radius; the Share link box and copy-button alignment.
- The diff review reports these further differences, low (≤ 2 px, or alpha only): composer shadow blur, agent
  pill font size, focused-tile border glow, multi-tile gap (7 px against 8 px), empty-state icon offset, files
  tree indent, git diff gutter, terminal background tint, code-block Copy margin; keyboard capture key
  spacing, harness status badge, model toggle spacing, tool permission toggle padding, skill card border,
  extension item spacing, people permission dropdown, and the switcher footer pill.
- Motion: the Share modal's Copy button is a placeholder with no handler, so its 1500 ms "Copied" reset cannot
  show; the working copy actions toast for 1800 ms instead.
- Chat: a failed tool call's red result has no width bound, so it overflows the row in a narrow tile.
- Tiles: titles truncate mid-word in very narrow tiles.
- Model picker: rows show the model's own description (or the agent's name) where the prototype shows a
  `harness · ctx` sub-line.
- Terminal: the surface follows the theme, but the ANSI colours are xterm's palette, not the theme's.
