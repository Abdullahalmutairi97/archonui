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
installed system-wide. The first build takes a few minutes. Run the same command again to update.

**You need:**
- A C compiler, `pkg-config` and `git` (Arch: `base-devel pkgconf git`, Debian/Ubuntu:
  `build-essential pkg-config git`, Fedora: `gcc pkgconf-pkg-config git`).
- Rust (the installer sets it up with [rustup](https://rustup.rs) if missing).
- A Wayland or X11 desktop with a Vulkan driver (already there on most desktops).
- At least one agent CLI on your `PATH`: `claude`, `codex`, `gemini`, `opencode` or `pi`.

**Other ways to install**
- **Prebuilt:** download the `.tar.gz` from [Releases](https://github.com/Abdullahalmutairi97/archonui/releases),
  unpack it and run `archonui`. Run `scripts/install-desktop.sh <binary>` for the launcher icon.
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
