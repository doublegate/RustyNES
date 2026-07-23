# ADR 0034 — PPU Snapshot v8: Serialize the Sprite-Evaluation FSM (Save-State Break)

**Status:** Accepted.
**Date:** 2026-07-22
**Author:** RustyNES maintainers
**Related:** ADR 0028 (v2.0.0 save-state / movie format break), ADR 0003
(version + compatibility policy), ADR 0031 (game-DB must not override
mapper-controlled state — the previous "the frontend, not the core, is
where the discrepancy lives" finding).

## Context

RustyNES's AccuracyCoin battery measures **141/141 (100.00%)** in the
headless harness (`crates/rustynes-test-harness/tests/accuracycoin.rs`).
Running the same ROM in the desktop frontend produced **138/141**, failing:

| Suite | Test | Error code |
|---|---|---|
| PPU Behavior | Rendering Flag Behavior | 2 |
| Sprite Evaluation | Arbitrary Sprite zero | 2 |
| Sprite Evaluation | Misaligned OAM behavior | 1 |

Neither number was wrong. The harness and the frontend do not run the
emulator the same way.

`[input] run_ahead` defaults to **1** (`default_run_ahead()`,
`crates/rustynes-frontend/src/config.rs`). At that setting every visible
frame goes through `RunAhead::run_frame_ahead` + `finish`
(`crates/rustynes-frontend/src/runahead.rs`):

1. one persistent `run_frame` — the real timeline;
2. `Nes::snapshot_core_into`;
3. `N` further `run_frame`s (hidden + visible);
4. `Nes::restore_quiet` back to (2).

So the desktop app performs a full core save-state round trip **sixty times
a second**, at an arbitrary point in the frame. The headless harness does
not, and therefore cannot observe an incomplete save-state schema at all.

Bisecting the frontend's configuration headlessly isolated it to run-ahead
alone (rewind, OAM decay, PPU revision, power-on RAM randomization, and the
palette knobs were all at their defaults and all irrelevant):

| run-ahead | rewind | result |
|---|---|---|
| 0 | off | 141 / 141 |
| 0 | on | 141 / 141 |
| 1 | on | **138 / 141** |
| 1 | off | **138 / 141** |

— reproducing the frontend's failure set and error codes exactly.

Auditing the 113 fields of `struct Ppu` against everything
`crates/rustynes-ppu/src/snapshot.rs` writes found the gap. `secondary_oam`
— the *buffer* the sprite-evaluation pass fills — was serialized from v1.
The **pointers and phase that fill it** were not:

- the per-dot eval FSM: `sprite_eval_read_latch`, `sprite_eval_n` / `_m` /
  `_found` / `_sec_idx`, and the `_copying` / `_done` / `_overflow_search` /
  `_zero_found` / `_first_iter` phase flags;
- the parallel OAM-data-bus model: `oam_bus_copybuffer`,
  `oam_bus_secondary[32]`, `oam_bus_addr_h` / `_addr_l` / `_secondary_addr`,
  `oam_bus_copy_done`, `oam_bus_sprite_in_range`,
  `oam_bus_overflow_counter`;
- `oam2_addr`, the secondary-OAM write pointer maintained across the dots
  1..=64 clear window.

A snapshot taken mid-eval therefore restored a full secondary-OAM buffer
alongside a power-on-default walker, once per frame, forever.

This is the **third** instance of the same bug class and the second to reach
users. The v5 tail closed it for the 2-cycle-ALE fetch state (ADR 0030); the
v6 tail closed it for the sprite-shifter halt latches and the OAM-corruption
arming state, which had produced the Wizards & Warriors half-blank playfield.
Each time, the mechanism was identical: live mid-frame PPU state absent from
the schema, invisible to every straight-`run_frame` test, exposed only by a
per-frame snapshot/restore.

Mesen2 serializes the equivalent set — `_spriteIndex`, `_spriteCount`,
`_sprite0Added`, `_sprite0Visible`, `_oamCopybuffer`, `_secondaryOamAddr`,
`_spriteInRange`, `_oamCopyDone`, `_overflowBugCounter`
(`Core/NES/NesPpu.cpp`, `NesPpu<T>::Serialize`) — independent confirmation
that this is live state rather than something derivable on load.

## Decision

**Bump `PPU_SNAPSHOT_VERSION` 7 → 8 and serialize all of the above** as a
50-byte tail. **Accept the resulting save-state break.**

The `.rns` container is version-**exact** per section
(`crates/rustynes-core/src/bus.rs`: `if s.version != PPU_SNAPSHOT_VERSION`
→ `SnapshotError::VersionMismatch`), so unlike the additive v3–v7 tails this
one makes existing save states fail to load. That is the intended outcome,
not a side effect worth engineering around:

- A pre-v8 `.rns` genuinely does not contain the eval state. "Loading" one
  means resuming with a reset walker beside a populated buffer — the exact
  defect this ADR closes. A migration path would have to invent the missing
  bytes.
- Failing closed with a named `VersionMismatch` is the honest report. ADR
  0028 established this precedent for exactly this reason: the project would
  rather refuse a state than silently misinterpret it.
- `Ppu::restore` retains its v1..=7 upconvert path (fields set to the
  constructor defaults — `0xFF` read latches, `[0xFF; 32]` parallel
  secondary OAM, `0`/`false` elsewhere) for direct callers that are not
  going through the version-exact container.

Movies (`.rnm`) and netplay are unaffected: both re-derive state from a
fresh power-on rather than from a stored blob. Netplay *rollback* and TAS
seeking take the same in-process round trip as run-ahead and so are fixed by
the same change.

Separately, the scanline-classification cache (`cached_visible`,
`cached_pre_render`, `cached_render_line`, keyed by `flags_cached_scanline`)
is **invalidated on restore at every schema version** rather than
serialized. It is a pure function of `scanline` + `region`, both already in
the blob, so recomputing costs nothing and adds no bytes — while a warm key
carried across a restore could satisfy the fast dot path's
`scanline == flags_cached_scanline` guard against a value computed under a
different timeline. Mesen2 makes the same call, recomputing its derived
state in the `if(!s.IsSaving())` post-load block.

## Consequences

**Positive.**

- The desktop frontend now measures **141/141 with run-ahead on**, at depth
  1 and 2. The headless and frontend numbers agree for the first time.
- Netplay rollback and TAS seek inherit the fix.
- `crates/rustynes-test-harness/tests/accuracycoin_runahead.rs` reruns the
  entire battery through the run-ahead cycle and asserts no test is lost,
  naming any that is. This closes the *class*, not just the instance: the
  existing `runahead.rs` regressions compare framebuffers on two ROMs, which
  is a weaker oracle than a battery that probes the eval FSM at dot
  resolution and reports which behavior broke.

**Negative.**

- **Existing `.rns` save states will not load.** Users must replay from a
  power-on or keep an older build to read them. There is no migration and
  none is possible.
- This is a compatibility break outside a MAJOR boundary, which ADR 0003's
  policy reserves for MAJOR releases. It is taken deliberately: the
  alternative is knowingly shipping states that restore a broken FSM. The
  release carrying it must say so plainly in its notes, not bury it.

**Neutral.**

- The blob grows 50 bytes (~0.02% against the 245,760-byte framebuffer that
  dominates it). No measurable cost to run-ahead, rewind, or rollback.
- No change to the emulation core's forward path: nothing outside
  `snapshot`/`restore` was touched, so a plain `run_frame` timeline — and
  therefore every golden vector, nestest, and the headless battery — is
  byte-identical.

## Follow-up

The audit that found this compared `struct Ppu`'s fields against the
serializer's field list mechanically. That diff is cheap and would have
caught all three instances of this bug class. Worth running as a check
whenever a field is added to a chip struct, and worth considering as a test
that fails when an unrecognized field appears in neither the schema nor an
explicit "derived / config, deliberately not serialized" allowlist.
