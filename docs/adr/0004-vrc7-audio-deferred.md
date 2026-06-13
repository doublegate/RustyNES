# ADR 0004 — VRC7 FM Audio Deferred to v1.x

**Status:** Superseded by [ADR 0006 — VRC7 OPLL FM Audio Landed
(via emu2413 Rust Port)](0006-vrc7-audio-landed.md) on 2026-05-25
(v1.1.0). The deferral decision applied to v1.0.0 only; v1.1.0
lands the actual FM synthesizer via a clean-room Rust port of
`emu2413 v1.5.9` (MIT). The historical content below is preserved
for the v1.0.0-tag audit trail.
**Date:** 2026-05-13
**Author:** RustyNES v2 maintainers
**Supersedes:** None.
**Superseded by:** ADR 0006 (2026-05-25).
**Relates to:** ADR 0003 (Save-state Cross-version Migration Policy) for
the version-bump policy referenced below; Track C2 / Phase 2.4 of the
v1.0.0 roadmap.

---

## Context

VRC7 is the audio-bearing Konami VRC variant assigned iNES mapper id 85.
Beyond the standard VRC banking + IRQ surface (PRG 3 × 8 KiB switchable
+ 1 fixed; CHR 8 × 1 KiB switchable; CPU-cycle IRQ counter identical to
VRC6's), the chip carries an on-cart **Yamaha YM2413 OPLL-derived FM
synthesizer**: 6 channels of 2-operator FM, a fixed 15-entry custom
instrument ROM (different from the YM2413's), and a 49,716 Hz update
rate driven from the chip's 3.58 MHz ceramic oscillator. The chip's
audio surface is two write-only registers: `$9010` latches an OPLL
register address and `$9030` writes data to the previously-latched
address.

Exactly **one** commercial title uses VRC7 audio: *Lagrange Point*
(Konami, 1991, Japan only). No other VRC7 cart ships any audio data.

The plan (`linked-puzzling-sutherland.md` Phase 2.4) called for wrapping
an existing OPLL crate rather than rolling a hand-built FM synthesizer.
Wrapping was estimated at 1-2 days; a hand-built implementation is
weeks of signal-processing work.

## Audit of available OPLL crates

Audited 2026-05-13 against six selection criteria: **permissive license
(MIT/Apache-2.0/BSD/ISC/zlib)**, **active maintenance (≤ 18 months since
last commit)**, **no_std friendly**, **VRC7-instrument-ROM awareness**,
**ergonomic API**, **determinism**.

| Crate | Source | License | Last commit | no_std | VRC7 | Verdict |
|-------|--------|---------|-------------|--------|------|---------|
| `opl-emu` | crates.io 0.4.2 | **GPL-3.0** | 2024 | partial | OPL2 only | **Disqualified** (copyleft license; OPL2 not OPLL) |
| `opl3-rs` | crates.io 0.2.3 | **LGPL-2.1** | 2024 | bindings | OPL3 only | **Disqualified** (copyleft license; OPL3 not OPLL) |
| `Nuked-OPLL` | github (nukeykt) | **GPL-2.0** | C source | n/a | yes | **Disqualified** (copyleft license; C source, no Rust port) |
| `emu2413` | github (digital-sound-antiques) | MIT | active C | n/a | yes | **No Rust port published.** Hand-port (~3 KLOC of dense C) would be weeks of work, not 1-2 days. |
| `VRC7-Sound` | github (0xJonas) | MIT | active C | n/a | yes (only VRC7) | **No Rust port published.** Same hand-port concern. |
| `ymfm-rs` / `kkw-opll` | crates.io / github | not published | — | — | — | **Do not exist** as searchable crates. |

`cargo search ym2413 opll vrc7 ymfm` returns zero relevant Rust packages.
Web search returns the same C-source projects above.

**Result:** there is no published Rust OPLL crate that meets the
permissive-license requirement. Of the two MIT-licensed C reference
implementations (`emu2413`, `VRC7-Sound`), porting either to Rust is
multi-week signal-processing work — not the 1-2 day integration the
plan assumed. Building wrapper bindings to C via `bindgen` + linking
the C source would meet the license bar but introduces a C build
dependency the rest of the chip stack (post-Track-C5 `no_std + alloc`,
buildable on `thumbv7em-none-eabihf` with zero C deps) does not have.

## Decision

**Defer VRC7 FM audio to v1.x.**

The base VRC7 mapper (PRG banking + CHR banking + mirroring control +
CPU-cycle IRQ counter) lands in this commit so that mapper 85 ROMs
load and run with banking + IRQ correctness — they will simply be
silent on the audio register surface.

The audio register surface itself (`$9010` address latch + `$9030`
data write) is **decoded and latched** into a small `Vrc7AudioRegs`
struct even with the audio synthesizer absent. This:

- Preserves the contract that "save-state round-trip across builds
  with/without audio implementations works" (consistent with VRC6 /
  Sunsoft 5B / Namco 163 / MMC5 audio surfaces).
- Means a future v1.x commit that lands the actual FM synthesizer only
  needs to call `Vrc7AudioRegs::output_sample()` from `Mapper::mix_audio`
  and `Vrc7AudioRegs::clock()` from `Mapper::notify_cpu_cycle` —
  it does not need to touch the banking / IRQ / save-state layout.
- Means the AccuracyCoin glyph-decoder and other deterministic tests
  see the same byte sequences on banking and IRQ paths as a future
  audio-enabled build would, on identical input.

`Mapper::mix_audio` returns 0 (silence) for VRC7 unconditionally,
whether or not the `mapper-audio` cargo feature is on. The feature gate
remains correct: with `mapper-audio` off, no other mapper produces
audio either.

The save-state format for VRC7 is version-tagged at `1`. When a future
v1.x commit adds the actual FM synthesizer state, the version bumps to
`2` with v1 backcompat per ADR-0003 (append the OPLL state fields at
the end of the body; v1 blobs default-initialize the audio state to
silent on load).

## Consequences

### Positive

- v1.0.0 ships with mapper 85 loading correctly: banking + IRQ work,
  the cart's title screen and gameplay logic run identically to a
  reference emulator. Only the audio is missing.
- No GPL contamination of the workspace. The repository keeps its
  permissive (MIT-or-Apache-2.0) license shape.
- No C build dependency added. The `no_std + alloc` migration (Track
  C5) stays clean — `cargo build -p nes-core --target
  thumbv7em-none-eabihf --no-default-features` continues to work.
- The audit table above documents the rationale so a future
  contributor does not re-evaluate the same six crates and reach the
  same dead-end independently. A new Rust OPLL crate appearing on
  crates.io that meets the criteria is the trigger to revisit.

### Negative / Costs

- *Lagrange Point* — the only commercial title affected — plays
  silently on RustyNES v1.0.0. The game is fully playable; the
  in-game music and SFX are absent. This is a documented user-visible
  gap in `docs/compatibility.md` and `docs/STATUS.md`.
- v1.0.0 does not ship a complete `mapper-audio` family even though
  the feature flag exists. Documented in the same places.

### Neutral

- VRC7 mapper 85 is now usable for non-audio workloads (testing
  banking; running gameplay logic; debugging non-audio bugs).

## Alternatives considered

1. **Hand-port `emu2413` (MIT C) to Rust.** Rejected: estimated
   2-3 weeks of dense signal-processing work plus a large equivalence-
   testing harness against the C reference; not the 1-2 day
   integration the plan assumed; and any FM-synthesis bug shipped in
   v1.0.0 would be a regression risk against the cycle-accuracy bar
   (Mesen2 / higan / ares). The plan explicitly says: "Stop and report
   if the candidate crate quality is poor — fall back to 'VRC7
   deferred to v1.x' rather than ship buggy FM." That contingency
   applies here.

2. **Wrap the C source via `cc` + `bindgen`.** Rejected: introduces a
   C compiler dependency to a workspace that has none, breaks the
   `thumbv7em-none-eabihf` `no_std` cross-compile (the C source uses
   `<math.h>` and `<stdlib.h>`), and shifts test scope to also cover
   the C-Rust FFI boundary. The bench/test corpus for the chip stack
   assumes pure-Rust determinism.

3. **License a GPL OPLL crate (`Nuked-OPLL`, `opl-emu`, `opl3-rs`).**
   Rejected: the repository ships under permissive license terms; a
   copyleft-licensed crate inside the workspace would force the entire
   workspace to GPL/LGPL terms downstream.

4. **Ship VRC7 mapper 85 entirely unimplemented (return
   `RomError::UnsupportedMapper(85)`).** Rejected because mapper 85's
   banking + IRQ are well-documented, identical to VRC6's IRQ counter,
   and trivial to implement. Loading the ROM correctly with silent
   audio is strictly better than refusing to load — most of the game
   is playable.

## Re-evaluation triggers

Revisit this ADR (and land VRC7 audio under `mapper-audio`) when **any
one** of the following becomes true:

- A pure-Rust OPLL/VRC7 crate is published to crates.io under MIT,
  Apache-2.0, BSD, ISC, or zlib license, with VRC7 instrument-ROM
  awareness and ≤ 18 months since last commit.
- A maintainer commits to a hand-port of `emu2413` to Rust as a
  separate crate (multi-week effort) plus the equivalence-testing
  harness; the resulting crate is then vendored / wrapped here.
- A maintainer commits to the `bindgen` + C-source-vendor approach
  with full understanding of the `no_std` cross-compile regression
  it implies (and a chosen workaround: e.g., feature-gating VRC7
  audio behind `std` only, leaving `no_std` builds without it).

Until any of those happens, VRC7 stays silent — banking + IRQ only.

---

## Implementation notes (for the future v1.x commit)

When VRC7 audio is implemented, the integration points are:

1. **`Vrc7::clock_audio`** — called once per CPU cycle from
   `notify_cpu_cycle`. Already wired (currently a no-op when audio is
   deferred). The chip's 3.58 MHz oscillator runs slightly faster than
   the NES CPU's 1.789773 MHz (the ratio is ~2:1); decimation /
   resampling to one sample per CPU cycle is the integration point's
   responsibility.
2. **`Vrc7::mix_audio`** — returns the OPLL's current i16 sample,
   scaled to the same per-mapper amplitude budget as VRC6 / Sunsoft 5B
   / Namco 163 / MMC5 (peak ~5,000 with ~5× APU headroom in i16).
3. **Save-state** — bump version `1 → 2` per ADR-0003, append the
   OPLL state fields at the end of the body. v1 blobs default-load
   the audio state to silent.
4. **Test coverage** — at minimum mirror the VRC6 audio test pattern:
   register-latch tests, channel-volume decode, custom-instrument
   bytes routing, `mix_audio` silence-when-idle, save-state v1
   backcompat, `mapper-audio` feature-OFF path returns 0.
