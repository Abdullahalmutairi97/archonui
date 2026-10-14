# ArchonUI

A desktop app for working with coding-agent CLIs — Claude Code, Codex CLI, Gemini CLI, OpenCode and
Pi — side by side in a tiled workspace, with a built-in editor, file tree, git panel, terminal and
in-app browser.

ArchonUI doesn't talk to any model itself. It detects the CLIs you already have installed, runs them as
subprocesses, and gives them a shared UI: tiled chat sessions, a real editor and terminal they can act
through, approvals for Claude Code's tool calls, and one settings surface for models, modes and keys.

## Install

```sh
curl -fsSL https://raw.githubusercontent.com/Abdullahalmutairi97/archonui/main/install.sh | sh
```

This clones the repo, builds a release binary with `cargo`, and installs an `archonui` command plus an
app-launcher icon for the current user. It's Linux-only, and installs nothing system-wide — everything
goes under `~/.local/share/archonui-src`, `~/.local/bin/archonui` and `~/.local/share/{icons,applications}`.

The first build compiles ArchonUI's dependencies (GPUI, a syntax highlighter, a terminal emulator, etc.)
from source, so it takes a few minutes. Later runs of the installer just update and rebuild.

### What you need first

- **Rust.** The installer installs one via [rustup](https://rustup.rs) if `cargo` isn't already on your
  `PATH`.
- **A C compiler, `pkg-config` and `git`.** On Arch: `sudo pacman -S base-devel pkgconf git`. On
  Debian/Ubuntu: `sudo apt install build-essential pkg-config git`. On Fedora:
  `sudo dnf install gcc pkgconf-pkg-config git`.
- **A Wayland or X11 desktop with a working Vulkan driver.** ArchonUI renders through GPUI/Vulkan; on most
  desktop installs this is already in place (Mesa on Linux ships one for Intel/AMD GPUs; NVIDIA needs its
  proprietary driver).
- **At least one agent CLI installed and on your `PATH`** (or in `~/.local/bin`, `~/.cargo/bin`,
  `~/.npm-global/bin`, `~/.bun/bin`): `claude`, `codex`, `gemini`, `opencode` or `pi`.
  ArchonUI detects whichever of these it finds; it has nothing to show with none installed.

### Manual install

If you'd rather not pipe a script into `sh`:

```sh
git clone https://github.com/Abdullahalmutairi97/archonui.git
cd archonui
cargo build --release
./scripts/install-desktop.sh target/release/archonui   # icon + .desktop entry
ln -s "$PWD/target/release/archonui" ~/.local/bin/archonui
```

### Updating

Re-run the install command — it pulls `main` and rebuilds in place.

## Running

```sh
archonui                 # open in the current folder
archonui path/to/file.rs # open with a file already attached
```

Or launch "ArchonUI" from your app launcher.

See [docs/CHANGELOG.md](docs/CHANGELOG.md) for what the UI can do.

## Development

```sh
cargo run              # debug build, opt-level 2 for dependencies (see Cargo.toml)
cargo test              # unit + integration tests
cargo run -- --detect   # headless: list detected agent CLIs
```

`cargo run -- --help`-style flags live in `src/main.rs` (`--ask`, `--detect`, `--discover`, `--test`,
and a few `--*-test` flags used by the test suite).

## License

Not yet decided — treat this as "all rights reserved" until a LICENSE file is added.
