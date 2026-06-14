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
- **REPRO + ISOLATION (2026-06-13):** `fds_smoke` (boots side A, pulses Start, NO
  disk swap) over all 8 local FDS games: 6 boot to a visible title (Metroid, both
  Zeldas, Bio Miracle, Otocky, Doki Doki Panic — incl. multi-side titles), but
  **Kid Icarus renders a fully black 1-colour screen at frame 1200** (Akumajou
  Dracula Disk-Writer variant is also suspicious, separate). So the stall is
  Kid-Icarus-specific and happens EARLY (the game needs side B right after the BIOS
  registration screen). **Blocker for root-cause: `fds_smoke` never swaps disk
  sides.** Next step = a small disk-swap reproduction harness using the existing
  API (`Nes::disk_side_count` / `inserted_disk_side` / `set_disk_side(Some/None)`,
  `crates/rustynes-core/src/nes.rs:423-440`): boot side A → advance past BIOS
  registration → eject + insert side B when the game requests it → trace the FDS
  `$4030-$4033` status + the BIOS disk-read state at the stall and diff against
  Mesen2/fceux FDS drive timing. (The fix itself is then likely in the FDS device
  in `crates/rustynes-mappers/src/fds.rs`.)
- **HARNESS BUILT + ROOT-CAUSE LEAD (2026-06-13):** added the
  `fds_swap_repro` diagnostic bin (`crates/rustynes-test-harness/src/bin/fds_swap_repro.rs`,
  `--features test-roms,commercial-roms`) — it scripts an eject→insert-side-N and
  logs a framebuffer-activity + disk-side timeline. Findings on Kid Icarus (2 sides):
  - **No swap (side A only):** the BIOS shows the Nintendo license/registration
    screen and then cycles 1↔2 colours **forever** — it is waiting for side B and
    never errors.
  - **Insert side B:** the BIOS reads side B and displays **`ERR. 07`** (an FDS BIOS
    disk-read/verify error) — it never reaches the game (working FDS titles hit
    9-11 colours; Kid Icarus stays at 1-2).
  - **Conclusion:** Kid Icarus genuinely needs side B, but RustyNES's side-B read
    fails the BIOS check with ERR.07. Suspect: the per-side **wire-image synthesis**
    in `fds.rs` (lead-in/inter-block gaps, the `0x80` start mark, CRC-16/KERMIT, and
    the block structure) for side B specifically — cf. the nesdev "Game Doctor FDS
    Format" note about special BIOS handling "after reading block type 2 of sides B
    and later".
- **ROOT-CAUSE NARROWED — NOT YET FIXED (2026-06-13).** Decoded the FDS BIOS error
  table (`nesdev_wiki/output/FDS_BIOS.md`): **`ERR.07` = "a,b side — Wrong side
  number"** (the `CheckDiskHeader` disk-ID offset-6 side-number compare failed).
  Ruled out, with evidence, every unit-level FDS path:
  - The raw `.fds` is correct — side 0 has side#=0, side 1 has side#=1 (direct byte
    read of the dump).
  - `parse_sides` splits the 131000-byte image into two correct 65500-byte sides;
    `do_set_disk_side(Some(1))` + `rebuild_wire` serve side 1's wire image.
  - Side-1 raw reads work (`insert_side_reads_from_that_side`), AND the **gap-skip
    re-sync after a swap** works — proven by a NEW regression test
    `swap_then_gap_skip_delivers_first_block_byte` (delivers side 1's `0x01` block
    code from head 0, no manual seek; covers a path the suite previously missed).
  - The failure is **structural, not timing** — swap frame 300/450/600/750/950 all
    converge to the same ERR.07 screen.
  So the defect is in a **higher-level real-BIOS side-B read interaction** (a
  multi-block read / IRQ / motor-restart subtlety during a real swap), NOT the data
  / parse / side-switch / raw-read / gap-skip. **A speculative FDS change was
  deliberately avoided** — without a verified root cause it would risk the 56
  passing FDS unit tests + the 6 working FDS games + the AccuracyCoin/oracle gates.
  **DEFINITIVE NEXT STEP:** capture a byte-level trace of the real BIOS reading
  side B (the `$4031` read stream + `$4030`/`$4032` status + the motor /
  transfer-reset writes) and diff it against Mesen2/fceux on the same ROM to find
  the exact block/byte where the read diverges. (fds.rs is `#![no_std]`, so use a
  trace buffer + a feature-gated dump, not `eprintln`.)

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
