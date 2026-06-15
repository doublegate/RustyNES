#!/usr/bin/env python3
# =============================================================================
# make_icon.py  --  Parametric icon generator for the "RustyNES" project
# -----------------------------------------------------------------------------
# DESIGN CONCEPT  ("Oxidized Cog / NES Shrine")
#   * Hero:    a rust-colored castellated COG (Rust language + the literal
#              "rusty"/oxidized pun), with square 8-bit teeth.
#   * Center:  a faithfully-styled NES-001 controller in the recessed hub --
#              black cross D-pad, recessed SELECT/START pills, and CONCAVE bright
#              cherry-red B/A buttons (dished inward).
#   * Wordmark: "Rusty" above and "NES" below the controller, set in the
#              Press Start 2P pixel face and Nintendo red, baked to vector
#              paths so the look is identical regardless of installed fonts.
#   * Corners: four classic NES peripherals tucked into the plate corners and
#              rotated along each diagonal -- the front-loader CONSOLE (TL), a
#              grey CARTRIDGE (TR), the grey ZAPPER (BL), and R.O.B. (BR). They
#              are drawn *behind* the cog so the teeth overlap them for depth.
#
#   No trademarked artwork is traced; every element is an original geometric
#   stylization. The wordmark uses the open-licensed (OFL) Press Start 2P font.
#
# RENDERING NOTES
#   Shading uses gradients + translucent overlay shapes only (no SVG <filter>),
#   so cairosvg rasterizes crisply at every size. Text is converted to <path>
#   via fontTools, removing any runtime font dependency.
#
# USAGE
#   python3 make_icon.py [out_dir] [font_path]
#     out_dir    default ./out
#     font_path  default ./PressStart2P.ttf
#
# DEPENDENCIES: cairosvg, Pillow, fonttools.
# =============================================================================

import math
import os
import sys

import cairosvg
from PIL import Image, ImageDraw, ImageFont
from fontTools.ttLib import TTFont
from fontTools.pens.svgPathPen import SVGPathPen
from fontTools.pens.transformPen import TransformPen

# -----------------------------------------------------------------------------
# Author-space canvas (512x512), scaled at rasterization time.
# -----------------------------------------------------------------------------
CANVAS = 512
CX = CY = CANVAS / 2.0
PLATE_RX = 112                      # rounded-square corner radius

# --- Cog geometry ------------------------------------------------------------
GEAR_TEETH     = 24          # finer gear: double the tooth count...
R_TOOTH_TIP    = 190.0       # ...at half the tooth height (16) + smaller OD (more corner room)
R_TOOTH_ROOT   = 174.0
TOOTH_FRACTION = 0.52

# --- Concentric rings --------------------------------------------------------
R_GROOVE = 156.0
R_HUB    = 132.0

# --- NES controller ----------------------------------------------------------
CTRL_W, CTRL_H = 232.0, 94.0
CTRL_CX, CTRL_CY = CX, CY

# --- Wordmark (cap height in px, and vertical center) ------------------------
WORD_TOP   = ("Rusty", 27.0, CX, 184.0)
WORD_BOT   = ("NES",   35.0, CX, 338.0)
NINTENDO_RED = "#e60012"
RED_SHADOW   = "#7a0008"

# --- Corner peripherals: (drawer, anchor_radius, angle_deg, scale) -----------
# Anchor radius is distance from icon center along the 45-degree diagonal.
def corner_xy(quadrant, r):
    """Return the (x, y) anchor for a quadrant at diagonal distance `r`."""
    d = r / math.sqrt(2.0)
    return {
        "tl": (CX - d, CY - d), "tr": (CX + d, CY - d),
        "bl": (CX - d, CY + d), "br": (CX + d, CY + d),
    }[quadrant]


# =============================================================================
# Geometry helpers
# =============================================================================
def polar(cx, cy, r, ang):
    return (cx + r * math.cos(ang), cy + r * math.sin(ang))


def gear_path(cx, cy, r_tip, r_root, teeth, tooth_frac):
    """SVG path for a castellated (square-toothed) cog."""
    pitch = 2.0 * math.pi / teeth
    tip_half = pitch * tooth_frac / 2.0
    pts = []
    for i in range(teeth):
        base = i * pitch
        pts.append(polar(cx, cy, r_tip,  base - tip_half))
        pts.append(polar(cx, cy, r_tip,  base + tip_half))
        pts.append(polar(cx, cy, r_root, base + tip_half))
        pts.append(polar(cx, cy, r_root, base + pitch - tip_half))
    d = "M {:.3f},{:.3f} ".format(*pts[0])
    d += " ".join("L {:.3f},{:.3f}".format(x, y) for x, y in pts[1:]) + " Z"
    return d


def cross_path(cx, cy, arm_len, arm_thk, r):
    """SVG path for a rounded plus/cross (the D-pad)."""
    L, T = arm_len, arm_thk
    corners = [(-T, -L), (T, -L), (T, -T), (L, -T), (L, T), (T, T),
               (T, L), (-T, L), (-T, T), (-L, T), (-L, -T), (-T, -T)]
    n = len(corners)
    dist = lambda p, q: math.hypot(q[0] - p[0], q[1] - p[1])
    lerp = lambda p, q, t: (p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t)
    d = ""
    for i in range(n):
        prev, cur, nxt = corners[(i - 1) % n], corners[i], corners[(i + 1) % n]
        rin = min(r, dist(prev, cur) / 2.0, dist(cur, nxt) / 2.0)
        a = lerp(cur, prev, rin / max(dist(prev, cur), 1e-6))
        b = lerp(cur, nxt,  rin / max(dist(cur, nxt),  1e-6))
        ax, ay, bx, by = cx + a[0], cy + a[1], cx + b[0], cy + b[1]
        qx, qy = cx + cur[0], cy + cur[1]
        d += ("M {:.2f},{:.2f} " if i == 0 else "L {:.2f},{:.2f} ").format(ax, ay)
        d += "Q {:.2f},{:.2f} {:.2f},{:.2f} ".format(qx, qy, bx, by)
    return d + "Z"


# =============================================================================
# Text -> vector path (Press Start 2P, baked to absolute SVG coordinates)
# =============================================================================
class _Font:
    """Lazy fontTools wrapper exposing glyph outlines + advances."""
    def __init__(self, path):
        self.tt = TTFont(path)
        self.upm = self.tt["head"].unitsPerEm
        self.cap = getattr(self.tt["OS/2"], "sCapHeight", None) or int(0.7 * self.upm)
        self.gs = self.tt.getGlyphSet()
        self.cmap = self.tt.getBestCmap()
        self.hmtx = self.tt["hmtx"]

    def text_path(self, text, cap_px, cx, cy_center):
        """Return (path_d, width_px). Glyphs are emitted in final SVG space:
        scaled to `cap_px` cap height, y-flipped, horizontally centered on `cx`
        and vertically centered on `cy_center`."""
        scale = cap_px / float(self.cap)
        advances = [self.hmtx[self.cmap[ord(c)]][0] for c in text]
        width_px = sum(advances) * scale
        origin_x = cx - width_px / 2.0
        baseline = cy_center + (self.cap * scale) / 2.0
        svg = SVGPathPen(self.gs)
        xc = 0.0
        for c in text:
            gname = self.cmap[ord(c)]
            mat = (scale, 0, 0, -scale, origin_x + xc * scale, baseline)
            self.gs[gname].draw(TransformPen(svg, mat))
            xc += self.hmtx[gname][0]
        return svg.getCommands(), width_px


