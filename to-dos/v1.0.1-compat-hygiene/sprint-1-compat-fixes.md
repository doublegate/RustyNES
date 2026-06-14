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
- **TRACE HARNESS BUILT + DIVERGENCE PINPOINTED (2026-06-13).** Added a runtime
  opt-in FDS read-stream trace (`Nes::enable_fds_trace` / `take_fds_trace` →
  `FdsTraceRec`; default-off, observation-only, not serialized → determinism
  intact) + the `fds_trace` diagnostic bin, which drains the `$4031` byte stream +
  `$4025` control writes + side changes and diffs the side-N disk-info block as the
  BIOS read it against the raw `.fds`. Result on Kid Icarus:
  - The BIOS reads side B's disk-info block **correctly through byte 0x15 —
    INCLUDING side#=1 (it matches!).** So the side-B *data read is correct*; ERR.07
    is NOT a corrupted read.
  - At block byte 0x16 the stream shows a SECOND `01 *NINTENDO-HVC*` — the BIOS
    **re-read block 1 from the start**. The `$4025` trace shows the head going
    `0 → 223 → 0`: RustyNES **rewinds the head to 0 on motor-off** (`fds.rs:1384`),
    whereas Mesen2 (`Fds.cpp:279-288`) defers the rewind to `_endOfHead` with a long
    `_delay` and idles on `_resetTransfer && !_scanningDisk` — a real divergence.
  - **Because the side# reads correctly as 1, ERR.07 ("wrong side number") means the
    BIOS is comparing against a DIFFERENT expected side — the boot reset-check's
    expected side 0** (nesdev: at boot the BIOS sets the DiskID side#/disk# to 0).
    That implies **side A's game program never takes control** (the side-A "cycling"
    is the BIOS still in its boot/disk loop), so the swap lands on the boot check,
    not the game's own `LoadFiles(side 1)`. The real defect is therefore in the
    **side-A multi-file load / game handoff** (and/or the motor-off rewind loop),
    not side B per se. NEXT: trace side A's full boot load with `fds_trace` (no
    swap) to find where the side-A load stalls / loops, and A/B the motor-off-rewind
    change (Mesen2-style deferred `_endOfHead` reset + `_delay`) — verifying the 56
    FDS unit tests + the 6 working FDS games stay green. (Still NOT fixed; a
    speculative motor-rewind change was avoided pending that A/B.)
- **SIDE-A TRACE CLOSED OUT (2026-06-13).** Traced side A's full boot load (no
  swap) and contrasted it against the working multi-side Metroid. Side A's reads
  are **correct** — disk-info block matches the raw `.fds`, **0 CRC errors**,
  **0 end-of-head**, side# reads as 0. The divergence is in *completion*:
  - Kid Icarus reads **138560 `$4031` bytes (≈2.6 disk passes)** and never stops.
  - Metroid reads **52054 bytes (< 1 pass)**, then motor-off → game runs.
  Both reach the same disk region and read identical uncorrupted data, but Kid
  Icarus **never decides "load complete"** — it re-scans the file structure forever
  (the cycling licence screen). **So the defect is FDS file-load COMPLETION, not the
  read path / CRC / side# / disk-wrap.** The earlier side-B ERR.07 is a downstream
  symptom (side A's game never takes control → a swap lands on the BIOS boot
  reset-check, which expects side 0).
  **DEFINITIVE NEXT STEP (now narrow):** the BIOS loads boot files whose ID ≤ the
  "boot read file code" in the disk-info block (`FDS_BIOS.md` `LoadFiles`), counted
  via the file-amount block (block 2). Diff the per-file load decision (which IDs
  load + when it stops) vs Mesen2 — most likely RustyNES mis-delivers the
  **block-2 file-count** or a **file-header size/type field**, so the BIOS's
  "loaded vs expected" count never converges. The read engine / CRC / side-switch /
  gap-skip are all verified correct, so the fix is in the block-2 / file-header
  **content-length** path of `fds.rs`, not the timing model.
- **RESOLUTION — NO CONFIRMED BUG; SIDE A WORKS (2026-06-13).** Ran side A long
  enough and captured a late frame: **at ~frame 3500 Kid Icarus is at its
  interactive `ナマエトウロク` (NAME REGISTRATION) screen and fully playable** —
  the katakana/hiragana entry grid responds to input (characters get entered).
  The disk-read stats also plateau (identical at 2000/4000/8000 frames: max head
  52669, 148617 bytes, then no more reads), i.e. **side A's load COMPLETES and the
  game runs.** The earlier "side-A stall / file-load-completion" reading was wrong
  — I was looking at the boot phase before the game appeared (~f60–540).
  - **The side-B ERR.07 was a REPRODUCTION ARTIFACT, not a RustyNES bug.** My
    headless swaps fired at frames 300–950 — during the BIOS boot — long before the
    game requests side B (the request is literally *post-**registration***, i.e.
    after the interactive name-entry screen + gameplay). Swapping during boot makes
    the BIOS run its reset disk-check, which **requires side# = 0**; inserting side
    B (side# = 1) then correctly yields ERR.07 ("wrong side number"). That is
    *expected* behaviour for a mis-timed swap.
  - Reaching the genuine side-B request needs **interactive play**: complete name
    registration (navigate to `トウロク オワル` / END), start the game, and reach the
    in-game "set disk side B" prompt — which random scripted input can't reliably
    drive blind. This **matches `docs/compatibility.md`'s original note that this
    item "needs interactive testing."**
  - **Disposition:** T-101-002 is reclassified from "bug to fix" to **UNCONFIRMED —
    side A verified working; side-B swap needs interactive verification by a human
    playing to the real post-registration prompt.** No FDS code change was made (no
    reproduced defect to fix). The trace facility + `fds_swap_repro` / `fds_trace`
    harnesses remain as permanent tooling for that interactive verification.

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
