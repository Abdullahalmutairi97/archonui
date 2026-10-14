#!/bin/bash
# Theme sweep on :116. Writes .qa/integration/theme-<id>.png for the 12 theme ids.
source /tmp/archonui-qa-116/env.sh
OUT=/home/archonminipc/ArchonUI/.qa/integration
mkdir -p "$OUT"

sweep() { # sweep <theme-id> <background> <outfile>
  kill_app
  cp $BASE/workspace-3tiles.json $BASE/cfg/workspace.json
  printf '{"theme":"%s","background":"%s"}\n' "$1" "$2" > $BASE/cfg/settings.json
  : > $BASE/log
  launch
  wait_ready
  shot "$3"
  echo "$(basename "$3") $(identify -format '%wx%h' "$3")"
}

if [ -n "$1" ]; then # single: theme bg out
  sweep "$1" "$2" "$3"
  exit 0
fi

for t in archonui blood-moon moonlight wreck citadel neon violet ink bonfire hunter shards vale; do
  sweep "$t" canvas "$OUT/theme-$t.png"
done
