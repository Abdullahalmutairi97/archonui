#!/bin/bash
# usage: cap.sh <name> <nav_y> [wheels]
export DISPLAY=:120
name=$1; navy=$2; wheels=${3:-0}
shot() {
  xdotool mousemove 1000 995; sleep 0.15; xdotool mousemove 100 $((navy)); sleep 0.25
  xdotool mousemove 400 600; sleep 0.2; xdotool mousemove 405 605; sleep 0.6
  import -window root /home/archonminipc/ArchonUI/.qa/review/ours-$1.png
}
xdotool mousemove 260 $navy; sleep 0.2; xdotool click 1; sleep 1.0
shot "$name"
if [ "$wheels" -gt 0 ]; then
  xdotool mousemove 800 600; sleep 0.2
  for i in $(seq 1 $wheels); do xdotool click 5; sleep 0.05; done
  sleep 0.8
  shot "$name-bottom"
fi
echo "captured $name"
