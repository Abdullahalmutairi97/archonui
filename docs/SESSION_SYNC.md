# Session sync and remote control (design)

Status: proposal, branch `feature/session-sync`.

## Optional by default

Everything here is off until the user turns it on (`SyncSettings::enabled`, Settings → Devices). One
machine without Tailscale and without a cloud copy works exactly as before: no listener, no
`tailscale` calls, no background copies.

## Goals

Three machines: **PC**, **laptop**, **VPS**.

1. **Remote control.** A chat lives on one machine (its *host*) and its agent runs there, but any other
   machine can open it, read it live and send prompts. PC → laptop and laptop → PC both work.
2. **Cloud copy.** Every project is mirrored to the VPS automatically, including uncommitted and
   untracked work, so a project left mid-edit on the PC is still reachable from the laptop when the PC is off.

## Why it fits ArchonUI

- A turn is already a separate process: `engine::session::start_turn` spawns the CLI and turns its
  output into `parse::Out` events. Those events are the whole wire protocol a remote viewer needs.
- `store::Session`, `Message`, `ToolCall`, `Usage` and `Todo` are already `serde` types.
- Tool approvals already go through a socket with a token (`engine::approve`), so forwarding them is
  a known shape.
- Chats can already change agent mid-conversation (`engine::handoff`), which is how a chat continues on
  another machine when its host is offline (see below).
- `tungstenite` and `notify` are already dependencies.

## Architecture

```
      laptop (UI) ◄──── tailnet (ws) ────► PC (UI + host)
              │                                 │
              └────────►  VPS (mirror)  ◄───────┘
                      always on, snapshots + chat history
```

**Network: Tailscale.** All three machines are on one tailnet, so they reach each other directly by
MagicDNS name (`pc`, `laptop`, `vps`) through NAT, with WireGuard encryption. That means:

- No relay is needed for remote control; the laptop connects straight to the PC.
- No TLS or public ports: hosts listen only on their Tailscale address (100.x / `tailscale0`), never
  on `0.0.0.0`.
- The VPS's job shrinks to the always-on mirror (projects + chat history).
- Any device on the tailnet may connect: being on the user's Tailscale network is the trust boundary.
  When the user has picked devices (below), only those may connect.

### Devices settings

Settings → Devices lists what `tailscale status --json` reports (`remote::tailscale`): this device and
its peers, online first. The user ticks which devices join the workspace and marks one as the
**cloud** (the always-on copy); any number of devices works, three is just the example. The choice
is saved as `SyncSettings { enabled, port, devices, cloud }`.

Without Tailscale, the fallback is every machine connecting outbound to the VPS as a relay.

### 1. Host mode: `archonui serve`

A headless mode of the same binary (no GPUI window). It owns the sessions stored on that machine and
runs their turns. The desktop app runs the same host loop in-process, so a PC with the app open is
also a host.

### 2. Protocol

JSON messages over a WebSocket on the host's Tailscale address (e.g. `ws://pc:7420`). `ssh host archonui
serve --stdio` also works and is handy for testing. Client → host:

| message | meaning |
|---|---|
| `hello {machine, token, version}` | authenticate |
| `list_sessions` / `subscribe {session}` | sidebar and live view |
| `prompt {session, text, attachments, mode, model, effort}` | start a turn on the host |
| `cancel {session}` | Esc |
| `approve {request, verdict}` | answer a tool approval |
| `draft {session, text}` | keep drafts in step |

Host → client: `sessions [...]`, `snapshot {session}`, `event {session, Out}`, `finished {session, result}`,
`approval_request {...}`, `host_status`.

`parse::Out` needs `#[derive(Serialize, Deserialize)]`; everything inside it already has it.

### 3. Sessions in the UI

- `Session` gains `host: String` (machine id). Local chats keep working unchanged.
- The sidebar groups or badges chats by host; a remote chat whose host is offline is read-only with a
  "Continue here" button.
- **Continue here** copies the chat to this machine and uses the existing handoff path: the agent's own
  session (e.g. Claude's resume id) lives on the PC and can't be resumed elsewhere, so the first prompt
  carries the conversation as text, exactly like switching agents.

### 4. Project mirror on the VPS

Plain file sync (rsync, Syncthing) is risky with two machines editing: last write wins silently.
Instead, use git without touching the user's branch or index:

1. On change (debounced `notify` watcher) or every N minutes, build a snapshot commit with a temporary
   index: `GIT_INDEX_FILE=tmp git add -A && git write-tree && git commit-tree`.
2. Push it to `refs/archon/wip/<machine>` on a bare repo on the VPS.
3. On another machine, "Open from cloud" checks the snapshot out into the project folder (or a worktree),
   and shows a diff first if local files differ.

This captures uncommitted and untracked files, respects `.gitignore` (no `target/`, `node_modules/`),
never rewrites the user's commits, and keeps history so a bad sync can be undone. Projects that are not
git repos get `git init` into a separate `GIT_DIR` under the ArchonUI data folder so the project itself is
untouched.

Session JSON files and attachments are mirrored to the VPS too, so chat history is visible from the
laptop even when the PC is off.

### 5. Paths

Machines have different home folders. Projects are matched by project id plus a per-machine path map
(`projects.toml`), never by absolute path.

### 6. Security

- Hosts listen only on their Tailscale address and refuse addresses outside the tailnet, and outside
  the picked devices when the user has picked some.
- A remote prompt can run any tool the agent can, so sharing is off by default.
- Secrets (`.env`) are excluded from the mirror unless the user opts in.

## Phases

1. Serializable `Out`, `archonui serve` listening on the Tailscale address, remote chats in the sidebar (prompt, stream,
   cancel). **Delivers PC ↔ laptop control.**
2. Approvals and drafts over the wire; host online/offline state; "Continue here".
3. Host discovery on the tailnet (`tailscale status --json`), so other machines appear without typing addresses.
4. WIP snapshots to the VPS and "Open from cloud".
5. Remote side panel: files, git and terminal of the host (terminal over the same socket via `portable-pty`).