# =============================================================================
# Corner peripherals  (each drawn centered on the local origin, "upright")
#   Palettes share a cohesive NES-grey family.
# =============================================================================
# Research-derived palette (Evan-Amos Wikimedia hardware photos, 2026 image study).
# Shared light-grey shell family -- the NES "warm off-white" ABS, top-lit.
SHELL_HI = "#e2ded4"     # top-edge catch-light
SHELL    = "#cfcdc6"     # main shell face
SHELL_MD = "#bdbcb6"     # mid shade
SHELL_LO = "#a7a299"     # shadow side / lower-light
GRY_ED   = "#5f5e59"     # thin edge key-line
# Mid + dark greys (bezels, control bands, neck ribs, receding faces)
DARK_GRY = "#5a5a5e"     # mid dark-grey accent (bezel / base collar / neck)
GRY_XD   = "#46453f"     # darkest grey (receding side faces / extruded thickness)
CHARCOAL = "#2b2b2e"     # near-black plastic (R.O.B. visor, dark plate, recess)
DARK1, DARK2 = "#26262a", "#16171b"   # deep recess interiors
# Device reds
LED_RED  = "#c0392b"     # power LED / R.O.B. eyes (lit) / device red
LED_CORE = "#ff4a3d"     # hot red highlight
RED_RING = "#7e1b12"     # red shadow / lens ring
# Legacy aliases (kept so any incidental reference still resolves)
GRY_HI, GRY_MD, GRY_LO = SHELL_HI, SHELL_MD, SHELL_LO
NES_DK, GRY_DOOR = DARK_GRY, "#d2d1cb"


def _shadow(d, dx=3, dy=4, op=0.32):
    """Wrap a path 'd' string as a soft offset drop shadow."""
    return f'<path d="{d}" transform="translate({dx},{dy})" fill="#000" fill-opacity="{op}"/>'


def draw_console():
    """NES-001 front-loader in a 3/4 cabinet projection (prominent flat top deck +
    front face), from the multi-angle Evan-Amos reference study. Light-grey body on a
    slightly darker base; the iconic recessed dark CARTRIDGE BAY in the center of the
    front (its dark slot carried up onto the top deck) with the top-deck vent louvers
    to its left; the red 'Nintendo Entertainment System' wordmark at front-left; two
    LIGHT-grey raised POWER/RESET buttons beside the red LED at lower-left; and two
    recessed dark 7-pin controller ports at lower-right."""
    LIGHT, LIGHT_HI, LIGHT_LO = "#cdccc6", "#deddd7", "#b4b3ad"
    BASE = "#8f8e88"                          # darker grey base / foot
    BAY, BAYHI = "#222226", "#3a3a40"
    PORT = "#141418"
    W, H = 116.0, 38.0                        # front-face footprint, centered on origin
    L, T = -W / 2.0, -H / 2.0
    d = 18.0                                  # top-deck depth (recede up-and-right)
    fx = lambda f: L + f * W
    fy = lambda f: T + f * H
    front = f"M {L},{T} h{W} v{H} h{-W} Z"
    top   = f"M {L},{T} h{W} l{d},{-d} h{-W} Z"
    side  = f"M {L + W},{T} l{d},{-d} v{H} l{-d},{d} Z"

    s = [_shadow(front)]
    # right side wall + flat top deck
    s.append(f'<path d="{side}" fill="{GRY_XD}"/>')
    s.append(f'<path d="{top}" fill="url(#conTop)" stroke="{GRY_ED}" stroke-width="1"/>')
    # top-deck vent louvers (group to the left of the bay)
    for i in range(8):
        vx = fx(0.16) + i * 2.6
        s.append(f'<path d="M {vx:.2f},{T} l{d},{-d}" stroke="{LIGHT_LO}" stroke-opacity="0.8" stroke-width="0.8"/>')
    # cartridge-bay slot carried onto the top deck (dark notch straddling the edge)
    s.append(f'<path d="M {fx(0.40):.2f},{T} l{d},{-d} h{0.20 * W:.2f} l{-d},{d} Z" fill="{BAY}"/>')
    # front face: light body
    s.append(f'<path d="{front}" fill="url(#conBody)" stroke="{GRY_ED}" stroke-width="1.3"/>')
    # darker base strip along the bottom
    s.append(f'<rect x="{L}" y="{fy(0.80):.2f}" width="{W}" height="{0.20 * H:.2f}" fill="url(#conBase)"/>')
    s.append(f'<line x1="{L}" y1="{fy(0.80):.2f}" x2="{L + W}" y2="{fy(0.80):.2f}" stroke="{GRY_ED}" stroke-width="0.8"/>')
    # recessed dark cartridge bay (center of the front)
    bx, by = fx(0.40), T + 2.0
    bw, bh = 0.20 * W, 0.74 * H
    s.append(f'<rect x="{bx:.2f}" y="{by:.2f}" width="{bw:.2f}" height="{bh:.2f}" rx="1.5" fill="url(#conBay)" stroke="#101013" stroke-width="0.8"/>')
    s.append(f'<rect x="{bx:.2f}" y="{by:.2f}" width="{bw:.2f}" height="2.2" fill="{BAYHI}" opacity="0.6"/>')
    s.append(f'<rect x="{bx + 1.5:.2f}" y="{by + bh * 0.46:.2f}" width="{bw - 3:.2f}" height="1.3" fill="#0d0d10"/>')   # door seam
    # red Nintendo wordmark (front-left)
    s.append(f'<text x="{fx(0.04):.2f}" y="{fy(0.40):.2f}" font-family="Arial,Helvetica,sans-serif" font-size="6.5" font-weight="700" font-style="italic" fill="{NINTENDO_RED}">Nintendo</text>')
    s.append(f'<text x="{fx(0.04):.2f}" y="{fy(0.40) + 4.3:.2f}" font-family="Arial,Helvetica,sans-serif" font-size="2.6" letter-spacing="0.3" fill="#3a3a3c">ENTERTAINMENT SYSTEM</text>')
    # lower-left: red LED + LIGHT-grey raised POWER/RESET buttons
    s.append(f'<circle cx="{fx(0.05):.2f}" cy="{fy(0.66):.2f}" r="1.5" fill="{LED_RED}"/>')
    s.append(f'<circle cx="{fx(0.05) - 0.4:.2f}" cy="{fy(0.66) - 0.5:.2f}" r="0.6" fill="#ffd0cc"/>')
    for bx0 in (fx(0.09), fx(0.21)):
        s.append(f'<rect x="{bx0:.2f}" y="{fy(0.58):.2f}" width="{0.09 * W:.2f}" height="{0.18 * H:.2f}" rx="1" fill="{LIGHT}" stroke="{GRY_ED}" stroke-width="0.7"/>')
        s.append(f'<rect x="{bx0:.2f}" y="{fy(0.58):.2f}" width="{0.09 * W:.2f}" height="1.5" rx="1" fill="{LIGHT_HI}"/>')
    s.append(f'<text x="{fx(0.085):.2f}" y="{fy(0.79):.2f}" font-family="Arial" font-size="2.0" fill="#7a2a20">POWER  RESET</text>')
    # lower-right: two recessed dark 7-pin controller ports
    for px0 in (fx(0.65), fx(0.80)):
        pw, ph = 0.11 * W, 0.26 * H
        py0 = fy(0.54)
        s.append(f'<rect x="{px0:.2f}" y="{py0:.2f}" width="{pw:.2f}" height="{ph:.2f}" rx="1.2" fill="{PORT}" stroke="{BASE}" stroke-width="0.7"/>')
        for j in range(7):
            hx = px0 + pw * 0.5 + (j - 3) * 1.2
            s.append(f'<circle cx="{hx:.2f}" cy="{py0 + ph * 0.5:.2f}" r="0.5" fill="#3a3a40"/>')
    return "\n".join(s)


