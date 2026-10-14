# ArchonUI Agent: port the new UI into the native Rust app

You get two inputs:

1. **The existing Rust codebase.** A working native desktop AI agent app that already implements most of the backend features.
2. **`ArchonUI Agent v3.dc.html` (in the attached zip).** A high-fidelity, fully interactive HTML prototype of the UI. Treat it as the **visual and behavioural spec**. Every layout, spacing value, colour, radius, font, animation, and interaction in it is intentional.

Your job: **replace the current UI of the Rust app with this design, and connect every UI control to real functionality.** If the prototype shows something the Rust app can't do yet, **build it**. Nothing in the final UI may be a non-functional mock.

---

## 0. Before you write code

1. Read the whole Rust codebase. Identify the UI framework (egui, iced, gpui, Slint, Tauri+webview, Dioxus, etc.), the agent/harness layer, the session model, persistence, config, and any existing tool integrations (terminal, git, files, browser).
2. Open the prototype in a browser and click through **everything**: every panel, menu, picker, settings page, theme, and tile operation. Read its source. All design values (hex colours, px sizes, font stacks, timing curves) are inline in the markup and logic class; lift them exactly.
3. Write `docs/UI_PORT_PLAN.md` containing:
   - A **feature matrix**: one row per UI feature in the prototype, with columns `Exists in Rust? (yes / partial / no)`, `Rust module/file`, `Work needed`.
   - Your plan for structuring the new UI modules.
   - Any framework limitation that blocks an effect (e.g. real backdrop blur), plus the closest native alternative you'll use.
4. Then implement, working down the matrix. Don't stop at "partial".

Keep the existing UI framework unless it truly can't reach the design (glass/translucency, smooth animations, custom text rendering). If you think it can't, say why in the plan and propose the smallest viable change before you switch.

---

## 1. Global look and feel

- **Minimalist, chrome-less.** There is **no top bar**. Window controls are integrated or hidden, matching the prototype. Use a frameless/transparent window where the OS allows it.
- **Canvas background:** user wallpaper (or one of the generated background styles) shows through **glass** surfaces. Use real compositor blur/vibrancy where available (macOS `NSVisualEffectView`, Windows Mica/Acrylic, Wayland/KDE blur protocols). Otherwise fall back to a pre-blurred wallpaper texture under semi-transparent panels.
- **Focused session tile is nearly transparent.** Unfocused tiles are slightly more opaque/dimmed. Match the prototype's alpha values.
- **Theme system:** 12 themes, with **Blood Moon** as the default. Port every theme's full token set (backgrounds, surfaces, borders, text tiers, accent, accent bands, status colours) exactly from the prototype. Themes switch live with no restart.
- **Fonts:** Geist, IBM Plex Sans, DM Sans, Instrument Sans, Manrope, Space Grotesk. Bundle them in the binary (`include_bytes!`) and switch live.
- **Motion:** match the prototype's durations and easing for sidebar/panel collapse, tile split/merge, hovers, pickers, and focus changes. Respect the OS reduced-motion setting.

---

## 2. Layout and window management

### Left sidebar (collapsible)
- **Sessions list:** one line per session. Show the session icon, title, a **harness badge** (Claude Code / Codex CLI / Gemini CLI / OpenCode / Aider / Goose), and a **last-used timestamp** (relative, auto-updating).
- **Clicking a session swaps it into the currently focused tile.** It does not open a new tile. Provide a modifier-click or context-menu action to open it in a new split.
- **Projects section:** group sessions by project or working directory. Support create, rename, and delete. Each project is bound to a directory on disk.
- **Collapsed state:** sessions render as icons only, sitting on **rotating 6-colour accent bands** (colours taken from the active theme, cycling per row as in the prototype).
- Collapse/expand animates, and the state persists.

### Session tiles (Hyprland-style tiling)
- Opening a session in a new tile splits the focused tile (alternating horizontal/vertical by aspect ratio, as in the prototype). Closing a tile collapses its split.
- **Resizable:** drag the gaps between tiles, and drag panel edges. Apply min-size constraints. Persist the layout tree per workspace.
- There is exactly one focused tile at a time. Clicking, or using the keyboard, changes focus.
- Implement the layout as a binary split tree (or reuse the existing one if present). Serialize it to the config.

### Right tools panel (collapsible)
Tabs: **Browser, IDE, Files, Git, Terminal.** They act on the focused session's working directory.
- **Browser:** embedded webview (e.g. `wry`) with URL bar, back/forward/reload. The agent can open URLs here.
- **IDE:** a code view/editor with syntax highlighting (e.g. `syntect` or tree-sitter). Open files from Files or from agent output. Save to disk.
- **Files:** a real filesystem tree of the project dir. Open, rename, create, delete. Reflects live changes (`notify` crate).
- **Git:** real repo status (`git2` or the CLI). Show changed files, diff view, stage/unstage, commit with message, branch name, log. Match the prototype's layout.
- **Terminal:** a real PTY (`portable-pty` + a VT parser such as `alacritty_terminal` or `vte`), themed with the active theme, with resizing and scrollback.

