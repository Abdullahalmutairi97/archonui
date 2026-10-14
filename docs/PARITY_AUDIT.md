# ArchonUI UI parity audit — old UI & prototype vs `--new-ui`

Read-only audit of the Rust/GPUI app. Scope: the old chat UI (`src/chat.rs` + `src/chat/*`) and the ported
prototype (`design/ArchonUI Agent v3.dc.html`, specs in `docs/design/*.md`) against the new UI (`src/ui/*`,
launched with `--new-ui` → `src/ui/shell.rs:377`). Evidence is `file:line` in the **new** code, or
"missing". A function existing is not proof it is wired; call sites were followed (e.g. model pins are
stored and used for ordering but have no control — see §4).

Legend: **Ported/Done** · **Changed** (works, differs) · **Partial** · **Missing** ·
**Disabled-with-tooltip** · **Not animated**.

---

## 1. OLD UI → NEW UI

Every user-facing feature, command, shortcut and setting of the old UI.

| # | Old-UI feature | Status | Evidence in new code |
|---|---|---|---|
| 1 | Slash commands `/new /stop /model /effort /mode /preview /terminal /edit /rescan /settings` | Ported | `src/ui/composer/palette.rs:12` (`LOCAL_COMMANDS`), dispatch `palette.rs:207` |
| 2 | Slash palette: prefix rank, ↑↓ select, Tab complete, Enter run, Esc close | Ported | `palette.rs:61,157`; `composer.rs:275,468,490,514` |
| 3 | Model picker (`/model`, "Agent default", list) | Ported | `src/ui/composer/picker.rs:286` |
| 4 | Effort picker (`/effort`, per-model levels, default) | Ported | `picker.rs:358`; `composer/palette.rs:124` |
| 5 | Mode picker (`/mode`, per-harness modes, Auto/Plan/Ask) | Ported | `picker.rs:206`; `palette.rs:145,243` |
| 6 | **Model pins** (Ctrl+P in picker, Pin/Unpin button, max 3, pinned-first) | **Missing** | Ordering uses pins (`picker.rs:303-307`, `palette.rs:97`), `toggle_pin` exists (`app/settings.rs:189`) but **no UI calls it**; Ctrl+P is gone (`composer.rs:468`) |
| 7 | Attachments — file picker | Ported | `src/ui/composer/attach.rs:198` |
| 8 | Attachments — drag-drop | Partial | `composer.rs:247` `on_drop` on the **composer card only**, not the whole tile/chat column |
| 9 | Attachments — paste image (Ctrl+V) | Ported | `attach.rs:79`; `composer.rs:488` |
| 10 | Attachments — count limit (10) | Ported | `attach.rs:44-69`; `engine/attach.rs:18` |
| 11 | Attachments — per-file size limit (64 MB) | Ported | `engine/attach.rs:16,51` (via `Attachment::inspect`) |
| 12 | Attachment chips with remove (×) | Ported | `composer.rs:130-150`, `remove_attachment:532` |
| 13 | Pending summary text ("N files, size · up to N, X each") | **Missing** | No such line in `composer.rs:118` (chips only) |
| 14 | Details tab — chat info (agent/version, sign-in, model, mode, effort, folder, message count, started) | Changed | `tiles.rs:563` (`render_info`) is a popover; drops sign-in and message-count rows |
| 15 | Details tab — files list with sizes | **Missing** | `tiles.rs:563` has no files section |
| 16 | Details tab — activity log ("what the agent did") | **Missing** | `app/state.rs:290` keeps `activity`, no render site |
| 17 | Notifications when a session finishes (window inactive) | Ported | `shell.rs:208` (`alert_turn_ended`); `app/notify.rs:43` |
| 18 | Sound on finish | Ported | `app/notify.rs:71`; `shell.rs:208` |
| 19 | Terminal attach command (tmux, click to copy) | Ported | `src/ui/tabs/terminal.rs:286-297` |
| 20 | Terminal Clear / Restart buttons | Ported | `terminal.rs:276,281` |
| 21 | Terminal tmux mode + reattach after exit | Ported | `terminal.rs:395,404`; setting `settings/tools.rs:185` |
| 22 | Terminal font-size setting | Ported | `settings/mod.rs:217`; `settings/tools.rs:156` |
| 23 | **IDE engine choice: Built-in ↔ VS Code (code-server)** | **Missing** | `IdeKind` defaults to Builtin (`store.rs:324`); `ide.rs:427` only switches **back** to built-in; no "use VS Code" control and no code-server-path/autostart settings UI |
| 24 | Preview/browser: URL bar, back/forward/reload, open local sites from replies | Ported | `src/ui/tabs/browser.rs:307,353`; `browser_open:648`; auto-open `settings/tools.rs:161` |
| 25 | Auto-open local sites from replies (setting) | Ported | `settings/tools.rs:161-173` |
| 26 | **Search-engine setting** | **Missing** | `settings.search_engine` used (`browser.rs:633`, `app/settings.rs:97`) but no settings control |
| 27 | Git: branch, changes, stage/unstage per file, commit, push, history | Ported | `src/ui/tabs/git.rs:171` (panel), `361` (stage), `374` (commit), `209` (push), `225` (history) |
| 28 | Git: **Pull** | **Missing** | No `pull` call site in `src/ui` |
| 29 | Git: Refresh button | Changed | Replaced by automatic refresh on watcher change (`git.rs:75,119`) |
| 30 | Git: **diff view** (select a file → coloured diff) | **Missing** | `src/ui/tabs/git.rs` has no diff pane (only `numstat` counts) |
| 31 | Git: "Initialize repository" on a non-repo | Ported | `git.rs:154,164` |
| 32 | Git: Stage all / Unstage all | **Missing** | Only per-file `git_toggle` (`git.rs:361`) |
| 33 | Editor: tabs, open by path (`/edit`), save (Ctrl+S), undo/redo, indent, word nav | Ported | `ide.rs:204,239,619`; `open_in_editor` `composer.rs:545` |
| 34 | Editor: "Open file…" picker button | Changed | Files open from the Files tab / `/edit`; no picker button in `ide.rs:723` |
| 35 | Editor: visible Save button | Changed | Ctrl+S only (`ide.rs:619`); toolbar has no Save button (`ide.rs:723-812`) |
| 36 | Editor: "Save changes?" prompt when closing a dirty tab | Changed | `ide.rs:603` refuses to close a dirty tab (no save/discard prompt) |
| 37 | Editor: conflict bar "changed on disk" + Reload / Keep mine | Partial | Status text only (`ide.rs:885`); no Reload/Keep-mine actions |
| 38 | Editor: Follow agent + agent change markers | Ported | `ide.rs:801` (toggle); `ide.rs:178` (marks), `ide.rs:885` (bands) |
| 39 | Editor: "Changes (N)" list + "Clear" + file tree pane | Changed | Tree moved to the Files tab (`tabs/files.rs:437`); no agent-changes list with Clear in the IDE tab |
| 40 | Keyboard: Enter send / Shift+Enter newline | Ported | `composer.rs:392` (Submit), rules `composer/rules.rs:6` |
| 41 | Keyboard: **Escape stops the running reply** | **Missing** | Esc now only closes the info popover (`tiles.rs:254`), lightbox (`stream.rs:392`), or denies approval (`approval.rs:114`); no stop binding |
| 42 | Keyboard: Ctrl+N new chat, Ctrl+I panel, Ctrl+, settings | Changed | Ctrl+N→new session, Ctrl+J→panel, Ctrl+K→switcher (`app/settings.rs:339-356`); Ctrl+,/Ctrl+I not bound |
| 43 | Session rename | Ported (net-new) | `sidebar.rs:374` (`start_rename`), `sidebar/menu.rs:76` |
| 44 | Session delete | Ported | `sidebar/menu.rs:87` (with confirmation) |
| 45 | Session context menu (open in split / rename / move to project / delete) | Ported (net-new) | `sidebar/menu.rs:63`; Alt+click split `sessions.rs:257` |
| 46 | Session switcher (Ctrl+K palette) | Ported (net-new) | `switcher.rs:80,268` |
| 47 | "New chat with <agent>" per-agent buttons | Changed | Single "New session" using the default agent (`sessions.rs:278`) |
| 48 | Rescan for agents/models | Changed | `/rescan` (`palette.rs:221`), auto-rescan on enable (`settings/harnesses.rs:205`); no sidebar Rescan button |
| 49 | Default folder for new chats (`Choose…`/`Reset`) | **Missing** | `settings.default_folder` used (`shell.rs:165`) but no settings UI |
| 50 | Settings: modal shell (nav with counts, backdrop/Esc close) | Ported | `settings/mod.rs:261,311,534` (1320×880 at `:293`) |
| 51 | Settings: General (restore, default project, default mode, sandbox, net-ask, show tools, max steps, notify, sound, channel, analytics, export, clear history) | Ported | `settings/general.rs:15` |
| 52 | Settings: data-folder "Open" button (old About section) | **Missing** | `general.rs:15` DATA section has Export + Clear only |
| 53 | Settings: Preview search engine | **Missing** | see #26 |
| 54 | Settings: IDE/VS Code/code-server | **Missing** | see #23 |
| 55 | Window controls: real minimise/maximise/close | Ported | `rpanel.rs:94-100` (`window.minimize_window` etc.) |
| 56 | Headless test commands (`--detect`, `--discover`, `--ask`, `--term-test`, `--tmux-test`, `--preview-shot`) | Ported | `main.rs:31-165` (unchanged) |
| 57 | Approval prompt for "Ask" tool permission | Ported (net-new) | `src/ui/approval.rs:36,128`; backend `engine::approve` |

