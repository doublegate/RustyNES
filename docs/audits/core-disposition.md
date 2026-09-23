# Core audit — disposition ledger

Report: [`core-audit-report.md`](core-audit-report.md). Verdict vocabulary and
the closing procedure: [`README.md`](README.md). Target release per
[ADR 0041](../adr/0041-hardware-release-is-v3.0.0.md).

**Stale facts in the report:** AccuracyCoin is 144/144, not "141/141" (§1.3).
`GEMINI.md` is a symlink to `AGENTS.md`, so the report's citations of it are
citations of `AGENTS.md`.

| Id | Finding (report §) | Verdict | Evidence | Release | PR |
| --- | --- | --- | --- | --- | --- |
| IMP-01 | PPU snapshot `spr_count` unbounded (§2.2) | FIXED | `PpuSnapshotError::InvalidSprCount` rejects `> 8`. Red-first: the v2.7.0 fuzz target crashed the unfixed tree at `ppu.rs:5104` ("len is 8 but the index is 8") in 29,326 runs. Test `a_corrupt_sprite_count_is_rejected_not_indexed`; mutation CAUGHT | v2.7.0 | |
| IMP-02 | APU channel state + BlipBuf unchecked (§2.3, §2.4) | FIXED | `ApuSnapshotError::FieldOutOfRange` bounds 12 register-width fields (duty, step, the three sweep fields, envelope volume/divider/decay, triangle step, DMC rate/bit count/DAC); `InvalidResampler` rejects a zero sample rate, a non-finite or non-positive CPU rate, a rate ratio above one output per CPU cycle (a finite huge ratio hangs exactly like an infinite one), a phase outside `[0, 1)`, and non-finite filter/held values. Tests `every_register_width_field_is_bounded_on_restore`, `resampler_fields_that_would_hang_or_poison_audio_are_rejected`; 18 mutations, all CAUGHT | v2.7.0 | |
| IMP-03 | `SectionIter` unchecked `body_start + len` (§2.5) | FIXED | `checked_add`. The overflow is reachable only where `usize` is 32 bits, so no native test can make it fail; the `thumbv7em` build compiles it but does not execute it. Same change fuses the iterator: a malformed section used to return the same error on every `next()`, an infinite iterator for any caller that skips errors. Test `section_iter_stops_after_a_malformed_section`; both fuse sites mutated, CAUGHT | v2.7.0 | |
| §2.1 | `unsafe_code = "warn"`; add `#![forbid(unsafe_code)]` to the chip crates | FIXED | `#![forbid(unsafe_code)]` in `rustynes-{cpu,ppu,apu,mappers,core}/src/lib.rs`; none of the five contained `unsafe`, so this makes an observation a guarantee | v2.7.0 | |
| IMP-04 | Branchless `set_nz` / `adc` / `cmp_with` (§3.2) | UNTRIAGED | Prior evidence bounds the win: `docs/performance.md` measures `status.rs` at 0.7% of the profile | v2.7.5 | |
| IMP-05 | `#[inline]` + cold-path outlining in `Cpu` (§3.3) | UNTRIAGED | Prior evidence against: `docs/performance.md` G7 measured `#[inline]` on the hot bus functions at **+0.60%, a regression**, and records two failed inline-hint attempts | v2.7.5 | |
| IMP-06 | PPU fast-path redundant stores, phase match, branchless palette (§3.4) | UNTRIAGED | | v2.7.5 | |
| IMP-07 | BlipBuf `drain_all` capacity churn + unbounded growth (§3.5) | UNTRIAGED | | v2.7.5 | |
| §3.1 | Scheduler hotspots A/B/C (divider polling, DMA drain store, per-dot NMI edge sampling) | UNTRIAGED | | v2.7.5 | |
| §3.5b | Pulse sweep-mute caching | UNTRIAGED | | v2.7.5 | |
| §3.6 | `cpu_read_unmapped` virtual call on every cart read | UNTRIAGED | | v2.7.5 | |
| §4.2 | `Bus`/`CpuBus` naming drift, `LockstepBus` name | UNTRIAGED | | v2.7.5 | |
| §4.3 | Orphaned `ApuBus` trait | UNTRIAGED | Deprecate in v2.7.5; removal decided at v2.9.0 (ADR 0041 §4) | v2.7.5 | |
| §4.4 | 22 dead methods on `rustynes_cpu::Bus` | UNTRIAGED | As §4.3 | v2.7.5 | |
| §4.5 | CPU open-bus has no decay; Sachen 150/243 `$4101` wipes open-bus high bits | UNTRIAGED | Accuracy change: needs a test ROM or nesdev citation before any fix | v2.7.2 | |
| IMP-12 | `PpuBusAdapter` built per dot in `run_ppu_to` (§4.6) | UNTRIAGED | | v2.7.5 | |
| §4.7 | Doc drift: zapper temporal-light default, FDS error text, CPU header | UNTRIAGED | | v2.7.5 | |
| IMP-08 | Bandai FCG EEPROM not exposed via `sram()` (§5.1) | CONFIRMED | No `fn sram` in `m016_bandai_fcg.rs`; trait default at `mapper.rs:562` returns `&[]` | v2.7.1 | |
| IMP-09 | Taito X1-005 128-byte RAM not exposed (§5.1) | CONFIRMED | No `fn sram` in `m080_taito_x1_005.rs` | v2.7.1 | |
| IMP-10 | TxSROM / TQROM do not forward `sram()` to `inner` (§5.1) | CONFIRMED | No `fn sram` in `m118_txsrom.rs`, `m119_tqrom.rs` | v2.7.1 | |
| §5.1e | Multicart 15 PRG-RAM not exposed | CONFIRMED | No `fn sram` in `multicart_discrete.rs`. Note: the CHANGELOG's "battery saves" entry (v2.6.21) concerned the MiSTer RTL, not these mappers | v2.7.1 | |
| IMP-11 | Namco 163 `$C000-$DFFF` nametable banking (§5.2) | CONFIRMED | `m019_namco163.rs:604-608` "Not wired up here" | v2.7.2 | |
| §5.3 | MMC5 multi-bank PRG-RAM ignored | CONFIRMED | `m005_mmc5.rs:1081` stores `prg_ram_bank`, never read; `:676`/`:985` "v0 supports a single 8 KiB PRG-RAM bank" | v2.7.2 | |
| §5.4 | MMC1 SUROM/SXROM 512 KiB PRG | CONFIRMED | `m001_mmc1.rs:168` `self.prg & 0x0F`; the comment at `:162-164` claims all 5 bits | v2.7.2 | |
| §5.5 | `$6000-$7FFF` returns 0 instead of open bus on discrete boards | UNTRIAGED | | v2.7.2 | |
| §5.6 | `has_hardwired_mirroring` missing on 11+ fixed-mirroring boards | UNTRIAGED | | v2.7.2 | |
| §6.2 | "Sanitize" Mesen2/puNES citations in `m085_vrc7.rs`, `m099_vs_system.rs`, `m244_cne_decathlon.rs` | REJECTED FIX | The comments quote Mesen2 **source expressions** (`m085_vrc7.rs:362,947`; `m099_vs_system.rs:135,160-161`; `m244_cne_decathlon.rs:115`), so they are derivation statements. Removing them would be laundering (AGENTS.md NEVER LAUNDER). **Fix instead:** a `// Provenance:` header on each file, plus `NOTICE` and `docs/originality-and-provenance.md` entries, reviewed by the maintainer before merge | v2.7.1 | |
| §6.3 | Derived-component attributions complete | NOT A DEFECT | Informational. The attributed-file count is a snapshot; regenerate with `grep -rn "^// Provenance:" crates/*/src/*.rs` | — | |

