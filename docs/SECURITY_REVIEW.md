# ArchonUI security review (read-only)

Scope: the surfaces named in `.archonui-dev/tasks/security.md`.

Method: source reading only. No source files were changed, no GUI was driven, no network
was touched. Evidence is quoted as `file:line` against the working tree as it stood at
**2026-10-13 ~08:45 (+03)**. The tree has no `.git` and was being edited concurrently while
this review was written (`src/ui/tabs/files.rs`, `src/ui/settings/general.rs`,
`src/ui/share.rs`, `src/app/settings.rs` all changed within the review window), so treat the
line numbers as a snapshot and re-grep by function name if the tree moved on. Every finding
below was re-checked against the current file contents at the end of the review.

Summary: **1 high, 3 medium, 4 low** findings. No shell-injection class bug was found — argv
is always built as an array (`std::process::Command::args`), never as a shell string.

| # | Severity | Surface | Finding |
|---|----------|---------|---------|
| H1 | High | `engine/approve.rs` | Approval socket has no peer authentication; any same-uid process can inject a fake approval card and poison the per-chat "Always allow" cache |
| M1 | Medium | `ui/settings/general.rs` | "Export settings" says API keys are left out, but MCP HTTP headers and stdio env (bearer tokens / keys) are exported verbatim |
| M2 | Medium | `engine/harness/codex.rs` | Codex MCP secrets (`env`, `http_headers`) are placed on the command line, world-readable in `/proc/<pid>/cmdline` |
| M3 | Medium | `engine/approve.rs` | Approval socket falls back to world-writable `/tmp` with a predictable name and a bind→chmod race; unbounded line reads and a thread per connection |
| L1 | Low | `app/persist.rs`, `store.rs`, `ide.rs` | ArchonUI's data files are created with the default umask (0644/0755); settings, sessions and code-server profile are world-readable on a multi-user host |
| L2 | Low | `preview.rs` | Headless Chrome profile dir is a predictable path in `/tmp`; `remove_stale_profiles` recursively deletes matching `/tmp` entries |
| L3 | Low | `engine/plugins.rs` | Extension spec is passed to the harness CLI without `--`; a crafted marketplace id can be read as a flag |
| L4 | Low | `app/notify.rs`, `ui/tabs/files.rs`, `preview.rs` | Minor: unsanitized notify app name; file tree follows symlinks for read; agent output can auto-navigate the preview to `file://` |

---

## H1 — Approval socket accepts any local process; no peer authentication

**Severity: High** (same-user attacker, or a sandboxed same-uid process such as a spawned
agent, can influence approvals).

**Evidence**
- `src/ui/approval.rs:38-50` starts `approve::listen(approve::socket(), …)` at app start, so
  the socket is live whenever the app runs.
- `src/engine/approve.rs:114-115` binds `$XDG_RUNTIME_DIR/archonui-<pid>.sock` and chmods it
  0600. Ownership is the *only* access control: there is no `SO_PEERCRED` check and no
  per-request secret.
- `src/engine/approve.rs:128-151` (`serve_call`) trusts the JSON line on the connection: the
  `session` and `tool_name` fields are attacker-chosen, and
  `if allowances.allowed(&call.session, &call.tool_name) { Verdict::Allow }` returns an
  allow **without asking the UI**.
- `src/ui/approval.rs:73-89` (`answer_approval`) caches the user's answer with
  `self.approvals.allowances.allow(session, tool)` when the card is answered with
  "Always allow", keyed only by (session string, tool name).

**Impact (escalation chain).** A same-uid process — including a sandboxed agent process that
shares the uid, which is exactly the deployment this project cares about — can
1. connect to the socket (0600 is satisfied by same-uid),
2. send `{"session":"<chat id>","tool_name":"Bash","input":{…}}`. If that chat already has
   "Always allow" for the tool, it receives `{"behavior":"allow",…}` back (decision leak);
3. send a spoofed card into a live chat (`approval_arrived`, `src/ui/approval.rs:56`). If the
   user clicks "Always allow", the cache is poisoned: the *real* Claude turn in that chat then
   runs `Bash` with no prompt (`approve.rs:137-139`).
The `session` value is not secret: it is the chat id, visible to any local user in
`/proc/<pid>/cmdline` of the Claude child, because it is passed as an argv element
(`src/engine/harness/claude.rs:185-189`, `main.rs:50-58`).

**Fix (concrete)**
1. On each accepted connection, read `SO_PEERCRED` (libc `getsockopt(SOL_SOCKET,
   SO_PEERCRED)`) and drop the connection unless `ucred.uid == geteuid()`. This alone closes
   the cross-uid path.
