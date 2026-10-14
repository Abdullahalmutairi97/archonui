#!/bin/bash
# Full settings capture on :120. usage: capture.sh
export DISPLAY=:120
R=/home/archonminipc/ArchonUI/.qa/review
shot() {
  xdotool mousemove 1000 995; sleep 0.15; xdotool mousemove 1000 990; sleep 0.2
  xdotool mousemove 400 620; sleep 0.2; xdotool mousemove 405 625; sleep 0.6
  import -window root $R/ours-$1.png
}
nav() { xdotool mousemove 260 $1; sleep 0.2; xdotool click 1; sleep 0.9; }
wheels() { xdotool mousemove 800 600; sleep 0.2; for i in $(seq 1 $1); do xdotool click 5; sleep 0.04; done; sleep 0.7; }

# settings modal
xdotool mousemove 600 500; sleep 0.3; xdotool key ctrl+comma; sleep 1.2
nav 182; shot settings-general
wheels 20; shot settings-general-bottom
nav 224; shot settings-appearance
wheels 8;  shot settings-appearance-presets-bottom
wheels 6;  shot settings-appearance-background
wheels 4;  shot settings-appearance-colors
wheels 12; shot settings-appearance-typography
wheels 30; shot settings-appearance-bottom
nav 265; shot settings-keyboard
wheels 20; shot settings-keyboard-bottom
nav 348; shot settings-harnesses
wheels 8;  shot settings-harnesses-bottom
nav 390; shot settings-models
wheels 20; shot settings-models-bottom
nav 473; shot settings-tools
wheels 20; shot settings-tools-bottom
nav 514; shot settings-skills
wheels 14; shot settings-skills-bottom
nav 555; shot settings-extensions
wheels 20; shot settings-extensions-bottom
xdotool key Escape; sleep 0.8
xdotool mousemove 265 968; sleep 0.4; xdotool click 1; sleep 1.4
shot share
xdotool key Escape; sleep 0.8
xdotool mousemove 800 500; sleep 0.2; xdotool key ctrl+k; sleep 1.4
shot switcher
xdotool key Escape; sleep 0.5
echo "all captured"
