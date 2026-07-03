# ADR 0028 — v2.0.0 Save-State and Movie Format Break (No Migration)

**Status:** Accepted.
**Date:** 2026-07-03
**Author:** RustyNES maintainers
**Supersedes:** None. Fulfills the MAJOR-boundary clause ADR 0003 §Decision
reserved for exactly this moment: "If a v2.x line introduces format breaks,
ADR 0004+ will define explicit migration." (Numbered 0028, not 0004+, per
the sequential numbering in force by the time this was written — see
`docs/adr/` for the intervening ADRs.)

## Context

The v2.0.0 "Timebase" release (`to-dos/plans/v2.0.0-master-clock-plan.md`)
collapsed a five-counter cycle-timebase substrate (`Cpu::master_clock`,
`Cpu::cycles`, `LockstepBus::cycle`, `LockstepBus::ppu_clock`,
`Apu::cpu_cycle`) into ONE canonical per-cycle counter, promoted to the
shipped default in beta.4 (PR #220). `Cpu::cycles` is now assigned from
`Bus::cycle_count()` at every `start_cycle` rather than incremented
independently — see `docs/adr/0029-one-clock-every-cycle-timebase.md` for
the full architectural decision.

This changes the *meaning* of the CPU snapshot's `cycles`/`master_clock`
pair without changing its *byte layout*. Before the promote, `cycles` and
`master_clock` were two independently-incremented counters kept in sync by
parallel bookkeeping at every call site (a fragile invariant — exactly the
kind of drift the beta.1 "one-clock counter collapse" was designed to
eliminate). After the promote, `cycles` is a pure function of
`master_clock` (via `Bus::cycle_count()`), so the pair is
consistent-by-construction rather than consistent-by-convention.

Per ADR 0003 §Policy, a "Different MINOR or MAJOR" version boundary is
where the project draws an honest line rather than shipping silent
best-effort compatibility it hasn't verified. v2.0.0 is exactly that MAJOR
boundary. Two formats are affected:

1. **`.rns` save states** (`crates/rustynes-core/src/save_state.rs` +
   `crates/rustynes-cpu/src/snapshot.rs`). `CPU_SNAPSHOT_VERSION` had
   already been bumped to 2 in a *pre-v2.0.0* session ("W3-Stage-4",
   2026-06-10) when `master_clock` was first added to the blob as inert
   parallel bookkeeping — that bump is unrelated to this decision. Investigation
   for this ADR found that `Nes::restore_inner` already enforces a *strict
   equality* check on the CPU section's version tag before ever calling
   `Cpu::restore` — meaning a v1 or stale-v2 CPU section is, in practice,
   *already* rejected via `SnapshotError::VersionMismatch` through the only
   real caller. The `Cpu::restore` function's own "v1 upconvert" branch
   (re-deriving `master_clock` as `cycles * 12` for a v1 blob) was dead
   code, unreachable through that gate.
2. **`.rnm` TAS movies** (`crates/rustynes-core/src/movie.rs`). Movies store
   only the input stream, not framebuffer/audio state, so there is no
   "wrong bytes get loaded" failure mode — a pre-v2.0.0 movie's INPUT
   STREAM still replays correctly (button semantics and frame timing are
   unchanged). What is unverified is the format's core promise:
   frame-for-frame *bit-identical* reproduction. That guarantee has only
   ever been proven within a single engine timebase; the one-clock promote
   changed how master-clock/PPU/CPU phase advances internally, so whether a
   v1-recorded movie reproduces byte-identically on the v2.0.0-line engine
   is unverified, not disproven and not guaranteed either way.

## Decision

**Clean rejection for save states, an honest warning (not a rejection) for
movies. No migration/transcoding code for either.**

### Save states — `CPU_SNAPSHOT_VERSION` 2 → 3

- The byte layout is **unchanged** from v2 (same fields, same order,
  same length). Only the version tag changes.
- `Cpu::restore` now rejects ANY version other than the current
  `CPU_SNAPSHOT_VERSION` outright — the v1-upconvert branch is deleted (it
  was dead code; see Context). This makes the *actual* behavior match what
  `Nes::restore_inner`'s gate already enforced, and removes a
  misleading code path that suggested best-effort upconversion was live
  when it was not reachable.
- `save_state::FORMAT_VERSION` (the container header) bumps 1 → 2 as a
  cheap, purely-documentary epoch marker: a `.rns` file's header alone now
  signals which release line produced it, without needing to inspect every
  section. The container's own on-wire layout is unchanged; this field only
  guards forward-compat (a reader rejects a blob whose `format_version`
  exceeds its own).
- Per ADR 0003 §Policy "Different MINOR or MAJOR": **no migration code path
  is written.** A pre-v2.0.0 slot file fails to load with a clear
  `SnapshotError::VersionMismatch { tag: "CPU ", file_version, chip_supports
  }`, not a silent reinterpretation.

### Movies — `MOVIE_FORMAT_VERSION` 1 → 2, warn-not-reject

- Unlike the CPU section, `Movie::deserialize`'s version guard only rejects
  blobs newer than the reader understands (`format_version >
  MOVIE_FORMAT_VERSION`) — it has never rejected OLDER, still-understood
  versions. That asymmetry is preserved deliberately: a movie's input
  stream is orthogonal to the internal timebase representation, so refusing
  to *play* an old movie would be strictly worse than the honest
  alternative below, with no correctness benefit.
- `MOVIE_FORMAT_VERSION` bumps 1 → 2 as the same kind of epoch marker as
  the save-state container: any movie with `format_version < 2` was
  necessarily recorded before the timebase promote.
- New: `rustynes_core::recorded_before_v2_timebase(bytes) -> Result<bool,
  MovieError>` — a lightweight header peek (no full parse) that lets a
  caller check the epoch before or after loading. Wired into both frontend
  movie-load paths (`app.rs`'s desktop file-picker handler and its
  `wasm32` counterpart): loading a pre-v2.0.0 movie now logs a warning
  ("input replay proceeds, but exact framebuffer/audio reproduction is not
  guaranteed across the engine-timebase boundary") instead of silently
  proceeding with no signal at all. Playback itself is unaffected.
- **Explicitly out of scope: timeline transcoding.** No code attempts to
  re-derive a "v2-native" recording from a v1 one, or to verify/repair
  bit-identical reproduction across the boundary. The honest move is
  surfacing the epoch to the caller, not promising equivalence the project
  hasn't proven.

## Consequences

### Positive

- The MAJOR-boundary decision ADR 0003 anticipated is now made explicitly,
  in writing, rather than left as an implicit assumption nobody verified.
- Save-state rejection is a NO-OP change in practice (the real gate already
  existed) but removes genuinely misleading dead code (the v1-upconvert
  branch) that could mislead a future contributor into thinking
  cross-version upconversion was a live, tested path.
- Movie playback keeps working across the boundary (no user-visible
  breakage for the common "just replay my inputs" case) while giving TAS
  authors an honest signal about the one case that matters to them
  (bit-identical verification).

### Negative / Costs

- Users with `.rns` slot files saved on any pre-v2.0.0 build (v1.0.0
  through v1.10.0, and any earlier v2.0.0 beta/rc built before this PR)
  cannot load them on v2.0.0+. This is expected and accepted per ADR 0003's
  own policy — slot files are user data, not user output, and the rewind
  ring is per-session regardless.
- TAS movies recorded pre-v2.0.0 carry an unverified (not proven false)
  determinism guarantee going forward. Authors who need bit-identical
  verification across the boundary must re-record on a v2.0.0+ build.

### Neutral

- No save-state or movie DATA changes — every field byte for byte is
  identical to what shipped in v1.10.0/beta.1-5. This is a version-tag and
  policy change, not a schema redesign, which is why the risk is Low
  despite touching two format-critical files.

## Alternatives considered

1. **Best-effort upconvert for the CPU section** (interpret a v1/v2 blob's
   `cycles`/`master_clock` pair as-is under the new derivation model).
   Rejected: since `cycles` and `master_clock` were kept in sync by
   parallel increments pre-promote, the VALUES likely remain compatible in
   practice — but "likely" is not the same as "verified," and ADR 0003's
   own policy explicitly prefers honest rejection at a MAJOR boundary over
   an unverified best-effort claim. There is also no pre-v2.0.0 `.rns`
   fixture committed to the test suite to empirically validate the claim
   against, which would be required before shipping it as a supported path.
2. **Reject pre-v2.0.0 movies outright**, mirroring the save-state
   decision. Rejected: movies store only inputs, which remain fully
   replayable — rejecting playback would be strictly worse for users than
   warning them, since the input-replay guarantee (unlike the
   bit-identical guarantee) is genuinely unaffected by the timebase change.
3. **Full migration/transcoding for either format.** Rejected as
   out-of-scope per the plan's explicit instruction ("Do NOT attempt
   timeline transcoding") and consistent with ADR 0003's "migration code
   earns its keep only when a format breaks compatibility" stance — the
   MAJOR-boundary event itself is the sanctioned point to NOT write
   migration code, by design.