2. Add a per-app random token: generate it once next to the socket, pass it to the child
   through the `--approval-mcp` argv (or better, the 0600 MCP config file already used for
   secrets) and require it in every `Call`. Same-uid sandboxes without access to the token
   are then rejected.
3. Keep the socket in a directory the app owns with mode 0700 (see M3), not `/tmp`.
4. Do not cache an allowance whose `session` the app cannot map to a live chat id (reject
   calls for unknown sessions).

---

## M1 — Settings export claims keys are excluded but exports MCP secrets

**Severity: Medium** (misleading UI + credential disclosure when a user shares the export).

**Evidence**
- The button text: `src/ui/settings/general.rs:288` — "Themes, providers, tools and
  keybindings as JSON. API keys are left out."
- The implementation serializes the **whole** `AppSettings` with no redaction:
  `src/ui/settings/general.rs:332-341` (`serde_json::to_string_pretty(&settings)`).
- `AppSettings.mcp_servers` is a plain serialized field (`src/app/settings.rs:95`,
  `McpServer` at `:281`), and `McpTransport` carries the secrets:
  `Http { headers: BTreeMap<String,String> }` and `Stdio { env: … }`
  (`src/app/settings.rs:290-305`). A remote MCP server's `Authorization: Bearer …` header or
  a stdio server's `GITHUB_TOKEN` env therefore lands in the exported JSON.

**Fix**
- Before export, clone the settings and blank/redact every `McpTransport::Http.headers` value
  and `McpTransport::Stdio.env` value (keep names, replace values with `"***"`), or add
  `#[serde(skip_serializing)]`-style redaction via a dedicated export DTO.
- If redaction is intentionally out of scope, change the label to say credentials **are**
  included.

---

## M2 — Codex receives MCP secrets on the command line (world-readable)

**Severity: Medium** (local information disclosure; secrets persist for the life of the turn).

**Evidence**
- `src/engine/harness/codex.rs:87-89` builds one `-c <key>=<value>` pair per MCP setting;
  `server_overrides` emits `mcp_servers.<name>.env={…}` (`:148`) and
  `mcp_servers.<name>.http_headers={…}` (`:154`) through `toml_table` (`:171`).
- These values become argv of the `codex` child. On Linux `/proc/<pid>/cmdline` is 0444 and
  readable by every local user, so bearer tokens / API keys are exposed for the whole turn.
- Contrast with the deliberate design elsewhere: `src/engine/mcp.rs:261-262` says the
  `--mcp-config` file exists "so tokens stay out of argv", and
  `src/engine/session.rs:147-151` writes it 0600.

**Fix**
- Write the Codex MCP overrides to the same 0600 config file (`mcp::config_file`-style) and
  pass Codex only the file path, or pass the secret values through the child environment
  (owner-only `/proc/<pid>/environ`) instead of `-c`. Keep non-secret keys (`enabled`,
  `command`, `args`, `url`) on `-c` if that is simplest.

---

## M3 — Approval socket in `/tmp`: predictable name, chmod race, unbounded reads

**Severity: Medium.**

**Evidence**
- `src/engine/approve.rs:97-101` — socket path is
  `$XDG_RUNTIME_DIR/archonui-<pid>.sock`, else `env::temp_dir()` (`/tmp`, mode 1777).
- `src/engine/approve.rs:114-115` — `UnixListener::bind` **then**
  `set_permissions(0o600)`. Between the two calls the socket inode has umask-derived
  permissions (typically 0755) inside a world-writable directory, so another local user can
  `connect(2)` during that window (`connect` to a Unix socket needs write permission on the
  inode). The PID in the name is trivially learned from `/proc`.
- `src/engine/approve.rs:135` and `:157-161` — `read_line` into an unbounded `String` on both
  the socket and the stdio server; `src/engine/approve.rs:117-121` spawns one thread per
  accepted connection with no cap. A client can exhaust memory/threads.

**Positive note.** Timeouts are handled correctly: the UI wait is capped at
`TIMEOUT` (`:25`, `serve_call`) and the client side sets a read timeout
(`ask_app`, `:177`); timeout/disconnect deny by default. Stale sockets are removed before
bind (`:113`). Those are good.

**Fix**
- Create an owner-only directory (mode 0700) under `temp_dir()` when `XDG_RUNTIME_DIR` is
  absent, and bind the socket inside it, so the chmod race is unreachable by other users.
- Set the process `umask` (or bind with `socket2` and set the mode before publishing) so the
  socket is never momentarily group/other-accessible.
- Cap the line length (`take(MAX)` before `read_line`, reject overlong calls), and bound the
  number of concurrent connections.