---

## 2. PROTOTYPE → NEW UI

Control inventory from `tools-and-behaviour.md §C` (items 1–88), specs section by section, cross-checked
against the HTML handlers (`grep` of `onClick/onChange/onKeyDown/onMouseDown/onDoubleClick/onFocus`).

### Left sidebar — collapsed rail (§C 1–5)
| # | Control | Status | Evidence |
|---|---|---|---|
| 1 | Expand sidebar | Done | `sidebar.rs:340` |
| 2 | New session (`+`) | Done | `sidebar.rs:348` |
| 3 | Session buttons, sigil + status dot on rotating bands | Done | `sidebar.rs:298-320`; band formula `theme.band` called at `sidebar.rs:305` (`theme.rs:476`) |
| 4 | Share | Done | `sidebar.rs:362` |
| 5 | Settings | Done | `sidebar.rs:368` |

### Left sidebar — expanded (§C 6–16)
| # | Control | Status | Evidence |
|---|---|---|---|
| 6 | Collapse | Done | `sidebar.rs:165` |
| 7 | Logo + brand name | Done | `sidebar.rs:174-190` |
| 8 | Segmented **Sessions** | Done | `sidebar.rs:213` |
| 9 | Segmented **Projects** | Done | `sidebar.rs:214` |
| 10 | New session + "Alt ⏎" | Done | `sessions.rs:141,155` |
| 11 | Search sessions | Changed | `sessions.rs:327`; matches **title + message text** (`sessions.rs:311`) vs prototype name+project |
| 12 | Session rows (sigil, badge, name, harness badge, time) | Done | `sessions.rs:160,229` |
| 13 | Project cards (name, count, path, branch, language) | Done (real) | `projects.rs:87`; probe `projects.rs:25-36` |
| 14 | "+ Open folder" | Done (real) | `projects.rs:191-208`, picker `projects.rs:246` |
| 15 | Footer Settings | Done | `sidebar.rs:241` |
| 16 | Footer Share | Done | `sidebar.rs:261` |