---

## 3. Session / chat view

- **Message stream:** user and agent messages styled exactly as in the prototype. Stream tokens live from the harness.
- **Rich reply blocks:** charts/graphs and images in agent replies render inside the **special framed boxes** from the prototype. Parse them from harness output (image paths/URLs, markdown images, and a structured chart block such as fenced JSON). Images can be opened full size.
- **Attachments:** drag-drop or picker. Attachments appear as **coloured type-badge cards** (colour and label by file type, as in the prototype), are removable before sending, and are passed to the harness.
- **Composer:** multiline input with send and stop, and attachment button. Keyboard behaviour as in the prototype.

### Model picker (single popover)
It contains all of these, as in the prototype:
- **Model list** for the session's harness.
- **Mode selector: Auto / Plan / Ask.** Map these to each harness's real flags or behaviours (permission modes, plan-only, read-only Q&A). Document the mapping per harness.
- **Effort:** shown as **moons**. The **number of levels is model/harness-dependent (2–6)**, taken from the per-model capability data. Map each level to the harness's real reasoning/effort parameter. Hide the control when a model doesn't support effort.

---

## 4. Session icons

- Each session gets a **unique icon** from **100 dot-grid patterns**. Port the generator/pattern table exactly from the prototype.
- Assign deterministically on creation (e.g. hash of the session id, avoiding collisions among live sessions). Store the assignment with the session.
- Icons tint with the theme accent and appear in the sidebar (expanded and collapsed), in tile headers, and anywhere else the prototype shows them.

---

## 5. Settings (full, every control functional)

Pages: **General, Appearance, Keyboard, Harnesses, Models, Tools, Skills, Extensions.** Replicate each page's layout exactly. Every toggle, slider, and field reads from and writes to the real config (TOML/JSON in the platform config dir) and applies live.

- **General:** includes the **Identity** section. Rename "ArchonUI" (the app and assistant display name is used everywhere it appears) and choose a **preset icon or upload a custom one** (PNG/SVG, stored in app data, applied to the window/dock/taskbar icon where the OS allows).
- **Appearance:**
  - 12 theme presets
  - 6 background styles
  - Wallpaper picker (local file)
  - Per-colour customization on top of a preset (saved as a custom theme)
  - Font row (the 6 fonts)
  - Layout sliders (gaps, radii, panel widths, transparency, and whatever else the prototype exposes)
- **Keyboard:** list every shortcut. Make them rebindable with conflict detection. Include tiling/focus/move shortcuts, sidebar/panel toggles, a new session, and a session switcher.
- **Harnesses:** Claude Code, Codex CLI, Gemini CLI, OpenCode, Aider, Goose. For each: detect whether it's installed, set the binary path, auth/API key status, default model, enable/disable, and a test-connection button. Use a shared `Harness` trait so adding one more is trivial. **Implement any harness the Rust app doesn't support yet.**
- **Models:** per-harness model list with capabilities. This is the source for the effort level count (2–6) and mode support.
- **Tools:** enable/configure the right-panel tools and the agent-callable tools.
- **Skills:** list, enable, and install skills (directory of skill files). Pass them to harnesses that support them.
- **Extensions:** list, enable, and configure extensions/MCP servers. Implement MCP client config if it's missing.

---

## 6. Persistence and state

Persist across restarts: sessions (messages, harness, model, mode, effort, icon, project, timestamps), projects, the tile layout, sidebar/panel collapsed state and widths, the active tool tab, theme/custom colours, font, wallpaper/background, layout sliders, identity, keybindings, and harness/model/tool/skill/extension config.

---

## 7. Rules

- **Pixel fidelity:** the prototype is the source of truth. Lift exact values; don't "improve" the design. If a value is ambiguous, check the prototype source and don't guess.
- **No dead UI:** every visible control must do something real. If the backend for it doesn't exist, build it. If a feature is truly impossible on a platform, show a clear disabled state with a tooltip explaining why, and list it in the plan.
- **Don't break existing features.** Anything the Rust app does that the prototype doesn't show should be kept and placed in the most fitting spot in the new UI (usually Settings or the tools panel). List these in the plan.
- Keep the UI layer cleanly separated from agent/harness logic: UI modules talk to a state store/event bus, not directly to processes.
- Performance: 60fps+ for scrolling, resizing, and streaming, with long transcripts virtualized. Keep idle CPU near zero.
- Run `cargo clippy` with no warnings. Add tests for the layout tree, the config round-trip, harness mode/effort mapping, and the icon assignment.

## 8. Deliverables

1. The updated Rust app, building on macOS, Windows, and Linux (note any platform gaps).
2. `docs/UI_PORT_PLAN.md`, with the feature matrix updated so every row is marked **Done**.
3. `docs/HARNESS_MAPPING.md`: how Auto/Plan/Ask and each effort level map to each harness.
4. A short changelog of new backend features you had to build.

Work through the whole matrix before reporting back. When you report, list anything still not at full parity, with the reason.