---

## L1 — ArchonUI data files use the default umask

**Severity: Low–Medium** on shared machines (single-user desktops unaffected).

**Evidence**
- `src/app/persist.rs:165-173` (`write_atomic`) uses `fs::File::create` → 0644; this writes
  `settings.json` (the file whose MCP headers/env are the M2/M1 secrets).
- `src/store.rs:442-447` (`save_to`) and `:362-369` (`save_settings`) use `fs::write` → 0644;
  session JSON holds full conversation text, working directories and attachment names.
- `src/ide.rs:42` creates `~/.local/share/archonui/code-server` with `create_dir_all` (0755);
  code-server stores its own settings/globalStorage there.
- The only hardening in the tree is `session.rs:151` (MCP config, 0600),
  `approve.rs:115` (socket, 0600) and the editor preserving an existing file's mode
  (`editor/buffer.rs:788`).

**Fix**
- Create `~/.config/archonui`, `~/.local/share/archonui` and subdirectories with 0700, and write
  settings/session files 0600 (`OpenOptions … .mode(0o600)` like `session.rs:151`), or set a
  restrictive process umask at startup.

---

## L2 — Headless Chrome profile in `/tmp`; stale-profile deletion

**Severity: Low.**

**Evidence**
- `src/preview.rs:80-102` — profile is
  `env::temp_dir()/archonui-browser-<pid>-<n>`, a predictable path in a world-writable directory;
  Chrome is handed it via `--user-data-dir`. A local attacker can race the launch and
  pre-create/symlink that path to influence what Chrome reads (e.g. a pre-seeded profile).
- `src/preview.rs:184-203` (`remove_stale_profiles`) walks `/tmp`, matches any entry named
  `archonui-browser-<digits>-…` and `remove_dir_all`s it when `/proc/<pid>` is gone. The
  attacker controls the on-disk name and can point it at paths of their choosing.
- Mitigations that are present and good: `--remote-debugging-port=0` binds loopback on a
  random port (`:93`), the DevTools websocket is a loopback connection, the process is killed
  as a group on drop (`preview.rs:137-142`), and `--allow-file-access-from-files` is **not** set, so a stray
  `file://` page cannot read other local files.

**Fix**
- Create the profile with `fs::create_dir` + `0700` (or `mkdtemp`) under an owner-only parent,
  and only delete directories the app itself created in this run (track paths in memory
  rather than pattern-matching `/tmp`).

---

## L3 — Extension install/remove pass an untrusted spec as a bare argument

**Severity: Low.**

**Evidence**
- `src/engine/plugins.rs:52-70` runs `claude plugin install <spec>` / `pi install <spec>` (and
  `uninstall`/`remove`) via `run` (`:228`), which uses argv (no shell), but no `--` separator
  before `spec`.
- `spec` comes from parsing the CLIs' own listings (`claude_extensions_of`, `pi_installed`,
  `split_id` `:106`), so a marketplace entry whose id starts with `-` is interpreted as a flag
  by the target CLI rather than as the plugin spec.

**Fix**
- Insert `--` before `spec`, or reject a `spec` that starts with `-`, in `install`, `remove`
  and `set_enabled`.

---

## L4 — Minor hardening notes

- **`notify-send` app name** (`src/app/notify.rs:60-70`): summary and body are correctly placed
  after `--`, but the app name is emitted before it (`-a <app>`); an app name from settings
  starting with `-` is parsed as a `notify-send` option. Cosmetic (the value is
  user-controlled), but pass `-a=<app>` or validate it. `spawn` (`:94`) uses argv, no shell.
- **File tree follows symlinks for reads** (`src/workspace.rs:47-72`): `list_dir` guards with
  `dir.starts_with(root)` and the walker does not descend symlinks, but `is_dir` uses
  `path.is_dir()`, so a symlink inside the root to an outside directory can be expanded and
  listed. Read-only display of the user's own files; no delete/rename path follows a symlink
  (`ui/tabs/files.rs:1203` rejects `/`, `\`, `.`, `..`; create uses `create_new` O_EXCL at
  `:1020`; `delete_for_good` at `:1283` and `gio trash` at `:1274` operate on the symlink, not
  its target).
- **Auto-preview to `file://`** (`src/preview.rs:804-836` `find_preview_target`,
  `src/app/state.rs:497`): an agent reply containing a `file://` URL or an existing `.html`
  path auto-navigates the preview when `auto_open_preview` is on (default). Only local
  targets are auto-opened (public URLs are ignored). Low risk given the absence of
  `--allow-file-access-from-files`; consider gating `file://` auto-open behind the setting too.
