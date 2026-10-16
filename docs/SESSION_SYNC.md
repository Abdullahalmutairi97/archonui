# Sharing chats between devices

Status: built on branch `feature/session-sync`. Off by default.

## What it does

- **Use one device's chats from another.** A chat lives on the device where it was started; its agent
  runs there. Your other devices show it in the sidebar with that device's name, and anything you
  type in it runs on that device, with the reply streaming in live. Stop and Esc reach that device.
  When the agent asks to use a tool, the approval card shows on every device following the chat;
  the first answer wins.
- **A cloud copy.** One device can be marked as the cloud (an always-on VPS, say). The others copy
  their projects to it, uncommitted and untracked work included, plus their chats, every five minutes
  and after each reply.
- **Away devices.** While a device is off, its chats still show (from the cloud). Writing in one
  continues it on the device you're on, with the conversation so far carried over, like switching
  agents. Settings → Devices → Cloud copy opens any project copy in a new folder.

Everything is off until Settings → Devices → "Share chats between devices" is on. One machine without
Tailscale works exactly as before: no listener, no `tailscale` calls, no background copies.

## Setting it up

1. Install Tailscale on each device and sign in to the same tailnet.
2. On each device running the app: Settings → Devices → turn sharing on, tick the devices to include,
   and mark one as the cloud.
3. On a device without a screen (a VPS or mini PC): `archonui --serve`. It uses the same settings
   file; or pass `--bind <tailscale-ip>:7420` to run without them. A `--serve` device keeps a cloud
   store, so it can be the cloud.

Command line, handy for testing: `archonui --remote <device> list | show <chat> | ask <chat> <prompt> |
new <agent> <folder> <prompt>`.

## How it works

```
      laptop (app) ◄──── tailnet (WebSocket :7420) ────► PC (app)
              │                                           │
              └──────────►  VPS (archonui --serve)  ◄─────┘
                         cloud: project snapshots + chats
```

| Piece | File | Role |
|---|---|---|
| Settings | `remote/mod.rs` | `SyncSettings { enabled, port, devices, cloud }` |
| Discovery | `remote/tailscale.rs` | `tailscale status --json` → this device and its peers |
| Protocol | `remote/proto.rs` | JSON messages, one per WebSocket frame |
| Sockets | `remote/hub.rs` | Listens on the Tailscale address, handshake, subscriptions |
| Chat logic | `remote/host.rs` | Answers requests with `AppState`; also the headless `--serve` loop |
| Client | `remote/client.rs` | Connects to another device; `--remote` command line |
| Snapshots | `remote/snapshot.rs` | Git snapshots of working files, bundles, restore |
| Cloud store | `remote/cloud.rs` | Bare repo per project, chats per device |
| Window | `ui/remote.rs` | Hosts the window's chats, shows other devices', backups |

- **Remote chats** are kept in `AppState` with `Session::origin` set to their device. They are never
  saved here; `remote_started`/`remote_event`/`remote_finished` mirror the host's turn.
- **Snapshots** use a temporary index (`GIT_INDEX_FILE`) and `commit-tree` onto
  `refs/archon/wip/<device>`, so HEAD, the index, branches and files are never touched. `.gitignore`
  is respected. A folder that isn't a repository gets a private git dir outside it. Copies to the
  cloud are incremental git bundles.

## Security

- Hosts listen only on their Tailscale address and refuse anything outside the tailnet. When devices
  are ticked, only those may connect. With sharing off, no tailnet device is accepted.
- Being on your tailnet is the trust boundary: a connected device can run prompts, and so tools, on
  the host. Only include your own devices.
- Ids from the network are checked before they become paths or git refs.
- `.env` files that aren't ignored are included in the cloud copy; add them to `.gitignore` to keep
  them out.

## Not done yet

- Attachments in remote chats.
- A remote side panel (the host's files, git and terminal).
- Opening a folder copy into an existing folder (it always goes into a new one).

## Testing checklist

1. Minipc: `archonui --serve --bind "$(tailscale ip -4):7420"`.
2. PC: `archonui --remote minipc new claude ~ "say hi"` streams a reply.
3. PC app: Devices → sharing on → tick the minipc → its chats appear, labelled.
4. Type in one: the reply streams; Esc stops it on the minipc.
5. Mark the minipc as cloud; within a reply or five minutes, Devices → Cloud copy lists your projects.
6. Stop the PC app; on the laptop app its chats still show; typing in one continues it locally.
