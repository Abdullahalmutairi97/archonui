#!/bin/sh
# ArchonUI installer: clones (or updates) the repo, builds a release binary and
# installs the `archonui` command plus a launcher entry for the current user.
set -eu

repo_url="${ARCHONUI_REPO:-https://github.com/Abdullahalmutairi97/archonui.git}"
src="${ARCHONUI_SRC:-${XDG_DATA_HOME:-$HOME/.local/share}/archonui-src}"
bin_dir="${ARCHONUI_BIN_DIR:-$HOME/.local/bin}"

[ "$(uname -s)" = Linux ] || { echo "install: ArchonUI supports Linux only" >&2; exit 1; }
command -v git >/dev/null 2>&1 || { echo "install: git is required" >&2; exit 1; }
command -v cc >/dev/null 2>&1 || { echo "install: a C compiler (cc) is required" >&2; exit 1; }
command -v pkg-config >/dev/null 2>&1 || { echo "install: pkg-config is required" >&2; exit 1; }

if ! command -v cargo >/dev/null 2>&1; then
  [ -x "$HOME/.cargo/bin/cargo" ] || {
    echo "install: cargo not found, installing Rust via rustup"
    curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal
  }
  PATH="$HOME/.cargo/bin:$PATH"
fi

if [ -d "$src/.git" ]; then
  git -C "$src" fetch --quiet origin main
  git -C "$src" reset --hard --quiet origin/main
else
  git clone --depth 1 "$repo_url" "$src"
fi

cargo build --release --manifest-path "$src/Cargo.toml"

mkdir -p "$bin_dir"
install -m755 "$src/target/release/archonui" "$bin_dir/archonui"
"$src/scripts/install-desktop.sh" "$bin_dir/archonui"

case ":$PATH:" in *":$bin_dir:"*) ;; *) echo "install: add $bin_dir to your PATH" ;; esac
echo "install: done. Run: archonui"