def draw_cartridge():
    """NES Game Pak, front view, classic 'black box' launch title (Super Mario Bros /
    Gyromite reference). Warm-grey shell: a LEFT column of vertical vent ridges
    (stopping at a ledge), a small top-edge notch, bottom corners that step inward to
    a narrower base, and the embossed downward triangle. The right-offset BLACK label
    has a colored pixel-art scene, a bold title, the red Nintendo pill, an action/robot
    series box, and the gold 'Seal of Quality'. Generic non-trademarked art."""
    G, GH, GL, GE = "#8a8884", "#9c9a96", "#6e6c68", "#5a5854"
    W, H = 52.0, 58.0                         # shell footprint, centered on origin
    L, T = -W / 2.0, -H / 2.0
    fx = lambda f: L + f * W
    fy = lambda f: T + f * H
    r = 2.4

    # shell silhouette: top notch + full body + bottom corners stepped inward
    shell = (
        f"M {L + r:.2f},{T:.2f} "
        f"L {fx(0.30):.2f},{T:.2f} L {fx(0.30):.2f},{fy(0.05):.2f} "
        f"L {fx(0.44):.2f},{fy(0.05):.2f} L {fx(0.44):.2f},{T:.2f} "          # top notch
        f"L {L + W - r:.2f},{T:.2f} Q {L + W:.2f},{T:.2f} {L + W:.2f},{T + r:.2f} "
        f"L {L + W:.2f},{fy(0.85):.2f} L {fx(0.955):.2f},{fy(0.89):.2f} "      # right edge + step
        f"L {fx(0.955):.2f},{T + H - r:.2f} Q {fx(0.955):.2f},{T + H:.2f} {fx(0.955) - r:.2f},{T + H:.2f} "
        f"L {fx(0.045) + r:.2f},{T + H:.2f} Q {fx(0.045):.2f},{T + H:.2f} {fx(0.045):.2f},{T + H - r:.2f} "
        f"L {fx(0.045):.2f},{fy(0.89):.2f} L {L:.2f},{fy(0.85):.2f} "         # bottom-left + step
        f"L {L:.2f},{T + r:.2f} Q {L:.2f},{T:.2f} {L + r:.2f},{T:.2f} Z"
    )
    s = [_shadow(shell)]
    s.append(f'<path d="{shell}" transform="translate(2.5,3.5)" fill="{GE}"/>')          # thickness
    s.append(f'<path d="{shell}" fill="url(#cartGrey)" stroke="{GE}" stroke-width="1.2"/>')
    # left vent panel: a slightly recessed rectangle filled with HORIZONTAL grooves
    px0, px1, py0, py1 = fx(0.13), fx(0.40), fy(0.15), fy(0.85)
    s.append(f'<rect x="{px0:.2f}" y="{py0:.2f}" width="{px1 - px0:.2f}" height="{py1 - py0:.2f}" fill="#838179"/>')
    s.append(f'<rect x="{px0:.2f}" y="{py0:.2f}" width="{px1 - px0:.2f}" height="{py1 - py0:.2f}" fill="none" stroke="{GE}" stroke-width="0.6"/>')
    ng = 26
    for i in range(ng):
        gy = py0 + (py1 - py0) * (i + 0.5) / ng
        s.append(f'<line x1="{px0 + 1:.2f}" y1="{gy:.2f}" x2="{px1 - 1:.2f}" y2="{gy:.2f}" stroke="{GL}" stroke-width="0.85"/>')
        s.append(f'<line x1="{px0 + 1:.2f}" y1="{gy + 0.5:.2f}" x2="{px1 - 1:.2f}" y2="{gy + 0.5:.2f}" stroke="{GH}" stroke-opacity="0.45" stroke-width="0.4"/>')
    # embossed downward triangle (lower-center)
    tcx, tcy, ts = fx(0.60), fy(0.90), 0.06 * W
    s.append(f'<path d="M {tcx - ts:.2f},{tcy:.2f} L {tcx + ts:.2f},{tcy:.2f} L {tcx:.2f},{tcy + ts:.2f} Z" fill="none" stroke="{GL}" stroke-width="0.9"/>')
    s.append(f'<path d="M {tcx - ts:.2f},{tcy + 0.7:.2f} L {tcx + ts:.2f},{tcy + 0.7:.2f} L {tcx:.2f},{tcy + ts + 0.7:.2f} Z" fill="none" stroke="{GH}" stroke-opacity="0.5" stroke-width="0.6"/>')
    # ---- right-offset BLACK label ----------------------------------------------
    lx, ly = fx(0.40), fy(0.14)
    lw, lh = 0.56 * W, 0.52 * H
    s.append(f'<rect x="{lx - 0.8:.2f}" y="{ly - 0.8:.2f}" width="{lw + 1.6:.2f}" height="{lh + 1.6:.2f}" rx="1" fill="#000" fill-opacity="0.25"/>')
    s.append(f'<rect x="{lx:.2f}" y="{ly:.2f}" width="{lw:.2f}" height="{lh:.2f}" rx="0.8" fill="#0b0b11" stroke="#000" stroke-width="0.4"/>')
    # pixel-art scene (top ~52% of label), colored background + blocks + figure
    ax, ay, aw, ah = lx + 1, ly + 1, lw - 2, lh * 0.50
    s.append(f'<rect x="{ax:.2f}" y="{ay:.2f}" width="{aw:.2f}" height="{ah:.2f}" fill="#15418f"/>')                   # sky
    for bxr, byr in [(0.56, 0.08), (0.74, 0.08), (0.56, 0.40), (0.74, 0.40), (0.65, 0.66)]:
        s.append(f'<rect x="{ax + aw * bxr:.2f}" y="{ay + ah * byr:.2f}" width="{aw * 0.15:.2f}" height="{ah * 0.26:.2f}" rx="0.3" fill="#9bd4ec"/>')   # blocks
    figx = ax + aw * 0.18
    s.append(f'<rect x="{figx:.2f}" y="{ay + ah * 0.20:.2f}" width="{aw * 0.13:.2f}" height="{ah * 0.18:.2f}" fill="#e6b58a"/>')   # head
    s.append(f'<rect x="{figx - 1:.2f}" y="{ay + ah * 0.38:.2f}" width="{aw * 0.17:.2f}" height="{ah * 0.30:.2f}" fill="#c8482e"/>')  # body
    s.append(f'<rect x="{figx:.2f}" y="{ay + ah * 0.68:.2f}" width="{aw * 0.15:.2f}" height="{ah * 0.22:.2f}" fill="#5a3a1e"/>')   # legs
    s.append(f'<circle cx="{ax + aw * 0.48:.2f}" cy="{ay + ah * 0.58:.2f}" r="{ah * 0.07:.2f}" fill="#f0c030"/>')                # coin
    # bold title + system line
    ty = ay + ah + 1.6
    s.append(f'<rect x="{lx + 1.6:.2f}" y="{ty:.2f}" width="{lw * 0.60:.2f}" height="{lh * 0.085:.2f}" rx="0.4" fill="#d83a2a"/>')      # title
    s.append(f'<rect x="{lx + 1.6:.2f}" y="{ty + lh * 0.095:.2f}" width="{lw * 0.44:.2f}" height="{lh * 0.045:.2f}" rx="0.3" fill="#e8902a"/>')  # subtitle
    # red Nintendo pill
    py = ty + lh * 0.155
    s.append(f'<rect x="{lx + 1.6:.2f}" y="{py:.2f}" width="{lw * 0.36:.2f}" height="{lh * 0.085:.2f}" rx="{lh * 0.042:.2f}" fill="#c0201c" stroke="#fff" stroke-width="0.3"/>')
    s.append(f'<text x="{lx + 1.6 + lw * 0.18:.2f}" y="{py + lh * 0.064:.2f}" font-family="Arial" font-size="2.0" font-weight="700" fill="#fff" text-anchor="middle">Nintendo</text>')
    # series box (lower-left) + gold seal (lower-right)
    sb = lh * 0.13
    s.append(f'<rect x="{lx + 1.6:.2f}" y="{ly + lh - sb - 1.2:.2f}" width="{sb:.2f}" height="{sb:.2f}" fill="none" stroke="#c8a23a" stroke-width="0.4"/>')
    s.append(f'<circle cx="{lx + 1.6 + sb / 2:.2f}" cy="{ly + lh - sb / 2 - 1.2:.2f}" r="{sb * 0.28:.2f}" fill="#e8c33a"/>')
    sd = lh * 0.085
    s.append(f'<circle cx="{lx + lw - sd - 1.2:.2f}" cy="{ly + lh - sd - 1.2:.2f}" r="{sd:.2f}" fill="#d4af37" stroke="#9a7a1f" stroke-width="0.4"/>')
    s.append(f'<circle cx="{lx + lw - sd - 1.2:.2f}" cy="{ly + lh - sd - 1.2:.2f}" r="{sd * 0.58:.2f}" fill="#f0d77a"/>')
    return "\n".join(s)


