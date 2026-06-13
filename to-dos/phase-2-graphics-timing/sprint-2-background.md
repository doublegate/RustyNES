# Sprint 2-2 — Background rendering + scrolling

**Phase:** Phase 2 — Graphics + Timing
**Sprint goal:** Per-dot background rendering with the loopy v/t/x/w scroll model. Mid-scanline register writes work. Pixel output written to the framebuffer.
**Estimated duration:** 2 weeks

## Tickets

### T-22-001 — 8-dot tile fetch state machine

**Description:** Implement the per-dot fetch sequencer for dots 1-256: NT byte → AT byte → PT low → PT high (each 2 dots). Load shift registers on dot 9, 17, ..., 257.

**Acceptance criteria:**
- [x] Tile fetch addresses computed correctly from `v` (NT, AT, PT-low, PT-high address formulas in `Ppu::fetch_*`).
- [x] Shift register reload at every 8th dot from latched fetch bytes.
- [x] Verified end-to-end: 22 BG/sprite-driven test ROMs pass via `rustynes-test-harness`.

**Dependencies:** Sprint 2-1 complete.
**Reference:** `docs/ppu-2c02.md` §Per-dot fetch sequencing.
**Estimated complexity:** L.

---

### T-22-002 — Coarse X / coarse Y / fine Y increments on `v`

**Description:** Implement the `v` register increment rules: coarse X at dots 8, 16, ..., 256, 328, 336; coarse Y wrap (29→0 with nametable Y flip); horizontal `v` reload from `t` at dot 257; vertical `v` reload from `t` at dots 280-304 of pre-render.

**Acceptance criteria:**
- [x] All increments match `docs/ppu-2c02.md` §Loopy: coarse-X every 8 dots, vert-V at dot 256 (29->0 wrap with nametable Y flip), hori-V copy at dot 257, vert-V copy at pre-render dots 280-304.
- [ ] Property test: random scroll values; assert end-of-frame `v` matches a hand-rolled reference. (Deferred — covered indirectly by `sprite_hit_tests` passing.)

**Dependencies:** T-22-001.
**Reference:** `docs/ppu-2c02.md` §Loopy.
**Estimated complexity:** M.

---

### T-22-003 — Background pixel emission

**Description:** Each visible dot, sample the BG shift registers + attribute shift registers + fine X to produce a 4-bit palette index. Look up palette RAM. Apply greyscale (PPUMASK bit 0) and emphasis (bits 7-5). Write RGBA to framebuffer.

**Acceptance criteria:**
- [x] Framebuffer correctly populated (verified by `sprite_hit_tests` corpus — sprite-zero hit detection requires correct BG pixels).
- [x] Greyscale + emphasis render correctly (palette helper `apply_emphasis`; greyscale handled at palette read level).
- [x] PPUMASK bits 1 (left-column BG) and 3 (BG enable) honored in `emit_pixel`.
- [x] Output is RGBA8 at 256×240 (FRAMEBUFFER_LEN constant).

**Dependencies:** T-22-002.
**Reference:** `docs/ppu-2c02.md` §Behavior, §Greyscale + emphasis.
**Estimated complexity:** L.

---

### T-22-004 — NES color palette (2C02 reference)

**Description:** Embed the NES color palette as a `[u32; 64]` (the canonical reference palette; emulators typically use Bisqwit's NTSC-derived palette or the Mesen-default). Provide a way to swap palettes later.

**Acceptance criteria:**
- [x] Palette compiled in as `NES_PALETTE: [[u8; 3]; 64]` in `rustynes-ppu/src/palette.rs` (FBX Smooth source).
- [x] Pixel emission uses the palette correctly (via `nes_color_to_rgba` + `apply_emphasis`).
- [ ] Visual diff against a known reference for a static color test ROM matches. (Deferred — Checkpoint 7 visual regression corpus.)

**Dependencies:** T-22-003.
**Reference:** `ref-docs/research-report.md` §NTSC composite filter.
**Estimated complexity:** S.

---

### T-22-005 — Mid-scanline scroll write support

**Description:** Verify that PPUSCROLL/PPUADDR writes mid-scanline correctly affect subsequent fetches. This is what enables status bars on top + scrolling playfield below.

**Acceptance criteria:**
- [ ] Visual diff for a synthetic test ROM that changes scroll mid-screen matches the Mesen2 reference.
- [ ] No regressions in existing PPU tests.

**Dependencies:** T-22-004.
**Reference:** `docs/ppu-2c02.md` §Edge cases item 1.
**Estimated complexity:** M.

---

## Sprint review checklist

- [ ] All tickets checked off.
- [ ] Background rendering produces correct output for a curated set of NROM demo ROMs.
- [ ] CHANGELOG entry: "PPU background rendering complete."
