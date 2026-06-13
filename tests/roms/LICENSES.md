# Test ROM provenance and licensing

All ROMs vendored under `tests/roms/` are public-domain works released
specifically for the purpose of validating NES emulator implementations.
No commercial Nintendo software is bundled. End users who want to test
against commercial dumps they own should drop them in
`tests/roms/external/` (gitignored).

The corpus below was sourced from
`https://github.com/christopherpow/nes-test-roms` (commit at clone time
on the build host). Each individual ROM has its own author and license
statement; aggregator status of the GitHub repository does not change
those underlying terms.

## blargg's NES test ROMs (Shay Green, `gblargg@gmail.com`)

Shay Green ("blargg") released his NES test ROM suites into the public
domain. The accompanying `readme.txt` for each suite identifies him as
the author and contains no usage restrictions. The suites are widely
mirrored and redistributed; their public-domain status is documented
e.g. on the NESdev wiki (`https://www.nesdev.org/wiki/Emulator_tests`).

The following ROMs are vendored:

| File | Suite | Source path in nes-test-roms | Mapper | Author | License |
|------|-------|------------------------------|--------|--------|---------|
| `sprint-2/cpu_timing_test.nes` | CPU timing | `cpu_timing_test6/cpu_timing_test.nes` | NROM (0) | blargg | Public domain |
| `sprint-2/branch_timing_1_basics.nes` | Branch timing | `branch_timing_tests/1.Branch_Basics.nes` | NROM (0) | blargg | Public domain |
| `sprint-2/branch_timing_2_backward.nes` | Branch timing | `branch_timing_tests/2.Backward_Branch.nes` | NROM (0) | blargg | Public domain |
| `sprint-2/branch_timing_3_forward.nes` | Branch timing | `branch_timing_tests/3.Forward_Branch.nes` | NROM (0) | blargg | Public domain |
| `sprint-2/oam_read.nes` | OAM | `oam_read/oam_read.nes` | NROM (0) | blargg | Public domain |
| `sprint-2/oam_stress.nes` | OAM | `oam_stress/oam_stress.nes` | NROM (0) | blargg | Public domain |
| `sprint-2/cpu_reset_ram_after_reset.nes` | CPU reset | `cpu_reset/ram_after_reset.nes` | NROM (0) | blargg | Public domain |
| `sprint-2/cpu_reset_registers.nes` | CPU reset | `cpu_reset/registers.nes` | NROM (0) | blargg | Public domain |
| `sprint-2/apu_01_len_ctr.nes` | APU 2005-07-30 | `blargg_apu_2005.07.30/01.len_ctr.nes` | NROM (0) | blargg | Public domain |
| `sprint-2/apu_02_len_table.nes` | APU 2005-07-30 | `blargg_apu_2005.07.30/02.len_table.nes` | NROM (0) | blargg | Public domain |
| `blargg/instr_test_v5/01-basics.nes` ... `16-special.nes` | instr_test-v5 sub-ROMs | `instr_test-v5/rom_singles/*.nes` | MMC1 (1) | blargg | Public domain |
| `blargg/instr_test_v5/all_instrs.nes` | instr_test-v5 single | `instr_test-v5/all_instrs.nes` | MMC1 (1) | blargg | Public domain |
| `blargg/instr_test_v5/official_only.nes` | instr_test-v5 single | `instr_test-v5/official_only.nes` | MMC1 (1) | blargg | Public domain |
| `blargg/instr_misc/instr_misc.nes` + `01-abs_x_wrap`..`04-dummy_reads_apu` | instr_misc (aggregate + 4 sub-ROMs) | `instr_misc/{instr_misc.nes,rom_singles/*.nes}` | MMC1 (1) | blargg | Public domain (T-71-003) |
| `blargg/instr_timing/instr_timing.nes` + `1-instr_timing`/`2-branch_timing` | instr_timing (aggregate + 2 sub-ROMs) | `instr_timing/{instr_timing.nes,rom_singles/*.nes}` | MMC1 (1) | blargg | Public domain (T-71-003) |
| `blargg/branch_timing_tests/*.nes` | Branch timing | `branch_timing_tests/*.nes` | NROM (0) | blargg | Public domain |
| `blargg/cpu_timing_test6/cpu_timing_test.nes` | CPU timing | `cpu_timing_test6/cpu_timing_test.nes` | NROM (0) | blargg | Public domain |
| `blargg/ppu_vbl_nmi/01-vbl_basics.nes` ... `10-even_odd_timing.nes` | PPU VBL/NMI timing (10 sub-ROMs) | `ppu_vbl_nmi/rom_singles/*.nes` | NROM (0) | blargg | Public domain |
| `blargg/ppu_open_bus/ppu_open_bus.nes` | PPU open bus | `ppu_open_bus/ppu_open_bus.nes` | NROM (0) | blargg | Public domain |
| `blargg/cpu_dummy_reads/cpu_dummy_reads.nes` | CPU dummy reads | `cpu_dummy_reads/cpu_dummy_reads.nes` | NROM (0) | blargg | Public domain |
| `blargg/cpu_dummy_writes/cpu_dummy_writes_oam.nes` | CPU dummy writes (OAM) | `cpu_dummy_writes/cpu_dummy_writes_oam.nes` | NROM (0) | blargg | Public domain |
| `blargg/cpu_dummy_writes/cpu_dummy_writes_ppumem.nes` | CPU dummy writes (PPU memory) | `cpu_dummy_writes/cpu_dummy_writes_ppumem.nes` | NROM (0) | blargg | Public domain |
| `blargg/sprite_overflow_tests/1.Basics.nes` ... `5.Emulator.nes` | Sprite overflow (5 sub-ROMs) | `sprite_overflow_tests/*.nes` | NROM (0) | blargg | Public domain |
| `blargg/sprite_hit_tests/01.basics.nes` ... `11.edge_timing.nes` | Sprite-zero hit (11 sub-ROMs) | `sprite_hit_tests_2005.10.05/*.nes` | NROM (0) | blargg | Public domain |
| `blargg/apu_test/1-len_ctr.nes` ... `8-dmc_rates.nes` | APU register I/O + DMC (8 sub-ROMs) | `apu_test/rom_singles/*.nes` | NROM (0) | blargg | Public domain |
| `blargg/apu_mixer/{square,triangle,noise,dmc}.nes` | APU non-linear mixer (4 sub-ROMs) | `apu_mixer/*.nes` | NROM (0) | blargg | Public domain |
| `blargg/dmc_dma_during_read4/{dma_2007_read,dma_2007_write,dma_4016_read,double_2007_read,read_write_2007}.nes` | DMC DMA + register-readout bug (5 sub-ROMs) | `dmc_dma_during_read4/*.nes` | NROM (0) | blargg | Public domain |
| `blargg/cpu_interrupts_v2/1-cli_latency.nes` ... `5-branch_delays_irq.nes` | CPU interrupt timing (5 sub-ROMs) | `cpu_interrupts_v2/rom_singles/*.nes` | NROM (0) | blargg | Public domain |
| `blargg/mmc3_test_2/1-clocking.nes` ... `6-MMC3_alt.nes` | MMC3 IRQ + banking validation (6 sub-ROMs; modern $6000 protocol) | `mmc3_test_2/rom_singles/*.nes` | MMC3 (4) | blargg | Public domain |
| `blargg/mmc3_irq_tests/1.Clocking.nes` ... `6.MMC3_rev_B.nes` | MMC3 IRQ counter (6 sub-ROMs; older visual-only protocol) | `mmc3_irq_tests/*.nes` | MMC3 (4) | blargg / kevtris | Public domain |

