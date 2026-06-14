# Sprint 1 - Source And Test Corpus Closure

**Goal:** make the local docs and test corpus match the Nesdev emulator-test and
hardware-reference coverage map.

## Tickets

- [x] **T-71-001 - Nesdev source map audit.** For each source cluster in
  `docs/nesdev-hardware-emulation-checklist.md`, verify that the matching
  subsystem doc links the local report and the exact upstream Nesdev page.
  Update stale or missing links.
- [x] **T-71-002 - Vendored `cpu_reset` coverage.** Locate a redistributable
  `cpu_reset` ROM or recreate an equivalent fixture. Cover CPU reset register
  state, stack pointer decrement, and RAM preservation. Document license in
  `tests/roms/LICENSES.md`.
- [x] **T-71-003 - Vendored `instr_misc` coverage.** Add `instr_misc` if
  licensing permits, or write equivalent focused ROM/unit fixtures for
  wraparound, dummy reads, and instruction edge cases.
- [x] **T-71-004 - Input-device test plan.** Identify permissive fixtures or
  create local tests for standard controller DMC conflict, Four Score, Zapper
  latch/read behavior, and NES 2.0 default-device metadata.
- [x] **T-71-005 - VRC24 fixture replacement.** The original `vrc24test` link is
  unavailable. Find a redistributable mirror or create an equivalent mapper
  register/wiring fixture for VRC2/VRC4 variants.
- [x] **T-71-006 - PAL/Dendy validation inventory.** Identify timing ROMs and
  screenshot/audio references that can validate PAL and Dendy behavior without
  reusing NTSC expectations.

## Exit Checklist

- [x] Every newly vendored ROM has a license entry. (`instr_misc`, `instr_timing` added to `tests/roms/LICENSES.md`.)
- [x] `docs/testing-strategy.md` names each missing category as closed or replaced.
- [x] `docs/STATUS.md` includes any new suites and pass counts.

**Sprint 1 outcome (v1.5.0, 2026-05-24):** +14 strict tests (instr_misc 5,
instr_timing 2, cpu_reset power-on 1, controller port-1 + re-strobe 2, VRC2/4
fixture 4) + 2 documented `#[ignore]` (cpu_reset interactive protocol).
Workspace `--features test-roms`: **650 strict + 10 ignored**, AccuracyCoin
RAM-direct 90.65% preserved. All gauntlet gates green.
