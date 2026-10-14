#!/bin/bash
# usage: shot.sh <name>   -- capture current screen with mouse wiggle
export DISPLAY=:120
xdotool mousemove 1000 995; sleep 0.15; xdotool mousemove 1000 990; sleep 0.2
xdotool mousemove 400 620; sleep 0.2; xdotool mousemove 405 625; sleep 0.6
import -window root /home/archonminipc/ArchonUI/.qa/review/ours-$1.png
