# RustyNES v2 Test ROM Library Audit Report

**Date:** 2026-05-17
**Branch:** `accuracy-stabilization` (work eventually merged forward to `main`)
**Scope:** Buildout of `tests/roms/` to cover all 15 supported mappers.

> **Provenance.** Originally authored in `/tmp/rustynes-rom-audit.md`
> by the ROM-library-buildout sub-agent. Preserved into the repo at
> `docs/audit/rom-library-buildout-2026-05-17.md` post-session because
> CachyOS wipes `/tmp` on every reboot and this document captures
> non-recreatable provenance + decision rationale (the *why* of the
> mapper-subdir layout, the rejected-but-investigated ROM list, the
> iNES-mismatch table). Cross-referenced from
> [`tests/roms/README.md`](../../tests/roms/README.md).

---

## 1. Commercial ROMs staged in `tests/roms/external/` (gitignored)

**Total: 60 `.nes` ROMs across 20 mapper-NNN subdirectories** (plus
the user's `Kid Icarus.ram` save file alongside its mapper-1 ROM).

All 15 supported mappers (NROM, MMC1, UxROM, CNROM, MMC3, MMC5, AxROM,
MMC2, MMC4, N163, VRC2a, VRC2b/VRC4e/f, VRC4a/c, VRC4b/d (+VRC2c),
VRC6a, VRC6b, GxROM, FME-7/Sunsoft-5B, VRC1, VRC7) now have at least
one verified commercial ROM. Every ROM was extracted from the user's
`~/Dropbox/ROMs/Nintendo Entertainment System - Famicom (2020)/` archive
and its iNES mapper byte was verified against the target mapper before
being placed in the corresponding `mapper-NNN-NAME/` subdirectory.

| Mapper | Subdir | Count | Notes |
|--------|--------|-------|-------|
| 0 NROM | `mapper-000-NROM/` | 6 | SMB, Excitebike, Donkey Kong, Balloon Fight, Ice Climber, Gyromite |
| 1 MMC1 | `mapper-001-MMC1/` | 7 | LoZ, Metroid, Final Fantasy, Mega Man 2, Castlevania II, Kid Icarus, Ninja Gaiden (originally placed as UxROM but corrected to MMC1) |
| 2 UxROM | `mapper-002-UxROM/` | 4 | Mega Man, Castlevania, Contra, DuckTales |
| 3 CNROM | `mapper-003-CNROM/` | 3 | Arkanoid, Gradius, Paperboy |
| 4 MMC3 | `mapper-004-MMC3/` | 7 | SMB3, Mega Man 3, Kirby's Adventure, SMB2 USA, Ninja Gaiden II, TMNT III, Tiny Toon 2 (USA) |
| 5 MMC5 | `mapper-005-MMC5/` | 3 | Castlevania III USA, Bandit Kings of Ancient China, Uchuu Keibitai SDF (Japan) |
| 7 AxROM | `mapper-007-AxROM/` | 4 | Battletoads, Marble Madness, Solstice, Cobra Triangle |
| 9 MMC2 | `mapper-009-MMC2/` | 2 | Mike Tyson's Punch-Out!!, Punch-Out!! (the only two commercial MMC2 carts) |
| 10 MMC4 | `mapper-010-MMC4/` | 3 | Famicom Wars (Japan), Fire Emblem 1 (Japan), Fire Emblem Gaiden (Japan) — fan-translated dumps; mapper byte preserved |
| 19 N163 | `mapper-019-Namco163/` | 4 | Final Lap, Famista '90, Famista '91, Mappy Kids (all Japan) |
| 21 VRC4a/c | `mapper-021-VRC2-VRC4/` | 1 | Wai Wai World 2 (Japan, En) |
| 22 VRC2a | `mapper-022-VRC2/` | 1 | TwinBee 3 (Japan, En) |
| 23 VRC2b/VRC4e/f | `mapper-023-VRC2-VRC4/` | 4 | Akumajou Special, Crisis Force, Ganbare Goemon 2, Wai Wai World (Japan, En) |
| 24 VRC6a | `mapper-024-VRC6/` | 1 | Castlevania III Retranslation hack (derives from Japan-only Akumajou Densetsu which is VRC6a) |
| 25 VRC4b/d | `mapper-025-VRC2-VRC4/` | 1 | Ganbare Goemon Gaiden (Japan, En) |
| 26 VRC6b | `mapper-026-VRC6/` | 2 | Esper Dream 2 (Japan, En), Mouryou Senki Madara (Japan, En) |
| 66 GxROM | `mapper-066-GxROM/` | 2 | Doraemon (Japan, En), Thunder & Lightning |
| 69 FME-7/5B | `mapper-069-FME7-Sunsoft5B/` | 2 | Mr. Gimmick (USA), Batman Return of the Joker |
| 75 VRC1 | `mapper-075-VRC1/` | 2 | Ganbare Goemon! Karakuri Douchuu, King Kong 2 (Japan, En) |
| 85 VRC7 | `mapper-085-VRC7/` | 1 | Lagrange Point (Japan, En) — the canonical VRC7 game; Tiny Toon 2 USA is on MMC3 |

### Reorganization actions

- Created 20 `mapper-NNN-NAME/` subdirectories (`000-NROM` … `085-VRC7`).
- Moved the user's 5 originally-loose ROMs (Super Mario Bros, Excitebike,
  Gyromite, Kid Icarus, Super Mario Bros. 3) into the correct mapper
  subdirectory based on the verified iNES mapper byte.
