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
packages_hint() {
  case " $distro " in
    *" arch "*|*" cachyos "*|*" manjaro "*|*" endeavouros "*)
      echo "sudo pacman -S --needed base-devel git pkgconf curl libxkbcommon-x11 libxcb freetype2 fontconfig wayland vulkan-icd-loader" ;;
    *" debian "*|*" ubuntu "*|*" linuxmint "*|*" pop "*)
      echo "sudo apt install build-essential git pkg-config curl libxkbcommon-x11-dev libxkbcommon-dev libxcb1-dev libx11-xcb-dev libfreetype-dev libfontconfig-dev libwayland-dev libvulkan1 mesa-vulkan-drivers" ;;
    *" fedora "*|*" rhel "*|*" centos "*)
      echo "sudo dnf install gcc git pkgconf-pkg-config curl libxkbcommon-x11-devel libxkbcommon-devel libxcb-devel freetype-devel fontconfig-devel wayland-devel vulkan-loader mesa-vulkan-drivers" ;;
    *" opensuse"*|*" suse "*)
      echo "sudo zypper install gcc git pkg-config curl libxkbcommon-x11-devel libxkbcommon-devel libxcb-devel freetype2-devel fontconfig-devel wayland-devel libvulkan1" ;;
    *) echo "a C compiler, git, pkg-config, curl, and the development files for xkbcommon(-x11), xcb, freetype2 and fontconfig" ;;
  esac
}

missing=""
for tool in git cc pkg-config; do
  command -v "$tool" >/dev/null 2>&1 || missing="$missing $tool"
done
if [ -z "$missing" ]; then
  for lib in xkbcommon xkbcommon-x11 xcb freetype2 fontconfig; do
    pkg-config --exists "$lib" 2>/dev/null || missing="$missing $lib"
  done
fi
if [ -n "$missing" ]; then
  echo "install: missing:$missing" >&2
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

# Agents: ArchonUI drives the CLIs you already have. Say which it found, and what to do if none.
found="$("$bin_dir/archonui" --detect 2>/dev/null || true)"
if [ -n "$found" ]; then
  say "agents found:"
  echo "$found" | sed 's/^/  /'
else
  say "no agent CLI found yet. Install at least one, for example:"
  echo "  Claude Code: curl -fsSL https://claude.ai/install.sh | bash"
  echo "  OpenCode:    curl -fsSL https://opencode.ai/install | bash   (works without an account, on free models)"
  echo "  Codex:       npm install -g @openai/codex"
  echo "  Gemini:      npm install -g @google/gemini-cli"
fi
say "done. Open ArchonUI from your app launcher, or run: archonui"