def draw_zapper():
    """Original 1985 grey NES Zapper (NES-005), side profile, barrel toward +x
    (reference study). Faithful two-tone: a LIGHT-grey shell/upper-slab over a
    DARK-grey barrel sleeve and grip, the single RED trigger sitting in an open
    notch, a wedge rear-sight hump at the top-back (the tallest point), horizontal
    grip texture ridges, and the black cable trailing from the grip butt. (Not the
    1989 all-grey/orange revision.)"""
    LIGHT, LIGHT_HI, LIGHT_LO = "#d9d7d0", "#ece9e2", "#c3c1b9"
    DARK, DARK_HI, DARK_LO = "#7c7d77", "#95968f", "#5e5f59"
    RED, RED_HI = "#c63a2c", "#e0584a"

    # long dark barrel (a STRAIGHT uniform tube with a beveled muzzle)
    barrel = "M 15,-11 L 62,-11 L 65,-9 L 65,-5 L 62,-3 L 15,-3 Z"
    # sleeker light-grey body/receiver (a horizontal wedge)
    body   = "M -28,-8 L 2,-11 L 20,-11 L 24,-7 L 24,3 L 8,5 L -16,8 L -27,5 Z"
    comb   = "M 2,-11 L 18,-11 L 16,-17 L 4,-17 Z"                                # raised top comb
    # long, near-vertical dark grip with a rounded butt
    grip   = "M -20,5 L -4,7 L -2,42 Q -2,47 -7,47 L -22,47 Q -27,47 -26,42 Z"

    s = [_shadow(body), _shadow(grip), _shadow(barrel)]
    # grip (dark) + fine horizontal texture ridges
    s.append(f'<path d="{grip}" fill="url(#zapDark)" stroke="{DARK_LO}" stroke-width="1.1"/>')
    for i in range(11):
        gy = 11 + i * 3.1
        s.append(f'<path d="M {-23 + i * 0.35:.2f},{gy:.2f} L {-3 + i * 0.35:.2f},{gy + 1.1:.2f}" stroke="{DARK_LO}" stroke-width="0.8"/>')
    # dark barrel + front sight + muzzle aperture
    s.append(f'<path d="{barrel}" fill="url(#zapBarrel)" stroke="{DARK_LO}" stroke-width="1"/>')
    s.append(f'<rect x="15" y="-11" width="47" height="1.5" fill="{DARK_HI}" opacity="0.5"/>')        # straight top sheen
    s.append(f'<rect x="52" y="-13" width="3" height="2.2" rx="0.4" fill="{DARK}" stroke="{DARK_LO}" stroke-width="0.4"/>')   # front sight
    s.append(f'<rect x="62.5" y="-9.5" width="2" height="5" rx="0.8" fill="#1c1c1c"/>')               # muzzle aperture
    # light-grey body (over barrel rear + grip top)
    s.append(f'<path d="{body}" fill="url(#zapLight)" stroke="{GRY_ED}" stroke-width="1.2"/>')
    s.append(f'<path d="M -28,-8 L 2,-11 L 20,-11 L 21,-9 L -26,-6 Z" fill="{LIGHT_HI}" opacity="0.6"/>')   # top catch-light
    s.append(f'<path d="M 8,5 L 24,3 L 24,4 L 8,6 Z" fill="{LIGHT_LO}" opacity="0.6"/>')                   # underside shade
    # raised comb of ~5 diagonal ridges on top, at the barrel junction
    s.append(f'<path d="{comb}" fill="{LIGHT_LO}" stroke="{DARK_LO}" stroke-width="0.5"/>')
    for i in range(5):
        cx0 = 4 + i * 2.6
        s.append(f'<path d="M {cx0:.2f},-11.5 L {cx0 + 2:.2f},-16.5" stroke="{DARK_LO}" stroke-width="0.7"/>')
    # screw dots + red Nintendo Zapper logo on the flank
    s.append(f'<circle cx="-12" cy="-2" r="1.0" fill="{DARK_LO}"/>')
    s.append(f'<circle cx="14" cy="-6" r="0.9" fill="{DARK_LO}"/>')
    s.append(f'<text x="-25" y="-4" font-family="Arial,Helvetica,sans-serif" font-size="3" font-weight="700" font-style="italic" fill="{RED}">Nintendo</text>')
    # prominent red trigger (the lone color accent)
    s.append(f'<path d="M -2,5 Q 4,9 3,16 Q -2,17 -5,12 L -5,5 Z" fill="{RED}" stroke="{RED_HI}" stroke-width="0.5"/>')
    # black cable from the grip butt, curling down-left
    s.append(f'<path d="M -14,46 Q -20,52 -28,50" fill="none" stroke="#1c1c1c" stroke-width="2.4"/>')
    return "\n".join(s)


