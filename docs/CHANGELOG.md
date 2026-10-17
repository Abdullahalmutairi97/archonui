# Changelog

## 0.2.7

- The redesigned Appearance page is back: theme cards show a small picture of each theme, wallpaper tiles
  sit full width under Image, and Titles and Text each get a font row with a Change button.
- Background style and wallpaper work at any window opacity. Below 100% they used to give way to a flat
  tint, so changing them did nothing; now every layer keeps its style and fades with the window.
- The picked wallpaper stays when you change theme. A pick saved per theme by an older version moves over.

## 0.2.6

- Skills, extensions and tools are marked with their agents. Every skill card shows which agents load it,
  each extension shows its agent, and each MCP server and built-in tool row on the Tools page shows which
  agents it reaches. An agent that ignores a setting is struck through, and its tooltip says why. Headers
  count each agent's skills, servers and tools.
- New Skills › Sync to all agents links skills so every agent that is on loads the same set. It only makes
  and prunes its own symlinks, never moves or deletes a skill folder, keeps a skill you turned off where it
  is, and keeps your own copy when an agent already has a skill of that name. A second Sync changes nothing.
- Skill discovery finds the claude.ai-synced skills and every agent's own skills folder.
- Gemini CLI extensions can be listed, installed, removed and switched on or off.
- Pi gets ArchonUI's MCP servers. Gemini, Codex and OpenCode honour tools switched Off and skills switched off.
- The Claude default model and default effort come from your Claude settings, not the plan's recommendation.
- An effort the model doesn't take is no longer sent; `--ask --effort` rejects an unknown one.
- The preview browser follows titles a page sets from script.
- A local slash command or an empty Pi reply no longer resets the context meter.
- `--ask` exits 1 when the turn fails. Pi's "No project session found" warning no longer heads its errors.
- Many small layout and wording fixes across Settings, the Files tab, the attach popover and replies.
- Appearance is compact. Themes are small thumbnails six to a row. Background has a Style segmented control
  (Canvas, Glass, Solid, Gaps, Tinted, Ambient), a row of 64×40 image tiles, and dim, panel blur and window
  opacity sliders. Fonts are a pairing control, an Apply to Titles or Text control, and a wrap of font chips
  marked T and Aa. Custom colours are collapsed behind an Edit button. Layout is unchanged, and Window holds
  the rest of the old Effects rows. Identity sits at the bottom, with smaller icon tiles.
- New Appearance › Background › Filter: Off, Duotone or Tritone dithers the background image or gradient into
  a few colours. When a filter is on, Dot size (1–8 px) sets the size of each dot.
- The agent's task list reads as a compact plan in its reply: a `▸ plan` header with the count done and
  in progress, then up to six numbered steps in the code font. ✓ green and muted when finished, ● in the
  accent with "active" for the step in progress, ○ for waiting ones; longer lists show a window around
  the active step and a "… N more steps" line. The streaming heading is gone.
- Gradient backgrounds are soft mesh blends: the theme's base colour with a glow of one colour from
  the top-left and another from the bottom-right, plus faint noise so they don't band. Aurora, Sunset,
  Ocean and Nebula have calm authored surfaces over their glows, Hermes a darker, less saturated navy,
  and inline code a lighter background in every theme.
- New neutral mark: a plain "A", for the app icon (all hicolor sizes) and the built-in themes' logo.
  The sword logo and the castle artwork are no longer in the repository.
- Context meter: a small bar beside the model picker shows how full the agent's context is, with no
  numbers. It turns the highlight colour past 70% and red past 90%. Clicking it shows the share, the
  tokens used of the window, and whether the window came from the agent or is the usual one for the
  model. Claude Code reports its window after each turn; Pi's comes from its `models.json`; other
  models go by family (Claude 200k, or 1M with `[1m]`; GPT-5 and Codex 400k; Gemini 1M). Usage is the
  latest model call for Claude, Pi and OpenCode, and the turn's total for Codex and Gemini. Switching
  agent resets it; changing model drops the reported window.
