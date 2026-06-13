# `blargg/` — Shay Green's NES Test Suites

The comprehensive blargg ("Shay Green") NES test ROM collection. All
ROMs are public-domain releases by the author, redistributed via the
`christopherpow/nes-test-roms` aggregator.

## Subdirectories

Each subdirectory matches an upstream suite name from
`christopherpow/nes-test-roms`.

| Subdir | Suite | Mapper | Sub-ROMs | What it tests |
|--------|-------|--------|----------|---------------|
| `instr_test_v5/` | CPU instruction behaviour v5 | MMC1 (1) | 16 sub + 2 wholes | All official + unofficial 6502 opcodes; flag effects; addressing modes |
| `cpu_dummy_reads/` | CPU dummy reads | NROM (0) | 1 | Phantom reads on RMW + indexed instructions |
| `cpu_dummy_writes/` | CPU dummy writes | NROM (0) | 2 | Phantom writes to OAM and PPU memory during indexed stores |
| `cpu_timing_test6/` | CPU timing | NROM (0) | 1 | Per-instruction cycle count |
| `branch_timing_tests/` | Branch timing | NROM (0) | 3 | Branch taken / not-taken + page-cross extra cycle |
| `cpu_interrupts_v2/` | CPU interrupt timing | NROM (0) | 5 | IRQ / NMI / BRK interactions, IRQ + DMA, branch delays |
| `ppu_vbl_nmi/` | PPU VBL / NMI timing | NROM (0) | 10 | VBL flag set/clear cycle-precise; NMI delay; PPUSTATUS read clearing VBL |
| `ppu_open_bus/` | PPU open bus | NROM (0) | 1 | PPU register read open-bus decay behaviour |
| `apu_test/` | APU register I/O + DMC | NROM (0) | 8 | Length counter, jitter, IRQ flag, DMC basics + rates |
| `apu_mixer/` | APU non-linear mixer | NROM (0) | 4 | Square / triangle / noise / DMC channel summing |
| `dmc_dma_during_read4/` | DMC DMA + register read bug | NROM (0) | 5 | DMA timing during `$2007` / `$4016` reads |
| `sprite_hit_tests/` | Sprite-zero hit | NROM (0) | 11 | Hit basics, alignment, edges, timing |
| `sprite_overflow_tests/` | Sprite overflow | NROM (0) | 5 | Overflow flag set / clear, evaluation order |
| `mmc3_test_2/` | MMC3 modern ($6000 protocol) | MMC3 (4) | 6 sub-ROMs | Clocking, details, A12 clocking, scanline timing, MMC3 revisions |
| `mmc3_irq_tests/` | MMC3 IRQ counter (older visual protocol) | MMC3 (4) | 6 sub-ROMs | Counter clocking, IRQ assert / clear, Sharp vs NEC reload semantics |

## Status protocol

All blargg ROMs (except the visual-protocol `mmc3_irq_tests`) use the
`$6000` write-result protocol:

- `$6000 = 0x80` while running.
- `$6000 = 0x81 .. 0xFF` while running (low byte of step).
- `$6000 = 0x00` if all tests pass.
- `$6000 = error-code` (non-zero, non-`0x80`-`0xFF`) on failure.

The integration tests at
`crates/nes-test-harness/tests/blargg_*.rs` drive each sub-ROM through
this protocol and assert on the final status byte. See
`docs/STATUS.md` for the current per-suite pass rate (`mmc3_test_2/4`
sub-test #3 is the v0.9.0 expected-fail residual; the 4 open
`cpu_interrupts_v2` sub-ROMs are the related cross-cycle physics
residuals — both gated on the v1.0.0 coordinated IRQ-timing rework
per ADR-0002).

## License

Public domain. blargg released each suite explicitly for emulator
validation purposes; the upstream `readme.txt` files (preserved in
each subdirectory) carry no usage restrictions.

## See also

- `tests/roms/LICENSES.md` — full provenance + per-file license table.
- `crates/nes-test-harness/tests/` — the integration tests that
  consume these ROMs.
- `docs/STATUS.md` — current pass / fail count per suite.
