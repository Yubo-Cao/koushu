#!/usr/bin/env python3
"""Draw the tray icons.

Three states, because a tray icon whose only job is to sit there is a waste of
the one pixel budget the user actually looks at:

    idle          outlined microphone   "running, not listening"
    recording     filled microphone     "your voice is being captured"
    transcribing  waveform bars         "audio is being turned into text"

They differ by *shape*, not only by colour, so the macOS template variant --
which throws colour away and keeps only alpha -- still reads as three distinct
states.

Size: 64x64. Both platforms downscale from here and never upscale.
  * Linux  tray-icon writes the RGBA buffer to a PNG and hands the path to
           libappindicator; Plasma renders it at ~22px logical, so on a 1.25x
           or 2x output it needs 28-44 real pixels. A 22px source would be
           upscaled and mushy.
  * macOS  tray-icon pins the NSImage to 18pt tall, so a Retina menu bar wants
           36 real pixels and gets them from the 64px bitmap.

Everything is drawn at 8x and box-filtered down, which is cheaper to reason
about than fighting PIL's aliasing, and gives clean curves at 22px.

Output MUST be 8-bit PNG. A 16-bit PNG panics Tauri's compile-time image
decoder with "expected pixel count 1024, got 2048" -- this project has been
bitten by exactly that before. The check at the bottom is not decoration.

Run:  python3 src-tauri/icons/tray/generate.py
Then: touch src-tauri/build.rs   (build.rs only watches tauri.conf.json, so
      changed icon bytes are otherwise not re-embedded)
"""

import pathlib
import struct

from PIL import Image, ImageDraw

SIZE = 64
SS = 8  # supersample factor
S = SIZE * SS

OUT = pathlib.Path(__file__).parent

# Terracotta from the app icon, lifted a little so it survives both a near
# black Plasma panel and a light one. The active states are deliberately not
# on-brand: red and amber are read as status before they are read as identity.
IDLE = (168, 85, 58, 255)
RECORDING = (214, 69, 65, 255)
TRANSCRIBING = (200, 134, 42, 255)
BLACK = (0, 0, 0, 255)


def canvas():
    return Image.new("RGBA", (S, S), (0, 0, 0, 0))


def u(v):
    """64-unit design space -> supersampled pixels."""
    return v * SS


def rounded_rect(d, box, radius, colour, width=0):
    x0, y0, x1, y1 = (u(v) for v in box)
    d.rounded_rectangle([x0, y0, x1, y1], radius=u(radius), fill=colour if width == 0 else None,
                        outline=None if width == 0 else colour, width=int(u(width)))


def microphone(colour, filled):
    """A microphone: capsule, cradle arc, stem, foot.

    Proportions follow the conventional mic glyph -- the cradle is a half
    circle wider than the capsule, and its flat ends sit at the capsule's
    waist, so the capsule reads as sitting *inside* the cradle. Tucking the arc
    under the capsule instead makes the pair read as a single lollipop.
    """
    img = canvas()
    d = ImageDraw.Draw(img)
    stroke = 5.0

    # Capsule. Filled reads as "hot", outlined as "standby" -- a difference in
    # shape, which survives being shrunk to 22px and stripped of colour by
    # macOS template rendering. Colour alone would survive neither.
    rounded_rect(d, (22, 7, 42, 37), 10, colour, width=0 if filled else stroke)

    # Cradle: bottom half of a circle centred on the capsule's waist, and wider
    # than the capsule so its arms show on both sides.
    d.arc([u(13), u(11), u(51), u(49)], start=0, end=180, fill=colour, width=int(u(stroke)))

    # Stem and foot.
    d.line([u(32), u(47), u(32), u(56)], fill=colour, width=int(u(stroke)))
    d.line([u(22), u(56), u(42), u(56)], fill=colour, width=int(u(stroke)))
    return img


def waveform(colour):
    """Five bars, tallest in the middle: audio being chewed on."""
    img = canvas()
    d = ImageDraw.Draw(img)
    bar_w = 7.5
    centres = [9.5, 20.75, 32.0, 43.25, 54.5]
    heights = [18.0, 34.0, 48.0, 30.0, 16.0]
    for cx, h in zip(centres, heights):
        rounded_rect(
            d,
            (cx - bar_w / 2, 32 - h / 2, cx + bar_w / 2, 32 + h / 2),
            bar_w / 2,
            colour,
        )
    return img


def save(img, name):
    small = img.resize((SIZE, SIZE), Image.LANCZOS)
    path = OUT / name
    # No optimize=True: it can pick a palette/greyscale encoding, and the
    # decoder on the other side wants straight 8-bit RGBA.
    small.save(path, format="PNG")

    raw = path.read_bytes()
    width, height = struct.unpack(">II", raw[16:24])
    depth, colour_type = raw[24], raw[25]
    assert (width, height) == (SIZE, SIZE), f"{name}: {width}x{height}"
    assert depth == 8, f"{name}: bit depth {depth}, must be 8"
    assert colour_type == 6, f"{name}: colour type {colour_type}, must be 6 (RGBA)"
    print(f"{name}: {width}x{height} depth={depth} rgba ok")


def main():
    save(microphone(IDLE, filled=False), "idle.png")
    save(microphone(BLACK, filled=False), "idle-template.png")
    save(microphone(RECORDING, filled=True), "recording.png")
    save(waveform(TRANSCRIBING), "transcribing.png")


if __name__ == "__main__":
    main()