### Tiles (§C 17–39)
| # | Control | Status | Evidence |
|---|---|---|---|
| 17 | mousedown focuses tile | Done | `tiles.rs:159` |
| 18 | `+` split new session | Done | `tiles.rs:531` |
| 19 | `×` close tile | Done | `tiles.rs:539` |
| 20 | Tool-call rows (icon, kind, target, result) | Done | `stream/blocks.rs:155,175` |
| 21 | Chart **Copy CSV** | Done | `blocks.rs:578` |
| 22 | Code **Copy** | Done | `blocks.rs:433` |
| 23 | Image area → lightbox | Done | `blocks.rs:631-650` |
| 24 | Image **Open** | Done | `blocks.rs:671` |
| 25 | Image **Attach** | Done | `blocks.rs:672` |
| 26 | User-message attachment chips (info only) | Done | `blocks.rs:767` |
| 27 | Composer chip **×** remove | Done | `composer.rs:531` |
| 28 | Composer placeholder per mode | Done | `composer/rules.rs:6`; `composer.rs:349` |
| 29 | Attach button (paperclip) | Done | `composer.rs:164` |
| 30 | Attach popover (repo files, screenshot, terminal, upload) | Done (real) | `composer/attach.rs:106-160` |
| 31 | Attach popover "ATTACH" header | Changed | No header (rows only) `attach.rs:160` |
| 32 | Model chip (mode icon, model, effort moon + label) | Done | `picker.rs:150` |
| 33 | `MODE` header | Done | `picker.rs:261` |
| 34 | Mode cards Auto/Plan/Ask (with sub-lines) | Done | `picker.rs:206-243` |
| 35 | `MODEL` header | Done | `picker.rs:263` |
| 36 | Model rows (name + `harness · ctx`, ✓) | Partial | `picker.rs:286,349`: ✓ and name/description, but no `harness · ctx` sub-line and no pin affordance |
| 37 | Effort track + moon buttons | Done | `picker.rs:358-470` |
| 38 | `EFFORT · N LEVELS` header + labels | Done | `picker.rs:455-470` |
| 39 | Send / Stop button | Done | `composer.rs:181-212` |