def draw_rob():
    """R.O.B. (Robotic Operating Buddy), NES grey, front view, ARMS RAISED holding a
    red GYRO spinner (Gyromite; NES-ROB / rob_buddy / gyro_rob reference study). The
    real proportions: a BOXY SLAB head (wider than tall) with an angled recessed black
    visor holding two round eyes, a flat top with the red LED, and side vent slits; a
    segmented rectangular SPINE; a WIDE FLAT SHOULDER-SAUCER (not a round torso) the
    dark-grey jointed arms hang from; and a wide ANGULAR octagonal two-tier pedestal
    with the red 'R.O.B.' label and feet. The arms raise to black grippers that hold
    the gyro's spindle (red flywheel disc below)."""
    ARM_O, JOINT, BLK, DARKB = "#54544f", "#46464a", "#161618", "#6a6a66"
    VISOR, SHH = "#131315", "#e6e5df"
    GYRO, GYRO_HI, SPIN = "#cc2a22", "#ef6256", "#a6a6a2"

    s = [_shadow("M -22,6 H 22 V 34 H -22 Z")]
    # ===== BASE: wide ANGULAR octagonal two-tier pedestal =====
    s.append(f'<path d="M -22,30 L -20,18 L -15,15 L 15,15 L 20,18 L 22,30 L 18,34 L -18,34 Z" fill="url(#robBase)" stroke="{GRY_ED}" stroke-width="1.1"/>')   # plinth
    s.append(f'<path d="M -22,30 L -20,18 L -15,15 L 0,15 L 0,34 L -18,34 Z" fill="{DARKB}" opacity="0.18"/>')                                                  # left shade
    s.append(f'<rect x="-15" y="24" width="30" height="5" rx="0.6" fill="#37151a"/>')                # red "R.O.B." label
    s.append(f'<rect x="-15" y="24.7" width="13" height="2" fill="#a52219"/>')
    s.append(f'<path d="M -8,30 l2,2.4 l2,-2.4 Z" fill="{JOINT}"/><path d="M 4,30 l2,2.4 l2,-2.4 Z" fill="{JOINT}"/>')   # insert arrows
    s.append(f'<rect x="-17" y="33.5" width="6" height="3" fill="{BLK}"/>')
    s.append(f'<rect x="11" y="33.5" width="6" height="3" fill="{BLK}"/>')                            # feet
    s.append(f'<path d="M -17,15 L -13,11 L 13,11 L 17,15 L 13,18 L -13,18 Z" fill="url(#robBase)" stroke="{GRY_ED}" stroke-width="0.9"/>')   # turntable collar
    s.append(f'<ellipse cx="0" cy="12.5" rx="11.5" ry="2.4" fill="{DARKB}" opacity="0.30"/>')
    # ===== LOWER SPINE (segmented), saucer to base =====
    for i in range(4):
        ny = 8 - i * 3.0
        s.append(f'<rect x="-4" y="{ny:.1f}" width="8" height="3.0" rx="1" fill="url(#robBody)" stroke="{GRY_ED}" stroke-width="0.4"/>')
        s.append(f'<rect x="-4" y="{ny + 2.0:.1f}" width="8" height="1.0" fill="{JOINT}"/>')
    # ===== SHOULDER SAUCER: WIDE + FLAT (the 'body') =====
    s.append(f'<path d="M -17,-7 Q -18,-12 -13,-14 L 13,-14 Q 18,-12 17,-7 Q 17,-2 12,-1 L -12,-1 Q -17,-2 -17,-7 Z" fill="url(#robBody)" stroke="{GRY_ED}" stroke-width="1.2"/>')
    s.append(f'<path d="M -13,-14 Q -18,-12 -17,-7 Q -17,-3 -13,-1.5 L -10,-1.5 Q -14,-4 -13,-10 Q -12,-13 -10,-14 Z" fill="{SHH}" opacity="0.45"/>')   # left highlight
    s.append(f'<ellipse cx="0" cy="-7.5" rx="13" ry="3" fill="{DARKB}" opacity="0.12"/>')           # top groove
    s.append(f'<circle cx="-13.5" cy="-8" r="2.4" fill="{JOINT}"/><circle cx="13.5" cy="-8" r="2.4" fill="{JOINT}"/>')   # arm sockets
    # ===== UPPER SPINE / NECK (segmented), head to saucer =====
    for i in range(5):
        ny = -16 - i * 2.8
        s.append(f'<rect x="-4.2" y="{ny:.1f}" width="8.4" height="2.8" rx="1.1" fill="url(#robBody)" stroke="{GRY_ED}" stroke-width="0.5"/>')
        s.append(f'<rect x="-4.2" y="{ny + 1.8:.1f}" width="8.4" height="1.0" fill="{JOINT}"/>')
    # ===== HEAD: BOXY SLAB + flat top + red LED + side vents + angled black visor =====
    s.append(f'<path d="M -14,-31 L -14,-42 Q -14,-44 -12,-44 L 12,-44 Q 14,-44 14,-42 L 14,-31 Q 14,-30 13,-30 L -13,-30 Q -14,-30 -14,-31 Z" fill="url(#robBody)" stroke="{GRY_ED}" stroke-width="1.2"/>')
    s.append(f'<rect x="-13" y="-43" width="26" height="2.2" rx="1" fill="{SHH}" opacity="0.5"/>')   # top highlight
    s.append(f'<circle cx="1" cy="-42" r="1.2" fill="{LED_RED}"/>')                                  # red LED dot
    for vx in (-13, 10.4):                                                                           # side vent slits (both sides)
        for j in range(3):
            s.append(f'<rect x="{vx}" y="{-39 + j * 2.4:.1f}" width="2.6" height="1.1" rx="0.3" fill="{JOINT}" opacity="0.6"/>')
    s.append(f'<path d="M -10,-40.5 L 10,-40.5 L 9,-31 L -9,-31 Z" fill="{VISOR}"/>')                # angled recessed black visor
    s.append(f'<path d="M -10,-40.5 L 10,-40.5 L 9.7,-38.5 L -9.7,-38.5 Z" fill="#000" opacity="0.5"/>')   # inner top shadow
    for ex in (-4.8, 4.8):                                                                           # two big round eyes
        s.append(f'<circle cx="{ex}" cy="-35.5" r="3.5" fill="url(#robLens)" stroke="#000" stroke-width="0.5"/>')
        s.append(f'<circle cx="{ex}" cy="-35.5" r="1.9" fill="#0a0a0d"/>')
        s.append(f'<circle cx="{ex - 1.2}" cy="-36.7" r="0.95" fill="#5a5a62" opacity="0.85"/>')     # reflection
    # ===== ARMS: dark-grey, jointed, raised from the saucer to the grippers =====
    for sgn in (-1, 1):
        pts = f"M {sgn*13.5},-8 L {sgn*11},-17 L {sgn*4.5},-23"
        s.append(f'<path d="{pts}" fill="none" stroke="{ARM_O}" stroke-width="5.0" stroke-linejoin="round" stroke-linecap="round"/>')   # outline
        s.append(f'<path d="{pts}" fill="none" stroke="url(#robArm)" stroke-width="3.6" stroke-linejoin="round" stroke-linecap="round"/>')
        s.append(f'<circle cx="{sgn*11}" cy="-17" r="1.7" fill="{JOINT}"/>')                          # elbow joint
    # ===== GYRO spinner held UP by the grippers =====
    s.append(f'<rect x="-1.0" y="-28" width="2.0" height="11" rx="0.6" fill="{SPIN}"/>')             # spindle
    s.append(f'<ellipse cx="0" cy="-19" rx="7.5" ry="2.8" fill="{GYRO}" stroke="#6e0d09" stroke-width="0.5"/>')   # red flywheel disc
    s.append(f'<ellipse cx="0" cy="-20" rx="7.5" ry="1.7" fill="{GYRO_HI}" opacity="0.55"/>')        # disc highlight
    s.append(f'<rect x="1.0" y="-25.5" width="3.3" height="3.6" rx="0.5" fill="{BLK}"/>')            # right gripper
    s.append(f'<rect x="-4.3" y="-25.5" width="3.3" height="3.6" rx="0.5" fill="{BLK}"/>')           # left gripper
    s.append(f'<ellipse cx="0" cy="-28" rx="2.4" ry="1.0" fill="{SPIN}"/>')                          # spindle top knob
    # ===== coiled cable on the right of the base =====
    s.append(f'<path d="M 18,21 q 6,-2 6,3 q 0,5 -5,3 q -5,-2 -1,4" fill="none" stroke="{BLK}" stroke-width="1.4"/>')
    return "\n".join(s)


