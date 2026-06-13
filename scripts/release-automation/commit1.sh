#!/usr/bin/env bash
set -e
cd /home/parobek/Code/Commercial_Private-Projects/RustyNES_v2
/usr/bin/git commit -q -F - <<'MSG'
fix(ci): commit the used nes-test-roms ROMs + finish accuracy-polish wiring

CI #141-143 were red: the v2.1.0/v2.2.0 test wiring reads ROMs from the
gitignored tests/roms/nes-test-roms/ clone, which is NOT present in CI (the
test-roms job does a plain checkout with no clone step) -> apu_reset.rs's
fs::read panicked. Also the clippy job (run WITHOUT --features test-roms)
tripped clippy::doc_markdown in v21_coverage_mappers.rs (its #![allow] sits
after the #![cfg(test-roms)] so it was stripped when the feature is off).

Fixes:
- De-nest the vendored nes-test-roms clone (it had its own .git, so the parent
  repo could not track files inside it) and force-add ONLY the 73 .nes files the
  CI test suite actually opens (19 suites: apu_reset, blargg_apu_2005.07.30,
  blargg_nes_cpu_test5, blargg_ppu_tests_2005.09.15b, cpu_exec_space, dmc_tests,
  instr_test-v3, nes_instr_test/rom_singles, nmi_sync, oam_read, PaddleTest3,
  pal_apu_tests, ppu_read_buffer, read_joy3, scanline, sprdma_and_dmc_dma,
  vaus-test, vbl_nmi_timing, volume_tests). The other ~190 unused ROMs + the
  whole clone stay gitignored; the env-gated fdsirqtests.fds stays out (.fds +
  no committed BIOS). These are the same public-domain blargg/homebrew test ROMs
  as the already-committed tests/roms/blargg/ corpus.
- Backtick `holy_mapperel` in v21_coverage_mappers.rs so doc_markdown never fires
  regardless of the cfg/allow ordering.

Accuracy-polish wiring (the "pick up the smaller items" work, finished):
- Item 3: wire blargg_nes_cpu_test5 (official strict + cpu visual smoke),
  instr_test-v3 (official_only strict + all_instrs smoke), nes_instr_test
  rom_singles, volume_tests, and PaddleTest3 (Vaus paddle, position-dependent).
- Item 2 (apu_reset len_ctrs_enabled + 4017_written): a $4017-retain reset
  re-arm was prototyped twice, held AccuracyCoin 100%/oracle 60/60 byte-identical
  but did NOT flip the two targets AND regressed the passing 4017_timing ->
  REVERTED + documented as needing the cycle-accurate reset-sequence
  (master-clock) axis, not a frame-granular re-arm.
- Defensive .gitignore: disksys.rom + *.fds (never-distributable).

Verified: full `cargo test --workspace --features test-roms` = 965 passed, 0
failed, 16 ignored (was 937 at v2.2.0; +28 new); CI-exact clippy (no test-roms)
clean. No chip code touched -> AccuracyCoin 100% + oracle 60/60 unchanged.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
MSG
echo "committed: $(/usr/bin/git rev-parse --short HEAD)"
echo "files in commit: $(/usr/bin/git show --stat --oneline HEAD | tail -1)"