- **MCP config files are never deleted** (`src/engine/mcp.rs:284`): `mcp/claude-<chat>.json`
  and `mcp/pi.json` (0600) persist after a chat is removed, leaving stale copies of MCP
  headers/env. Clean them up when the session is deleted.
- **Dead but dangerous** (`src/engine/mcp.rs:35-84`): `import_servers` reads a project's
  `.mcp.json` from the chat's `cwd`. It is currently unused (`#![allow(dead_code)]`), so no
  repo can auto-register a server that `check()` would spawn. If it is ever wired to the UI,
  a repository could make ArchonUI spawn an attacker command.

---

## Checked and found fine

- **No shell injection anywhere in production code.** Every process launch uses
  `std::process::Command` with an argv array; the only `sh -c`/`bash -c` occurrences are
  tests (`src/ide.rs:393`, `src/engine/plugins.rs:308-317`) and the shell-wrapper *parser*
  (`src/engine/parse.rs:441-446`).
- **Prompt/flag confusion is guarded.** `harness/mod.rs:238-244` (`prompt_arg`) prefixes a
  prompt starting with `-` with a space; prompts are always the last positional argument.
- **MCP config escaping.** Codex TOML keys/values are escaped by `toml_string`
  (`codex.rs:162-177`); Claude/Pi config is real JSON (`mcp.rs:249-284`); OpenCode builds a
  JSON object (`opencode.rs:152-188`). Server names/args/env cannot break out of the config.
- **Secrets kept out of argv on the Claude/Pi path.** The `--mcp-config` file is written 0600
  (`session.rs:147-151`) under ArchonUI's data dir and only its path is passed on argv; the
  Claude `--settings` argument (`claude.rs:74-77`) contains sandbox flags only, no secrets.
  OpenCode passes secrets in `OPENCODE_CONFIG_CONTENT` (`opencode.rs:152`, owner-readable
  `/proc/<pid>/environ`), which is the better of the two env styles.
- **MCP `check()` runs only from user action.** The only callers are the Tools settings page
  click (`ui/settings/mod.rs:403`), the enable toggle (`ui/settings/tools.rs:224`) and the
  per-row test button (`tools.rs:295`); `engine::mcp::check` (`mcp.rs:290`) spawns the
  configured command (`mcp.rs:359`) with its own process group and kills the group on drop
  (`mcp.rs:390-396`). No path spawns a server from a repository.
- **IDE (`ide.rs`).** Binds `127.0.0.1:0` with `--auth password` and an empty config file so
  the user's own code-server config cannot change the login (`:44-63`); the password is 16
  bytes from `/dev/urandom` written to the child env, not argv (`:48`, `:63`, `:316-323`);
  the login POST is loopback-only and the cookie is parsed and then set `httpOnly` in the
  preview browser (`ide.rs:118-140`, `chat/preview_panel.rs:305`); `url_for` percent-encodes
  the folder (`ide.rs:113-127`); `--reuse-window` passes the file as its own argv element
  (`ide.rs:215-229`); the process gets `PR_SET_PDEATHSIG` and a group kill on drop.
- **Preview (`preview.rs`).** DevTools on a random loopback port (`:93`); `normalize_address`
  (`:712`) treats only `http(s)`, `file`, `about`, `data` as addresses and routes
  `javascript:`, `chrome:`, `chrome-extension:`, `view-source:`, `devtools:` and unknown
  schemes to a search (`:726-730`); `find_preview_target` auto-opens only local hosts and
  existing `.html` files (`:804-836`); each launch gets its own profile and the Chrome process
  group is killed and the profile removed on drop (`:137-142`).
- **Terminal (`terminal.rs`).** tmux session names are sanitized
  (`tmux_session_name`, `:345-353`); tmux runs on a dedicated `-L` socket
  (`tmux_socket`, `:356-358`) in tmux's own owner-only `$TMPDIR/tmux-<uid>` directory, so
  cross-user access is tmux's 0700 dir, not shared; no shell string is built for tmux.
- **Share export (`ui/share.rs`).** Exports only the focused session (Markdown or the stored
  JSON), to a path the user picks through the native dialog (`:349-374`); it re-serializes
  session data only — no settings, no MCP config, no code-server cookies.
- **Approval protocol (`approve.rs`).** Deny-by-default: unknown JSON-RPC methods error,
  malformed JSON gets `-32700`, a missing app/socket denies (`ask_app`), a dropped request
  denies, and the tool schema is fixed to the one `approve` tool (`:194-261`). Allowances are
  in-memory only (`:74-79`).
- **MCP over HTTP (`mcp.rs:463-520`).** Requests go through `curl` with `--max-time` and
  argv headers; the deadline is enforced per request and the child is killed on drop.

