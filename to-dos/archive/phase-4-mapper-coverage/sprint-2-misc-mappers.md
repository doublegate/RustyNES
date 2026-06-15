# Sprint 4-2 — Misc mappers

**Phase:** Phase 4 — Mapper Coverage
**Sprint goal:** Implement MMC2, MMC4, Color Dreams, CPROM, BNROM/NINA-001, Camerica, VRC1.
**Estimated duration:** 1-2 weeks

**Status:** complete — all banking + latch + bus-conflict logic landed; per-mapper smoke tests via existing unit tests.  Each mapper has its own constructor, CHR/PRG resolution path, and save-state round-trip.

## Tickets

- [x] T-42-001 — MMC2 (Punch-Out): tile-fetch-driven CHR latch ($FD/$FE).  Implemented in `rustynes-mappers::sprint2::Mmc2`.
- [x] T-42-002 — MMC4: like MMC2 with full PRG banking.  `rustynes-mappers::sprint2::Mmc4`.
- [x] T-42-003 — Color Dreams (mapper 11): bank switching + bus conflict.  `rustynes-mappers::sprint2::ColorDreams`.
- [x] T-42-004 — CPROM (mapper 13): Videomation only.  `rustynes-mappers::sprint2::Cprom`.
- [x] T-42-005 — BNROM (mapper 34) + NINA-001 submapper.  `rustynes-mappers::sprint2::M34` + `M34Variant`.
- [x] T-42-006 — Camerica BF9093 (mapper 71): Codemasters titles.  `rustynes-mappers::sprint2::Camerica`.
- [x] T-42-007 — VRC1 (mapper 75).  `rustynes-mappers::sprint2::Vrc1`.
- [x] T-42-008 — Per-mapper smoke test for each.  Inline unit tests cover banking, bus-conflict semantics, and the MMC2 latch.

## Reference docs

- [docs/mappers.md](../../docs/mappers.md) §Mapper coverage matrix
