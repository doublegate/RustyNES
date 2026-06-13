# `tests/roms/` — Test ROM Corpora

This directory holds all the test ROMs that ship with RustyNES v2.
Everything here is **committed** to git (the directory and its
subdirectories — `external/` is the exception, see below). Every file
under a committed corpus is under a public-domain / CC0 / MIT / BSD /
zlib / equivalently-permissive license. Full provenance and licensing
is in [`LICENSES.md`](./LICENSES.md).

The integration tests in `crates/nes-test-harness/tests/` consume these
files directly. The full workspace test count (510 + 6 `#[ignore]`'d
expected-fails across 34 suites with `--features test-roms`) is gated
on this corpus.

## Subdirectories

| Path | What it is | Author / source | License |
|------|------------|-----------------|---------|
| [`nestest/`](./nestest/) | The kevtris CPU instruction validation ROM + matching Nintendulator golden log. | kevtris | Public domain |
| [`blargg/`](./blargg/) | Shay Green ("blargg")'s full NES test suites: CPU instr_test-v5, CPU dummy reads/writes, CPU interrupts v2, CPU timing test6, PPU vbl_nmi, PPU open bus, APU test + mixer + DMC DMA, sprite hit / overflow tests, branch timing tests, MMC3 IRQ tests v2, MMC3 IRQ tests (older visual). | blargg | Public domain |
| [`sprint-2/`](./sprint-2/) | Extra blargg sub-suites kept separate by historical convention: branch_timing, cpu_reset, oam_read/stress, apu 2005-07-30 (len_ctr / len_table), full_palette + flowing_palette, a copy of `nestest`. | blargg / kevtris | Public domain |
| [`holy_mapperel/`](./holy_mapperel/) | Damian Yerrick's "Holy Mapperel" cartridge-PCB-assembly tests (mapper-detection + bank-reachability). 17 ROMs covering mappers 0, 1, 2, 3, 4, 7, 9, 10, 34, 66, 69. | Damian Yerrick / tepples | zlib |
| [`mmc5/`](./mmc5/) | MMC5 (mapper 5) accuracy suite from `christopherpow/nes-test-roms`: split-screen, ExRAM modes, scanline IRQ. | Various (aggregator) | Public domain |
| [`accuracycoin/`](./accuracycoin/) | Chris Siebert's 144-test single-NROM AccuracyCoin battery — the **single source of truth** for the v0.9.x → v1.0.0 quality bar. | Chris Siebert (100thCoin) | MIT |
| [`AccuracyCoin/`](./AccuracyCoin/) | The upstream `SOURCE_CATALOG.tsv` (144 test-name catalog parsed by the RAM-direct decoder) plus a copy of `AccuracyCoin.nes` for symmetry. The test catalog is `include_str!`ed by `nes-test-harness::accuracy_coin_catalog`. | Chris Siebert | MIT |
| [`audio-tests/`](./audio-tests/) | Brad Smith (`bbbradsmith`)'s `nes-audio-tests` corpus — expansion-audio relative-loudness comparisons, VRC7 / N163 / FME-7 / MMC5 audio quirks, APU DAC linearity. Covers mappers 5, 19, 24, 26, 69, 85. | Brad Smith | "Freely redistributed and modified for any purpose" (effectively PD) |
| [`m22/`](./m22/) | NewRisingSun's VRC2 (mapper 22) CHR-banking smoke test. | NewRisingSun (aggregated in `christopherpow/nes-test-roms`) | Public domain (aggregator) |
| [`mmc1_a12/`](./mmc1_a12/) | tepples's MMC1 + PPU A12 transition test (control case for the MMC3 A12-IRQ axis). | tepples (aggregated) | Public domain (aggregator) |
| [`external/`](./external/) | **gitignored.** Reserved for end-user-provided commercial dumps. See [`external/README.md`](./external/README.md). | n/a | n/a |

## Mapper coverage matrix (committed ROMs)

This table shows, for each of the 15 mappers RustyNES v2 supports, the
ROMs in this directory that exercise it, the major mapper feature(s)
they cover, and a pointer to the corresponding `external/` mapper
subdirectory for end-user smoke testing against commercial games.

