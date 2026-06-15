# Sprint 4-1 — MMC3

**Phase:** Phase 4 — Mapper Coverage
**Sprint goal:** Implement MMC3 with cycle-accurate PPU-A12-driven IRQ counter and Sharp/NEC revision distinction.
**Estimated duration:** 2 weeks

## Tickets

### T-41-001 — MMC3 register layout + bank switching

**Description:** Implement `$8000-$E001` even/odd register pairs. PRG bank modes (mode 0/1 swap fixed window). CHR bank modes (2KB+1KB layout swap). Mirroring control via `$A000`. PRG-RAM enable + protect via `$A001`.

**Acceptance criteria:**

- [x] All 8 R0-R7 registers writable.
- [x] PRG mode 0/1 produce the correct bank arrangement.
- [x] CHR mode 0/1 produce the correct bank arrangement.
- [x] Mirroring switches between H/V via bit 0 of `$A000`.

**Reference:** `docs/mappers.md`; `ref-docs/research-report.md` §MMC3.
**Estimated complexity:** L.

---

### T-41-002 — PPU A12 edge-detection filter

**Description:** Implement the "3 falling edges of M2" filter. PPU calls `Mapper::notify_a12(level)` on every transition; mapper internally tracks the M2-cycles-since-last-fall and counts a rising edge only when the gap is ≥ 3.

**Acceptance criteria:**

- [x] Filter behavior matches the documented spec.
- [x] Test: synthetic A12 toggle pattern produces the expected count (`a12_filter_rejects_close_rising_edges`).
- [x] Standard pattern-table layout (BG=$0000, sprites=$1000) produces exactly 1 count per scanline (PPU `observe_a12_addr` calls in BG/sprite fetch helpers).  Validated end-to-end by `mmc3_test_2/2-details` "Counter should be clocked 241 times in PPU frame", which passes after the PPU now (a) emits A12 transitions for all 8 sprite tile fetches per scanline including dummy fetches for unused slots, (b) runs sprite fetches on the pre-render scanline as well as visible scanlines, and (c) no longer spuriously emits A12 transitions from `inc_hori_v` / `inc_vert_v` (those are internal loopy increments, not address-bus drivers).  Per-frame guard: `rustynes-ppu::tests::a12_rising_edges_match_241_per_ntsc_frame_standard_layout`.

**Reference:** `docs/mappers.md`; `ref-docs/research-report.md` §MMC3 → IRQ counter mechanism.
**Estimated complexity:** L.

---

### T-41-003 — IRQ counter + assert + Sharp/NEC distinction

**Description:** On each filtered A12 rising edge: if counter == 0 or reload flag set, reload; else decrement. If post-action counter == 0 and IRQs enabled, assert IRQ. Sharp variant: also assert if counter was reloaded to 0; NEC: do not.

**Acceptance criteria:**

- [x] Counter behavior matches the spec.
- [x] Sharp/NEC distinction surfaced via `Mmc3Revision` enum (Sharp/Nec).
- [x] Default revision Sharp; NES 2.0 submapper 1 overrides to NEC.
- [ ] Star Trek: 25th Anniversary boots (untested — no commercial ROM).  Confirmed via `mmc3_test_2/5-MMC3.nes` (the rev-A acceptance ROM) which PASSES.

**Reference:** `docs/mappers.md`; `ref-docs/research-report.md` §MMC3 → Hardware revisions.
**Estimated complexity:** M.

---

### T-41-004 — `mmc3_test_2` and `mmc3_irq_tests` pass

**Description:** Iterate on the MMC3 IRQ implementation until both test suites pass.

**Acceptance criteria:**

- [ ] All 5 `mmc3_test_2` sub-ROMs pass.  **Status: 4/6 after the PPU A12 sprite-fetch correction.**  PASS: 1-clocking, 2-details, 3-A12_clocking, 5-MMC3 (the rev-A acceptance ROM).  FAIL: 4-scanline_timing (#2, "Scanline 0 IRQ should occur later when $2000=$08" — finer-grained timing precision residual), 6-MMC3_alt (the rev-B acceptance ROM, mutually exclusive with 5-MMC3 on a Sharp default).  2-details flipped to PASS once the PPU stopped (a) only fetching `0..spr_count` sprite tiles (real hardware always does 8, including dummy fetches for unused slots — those are what produce the per-scanline A12 rise on lines with no visible sprites), (b) skipping sprite tile fetch on the pre-render scanline (real hardware does it for scanline 0's sprites, contributing the 241st rise), and (c) emitting spurious A12 transitions from `inc_hori_v` / `inc_vert_v` (loopy register increments — internal, not address-bus driving).
- [ ] All 6 `mmc3_irq_tests` sub-ROMs pass.  **Status: untestable from the harness — these older ROMs use a visual-only protocol and don't write to $6000.  Run-but-don't-assert hooks added.**

**Reference:** `docs/testing-strategy.md` §Layer 3.
**Estimated complexity:** L.

---

### T-41-005 — Reversed pattern-table layout edge case

**Description:** When BG=$1000 and sprites=$0000, the IRQ decrement happens at PPU cycle 324 of the previous scanline (causing Wario's Woods flicker if mishandled). Verify behavior.

**Acceptance criteria:**

- [ ] Reversed layout test ROM (if available in nes-test-roms) passes.
- [ ] Manual check: a synthetic ROM that reverses pattern tables produces the expected IRQ timing.

**Reference:** `docs/mappers.md`; `ref-docs/research-report.md` §MMC3 → Timing details.
**Estimated complexity:** M.

---

## Sprint review checklist

- [ ] All tickets checked off.
- [ ] At least 5 known MMC3 ROMs boot and play correctly (smoke test).
- [ ] CHANGELOG entry: "MMC3 mapper complete with cycle-accurate IRQ."
