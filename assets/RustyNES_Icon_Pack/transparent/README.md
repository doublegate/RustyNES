# RustyNES Icon Pack — Transparent Variants

Best-effort **transparent-background** versions of the RustyNES logo and icon, for
embedding on light or arbitrary backgrounds (READMEs, websites, slides, overlays)
where the opaque dark art would show as a black box.

> These are **additive** — they do not replace anything in the parent pack. The
> opaque assets remain the correct choice for OS app icons (macOS `.icns` is
> full-bleed; iOS `apple-touch-icon` must be opaque; Android maskable needs an
> opaque safe zone).

## How they were made

`make_transparent.py` does a **corner flood-fill key**: it removes the background
region 4-connected to the image border and within a colour threshold of the corner
colour (the masters have a near-uniform near-black field, `~#000214`), leaving the
emblem and any dark pockets *inside* it intact. Keying is done at full master
resolution and then downscaled with Lanczos, so the alpha edge antialiases on the
way down. Because the near-black field is flat, the result is clean; a faint 1–2 px
glow fringe can remain only at extreme zoom. For pristine output a
transparent-background master render would be needed.

Regenerate / retune:

```bash
python3 make_transparent.py                # defaults (thresh 48, feather 0.6)
python3 make_transparent.py --thresh 56    # remove a touch more of the glow blend
```

## Contents

```text
transparent/
├── icon/
│   ├── rustynes-icon-transparent-master.png   (1254, full-res keyed)
│   ├── rustynes-icon-transparent-<n>.png       (1024,512,256,128,64)
│   └── rustynes-transparent.ico                (16..256, multi-res)
├── favicon/
│   └── rustynes-favicon-transparent-<n>.png    (64,48,32,16; simplified emblem)
└── logo/
    └── rustynes-logo-transparent-<w>x<h>.png   (2172×724, 1200×400, 600×200)
```

Verified by compositing over white and mid-grey — the emblem, circuit traces,
wordmark, and tagline stay crisp with no visible dark halo.
