#!/bin/bash
# Capture one state of OUR app on :129 at 1920x1080 (1 design px == 1 screenshot px, ui_scale=1.0).
# usage: main2.sh <name> <panel_tab> <left_tab> <tiles-json> <focus> [right_left]
#   tiles-json: e.g. '["archonui-test-s1","archonui-test-s2"]'  (empty [] -> no tiles)
set -u
export DISPLAY=:129
R=/home/archonminipc/ArchonUI/.qa/review
QA=/tmp/archonui-qa-129
CFG=$QA/cfg
APP=/home/archonminipc/ArchonUI/target/debug/archonui
name=$1; tab=${2:-ide}; ltab=${3:-sessions}; tiles=${4:-'["archonui-test-s1"]'}; focus=${5:-archonui-test-s1}
LEFT=${LEFT:-1}; RIGHT=${RIGHT:-1}
export LEFT RIGHT

# kill only our own instance (environ check, never pkill -f)
for p in $(pgrep -f 'archonui --new-ui'); do
  if tr '\0' '\n' < /proc/$p/environ 2>/dev/null | grep -q "^ARCHONUI_CONFIG_DIR=$CFG$"; then kill "$p" 2>/dev/null; fi
done
sleep 1

printf '{"ui_scale":1.0}\n' > $CFG/settings.json

python3 - "$CFG" "$tab" "$ltab" "$tiles" "$focus" "$LEFT" "$RIGHT" <<'PY'
import json, sys
cfg, tab, ltab, tiles, focus, left, right = sys.argv[1:8]
tiles = json.loads(tiles)
left, right = left == "1", right == "1"
ws = {"layout": {"tiles": tiles, "focus": (focus or None) if tiles else None, "mode": "Dwindle",
                 "ratios": {}, "master_ratio": 0.56},
      "left_open": left, "left_width": 300.0, "left_tab": ltab,
      "right_open": right, "right_tab": tab, "right_width": 480.0, "current_project": None}
json.dump(ws, open(cfg + "/workspace.json", "w"))
PY

mkdir -p $QA/xdg
(setsid env -u WAYLAND_DISPLAY DISPLAY=:129 ARCHONUI_DATA_DIR=$QA/data ARCHONUI_CONFIG_DIR=$CFG \
  ARCHONUI_TMUX_SOCKET=archonui-qa-129 XDG_RUNTIME_DIR=$QA/xdg \
  VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json $APP --new-ui >$QA/log-$name 2>&1 &)
sleep 12

pid=$(pgrep -f 'archonui --new-ui' | while read p; do
  if tr '\0' '\n' < /proc/$p/environ 2>/dev/null | grep -q "^ARCHONUI_CONFIG_DIR=$CFG$"; then echo $p; fi
done | head -1)
wid=$(xdotool search --onlyvisible --pid "$pid" 2>/dev/null | head -1)
if [ -n "$wid" ]; then
  xdotool windowsize "$wid" 1920 1080; sleep 0.7; xdotool windowmove "$wid" 0 0; sleep 1.2
fi
xdotool mousemove 900 500; sleep 0.25; xdotool mousemove 903 503; sleep 0.7
import -window root $R/main2-$name.png
echo "$name: pid=$pid wid=$wid -> $R/main2-$name.png ($(identify -format '%wx%h' $R/main2-$name.png))"