CORNERS = [
    # (drawer, quadrant, radius-from-center, svg_rotation_deg, scale)
    # Each peripheral is authored upright and rotated to lie along its corner
    # diagonal; they sit BEHIND the cog, so the teeth overlap their inner edges
    # for depth. Enlarged to fill the corners now that the gear is finer/smaller.
    (draw_console,   "tl", 227, -45, 1.02),
    (draw_cartridge, "tr", 252,  45, 1.45),
    (draw_zapper,    "bl", 228,  45, 1.42),
    (draw_rob,       "br", 235,  45, 1.50),
]


def corner_group(drawer, quadrant, r, angle, scale):
    ax, ay = corner_xy(quadrant, r)
    return (f'<g transform="translate({ax:.2f},{ay:.2f}) rotate({angle}) '
            f'scale({scale})">\n{drawer()}\n</g>')


# =============================================================================
# NES controller (with CONCAVE bright-red A/B buttons)
# =============================================================================
def nes_controller(cx, cy, w, h):
    """NES-001 control pad, plan view (multi-angle reference study): a light-grey
    body dominated by a large dark charcoal FACE PLATE that carries the black cross
    D-pad (left), two medium-grey accent bars + the SELECT/START dark pills in a
    light-grey housing with red labels above (center), the red Nintendo logo
    (upper-right), and the two concave bright-red A/B buttons in a light-grey housing
    with red labels below (right)."""
    left, right = cx - w / 2.0, cx + w / 2.0
    top = cy - h / 2.0
    body_r = 0.14 * h
    plx, ply, plw, plh = left + 0.045 * w, top + 0.11 * h, 0.91 * w, 0.78 * h   # dark face plate
    dcx, dcy = left + 0.205 * w, cy + 0.07 * h
    daL, daT = 0.245 * h, 0.088 * h
    ssx, ssy = cx - 0.015 * w, cy + 0.13 * h
    ss_w, ss_h = 0.25 * w, 0.20 * h
    pw, ph = 0.092 * w, 0.066 * h
    pL, pR = (ssx - 0.064 * w, ssy), (ssx + 0.064 * w, ssy)
    abx, aby = left + 0.805 * w, cy + 0.10 * h
    ab_w, ab_h = 0.295 * w, 0.30 * h
    br = 0.118 * h
    bB, bA = (abx - 0.068 * w, aby), (abx + 0.068 * w, aby)

    f = []
    # body shadow + light-grey body + inner bevel
    f.append(f'<rect x="{left-3}" y="{top+6}" width="{w+6}" height="{h+6}" rx="{body_r+3}" fill="#000" fill-opacity="0.22"/>')
    f.append(f'<rect x="{left-1}" y="{top+3}" width="{w+2}" height="{h+3}" rx="{body_r+1}" fill="#000" fill-opacity="0.30"/>')
    f.append(f'<rect x="{left}" y="{top}" width="{w}" height="{h}" rx="{body_r}" fill="url(#cbody)" stroke="#6f6e69" stroke-width="1.5"/>')
    f.append(f'<rect x="{left+1.5}" y="{top+1.5}" width="{w-3}" height="{h-3}" rx="{body_r-1.5}" fill="none" stroke="#ffffff" stroke-opacity="0.4" stroke-width="1.5"/>')
    # dark charcoal face plate (the dominant feature)
    f.append(f'<rect x="{plx}" y="{ply}" width="{plw}" height="{plh}" rx="{0.07*h}" fill="#37373a" stroke="#1d1d1f" stroke-width="1.2"/>')
    f.append(f'<rect x="{plx+1}" y="{ply+1}" width="{plw-2}" height="{plh-2}" rx="{0.06*h}" fill="none" stroke="#525256" stroke-opacity="0.55" stroke-width="0.8"/>')
    # D-pad: recessed dark square + black cross
    well = 0.60 * h
    f.append(f'<rect x="{dcx-well/2}" y="{dcy-well/2}" width="{well}" height="{well}" rx="{0.10*h}" fill="#27272a"/>')
    f.append(f'<path d="{cross_path(dcx+1, dcy+1.3, daL, daT, 0.03*h)}" fill="#000" fill-opacity="0.4"/>')
    f.append(f'<path d="{cross_path(dcx, dcy, daL, daT, 0.03*h)}" fill="url(#dpadg)" stroke="#000" stroke-opacity="0.5" stroke-width="1" stroke-linejoin="round"/>')
    f.append(f'<circle cx="{dcx}" cy="{dcy}" r="{0.105*h}" fill="#26282d"/>')
    f.append(f'<circle cx="{dcx-1.4}" cy="{dcy-1.4}" r="{0.045*h}" fill="#4a4d53"/>')
    # two medium-grey accent bars (upper-center of the plate)
    for i in range(2):
        aby0 = top + 0.27 * h + i * 0.10 * h
        f.append(f'<rect x="{cx-0.135*w}" y="{aby0}" width="{0.27*w}" height="{0.05*h}" rx="{0.025*h}" fill="#9a9a98"/>')
    # SELECT / START: light-grey housing + dark pills + red labels ABOVE
    f.append(f'<rect x="{ssx-ss_w/2}" y="{ssy-ss_h/2}" width="{ss_w}" height="{ss_h}" rx="{ss_h/2}" fill="#c6c4bf" stroke="#7d7c77" stroke-width="0.8"/>')
    for (px, py) in (pL, pR):
        f.append(f'<rect x="{px-pw/2}" y="{py-ph/2}" width="{pw}" height="{ph}" rx="{ph/2}" fill="#2e3138"/>')
        f.append(f'<rect x="{px-pw/2}" y="{py-ph/2}" width="{pw}" height="{ph/2}" rx="{ph/2}" fill="#474b53"/>')
    for (px, txt) in ((pL[0], "SELECT"), (pR[0], "START")):
        f.append(f'<text x="{px}" y="{ssy-ss_h/2-2}" font-family="Arial,Helvetica,sans-serif" font-size="6" font-weight="700" fill="{NINTENDO_RED}" text-anchor="middle">{txt}</text>')
    # red Nintendo logo (upper-right of the plate)
    f.append(f'<text x="{abx}" y="{top+0.34*h}" font-family="Arial,Helvetica,sans-serif" font-size="{0.13*h}" font-weight="700" font-style="italic" fill="{NINTENDO_RED}" text-anchor="middle">Nintendo</text>')
    # A / B: light-grey housing + dark rings + CONCAVE bright-red buttons + red labels
    f.append(f'<rect x="{abx-ab_w/2}" y="{aby-ab_h/2}" width="{ab_w}" height="{ab_h}" rx="{0.28*ab_h}" fill="#c6c4bf" stroke="#7d7c77" stroke-width="0.8"/>')
    for (bx, by) in (bB, bA):
        f.append(f'<circle cx="{bx}" cy="{by+1}" r="{br+2.0}" fill="#000" fill-opacity="0.4"/>')             # drop shadow
        f.append(f'<circle cx="{bx}" cy="{by}" r="{br+2.0}" fill="#26282c"/>')                               # dark ring well
        f.append(f'<circle cx="{bx}" cy="{by}" r="{br}" fill="#9a1c1c"/>')                                   # rim base (dark red)
        f.append(f'<circle cx="{bx}" cy="{by}" r="{br*0.82}" fill="url(#btnDish)"/>')                        # dished face (dark top -> light bottom)
        f.append(f'<circle cx="{bx}" cy="{by}" r="{br*0.82}" fill="url(#dishShade)"/>')                      # inner shadow from the top rim
        f.append(f'<ellipse cx="{bx}" cy="{by+br*0.42}" rx="{br*0.42}" ry="{br*0.17}" fill="#ff9a90" fill-opacity="0.55"/>')  # lower-wall catch-light
    for (bx, txt) in ((bB[0], "B"), (bA[0], "A")):
        f.append(f'<text x="{bx}" y="{aby+ab_h/2+8}" font-family="Arial,Helvetica,sans-serif" font-size="10" font-weight="700" fill="{NINTENDO_RED}" text-anchor="middle">{txt}</text>')
    return "\n  ".join(f)