- Deleted the legacy single-digit empty `mapper-0/` … `mapper-4/`
  directories (each had only a `.gitkeep` placeholder) — these were
  superseded by the new zero-padded naming scheme.
- Did NOT delete the user's `Kid Icarus.ram` save file — moved it into
  `mapper-001-MMC1/` alongside the ROM.

### iNES-mapper mismatches discovered during verification

These commercial ROMs were initially queued for one mapper but turned
out to be in a different one. Each was re-staged correctly:

| Initial guess | Actual mapper | Game |
|---------------|---------------|------|
| 2 UxROM | 1 MMC1 | Ninja Gaiden |
| 4 MMC3 | 9 MMC2 | Mike Tyson's Punch-Out!! |
| 23 VRC2/VRC4 | 22 VRC2a | TwinBee 3 |
| 23 VRC2/VRC4 | 75 VRC1 | Ganbare Goemon! Karakuri Douchuu |
| 24 VRC6 | 26 VRC6 | Esper Dream 2, Mouryou Senki Madara |
| 25 VRC2/VRC4 | 21 VRC4a/c | Ganbare Goemon Gaiden 2 |
| 85 VRC7 | 4 MMC3 | Tiny Toon Adventures 2 (USA version is MMC3, only Japan is VRC7) |
| 66 GxROM | 0/1/95 | Various — none of the USA "obvious" GxROM games turned out to be on mapper 66 |
| 7 AxROM | 1 MMC1 | Iron Tank, Conflict |

The verification script (`docs/audit/check-mapper.sh`, originally
authored in `/tmp/rustynes-rom-staging/` and preserved into the repo
post-session) reads iNES bytes 6 / 7 / 8 and computes the canonical
mapper number (including iNES 2.0 high-nibble extension); any mismatch
caused `extract-batch.sh` (also preserved alongside it) to exit
non-zero without copying the file. The final inventory is 100%
header-verified.

---

## 2. New committed test ROMs added to `tests/roms/`

**Total: 21 ROMs across 3 new subdirectories.**

All sourced from license-clean upstreams:

| Subdir | ROMs | Mapper coverage | Upstream | License |
|--------|------|-----------------|----------|---------|
| `audio-tests/` | 19 `.nes` + UPSTREAM_README.md | 0, 5, 19, 24, 26, 69, 85 | `bbbradsmith/nes-audio-tests` | "Freely redistributed and modified for any purpose" (effectively PD) |
| `m22/` | 1 (`0-127.nes`) | 22 (VRC2a) | `christopherpow/nes-test-roms/m22chrbankingtest/` | Public domain (aggregator) |
| `mmc1_a12/` | 1 (`mmc1_a12.nes`) | 1 (MMC1) | `christopherpow/nes-test-roms/MMC1_A12/` | Public domain (aggregator) |

### Mapper-by-mapper additions

The 13 new audio-tests + 2 new mapper-specific ROMs fill these
previously-uncovered mappers on the committed side:

| Mapper | New committed coverage | What it tests |
|--------|------------------------|---------------|
| 19 N163 | `db_n163.nes`, `test_n163_longwave.nes` | Wavetable audio amplitude, long-period accuracy |
| 22 VRC2a | `m22/0-127.nes` | CHR-bank 0..127 reachability |
| 24 VRC6a | `db_vrc6a.nes` | VRC6 audio with Akumajou pinout |
| 26 VRC6b | `db_vrc6b.nes` | VRC6 audio with Madara pinout |
| 69 FME-7/5B | `db_5b.nes`, `clip_5b.nes`, `noise_5b.nes`, `sweep_5b.nes`, `envelope_5b.nes`, `phase_5b.nes` | 5B envelope / LFSR / sweep / clip / phase semantics |
| 85 VRC7 | `db_vrc7.nes`, `test_vrc7.nes`, `patch_vrc7.nes`, `clip_vrc7.nes`, `noise_vrc7.nes` | VRC7 register surface; future OPLL fixture |
| 1 MMC1 | `mmc1_a12.nes` | A12 transition control case |
| 5 MMC5 | `db_mmc5.nes` | MMC5 raw-PCM amplitude |
| 0 NROM | `db_apu.nes`, `tri_silence.nes`, `dac_square.nes` | APU triangle silence, DAC linearity, baseline |

### Mappers WITHOUT committed CC0/permissive test ROMs (gap)

After thorough research the following mappers have **no
permissively-licensed dedicated test ROM** that I could locate:

- **Mapper 21 (VRC4a/c)** — Konami-internal IRQ counter; no public test
  ROM published. The shared VRC4 test coverage is via the VRC6 audio
  tests' adjacent IRQ paths.
- **Mapper 23 (VRC2b / VRC4e / VRC4f)** — Same. The `m22` test exercises
  the VRC2 CHR addressing on mapper 22, which shares its addressing
  algebra with VRC2 on mapper 23.
- **Mapper 25 (VRC4b / VRC4d + VRC2c)** — Same.
- **Mapper 75 (VRC1)** — Same. No published VRC1 test ROM.

These four mappers are still **commercially covered** in
`tests/roms/external/` (Wai Wai World 2 for 21, etc.). Their
coverage gap is a function of upstream publishing decisions, not a
license-clarity rejection.

### License-clean candidates discovered but NOT committed

| ROM | Upstream | Reason for non-inclusion |
|-----|----------|--------------------------|
| `240pee.nes` / `240pee-bnrom.nes` | `christopherpow/nes-test-roms/240pee` | GPL-licensed per Damian Yerrick — too restrictive for a permissive corpus when blargg / kevtris equivalents already cover the same ground. |
| `apu_reset/*` | `christopherpow/nes-test-roms` | Useful but not yet wired to a harness; deferred to the next sprint. |
| `cpu_exec_space/*` | `christopherpow/nes-test-roms` | APU and PPU-IO open-bus tests; deferred (the existing `cpu_dummy_reads/writes` covers the most common paths). |
| `dmc_tests/*` | `christopherpow/nes-test-roms` | DMC buffer / latency / status / IRQ — deferred until a regression suite for DMC is added. |
| `dpcmletterbox/*` | `christopherpow/nes-test-roms` | Visual DMC stress; not a pass/fail ROM. |
| `instr_misc/*`, `instr_test-v3/*`, `instr_timing/*` | `christopherpow/nes-test-roms` | Older / less-comprehensive versions of `instr_test-v5`; superseded. |
| `nsf2_*.nsf` family | `bbbradsmith/nes-audio-tests/build` | NSF2 player tests, not iNES `.nes` ROMs; out of scope for an iNES-only emulator test suite. |
| `*_nrom.nes` variants from `nes-audio-tests` | `bbbradsmith/nes-audio-tests/build` | These are NROM-shim builds of the same tests for use on dev carts; the native-mapper version (e.g. `db_vrc6a.nes`) is what we want. Skipped. |
| `*.nsf` companions in `nes-audio-tests/build` | `bbbradsmith/nes-audio-tests/build` | NSF format, not iNES. |

No ROM was rejected for "license unclear" — every upstream surveyed had
an explicit license statement.

---

## 3. Documentation deliverables

| File | Purpose | Status |
|------|---------|--------|
| `tests/roms/README.md` | Top-level index + mapper coverage matrix (committed side) | Created |
| `tests/roms/external/README.md` | Mapper-by-mapper coverage table for commercial ROMs, with SHA-256 + rationale per ROM | Rewritten (was stale Sprint-4-era doc) |
| `tests/roms/audio-tests/README.md` | New subdir README — what each `db_*` / `test_*` / etc. exercises | Created |
| `tests/roms/m22/README.md` | New subdir README — VRC2a CHR banking | Created |
| `tests/roms/mmc1_a12/README.md` | New subdir README — MMC1 A12 control case | Created |
| `tests/roms/nestest/README.md` | kevtris CPU validation provenance + harness pointer | Created (was missing) |
| `tests/roms/blargg/README.md` | Per-subsuite coverage matrix | Created (was missing) |
| `tests/roms/sprint-2/README.md` | Sprint-2-era extra ROMs index | Created (was missing) |
| `tests/roms/mmc5/README.md` | MMC5 test ROM index | Created (was missing) |
| `tests/roms/accuracycoin/README.md` | Runtime ROM directory README | Created (was missing) |
| `tests/roms/AccuracyCoin/README.md` | Catalog directory README (explains why two dirs exist) | Created (was missing) |
| `tests/roms/LICENSES.md` | Added 3 new sections: bbbradsmith audio tests, m22 CHR banking, MMC1 A12 (21 new rows of provenance) | Updated |