## kevtris

| File | Source | Mapper | Author | License |
|------|--------|--------|--------|---------|
| `nestest/nestest.nes` | `other/nestest.nes` (kevtris, 2004) | NROM (0) | kevtris | Public domain |
| `nestest/nestest.log` | `other/nestest.txt` (Nintendulator-generated) | n/a | kevtris / Nintendulator | Public domain |

`nestest` was written by kevtris specifically for emulator validation and
released into the public domain together with the matching
Nintendulator-generated golden log. See
`https://www.qmtpro.com/~nes/misc/nestest.txt` for the canonical source.

## MMC5 mapper test ROMs

| File | Source | Mapper | Author | License |
|------|--------|--------|--------|---------|
| `mmc5/mapper_mmc5test_v1.nes` | `mmc5test/mmc5test_v1.nes` | MMC5 (5) | Various (`christopherpow/nes-test-roms` aggregator) | Public domain (per upstream README) |
| `mmc5/mapper_mmc5test_v2.nes` | `mmc5test/mmc5test_v2.nes` | MMC5 (5) | Various | Public domain |
| `mmc5/mapper_mmc5exram.nes` | `mmc5test/mmc5exram.nes` | MMC5 (5) | Various | Public domain |

These suites exercise MMC5 bank switching, ExRAM modes (00 nametable / 01
ExGrafix attributes / 10 general RAM / 11 read-only), PRG banking modes,
CHR banking, and the scanline IRQ counter. They are aggregated in the
`mmc5test` directory of `nes-test-roms`, whose root README places the
collection under public-domain terms.

