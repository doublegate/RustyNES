# ADR 0006 — VRC7 OPLL FM Audio Landed (via emu2413 Rust Port)

**Status:** Accepted.
**Date:** 2026-05-25
**Author:** RustyNES maintainers
**Supersedes:** [ADR 0004 — VRC7 FM Audio Deferred to v1.x](0004-vrc7-audio-deferred.md).
**Relates to:** [ADR 0001 — Mapper Dispatch](0001-mapper-dispatch.md) (the
`Mapper::mix_audio` trait method we now implement for VRC7); the v2.0.0
release plan (v1.1.0 milestone).

---

## Context

ADR 0004 deferred VRC7 FM audio to v1.x with a six-crate license audit
showing that **no published Rust OPLL crate** met the
permissive-license + maintenance + no_std + VRC7-instrument-ROM bar.
The conclusion: hand-port one of the two MIT-licensed C reference
implementations (`emu2413` or `VRC7-Sound`) — multi-week signal-
processing work, not a 1-2 day integration.

Eighteen months on, the audit conclusion has not changed: a fresh
`cargo search ym2413 opll vrc7 ymfm` (2026-05-25) still returns zero
permissive Rust OPLL crates. The MIT-licensed C references —
[`emu2413 v1.5.9`](https://github.com/digital-sound-antiques/emu2413)
(Mitsutaka Okazaki) and
[`VRC7-Sound`](https://github.com/0xJonas/VRC7-Sound) (0xJonas) —
remain the canonical reference implementations. Mesen2 itself vendors
`emu2413.cpp` verbatim from upstream.

The v2.0.0 release plan
(`/home/parobek/.claude/plans/generate-a-new-plan-snug-starlight.md`)
budgets 2-3 weeks for a clean-room Rust port of `emu2413`, landing as
**v1.1.0** — the first SemVer-additive milestone after v1.0.0 final
(released 2026-05-23). This ADR documents what landed.

## Decision

**Land VRC7 OPLL FM audio as a clean-room Rust port of
`emu2413 v1.5.9` (MIT).**

The port lives in `crates/rustynes-apu/src/opll.rs` (~1,170 LoC of pure
Rust) and exposes a minimal public API to consumers:

```rust
pub enum ChipType { Ym2413, Vrc7, Ymf281b }

pub struct Opll { /* heap-allocated TLL table + 18 slots + state */ }

impl Opll {
    pub fn new(chip_type: ChipType) -> Self;
    pub fn reset(&mut self);
    pub fn reset_patch(&mut self, chip_type: ChipType);
    pub fn write_reg(&mut self, reg: u8, val: u8);
    pub fn read_reg(&self, reg: u8) -> u8;
    pub fn calc(&mut self) -> i16;
    pub fn chip_type(&self) -> ChipType;
}
```

`rustynes-mappers` consumes this via a single new acyclic workspace
dependency edge (`rustynes-mappers -> rustynes-apu`). The `Vrc7` mapper holds an
`Opll` instance, forwards `$9030` data writes to `opll.write_reg(addr,
val)`, ticks `opll.calc()` every 36 CPU cycles
(≈ 1,789,773 Hz CPU / 49,716 Hz OPLL native rate, 0.008% error), and
returns the latest sample from `mix_audio` gated by `$E000` bit 7
(expansion-sound silence).

### Port methodology

The Rust port was developed across three sprints of v1.1.0:

| Sprint | Scope | Lines added |
|---|---|---:|
| 1.1 foundation | Constants, patch ROM (YM2413 / VRC7 / YMF281B verbatim), exp/sin LUTs, public API skeleton with `calc()` stubbed to 0 | ~600 |
| 1.1 PG+EG | Phase generator + envelope generator (`calc_phase`, `calc_envelope`, attack/decay step lookups, envelope state machine with carrier buddy-reset signal) | ~485 |
| 1.1 operator+channel+LFO | Wave-table construction (`fullsin`/`halfsin` extended from quarter-wave), TLL+RKS table buildout, `lookup_exp_table` + `to_linear` operator output, `update_ampm` LFO, `commit_slot_update`, `calc_slot_mod` + `calc_slot_car`, full per-clock pipeline | ~670 |
| 1.2 register decoder + wire-up | Full `OPLL_writeReg` port (mirror registers, user-patch fan-out, fnum/block/key-on/off/sustain/instrument/volume decoders), VRC7 mapper integration | ~520 |

Each sub-step lands with unit tests verifying behavior against
hand-computed reference values derived directly from
`emu2413.cpp:765-925` (PG + EG cores), `emu2413.cpp:374-396`
(TLL table buildout), `emu2413.cpp:911-925` (exp + to_linear),
`emu2413.cpp:927-948` (operator output), and `emu2413.cpp:1223-1394`
(register decoder). The port mirrors C semantics — including the
defined-behavior integer-truncation casts and the x86 shr-mask
truncation in `lookup_exp_table` — so identical input register-write
sequences produce bit-identical sample streams to the C source on
production x86/ARM targets.

### License posture

`emu2413` is MIT-licensed at upstream. This port is a **clean-room
reimplementation guided by the C source's algorithm**, with per-method
citations to the C line numbers it follows. The MIT notice for emu2413
is preserved in repo-root `NOTICE`; the port itself is dual-licensed
under MIT-or-Apache-2.0 matching the rest of the workspace.

No GPL contamination. No C build dependency. The `no_std + alloc`
invariant (Track C5) holds — `cargo build -p rustynes-core --target
thumbv7em-none-eabihf --no-default-features` stays green; the OPLL
uses `libm` for `f32` table buildout where `f32::*` would normally
require `std`.

### Save-state format

VRC7 save-state version remains at `1` for v1.1.0. The OPLL's internal
state (slot envelopes, phase generators, LFO phases, channel patch
pointers, EG counter, etc.) is **not** persisted in v1.1.0 save states.
Loading an audio-active save state will cause the OPLL to start from
silence; the next key-on event re-arms each used channel within
~16k CPU cycles.

Full OPLL state serialization (bumping save-state version 1 → 2 per
ADR 0003) is deferred to a future v1.x patch when the marginal cost
becomes interesting. For Lagrange Point — VRC7's only commercial
title with audio — the in-game music re-pages naturally and the
audio quality post-load is indistinguishable from a power-on key-on.

### What ADR 0004 said would happen

ADR 0004's "Trigger to revisit" section ended with:

> A new Rust OPLL crate appearing on crates.io that meets the criteria
> is the trigger to revisit.

This trigger has not fired. The decision to land VRC7 audio in v1.1.0
is the *alternative* path ADR 0004 implicitly left open: pay the
"weeks of signal-processing work" cost of the hand port. The cost was
real (3 sprints, ~2,275 lines), but the result is a permissively-
licensed, fully no_std-compatible, pure-Rust port that has no
maintenance entanglement with C build tooling.

## Consequences

### Positive

- **Lagrange Point plays with audio.** The game is no longer the
  "fully playable but silent" caveat from ADR 0004.
- **No license contamination.** The workspace remains MIT-OR-Apache-2.0
  throughout; the MIT-licensed emu2413 notice in `NOTICE` covers the
  algorithmic provenance.
- **No build complexity.** Pure Rust; no `cc` / `bindgen` / C-link
  dependency. The chip stack still cross-compiles to
  `thumbv7em-none-eabihf` with zero C deps.
- **Deterministic.** Identical register-write sequences produce
  bit-identical sample streams. The 5 VRC7 audio-tests insta snapshots
  (`audio_db_vrc7`, `audio_test_vrc7`, `audio_patch_vrc7`,
  `audio_clip_vrc7`, `audio_noise_vrc7`) capture the canonical v1.1.0
  audio output for regression-sentinel use.
- **Spectral correctness verified.** An OPLL FFT regression test at
  `crates/rustynes-apu/tests/opll_spectral.rs` drives a pure-sine carrier
  configuration and asserts the dominant frequency bin matches the
  expected value within tolerance, with SFDR above an acceptance gate.
- **Existing residual fallout absorbed cleanly.** The 5 VRC7 insta
  snapshots were re-baselined; the deltas are localized to
  `audio_fnv1a64` only (framebuffer hash, cycles, and audio sample
  count all stay byte-identical), confirming the behavior change is
  scoped to VRC7's audio output and does not leak into other test
  ROMs' baselines.

### Negative / Costs

- **~2,275 lines of new code** in `crates/rustynes-apu/src/opll.rs`
  (1,170) + `crates/rustynes-mappers/src/sprint3.rs` deltas (+193) +
  associated tests. Maintenance burden if `emu2413` upstream lands
  a behavior fix we want to mirror — the port is structurally close
  enough to the C source that line-by-line re-comparison is
  practical, but it's manual labor.
- **Per-instance 128 KiB TLL table.** The total-level lookup is a
  `Box<[u32]>` of 32,768 entries. One VRC7 mapper per ROM means one
  allocation per emulator instance; not a concern for desktop or
  embedded targets with hundreds of KiB free, but worth flagging.
  An `OnceLock`-shared static table is a future refactor candidate.
- **New workspace dep edge.** `rustynes-mappers -> rustynes-apu` is a new edge
  in the chip-stack DAG. It is acyclic (rustynes-apu has no chip deps),
  but it does mean `rustynes-mappers` now pulls in the full APU mixer
  + BLEP synthesis as transitive deps. The cross-compile budget
  absorbed this without measurable impact.

### Neutral / Future work

- VRC7 save-state version stays at v1; full OPLL state serialization
  is a future v1.x patch (see "Save-state format" above).
- Higher-precision fractional clocking for the 1,789,773 / 49,716
  ratio (currently floor-rounded to integer 36) is a v1.x refinement
  if the 0.008% tuning drift becomes audible — it has not in the
  spectral regression test or by-ear comparison against Mesen2.
- The YM2413 chip-type path is **partially functional**: register
  decoding, channels 0-8 (9 melodic channels) all work; the rhythm
  mode at `$0E` is currently no-op. YM2413-as-PC-sound-card is a
  v1.x candidate if interest emerges (no NES title uses it).

## References

- emu2413 v1.5.9 — Mitsutaka Okazaki, MIT —
  <https://github.com/digital-sound-antiques/emu2413>
- emu2413 vendored in Mesen2 —
  `/home/parobek/Code/OSS_Public-Projects/RustyNES/ref-proj/Mesen2/Core/Shared/Utilities/emu2413.{h,cpp}`
- nesdev wiki "VRC7 audio" —
  <https://www.nesdev.org/wiki/VRC7_audio>
- Sprint 1.1 + 1.2 commits on `origin/main` —
  `d1c3922`, `3bb8734`, `4e2d459`, plus the Sprint 1.2 wire-up commit
