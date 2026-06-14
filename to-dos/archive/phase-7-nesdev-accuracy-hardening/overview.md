# Phase 7 - Nesdev Accuracy Hardening

**Goal:** close the v1.x hardware-accuracy and documentation gaps identified by
`ref-docs/nesdev-wiki-technical-report.md` and
`docs/nesdev-hardware-emulation-checklist.md`.

**Exit criterion (MET, as-shipped v1.5.0):** all supported stock NES/Famicom
behavior in the checklist is implemented, explicitly out of scope, or guarded by
tests; AccuracyCoin held at 90.65% (additive-only — the master-clock-axis
residuals are deferred to v2.0, NOT closed here); missing Nesdev-indexed test ROM
categories are either vendored with license notes or replaced by equivalent
in-tree fixtures; unsupported platforms have clear compatibility docs.

## Sprint Index

- [Sprint 1 - Source and test corpus closure](sprint-1-source-test-corpus.md)
- [Sprint 2 - CPU, DMA, and internal bus closure](sprint-2-cpu-dma-internal-bus.md)
- [Sprint 3 - PPU residuals and region variants](sprint-3-ppu-region-variants.md)
- [Sprint 4 - Mapper, expansion audio, and platform variants](sprint-4-mappers-expansion-platforms.md)

## Workstreams

### Source Completeness

- Keep `docs/nesdev-hardware-emulation-checklist.md` aligned with Nesdev pages,
  not only with local code.
- Add exact upstream links to subsystem docs when a hardware behavior is fixed
  or deferred.
- Track any Nesdev page whose behavior is intentionally not implemented in
  `docs/compatibility.md`.

### Test Completeness

- Fill test ROM gaps from the Nesdev emulator-test index:
  `cpu_reset`, `instr_misc`, missing input-device coverage, and mapper-specific
  fixtures whose original links have rotted.
- Preserve license proof in `tests/roms/LICENSES.md` before committing any ROM.
- Prefer equivalent homegrown fixtures when upstream licensing or availability
  is unclear.

### Accuracy Completeness (as-shipped re-scope — v1.5.0 was additive-only)

- C1 IRQ-sample-timing axis — documented + **deferred to the v2.0 master-clock
  refactor** (NOT closed in Phase 7).
- Remaining AccuracyCoin CPU/internal-bus, APU/DMA, and PPU residuals — likewise
  **deferred to v2.0** (most fall out of the master-clock refactor for free).
- Validate PAL and Dendy timing with dedicated tests instead of NTSC-derived
  assumptions — **DONE** (per-region constant table + frame-structure test).
- Make power-on randomization a developer option while keeping CI deterministic
  — **DONE** (`Nes::from_rom_with_power_on_seed`; default path unchanged).

### Platform Completeness

- Decide whether v1.x includes FDS, Vs. System, PlayChoice-10, VRC7 FM audio,
  MMC5 audio, expanded input devices, or only clearer unsupported-platform
  diagnostics.

## Dependencies

- Phase 6 release closeout decides which residuals carry into this phase.
- `docs/STATUS.md` remains the source of truth for current pass counts.
- Any new commercial-ROM canary remains user-supplied and gitignored.
