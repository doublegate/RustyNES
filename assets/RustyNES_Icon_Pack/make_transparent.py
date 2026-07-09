#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
make_transparent.py
===================

Best-effort transparent-background variants of the RustyNES logo + icon.

The source masters are fully opaque with a near-uniform near-black field
(~#000214) around a centered, glowing emblem. There is no layered/vector source,
so a *clean* alpha cannot be regenerated; this script instead does a **corner
flood-fill key**: it removes the background region that is 4-connected to the
image border and within a colour threshold of the corner colour, leaving the
emblem (and any dark pockets *inside* it) intact.

Quality notes / caveats
------------------------
* Keying is done at FULL master resolution, then downscaled with Lanczos, so the
  alpha edge antialiases on the way down and the glow fringe is minimized.
* The emblem's outer cyan glow fades into the near-black field; the key cannot
  perfectly separate glow from background, so a faint 1-2 px dark fringe can
  remain at high zoom. A mild alpha feather softens it. For pristine output a
  transparent-background master render would be required.
* Output goes to `transparent/` and does NOT touch the opaque pack.

Usage
-----
    python3 make_transparent.py                 # defaults: read source/, write transparent/
    python3 make_transparent.py --thresh 48     # tune the background colour tolerance
"""

from __future__ import annotations

import argparse
from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter

# Sentinel colour used to mark flood-filled background (absent from the art).
SENTINEL = (255, 0, 255)

# Square-icon (primary art) transparent sizes.
ICON_SIZES = [1024, 512, 256, 128, 64]
# Simplified-emblem favicon transparent sizes.
FAVICON_SIZES = [64, 48, 32, 16]
# Logo/banner transparent widths (aspect preserved).
LOGO_WIDTHS = [2172, 1200, 600]

# Windows .ico frames for the transparent icon.
ICO_SIZES = [16, 24, 32, 48, 64, 128, 256]


def key_background(path: Path, thresh: int, feather: float) -> Image.Image:
    """Flood-fill the border-connected background to transparent at full res."""
    img = Image.open(path).convert("RGBA")
    w, h = img.size
    rgb = img.convert("RGB")

    # Fill the region 4-connected to each corner, within `thresh` of the corner
    # colour, to the sentinel. Internal dark pixels (inside the emblem) are not
    # border-connected, so they are preserved.
    for corner in [(0, 0), (w - 1, 0), (0, h - 1), (w - 1, h - 1)]:
        ImageDraw.floodfill(rgb, corner, SENTINEL, thresh=thresh)

    alpha = bytes(0 if p == SENTINEL else 255 for p in rgb.getdata())
    a = Image.frombytes("L", (w, h), alpha)
    if feather > 0:
        a = a.filter(ImageFilter.GaussianBlur(feather))
    img.putalpha(a)
    return img


def downscale(img: Image.Image, size: int) -> Image.Image:
    """Lanczos downscale of a square keyed image."""
    return img.resize((size, size), Image.LANCZOS)


def save_png(img: Image.Image, path: Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    img.save(path, format="PNG", optimize=True)


def main() -> None:
    here = Path(__file__).resolve().parent
    src = here / "source"
    ap = argparse.ArgumentParser(description="RustyNES transparent-variant generator")
    ap.add_argument("--primary", default=src / "rustynes_primary_app_icon_1x1.png", type=Path)
    ap.add_argument("--favicon", default=src / "rustynes_simplified_favicon_icon_1x1.png", type=Path)
    ap.add_argument("--banner", default=src / "rustynes_primary_logo_banner_3x1.png", type=Path)
    ap.add_argument("--out", default=here / "transparent", type=Path)
    ap.add_argument("--thresh", default=48, type=int, help="background colour tolerance")
    ap.add_argument("--feather", default=0.6, type=float, help="alpha feather radius (px)")
    args = ap.parse_args()

    out = args.out

    # --- Icon (primary art), keyed at full res then downscaled --------------- #
    icon_master = key_background(args.primary, args.thresh, args.feather)
    save_png(icon_master, out / "icon" / "rustynes-icon-transparent-master.png")
    for s in ICON_SIZES:
        save_png(downscale(icon_master, s), out / "icon" / f"rustynes-icon-transparent-{s}.png")
    # A transparent multi-res .ico.
    frames = [downscale(icon_master, s) for s in ICO_SIZES]
    (out / "icon").mkdir(parents=True, exist_ok=True)
    frames[-1].save(out / "icon" / "rustynes-transparent.ico", format="ICO",
                    sizes=[(s, s) for s in ICO_SIZES], append_images=frames[:-1])

    # --- Favicon (simplified emblem) ---------------------------------------- #
    fav_master = key_background(args.favicon, args.thresh, args.feather)
    for s in FAVICON_SIZES:
        save_png(downscale(fav_master, s), out / "favicon" / f"rustynes-favicon-transparent-{s}.png")

    # --- Logo / banner ------------------------------------------------------- #
    logo_master = key_background(args.banner, args.thresh, args.feather)
    lw, lh = logo_master.size
    for tw in LOGO_WIDTHS:
        th = round(lh * (tw / lw))
        img = logo_master if tw == lw else logo_master.resize((tw, th), Image.LANCZOS)
        save_png(img, out / "logo" / f"rustynes-logo-transparent-{tw}x{th}.png")

    total = sum(1 for _ in out.rglob("*") if _.is_file())
    print(f"Done. {total} transparent files under {out}/  (thresh={args.thresh}, feather={args.feather})")


if __name__ == "__main__":
    main()
