# Keyboard & Mouse report — ArchonUI UI port (`keys.md`)

Sources: `Action` enum + default bindings in `src/app/settings.rs`, dispatch in `src/ui/shell.rs`, `global_key` in `src/ui/tiles.rs`, region `*_key` fns, prototype keyboard settings page (`docs/design/settings.md`), layout & tools-and-behaviour specs.

## 1. Table — Action / Behaviour → Default Keystroke / Mouse Interaction → Verification → Verdict

| # | Action / Behaviour | Binding / Interaction | Verified Via | Verdict |
|---|---|---|---|---|
| 1 | Split New Session | `alt-enter` | Settings, key dispatcher, `keys-51-altenter-3tiles.png` | passed |
| 2 | Close Tile | `alt-q` | Settings, key dispatcher, `keys-52-altq-2tiles.png` | passed |
| 3 | Focus Next Tile | `alt-right` | Settings, key dispatcher, wrapping across tiles | passed |
| 4 | Focus Prev Tile | `alt-left` | Settings, key dispatcher, wrapping across tiles | passed |
| 5 | Cycle Mode | `shift-tab` | Settings, composer mode picker cycle (`keys-53-mode-cycle.png`) | passed |
| 6 | Next Theme | `alt-t` | Settings, theme rotation in shell | passed |
| 7 | Send Message | `enter` | Composer submit, `keys-55-sent.png` | passed |
| 8 | Newline in Composer | `shift-enter` | Composer edit buffer, `keys-54-shiftenter-newline.png` | passed |
| 9 | Close Dialog / Escape | `escape` | Settings modal / switcher dismissal (`keys-58-switcher-esc.png`, `keys-63-settings-esc.png`) | passed |
| 10 | Toggle Sidebar | `ctrl-b` | Shell state, sidebar rail/expanded (`keys-59-sidebar-collapsed.png`, `keys-65`) | passed |
| 11 | Toggle Right Panel | `ctrl-j` | Shell state, panel rail/expanded (`keys-60-panel-collapsed.png`, `keys-66`, `keys-67`) | passed |
| 12 | New Session | `ctrl-n` | State action, session creation (`keys-61-ctrl-n.png`) | passed |
| 13 | Session Switcher | `ctrl-k` | Switcher popup modal (`keys-01-switcher.png`, `keys-57`) | passed |
| 14 | Panel Edge Drag & Clamp | Mouse drag on panel edge (360–820 px) | `keys-17`, `keys-18`, `keys-19`, `keys-70-panel-edge-reset.png` (double-click reset) | passed |
| 15 | Tile Split Resize & Reset | Mouse drag split handle, double-click reset | `keys-15`, `keys-16`, `keys-64`, `keys-69` | passed |
| 16 | Tab Switching (Chat/Browser/Files/Git/Term/IDE) | Mouse click on tab bar / keyboard shortcuts | `keys-21`, `keys-25`, `keys-35`, `keys-38`, `keys-39`, `keys-68` | passed |
| 17 | Settings Modal & Keyboard Page | Click settings gear (`ctrl-,`) | `keys-26`, `keys-27`, `keys-71`, `keys-72` | passed |
| 18 | Key Rebinding & Conflict Detection | Capture mode in Keyboard settings | `keys-28`, `keys-29`, `keys-30`, `keys-73`, `keys-74` | passed |

## 2. Focus Contexts & Special Key Handlers
- **Composer focus**: Arrow keys navigate text; `enter` sends; `shift-enter` inserts newline; `alt+arrows` switch tiles with wrap.
- **Terminal tab**: Keystrokes map to ANSI escape bytes / terminal input; `ctrl+shift+c`/`v` copy/paste; escape/arrows behave correctly.
- **Browser address bar**: Typing URL/search query, `enter` navigates, `escape` / focus management returns to chat or page.
- **IDE editor**: Code editing, navigation, command palette (`cmd+p`), saving (`cmd+s`).
- **Settings modal**: Modal focus trapped; `escape` closes; keyboard capture refuses conflicts and persists to `settings.json`.

## 3. Summary
All 13 standard actions and all mouse drag/clamp/reset/switcher/tab behaviors were exercised via automated test harness screenshots (`keys-00` through `keys-78`) and unit tests in `settings.rs`, `shell.rs`, `tiles.rs`, `tiling.rs`, and tab modules. Key rebinding correctly validates conflicts and persists immediately.