### Tiling (§C 40–41)
| # | Control | Status | Evidence |
|---|---|---|---|
| 40 | Split handles: drag (0.12–0.88), dbl-click reset | Done | `tiles.rs:189,367`; `tiling.rs:257,290` |
| 41 | Panel edge: drag, dbl-click reset 480 | Done | `rpanel.rs:258-283`; `tiling`→`app/workspace.rs:11` |

### Right tools panel (§C 42–68)
| # | Control | Status | Evidence |
|---|---|---|---|
| 42 | Collapse chevron | Done | `rpanel.rs:184` |
| 43 | Tabs Browser/IDE/Files/Git/Terminal | Done | `rpanel.rs:27,180,218` |
| 44 | Rail tab icons | Done | `rpanel.rs:237` |
| 45 | Minimize/Maximize/Close | Done (real) | `rpanel.rs:94-100` (not a toast) |
| 46 | Browser back/forward/reload | Done (real) | `browser.rs:353-355` |
| 47 | Browser URL input | Done (real) | `browser.rs:307` (`address_input`) |
| 48 | Mock page | Changed | Real headless Chrome (`browser.rs:415` `web_page`) |
| 49 | IDE file tabs | Done | `ide.rs:723` |
| 50 | IDE tab × close | Done | `ide.rs:755,603` |
| 51 | IDE code area | Done (editable) | `ide.rs:812` |
| 52 | Files **New file** | Done (real) | `files.rs:370,899` |
| 53 | Files **New folder** | Done (real) | `files.rs:371,899` |
| 54 | Files **Collapse all** | Done | `files.rs:372,885` |
| 55 | Files **Filter** | Done | `files.rs:376` |
| 56 | Files count label | **Missing** | `files.rs:337` header has name/path only |
| 57 | Files tree row click (expand/select) | Done | `files.rs:437,842` |
| 58 | Files dbl-click → open in IDE | Done | `files.rs:842` |
| 59 | Files footer **Open** | Done | `files.rs:655` |
| 60 | Files footer **Attach** | Done | `files.rs:659` |
| 61 | Files footer **Copy path** | Done | `files.rs:663` |
| 62 | Git **Push** | Done (real) | `git.rs:209` |
| 63 | Git change-row stage toggle | Done | `git.rs:294,361` |
| 64 | Git commit textarea | Done | `git.rs:316` |
| 65 | Git **Commit** | Done (real) | `git.rs:330,374` |
| 66 | Git history rows | Done | `git.rs:225` |
| 67 | Terminal click-to-focus | Done | `terminal.rs:156` |
| 68 | Terminal Enter runs a command | Done (real PTY) | `terminal.rs:443` |