- Appearance › Effects › Window opacity (20–100%): below 100% the desktop becomes the wallpaper.
  Only the theme's base colour covers it, at that opacity, and sidebars and sessions add a light tint
  over it; the background image and glass blur are left out, so layers don't stack into an uneven,
  murky cover. Text and popovers stay solid. A compositor blur rule blurs what shows through.
- IDE: scrolling no longer empties the editor. The code area drew a spacer for every row scrolled
  past above the visible rows, and since the area doesn't scroll itself, that pushed them below its
  bottom edge. Code text is smaller (11.5px on 17.5px rows, from 12.5px on 21.25px), so more fits.
- The agent's task list shows in its reply. Claude Code runs with
  `CLAUDE_CODE_ENABLE_TODO_TOOLS=1`, which print mode needs for TaskCreate and TaskUpdate; TodoWrite is
  read too. Codex's `todo_list`, OpenCode's `todowrite` and Gemini's `write_todos` feed it as well.
  Pi has no task list tool.
- A reply that used tools separates its final answer: the notes written between tool calls are
  dimmed and smaller, then a FINAL ANSWER divider, then the answer at full size.
- A finished reply ends with a line: ✓ Done, how long it worked, tokens in and out, the share of input
  read from the prompt cache, the cost when the agent reports one, and the lines its edits added
  (green) and removed (red). Tokens come from Claude Code's and Gemini's result line, Codex's
  `turn.completed`, Pi's `message_end` and OpenCode's `step_finish`.
- Edit rows show their `+N −M` in green and red.
- Steering: a message sent while the agent works is queued above the message box instead of being
  refused. The first queued message goes out when the reply finishes. Steer now stops the reply and
  sends it at once, so the agent continues the same session with the new instructions; × drops it.
  After Esc, the queue waits and each row offers Send.
- Esc stops the focused session's reply from anywhere in the window, not only from the message box.
  Popups, the terminal and the editor still get Esc first. The working line says "Esc to stop".