# =============================================================================
# SVG assembly
# =============================================================================
def build_svg(font):
    gear_d = gear_path(CX, CY, R_TOOTH_TIP, R_TOOTH_ROOT, GEAR_TEETH, TOOTH_FRACTION)
    controller = nes_controller(CTRL_CX, CTRL_CY, CTRL_W, CTRL_H)
    corners = "\n".join(corner_group(*c) for c in CORNERS)

    rusty_d, _ = font.text_path(*WORD_TOP)
    rusty_sh, _ = font.text_path(WORD_TOP[0], WORD_TOP[1], WORD_TOP[2], WORD_TOP[3] + 2.5)
    nes_d, _ = font.text_path(*WORD_BOT)
    nes_sh, _ = font.text_path(WORD_BOT[0], WORD_BOT[1], WORD_BOT[2], WORD_BOT[3] + 3.0)

    return f'''<?xml version="1.0" encoding="UTF-8"?>
<!-- RustyNES icon - master vector (512x512). Oxidized cog + NES controller hub,
     red Press Start 2P wordmark, and four corner NES peripherals. -->
<svg xmlns="http://www.w3.org/2000/svg" width="{CANVAS}" height="{CANVAS}" viewBox="0 0 {CANVAS} {CANVAS}">
  <defs>
    <linearGradient id="plate" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#30343d"/><stop offset="0.55" stop-color="#1d2026"/><stop offset="1" stop-color="#121419"/>
    </linearGradient>
    <radialGradient id="vignette" cx="0.5" cy="0.46" r="0.72">
      <stop offset="0.55" stop-color="#000" stop-opacity="0"/><stop offset="1" stop-color="#000" stop-opacity="0.45"/>
    </radialGradient>
    <linearGradient id="rust" x1="0.12" y1="0.05" x2="0.9" y2="0.98">
      <stop offset="0" stop-color="#f3a64d"/><stop offset="0.34" stop-color="#d2701f"/>
      <stop offset="0.68" stop-color="#a64a18"/><stop offset="1" stop-color="#6f2d10"/>
    </linearGradient>
    <radialGradient id="sheen" cx="0.3" cy="0.24" r="0.62">
      <stop offset="0" stop-color="#ffe6c4" stop-opacity="0.7"/><stop offset="0.45" stop-color="#ffd9a8" stop-opacity="0.14"/><stop offset="1" stop-color="#ffd9a8" stop-opacity="0"/>
    </radialGradient>
    <radialGradient id="patina" cx="0.76" cy="0.82" r="0.6">
      <stop offset="0" stop-color="#2f1004" stop-opacity="0"/><stop offset="1" stop-color="#2f1004" stop-opacity="0.62"/>
    </radialGradient>
    <radialGradient id="hub" cx="0.5" cy="0.42" r="0.62">
      <stop offset="0" stop-color="#20242b"/><stop offset="0.7" stop-color="#0f1116"/><stop offset="1" stop-color="#070809"/>
    </radialGradient>
    <linearGradient id="cbody" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#dcdbd6"/><stop offset="0.5" stop-color="#bebdb8"/><stop offset="1" stop-color="#999893"/>
    </linearGradient>
    <linearGradient id="recess" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#202329"/><stop offset="1" stop-color="#050608"/>
    </linearGradient>
    <linearGradient id="dpadg" x1="0.2" y1="0.1" x2="0.8" y2="0.95">
      <stop offset="0" stop-color="#3c3f45"/><stop offset="0.5" stop-color="#1c1e22"/><stop offset="1" stop-color="#08090b"/>
    </linearGradient>
    <!-- Concave A/B button: dished inward (dark at the top, light at the bottom,
         inverse of a dome) plus an inner top shadow, so the face reads as scooped. -->
    <linearGradient id="btnDish" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#600d0d"/><stop offset="0.5" stop-color="#a81d1d"/><stop offset="1" stop-color="#e8635a"/>
    </linearGradient>
    <radialGradient id="dishShade" cx="0.5" cy="0.12" r="0.85">
      <stop offset="0" stop-color="#000" stop-opacity="0.6"/><stop offset="0.6" stop-color="#000" stop-opacity="0.12"/><stop offset="1" stop-color="#000" stop-opacity="0"/>
    </radialGradient>
    <!-- Molded-plastic shading gradients for the corner peripherals (top-lit in
         each item's local frame). -->
    <linearGradient id="conBody" x1="0" y1="0" x2="0.15" y2="1">
      <stop offset="0" stop-color="#dad9d3"/><stop offset="0.5" stop-color="#c9c8c2"/><stop offset="1" stop-color="#b3b2ac"/>
    </linearGradient>
    <linearGradient id="conTop" x1="0" y1="0" x2="0.3" y2="1">
      <stop offset="0" stop-color="#e7e6e0"/><stop offset="1" stop-color="#cdccc6"/>
    </linearGradient>
    <linearGradient id="conBase" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#94938d"/><stop offset="1" stop-color="#7c7b75"/>
    </linearGradient>
    <radialGradient id="conBay" cx="0.5" cy="0.3" r="0.9">
      <stop offset="0" stop-color="#2e2e33"/><stop offset="1" stop-color="#161619"/>
    </radialGradient>
    <linearGradient id="cartGrey" x1="0.1" y1="0" x2="0.4" y2="1">
      <stop offset="0" stop-color="#9b9a94"/><stop offset="0.5" stop-color="#8a8884"/><stop offset="1" stop-color="#73726c"/>
    </linearGradient>
    <linearGradient id="zapLight" x1="0" y1="0" x2="0.2" y2="1">
      <stop offset="0" stop-color="#eceae3"/><stop offset="0.5" stop-color="#dad8d1"/><stop offset="1" stop-color="#bdbbb3"/>
    </linearGradient>
    <linearGradient id="zapDark" x1="0" y1="0" x2="0.25" y2="1">
      <stop offset="0" stop-color="#8e8f89"/><stop offset="0.5" stop-color="#7c7d77"/><stop offset="1" stop-color="#5e5f59"/>
    </linearGradient>
    <linearGradient id="zapBarrel" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#90918b"/><stop offset="0.45" stop-color="#76776f"/><stop offset="1" stop-color="#565751"/>
    </linearGradient>
    <linearGradient id="robBody" x1="0.1" y1="0" x2="0.4" y2="1">
      <stop offset="0" stop-color="#e0dfd9"/><stop offset="0.5" stop-color="#cdccc6"/><stop offset="1" stop-color="#a9a8a1"/>
    </linearGradient>
    <linearGradient id="robBase" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#bbbab4"/><stop offset="1" stop-color="#8f8e88"/>
    </linearGradient>
    <linearGradient id="robArm" x1="0" y1="0" x2="0.3" y2="1">
      <stop offset="0" stop-color="#8a8a86"/><stop offset="0.5" stop-color="#73736e"/><stop offset="1" stop-color="#56565a"/>
    </linearGradient>
    <radialGradient id="robLens" cx="0.4" cy="0.35" r="0.75">
      <stop offset="0" stop-color="#46464e"/><stop offset="0.6" stop-color="#26262b"/><stop offset="1" stop-color="#0e0e12"/>
    </radialGradient>
    <clipPath id="gearClip"><path d="{gear_d}"/></clipPath>
  </defs>

  <!-- Console plate -->
  <rect x="0" y="0" width="{CANVAS}" height="{CANVAS}" rx="{PLATE_RX}" fill="url(#plate)"/>
  <rect x="0" y="0" width="{CANVAS}" height="{CANVAS}" rx="{PLATE_RX}" fill="url(#vignette)"/>
  <rect x="6" y="6" width="{CANVAS-12}" height="{CANVAS-12}" rx="{PLATE_RX-6}" fill="none" stroke="#fff" stroke-opacity="0.05" stroke-width="2"/>

  <!-- Corner peripherals (behind the cog, tucked under the teeth) -->
  {corners}

  <!-- Cog body + shading -->
  <path d="{gear_d}" fill="url(#rust)" stroke="#3c1808" stroke-opacity="0.85" stroke-width="3" stroke-linejoin="round"/>
  <g clip-path="url(#gearClip)">
    <rect x="0" y="0" width="{CANVAS}" height="{CANVAS}" fill="url(#patina)"/>
    <rect x="0" y="0" width="{CANVAS}" height="{CANVAS}" fill="url(#sheen)"/>
  </g>

  <!-- Machined groove ring -->
  <circle cx="{CX}" cy="{CY}" r="{R_GROOVE}" fill="none" stroke="#4a1f0c" stroke-width="9"/>
  <circle cx="{CX}" cy="{CY}" r="{R_GROOVE-5.5}" fill="none" stroke="#f0b070" stroke-opacity="0.30" stroke-width="2"/>

  <!-- Recessed hub -->
  <circle cx="{CX}" cy="{CY}" r="{R_HUB}" fill="url(#hub)" stroke="#000" stroke-opacity="0.6" stroke-width="3"/>
  <circle cx="{CX}" cy="{CY}" r="{R_HUB-2}" fill="none" stroke="#fff" stroke-opacity="0.06" stroke-width="2"/>

  <!-- Wordmark: "Rusty" (top) / "NES" (bottom), Nintendo red, drop-shadowed -->
  <path d="{rusty_sh}" fill="{RED_SHADOW}"/>
  <path d="{rusty_d}" fill="{NINTENDO_RED}"/>
  <path d="{nes_sh}" fill="{RED_SHADOW}"/>
  <path d="{nes_d}" fill="{NINTENDO_RED}"/>

  <!-- NES controller -->
  {controller}
</svg>
'''


# =============================================================================
# Rasterization pipeline
# =============================================================================
PNG_SIZES = [16, 24, 32, 48, 64, 128, 256, 512, 1024]
ICO_SIZES = [16, 24, 32, 48, 64, 128, 256]


def main():
    out_dir = sys.argv[1] if len(sys.argv) > 1 else "out"
    font_path = sys.argv[2] if len(sys.argv) > 2 else "PressStart2P.ttf"
    png_dir = os.path.join(out_dir, "png")
    os.makedirs(png_dir, exist_ok=True)

    font = _Font(font_path)
    svg = build_svg(font)
    svg_path = os.path.join(out_dir, "rustynes.svg")
    with open(svg_path, "w") as fh:
        fh.write(svg)
    data = svg.encode("utf-8")

    for n in PNG_SIZES:
        cairosvg.svg2png(bytestring=data, write_to=os.path.join(png_dir, f"icon-{n}.png"),
                         output_width=n, output_height=n)
    Image.open(os.path.join(png_dir, "icon-256.png")).convert("RGBA").save(
        os.path.join(out_dir, "rustynes.ico"), sizes=[(s, s) for s in ICO_SIZES])
    print("wrote", svg_path, "+ png set + ico")


if __name__ == "__main__":
    main()
