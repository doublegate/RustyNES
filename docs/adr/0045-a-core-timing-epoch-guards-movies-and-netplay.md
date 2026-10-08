# 45. A core timing epoch guards movies and netplay against a core that emulates differently

Date: 2026-10-05

## Status

Accepted. Decided by the maintainer on 2026-10-05, at the v3.0.0 planning
("Add a core timing epoch"). It extends
[ADR 0044](0044-movies-and-netplay-carry-the-emulation-options.md), which made a
movie and a netplay peer carry the *options* they run under. This ADR makes
them carry the *emulator behaviour* as well. Ticket: T-EMULATION-EPOCH
(`to-dos/ROADMAP.md`).

## Context

ADR 0044's rule is that "a replay or a peer can never silently diverge". It
enforces that rule for every emulation option, but not for the emulator
itself. Two builds with the same options can emulate the same game
differently whenever an accuracy fix lands, and nothing records which
behaviour a movie or a peer expects.

v3.0.0 makes this concrete. T-MMC3-BG-A12 changes the A12 stream an MMC3
counts when the background sits at `$1000`, and the IRQ moves with it. At
v2.9.9:

- netplay `PROTOCOL_VERSION` was 5, and `config_digest` holds no core
  version, so a v2.9.9 peer and a v3.0.0 peer would connect, agree, and
  desync;
- `.rnm` format 4 was the current format, so a v2.9.9 movie would load and
  replay under the new timing.

Neither says why. A per-release check, such as "same crate version", would
be too strict: most releases change no emulated output. A per-format check,
bumping the movie format only when the bytes change, misses this case
entirely, because the bytes do not change.

## Decision

1. **`rustynes_core::EMULATION_EPOCH: u32`**, one constant for the whole core.
   It is **1** at v3.0.0, the first release that carries it.
2. **The bump rule.** Increment the epoch, in the same change, whenever a
   change alters what the core produces from the same inputs, compared with
   the last release: a framebuffer, an audio sample, or a CPU/PPU bus cycle.
   Every such change already shows up as a re-blessed golden or a moved
   commercial snapshot, so the rule has a mechanical trigger. A change whose
   goldens and snapshots all stand does not bump it. Pure refactors,
   performance work that passes `ab_check.sh`'s byte-identity rule, frontend
   features and new mappers (which have no earlier output to differ from) do
   not bump it.
3. **Movies** record the epoch in the `.rnm` header. This is **format 5**, and
   `MIN_MOVIE_FORMAT_VERSION` becomes 5.
   - **Older than format 5:** refused, as every format raise since ADR 0044
     has been.
   - **Format 5 with a different epoch:** refused, with an error that names
     both epochs and says the movie was recorded by a version of RustyNES
     that emulates differently.
   - **Foreign imports** (`.fm2`, `.bk2`, `.fcm`, `.fmv`, `.vmv`) carry no
     RustyNES timing. They import at the current epoch, as they import at the
     stock NES options (ADR 0044).
4. **Netplay** carries the epoch in `SessionIdentity`. This is **protocol 6**,
   with a new `Sync` magic.
   - A peer with another epoch is refused with
     `IdentityMismatch::Emulator { ours, theirs }`, and a disconnect reason that
     says "a different emulator version", not "settings differ".
   - The epoch is checked before the ROM and the configuration, because it is
     the more fundamental disagreement.
5. **Peers from before the epoch.** A `Sync` that carries one of RustyNES's own
   older magics (`"RNES"`, protocol 4; `"RNE5"`, protocol 5) is recognised and
   refused with the same "different emulator version" reason. It is no longer
   ignored as a stray datagram, which would let the session time out with no
   explanation. Unknown magics are still ignored, as before.
6. **Save states (`.rns`) do not carry the epoch.** A state is a snapshot the
   user resumes; continuing it under a newer behaviour is the expected upgrade
   path, and the section versions already refuse a layout that changed.
   Rollback and run-ahead restore states the same build wrote. A state used
   by netplay is covered by the peers' epochs.

## Consequences

- **v2.9.9 and v3.0.0 peers will not play together.** The v3.0.0 side reports
  why. The v2.9.9 side cannot be changed, and sees a peer that never answers.
- **Every v2.9.9 movie is refused by v3.0.0** (format 4 < 5). Players
  re-record. The release notes must say so, with ADR 0044's other movie
  breaks.
- **The epoch is a promise that the next behaviour change keeps.** The bump
  rule belongs in the release ceremony (AGENTS.md): a release whose goldens
  moved must have raised the epoch. If a release forgets, a later movie or
  peer can diverge silently again, which is exactly this ADR's subject. A
  check that ties the epoch to a fingerprint of a fixed panel of outputs
  would enforce the rule. If such a check proves practical, it is added as
  part of this decision's implementation; its coverage limit (the panel is
  not every game) is stated where it lives.

  **Amendment, 2026-10-07 (v3.1.0, `T-EPOCH-FINGERPRINT`): the check exists.**
  `crates/rustynes-test-harness/tests/epoch_fingerprint.rs` fingerprints
  seven committed test ROMs (every frame's framebuffer, all audio, end RAM
  and the CPU cycle count) against `golden/epoch_fingerprint.tsv`, which
  records the epoch and `last_release_epoch`, the epoch the last release
  shipped. Output that moves while `EMULATION_EPOCH` equals
  `last_release_epoch` fails, and a re-bless is refused in that state; once
  the epoch is raised, a re-bless records the new output, and a second
  change in the same release needs only another re-bless. It runs in CI's
  `test-roms` job. Shown on two seeded mutants (this release's own two
  accuracy fixes, reverted): each fails, its bless is refused, and raising
  the epoch then re-blessing passes. **Coverage limit:** a change that moves
  nothing on the panel passes; the commercial snapshot suites stay the wider
  net. **The release cut must set `last_release_epoch`** to the shipped epoch
  (`docs/agents/ci-and-release.md`), or the gate stops guarding.
- **Libretro is unaffected.** RetroArch netplay compares serialized states,
  which already differ between core versions.

## Alternatives considered

1. **Bump the protocol and movie format only, with no reusable epoch.** This
   closes v3.0.0's gap, but the next accuracy fix faces the same gap, and the
   format number would be raised for a reason it does not describe.
2. **Put the crate version in the identity.** That refuses far more than it
   needs to: most releases emulate identically. It would also make a patch
   release break every recorded movie.
3. **Leave it and document it.** The maintainer chose against this: it is the
   silent divergence ADR 0044 exists to prevent.