- Themes are data. ArchonUI ships 13 themes with no wallpaper: ArchonUI (neutral, the default),
  Claude Code, Codex, Gemini and Hermes (each matched to that app's dark look), the classic editor
  palettes Nord, Catppuccin, Tokyo Night and Gruvbox, and the gradient themes Aurora, Sunset, Ocean
  and Nebula. Without an image, a theme's background is a three-colour diagonal blend, generated in
  memory, which the glass and blur styles use like a wallpaper.
- Theme files: every `.json` file in `~/.config/archonui/themes` adds a theme (or replaces a built-in
  one with the same id): `id`, `name`, `colors` (bg, fg, acc required; muted, faint, acc2, deep, alt,
  on, soft optional), optional `tune`, `gradient`, `wallpaper`, `logo`, `pixelated` and `fonts`. A
  relative wallpaper is looked up in `~/.config/archonui/backgrounds`, a relative logo in
  `~/.config/archonui/logos`. A file that doesn't parse is skipped and reported on stderr; a saved
  theme id with no theme shows the default theme.
- Backgrounds: Settings › Background offers the theme's own background and every image in
  `~/.config/archonui/backgrounds`, for any theme. Uploads are copied there, so they stay available.
- The twelve wallpaper themes (Blood Moon, Moonlight, Citadel and the rest) and their wallpapers and
  logos are no longer in the repository. Only the ArchonUI mark is still embedded.
- Appearance › Typography: the Minimal, Display font and Interface font sections are one All fonts
  list, each family once. Each font card has a Titles button (the display font) and a Text button
  (the interface font, also used by the chat and the message box). The theme pairings stay above it.
- The chat and the message box share one font. Without a Text pick, a monospace theme font is
  replaced there by Geist; with one, the picked font is used as is.
- Appearance › Colors: the colour cards are grouped under Surfaces, Text, Accents, and Lines and
  highlights. The last group is new: Borders, Strong borders, Selection (selected rows and your
  message bubble), and Highlight start and end (the focused session's ring). Borders and Selection
  keep their transparency; the colour picked sets their hue.
- Appearance › Layout › Edge to edge: sessions fill the whole area, with no gaps between them or at
  the sides, square corners and no borders. Gaps, Corner radius and Borders keep their values and
  apply again when it is off.
- Appearance › Effects › Borders: off removes the borders around sessions (focus ring included) and
  the line along the tools panel's edge; the sessions grow into the space. Gaps are unchanged.
- Appearance › Effects › Session highlight: off removes the coloured ring around the focused
  session, which then gets the same plain border as the others.

## 0.2.5

- The conversation is a centred column up to 880px wide, so on a wide window messages read down
  the middle instead of hugging the left edge. It is centred with auto margins: a flex-row wrapper
  made the message list measure rows at the wrong height, so messages overlapped.

## 0.2.4

- Replies are easier to read. Paragraphs, lists, tables and code have 14px between them, and a
  heading gets extra space above it and a size by level (`#` 1.4×, `##` 1.22×, `###` and below 1.08×
  the body text). Reply text is at most 820px wide, and it is set in a proportional font (Geist) when
  the theme's interface font is monospace; code keeps the code font.
- Every agent is asked to format its replies: short paragraphs separated by blank lines, headings for
  longer replies, lists for steps and changes, tables for comparisons, fenced code with a language.
  Claude Code and Pi get it as a system prompt addition on every turn; Codex, Gemini and OpenCode at
  the start of the first message of each agent session, except a message that is a slash command.

## 0.2.3

- A skill reached through a symlink (for example `~/.claude/skills/x` linking to `~/.agents/skills/x`,
  so every agent shares one copy) is listed once in Settings, under the folder it really lives in.

## 0.2.2

- Switch agents mid-chat. The model picker has an AGENT row with every enabled agent, and `/agent`
  lists them (`/agent codex` switches directly). The chat keeps its messages, folder and project; the
  model and effort reset to the new agent's defaults, and the mode is kept if the new agent has it.
  Agents can't resume each other's sessions, so the new agent starts its own, and your next message
  carries the conversation so far (the newest ~60k characters, tool calls listed by file or command).
  A slash command is sent as typed, and the conversation goes with the message after it.
  Switching is unavailable while a reply is running.
- In a chat with replies from more than one agent, each reply's header names the agent that wrote it.

## 0.2.1

- The agent knows which project its chat is in. A chat filed under a project tells the agent the
  project's folder, so a request that names no project ("fix the UI", "the app") means that one.
  It is context, not a limit: ask for another folder and the agent goes there. Claude Code and Pi
  get it as a system prompt addition on every turn; Codex, Gemini and OpenCode at the start of the
  first message of each agent session.

## 0.2.0

- A chat runs in its project's folder. New chats filed under a project start there whichever way they
  are opened (new tile, split, startup files), not in the folder ArchonUI was launched from or the
  default folder, so the agent works on the project the chat belongs to. A chat under a project that
  hasn't sent its first message yet is moved to the project's folder when it sends.
- The browser opens local files only from the project folder. `file://` addresses typed in the
  address bar, and links, frames, redirects and scripts in a page, can't load anything outside it
  (symlinks and escaped `..` included); with no project, no local file opens. Web pages are unchanged.
- The browser finds Playwright's Chromium in newer Playwright releases (`chrome-linux64`).

## 0.1.9

- Auto mode with Claude Code is now Claude's own `auto` permission mode, as in an interactive
  Claude Code session: a safety check approves or blocks each action and nothing stops to ask.
  Tools on Allow skip the check, tools on Off stay blocked, and tools on Ask are left to the check.
  ArchonUI's approval card is no longer shown in Auto.

## 0.1.8

- The terminal starts instantly with fish. The terminal now answers the device-attributes,
  cursor-position and status queries a shell sends at startup; fish waited seconds for the answer
  before drawing its first prompt.

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
