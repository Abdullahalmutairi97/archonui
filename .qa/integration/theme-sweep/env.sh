#!/bin/bash
# Shared helpers for the :116 theme sweep. Never pkill -f; PID found via /proc environ.
N=116
export DISPLAY=:$N
BASE=/tmp/archonui-qa-$N
BIN=/home/archonminipc/ArchonUI/target/debug/archonui
QA=/home/archonminipc/ArchonUI/.qa

app_pid() {
  for p in $(pgrep -f "archonui" 2>/dev/null); do
    if tr '\0' '\n' < /proc/$p/environ 2>/dev/null | grep -q "^ARCHONUI_DATA_DIR=$BASE/data$"; then echo "$p"; return 0; fi
  done
  return 1
}

kill_app() {
  local p
  p=$(app_pid) || return 0
  kill "$p" 2>/dev/null
  for _ in $(seq 1 40); do sleep 0.25; app_pid >/dev/null || { sleep 0.7; return 0; }; done
  kill -9 "$(app_pid)" 2>/dev/null
  sleep 0.7
}

launch() {
  (setsid env -u WAYLAND_DISPLAY DISPLAY=:$N \
     ARCHONUI_DATA_DIR=$BASE/data ARCHONUI_CONFIG_DIR=$BASE/cfg ARCHONUI_TMUX_SOCKET=archonui-qa-$N \
     VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json \
     $BIN --new-ui >>$BASE/log 2>&1 &)
}

grab() { # grab <path> : wiggle then capture, no readiness check
  xdotool mousemove 900 500 2>/dev/null; sleep 0.3
  xdotool mousemove 906 506 2>/dev/null; sleep 0.5
  import -window root "$1"
}

# Waits for the window, then for two nearly identical non-black frames (first paint + wallpaper decode).
wait_ready() {
  local p
  for _ in $(seq 1 60); do
    sleep 0.5
    p=$(app_pid) || { echo "app exited"; return 1; }
    xdotool search --name "archonui" >/dev/null 2>&1 && break
  done
  sleep 4
  local a=$BASE/.probe-a.png b=$BASE/.probe-b.png mean ae
  for _ in $(seq 1 12); do
    grab "$a"; sleep 1.6; grab "$b"
    mean=$(convert "$b" -resize 1x1 -format '%[fx:mean]' info:)
    ae=$(compare -metric AE -fuzz 2% "$a" "$b" null: 2>&1 | awk '{print $1}')
    if awk -v m="$mean" 'BEGIN{exit !(m>0.03)}' && awk -v a="$ae" 'BEGIN{exit !(a<2000)}'; then
      sleep 1.5
      grab "$a"; sleep 1.2; grab "$b"
      ae=$(compare -metric AE -fuzz 2% "$a" "$b" null: 2>&1 | awk '{print $1}')
      awk -v a="$ae" 'BEGIN{exit !(a<2000)}' && return 0
    fi
  done
  echo "warning: frames never settled (mean=$mean ae=$ae)"
  return 1
}

shot() { grab "$1"; }
