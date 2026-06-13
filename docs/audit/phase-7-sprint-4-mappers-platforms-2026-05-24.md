# Phase 7 Sprint 4 — Mappers, expansion audio, and platform variants

**Date:** 2026-05-24 (v1.5.0)
**Scope:** decide and document the v1.x cartridge/platform scope beyond stock
mapper behavior; add the one missing submapper fixture. Additive only —
AccuracyCoin 90.65%, oracle / sacred trio / B4 byte-identical.

## Ticket disposition

### T-74-001 — NES 2.0 submapper audit — **DONE**

The revision-sensitive supported mappers were audited; coverage is now complete:

| Mapper family | Revision behavior | Coverage |
|---|---|---|
| MMC3 (4) | Sharp rev A default; NEC rev B via NES 2.0 submapper | `mmc3.rs` Sharp/NEC reload-pending discriminator tests; `mmc3_test_2/6-MMC3_alt` `#[ignore]` by-design |
| VRC2/VRC4 (21/22/23/25) | a0/a1 pin rewiring per board + sub-variant | `sprint3.rs::vrc24_a_bits_per_board_pin_rewiring` + VRC2 banking/mirroring/CHR tests (Sprint 1 T-71-005) |
| BNROM / NINA-001 (34) | submapper 1 = NINA-001 (distinct $7FFD/E/F layout) | `sprint2.rs::m34_bnrom_swap` + **new** `m34_nina001_variant_register_layout` |
| Bus-conflict boards (3/7/11/66/…) | mapper sees CPU value AND PRG byte | `gxrom_bus_conflict_at_offset_zero`, `color_dreams_bus_conflict`, `camerica_bank_swap` |

The only gap was a NINA-001 register-layout test (BNROM was tested, NINA-001 was
not); added this sprint.

### T-74-002 — MMC5 deferred features — **DONE (decision); MMC5 is feature-complete for v1.x**

Re-audit corrects a stale `docs/STATUS.md` mapper-row claim ("audio deferred"):
**MMC5 audio is implemented** — `crates/nes-mappers/src/mmc5.rs::Mmc5Audio` (2
pulse channels + raw PCM), gated on the `mapper-audio` feature (default ON),
landed in Track C2 / Phase 2.3. Multi-bank PRG-RAM banking via `$5113`
(`prg_ram_bank`) is also implemented. **Decision: no further MMC5 work in
v1.x** — the remaining MMC5 corners (PRG-RAM > 8 KiB multi-chip configs) are
exercised by no test ROM in the corpus and have no user-demand signal; tracked
in the long-tail policy (T-74-007). The stale STATUS.md row is corrected.

### T-74-003 — VRC7 FM audio decision — **DONE (resolved in v1.1.0)**

The ADR-0004 deferral is closed: VRC7 OPLL FM audio shipped in **v1.1.0** as a
clean-room pure-Rust port of `emu2413 v1.5.9` (MIT) in
`crates/nes-apu/src/opll.rs`; ADR 0006 supersedes ADR 0004; *Lagrange Point*
plays with in-game audio. No action needed.

### T-74-004 — FDS platform plan — **DONE (scope/plan documented); implementation DEFERRED to v2.0**

FDS (Famicom Disk System) is **not** implemented in v1.x. The scope and plan are
documented in `docs/compatibility.md` (FDS section) and the v2.0 release plan
(Sprint B): `.fds` disk-image parser, `disksys.rom` BIOS (user-supplied, never
committed — Nintendo copyright), motor/transfer timing + disk IRQ, writable-disk
persistence, and FDS audio (single 64-step wavetable + LFO mod). Mesen2
`Core/NES/Mappers/FDS/` is the structural-only (GPL-3.0) reference; nesdev wiki
`FDS*.md` pages are the primary source. Deferred to v2.0 as its own platform
initiative.

### T-74-005 — Expanded input devices — **DONE (decision); standard pad only in v1.x**

The standard controller is implemented and tested (both ports, Sprint 1
T-71-004). **Decision: Four Score, Zapper, Famicom expansion-port devices, the
microphone, and DMC-DMA controller-bit corruption are deferred** — no
permissively-licensed test fixture exists for them in the corpus, and the NES
2.0 default-expansion-device field (header byte 15) is parsed but not yet routed
to a device model. Documented in `docs/compatibility.md`. Implementing Four
Score (the highest-value addition) is a clean v1.x follow-up when a test fixture
or user request appears.

### T-74-006 — Vs. System / PlayChoice-10 decision — **DONE (out of scope)**

Documented in `docs/compatibility.md` (PPU variant scoping, Sprint 3 T-73-007 +
this sprint): the RGB-PPU arcade variants (2C03/04/05) remain load-time
diagnostics, not implemented features — a separate platform initiative.

### T-74-007 — Long-tail mapper policy — **DONE (doc)**

Acceptance policy for pirate / multicart / homebrew-only mappers documented in
`docs/compatibility.md`: a mapper is accepted when (a) there is concrete user
demand or a notable title that needs it, AND (b) a redistributable test fixture
or a well-specified nesdev page exists, AND (c) it carries NES 2.0 metadata for
unambiguous detection, weighed against maintenance cost. Absent those, it stays
out of scope.

## Exit-checklist status

- `docs/compatibility.md` reflects accepted/rejected platform scope (FDS plan,
  input devices, Vs/PC10, long-tail policy).
- Mapper coverage matrix: no new *supported* mappers this sprint (the audit
  confirmed existing coverage); the NINA-001 submapper variant is now
  test-guarded and the MMC5-audio status row is corrected.
- Expansion-audio behavior: VRC6 / Sunsoft 5B / Namco 163 / MMC5 / **VRC7 FM**
  tested; FDS audio explicitly deferred to v2.0.