## Holy Mapperel (Damian Yerrick / Tepples)

| File | Source | Mapper | Author | License |
|------|--------|--------|--------|---------|
| `holy_mapperel/M0_P32K_CR8K_V.nes` | `holy-mapperel-bin-0.02.7z/testroms/M0_P32K_CR8K_V.nes` | NROM (0) | Damian Yerrick | zlib |
| `holy_mapperel/M0_P32K_CR32K_V.nes` | as above | NROM (0) | Damian Yerrick | zlib |
| `holy_mapperel/M1_P128K_CR8K.nes` | as above | MMC1 (1) | Damian Yerrick | zlib |
| `holy_mapperel/M1_P128K_C32K.nes` | as above | MMC1 (1) | Damian Yerrick | zlib |
| `holy_mapperel/M2_P128K_CR8K_V.nes` | as above | UxROM (2) | Damian Yerrick | zlib |
| `holy_mapperel/M3_P32K_C32K_H.nes` | as above | CNROM (3) | Damian Yerrick | zlib |
| `holy_mapperel/M4_P128K_CR8K.nes` | as above | MMC3 (4) | Damian Yerrick | zlib |
| `holy_mapperel/M4_P128K_CR32K.nes` | as above | MMC3 (4) | Damian Yerrick | zlib |
| `holy_mapperel/M4_P256K_C256K.nes` | as above | MMC3 (4) | Damian Yerrick | zlib |
| `holy_mapperel/M7_P128K_CR8K.nes` | as above | AxROM (7) | Damian Yerrick | zlib |
| `holy_mapperel/M9_P128K_C64K.nes` | as above | MMC2 (9) | Damian Yerrick | zlib |
| `holy_mapperel/M10_P128K_C64K_W8K.nes` | as above | MMC4 (10) | Damian Yerrick | zlib |
| `holy_mapperel/M10_P128K_C64K_S8K.nes` | as above | MMC4 (10) | Damian Yerrick | zlib |
| `holy_mapperel/M34_P128K_CR8K_H.nes` | as above | M34 (34) | Damian Yerrick | zlib |
| `holy_mapperel/M66_P64K_C16K_V.nes` | as above | GxROM (66) | Damian Yerrick | zlib |
| `holy_mapperel/M69_P128K_C64K_W8K.nes` | as above | FME-7 (69) | Damian Yerrick | zlib |
| `holy_mapperel/M69_P128K_C64K_S8K.nes` | as above | FME-7 (69) | Damian Yerrick | zlib |
| `holy_mapperel/README.md` | as above | n/a | Damian Yerrick | zlib |
| `holy_mapperel/CHANGES.txt` | as above | n/a | Damian Yerrick | zlib |

Source URL: <https://github.com/pinobatch/holy-mapperel> (release v0.02,
2018-09-29). README's "Legal" section states "Copyright 2013-2017
Damian Yerrick / Available under zlib License." The zlib license is
permissive: redistribution requires only that the origin not be
misrepresented and that altered source versions be clearly marked.

These ROMs are cartridge-PCB-assembly tests that detect the mapper via
mirroring tests, then size PRG/CHR and exercise bank reachability.
Output is **visual** (on-screen text + Morse-coded audio beeps), not
the blargg `$6000` status protocol — so the integration tests in
`crates/nes-test-harness/tests/holy_mapperel.rs` are smoke gates.

We exclude `M28*`, `M78.3*`, `M118*`, `M180*` because the project does
not implement those mappers (per `docs/STATUS.md` §"Mapper coverage").

## AccuracyCoin (100thCoin / Chris Siebert)