| Mapper | Committed ROM(s) | Features exercised | External counterpart |
|--------|------------------|-------------------|---------------------|
| **0 NROM** | `nestest/nestest.nes`; all `blargg/{cpu,ppu,apu,sprite,dmc,branch}_*` ROMs; `sprint-2/*`; `holy_mapperel/M0_*`; `accuracycoin/AccuracyCoin.nes`; `audio-tests/{db_apu,tri_silence,dac_square}.nes` | CPU instructions, PPU VBL/NMI, APU, branch timing, sprite-zero hit, OAM, full palette | [`external/mapper-000-NROM/`](./external/mapper-000-NROM/) |
| **1 MMC1** | `blargg/instr_test_v5/*` (16 sub-ROMs); `holy_mapperel/M1_P128K_*` (2); `mmc1_a12/mmc1_a12.nes` | Serial-shift register banking, PRG/CHR variants, A12-event smoke test | [`external/mapper-001-MMC1/`](./external/mapper-001-MMC1/) |
| **2 UxROM** | `holy_mapperel/M2_P128K_CR8K_V.nes` | Bank switching with fixed CHR | [`external/mapper-002-UxROM/`](./external/mapper-002-UxROM/) |
| **3 CNROM** | `holy_mapperel/M3_P32K_C32K_H.nes` | CHR-only banking | [`external/mapper-003-CNROM/`](./external/mapper-003-CNROM/) |
| **4 MMC3** | `blargg/mmc3_test_2/*` (6 sub-ROMs); `blargg/mmc3_irq_tests/*` (6 sub-ROMs); `holy_mapperel/M4_*` (3) | A12-triggered IRQ counter, PRG/CHR banking modes, Sharp vs. NEC revision | [`external/mapper-004-MMC3/`](./external/mapper-004-MMC3/) |
| **5 MMC5** | `mmc5/mapper_mmc5test_v1.nes`, `mapper_mmc5test_v2.nes`, `mapper_mmc5exram.nes`; `audio-tests/db_mmc5.nes` | Split-screen, ExRAM modes (00/01/10/11), scanline IRQ counter, raw-PCM audio amplitude | [`external/mapper-005-MMC5/`](./external/mapper-005-MMC5/) |
| **7 AxROM** | `holy_mapperel/M7_P128K_CR8K.nes` | 32 KiB PRG + single-screen mirroring select | [`external/mapper-007-AxROM/`](./external/mapper-007-AxROM/) |
| **9 MMC2** | `holy_mapperel/M9_P128K_C64K.nes` | Latched CHR banks (Punch-Out animation trick) | [`external/mapper-009-MMC2/`](./external/mapper-009-MMC2/) |
| **10 MMC4** | `holy_mapperel/M10_P128K_C64K_W8K.nes`, `M10_P128K_C64K_S8K.nes` | MMC2 with 16 KiB PRG instead of 8 KiB; battery option | [`external/mapper-010-MMC4/`](./external/mapper-010-MMC4/) |
| **19 Namco 163** | `audio-tests/db_n163.nes`, `test_n163_longwave.nes` | 1-8 channel wavetable audio amplitude, long-wave period accuracy | [`external/mapper-019-Namco163/`](./external/mapper-019-Namco163/) |
| **21 VRC4a/c** | (covered via VRC family — `audio-tests/db_vrc6*` exercise sibling Konami silicon; no mapper-21-specific committed ROM) | iNES alias for some VRC4 PCBs | [`external/mapper-021-VRC2-VRC4/`](./external/mapper-021-VRC2-VRC4/) |
| **22 VRC2a** | `m22/0-127.nes` | CHR-bank 0..127 reachability (nybble-swap addressing) | [`external/mapper-022-VRC2/`](./external/mapper-022-VRC2/) |
| **23 VRC2b/VRC4e/f** | (VRC family — no mapper-23-specific committed ROM yet) | iNES alias group | [`external/mapper-023-VRC2-VRC4/`](./external/mapper-023-VRC2-VRC4/) |
| **24 VRC6a (Akumajou pinout)** | `audio-tests/db_vrc6a.nes` | VRC6 audio amplitude (2 pulse + sawtooth) with Akumajou Densetsu CHR pin wiring | [`external/mapper-024-VRC6/`](./external/mapper-024-VRC6/) |
| **25 VRC4b/d (+ VRC2c)** | (VRC family — no mapper-25-specific committed ROM yet) | iNES alias group | [`external/mapper-025-VRC2-VRC4/`](./external/mapper-025-VRC2-VRC4/) |
| **26 VRC6b (Madara pinout)** | `audio-tests/db_vrc6b.nes` | VRC6 audio with Madara CHR pin wiring | [`external/mapper-026-VRC6/`](./external/mapper-026-VRC6/) |
| **66 GxROM** | `holy_mapperel/M66_P64K_C16K_V.nes` | PRG (4×32 KiB) + CHR (4×8 KiB) in one register | [`external/mapper-066-GxROM/`](./external/mapper-066-GxROM/) |
| **69 FME-7 / Sunsoft 5B** | `holy_mapperel/M69_P128K_C64K_W8K.nes`, `M69_P128K_C64K_S8K.nes`; `audio-tests/db_5b.nes`, `clip_5b.nes`, `noise_5b.nes`, `sweep_5b.nes`, `envelope_5b.nes`, `phase_5b.nes` | FME-7 IRQ, PRG/CHR banking; 5B audio envelope / LFSR noise / sweep / phase / clipping | [`external/mapper-069-FME7-Sunsoft5B/`](./external/mapper-069-FME7-Sunsoft5B/) |
| **75 VRC1** | (no mapper-75-specific committed ROM yet — VRC family only) | iNES alias for Konami VRC1 PCB | [`external/mapper-075-VRC1/`](./external/mapper-075-VRC1/) |
| **85 VRC7** | `audio-tests/db_vrc7.nes`, `test_vrc7.nes`, `patch_vrc7.nes`, `clip_vrc7.nes`, `noise_vrc7.nes` | VRC7 OPLL FM register surface; PRG/CHR/IRQ smoke. (Our OPLL synth is deferred per ADR-0004; `mix_audio` returns 0.) | [`external/mapper-085-VRC7/`](./external/mapper-085-VRC7/) |

