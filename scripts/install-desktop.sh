#!/usr/bin/env bash
# Installs the ArchonUI launcher icon and desktop entry for the current user.
# Usage: scripts/install-desktop.sh <path-to-archonui-binary>
set -euo pipefail

if [ "$#" -ne 1 ]; then
  echo "usage: $0 <path-to-archonui-binary>" >&2
  exit 2
fi

bin="$(readlink -f "$1")"
if [ ! -x "$bin" ]; then
  echo "install-desktop: not an executable: $bin" >&2
  exit 1
fi

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
icons_src="$repo/assets/icons/hicolor"
data_home="${XDG_DATA_HOME:-$HOME/.local/share}"
icons_dir="$data_home/icons/hicolor"
apps_dir="$data_home/applications"

for size_dir in "$icons_src"/*/apps; do
  size="$(basename "$(dirname "$size_dir")")"
  install -Dm644 "$size_dir/archonui.png" "$icons_dir/$size/apps/archonui.png"
done

install -d "$apps_dir"
sed "s|^Exec=.*|Exec=$bin %F|" "$repo/assets/archonui.desktop" >"$apps_dir/archonui.desktop"

if command -v gtk-update-icon-cache >/dev/null 2>&1; then
  gtk-update-icon-cache -f -t "$icons_dir"
else
  echo "install-desktop: gtk-update-icon-cache not found, skipping icon cache update"
fi

if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database "$apps_dir"
else
  echo "install-desktop: update-desktop-database not found, skipping desktop database update"
fi

if command -v kbuildsycoca6 >/dev/null 2>&1; then
  kbuildsycoca6
elif command -v kbuildsycoca5 >/dev/null 2>&1; then
  kbuildsycoca5
fi

echo "install-desktop: $apps_dir/archonui.desktop -> $bin"