| File | Source | Mapper | Author | License |
|------|--------|--------|--------|---------|
| `accuracycoin/AccuracyCoin.nes` | `AccuracyCoin.nes` at repo root | NROM (0) | Chris Siebert (100thCoin) | MIT |
| `accuracycoin/LICENSE` | as above | n/a | Chris Siebert | MIT |
| `AccuracyCoin/sub-tests/controller-strobing.nes` | derived from upstream `AccuracyCoin.asm` (suite 13 / test 7 — `TEST_ControllerStrobing`) | NROM (0) | derivative of Chris Siebert (100thCoin) | MIT (inherits upstream) |
| `AccuracyCoin/sub-tests/implied-dummy-reads.nes` | derived from `AccuracyCoin.asm` (suite 19 / test 1 — `TEST_ImpliedDummyRead`) | NROM (0) | derivative of Chris Siebert | MIT (inherits) |
| `AccuracyCoin/sub-tests/frame-counter-irq.nes` | derived from `AccuracyCoin.asm` (suite 13 / test 2 — `TEST_FrameCounterIRQ`) | NROM (0) | derivative of Chris Siebert | MIT (inherits) |
| `AccuracyCoin/sub-tests/apu-reg-activation.nes` | derived from `AccuracyCoin.asm` (suite 13 / test 6 — `TEST_APURegActivation`) | NROM (0) | derivative of Chris Siebert | MIT (inherits) |

The four sub-test ROMs under `AccuracyCoin/sub-tests/` are derivative
works produced by patching the upstream `AccuracyCoin.asm` source to
jump directly into a single target test at boot (bypassing both the
menu-screen and the full-battery loop). They are built by
`scripts/accuracycoin-build/build_sub_test_rom.py` and inherit the
upstream MIT license. The patch logic is documented in the build
script; the patched `AutomaticallyRunEveryTestInROM` routine is
streamlined to "set Y=suite_idx, X=test_idx, JSR RunTest, halt" and the
boot path's `InfiniteLoop` spin is redirected to enter that wrapper
immediately. Each sub-test ROM reaches its target test by frame ~30 on
RustyNES (verified via `crates/nes-test-harness/src/bin/
validate_sub_test_rom.rs`), unblocking the Session-22 Mesen2 wall-time
oracle blocker for the v1.0.0-final Phase 3 / Phase 4 work.

Source URL: <https://github.com/100thCoin/AccuracyCoin> (main branch,
fetched 2026-05-10). Upstream `LICENSE` file is the MIT License
("Copyright (c) 2025 Chris Siebert"); the full text is vendored
alongside the .nes file.

AccuracyCoin is a single-NROM-cartridge battery of ~139 NES accuracy
tests. The ROM is **interactive** — pass/fail results are reported
visually (on-screen "PASS"/"FAIL" + hex error codes) and the user
navigates the test menu with D-Pad + A + Start. There is no `$6000`
status protocol, so the integration test in
`crates/nes-test-harness/tests/accuracycoin.rs` is a boot-without-crash
smoke gate only. v1.0.0 will need a pixel-decoding harness to extract
the pass rate (currently un-measured; the ≥ 90% bar is documented in
`docs/STATUS.md` §"Version policy").

## "full palette" ROMs

| File | Source | Mapper | Author | License |
|------|--------|--------|--------|---------|
| `sprint-2/full_palette.nes` | `full_palette/full_palette.nes` | NROM (0) | blargg | Public domain |
| `sprint-2/flowing_palette.nes` | `full_palette/flowing_palette.nes` | NROM (0) | blargg | Public domain |
| `sprint-2/nestest.nes` | `other/nestest.nes` | NROM (0) | kevtris | Public domain |

## Expansion-audio tests (Brad Smith / `bbbradsmith`)

Source URL: <https://github.com/bbbradsmith/nes-audio-tests> (master
branch, commit at clone time on the build host). Upstream `readme.md`
"License" section states:

> These files may be freely redistributed and modified for any purpose.
> Credit to the original author and/or a link to the original source
> would be appreciated, but is not required.

This is effectively a public-domain dedication; the credit request is a
courtesy, not a redistribution restriction. The upstream `readme.md` is
vendored alongside the .nes files as `audio-tests/UPSTREAM_README.md`.

These ROMs exercise expansion-audio chip behaviour that the blargg /
kevtris suites do not cover: full-volume relative-loudness comparisons
(`db_*` family), VRC7 patch / clip / filter-noise tests, N163 long-wave
period accuracy, 5B (Sunsoft FME-7) envelope / phase / sweep / clip
behaviour, and APU triangle silence / DAC linearity edge cases.