### Modals / overlays (§C 69–88)
| # | Control | Status | Evidence |
|---|---|---|---|
| 69 | Share backdrop close | Done | `share.rs:267` |
| 70 | Share × close | Done | `share.rs:267` |
| 71 | Share visibility segmented | Disabled-with-tooltip (no server) | `share.rs:321,383` (`unavailable`) |
| 72 | Share Copy link | Disabled-with-tooltip (no server) | `share.rs:384,407` |
| 73 | Share "Include file diffs / terminal output" | **Missing** | not rendered in `share.rs:267` |
| 74 | Share Invite + input | Disabled-with-tooltip (no server) | `share.rs:433` |
| 75 | Share Markdown / JSON export | Done (real) | `share.rs:225,331-342` |
| 76 | Share PEOPLE rows | Missing (part of disabled invite block) | `share.rs:433` |
| 77 | Settings backdrop/× close | Done | `settings/mod.rs:328,152` |
| 78 | Settings nav, 8 pages | Done | `settings/mod.rs:59,261` |
| 79 | Settings · General | Done | `general.rs:15-265` |
| 80 | Settings · Appearance (identity, presets, background, colours, typography, layout, effects) | Done | `appearance.rs:139,152,306,382,556,672,771,816` |
| 81 | Settings · Keyboard | Done (rebindable, better) | `keyboard.rs:36,61`; conflicts `app/settings.rs:364` |
| 82 | Settings · Harnesses Test/Enable/Install | Done; **+ Add harness** stub | `harnesses.rs:27,65,73`; add is a toast `harnesses.rs:146` |
| 83 | Settings · Models (default radio, enable toggles, default effort, fast titles, long context) | Done | `models.rs:21,48,64,138` |
| 84 | Settings · Tools MCP rows + built-in permissions | Done; **+ Add server** stub | `tools.rs:90,200`; add is a toast `tools.rs:94` |
| 85 | Settings · Skills toggles, Import folder, New skill | Done (real) | `skills.rs:17,118,136` |
| 86 | Settings · Extensions (search, install, remove, toggle) | Done (real) | `extensions.rs:29,123,235` |
| 87 | Lightbox click-to-close | Done | `stream.rs:392` |
| 88 | Toast 1800 ms | Done | `widgets/toast.rs:14,48` |

### Prototype global keys (tools-and-behaviour §B1)
| Key | Prototype | New UI | Evidence |
|---|---|---|---|
| Alt+Enter | split new session | Done | `app/settings.rs:341`; `tiles.rs:264` |
| Alt+Q | close tile | Done | `app/settings.rs:342` |
| Alt+←/→/↑/↓ | move focus | Done (←/→; ↑/↓ handled at `tiles.rs:282`) | `app/settings.rs:343-344` |
| Alt+T | next theme | Done | `app/settings.rs:346` |
| Shift+Tab | cycle mode | Done | `composer.rs:479`; `rules.rs:20` |
| Enter / Shift+Enter | send / newline | Done | `composer.rs:392` |
| Esc | close modal/lightbox | Done (and info popover, approval) | `settings/mod.rs:534`, `stream.rs:392`, `approval.rs:114` |

---

## 3. MOTION

Prototype transitions (`layout.md §6`) vs the new code. Durations/easings live in `src/ui/anim.rs:120-145`;
`Animator` usage was grepped across `src/ui` (only `shell.rs` and `tiles.rs` construct one).

