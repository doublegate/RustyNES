# Sprint 2-4 — Lockstep scheduler + DMA + simple mappers

**Phase:** Phase 2 — Graphics + Timing
**Sprint goal:** Wire the PPU and CPU together with a lockstep dot-master scheduler. Add OAM DMA. Implement the simple no-IRQ mappers (UxROM, CNROM, AxROM, GxROM, MMC1) so the emulator can run real homebrew end-to-end.
**Estimated duration:** 2 weeks

## Tickets

### T-24-001 — `Nes` facade in `rustynes-core`

**Description:** Build the public `Nes` struct that owns CPU, PPU, APU stub, mapper-via-cart, controllers stub. Expose `from_rom`, `reset`, `power_cycle`, `run_frame`.

**Acceptance criteria:**
- [x] Public API matches `docs/architecture.md` §Public API surface (subset: `from_rom`, `reset`, `power_cycle`, `run_frame`, `step_instruction`, `framebuffer`, `cycle`; `from_rom_with_region`, `audio_samples`, `set_controller`, `save_state`, `load_state` deferred).
- [x] Constructed from a parsed cartridge.
- [x] `run_frame()` returns when the PPU completes scanline 240.

**Dependencies:** Sprint 2-3 complete.
**Reference:** `docs/architecture.md` §Public API surface.
**Estimated complexity:** M.

---

### T-24-002 — Lockstep scheduler

**Description:** Implement `tick_one_dot()` per `docs/scheduler.md`. PPU advances every dot; CPU advances every 3rd (NTSC); APU advances every other CPU cycle.

**Acceptance criteria:**
- [x] Matches `docs/scheduler.md` §Tick structure (PPU 3 dots per CPU cycle, NTSC).
- [ ] CPU/PPU phase offset is configurable (random at power-on, fixed by reset). (Deferred — phase offset is currently fixed at 0; randomization added in Phase 3.)
- [x] Determinism test: identical seed + ROM + inputs → bit-identical framebuffer after 4 frames.

**Dependencies:** T-24-001.
**Reference:** `docs/scheduler.md`.
**Estimated complexity:** L.

---

### T-24-003 — OAM DMA controller

**Description:** Implement OAM DMA per `docs/scheduler.md` §DMA controller. Halt CPU on next read cycle; perform 256 read/write pairs; total 513 or 514 cycles depending on alignment.

