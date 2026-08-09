#!/bin/bash
# One batched visual pass: launch, capture every state on every backdrop, quit.
#
# Written this way because the machine this runs on is somebody's working
# desktop, not a test rig. The bar is a `.statusBar` panel, so it covers every
# window the user owns; the rules that follow from that are:
#
#   * the app is launched, used, and quit inside this script — it never stays
#     resident, and it never appears unless this script asks it to;
#   * no global hotkey (`--hotkey` is not passed), so nothing is stolen from
#     whatever the user is typing into;
#   * nothing makes a sound — the levels come from the real microphone picking
#     up the real room, and `screencapture -x` is the silent form;
#   * every state is captured in a single run rather than across many launches,
#     so the interruption is one short block instead of a drip.
set -euo pipefail

APP=${APP:-$HOME/Applications/FunASRBar.app}
OUT=${OUT:-/tmp/barshot/batch}
CROP=${CROP:-"744,972,560,130"}

mkdir -p "$OUT"; rm -f "$OUT"/*.png
cmd(){ echo "$1" > ~/.funasr-bar-cmd; sleep "${2:-0.35}"; }
shot(){ screencapture -x -R"$CROP" -t png "$OUT/$1.png"; }   # -x = no shutter sound

pkill -f FunASRBar 2>/dev/null || true; sleep 0.5
open -a "$APP" --args --mic          # hidden on launch, no hotkey, no menu bar
sleep 2.0
cmd "show" 0.5

for AP in dark light; do
  cmd "appearance $AP"
  for BD in light terminal color; do
    cmd "backdrop $BD" 0.45
    cmd "idle" 0.5;   shot "idle_${BD}_${AP}"
    cmd "record" 0.7; shot "rec_${BD}_${AP}"
    cmd "stop" 0.1
    cmd "text" 0.6;   shot "text_${BD}_${AP}"
  done
done

# Morph filmstrip. The spring is deterministic, so sampling one fixed delay per
# run and varying the delay reconstructs the transition frame by frame — a
# single screencapture is far too slow to film a 0.38s spring live.
cmd "appearance dark"; cmd "backdrop terminal" 0.4
for D in 0.00 0.06 0.12 0.18 0.26 0.40; do
  cmd "idle" 0.6
  echo "record" > ~/.funasr-bar-cmd
  sleep "$D"
  shot "morph_t${D}"
  cmd "stop" 0.1
done

cmd "idle"; cmd "backdrop none" 0.3
cmd "hide" 0.3
cmd "quit" 0.5
pkill -f FunASRBar 2>/dev/null || true

echo "captured $(ls "$OUT"/*.png | wc -l) frames into $OUT"