| # | Prototype animated thing | Spec (dur/easing) | New UI | Evidence |
|---|---|---|---|---|
| 1 | wallpaper layer `opacity` | 0.4 s ease | **Not animated** | `WALLPAPER_OPACITY` (`anim.rs:120`) has no user; wallpaper opacity is a static `.opacity()` (`glass.rs:553`) |
| 2 | wallpaper layer `left`/`width` | 0.3 s ease | **Not animated** | `WALLPAPER_EXTENT` (`anim.rs:122`) unused |
| 3 | sidebar collapse/expand `width` | 0.3 s ease | Done | `SIDEBAR_COLLAPSE` (`anim.rs:124`); `shell.rs:122,290`; `widths_changed` `shell.rs:286` |
| 4 | sidebar `background` | 0.3 s ease | **Not animated** | bag is a static glass fill; no animator |
| 5 | right panel `width`/`left` | 0.3 s ease | Done | `PANEL_COLLAPSE` (`anim.rs:126`); `shell.rs:123,291` |
| 6 | right panel `background` | 0.3 s ease | **Not animated** | static glass fill |
| 7 | panel `background` while dragging | 0.3 s ease | **Not animated** | `PANEL_DRAG` (`anim.rs:128`) unused; drag is instant (`rpanel.rs:331`) |
| 8 | tile split/merge/reflow `left/top/width/height` | 0.32 s `cubic-bezier(.2,.8,.2,1)` | Done | `TILE_REFLOW` (`anim.rs:130`); `tiles.rs:101` |
| 9 | tile transition off while dragging a split | none | Done | `INSTANT` (`tiles.rs:99`) |
| 10 | resize-handle grip `background` | 0.15 s ease | **Not animated** | `GRIP` (`anim.rs:133`) unused; grip colour swaps instantly (`tiles.rs:172`, `rpanel.rs:261`) |
| 11 | effort slider fill `width` | 0.25 s ease | **Not animated** | `EFFORT_FILL` (`anim.rs:135`) unused; fill width set directly (`picker.rs:389`) |
| 12 | effort moon buttons `filter`/`color` | 0.2 s ease | **Not animated** | `EFFORT_NODE` (`anim.rs:137`) unused (`picker.rs:426`) |
| 13 | toggle track `background` + knob `left` | 0.2 s ease | **Not animated** | `TOGGLE` (`anim.rs:139`) unused; `widgets/toggle.rs:2` states "The knob doesn't animate yet" |
| 14 | hover states (rows, buttons, chips) | instant | Done (matches) | `.hover(...)` transitions are instant; `HOVER`=`INSTANT` (`anim.rs:143`) |
| 15 | popovers/pickers mount | none | Done (matches) | `POPOVER`=`INSTANT` (`anim.rs:145`); popovers float via `anchored` (`composer.rs:93`) |
| 16 | focus ring / tile blur swap | instant | Done (matches) | ring drawn from `theme.border` (`tiles.rs:109`), no animator |
| 17 | modal open | none | Done (matches) | `settings/mod.rs:261`, `share.rs:267` mount with no transition |
| 18 | toast lifetime | 1800 ms | Done | `widgets/toast.rs:14` (`TOAST_MS`) |
| 19 | "Copied" label resets | 1500 ms | **Missing** | no per-label reset; new UI uses the single 1800 ms toast (`toast.rs:48`) |
| 20 | streaming caret | (none in prototype) | n/a | prototype has no caret; new UI shows italic "Working · N tool calls" (`stream/blocks.rs:103-113`) |
| 21 | reduced motion (`prefers-reduced-motion`) | not in prototype | **Respected**, no UI toggle | `anim.rs:280` (`reduced_motion`), env/`gsettings`; `shell.rs:289`; `set_reduced_motion` (`anim.rs:270`) is never called from settings |