## Not reviewed (out of scope)

- The editor buffer, git tab, thumbs, and the headless `--ask`/`--detect`/`--preview-shot`
  CLI paths beyond the argv-construction points already covered.
- Anything requiring a running GUI, a network, or real credentials (the task asked for a
  read-only source review).

## Fixed

Fix pass on the findings above except the approval-socket ones (H1, M3), which are owned
elsewhere. No GUI was driven; `cargo build` is clean for the files touched, and the tests
listed below pass.

- **M1 — Settings export redacts MCP secrets** (`ui/settings/general.rs`). `export_settings`
  now runs the settings through `redact_secrets`, which keeps every MCP server, header and
  env *name* but replaces all HTTP `headers` and stdio `env` *values* with `"<redacted>"`.
  The button copy is updated to "API keys and MCP credentials are left out.", which is now
  true. Test: `ui::settings::general::tests::an_export_blanks_mcp_credentials`.
- **M2 — Codex MCP secrets off the command line** (`engine/harness/codex.rs`). `mcp_args` no
  longer emits `mcp_servers.<n>.env=…` or `.http_headers=…`; the secret values go into the
  child's environment (`Plan.env`, owner-only `/proc/<pid>/environ`) and Codex references them
  by name: stdio values become `mcp_servers.<n>.env_vars=["NAME", …]` (forwarded from the
  parent env), HTTP headers become `mcp_servers.<n>.env_http_headers={"Header" = "ARCHONUI_MCP_…"}`
  with a generated, collision-free env name. Verified against `codex --help` and the Codex
  config reference (`mcp_servers.<id>.env_vars`, `.env_http_headers`,
  `.bearer_token_env_var`). A stdio value whose name another server already claimed with a
  different value is skipped and reported (`eprintln!`), since forwarding cannot rename it.
  Tests: `engine::harness::tests::codex_moves_stdio_env_off_the_command_line_by_name`,
  `engine::harness::tests::servers_reach_claude_pi_and_codex`.
- **L1 — ArchonUI data files are owner-only** (`store.rs`, `app/persist.rs`, `engine/session.rs`,
  `ide.rs`). New `store::create_private_dir` creates directories 0700 (`DirBuilderExt`).
  `write_atomic` (settings/workspace/projects) and `save_settings`/`save_to` (sessions) write
  through `OpenOptionsExt::mode(0o600)`; attachments are copied then chmodded 0600, and pasted
  bytes use the same 0600 path. `session.rs`'s `--mcp-config` directory and `ide.rs`'s
  code-server data dir are created 0700.
- **L2 — Chrome profile is not a predictable `/tmp` path** (`preview.rs`). Profiles now live
  under `profile_root()`: `$XDG_RUNTIME_DIR/archonui` (0700) when set, else `browser/` under
  ArchonUI's data dir. Each profile is created 0700 and stamped with a `.archonui-chrome-profile`
  marker. `remove_stale_profiles(root)` deletes only directories carrying the marker, only
  when their pid is gone, and uses `symlink_metadata` so a symlink is never followed or
  removed. Test: `preview::tests::stale_profiles_are_removed_without_following_symlinks`.
- **L3 — Extension specs cannot be flags** (`engine/plugins.rs`). Specs starting with `-` are
  rejected (`safe_spec`) for install, remove and enable/disable. Claude's CLI also gets `--`
  before the spec (verified: `claude plugin install -- x` treats `x` as the plugin). Pi's CLI
  rejects `--` ("Unknown option --"), so it relies on the reject check alone. Test:
  `engine::plugins::tests::specs_that_look_like_flags_are_refused`.
- **L4 — notify app name cannot be an option** (`app/notify.rs`). `notify_args` now passes the
  app name as `--app-name=<app>` instead of `-a <app>`, so a name starting with `-` is a value.
- **L4 — file tree does not expand symlinked directories** (`workspace.rs`). `list_dir` uses
  `entry.file_type()` (no symlink following) instead of `path.is_dir()`, so a link out of the
  root is displayed but not descended into. Test:
  `workspace::tests::list_dir_does_not_expand_symlinked_directories`.
- **L4 — per-chat MCP config is deleted with the chat** (`engine/mcp.rs`, `store.rs`).
  `mcp::session_file` names the 0600 file `claude-<id>.json`; `store::delete` removes it
  alongside the session and its attachments. (The shared `claude.json`/`pi.json` files are
  rewritten each turn and are left in place.)

Still open: **H1** and **M3** (approval socket peer authentication, `/tmp` socket, unbounded
reads) — owned by another worker.
