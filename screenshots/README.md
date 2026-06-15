# `screenshots/` — commercial-game visual corpus + showcase

Committed PNG snapshots of the commercial games RustyNES runs, used as a
human-readable compatibility reference and as the README showcase imagery. The
machine-readable regression baselines are the `fnv1a64` hashes in
`crates/nes-test-harness/tests/snapshots/*.snap` — these PNGs are the visual
companion (a 5-second eyeball check instead of a recapture/accept/compare loop).

## Layout

```text
screenshots/
├── README.md            (this file)
├── showcase.png         8-game showcase montage (the README hero image)
├── montage.png          ~28-game showcase montage (NROM → MMC5/FME7/VRC + FDS + Vs RGB)
├── m22/                 VRC2a CHR-banking test-ROM frame
├── mmc1_a12/            MMC1 A12-control test-ROM frame
└── external/            commercial dumps, ONE PNG per game, sorted to mirror
                         tests/roms/external/:
    ├── mapper-NNN-FAMILY/   (per-mapper subdirs — NROM, MMC1, MMC3, MMC5,
    │                         VRC2/4/6/7, FME7, Taito X1, Sunsoft, … 35 mappers)
    ├── fds/                 Famicom Disk System titles (real-BIOS boot)
    ├── vs-system/           Vs. arcade dumps (2C03/2C04 RGB palettes)
    └── pc10/                PlayChoice-10 dumps (2C03 RGB)
```

~170 PNGs across 37 subdirs. The subdir names match `tests/roms/external/` exactly,
so a screenshot maps 1:1 to its ROM directory.

## Known non-rendering frames (expected)

A handful of games can't produce a meaningful frame and are kept at their best
(near-blank) capture, documented in `docs/compatibility.md`:

- The Vs. **DualSystem** titles (Balloon Fight / Tennis / Mahjong / Wrecking Crew VS) —
  two-CPU/two-PPU hardware this single-system core can't boot past attract.
- **Mito Koumon** (mapper 89) — a PPU rendering-enable dependency (deferred axis).

## Regenerating

The corpus is regenerated with the diagnostic `coverage_smoke` (iNES) +
`fds_smoke` (FDS) bins (gated on `--features commercial-roms`; the gitignored ROMs
live under `tests/roms/external/` per the README there), then sorted into the
`mapper-NNN-FAMILY/` + `fds/` + `vs-system/` + `pc10/` layout:

```bash
# iNES (recurses tests/roms/external; RUSTYNES_VS_COIN=1 inserts a coin for Vs.):
RUSTYNES_VS_COIN=1 cargo run -p nes-test-harness --features commercial-roms --release \
    --bin coverage_smoke -- tests/roms/external 1500 /tmp/ss "" 120
# FDS (real BIOS — never committed):
cargo run -p nes-test-harness --features commercial-roms --release \
    --bin fds_smoke -- tests/roms/external/fds/disksys-fcd.rom tests/roms/external/fds 2500 /tmp/ss-fds
# coverage_smoke dumps `<subdir>__<game>.png`; sort by converting `__` -> `/`.
```

The montages are built from the sorted corpus with ImageMagick `montage`.

## Reference cross-links

- Machine-readable baselines: `crates/nes-test-harness/tests/snapshots/*.snap`
- Harness + diagnostic bins: `crates/nes-test-harness/{tests,src/bin}/`
- Compatibility notes (incl. the non-rendering cases): `docs/compatibility.md`
- ROM library buildout audit: `docs/audit/rom-library-buildout-2026-05-17.md`
