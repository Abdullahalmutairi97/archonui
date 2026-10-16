# Handoff: session sync (test and finish)

For the agent on the minipc. You can SSH into **ashpc** (the user's desktop PC) and **vps**. Your
job: build branch `feature/session-sync` on the three machines, run the test plan below, fix what
breaks, and report back to the user.

Read `docs/SESSION_SYNC.md` first. It explains the feature, the files, and the design.

## Context in one paragraph

archonUI can now share chats between the user's devices over Tailscale. A chat lives on the device
where it started, and its agent runs there. Other devices show it in the sidebar with the device's
name; prompts typed there run on the owning device, and the reply streams back. One device can be
marked as the **cloud**. The others copy their projects (git snapshots, uncommitted work included)
and chats to it every 5 minutes and after each reply. While a device is off, its chats show from the
cloud, and typing in one continues it locally. All of it is **off by default**: a single machine
without Tailscale must behave exactly as before. That is a hard requirement from the user.

## State of the branch

- Every commit builds and passes `cargo test` on GitHub CI (`.github/workflows/ci.yml` runs on
  `feature/**`; failing test names show up as check annotations).
- **None of it has run on real devices yet.** The previous agent's sandbox could not reach the
  tailnet. Expect bugs in the networking and UI parts; the unit-tested parts (protocol, snapshots,
  cloud store, state) are likely solid.
- Possibly landing soon from the previous agent (run `git pull` before starting):
  - attachments in remote chats (`ClientMsg::Prompt.files`);
  - leaving secret files (`.env`, keys) out of the cloud copy by default (`SyncSettings::copy_secrets`).

## Machines

| Machine | Role in the test | How it runs archonUI |
|---|---|---|
| minipc | a host with no screen | `archonui --serve` |
| ashpc | the desktop | the app window |
| vps | the cloud | `archonui --serve` |

Find each device's Tailscale name and IP with `tailscale status` and `tailscale ip -4`. The names
below (`minipc`, `ashpc`, `vps`) stand for those MagicDNS names.

## Build (each machine)

```sh
git clone -b feature/session-sync https://github.com/Abdullahalmutairi97/archonui ~/archonui-sync
# or, in an existing clone: git fetch origin && git checkout feature/session-sync && git pull
cd ~/archonui-sync && cargo build --release
```

Needs a C compiler, `pkg-config` and `git`. The desktop also needs the GPUI libs listed in the
README (CI installs: `libxkbcommon-x11-dev libxkbcommon-dev libwayland-dev libxcb1-dev
libx11-xcb-dev libfontconfig-dev libvulkan-dev libasound2-dev libssl-dev`). Headless builds of the
same binary work on the VPS; `--serve` never opens a window.

At least one agent CLI (`claude`, `codex`, `gemini`, `opencode`, `pi`) must be installed and signed
in on any machine that runs chats (minipc, ashpc). Check with `./target/release/archonui --detect`.

## Test plan

Stop at the first failure, fix it, push, and go on. Keep notes of what you saw.

**1. Headless host and client (minipc ↔ ashpc).**
```sh
# minipc
./target/release/archonui --serve --bind "$(tailscale ip -4):7420"
# ashpc (over SSH is fine)
./target/release/archonui --remote minipc list
./target/release/archonui --remote minipc new claude ~ "Reply with one short line"
./target/release/archonui --remote minipc list          # the new chat is listed
./target/release/archonui --remote minipc ask <chat-id> "And another line"
```
Expect the reply to stream into the ashpc terminal, and the chat to be saved on the minipc (its
sessions folder, `~/.local/share/archonui/sessions/`).

**2. Refusals.** From a machine that isn't on the tailnet, or with `--bind 127.0.0.1:7420` versus a
tailnet address, check that only loopback and tailnet addresses connect.

**3. Desktop app (ashpc).** Run the app from the branch. Settings → Devices: turn sharing on (the
device list should fill in from Tailscale), tick the minipc. Within seconds the minipc's chats
appear in the sidebar, labelled "minipc". Open one, type a prompt: the reply streams; Esc stops it
on the minipc. Turn sharing off: the minipc's chats disappear and nothing listens.

**4. The window as host.** With sharing on in the ashpc app, from the minipc:
`archonui --remote ashpc list` should list the app's own chats; `ask` into one should run in the
app, and the app window shows the turn live.

**5. Approvals.** In a remote chat, use a mode or prompt where Claude asks permission for a tool. The
approval card should show on ashpc; Allow runs the tool on the minipc.

**6. Cloud.** On the vps: `archonui --serve --bind "$(tailscale ip -4):7420"`. In the ashpc app,
include the vps and mark it as the cloud. Add a project with uncommitted changes. After a reply (or
5 minutes), Settings → Devices → Cloud copy lists it. On the vps, check
`~/.local/share/archonui/cloud/projects/<id>.git` has `refs/archon/wip/<device>`. Then confirm on
ashpc that `git status` in that project is unchanged: snapshots must never touch the user's index,
HEAD or files.

**7. Away device.** Stop the ashpc app. On the minipc (or a laptop running the app), the ashpc chats
should come from the cloud; typing in one continues it locally with the history carried over.
"Open copy" in Cloud copy restores a project into a new folder.

**8. Off by default.** Fresh config dir (`ARCHONUI_CONFIG_DIR=$(mktemp -d)`): the app starts, nothing
listens on 7420 (`ss -ltn`), and `tailscale` is never run.

## Where things are

- `src/remote/`: `proto.rs` (messages), `hub.rs` (sockets, peer checks), `host.rs` (chat handling +
  headless loop + approvals), `client.rs` (`--remote`), `tailscale.rs`, `snapshot.rs`, `cloud.rs`.
- `src/ui/remote.rs`: the window's side (hosting, remote chats, backups, away devices, approvals).
- `src/ui/settings/devices.rs`: the Devices page.
- `src/app/state.rs`: remote chats in `AppState` (`put_remote`, `remote_*`, `adopt_remote`).

## Known gaps and risks to watch

- Running `--serve` and the app on the **same** machine: both use port 7420 and the same chat files.
  The app will fail to listen (it shows a toast). Don't do it; or give one a different `--bind` port.
- New chats on another device can only be started with `--remote … new` for now; there's no UI
  button yet. That was the next task: a "New chat on <device>" action in the sidebar. On the host,
  `ClientMsg::NewSession` with an empty `cwd` should fall back to the host's default folder.
- Not built: the other device's files, git and terminal in the side panel.
- The Devices page sends `ListCloud` on each render until the cloud answers. Fine while connected;
  worth tidying.
- A cloud copy that fails as an incremental bundle falls back to a full one on the next round
  (`pushed` is cleared on a `git …` error from the cloud).

## Working conventions

- Commit to `feature/session-sync`; don't push to `main`.
- Match the repo's style: short, plain doc comments; no `unwrap()` outside tests.
- Run `cargo test` before pushing; CI runs it too.
- Keep everything optional and off by default.

## Report back

Tell the user which steps passed, what you fixed (commit list), and anything still broken, with the
exact error text.