## Found during triage (not in the report)

| Id | Finding | Verdict | Evidence | Release | PR |
| --- | --- | --- | --- | --- | --- |
| T-01 | Pulse-1 sweep with negate and shift 0 (the `$4001=$08` idiom) computes `per - per - 1 = 0xFFFF` and mutes | FIXED | CONFIRMED by `pulse1_negate_shift0_clamps_to_zero_and_does_not_mute`, red on v2.6.23. Fix: a negated target saturates at zero (NESdev "APU Sweep"). `pulse1_negate_clamp_is_inert_for_nonzero_shift` shows the clamp changes nothing for shift 1-7 over periods 8-`$7FF`. Full `--features test-roms` suite: 2,612 passed, 0 failed; no golden moved. The v2.8.2 co-sim gate still follows | v2.7.0 | |
| F-01 | Restored `dma_mc_consumed` trips `Cpu::end_cycle`'s structural-zero `debug_assert_eq!` (not in the report) | FIXED | Found by the v2.7.0 fuzz target in 6,848 runs. Dev/test/fuzz builds only: release drains and discards the value, so it is now discarded on restore (bytes still consumed). Test `a_restored_dma_mc_consumed_is_discarded_not_loaded`; mutation CAUGHT | v2.7.0 | |
| F-02 | Restored OAM-DMA byte index `uni_oam_addr` unbounded (not in the report) | FIXED | Found by the fuzz target in 287,867 runs (`bus.rs:3871`, add overflow). An active transfer at 256 or above never completes and overflows the `u16`. Bounds: `<= 255` while active, `<= 256` otherwise. Test `an_out_of_range_oam_dma_index_is_rejected`; both halves mutated, CAUGHT | v2.7.0 | |
| F-03 | Restored PPU raster position unbounded (not in the report) | FIXED | Found by the fuzz target once its base machine had a live APU, in 76,381 runs (`ppu.rs:6078`, `dot += 1` overflow). The per-dot advance wraps only at dot 340 and the pre-render line, so a position past either never produces a frame. `PpuSnapshotError::InvalidRasterPosition`: dot `<= 340`, scanline `-1..=` pre-render line (-1 is the power-on position, which an existing round-trip test caught when the first draft excluded it). Test `an_out_of_range_raster_position_is_rejected`; 4 mutations, all CAUGHT | v2.7.0 | |
| F-04 | Restored PPU fine X and ten sprite-evaluation / OAM-bus counters unbounded (not in the report) | FIXED | The fuzz target found fine X (`ppu.rs:4788`, `0x8000 >> x` shift overflow) in 92,496 runs; the rest were then swept by reading every field `restore` loads rather than left to the fuzzer one run at a time. `PpuSnapshotError::FieldOutOfRange` bounds `x` 7, sprite-eval `n` 63 / `m` 3 / `found` 8 / `sec_idx` 32, OAM-bus address 63 / 3, secondary address 32, overflow counter 3, `oam2_addr` 31, corruption index 32 -- each the range the running PPU keeps it in. One existing round-trip test set the overflow counter to 5, a marker value the PPU cannot reach (it loads 3 and counts down); changed to 2. Test `every_ppu_counter_and_index_is_bounded_on_restore`; 11 mutations, all CAUGHT | v2.7.0 | |