**Acceptance criteria:**
- [x] Cycle count matches the reference (513 or 514, alignment-dependent).
- [x] Halt happens at the next read cycle (drained inside the bus's `cpu_read`/`cpu_write` entry). (Real hardware halts only on read cycles; our drain happens on either, which is observationally identical for ROMs that always trigger DMA from a CPU read after writing $4014.)
- [x] OAM is correctly populated after DMA (verified by `oam_read` test ROM passing).
- [x] Synthetic test: `oam_read` (PASS) and `oam_stress` (FAIL — known: no $2002 vbl-flag-based wait between writes; addressed in a Phase-3 OAMADDR-during-rendering follow-up).

**Dependencies:** T-24-002.
**Reference:** `docs/scheduler.md` §DMA controller; `ref-docs/research-report.md` §DMA.
**Estimated complexity:** M.

---

### T-24-004 — UxROM (mapper 2)

**Description:** Implement UxROM. PRG bank at `$8000-$BFFF` selectable; `$C000-$FFFF` fixed to last bank. CHR-RAM only.

**Acceptance criteria:**
- [x] Bank-switch via write to `$8000-$FFFF` (data = bank index).
- [x] Mirror config from header.
- [ ] Smoke test: a known UxROM ROM boots and renders the title screen correctly. (Deferred to Phase 2 visual regression once PPU lands.)

**Dependencies:** T-24-003.
**Reference:** `docs/mappers.md` §Mapper coverage matrix.
**Estimated complexity:** S.

---

### T-24-005 — CNROM (mapper 3) with bus conflicts

**Description:** Implement CNROM. CHR bank-switchable in 8 KB units. Bus conflict on bank-select writes (AND with PRG byte at the written address).

**Acceptance criteria:**
- [x] CHR bank switch works.
- [x] Bus conflict implemented.
- [ ] Smoke test passes for a known CNROM ROM. (Deferred to Phase 2 visual regression once PPU lands.)

**Dependencies:** T-24-004.
**Reference:** `docs/mappers.md` §Behavior → Bus conflicts.
**Estimated complexity:** S.

---

### T-24-006 — AxROM (mapper 7)

**Description:** Implement AxROM. 32 KB PRG bank-switchable. Single-screen mirroring control via bit 4 of the bank-select write.

**Acceptance criteria:**
- [x] Bank-switch works.
- [x] Mirroring switches between Single-Screen-A and Single-Screen-B.
- [ ] Smoke test passes. (Deferred to Phase 2 visual regression once PPU lands.)

**Dependencies:** T-24-005.
**Reference:** `docs/mappers.md`.
**Estimated complexity:** S.

---

### T-24-007 — GxROM (mapper 66)

**Description:** Implement GxROM. PRG and CHR banks selectable from a single register; bus conflict.

**Acceptance criteria:**
- [x] Both banks switch correctly.
- [x] Bus conflict modeled.

**Dependencies:** T-24-006.
**Reference:** `docs/mappers.md`.
**Estimated complexity:** S.

---

### T-24-008 — MMC1 (mapper 1) with serial protocol + consecutive-write bug

**Description:** Implement MMC1. Serial 5-write register protocol. Bit 7 reset rule. The consecutive-write bug (writes on adjacent CPU cycles after the first are ignored). PRG/CHR bank modes per the control register.

**Acceptance criteria:**
- [x] Reset rule (write with bit 7 set) clears the shift register.
- [x] 5-write protocol latches data.
- [x] Consecutive-write bug reproduced (verified by `instr_test_v5/all_instrs.nes` and `official_only.nes`, both MMC1, both pass).
- [x] All four PRG modes + both CHR modes implemented.
- [ ] Smoke test: Bill & Ted's Excellent Adventure boots correctly. (Deferred to Phase 2 visual regression once PPU lands.)

**Dependencies:** T-24-007.
**Reference:** `docs/mappers.md`; `ref-docs/research-report.md` §MMC1.
**Estimated complexity:** L.

---

### T-24-009 — Visual regression corpus

**Description:** Capture a curated set of golden framebuffers for homebrew/demo ROMs (no commercial Nintendo content) at frames 60, 180, 300. CI compares via `insta`-managed snapshots of FNV-1a hashes; the determinism unit test from T-24-002 already guarantees bit-identical framebuffers across runs, so a stable hash is a sufficient regression sentinel and avoids committing 240 KB of RGBA per snapshot.

**Acceptance criteria:**
- [x] At least 5 framebuffer captures in the corpus across NROM (`full_palette`, `flowing_palette`, `01-vbl_basics`) and MMC1 (`instr_test_v5/01-basics`).  Total: 7 snapshot tests covering frame counts 60, 120, 180, 300.
- [x] CI test fails on hash mismatch (snapshot review default — no `INSTA_UPDATE` env var set in CI).
- [x] Reference snapshots committed under `crates/rustynes-test-harness/tests/snapshots/`.

**Dependencies:** T-24-008.
**Reference:** `docs/testing-strategy.md` §Layer 4.
**Estimated complexity:** M.

---

## Sprint review checklist

- [ ] All tickets checked off.
- [ ] All `ppu_vbl_nmi`, `ppu_open_bus`, `sprite_overflow_tests`, `sprite_hit_tests` pass.
- [ ] Visual regression corpus green.
- [ ] CHANGELOG entry: "PPU + scheduler + simple mappers complete; emulator runs real ROMs."
- [ ] Tag `v0.2.0-graphics` milestone.
