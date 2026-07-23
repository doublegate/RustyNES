# `assorted/` — mixed blargg / kevtris test ROMs

An assorted corpus: the blargg and kevtris ROMs that do not belong to any one
structured suite directory. It spans CPU timing and branch timing, OAM read /
stress, CPU reset semantics, the first two APU length-counter ROMs, the
palette renderers, and `nestest` -- so it is grouped by "everything else"
rather than by subsystem, and is kept separate from the per-suite `blargg/`
corpus for that reason.

| File | Suite | Mapper | Author | License |
|------|-------|--------|--------|---------|
| `cpu_timing_test.nes` | CPU timing | NROM (0) | blargg | Public domain |
| `branch_timing_1_basics.nes` | Branch timing | NROM (0) | blargg | Public domain |
| `branch_timing_2_backward.nes` | Branch timing | NROM (0) | blargg | Public domain |
| `branch_timing_3_forward.nes` | Branch timing | NROM (0) | blargg | Public domain |
| `oam_read.nes` | OAM | NROM (0) | blargg | Public domain |
| `oam_stress.nes` | OAM | NROM (0) | blargg | Public domain |
| `cpu_reset_ram_after_reset.nes` | CPU reset | NROM (0) | blargg | Public domain |
| `cpu_reset_registers.nes` | CPU reset | NROM (0) | blargg | Public domain |
| `apu_01_len_ctr.nes` | APU 2005-07-30 | NROM (0) | blargg | Public domain |
| `apu_02_len_table.nes` | APU 2005-07-30 | NROM (0) | blargg | Public domain |
| `full_palette.nes` | Palette | NROM (0) | blargg | Public domain |
| `flowing_palette.nes` | Palette | NROM (0) | blargg | Public domain |
| `nestest.nes` | CPU validation | NROM (0) | kevtris | Public domain |

## Source

All files were copied from `christopherpow/nes-test-roms` (specific
upstream paths are in `tests/roms/LICENSES.md` "blargg's NES test ROMs"
section).

## What they test

- **`cpu_timing_test.nes`** — End-to-end CPU per-instruction cycle
  count, from blargg's `cpu_timing_test6` suite (see `tests/roms/LICENSES.md`).
- **`branch_timing_*`** — Branch taken / not-taken; backward branch
  page-cross; forward branch page-cross. (`blargg/branch_timing_tests/`
  has the same set but in a different sub-ROM layout.)
- **`oam_read.nes`** — OAMDATA (`$2004`) read behaviour, including
  the open-bus + glitch bits on certain alignments.
- **`oam_stress.nes`** — OAM corruption under PPU rendering with
  CPU-side OAMDMA + manual `$2004` writes.
- **`cpu_reset_*`** — RAM and register state after a soft reset.
- **`apu_01_len_ctr.nes` / `apu_02_len_table.nes`** — Length counter
  edge cases (the 2005-07-30 suite predates the more comprehensive
  `apu_test/`).
- **`full_palette.nes` / `flowing_palette.nes`** — Renders all 64
  NES palette entries in a static + animated configuration. Used as
  golden frames for the PPU rendering tests.
- **`nestest.nes`** — A copy of the kevtris CPU validation ROM, used
  for convenience by some sprint-2-era harness paths.

## License

All public domain (per upstream readme files and per the
`nes-test-roms` aggregator README).
