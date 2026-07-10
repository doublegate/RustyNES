# ADR 0031 — The game database must never override hardware/mapper-controlled state

**Status:** Accepted
**Date:** 2026-07-10
**Author:** RustyNES maintainers
**Supersedes:** none
**Related:** the per-game database (T-110-B4), ADR 0015 (browser-RA scaffolding is unrelated)

## Context

`crates/rustynes-frontend/src/game_database.txt` is a 2682-row table vendored from
TetaNES (GoodNES-derived), keyed by the CRC32 of PRG+CHR. At ROM load the frontend
applies four columns as *corrections*:

- `region` / `mapper` / `submapper` — patched into the iNES header **before**
  construction (`game_db::apply_header_overrides`, mismatch-guarded — only rewrites a
  header byte that actually differs);
- `mirroring` — applied **after** construction via
  `Nes::set_mirroring_override(Some(m))` (`App::apply_game_db` + the per-game overlay).

The mirroring override was intended to fix ROMs whose iNES header carries the wrong
nametable-mirroring solder-pad bit. Crucially, `parse_row` makes **every** matched
row an active override (there is no "only when it differs" gate on the mirroring
column), and `set_mirroring_override` **unconditionally wins** over the mapper in the
bus nametable-resolution path (`bus.rs`: `match self.nt_mirroring_override { Some(m) =>
override_nt_addr(m, addr), None => self.mapper.nametable_address(addr) }`).

That is wrong for any mapper that controls its **own** mirroring at runtime.

### The bug it caused (Wizards & Warriors)

Wizards & Warriors (USA) (Rev A), CRC `26535EF5`, is **mapper 7 (AxROM)**. AxROM has
no hardwired mirroring: it selects **single-screen A** or **single-screen B** from bit
4 of each `$8000` write, and W&W flips A↔B *mid-frame* (guarded by a sprite-0 hit) to
paint its bottom status bar from nametable B. The game-DB row lists this game as
`Horizontal` (the meaningless header bit of an AxROM cart). Force-applying `Horizontal`
pinned the nametable mapping, so the status-bar half of the split rendered blank, the
sprite-0 hit that drives the split stopped firing, and the game wedged in its
sprite-0 wait loop at level load — a hard, silent, deterministic freeze on desktop and
WASM, while a headless core (which never consults the DB) played the game perfectly.

Diagnosis cost far more than the fix: because the freeze reproduced only through the
full frontend, the investigation chased run-ahead, threading, the audio sink, sample
rate, power-cycle determinism, catch-up-burst pacing, and RetroAchievements before a
**full-state save-state snapshot diff** (desktop vs a byte-identical headless replay)
localized the divergence to **one byte** — the mirroring-override tag — present from
frame 1. One wrong database row masqueraded as a deep emulation-timing bug.

### Scope

The mirroring override is applied to **1914** of the 2682 rows whose mapper controls
its own mirroring (mapper 1 MMC1 = 716 rows, 4 MMC3 = 647, 7 AxROM = 60, 5 MMC5, 9
MMC2, 16/18/19 Bandai/Jaleco/Namco, VRC, Sunsoft FME-7, …). Every one was a latent
W&W-class freeze/corruption waiting for a game that switches mirroring at the wrong
moment. Only 768 rows sit on genuinely hardwired boards (NROM/UxROM/CNROM/GxROM).

## Decision

**A game-database (or per-game) override may only set state that is *not* determined by
the mapper/hardware at runtime. State the mapper owns must never be force-applied.**

Concretely, for the mirroring column:

1. Add `Mapper::has_hardwired_mirroring(&self) -> bool`, **defaulting to `false`**
   (assume the mapper controls its own mirroring — the *safe* default: a missing
   annotation merely declines a rarely-needed cosmetic correction and can never break
   a working game). Only the classic fixed-mirroring discrete boards override it to
   `true`: NROM (0), UxROM (2), CNROM (3), GxROM (66).
2. Expose it as `Nes::mapper_has_hardwired_mirroring()`.
3. Gate both application sites (`App::apply_game_db` and the per-game overlay) on that
   query; the per-game path logs a note when it declines an explicit override so the
   drop is never silent.

The `region` / `mapper` / `submapper` header corrections are a **different class**:
they correct the cartridge's *static identity* (what the header should have said), not
state the mapper produces at runtime, and they are already mismatch-guarded. They are
left in place — but the general principle below governs any future consumer.

### General principle (applies to every column, present and future)

Before the frontend force-applies a database value, ask: *does the emulated hardware
determine this value itself?* If yes, the database may only supply it as a **hint the
core is free to ignore** — never as an override that wins over the running hardware.
Runtime-controlled state (mirroring on register-mirrored mappers, CHR/PRG banking,
IRQ configuration, expansion audio, …) is owned by the mapper; the database describes
cartridge identity, not live state.

## Consequences

- W&W and the other 1913 mapper-controlled rows can no longer be corrupted by a stray
  mirroring value; verified by a full-state snapshot diff (post-fix W&W is
  byte-identical to the clean headless baseline).
- A regression test (`rustynes-mappers` `hardwired_mirroring_gate_matches_board_type`)
  pins the contract: AxROM/MMC1/MMC3/MMC5/MMC2 report `false`; NROM/UxROM/CNROM/GxROM
  report `true`.
- Hardwired boards **other than** {0,2,3,66} (Color Dreams, NINA-003-006, Sachen, …)
  now fall back to their header mirroring instead of the DB value. For a correctly
  dumped ROM this is identical (the DB value matches the header); only a
  mis-dumped-header ROM on one of those boards loses a cosmetic correction. Extending
  coverage is a one-line `has_hardwired_mirroring() -> true` on the specific mapper.
- The core is unchanged on the no-database path, so determinism / AccuracyCoin / the
  commercial oracle are byte-identical (the core test suites never consult the DB).
- Lesson recorded: a single vendored-data row can imitate a deep engine bug; when a
  failure reproduces only through the frontend, diff the **full** core state
  (save-state snapshot) between the frontend and a headless replay early — it collapses
  the search to the exact differing byte.
