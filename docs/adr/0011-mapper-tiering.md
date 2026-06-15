# ADR 0011 — Mapper accuracy tiering (Core / Curated / BestEffort)

**Status:** Accepted.
**Date:** 2026-06-15
**Author:** RustyNES maintainers
**Relates to:** [ADR 0001 — Mapper Dispatch](0001-mapper-dispatch.md) (the
`Box<dyn Mapper>` `parse()` match this classifier shadows); the v1.2.0
"Curator" release plan (Workstream A).

## Context

RustyNES shipped v1.1.0 with **51 mapper families**, every one of which is
covered by the AccuracyCoin / commercial-ROM oracle suites — the project's
"test-ROM-is-spec" guarantee. The v1.2.0 plan deliberately pushes mapper
coverage hard, in two ways at once:

1. a **curated** batch of well-documented long-tail boards that have notable
   games and a redistributable fixture or a precise decode-table spec, and
2. an **aggressive sweep** toward GeraNES-level coverage (100+ families),
   most of which are pirate / multicart / homebrew boards that have **no
   redistributable test ROM** at all.

The sweep creates a credibility hazard. If a board ported "by reading the
GeraNES/Mesen2 source" (with no hardware-test oracle proving it) silently
enters the same coverage count as the AccuracyCoin-gated families, the
project's headline accuracy claims stop being honest: "N mappers, 100%
AccuracyCoin" would conflate boards we can *prove* against boards we merely
*believe*. We need a way to grow coverage without diluting the accuracy
guarantee — and the distinction must be machine-checkable, not prose.

## Decision

Classify every supported mapper family into one of three tiers, recorded by a
single `const fn` source of truth, `mapper_tier(id, submapper)` in
`crates/rustynes-mappers/src/tier.rs`:

- **`Core`** — the original 51 families. Spec-implemented and gated by the
  AccuracyCoin / commercial-ROM oracle suites. Unchanged.
- **`Curated`** — long-tail families added with concrete game demand **plus** a
  redistributable fixture or a precise decode spec. Register-decode
  unit-tested, and boot-smoked / oracle-gated wherever a free fixture exists.
  Full accuracy citizens.
- **`BestEffort`** — long-tail families ported from reference emulators with
  **no redistributable test fixture**. Register-decode unit-tested only, and
  **explicitly excluded** from the AccuracyCoin / oracle gate.

The tier is an **honesty marker, not a behavioural one**: a mapper's runtime
behaviour, determinism, and save-state round-trip are identical regardless of
tier. The tier records only how much external evidence backs its correctness.

The load-bearing invariant — *no `BestEffort` mapper may back a ROM in the
accuracy oracle corpus* — is enforced on two levels:

1. **At the classifier (in-repo, runs in CI):** `BestEffort` is structurally
   never accuracy-gated (`MapperTier::is_accuracy_gated()` returns `false` for
   it, asserted by `best_effort_is_not_accuracy_gated`), the three tier id-sets
   are pairwise disjoint (`tiers_are_pairwise_disjoint`), and `parse()` /
   `mapper_tier()` are kept in lockstep (every parseable id has a tier; an
   unsupported id has none), guarded by per-tier classification tests.
2. **At the oracle corpus (by construction):** the byte-identical commercial
   oracle (the snapshot-backed `external_extended` / `external_real_games`
   suites) references only a curated, licensed-game ROM set — all Core/Curated
   mappers. The committed *coverage* ROMs (e.g. the `holymapperel` boards) may
   exercise `BestEffort` mappers on purpose; they are boot/coverage tests, not
   byte oracles, so they do not constrain the accuracy claim.

A per-oracle-ROM tier assertion in the (local-only, `commercial-roms`-gated)
oracle loader is a noted refinement — the oracle ROMs are not redistributable,
so it cannot run in headless CI.

The tier is also surfaced to the user (the mapper debug panel badges the loaded
game's tier) and to the docs (`docs/mappers.md` + `docs/STATUS.md` carry a Tier
column and an honest top-line split between accuracy-gated and best-effort
counts).

## Options considered

1. **One undifferentiated mapper count.** Simplest, but dishonest — it would let
   an unverified pirate board inflate the same number that carries the
   AccuracyCoin guarantee. Rejected: it erodes the project's central claim.

2. **Infer the tier from the `parse()` match arms / module comments.** Avoids a
   second list, but makes the "best-effort, not accuracy-gated" status a prose
   convention rather than a machine-checkable invariant — nothing stops an
   oracle ROM from mapping to an unverified board. Rejected.

3. **Refuse to add any board without a redistributable fixture.** Keeps the
   guarantee trivially, but abandons the maintainer's explicit goal of a broad
   long-tail sweep (the majority of remaining boards have no free fixture).
   Rejected.

4. **Explicit `const fn` classifier + CI invariant (chosen).** A single source
   of truth that the docs generator, the CI oracle-corpus gate, and the UI badge
   all read. Coverage can grow without bound while the accuracy claim stays
   precise and enforced.

## Consequences

- Coverage and accuracy are now **separately reported**: "X accuracy-gated
  families (Core + Curated) + Y best-effort families," never a single conflated
  number.
- Adding a `BestEffort` board is cheap (a `parse()` arm + a `tier.rs` arm +
  register-decode tests) and provably cannot weaken the oracle gate.
- A `BestEffort` board can be **promoted** to `Curated`/`Core` later by moving
  its id between `mapper_tier()` arms once a fixture or hardware test exists —
  the CI invariant then begins to enforce its accuracy.
- The classifier carries a `submapper` argument (unused today) so a future
  family with a best-effort submapper variant can be expressed without an API
  change.
- The invariant is **enforced by tests** in
  `crates/rustynes-test-harness/tests/mapper_tier_honesty.rs`: a headless
  (CI-runnable) check parses the byte-identity oracle test sources
  (`external_real_games.rs` / `external_extended.rs`) and asserts every
  `mapper-NNN-*` corpus dir they reference is accuracy-gated, plus a headless
  check on the committed `AccuracyCoin` / `nestest` ROMs; the
  `commercial-roms`-gated check walks the local oracle dumps and fails on any
  unclassified mapper (it reports `BestEffort` register-decode verification
  dumps, which are permitted in the tree but never wired to a byte-identity
  oracle).
