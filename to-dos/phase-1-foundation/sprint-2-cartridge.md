# Sprint 2 — Cartridge parser (iNES + NES 2.0)

**Phase:** Phase 1 — Foundation
**Sprint goal:** Parse iNES 1.0 and NES 2.0 ROM files into a `Cartridge` value with PRG/CHR data, mapper id, mirroring, region, and submapper. NROM mapper implementation included.
**Estimated duration:** 1 week

## Tickets

### T-12-001 — `Cartridge`, `Mirroring`, `Region`, `RomError` types

**Description:** Define the public types in `crates/rustynes-mappers/src/cartridge.rs`. Per `docs/cartridge-format.md` §Public API.

**Acceptance criteria:**
- [x] Types compile and pass `cargo doc`.
- [x] `RomError` is `#[non_exhaustive]` and uses `thiserror`.

**Dependencies:** Sprint 1 complete.
**Reference:** `docs/cartridge-format.md` §Public API.
**Estimated complexity:** S.

---

### T-12-002 — iNES 1.0 header parsing

**Description:** Parse the 16-byte header for iNES 1.0 files (NES 2.0 detection bit clear). Extract magic, PRG/CHR sizes, mapper id (8-bit), mirroring, battery, trainer, four-screen.

**Acceptance criteria:**
- [x] Round-trip: parse header, re-serialize, byte-equal.
- [x] Returns `RomError::BadMagic` for non-`"NES\x1A"` prefix.
- [x] Mapper id correctly assembled from `(header[6] >> 4) | (header[7] & 0xF0)`.
- [x] Unit tests for at least 5 distinct iNES headers from real ROM dumps.

**Dependencies:** T-12-001.
**Reference:** `docs/cartridge-format.md` §Header layout.
**Estimated complexity:** M.

---

### T-12-003 — NES 2.0 header parsing

**Description:** Detect NES 2.0 via `(header[7] & 0x0C) == 0x08`. Parse 12-bit mapper, submapper, exponent-multiplier ROM sizing, RAM shifts, region, console type, expansion device.

**Acceptance criteria:**
- [x] Round-trip: parse, re-serialize, byte-equal.
- [x] Exponent-multiplier sizing correctly handles MSB nibble = `$F`.
- [x] RAM shift encoding: shift=0 → 0 bytes, shift>0 → `64 << shift`.
- [x] Region byte 12 maps to `Region::{Ntsc, Pal, Multi, Dendy}`.
- [x] Unit tests with known NES 2.0 headers.

**Dependencies:** T-12-002.
**Reference:** `docs/cartridge-format.md` §Header layout.
**Estimated complexity:** M.

---

### T-12-004 — `parse(bytes)` end-to-end

**Description:** The public entry point. Validates magic, applies detection rule, extracts trainer (if present), PRG-ROM, CHR-ROM, misc-ROM (NES 2.0 only). Returns `Cartridge` or typed `RomError`.

**Acceptance criteria:**
- [x] Truncated files return `RomError::Truncated { needed, got }`.
- [x] Unsupported mapper IDs (outside the coverage matrix) return `RomError::UnsupportedMapper(id)` for now (will be expanded as mappers are added).
- [x] Successful parse returns `Cartridge` with all fields populated.
- [x] Property test: random bytes never panic.

**Dependencies:** T-12-003.
**Reference:** `docs/cartridge-format.md` §Public API.
**Estimated complexity:** M.

---

### T-12-005 — `Mapper` trait + NROM implementation

**Description:** Define the `Mapper` trait in `crates/rustynes-mappers/src/mapper.rs`. Implement NROM (mapper 0): no banking, fixed PRG window, fixed CHR window, fixed mirroring (from header).

**Acceptance criteria:**
- [x] `Mapper` trait matches `docs/mappers.md` §Interfaces.
- [x] NROM implementation handles 16 KB and 32 KB PRG variants (16 KB is mirrored at `$8000-$BFFF` and `$C000-$FFFF`).
- [x] NROM CHR-RAM variant supported (when CHR-ROM size = 0).
- [x] Unit tests: read every byte of a synthetic NROM cartridge, verify mirroring.

**Dependencies:** T-12-004.
**Reference:** `docs/mappers.md` §Mapper coverage matrix, `docs/architecture.md` §Public API.
**Estimated complexity:** M.

---

### T-12-006 — Corpus round-trip test

**Description:** Bring in a curated subset of `nes-test-roms` (NROM-only test ROMs) under `tests/roms/sprint-2/`. Test: parse every ROM, assert no `RomError`, assert NROM construction succeeds.

**Acceptance criteria:**
- [x] At least 10 NROM test ROMs vendored with their CC0/PD license documented in `tests/roms/LICENSES.md`. (13 vendored.)
- [x] Test passes; runs as part of `cargo test --workspace --features test-roms`. (Default `cargo test --workspace` does not need the corpus.)

**Dependencies:** T-12-005.
**Reference:** `docs/testing-strategy.md` §Layer 3.
**Estimated complexity:** S.

---

## Sprint review checklist

- [x] All tickets checked off.
- [x] `cargo test -p rustynes-mappers` green (29 tests; 31 with `--features test-roms`).
- [x] CHANGELOG entry added for cartridge parser + NROM.
