#!/usr/bin/env bash
# Installs the ArchonUI launcher icon and desktop entry for the current user.
# Usage: scripts/install-desktop.sh [path-to-archonui-binary]
# Works from a source checkout and from an unpacked release tarball. Without an argument it uses
# the tarball's `archonui`, else target/release/archonui, else `archonui` on PATH.
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

if [ "$#" -gt 1 ]; then
  echo "usage: $0 [path-to-archonui-binary]" >&2
  exit 2
elif [ "$#" -eq 1 ]; then
  bin="$1"
elif [ -x "$repo/archonui" ]; then
  bin="$repo/archonui"
elif [ -x "$repo/target/release/archonui" ]; then
  bin="$repo/target/release/archonui"
else
  bin="$(command -v archonui || true)"
fi
bin="$(readlink -f "${bin:-archonui}" 2>/dev/null || true)"
if [ -z "$bin" ] || [ ! -x "$bin" ] || [ -d "$bin" ]; then
  echo "install-desktop: no archonui binary found; pass its path: $0 <path-to-archonui>" >&2
  exit 1
fi

icons_src="$repo/assets/icons/hicolor"
if [ ! -d "$icons_src" ] || [ ! -f "$repo/assets/archonui.desktop" ]; then
  echo "install-desktop: icons or archonui.desktop missing under $repo/assets" >&2
  exit 1
fi
data_home="${XDG_DATA_HOME:-$HOME/.local/share}"
icons_dir="$data_home/icons/hicolor"
apps_dir="$data_home/applications"

for size_dir in "$icons_src"/*/apps; do
  size="$(basename "$(dirname "$size_dir")")"
  install -Dm644 "$size_dir/archonui.png" "$icons_dir/$size/apps/archonui.png"
done

install -d "$apps_dir"
sed "s|^Exec=.*|Exec=\"$bin\" %F|" "$repo/assets/archonui.desktop" >"$apps_dir/archonui.desktop"

# Refresh the icon cache (and the dir's mtime) so launchers pick up the new icon without a re-login.
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
  gtk-update-icon-cache -f -t -q "$icons_dir" || true
elif command -v gtk4-update-icon-cache >/dev/null 2>&1; then
  gtk4-update-icon-cache -f -t -q "$icons_dir" || true
fi
touch "$icons_dir" 2>/dev/null || true

if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database -q "$apps_dir" || true
fi

if command -v kbuildsycoca6 >/dev/null 2>&1; then
  kbuildsycoca6 >/dev/null 2>&1 || true
elif command -v kbuildsycoca5 >/dev/null 2>&1; then
  kbuildsycoca5 >/dev/null 2>&1 || true
fi

echo "install-desktop: $apps_dir/archonui.desktop -> $bin"
