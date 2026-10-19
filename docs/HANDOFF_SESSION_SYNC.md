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
- Ran on real devices on 2026-10-17 (minipc headless host, ashpc desktop, vps cloud as root), all on
  commit 85619cf or later. Step 1 passed (after fixing "~" folders and replies printed twice). Step 2
  passed: a non-tailnet source (docker 172.17.0.3) was refused, loopback accepted, a browser-style
  handshake with an Origin header gets 403. Step 4 passed from the command line (new/ask into the
  ashpc app's chats). Step 6 passed: the vps copy held modified, staged and untracked files, `.env`
  was left out, the project's git status, index and HEAD were unchanged, and the cloud folder is 0700.
  Step 8 passed: fresh config, nothing listening, a fake `tailscale` first on PATH was never called.
- 2026-10-19, driven in the ashpc app with `wtype` (Ctrl+K to open a chat): step 3 passed (a
  prompt typed on ashpc ran on the minipc and streamed back; Esc stopped it there and it shows as
  Stopped). Step 7 passed (minipc went away; typing within 60 s says it is reconnecting; after
  that the chat continued on ashpc as a new chat with the history). "Open copy" was not driven
  (needs a mouse). Step 5 can't run: no agent asks for approvals today (Claude is started without
  `--permission-prompt-tool` in every mode), so forwarding approvals is covered only by tests.
- Old and new builds (0dce615, d30ae41, current) were checked against each other on loopback; every
  message decodes both ways and the cloud round trip works. Wire JSON of every message is pinned by
  tests in src/remote/proto.rs.
- Also built: attachments in remote chats (`ClientMsg::Prompt.files`), secret files (`.env`, keys)
  left out of the cloud copy by default (`SyncSettings::copy_secrets`), and "+ New on <device>"
  buttons in the sidebar to start a chat on another device.

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
./target/release/archonui --remote minipc new claude "~" "Reply with one short line"
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

- Running `--serve` and the app on the **same** machine: both would use port 7420 and the same chat
  files. A lock on the chats folder now stops whichever starts second, with a message (the app's goes
  to stderr only). To run both, give `--serve` its own `ARCHONUI_DATA_DIR` and `--bind` port.
- "+ New on <device>" sends `NewSession` with an empty agent and folder; the host fills in its own
  defaults. Check the new chat opens in the focused tile on the asking device.
- Not built: the other device's files, git and terminal in the side panel.
- A project whose git history ever held a secret is now copied without that history. Secrets
  already in a cloud (copied by an older build, or while "Copy secret files" was on) stay there;
  turning the setting off doesn't remove them.
- Building on a machine where rustc crashes with SIGSEGV: use `RUST_MIN_STACK=67108864 cargo build
  --release` (seen on ashpc).
- Tests in `engine::skills` and `engine::discover::tests::run_text_output_is_private` fail when
  TMPDIR is under $HOME; unrelated to sync; CI passes them.

## Working conventions

- Commit to `feature/session-sync`; don't push to `main`.
- Match the repo's style: short, plain doc comments; no `unwrap()` outside tests.
- Run `cargo test` before pushing; CI runs it too.
- Keep everything optional and off by default.

## Report back

Tell the user which steps passed, what you fixed (commit list), and anything still broken, with the
exact error text.
