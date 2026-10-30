# ArchonUI

A desktop app for running coding-agent CLIs side by side in one tiled workspace, with a built-in editor,
file tree, git panel, terminal and browser.

> **Beta.** It works day to day for the author, but expect rough edges and breaking changes. Bug reports are welcome.

**Supported agents:** Claude Code, Codex CLI, Gemini CLI, OpenCode, Pi.

ArchonUI never talks to a model itself. It finds the CLIs you already have installed, runs them, and gives
them one shared interface.

## What you get

**Workspace**
- Up to six chat sessions on screen at once, tiled and resizable. The layout is saved.
- Sessions grouped by project, with search (Ctrl+K), drafts kept, and sessions restored on launch.
- Switch agents in the middle of a chat; the conversation carries over.

**Chat**
- Markdown, code blocks, tool-call rows, charts and images. A compact task plan, a final-answer divider and a
  usage footer (time, tokens, cost, lines added and removed).
- Auto, Plan and Ask modes. Claude Code can ask for approval inside the app before using a tool.
- Send messages while the agent is working (they queue, or steer it right away). Esc stops a reply.
- Attach files by drag and drop, paste or the file picker. A context meter shows how full the agent is.

**Side panel**
- **Browser** for local sites and URLs, **IDE** (built-in editor or VS Code), **Files**, **Git**
  (stage, commit, push, pull, diff) and a real **Terminal**.

**Tools, skills and settings**
- One place for MCP servers, skills, extensions, models, tool permissions and API keys.
- Sync skills to every agent so they all load the same set.
- Desktop notification when a reply finishes.

**Looks**
- 13 built-in themes, your own theme files, wallpapers, background filters, window opacity, fonts and
  custom colours. Arabic and other right-to-left text display correctly.

## Install (Linux)

```sh
curl -fsSL https://raw.githubusercontent.com/Abdullahalmutairi97/archonui/main/install.sh | sh
```

This builds ArchonUI and adds an `archonui` command and a launcher icon for your user only. Nothing is
installed system-wide. The first build takes a few minutes.

To update, open Settings → General → Updates: it shows how many commits are new on GitHub and
updates with one click (it pulls the checkout install.sh made, builds it and swaps the binary, then
offers Restart now). A checkout with local changes is left alone, and a failed build keeps the old
version. A small dot on Settings means an update is available (checked at most once a day; turn it off
there). From a terminal: `archonui --check-update`, `archonui --update`, or run the install command again.

**You need:**
- A C compiler, `git`, `pkg-config` and the usual desktop libraries. The installer checks and, if
  something is missing, offers to install it (`sudo` asks for your password), or prints the one command
  for your distro. For reference:
  - Arch: `sudo pacman -S --needed base-devel git pkgconf curl libxkbcommon-x11 libxcb freetype2 fontconfig wayland vulkan-icd-loader`
  - Debian/Ubuntu: `sudo apt install build-essential git pkg-config curl libxkbcommon-x11-dev libxkbcommon-dev libxcb1-dev libx11-xcb-dev libfreetype-dev libfontconfig-dev libwayland-dev libvulkan1 mesa-vulkan-drivers`
  - Fedora: `sudo dnf install gcc git pkgconf-pkg-config curl libxkbcommon-x11-devel libxkbcommon-devel libxcb-devel freetype-devel fontconfig-devel wayland-devel vulkan-loader mesa-vulkan-drivers`
- Rust (the installer sets it up with [rustup](https://rustup.rs) if missing).
- A Wayland or X11 desktop with a Vulkan driver (already there on most desktops).
- At least one agent CLI: `claude`, `codex`, `gemini`, `opencode` or `pi`. ArchonUI finds them on your
  `PATH` and in the usual install folders (npm, nvm, fnm, volta, bun, pnpm, `~/.local/bin`, …), also
  when opened from the app launcher. OpenCode works without an account, on its free models. If none is
  found, the installer offers to install one. In the app, Settings → Harnesses has **Install**, **Update**
  and **Sign in** for each agent: they install for your user only, and only a step that needs root (Node
  itself, say) shows your desktop's password prompt.

**Other ways to install**
- **Prebuilt:** download the `.tar.gz` from [Releases](https://github.com/Abdullahalmutairi97/archonui/releases),
  unpack it and run `./archonui`. Run `./scripts/install-desktop.sh` in the unpacked folder for the launcher icon.
- **Arch Linux:** `cd packaging && makepkg -si`
- **Manual:**
  ```sh
  git clone https://github.com/Abdullahalmutairi97/archonui.git && cd archonui
  cargo build --release
  ./scripts/install-desktop.sh target/release/archonui
  ln -s "$PWD/target/release/archonui" ~/.local/bin/archonui
  ```

## Use

```sh
archonui                  # open in the current folder
archonui path/to/file.rs  # open with a file attached
archonui --detect         # list the agent CLIs it found
archonui --ask AGENT "prompt"   # one-off turn from the terminal (no window)
```

Or open "ArchonUI" from your app launcher.

**Shortcuts:** Alt+Enter split, Alt+Q close tile, Alt+←/→ move focus, Alt+T next theme, Ctrl+K find a
session, Shift+Tab switch mode, Esc stop. All rebindable in Settings.

**Your files** live in `~/.config/archonui` (settings, projects, `themes/`, `backgrounds/`).

## Develop

```sh
cargo run     # debug build
cargo test    # unit tests, plus a test that drives the binary with a fake agent
```

More detail: [changelog](docs/CHANGELOG.md), [how modes map to each CLI](docs/HARNESS_MAPPING.md),
[security review](docs/SECURITY_REVIEW.md).

## License

[MIT](LICENSE)
