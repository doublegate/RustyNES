#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Write the app's per-platform icons from this pack's masters (v3.7.1).

`generate_icons.py` builds the pack itself (Windows, macOS, Linux, web,
branding). This script takes the pack's outputs and writes the files the
Android and iOS apps build from, in the places their build systems read:

  android/app/src/main/res/mipmap-<density>/ic_launcher.png         legacy square
  android/app/src/main/res/mipmap-<density>/ic_launcher_round.png   legacy round
  android/app/src/main/res/mipmap-<density>/ic_launcher_foreground.png
                                         adaptive-icon foreground (108 dp canvas)
  android/app/src/main/res/mipmap-<density>/ic_launcher_monochrome.png
                                         Material You themed-icon layer
  android/app/src/main/res/drawable-xhdpi/tv_banner.png             Leanback banner
  ios/RustyNES/Assets.xcassets/AppIcon.appiconset/AppIcon-1024.png   single-size icon

Design rules it follows, each from the platform's own documentation:

* **Adaptive foreground** (Android): a 108 x 108 dp layer of which the system
  shows at most the central 72 dp and guarantees only a 66 dp circle, so the
  art (the transparent "RN" cartridge, trimmed to its own bounds) is scaled to
  fit 66 dp and centred. The background is the colour
  `@color/ic_launcher_background`, set to the art's own field, `#000214`.
* **Monochrome** (Android 13 themed icons): the system tints the layer by its
  ALPHA alone. The colour foreground's cartridge is opaque throughout, so it
  would tint to a featureless block; this layer's alpha is the glyph's alpha
  weighted by its brightness, so the bright "RN" letters and circuit traces
  carry the shape and the dark cartridge body falls back.
* **Legacy icons** (pre-API-26 launchers): the full-bleed square art at
  48 dp, and the same clipped to a circle for `ic_launcher_round`.
* **TV banner**: "an xhdpi resource with a size of 320 x 180 px" (Android TV's
  launcher guidance), one file; the pack's transparent wordmark centred on
  the art's field.
* **iOS**: one 1024 x 1024 universal icon (Xcode 14+ derives every size). It
  must be OPAQUE: App Store validation rejects an icon with an alpha
  channel, so the alpha is dropped onto the field colour.

Run from the repository root (needs Pillow):

    python3 assets/RustyNES_Icon_Pack/make_platform_icons.py
"""

from __future__ import annotations

import sys
from pathlib import Path

from PIL import Image, ImageChops, ImageDraw

PACK = Path(__file__).resolve().parent
ROOT = PACK.parent.parent
RES = ROOT / "android/app/src/main/res"
IOS = ROOT / "ios/RustyNES/Assets.xcassets/AppIcon.appiconset"

# The field colour of the pack's full-bleed art (sampled from its corners).
FIELD = (0x00, 0x02, 0x14)

SQUARE = PACK / "master/rustynes-app-icon-master.png"  # full-bleed, 1024
SQUARE_SMALL = PACK / "master/rustynes-small-icon-master.png"  # simplified, 1024
GLYPH = PACK / "transparent/icon/rustynes-icon-transparent-1024.png"
WORDMARK = PACK / "transparent/logo/rustynes-logo-transparent-2172x724.png"

# Android density buckets: scale factor against mdpi (1 dp = 1 px).
DENSITIES = {"mdpi": 1.0, "hdpi": 1.5, "xhdpi": 2.0, "xxhdpi": 3.0, "xxxhdpi": 4.0}


def _fit(img: Image.Image, box: int) -> Image.Image:
    """`img` scaled (Lanczos) so its longer side is `box` px."""
    w, h = img.size
    s = box / max(w, h)
    return img.resize((max(1, round(w * s)), max(1, round(h * s))), Image.LANCZOS)


def _square(size: int) -> Image.Image:
    # The pack's own rule: the simplified emblem at 48 px and below.
    src = SQUARE_SMALL if size <= 48 else SQUARE
    return Image.open(src).convert("RGBA").resize((size, size), Image.LANCZOS)


def _round(size: int) -> Image.Image:
    sq = _square(size)
    mask = Image.new("L", (size * 4, size * 4), 0)
    ImageDraw.Draw(mask).ellipse((0, 0, size * 4 - 1, size * 4 - 1), fill=255)
    mask = mask.resize((size, size), Image.LANCZOS)
    out = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    out.paste(sq, (0, 0), mask)
    return out


def _foreground(canvas: int) -> Image.Image:
    """The adaptive foreground: the glyph trimmed to its bounds, fitted to the
    66/108 safe circle's bounding square, centred on a transparent canvas."""
    glyph = Image.open(GLYPH).convert("RGBA")
    glyph = glyph.crop(glyph.getbbox())
    art = _fit(glyph, round(canvas * 66 / 108))
    out = Image.new("RGBA", (canvas, canvas), (0, 0, 0, 0))
    out.paste(art, ((canvas - art.width) // 2, (canvas - art.height) // 2), art)
    return out


def _monochrome(canvas: int) -> Image.Image:
    """The themed-icon layer: white, alpha = foreground alpha x luminance,
    stretched so the brightest detail is fully opaque."""
    fg = _foreground(canvas)
    mono = ImageChops.multiply(fg.convert("L"), fg.getchannel("A"))
    hi = max(mono.getextrema()[1], 1)
    mono = mono.point(lambda v: min(255, v * 255 // hi))
    out = Image.new("RGBA", (canvas, canvas), (255, 255, 255, 0))
    out.putalpha(mono)
    return out


def _banner(w: int, h: int) -> Image.Image:
    mark = Image.open(WORDMARK).convert("RGBA")
    mark = mark.crop(mark.getbbox())
    s = min(w * 0.9 / mark.width, h * 0.8 / mark.height)
    mark = mark.resize((round(mark.width * s), round(mark.height * s)), Image.LANCZOS)
    out = Image.new("RGBA", (w, h), FIELD + (255,))
    out.paste(mark, ((w - mark.width) // 2, (h - mark.height) // 2), mark)
    return out.convert("RGB")


def main() -> int:
    for name, k in DENSITIES.items():
        d = RES / f"mipmap-{name}"
        d.mkdir(parents=True, exist_ok=True)
        _square(round(48 * k)).save(d / "ic_launcher.png", optimize=True)
        _round(round(48 * k)).save(d / "ic_launcher_round.png", optimize=True)
        _foreground(round(108 * k)).save(d / "ic_launcher_foreground.png", optimize=True)
        _monochrome(round(108 * k)).save(d / "ic_launcher_monochrome.png", optimize=True)
    d = RES / "drawable-xhdpi"
    d.mkdir(parents=True, exist_ok=True)
    _banner(320, 180).save(d / "tv_banner.png", optimize=True)
    IOS.mkdir(parents=True, exist_ok=True)
    ios = Image.new("RGB", (1024, 1024), FIELD)
    sq = Image.open(SQUARE).convert("RGBA").resize((1024, 1024), Image.LANCZOS)
    ios.paste(sq, (0, 0), sq)
    ios.save(IOS / "AppIcon-1024.png", optimize=True)
    print("wrote Android launcher, foreground, round, TV banner; iOS AppIcon-1024")
    return 0


if __name__ == "__main__":
    sys.exit(main())
