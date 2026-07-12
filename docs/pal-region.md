# PAL / Region

**References:** the authoritative timing lives in [`ppu-2c02.md`](ppu-2c02.md) and [`apu-2a03.md`](apu-2a03.md); pass counts in [`accuracy-ledger.md`](accuracy-ledger.md). This page is a curated handbook entry point.

RustyNES emulates three console regions, selected per-ROM (from the iNES / NES 2.0
header, overridable in the per-game DB) and carried through save-state / reset:

- **NTSC** (2A03 / 2C02) — the North American / Japanese console. The default.
- **PAL** (2A07 / 2C07) — the European console: a different master clock, a
  taller frame, and a distinct APU frame-counter + noise/DMC calibration.
- **Dendy** — a Famiclone hybrid: PAL-style video timing with an NTSC-style APU.

`Region` threads through the core so a subsystem branches on it exactly once at
construction, never per-tick on the shipped default.

## PPU (2C07) timing

The PAL frame is taller: vertical blank runs scanlines 241–310 and pre-render is
scanline 311 (vs. 241–260 / 261 on NTSC), so a PAL frame has 70 vblank
scanlines. The NTSC odd-frame dot-skip does **not** occur on PAL. The `ntsc_phase`
video-phase counter cycles 0..=2 on NTSC and 0..=1 on PAL/Dendy. See
[`ppu-2c02.md`](ppu-2c02.md) for the exact scanline/dot table.

## APU (2A07) timing (v2.1.5 oracle)

PAL has **separate frame-counter step positions** — they are *not* derivable by
scaling the NTSC ones. The PAL 2A07 sequencer step positions are modeled
(v2.1.5), gated on `Region` so only `Region::Pal` takes the PAL arm; NTSC and
Dendy keep the NTSC positions unchanged, so the default build and every NTSC/Dendy
tick stays **byte-identical**. PAL also uses distinct noise periods and DMC rate
tables (`PAL_NOISE_PERIODS`).

This was pinned by the **first PAL-region APU oracle**: blargg's PAL-calibrated
`pal_apu_tests` (10 sub-ROMs, `tests/pal_apu_tests.rs`) passes **10/10** from
v2.1.5, covering the PAL frame-counter-timing checks (clock jitter, mode-0/1
length timing, the two PAL terminal steps) plus a length halt/reload
write-ordering fix that was latent on NTSC too yet stays NTSC-byte-identical. See
[`apu-2a03.md`](apu-2a03.md) § PAL for the step-position values and
[`accuracy-ledger.md`](accuracy-ledger.md) for the ledger.

## Palette

PAL and NTSC differ in the composite-video signal RustyNES models to *generate*
its base palette (`rustynes_ppu::generate_base_palette`, a Bisqwit / ares YIQ
integration) rather than shipping a hand table. See the
[CRT / Composite Video](crt-composite.md) page and [`ppu-2c02.md`](ppu-2c02.md).
