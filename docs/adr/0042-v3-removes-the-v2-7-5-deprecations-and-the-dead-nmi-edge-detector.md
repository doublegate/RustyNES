# 42. v3.0.0 removes the v2.7.5 deprecations and the dead NMI edge detector, and renames `LockstepBus`

Date: 2026-09-26

## Status

Accepted. Decided by the maintainer at v2.9.0, the point
[ADR 0041](0041-hardware-release-is-v3.0.0.md) reserved for it. Amends ADR 0041
item 4, whose "No save-state break is planned" no longer holds for the BUS
section. Leaves ADR 0003 and ADR 0028 in force: this is a MAJOR-release change of
the kind ADR 0003's policy already allows.

## Context

v2.7.5 "Tally" deprecated the dead surface the core audit located (core ledger
§3.1, §4.2-§4.4). Measured at v2.9.0:

- **19 items, all `since = "2.7.5"`**: 18 methods on `rustynes_cpu::Bus`
  (`crates/rustynes-cpu/src/bus.rs`: the `poll_nmi` / `poll_irq` family, the
  pre-v2.0.0 per-phase and DMA-step hooks, the DMC-overlap hooks) and the
  `rustynes_apu::ApuBus` trait. No `Bus` / `CpuBus` type aliases exist; the core
  ledger declined that suggestion.
- **Zero callers.** The only calls are two inside the trait's own default bodies.
  The 18 overrides (16 in `LockstepBus`, 2 in test stubs used only by ignored
  tests) are dead with them. `ApuBus` has no implementor.
- **No external surface.** No crate is published to crates.io, and none of the
  items is reachable through the libretro core, the UniFFI mobile bridge, the
  Lua API or the wasm build. They are nameable only as `rustynes_core::rustynes_cpu::Bus`
  and `rustynes_core::rustynes_apu::ApuBus`, through whole-crate re-exports.
- **A trial removal** in a scratch copy (6 files, about 510 lines, plus the
  54-line `dmc_dma_step_impl` that becomes dead) compiled and passed clippy, the
  no_std build, the chip tests, AccuracyCoin, nestest, the DMA pins, the
  save-state and visual-regression suites and the snapshot-schema audit. The
  full `test-roms` suite, the frontend feature clippy set and the wasm gates were
  not run on it.
- **The NMI edge detector is the entangled part.** `sample_nmi_edge` runs in the
  live PPU catch-up loop but feeds only the deprecated `poll_nmi`
  (`docs/performance.md` says to remove them together). Its two fields,
  `nmi_edge_latch` and `last_nmi_level`, are written into the `.rns` BUS section
  (`bus_snapshot.rs`, `BUS_SECTION_VERSION` 1). Removing it changes the save-state
  layout; removing only the methods would not.
- **Also dead and not deprecated**: `oam_dma_overlap_cycle`, whose only call site
  is inside a deprecated override, and the fields `dmc_step_was_get` (becomes
  write-only) and `dma_total` (read only by `oam_dma_overlap_cycle`). The v2.7.5
  count of "18 dead methods" was therefore a lower bound.

## Decision

**v3.0.0 removes all of it**: the 19 deprecated items and their overrides,
`dmc_dma_step_impl`, the NMI edge detector and its two fields, and the other dead
code the removal exposes. The version number is already paid for: v3.0.0 is
MAJOR under ADR 0041's new-deliverable-class trigger.

It is done at v3.0.0, not before: a v2.9.x release is MINOR, and removing public
items is an API break.

**`LockstepBus` is renamed in the same break** (maintainer decision, 2026-09-26,
raised by the v2.9.0 core re-audit). The name describes the pre-v2.0.0
dot-lockstep scheduler that ADR 0002 / ADR 0029 retired; AGENTS.md already has
to explain that it "predates v2.0.0". The new name is chosen at v3.0.0 (for
example `Bus` or `SystemBus`; `rustynes_cpu::Bus` is the trait it implements, so
a collision has to be avoided). It is one more item in a release that already
breaks the API, and it stops the type's name describing a design that no longer
exists.

## Consequences

- **An API break**, with a CHANGELOG migration note as `VERSION-PLAN.md`
  requires. The note is short, because nothing in the project called these items.
- **A BUS-section format change**: `BUS_SECTION_VERSION` 1 -> 2, the two fields
  gone. **How older `.rns` files load is decided at v3.0.0**, with this fact on
  the table: the two fields feed only the removed `poll_nmi`, so a v3.0.0 reader
  could accept a version-1 BUS section by reading and discarding them, losing
  nothing that affects emulation. The alternative is ADR 0003's clean rejection
  with a clear error, as v2.0.0 did (ADR 0028). Either way, emulation output must
  not change: the removed code is unreachable, and the gate is byte-identity on
  the goldens plus AccuracyCoin and nestest.
- **Docs to update at v3.0.0**: `docs/scheduler.md`, `docs/performance.md`,
  `docs/apu-2a03.md`, `AGENTS.md`'s Bus paragraph, the core ledger rows, and the
  reason strings in `snapshot_schema_audit.rs` -- and every mention of
  `LockstepBus`, which the rename touches across the workspace.
- **No provenance record changes**: none of the affected files carries a
  `// Provenance:` header.

## Amendment (2026-09-27, v2.9.1): the detector has a measured cost

The "unreachable, so emulation cannot change" argument above was about
correctness. v2.9.1 adds a cost: re-measured with the fixed `ab_check.sh`,
dropping the per-dot `sample_nmi_edge` call is **−4.1% to −4.7%** on both
palette workloads and **−0.7% to −1.3%** on `nestest`, in two independent runs
with order-bias controls within ±0.8% (`docs/performance.md` §v2.9.1). v2.7.6
had recorded this probe as "zero" with a tool that compared the old code with
itself.

The maintainer kept the removal at v3.0.0 rather than pulling the call's
removal into v2.9.1 (2026-09-27): doing it early would change what the
deprecated `poll_nmi` reports and what the two `.rns` fields hold in a MINOR
release. The decision above is unchanged. What changes is the gate at v3.0.0:
besides byte-identity, the removal should show this speed-up in an `ab_check.sh`
run, or the difference be explained.

## Amendment (2026-09-29, v2.9.3): `serialize_header` joins the removal list

v2.9.3 found that the header editor wrote headers with
`rustynes_mappers::serialize_header`, which encodes a `Header` from scratch and
so zeroes every bit `Header` has no field for (Vs. hardware types 1-4 and 6,
the extended console type, bytes 14-15, the NVRAM nibbles, iNES 1.0 bytes
8-15). The first fix added four public fields to `Header`. That is an API break
by `VERSION-PLAN.md`'s definition: `rustynes-core` re-exports the whole crate,
and a new field on a struct whose fields are all public breaks struct-literal
construction. The maintainer replaced it (2026-09-29) with the pattern this ADR
already applies. v2.9.3 adds `serialize_header_preserving(h, original)`, which
rewrites only the fields that changed, moves the editor to it, and deprecates
`serialize_header` with `since = "2.9.3"`.

Two items for v3.0.0, alongside the decision above:

- **Remove `serialize_header`**, and make `canonical_header` its private
  replacement where a from-scratch encoding is still wanted (the preserving
  path already uses it for a format change or an unparsable original).
- **Decide whether `Header` models the remaining bytes**: the Vs. hardware type,
  the extended console type, bytes 14-15 and the NVRAM split. If it does, mark
  it `#[non_exhaustive]` in the same break, so later fields stop being API
  breaks. The editor does not need them either way, since the preserving
  writer keeps what it does not model.
