#!/bin/bash
# One batched visual pass: launch, capture every surface in every state, quit.
#
# Written this way because the machine this runs on is somebody's working
# desktop, not a test rig:
#
#   * the app is launched, used and quit inside this script — it never stays
#     resident, and it never appears unless this script asks it to;
#   * no global hotkey (`--hotkey` is not passed), so nothing is stolen from
#     whatever the user is typing into;
#   * nothing makes a sound — the levels come from the real microphone picking up
#     the real room, and `screencapture -x` is the silent form;
#   * every state is captured in a single run rather than across many launches,
#     so the interruption is one short block instead of a drip.
#
# It does take focus, once, and only for the window shots: an ordinary window has
# to be frontmost to be photographed, and this app is `.accessory` until one
# opens. Nothing belonging to the user is moved, resized or closed.
#
# Glass is entirely a function of what is behind it, so nothing here is captured
# against a single background. The voice bar sits above every window, so its
# backdrops go above them too; the main and settings windows sample what is
# *behind* the window, so theirs go underneath.
set -euo pipefail
cd "$(dirname "$0")"

APP=${APP:-$HOME/Applications/Koushu.app}
OUT=${OUT:-/tmp/koushu-shots}
STATUS=~/.funasr-bar-status

mkdir -p "$OUT"; rm -f "$OUT"/*.png

cmd(){ echo "$1" > ~/.funasr-bar-cmd; sleep "${2:-0.35}"; }

# -x is what makes screencapture silent. Without it every frame plays a shutter
# through the speakers, which is exactly the interruption this script exists to
# avoid.
shot_region(){ screencapture -x -R"$1" -t png "$OUT/$2.png"; }

# The bar reports its own rect, so the crop follows the morph instead of being a
# constant that goes stale the first time a width changes.
rect_of(){ python3 - "$1" <<'PY'
import json, os, sys

name = sys.argv[1]
with open(os.path.expanduser("~/.funasr-bar-status")) as handle:
    status = json.load(handle)

rect = status.get("barCapture") if name == "bar" else (status.get("windows") or {}).get(name)
if not rect:
    raise SystemExit
# A margin, because the shadow and the glass rim are part of what is being
# judged and a tight crop cuts exactly those off.
print("%d,%d,%d,%d" % (rect["x"] - 24, rect["y"] - 24, rect["w"] + 48, rect["h"] + 48))
PY
}

shot_bar(){ local r; r=$(rect_of bar); [ -n "$r" ] && shot_region "$r" "$1"; }
shot_win(){ local r; r=$(rect_of "$1"); [ -n "$r" ] && shot_region "$r" "$2"; }

pkill -f Koushu 2>/dev/null || true; sleep 0.5
open -a "$APP" --args --mic          # no window, no hotkey, no menu bar item
sleep 2.0

# ---------------------------------------------------------------- voice bar --
cmd "show" 0.6
for AP in dark light; do
  cmd "appearance $AP"
  for BD in light terminal color; do
    cmd "backdrop $BD" 0.45
    cmd "idle" 0.5;   shot_bar "bar_idle_${BD}_${AP}"
    cmd "record" 0.8; shot_bar "bar_rec_${BD}_${AP}"
    cmd "stop" 0.1
    cmd "text" 0.7;   shot_bar "bar_text_${BD}_${AP}"
  done
done

# Morph filmstrip. The spring is deterministic, so sampling one fixed delay per
# run and varying the delay reconstructs the transition frame by frame — a single
# screencapture is far too slow to film a 0.38s spring live.
cmd "appearance dark"; cmd "backdrop terminal" 0.4
for D in 0.00 0.06 0.12 0.18 0.26 0.40; do
  cmd "idle" 0.6
  echo "record" > ~/.funasr-bar-cmd
  sleep "$D"
  shot_bar "morph_t${D}"
  cmd "stop" 0.1
done
cmd "idle"; cmd "hide" 0.3

# ------------------------------------------------------------------ windows --
# From here the app is frontmost. One block, then it quits.
for AP in dark light; do
  cmd "appearance $AP"
  for BD in light terminal color; do
    cmd "backdrop $BD below" 0.4

    cmd "main" 1.4
    cmd "search" 0.2                  # plain session list
    shot_win koushu.main "main_sessions_${BD}_${AP}"

    cmd "search 转写" 0.6             # results, grouped, with the terms marked
    shot_win koushu.main "main_search_${BD}_${AP}"
    cmd "search" 0.4

    cmd "filter archived" 0.6         # the scope that is otherwise invisible
    shot_win koushu.main "main_archived_${BD}_${AP}"
    cmd "filter none" 0.4

    cmd "settings" 1.2
    shot_win koushu.settings "settings_${BD}_${AP}"
  done
done

# Both locales, once each. The layout is what is being checked here, not the
# material, so it does not need the three backdrops as well: Chinese sets denser
# per character and it is the controls that overflow, not the glass.
cmd "appearance dark"; cmd "backdrop terminal below" 0.4
for LOC in zh en; do
  cmd "locale $LOC" 0.8
  cmd "main" 1.0;     shot_win koushu.main "locale_main_${LOC}"
  cmd "settings" 1.0; shot_win koushu.settings "locale_settings_${LOC}"
done

cmd "backdrop none" 0.3
cmd "quit" 0.6
pkill -f Koushu 2>/dev/null || true

echo "captured $(ls "$OUT"/*.png 2>/dev/null | wc -l | tr -d ' ') frames into $OUT"
