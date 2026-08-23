# The permitted sources for the 2C02, 2A03 and top-six mappers — a source map, not a summary

**Dated supplemental reference, 2026-08-23.** `ref-docs/` is immutable.

## Why this is a map and not a summary

Under the provenance firewall (ADR 0037), the RTL for the PPU, APU and mappers may
be written **only** from public documentation. This file exists to make that
concrete: it names the exact page, locally present, for each behaviour the RTL has
to implement — so that "written from documentation" is a checkable claim rather
than an assertion.

**It deliberately does not restate the hardware behaviour.** A paraphrase here
would become a third source that drifts from both the wiki and
`docs/ppu-2c02.md`, and this project has already published a false claim assembled
from two true statements nobody re-read together. Read the cited page.

**All paths are relative to the repository root.** The corpus is 3,407 files;
these are the ones that matter for v2.5.1 → v2.7.0.

---

## Rung 3 — the 2C02

| Behaviour | Primary source | Project cross-reference |
|---|---|---|
| Per-dot rendering pipeline, the 341×262 grid | `nesdev_wiki/PPU_rendering.xhtml` | `docs/ppu-2c02.md` |
| `v`/`t`/`x`/`w`, the `$2005`/`$2006` sequence, mid-frame scroll | `nesdev_wiki/PPU_scrolling.xhtml` | `docs/ppu-2c02.md` |
| **Sprite evaluation, per-dot OAM access** | `nesdev_wiki/PPU_sprite_evaluation.xhtml` | `docs/ppu-2c02.md` |
| Register side effects, the `$2002` read race | `nesdev_wiki/PPU_registers.xhtml` | `docs/ppu-2c02.md` |
| Nametable layout and mirroring | `nesdev_wiki/PPU_nametables.xhtml` | `docs/mappers.md` |
| Pattern-table addressing | `nesdev_wiki/PPU_pattern_tables.xhtml` | — |
| Palette RAM, mirrors, backdrop override | `nesdev_wiki/PPU_palettes.xhtml` | `docs/ppu-2c02.md` |
| VBlank/NMI timing, **odd-frame skip** | `nesdev_wiki/PPU_frame_timing.xhtml` | `docs/scheduler.md` |
| Power-on state | `nesdev_wiki/PPU_power_up_state.xhtml` | `docs/ppu-2c02.md` |

**Sprite evaluation is the hardest single item in the programme**, and it is where
the firewall is under most pressure — see the risk table in the plan. Its gate is
`index_framebuffer` plus the sprite-0/overflow ROMs, **never** `ppu-state-trace`,
which encodes RustyNES's FSM rather than hardware.

## Rung 4 — the 2A03

| Behaviour | Primary source |
|---|---|
| Register map | `nesdev_wiki/APU_registers.xhtml` |
| Frame counter, its IRQ, the 4/5-step sequence | `nesdev_wiki/APU_Frame_Counter.xhtml` |
| Pulse channels | `nesdev_wiki/APU_Pulse.xhtml`, `nesdev_wiki/APU_Sweep.xhtml`, `nesdev_wiki/APU_Envelope.xhtml` |
| Triangle | `nesdev_wiki/APU_Triangle.xhtml` |
| Noise, the LFSR | `nesdev_wiki/APU_Noise.xhtml` |
| DMC, and its **DMA stealing back into the CPU** | `nesdev_wiki/APU_DMC.xhtml` |
| Length counters | `nesdev_wiki/APU_Length_Counter.xhtml` |
| Period tables | `nesdev_wiki/APU_period_table.xhtml` |
| Status/`$4015` | `nesdev_wiki/APU_Status.xhtml` |
| Mixing | `nesdev_wiki/APU_Mixer.xhtml` |

Project cross-reference throughout: `docs/apu-2a03.md`.

**The mixer pages are reference only, not a gate.** Rung 4 compares the integer
channel levels in `MixRecord`; RustyNES's non-linear mixer and BLEP resampler are
software artifacts with no hardware counterpart, and gating on the mixed `f32`
would either force the RTL to reproduce them or produce permanent unresolvable
false failures.

## Rung 7 — the top six mappers

| Board | Source | Notes |
|---|---|---|
| NROM | `nesdev_wiki/NROM.xhtml` | 327 Kb, fits on-chip; no SDRAM needed |
| MMC1 | `nesdev_wiki/MMC1.xhtml`, `nesdev_wiki/MMC1_pinout.xhtml` | Serial shift register; the WRAM write-protect layers RustyNES closed in v2.2.3 |
| UxROM | `nesdev_wiki/UxROM.xhtml` | Simple PRG banking |
| CNROM | `nesdev_wiki/CNROM.xhtml` | Simple CHR banking |
| AxROM | `nesdev_wiki/AxROM.xhtml` | PRG banking + one-screen mirroring |
| **MMC3** | `nesdev_wiki/MMC3.xhtml` | **A12 filtering and the IRQ counter — the one with substance** |

MMC3's A12 behaviour is the item most likely to need iteration. RustyNES's own
implementation and the shared MMC3-clone timing oracle are in `docs/mappers.md`;
the test ROMs are `tests/roms/mmc3_test_2/` and `tests/roms/mmc1_a12/`.

---

## Independent oracles available per rung

The plan's risk 6 is *"the oracle can be wrong"* — 141/141 on AccuracyCoin is not
"matches silicon". Every rung is therefore labelled by whether it has a source of
truth **independent of RustyNES**:

| Rung | Independent oracle | Where |
|---|---|---|
| 1–2 (6502) | nestest, 0-diff against **Nintendulator** | `tests/roms/nestest/` |
| 2 (interrupts) | **`cpu_interrupts_v2`** — but it uses the APU frame IRQ, so it only becomes runnable at rung 4 | `tests/roms/blargg/cpu_interrupts_v2/` |
| 3 (PPU) | blargg PPU timing and sprite ROMs | `tests/roms/blargg/` |
| 4 (APU) | `apu_test`, `apu_mixer`, `dmc_dma_during_read4` | `tests/roms/blargg/` |
| 5 (system) | AccuracyCoin | `tests/roms/accuracycoin/` |
| 7 (mappers) | `holy_mapperel`, `mmc3_test_2`, `mmc1_a12` | `tests/roms/` |

**Worth noting for v2.5.1:** `cpu_interrupts_v2` is a genuine third-party oracle
for interrupt behaviour, and it does not depend on the ADR 0038 injection API. It
does depend on the APU frame IRQ, so it cannot run until rung 4 — which means the
interrupt work is verified twice, by different means, at two different points in
the line. That is worth more than either alone.
