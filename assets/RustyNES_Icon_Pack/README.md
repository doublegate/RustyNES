# RustyNES Icon Pack

The single, consolidated icon & image set for **RustyNES** — ready-to-ship assets
for **Windows**, **macOS**, and **Linux**, plus a **web/PWA** favicon set and a
**branding** banner set. Everything is generated from the square/banner master
renders by `generate_icons.py`.

> **Provenance.** This pack supersedes and merges the two earlier bundles
> (`RustyNES_CrossPlatform_Icons` and `RustyNES_Platform_Icon_Pack`) into one. It
> takes the *union of the best* of both — every size and variant either produced —
> regenerated in a single pass so nothing is mixed from two different render runs.
> It is **not yet wired into the build**; the app currently uses the separate
> geometric icon in `assets/RustyNES_Icon/` (winit window icon, About dialog,
> README header). Adopting this pack is a follow-up decision.

## The art

A NES-cartridge **"RN" monogram** (blue `R` / orange `N`) wrapped in circuit
traces on a dark-navy field (`#1b2a4a`), with a matching **"RustyNES"** wordmark
banner ("Precise. Pure. Powerful."). Two square masters drive everything: the
**primary** app icon (full detail + circuit traces) and a **simplified** emblem
(cleaner, for small sizes). No third-party trademark text.

## Source-selection strategy

| Target size | Source used            | Why                                                    |
|-------------|------------------------|--------------------------------------------------------|
| ≤ 48 px     | simplified emblem      | the dense circuit traces turn to mush below ~64 px     |
| ≥ 64 px     | primary app icon       | full detail stays legible                              |

All resamples are **downscales** (masters are 1254 px; the largest square target
is 1024 px), so there is no upscaling softness. Downsampling is Lanczos; sizes
≤ 48 px get a mild unsharp pass to recover edge detail. Every raster is RGBA.

---

## Directory layout

```text
RustyNES_Icon_Pack/
├── README.md
├── generate_icons.py                     ← the generator (regenerates everything)
├── make_transparent.py                   ← best-effort transparent-variant generator
├── transparent/                          ← transparent-background logo/icon (see transparent/README.md)
├── source/                               ← the original supplied masters (inputs)
│   ├── rustynes_primary_app_icon_1x1.png       (1254×1254)
│   ├── rustynes_simplified_favicon_icon_1x1.png (1254×1254)
│   ├── rustynes_primary_logo_banner_3x1.png    (2172×724)
│   └── rustynes_icon_set_showcase_1x1.png       (contact sheet)
├── master/                               ← hi-res derived masters
│   ├── rustynes-app-icon-master.png            (1024)
│   ├── rustynes-small-icon-master.png          (1024)
│   └── rustynes-logo-banner-master.png         (native 2172×724)
├── windows/
│   ├── RustyNES.ico          ← multi-res: 16,20,24,32,40,48,64,128,256
│   ├── RustyNES-small.ico    ← small-icon-optimized: 16,24,32,48 (simplified art)
│   └── png/RustyNES-<n>.png  ← standalone PNGs (installers, store art)
├── macos/
│   ├── RustyNES.icns         ← Retina-complete container (16…1024, @1x/@2x)
│   └── RustyNES.iconset/     ← Apple-named PNGs (rebuild with iconutil)
├── linux/
│   ├── hicolor/<n>x<n>/apps/rustynes.png   ← freedesktop theme tree (16…512, incl. 22 & 96)
│   ├── png/rustynes-<n>x<n>.png            ← flat convenience copies
│   ├── rustynes.png          ← 512 px master (/usr/share/pixmaps)
│   └── rustynes.desktop      ← example launcher entry
├── web/
│   ├── favicon.ico           ← 16,32,48
│   ├── favicon-16x16.png · favicon-32x32.png · favicon-96x96.png
│   ├── apple-touch-icon.png  ← 180 px
│   ├── android-chrome-192x192.png · android-chrome-512x512.png
│   ├── mstile-150x150.png    ← Windows/Edge pinned tile
│   └── site.webmanifest
└── branding/
    └── rustynes-banner-<w>x<h>.png   ← 900×300, 1200×400, 1500×500, 1800×600, 2172×724
```

---

## Platform usage

