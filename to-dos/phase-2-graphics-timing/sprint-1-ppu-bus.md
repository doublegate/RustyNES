# Sprint 2-1 — PPU bus, registers, memory map

**Phase:** Phase 2 — Graphics + Timing
**Sprint goal:** A `Ppu` struct that responds correctly to all 8 CPU-facing registers, with the documented quirks (read-buffer, open-bus, w toggle, OAMADDR rules).
**Estimated duration:** 2 weeks

## Tickets

### T-21-001 — `Ppu` struct, internal state, region-aware constants

**Description:** Define `Ppu` with internal VRAM (2 KB), OAM (256 B), secondary OAM (32 B), palette RAM (32 B), loopy v/t/x/w, status latches, open-bus latch with decay, NMI line. Region-aware: NTSC vs PAL vs Dendy scanline counts and post-reset masking window.

**Acceptance criteria:**
- [x] `Ppu::new(region)` initializes correctly for each region (NTSC / PAL / Dendy).
- [x] `Ppu::reset()` resets the documented subset (PPUCTRL / PPUMASK / `w` toggle / data buffer / NMI line; restarts the post-reset masking window).
- [ ] `Ppu::power_cycle()` zeroes all RAM with the documented power-on pattern. (Power-cycle path lives at the `Nes` facade in Sprint 2-4.)

**Dependencies:** Phase 1 complete.
**Reference:** `docs/ppu-2c02.md` §State.
**Estimated complexity:** M.

---

### T-21-002 — `PpuBus` trait + simple test bus

**Description:** Define the `PpuBus` trait per `docs/ppu-2c02.md` §Interfaces. Implement a test bus that owns nametable VRAM and a CHR-RAM array (8 KB) with horizontal mirroring as default.

**Acceptance criteria:**
- [x] `PpuBus` trait compiles and is documented.
- [x] Test bus implements horizontal/vertical mirroring; unit test for each. (See `crates/rustynes-ppu/src/ppu.rs` `tests::TestBus`.)
- [x] `notify_a12` is a no-op for now (mappers will use it in Phase 4); transition counter test verifies the trait method is invoked on every A12 edge.

**Dependencies:** T-21-001.
**Reference:** `docs/ppu-2c02.md` §Interfaces.
**Estimated complexity:** S.

---

### T-21-003 — PPU register reads ($2002, $2004, $2007)

**Description:** Implement `cpu_read_register`. PPUSTATUS clears VBL + w; PPUDATA buffered read; palette read special case; open-bus reads return the latch value.

**Acceptance criteria:**
- [x] PPUSTATUS bit 7 (VBL) read clears the flag and w toggle.
- [x] PPUDATA returns previous buffer; updates buffer with new address; palette `$3F00-$3FFF` reads bypass buffer but update it with underlying nametable.
- [x] PPUDATA increments `v` by 1 or 32 per PPUCTRL bit 2.
- [x] PPUSTATUS bits 4-0 are open-bus (reflect latch).
- [x] OAMDATA read at `$2004` returns OAM[OAMADDR] without incrementing.
- [x] Unit tests for each.

**Dependencies:** T-21-002.
**Reference:** `docs/ppu-2c02.md` §Register quirks.
**Estimated complexity:** M.

---

### T-21-004 — PPU register writes ($2000, $2001, $2003-$2007)

**Description:** Implement `cpu_write_register`. PPUCTRL updates `t` (nametable bits + NMI enable); PPUMASK; OAMADDR direct write; OAMDATA write incrementing OAMADDR; PPUSCROLL two-write; PPUADDR two-write copying `t` to `v` on second write; PPUDATA write to PPU bus + increment.

**Acceptance criteria:**
- [x] Writes to `$2000`/`$2001`/`$2005`/`$2006` ignored during the post-reset masking window (~29,658 NTSC CPU cycles).
- [x] PPUCTRL NMI bit 0→1 while VBL set asserts NMI immediately.
- [x] PPUSCROLL bit splits per `docs/ppu-2c02.md` §Loopy.
- [x] PPUADDR second write copies `t` to `v`; bit 14 of `t` forced to 0 on first write.
- [x] OAMDATA write during rendering does *not* modify OAM but does perform glitchy OAMADDR increment.
- [x] Open-bus latch updated on every register access.

**Dependencies:** T-21-003.
**Reference:** `docs/ppu-2c02.md` §Register quirks, §Loopy.
**Estimated complexity:** L.

---

### T-21-005 — Open-bus decay model

**Description:** Track per-bit-group decay timers. Implement a coarse 600 ms decay (sufficient for `ppu_open_bus` test).

**Acceptance criteria:**
- [x] Open-bus bits decay to 0 after a configurable interval. (Coarse single-counter model at 1,000,000 CPU cycles, ~600 ms; per-bit-group decay deferred unless `ppu_open_bus` requires it.)
- [x] Resetting the latch (a relevant write) restarts the decay timer for those bits.
- [ ] `ppu_open_bus` test ROM passes. (ROM not yet vendored; gated on Sprint 2-2 wiring of the PPU into the test harness.)

**Dependencies:** T-21-004.
**Reference:** `docs/ppu-2c02.md` §Open-bus.
**Estimated complexity:** M.

---

### T-21-006 — VBL flag set + NMI assertion at scanline 241 dot 1

**Description:** Implement the scanline counter (without rendering yet). On scanline 241 dot 1, set VBL and assert NMI (if PPUCTRL bit 7 set). Clear VBL at scanline 261 dot 1.

**Acceptance criteria:**
- [x] `Ppu::tick()` advances scanline/dot correctly.
- [x] Frame complete signal at end of scanline 240. (Latch fires when the FSM rolls past the pre-render scanline; equivalent observable timing.)
- [x] NMI line asserts at scanline 241 dot 1 (if enabled). Verified by `vbl_set_and_nmi_at_scanline_241_dot_1` unit test.
- [x] `ppu_vbl_nmi/01-vbl_basics.nes` passes. 01-09 all pass; 10-even_odd_timing fails subtest 3 (1 PPU clock skew at the rendering-enabled boundary, which would require splitting `cpu_read`/`cpu_write` into a TetaNES-style mid-cycle bus access to fix).

**Dependencies:** T-21-005.
**Reference:** `docs/ppu-2c02.md` §Frame structure.
**Estimated complexity:** M.

---

## Sprint review checklist

- [x] All tickets checked off (T-21-001 through T-21-006 implementation criteria met; integration test ROMs deferred to Sprint 2-4 lockstep).
- [ ] `ppu_vbl_nmi` sub-tests 01 and 02 pass. *Deferred — ROMs not yet vendored; requires Sprint 2-4 lockstep `Nes` facade.*
- [ ] `ppu_open_bus` passes. *Deferred — same gate.*
- [x] CHANGELOG entry for PPU bus + register interface.
