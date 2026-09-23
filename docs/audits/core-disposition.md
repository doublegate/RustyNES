# Core audit — disposition ledger

Report: [`core-audit-report.md`](core-audit-report.md). Verdict vocabulary and
the closing procedure: [`README.md`](README.md). Target release per
[ADR 0041](../adr/0041-hardware-release-is-v3.0.0.md).

**Stale facts in the report:** AccuracyCoin is 144/144, not "141/141" (§1.3).
`GEMINI.md` is a symlink to `AGENTS.md`, so the report's citations of it are
citations of `AGENTS.md`.

| Id | Finding (report §) | Verdict | Evidence | Release | PR |
| --- | --- | --- | --- | --- | --- |
| IMP-01 | PPU snapshot `spr_count` unbounded (§2.2) | CONFIRMED | `rustynes-ppu/src/snapshot.rs:676` reads `spr_count` raw; `ppu.rs:3782/3881/4816/5103` loop `0..spr_count` over `[_; 8]` arrays | v2.7.0 | |
| IMP-02 | APU channel state + BlipBuf unchecked (§2.3, §2.4) | CONFIRMED | `rustynes-apu/src/snapshot.rs:282-297` (duty/step), `:325` (triangle step), `:235` (decay), `:401,417` (dac) raw; `read_blip:543-548` accepts non-finite `phase`; `blip.rs:269` loop never terminates on it | v2.7.0 | |
| IMP-03 | `SectionIter` unchecked `body_start + len` (§2.5) | PARTIAL | `rustynes-core/src/save_state.rs:504-505`. `len` is a `u32` widened to `usize`, so it cannot wrap on 64-bit; it can on 32-bit targets (`wasm32` ships) | v2.7.0 | |
| §2.1 | `unsafe_code = "warn"`; add `#![forbid(unsafe_code)]` to the chip crates | UNTRIAGED | | v2.7.0 | |
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
| T-01 | Pulse-1 sweep with negate and shift 0 (the `$4001=$08` idiom) computes `per - per - 1 = 0xFFFF` and mutes | UNTRIAGED (suspected) | `rustynes-apu/src/pulse.rs:160-163,174`; `nesdev_wiki/APU_Sweep.xhtml`: a negative target clamps and negate never mutes. The sibling RTL follows the wiki, so no co-sim gate covers the difference. Needs a red-first unit test before it is believed | v2.7.0 | |
