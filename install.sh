#!/bin/sh
# ArchonUI installer: clones (or updates) the repo, builds a release binary and
# installs the `archonui` command plus a launcher entry for the current user.
set -eu

repo_url="${ARCHONUI_REPO:-https://github.com/Abdullahalmutairi97/archonui.git}"
branch="${ARCHONUI_BRANCH:-main}"
src="${ARCHONUI_SRC:-${XDG_DATA_HOME:-$HOME/.local/share}/archonui-src}"
bin_dir="${ARCHONUI_BIN_DIR:-$HOME/.local/bin}"

say() { echo "install: $*"; }
die() { echo "install: $*" >&2; exit 1; }

[ "$(uname -s)" = Linux ] || die "ArchonUI supports Linux only"

# The distro's package names for what the build needs, to print one command to copy.
distro=""
if [ -r /etc/os-release ]; then
  # shellcheck disable=SC1091
  distro="$(. /etc/os-release && echo "${ID:-} ${ID_LIKE:-}")"
fi
# The command that installs what the build needs, without `sudo`, or "" for an unknown distro.
packages_cmd() {
  case " $distro " in
    *" arch "*|*" cachyos "*|*" manjaro "*|*" endeavouros "*)
      echo "pacman -S --needed --noconfirm base-devel git pkgconf curl libxkbcommon-x11 libxcb freetype2 fontconfig wayland vulkan-icd-loader" ;;
    *" debian "*|*" ubuntu "*|*" linuxmint "*|*" pop "*)
      echo "apt-get install -y build-essential git pkg-config curl libxkbcommon-x11-dev libxkbcommon-dev libxcb1-dev libx11-xcb-dev libfreetype-dev libfontconfig-dev libwayland-dev libvulkan1 mesa-vulkan-drivers" ;;
    *" fedora "*|*" rhel "*|*" centos "*)
      echo "dnf install -y gcc git pkgconf-pkg-config curl libxkbcommon-x11-devel libxkbcommon-devel libxcb-devel freetype-devel fontconfig-devel wayland-devel vulkan-loader mesa-vulkan-drivers" ;;
    *" opensuse"*|*" suse "*)
      echo "zypper --non-interactive install gcc git pkg-config curl libxkbcommon-x11-devel libxkbcommon-devel libxcb-devel freetype2-devel fontconfig-devel wayland-devel libvulkan1" ;;
  esac
}
packages_hint() {
  cmd="$(packages_cmd)"
  if [ -n "$cmd" ]; then
    echo "sudo $cmd"
  else
    echo "a C compiler, git, pkg-config, curl, and the development files for xkbcommon(-x11), xcb, freetype2 and fontconfig"
  fi
}

# Asks a yes/no question on the terminal, also when this script is piped into sh (stdin is the
# script then, so the answer is read from /dev/tty). Yes is the default. With no terminal, says no.
ask() {
  ( true </dev/tty ) 2>/dev/null || return 1
  printf 'install: %s [Y/n] ' "$1" >/dev/tty
  read -r answer </dev/tty || return 1
  case "$answer" in [nN]*) return 1 ;; *) return 0 ;; esac
}

find_missing() {
  missing=""
  for tool in git cc pkg-config; do
    command -v "$tool" >/dev/null 2>&1 || missing="$missing $tool"
  done
  if [ -z "$missing" ]; then
    for lib in xkbcommon xkbcommon-x11 xcb freetype2 fontconfig; do
      pkg-config --exists "$lib" 2>/dev/null || missing="$missing $lib"
    done
  fi
}

find_missing
if [ -n "$missing" ]; then
  echo "install: missing:$missing" >&2
  cmd="$(packages_cmd)"
  if [ -n "$cmd" ] && command -v sudo >/dev/null 2>&1 && ask "Install them now with: sudo $cmd ?"; then
    # sudo asks for the password itself; this script never sees it.
    # shellcheck disable=SC2086
    sudo $cmd </dev/tty || die "installing the packages failed"
    find_missing
  fi
fi
if [ -n "$missing" ]; then
  echo "install: still missing:$missing" >&2
  echo "install: install them with:" >&2
  echo "  $(packages_hint)" >&2
  exit 1
fi

if ! command -v cargo >/dev/null 2>&1; then
  if [ ! -x "$HOME/.cargo/bin/cargo" ]; then
    command -v curl >/dev/null 2>&1 || die "Rust is missing and curl is needed to install it: $(packages_hint)"
    say "cargo not found, installing Rust via rustup"
    curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal
  fi
  PATH="$HOME/.cargo/bin:$PATH"
fi

if [ -d "$src/.git" ]; then
  git -C "$src" fetch --quiet origin "$branch"
  git -C "$src" reset --hard --quiet FETCH_HEAD
else
  git clone --depth 1 --branch "$branch" "$repo_url" "$src"
fi

say "building (the first build takes a few minutes)"
cargo build --release --manifest-path "$src/Cargo.toml"

target="${CARGO_TARGET_DIR:-$src/target}"
mkdir -p "$bin_dir"
install -m755 "$target/release/archonui" "$bin_dir/archonui"
"$src/scripts/install-desktop.sh" "$bin_dir/archonui"

case ":$PATH:" in
  *":$bin_dir:"*) ;;
  *) say "$bin_dir is not on your PATH: start ArchonUI from your app launcher, or add it to PATH to run \`archonui\` in a terminal" ;;
esac

# Agents: ArchonUI drives the CLIs you already have. Say which it found, and offer one if none.
found="$("$bin_dir/archonui" --detect 2>/dev/null || true)"
if [ -z "$found" ] && ( true </dev/tty ) 2>/dev/null; then
  say "no agent CLI found yet. Install one now? (all install for your user only)"
  echo "  1) OpenCode     works without an account, on free models"
  echo "  2) Claude Code"
  echo "  3) Codex        needs npm"
  echo "  4) Gemini CLI   needs npm"
  printf 'install: pick 1-4, or Enter to skip: ' >/dev/tty
  read -r pick </dev/tty || pick=""
  case "$pick" in
    1) curl -fsSL https://opencode.ai/install | bash || say "installing OpenCode failed" ;;
    2) curl -fsSL https://claude.ai/install.sh | bash || say "installing Claude Code failed" ;;
    3|4)
      package="@openai/codex"
      [ "$pick" = 4 ] && package="@google/gemini-cli"
      if command -v npm >/dev/null 2>&1; then
        npm install -g --prefix "$HOME/.local" "$package" || say "installing $package failed"
      else
        say "npm is missing: install Node.js and npm first, or use the Install button in Settings → Harnesses"
      fi ;;
  esac
  found="$("$bin_dir/archonui" --detect 2>/dev/null || true)"
fi
if [ -n "$found" ]; then
  say "agents found:"
  echo "$found" | sed 's/^/  /'
  say "sign in from Settings → Harnesses in the app"
else
  say "no agent CLI yet. Install one from Settings → Harnesses in the app, or for example:"
  echo "  Claude Code: curl -fsSL https://claude.ai/install.sh | bash"
  echo "  OpenCode:    curl -fsSL https://opencode.ai/install | bash   (works without an account, on free models)"
  echo "  Codex:       npm install -g @openai/codex"
  echo "  Gemini:      npm install -g @google/gemini-cli"
fi
say "done. Open ArchonUI from your app launcher, or run: archonui"
