# v1.0.1 · Sprint 1 — Compatibility fixes

Game-specific compatibility gaps documented in `docs/compatibility.md`. **Method:**
pin a failing expectation first (capture-frame harness), then fix; never commit
commercial ROMs (they live in the gitignored `tests/roms/external/`).

## T-101-001 — Mito Koumon (mapper 89 / Sunsoft-2) background rendering  ✅ FIXED (2026-06-13)

**FIXED.** Root cause: the mapper-89 register decode had **bit 7 and bit 3 swapped**.
RustyNES used bit 7 for the one-screen mirroring select and bit 3 for the CHR-bank
high bit; the correct layout (nesdev `INES_Mapper_089` + Mesen2 `Sunsoft89`) is
`CPPP MCCC` — **bit 7 = CHR high bit (A16)**, **bit 3 = mirroring**. The wrong
mirroring made the game's background (written to single-screen A) display from the
empty single-screen B, so the BG looked empty while sprites rendered. Fix in
`crates/rustynes-mappers/src/sunsoft2.rs::cpu_write` (+ doc comment + the 2 unit
tests that had codified the swap). Re-captured frame 250 now shows the correct
"© 1987 SUNSOFT / GAME START / CONTINUE" title screen. Isolated to mapper 89 (the
only m89 game is Mito Koumon) — AccuracyCoin (NROM) + all other oracle games
unaffected; m89 is not in the automated oracle, so no snapshot re-baseline needed.
Follow-up: regenerate the visual reference `screenshots/external/mapper-089-Sunsoft2/…png`
(currently shows the pre-fix broken BG) and consider adding Mito Koumon to
`external_extended` as a correct-output regression guard.

### (original investigation notes)

- **Symptom:** boots and executes (CPU/RAM live, sprites briefly render), but the
  BG nametable stays empty and the picture degrades to the backdrop colour after
  ~400 frames.
- **Diagnosis (from v1.0.0 notes):** a PPU **rendering-enable / setup** dependency,
  NOT a banking bug — mapper 89 banking is spec-correct.
- **Plan:** capture frames via `crates/rustynes-test-harness/src/bin/capture_left_edge.rs`
  pattern; trace PPUMASK/PPUCTRL writes + rendering-enable timing for this title vs a
  known-good Sunsoft-2 title; compare against `ref-proj/Mesen2` and `ref-proj/nestopia`
  mapper-89 + PPU paths.
- **Done when:** BG renders correctly for the full session; a pinned screenshot/
  framebuffer regression is added; oracle stays byte-identical for all other games.
- **REPRO CONFIRMED (2026-06-13):** captured frames 60/250/480 via
  `capture_left_edge` on `Tenka no Goikenban - Mito Koumon (Japan).nes`. The
  **background layer is entirely empty (uniform backdrop) from boot** — sprites
  render fine, the BG nametable never appears. Consistent across all three frames
  (so the "degrades after ~400 frames" note understates it: BG is empty from
  frame 60). Next: trace PPUMASK/PPUCTRL writes + the m89 CHR bank value +
  nametable[0] contents at ~frame 60, and diff against Mesen2's m89 + PPU path to
  find whether BG-enable is unset, the CHR bank points at a blank bank, or the
  nametable fetch is wrong. (m89 packs CHR-high-bit/PRG/1-screen-mirror/CHR-low
  into one register — verify the decode, but the v1.0.0 note diagnosed this as a
  PPU rendering-enable/setup axis, not banking.)

## T-101-002 — FDS Kid Icarus side-B post-registration stall

- **Symptom:** Kid Icarus stalls at the side-B post-registration step; device/drive
  core are otherwise functional (real-BIOS boot works on other FDS titles).
- **Plan:** verify the `$4031` completion-signal path interactively; check the
  disk-swap / multi-side eject-insert sequencing in the FDS device
  (`crates/rustynes-mappers/` FDS). Cross-check `ref-proj/Mesen2` + `ref-proj/fceux`
  FDS drive timing.
- **Done when:** side-B advances; a smoke test (or documented manual verification)
  is recorded; no regression to other FDS titles.

## T-101-003 — GxROM-66 + SMB3 "Mario flashing" (un-reproduced reports)

- **Symptom:** two un-reproduced reports (GxROM mapper 66 + SMB3 sprite flashing).
- **Plan:** first attempt a **headless repro** with the capture harness. If
  reproducible, root-cause + fix + pin a regression. If not reproducible after a
  bounded effort, **document as closed/not-a-bug** in `docs/compatibility.md`.
- **Done when:** fixed-with-regression OR documented-closed with the repro attempt recorded.

## T-101-004 — Dependency-advisory hygiene

- Accept any fresh Dependabot bumps raised against the v1.0.0 tree (the old PRs were
  obsolete and closed). `deny.toml` already allows the v1.0.0 license set and ignores
  the transitive `paste` advisory (RUSTSEC-2024-0436).
- **Done when:** `cargo audit` + `cargo deny check` clean; CI Security workflow green.