Net: only **sidebar/panel widths** (#3, #5) and **tile reflow** (#8, #9) animate with matching values.
Nine spec transitions are defined constants but unused, and reduced motion is honoured from the OS/env
but has no settings control.

---

## 4. Top gaps (by user impact)

1. **No way to switch the IDE to VS Code (code-server).** `IdeKind` defaults to Built-in (`store.rs:324`);
   the only control (`ide.rs:427`) switches *back* from VS Code. There is also no code-server path /
   autostart setting (old `settings_panel.rs` had both). Fix touches `src/ui/tabs/ide.rs`,
   `src/ui/settings/tools.rs`.
2. **Git panel regressions:** no **Pull**, no **diff view**, no **Stage all / Unstage all**. Old
   `chat/git_panel.rs` had all three. Fix touches `src/ui/tabs/git.rs`.
3. **Model pins have no control.** `pinned_models` + `toggle_pin` exist and drive ordering
   (`picker.rs:303`), but neither Ctrl+P nor a Pin button exists. Fix touches
   `src/ui/composer/picker.rs`, `src/ui/settings/models.rs`.
4. **Details regressions:** the session-info popover (`tiles.rs:563`) drops the old **files list** and
   **activity log** (and the sign-in row). Fix touches `src/ui/tiles.rs`, `src/app/state.rs`.
5. **Motion is mostly unwired.** Effort slider/moons, toggles, grip hovers, panel-drag and wallpaper
   change do not animate though the constants exist. Fix touches `src/ui/widgets/toggle.rs`,
   `src/ui/composer/picker.rs`, `src/ui/tiles.rs`, `src/ui/rpanel.rs`, `src/ui/glass.rs`, `src/ui/anim.rs`.
6. **"Search engine" setting unreachable** (no settings control). Fix touches `src/ui/settings/tools.rs`.
7. **"Default folder for new chats" unreachable** (no settings control). Fix touches
   `src/ui/settings/general.rs`.
8. **Escape no longer stops a running reply** (old Esc=stop). Fix touches `src/ui/composer.rs` /
   global key handling in `src/ui/tiles.rs`.
9. **Editor UX gaps:** no Save button, no open-file picker, no "changed on disk" Reload/Keep-mine
   actions, no dirty-close save prompt, no agent-changes list with Clear. Fix touches
   `src/ui/tabs/ide.rs`, `src/ui/tabs/files.rs`.
10. **Drag-and-drop only works on the composer card** (`composer.rs:247`), not the whole tile as before.
    Fix touches `src/ui/composer.rs` / `src/ui/tiles.rs`.
11. **Settings palette parity:** "Export settings" exists but the data-folder "Open" button is gone; the
    Settings modal replaces the old in-column page (behaviour change, not a defect). Fix touches
    `src/ui/settings/general.rs`.
12. **Share modal:** "Include file diffs / terminal output" toggles are absent (not merely disabled).
    Fix touches `src/ui/share.rs`.
13. **Attach/pending affordances:** no "ATTACH" header and no "N files, up to N, size each" summary.
    Fix touches `src/ui/composer/attach.rs`, `src/ui/composer.rs`.
14. **Model rows lost the `harness · ctx` sub-line** the prototype shows (chat.md §5.3). Fix touches
    `src/ui/composer/picker.rs`.
15. **Sidebar search semantics changed** to title+body text where the prototype filtered name+project
    — good for recall, but note it is a divergence (`sessions.rs:311`).
16. **Manual Git Refresh is gone** (`git.rs:75` auto-refresh); harmless but a control the old UI had.
17. **Files header lost the file-count label** mentioned in the prototype (§C 56). Fix touches
    `src/ui/tabs/files.rs`.
18. **Reduced-motion has no settings toggle**; `set_reduced_motion` exists but is never called. Fix
    touches `src/ui/settings/appearance.rs` (Effects) and wiring in `src/ui/shell.rs`.

*Non-gaps verified:* window controls are real; browser back/forward/reload and URL edit are real; the
attach popover's 7 rows are real (repo files, browser screenshot, terminal output, upload); Skills
import/new, Extensions install/remove, Harness Test/Enable, MCP reconnect, and Markdown/JSON export are
real. `+ Add harness` and `+ Add server` remain toast-only stubs, matching the prototype's `[DEAD]`
markers (`harnesses.rs:146`, `tools.rs:94`).