### Windows

Use `windows/RustyNES.ico` as the application/executable icon — it embeds every
resolution Explorer, the taskbar, and Alt-Tab request. `RustyNES-small.ico` is a
list/tray-optimized variant built from the simplified emblem. The standalone PNGs
suit installer UIs (NSIS/WiX/Inno) and Microsoft Store listings.

### macOS

`macos/RustyNES.icns` drops into an app bundle at `YourApp.app/Contents/Resources/`
with `CFBundleIconFile` pointing at it — it carries the full Retina set (16…1024).
To rebuild from the iconset on a Mac:

```bash
iconutil -c icns macos/RustyNES.iconset -o RustyNES.icns
```

> These are full-bleed square icons. For the Big Sur "squircle" with inset
> padding, apply that mask to the 1024 px master before rebuilding — the raw art
> is intentionally left unmasked so you can choose.

### Linux

```bash
sudo cp -r linux/hicolor/*   /usr/share/icons/hicolor/
sudo cp linux/rustynes.png   /usr/share/pixmaps/
sudo cp linux/rustynes.desktop /usr/share/applications/
sudo gtk-update-icon-cache /usr/share/icons/hicolor
sudo update-desktop-database
```

The `.desktop` references the icon by name (`Icon=rustynes`) — the theme-correct
approach; edit `Exec=` to point at your binary. `linux/png/` holds flat copies for
tooling that prefers a single directory of sizes.

### Web / PWA

```html
<link rel="icon" href="/favicon.ico" sizes="any">
<link rel="icon" type="image/png" sizes="32x32" href="/favicon-32x32.png">
<link rel="icon" type="image/png" sizes="16x16" href="/favicon-16x16.png">
<link rel="apple-touch-icon" sizes="180x180" href="/apple-touch-icon.png">
<link rel="manifest" href="/site.webmanifest">
<meta name="msapplication-TileImage" content="/mstile-150x150.png">
<meta name="msapplication-TileColor" content="#1b2a4a">
```

---

## Full size / format inventory

| Platform | File(s)                       | Sizes (px)                                   | Format |
|----------|-------------------------------|----------------------------------------------|--------|
| Windows  | RustyNES.ico                  | 16,20,24,32,40,48,64,128,256 (one file)      | ICO    |
| Windows  | RustyNES-small.ico            | 16,24,32,48 (one file)                       | ICO    |
| Windows  | png/RustyNES-*.png            | 16,20,24,32,40,48,64,128,256                 | PNG    |
| macOS    | RustyNES.icns                 | 16,32,128,256,512 (+@2x → 1024)              | ICNS   |
| macOS    | RustyNES.iconset/*.png        | 16…1024 (Apple-named)                        | PNG    |
| Linux    | hicolor/*/apps/rustynes.png   | 16,22,24,32,48,64,96,128,256,512             | PNG    |
| Linux    | png/rustynes-*.png            | 16,24,32,48,64,96,128,256,512                | PNG    |
| Linux    | rustynes.png                  | 512                                          | PNG    |
| Web      | favicon.ico                   | 16,32,48 (one file)                          | ICO    |
| Web      | favicon-*, chrome-*, apple-*, mstile | 16,32,96,150,180,192,512              | PNG    |
| Branding | rustynes-banner-*.png         | 900,1200,1500,1800,2172 wide (exact 3:1)     | PNG    |

---

## Regenerating

All rasters are produced by `generate_icons.py` (requires **Pillow**; optionally
**icnsutil** for a fully-complete `.icns` — without it, Pillow writes a slightly
smaller container and the complete `.iconset/` is still emitted for `iconutil`).
A bare run inside the pack reads `source/` and rewrites everything:

```bash
python3 generate_icons.py
# or explicitly:
python3 generate_icons.py \
  --primary source/rustynes_primary_app_icon_1x1.png \
  --favicon source/rustynes_simplified_favicon_icon_1x1.png \
  --banner  source/rustynes_primary_logo_banner_3x1.png \
  --out     .
```

Tunables at the top of the script: `SIMPLIFIED_MAX` (the emblem/primary crossover),
`UNSHARP_MAX`/`UNSHARP_PARAMS` (small-size sharpening), and the per-platform size
lists.