The aspirational `scripts/verify-roms.sh` reference in the old
external README is dropped; the harness performs header verification
in `external_real_games.rs::run_smoke_battery_for_path`.

---

## 4. Cargo test path updates

**None required.** No committed subdirectory was renamed. The two
existing path references to lowercase `accuracycoin/` and uppercase
`AccuracyCoin/` (in `crates/nes-test-harness/src/accuracy_coin.rs:176`
and `crates/nes-test-harness/src/accuracy_coin_catalog.rs:64`
respectively) continue to resolve correctly because both directories
are preserved.

The only edits to `crates/` were the pre-existing uncommitted FSM fix
in `nes-ppu/src/ppu.rs` (NOT touched per the task constraint) and the
unmodified pre-existing edits to `nes-test-harness/Cargo.toml` and
`tests/external_real_games.rs` (those belong to the parent session's
`commercial-roms` feature work, also NOT touched here).

---

## 5. Final remaining gaps

| Mapper | Commercial cover? | Committed cover? | Gap notes |
|--------|------------------|------------------|-----------|
| 21 VRC4a/c | Yes (Wai Wai World 2) | NO dedicated test ROM | No public mapper-21 test ROM published |
| 23 VRC2b/VRC4e/f | Yes (4 games) | NO dedicated test ROM | No public mapper-23 test ROM published |
| 25 VRC4b/d | Yes (Ganbare Goemon Gaiden) | NO dedicated test ROM | No public mapper-25 test ROM published |
| 75 VRC1 | Yes (2 games) | NO dedicated test ROM | No public mapper-75 test ROM published |
| 24 VRC6a (commercial) | Translated hack only | `db_vrc6a.nes` | The clean Japanese Akumajou Densetsu original is not in the user's Dropbox archive; the fan-translation hack of the same ROM is. |
| 85 VRC7 (commercial) | Lagrange Point only | 5 ROMs | Tiny Toon 2 Japan would be a second VRC7 ROM but is not in the user's archive. |

All other 9 mappers have **both** a committed permissive test ROM AND
at least one commercial ROM. These 4 mappers without a dedicated
public test ROM (21, 23, 25, 75) rely on commercial ROMs in `external/`
for end-to-end smoke testing — this is a project-policy decision, not
a license clarity issue.

---

## 6. Files modified outside `tests/roms/`

None. The task constraints prohibited touching code outside `tests/roms/`
except for `LICENSES.md` (which IS inside `tests/roms/`) and cargo test
source files if a committed subdir was renamed (none were).

The pre-existing uncommitted edits to `Cargo.lock`,
`crates/nes-ppu/src/ppu.rs`, `crates/nes-test-harness/Cargo.toml`, and
`crates/nes-test-harness/tests/external_real_games.rs` from the parent
session are left untouched.

---

## 7. Final `git status --short`

```
 M Cargo.lock
 M crates/nes-ppu/src/ppu.rs
 M crates/nes-test-harness/Cargo.toml
 M crates/nes-test-harness/tests/external_real_games.rs
 M tests/roms/LICENSES.md
?? tests/roms/AccuracyCoin/README.md
?? tests/roms/README.md
?? tests/roms/accuracycoin/README.md
?? tests/roms/audio-tests/
?? tests/roms/blargg/README.md
?? tests/roms/m22/
?? tests/roms/mmc1_a12/
?? tests/roms/mmc5/README.md
?? tests/roms/nestest/README.md
?? tests/roms/sprint-2/README.md
```

The four `M`-marked pre-existing changes are from the parent session
(NOT touched here). All other changes are exclusively under
`tests/roms/`.

The `external/` directory remains gitignored (verify:
`git check-ignore tests/roms/external/mapper-000-NROM/`) and is not in
this status output.
