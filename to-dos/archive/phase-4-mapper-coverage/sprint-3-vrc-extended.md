# Sprint 4-3 — VRC family + Sunsoft FME-7 + Namco 163

**Phase:** Phase 4 — Mapper Coverage
**Sprint goal:** Implement Konami VRC2/4/6 mappers with their CPU-cycle IRQ counters; Sunsoft FME-7 (with 5B audio variant); Namco 163 (with audio).
**Estimated duration:** 2 weeks

**Status:** banking + IRQ landed for all five mappers; **all mapper-extended audio explicitly DEFERRED** behind a future cargo feature.  See `docs/apu-2a03.md` open questions and the per-mapper `mix_audio` no-op stubs.

## Tickets

- [x] T-43-001 — VRC2 (mappers 22 and submapper variants of 21/23/25): basic banking + mirroring.  `rustynes-mappers::sprint3::Vrc2`.
- [x] T-43-002 — VRC4 (mappers 21/23/25 with submappers): IRQ counter (CPU-cycle or scanline-mode).  `rustynes-mappers::sprint3::Vrc4`.  Cycle-mode counter increments per CPU cycle and asserts on wrap from $FF; scanline-mode prescales by ~114 cycles/clock.  Enable-after-ack flag honored.
- [x] T-43-003 — VRC6 (mappers 24 and 26): banking + IRQ.  Audio extension DEFERRED — `mix_audio` returns 0; banking + IRQ alone suffices for many ROMs.
- [ ] T-43-004 — `vrc24test` passes for all VRC2/4 variants.  Test ROM not vendored.  No regression — the existing test suite reports parse-success for mapper IDs 21/22/23/24/25/26.
- [x] T-43-005 — Sunsoft FME-7 (mapper 69): IRQ.  `rustynes-mappers::sprint3::Fme7`.  16-bit CPU-cycle IRQ counter.  5B audio DEFERRED.
- [x] T-43-006 — Namco 163 (mapper 19): IRQ.  `rustynes-mappers::sprint3::Namco163`.  15-bit CPU-cycle IRQ counter with bit-15 enable.  Wavetable audio DEFERRED.
- [ ] T-43-007 — Audio mixing integration.  `Mapper::mix_audio()` trait method already exists (returns 0 by default); per-mapper integration deferred behind a future `mapper-audio` cargo feature.

## Reference docs

- [docs/mappers.md](../../docs/mappers.md)
- [docs/apu-2a03.md](../../docs/apu-2a03.md) §Open questions → mapper-extended audio
