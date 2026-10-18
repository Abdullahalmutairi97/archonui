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

Running `--serve`:

- It needs a home folder for its chats (`HOME`, or `ARCHONUI_DATA_DIR` and `ARCHONUI_CONFIG_DIR`), so
  give a systemd unit `Environment=HOME=/root` or `User=`. The cloud store is
  `<data>/cloud`. With `ARCHONUI_DATA_DIR=/x/sessions` it is `/x/cloud`.
- Ctrl-C, `kill` and `systemctl stop` stop running replies as Stop does, save them, and end the agents.
  `kill -9` cannot: the agent keeps running and the reply so far is lost.
- It refuses to share a chats folder with the app or another `--serve` on the same device; give one
  its own `ARCHONUI_DATA_DIR`. The app likewise exits while a `--serve` holds the folder.

Command line, handy for testing: `archonui --remote <device> list | show <chat> | ask <chat> <prompt> |
new <agent> <folder> <prompt>`. `<device>` is a MagicDNS name or an address; when the name does not
resolve (MagicDNS off), it is looked up in `tailscale status` by host name.

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
  `refs/archon/snap/<project>/<device>` (one per project, so worktrees and subfolders of one
  repository don't share it), so HEAD, the index, branches and files are never touched. `.gitignore`
  is respected. A folder that isn't a repository gets a private git dir outside it. Copies to the
  cloud are incremental git bundles of `refs/archon/wip/<device>`, the name the cloud stores.
- **Errors** name the request they answer (`about`: its `type`, and the project for `put_project`), so
  only a failed copy resets that project's copy. From a host older than `about`, any error from the
  cloud while copies are on their way still counts as a failed copy.

## Security

- Hosts listen only on their Tailscale address and refuse anything outside the tailnet. When devices
  are ticked, only those may connect. With sharing off, no tailnet device is accepted. Unticking a
  device, or turning sharing off, closes its open connections too.
- Being on your tailnet is the trust boundary: a connected device can run prompts, and so tools, on
  the host. Only include your own devices.
- Browsers are refused: a handshake with an `Origin` header gets 403, so a web page (DNS rebinding
  included) can't reach `ws://127.0.0.1:7420` or the Tailscale address.
- A connection from this machine itself (loopback, or its own Tailscale address) must come from the
  same user as the host; its owner is read from `/proc/net/tcp`. Other accounts on a shared box, or on
  the VPS where `--serve` runs as root, are refused. Connections from other devices skip this check.
- At most 32 connections are open at once.
- Only a connection following a chat can answer that chat's approvals.
- Ids from the network are checked before they become paths or git refs. A chat from another device
  never replaces one of this device's own chats, and that device's turn events, drafts and approvals
  only reach the chats shown from it.
- The cloud store and the snapshot temp files are owner-only (0700), so other accounts on the cloud
  device can't read the copies.
- Secret-looking files (`.env`, keys, certificates, credential files) are left out of the cloud copy
  unless Settings → Devices → "Copy secret files" is on. `.env.example` and the like are kept.
- A project whose git history ever held a secret file is copied without that history (just the current files),
  unless "Copy secret files" is on.

## Not done yet

- A remote side panel (the host's files, git and terminal).
- Opening a folder copy into an existing folder (it always goes into a new one).

## Testing checklist

1. Minipc: `archonui --serve --bind "$(tailscale ip -4):7420"`.
2. PC: `archonui --remote minipc new claude "~" "say hi"` streams a reply.
3. PC app: Devices → sharing on → tick the minipc → its chats appear, labelled.
4. Type in one: the reply streams; Esc stops it on the minipc.
5. Mark the minipc as cloud; within a reply or five minutes, Devices → Cloud copy lists your projects.
6. Stop the PC app; on the laptop app its chats still show; typing in one continues it locally.