Mappers not yet covered by a committed ROM (21, 23, 25, 75) have at
least one commercial ROM available via `external/` — those rely on
manual smoke-testing through the `commercial-roms` feature. There is
no permissively-licensed VRC1 test ROM published at the time of
writing; this gap is noted in
[`docs/audit/rom-library-buildout-2026-05-17.md`](../../docs/audit/rom-library-buildout-2026-05-17.md).

## What is NOT committed

- Anything copyrighted by Nintendo or any third-party publisher.
- Anything with an unclear license (we err on the side of "drop it" —
  ROMs investigated and rejected for license clarity are logged in the
  audit report referenced above).
- The `external/` subdirectory itself. Verify with
  `git check-ignore tests/roms/external/anything` — it returns the
  matched gitignore line.

## How to run the tests

```bash
# Stable, fast set (no test-rom ROMs):
cargo test --workspace

# Full suite (this corpus):
cargo test --workspace --features test-roms

# Single corpus:
cargo test -p nes-test-harness --features test-roms blargg
cargo test -p nes-test-harness --features test-roms mmc3
cargo test -p nes-test-harness --features test-roms accuracy_coin
cargo test -p nes-test-harness --features test-roms,commercial-roms external_real_games
```

The `commercial-roms` feature is gated separately precisely because it
depends on `external/` ROMs that are not in the git tree.

## See also

- [`LICENSES.md`](./LICENSES.md) — per-file provenance + license citation.
- [`external/README.md`](./external/README.md) — commercial-ROM directory layout + mapper coverage.
- `docs/STATUS.md` (workspace root) — single source of truth for total
  test count + AccuracyCoin pass rate + mapper coverage matrix.
- `docs/testing-strategy.md` — the six testing layers (unit, suite,
  golden, fuzz, bench, commercial smoke).