| File | Source | Mapper | Author | License |
|------|--------|--------|--------|---------|
| `audio-tests/db_apu.nes` | `build/db_apu.nes` | NROM (0) | bbbradsmith | Permissive (effectively PD) |
| `audio-tests/db_vrc6a.nes` | `build/db_vrc6a.nes` | VRC6a (24) | bbbradsmith | Permissive |
| `audio-tests/db_vrc6b.nes` | `build/db_vrc6b.nes` | VRC6b (26) | bbbradsmith | Permissive |
| `audio-tests/db_vrc7.nes` | `build/db_vrc7.nes` | VRC7 (85) | bbbradsmith | Permissive |
| `audio-tests/db_n163.nes` | `build/db_n163.nes` | Namco 163 (19) | bbbradsmith | Permissive |
| `audio-tests/db_5b.nes` | `build/db_5b.nes` | FME-7 / 5B (69) | bbbradsmith | Permissive |
| `audio-tests/db_mmc5.nes` | `build/db_mmc5.nes` | MMC5 (5) | bbbradsmith | Permissive |
| `audio-tests/test_vrc7.nes` | `build/test_vrc7.nes` | VRC7 (85) | bbbradsmith | Permissive |
| `audio-tests/test_n163_longwave.nes` | `build/test_n163_longwave.nes` | Namco 163 (19) | bbbradsmith | Permissive |
| `audio-tests/patch_vrc7.nes` | `build/patch_vrc7.nes` | VRC7 (85) | bbbradsmith | Permissive |
| `audio-tests/clip_vrc7.nes` | `build/clip_vrc7.nes` | VRC7 (85) | bbbradsmith | Permissive |
| `audio-tests/noise_vrc7.nes` | `build/noise_vrc7.nes` | VRC7 (85) | bbbradsmith | Permissive |
| `audio-tests/clip_5b.nes` | `build/clip_5b.nes` | FME-7 / 5B (69) | bbbradsmith | Permissive |
| `audio-tests/noise_5b.nes` | `build/noise_5b.nes` | FME-7 / 5B (69) | bbbradsmith | Permissive |
| `audio-tests/sweep_5b.nes` | `build/sweep_5b.nes` | FME-7 / 5B (69) | bbbradsmith | Permissive |
| `audio-tests/envelope_5b.nes` | `build/envelope_5b.nes` | FME-7 / 5B (69) | bbbradsmith | Permissive |
| `audio-tests/phase_5b.nes` | `build/phase_5b.nes` | FME-7 / 5B (69) | bbbradsmith | Permissive |
| `audio-tests/tri_silence.nes` | `build/tri_silence.nes` | NROM (0) | bbbradsmith | Permissive |
| `audio-tests/dac_square.nes` | `build/dac_square.nes` | NROM (0) | bbbradsmith | Permissive |
| `audio-tests/UPSTREAM_README.md` | `readme.md` | n/a | bbbradsmith | Permissive |

## VRC2 mapper-22 CHR-banking test

| File | Source | Mapper | Author | License |
|------|--------|--------|--------|---------|
| `m22/0-127.nes` | `m22chrbankingtest/0-127.nes` | VRC2a (22) | NewRisingSun (aggregated in `christopherpow/nes-test-roms`) | Public domain (aggregator) |

This is a CHR-banking smoke test for mapper 22 — verifies all 128
4 KiB CHR banks are reachable. Sourced from the
`m22chrbankingtest/` directory of `christopherpow/nes-test-roms`. The
aggregator's root README places the corpus under public-domain terms.

## MMC1 A12 test

| File | Source | Mapper | Author | License |
|------|--------|--------|--------|---------|
| `mmc1_a12/mmc1_a12.nes` | `MMC1_A12/mmc1_a12.nes` | MMC1 (1) | tepples (aggregated in `christopherpow/nes-test-roms`) | Public domain (aggregator) |

Verifies MMC1's behaviour with respect to PPU A12 transitions — a
quirk that affects mappers (like MMC3) that depend on A12 for IRQ
counter clocking. The MMC1 path is the control case (no A12-based
IRQ). Sourced from `MMC1_A12/` in `nes-test-roms`.

## Notes

- The `tests/roms/external/` directory is gitignored and reserved for
  end users who want to plug in commercial dumps they own. Nothing
  copyrighted by Nintendo or any third-party publisher is committed
  here.
- If a ROM's licensing is found to be unclear after inclusion, file an
  issue and the ROM will be removed in the next commit.
