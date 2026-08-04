# Engineering Originality and Provenance

This document explains where RustyNES advances, diverges from, or independently
re-derives NES emulation technique; how the project was actually built (research
first, test-driven, measured); and how it treats the licenses of the reference
emulators and test ROMs consulted during development.

It is written to be **honest rather than triumphal**. RustyNES is not a clean-room
project that never looked at prior art, and it is not a fork or a translation of
another emulator either. It is an independent implementation whose *architecture*
and *engineering method* are its own, and which incorporates a small number of
clearly-attributed components from permissively-licensed projects while using
copyleft-licensed emulators only as behavioral oracles. The sections below spell
out exactly which is which, with file-level and ADR-level citations so the claims
can be checked against the tree.

Authoritative companions to this document: `docs/STATUS.md` (per-suite pass
counts and the mapper matrix), `CHANGELOG.md` (user-visible history), `docs/adr/`
(the decision record), `NOTICE` (the legal attribution file), and
`tests/roms/LICENSES.md` (test-ROM provenance).

---

## 1. Thesis: an independent build with attributed borrowings

The honest claim RustyNES can make is not "no line resembles any other emulator."
It is this:

- **The architecture is original.** The scheduler substrate, the ownership model,
  the crate/dependency topology, the determinism contract, the accuracy-honesty
  gates, and the save-state schema discipline are RustyNES's own design decisions,
  recorded as ADRs and implemented in its own `#![no_std]` Rust idiom.
- **The engineering method is original and auditable.** Behaviors are implemented
  from public hardware documentation, pinned to public test ROMs first, and every
  performance change is measured — including the ones that were measured and
  *rejected*. The discipline is machine-checked in CI, not asserted in prose.
- **Specific algorithms are deliberately, transparently borrowed** from
  permissively-licensed projects (TriCNES, emu2413, rcheevos), each attributed in
  source and in `NOTICE` under its MIT license.
- **Copyleft-licensed emulators were used only as oracles** — to observe and
  cross-check documented hardware behavior — never as a source of copied code.

Put differently: RustyNES's originality lives less in any single novel algorithm
(most hardware behaviors are, by definition, shared by every accurate emulator)
and more in the *system* that produces and guarantees that accuracy. That is the
claim the rest of this document substantiates.

**A note on AI assistance.** RustyNES is heavily AI-assisted software: much of it
was produced with LLM tooling under a human-directed, test-driven workflow, with
public test ROMs as the oracle, a `no_std` core as a hard baseline, and continuous
CI as the gate. That is disclosed plainly here and in the README because it belongs
in an honest provenance record — and because the licensing lapses this document
corrects (comments that called hardware-behavior implementations "ports" of
copyleft emulators) are exactly the kind of mistake AI-assisted authoring is prone
to. The remedy is the same either way: audit against the sources, attribute
accurately, and let the machine-checked gates — not the prose — carry the accuracy
claims.

**Not a superiority claim.** Nothing here asserts that RustyNES is "better" than
the emulators that came before it. Where this document compares RustyNES to a
reference, the comparison is exactly that — a comparison against a project RustyNES
was measured against — and every accuracy figure is independently checkable by
running the public suites (see the README Acknowledgments for the references and
components the project builds on).

---

## 2. Where RustyNES advances or diverges from prior art

Each subsection names the mechanism, the measurable result where one exists, the
governing ADR, and — where relevant — the specific reference emulator RustyNES
agrees or disagrees with.

### 2.1 The one-clock, every-cycle-bus-access timebase (ADR 0029)

Most NES emulators either batch subsystem work per scanline/instruction (fast,
less accurate) or run a multi-counter dot-lockstep (accurate, complex). RustyNES's
v2.0.0 "Timebase" rewrite collapses scheduling to a **single canonical cycle
counter** in which *every* CPU cycle is a real bus access, and PPU catch-up is
split around that access via paired `start_cycle` / `end_cycle` hooks. This makes
sub-instruction PPU state visible to the very next CPU read without per-quirk
patches — mid-scanline scroll writes, a sprite-zero hit at a precise dot, an MMC3
IRQ at PPU dot 260 all fall out of the model rather than being special-cased.

The structural choice mirrors Mesen2's cycle-stepped approach conceptually, but
the implementation, the counter model, and the split-around-access hook design are
RustyNES's own (`crates/rustynes-core`, `docs/scheduler.md`). It is a deliberate
MAJOR-boundary change: the old five-counter dot-lockstep scheduler was retired
outright, and the save-state / movie formats broke by design (see 2.9 and ADR
0028). See ADR 0029 for the full rationale.

### 2.2 The 2-cycle-ALE octal-latch PPU fetch: an independent, transistor-literal model (ADR 0030)

This is a clear example of independent, evidence-led accuracy work. The PPU
multiplexes its low VRAM address pins with the data pins; an external
74LS373-class octal latch captures the low address bits on the address-latch-enable
(ALE) half of each two-cycle VRAM access, and the PPU drives only the high bits on
the read half. When those halves desync (a mid-fetch `$2006` update, or a `$2007`
read overlapping the fetch cadence), the PPU reads a "hybrid" address it never
coherently drove.

Two AccuracyCoin tests ("ALE + Read", `$0491`; "Hybrid Addresses", `$0492`)
exercise exactly this, and RustyNES passes both by modeling the octal latch
explicitly. The instructive part is *how the references differ* (ADR 0030):
Mesen2 also passes these tests, but via a persistent internal bus-address
abstraction rather than a literal latch; higan and ares, by contrast, genuinely
fail them (higan blocks `$2007` during rendering and models no bus latch; ares
does not implement the `$2006` hybrid corruption). RustyNES deliberately took the
transistor-literal modeling approach of TriCNES — the die-level emulator by the
AccuracyCoin author — over the higher-level abstraction, because a physical
octal-latch model is what makes the hybrid-address cases fall out of the design
rather than being special-cased. It promoted the 2-cycle-ALE fetch to the
unconditional default in v2.0.3 (both prior experimental flags retired). See ADR
0030 for the campaign audit. This is independent modeling, not copying: RustyNES
re-derived the physical mechanism from die-level evidence, converging with some
references and diverging from others on the strength of the hardware model rather
than by following any single one of them.

**An honest caveat on the calibration (added v2.2.6).** The framing above understates
one dependency, and a NESdev reviewer (Fiskbit) was right to flag it. Beyond using
TriCNES as a pass/fail oracle for the two AccuracyCoin tests, RustyNES calibrated the
octal-latch *timing itself* against TriCNES's per-dot trace — specifically the
delayed-`CopyV` countdown (`COPY_V_DELAY = 4`), tuned to match TriCNES rather than
derived from an independent hardware measurement. That went beyond black-box oracle
use: it is behavioral calibration to one specific emulator's model. The consequence is
concrete — TriCNES's hybrid-address handling was itself imperfect (it has since been
revised upstream), and RustyNES inherited a matching artifact that mis-renders games
performing mid-render `$2006` writes (e.g. **Rad Racer**'s road/horizon split). This is
disclosed here rather than glossed. The **v2.3.0 "Datum II"** release reworks the
hybrid-address model to be derived from public hardware documentation and validated
against real-game behavior (Rad Racer) — not calibrated to any single emulator — behind
the project's standard default-off-flag / oracle-gated guardrails (see ADR 0030). No
TriCNES code was ever incorporated (it is MIT-licensed regardless); the issue was
behavioral fidelity, and the remedy is to make the behavior documentation-derived.

### 2.3 The sprite-evaluation FSM and OAM data bus (ADR 0034)

RustyNES models the PPU's sprite-evaluation datapath as an explicit per-dot state
machine (secondary-OAM clear at dots 1-64, evaluation at 65-256, sprite fetch at
257-320) plus an isolated OAM-data-bus model that reproduces what `$2004` returns
while the screen is drawn. A standing field-vs-schema audit (2.4) found that this
FSM state and the OAM data-bus latch were not fully serialized, which is what let
AccuracyCoin regress under run-ahead; serializing them (PPU snapshot version 8)
restored a full pass through run-ahead as well as without it. The model is
implemented from the NESdev-documented sprite-evaluation sequence; see ADR 0034.

### 2.4 Machine-checked accuracy honesty: mapper tiering and schema audits (ADR 0011)

Rather than claim uniform accuracy, RustyNES classifies every mapper family into
**Core / Curated / BestEffort** tiers and enforces, via a CI honesty gate, that
the suite cannot advertise support or accuracy it does not actually verify against
a test ROM or oracle. As of the v2.2.x line this covers 172 mapper families across
the three tiers (see `docs/STATUS.md` for the current split and the authoritative
counts). A second machine check, `snapshot_schema_audit`, parses the emulator's
live struct fields and fails the build if any new stateful field is not covered by
the save-state schema — the mechanism that mechanically surfaced the gap in 2.3.
Honesty here is a build gate, not a promise. See ADR 0011.

### 2.5 Determinism as a hard contract (the `#![no_std]` core)

The chip stack (`rustynes-{cpu,ppu,apu,mappers,core}`) is `#![no_std]` +
`extern crate alloc`, with a strictly one-directional dependency graph in which the
Bus owns all mutable subsystems and each chip borrows the narrowest trait it needs.
The contract is exact: same seed + ROM + input sequence yields a bit-identical
framebuffer and audio stream. Power-on CPU/PPU phase alignment is drawn from a
seeded PRNG and preserved across reset, save-state, TAS replay, and netplay
rollback. Wall-clock, OS RNG, thread scheduling, and unordered-map iteration are
kept out of the core by construction. This is what makes the entire test and
regression apparatus meaningful, and it is enforced by the `no_std` cross-compile
job (`thumbv7em-none-eabihf`, no default features) in CI. See
`docs/architecture.md`.

### 2.6 Measure-first performance, including documented rejections

RustyNES treats performance as an accuracy-subordinate, evidence-gated activity: a
change is adopted only if it is Criterion-stable above a threshold **and** proven
byte-identical by the differential net, and it is documented in `docs/performance.md`
*whether or not it cleared the bar*. Concrete outcomes:

- The specialized fast PPU dot path was measured at roughly **-11.3%** frame time
  on a rendering-heavy workload (clean-host Criterion, v2.2.3), differential-tested
  bit-identical every frame, and only then promoted to the default and exposed to
  users.
- Two optimizations were **measured and rejected with their numbers**: an
  `emit_pixel` bounds-check elision made the shipped default *slower*
  (+4.32% / +3.35% on the fast workloads, p <= 0.02), and a `cpu_clock`
  micro-optimization was capped at <= 1.9% with the textbook wins already in place.
- Release builds ship PGO-optimized Linux binaries only when the >3%-and-byte-
  identical gate passes; a same-runner relative frame-time regression gate closes a
  hole the deliberately-loose absolute ceiling left open.

Publishing rejected optimizations with p-values is unusual and is itself a form of
originality: the record shows the discipline, not just the wins. See
`docs/performance.md`.

### 2.7 Signal-level video and expansion-audio calibration

RustyNES includes a raw NTSC composite signal-decode path (`rustynes-ppu::raw_signal`)
feeding a naga-validated WGSL CRT-shader stack, and a decibel oracle that asserts
measured expansion-audio channel levels against hardware / Mesen2 targets (which,
for the Sunsoft 5B, required widening the mapper audio-mix path to `i32` to
represent full-scale tone without overflow). The base 2A03 NTSC output remains
byte-identical across these additions. See `docs/performance.md`, `docs/ppu-2c02.md`,
and the audio expansion oracle in `crates/rustynes-test-harness`.

### 2.8 Rollback netplay kept out of the deterministic core

Netplay's dynamic rate control, run-ahead, and snapshot-restore orchestration live
entirely in the frontend; the core's synthesis never sees them. This is what lets
the same deterministic core serve save-states, TAS replay, and rollback netplay
without any of them perturbing byte-identity. Keeping timing jitter and rate
control at the frontend boundary — never in the core — is a deliberate ownership
decision (`docs/frontend.md`, `docs/architecture.md`).

### 2.9 Explicit, versioned save-state schema (ADR 0028)

Save-state and movie formats carry explicit version epochs. A pre-v2.0.0 slot
fails to load with a clear error rather than silently misinterpreting stale bytes,
and additive schema growth (e.g. the PPU snapshot version 8 tail in 2.3) upconverts
older blobs where compatible. The one intentional format break is the v2.0.0
MAJOR boundary; see ADR 0028.

---

## 3. How the project was built

RustyNES did not begin as a copy to be modified. Its development record shows a
research-first, test-driven, verify-last cadence, and — importantly for the "not a
port" claim — the emulation core was **replaced wholesale** partway through the
project rather than incrementally grown from a single seed.

**Research before code.** The `ref-docs/` tree holds an immutable hardware and
emulation reference corpus (a 60-plus-source research report plus a set of
emulator technical studies). Behaviors were specified against this documentation
and against public test ROMs before implementation. Corrections to the corpus land
as new dated supplements, never in-place rewrites, so the research record stays
auditable.

**Test-as-spec.** For accuracy work the failing test-ROM expectation is pinned
first, then code is written until it passes; where the prose docs and a passing
test ROM disagree, the ROM wins and the docs are corrected. The suites in
`tests/roms/` (blargg, kevtris, mmc3_test_2, AccuracyCoin, and others) are treated
as the closed-form definition of "cycle-accurate."

**A documented lineage, honestly labeled.** The current core is a synthesis, cut
as v1.0.0 on 2026-06-13 (`docs/v1.0.0-synthesis-handoff-2026-06-13.md`), that
replaced the earlier v0.8.x emulation core with a cycle-accurate engine developed
through documentary stages v0.9.0-v0.9.7. Two cautions are recorded so the history
is not misread:

- The engine lineage carries its own internal "v1.x / v2.x" accuracy milestones
  that are *not* RustyNES release versions; they are folded into the v0.9.x stages
  and shipped as the v1.0.0 production core.
- Consequently, **two distinct "v2.0"s exist and must not be conflated**: the
  engine-lineage master-clock work (which shipped *as* the v1.0.0 core), and
  RustyNES's own **v2.0.0 "Timebase"** release (2026-07-03), which *replaces* that
  same dot-lockstep scheduler with the one-clock model of 2.1.

**Then continuous, gated deepening.** After v1.0.0 came the platform ports
(Android, iOS, the libretro/RetroArch core), the v2.0.0 Timebase rewrite, and the
v2.1.x "Fathom" accuracy line capped by the v2.2.0 "Capstone" milestone — each
release additive or default-off on the shipped core, verified NTSC-byte-identical
(AccuracyCoin 141/141) except where a break was explicitly announced (v2.0.0). The
decision record for all of this is `docs/adr/` (0001 through 0034 as of writing),
backed by over a hundred implementation-audit logs under `docs/audit/` (about
113 at time of writing). The
current release is v2.2.5 "Colophon" (this release); `docs/STATUS.md` is the source of truth for
per-suite counts.

---

## 4. Independence: oracle versus port

The distinction that matters for the "not just a port" question is **how** each
reference was used. RustyNES's sources fall into three categories, and the source
tree is written so a reader can tell which applies at any given site.

1. **Implemented from public hardware documentation.** The overwhelming majority
   of chip, mapper, and peripheral behavior is written from the NESdev wiki,
   Disch's mapper write-ups, published datasheets (e.g. the Xicor/Intersil I2C
   serial EEPROMs, the Yamaha YM2413), the documented 6502 unofficial-opcode
   behavior, and the Visual 6502 / Visual 2C02 die studies — then pinned to public
   test ROMs. Hardware behavior is factual; every accurate emulator necessarily
   agrees on it.
2. **Ported from a permissively-licensed project, with attribution.** A small,
   named set of components is genuinely incorporated as a Rust port under a
   compatible (MIT) license — principally TriCNES (the PPU address/data-multiplex
   and OAM-corruption models; see `crates/rustynes-ppu/src/ppu.rs`), the emu2413
   OPLL synthesizer for VRC7 audio, and the rcheevos RetroAchievements runtime.
   Each carries an in-source attribution and a `NOTICE` entry (Section 5.3).
3. **Consulted only as a behavioral oracle.** Copyleft-licensed emulators
   (Mesen2/MesenCE and higan and GeraNES under GPLv3; FCEUX, Nestopia UE, and
   puNES under GPLv2) — plus ares (ISC) — were run to observe and cross-check
   documented behavior when test-ROM results were ambiguous. No code from any of
   them is incorporated.

The octal-latch work in 2.2 illustrates the difference between categories 2 and 3:
RustyNES took TriCNES's transistor-literal *modeling approach* for the ALE fetch
(a permissively-licensed influence) while treating Mesen2, higan, and ares purely
as oracles to check the result — passing `$0491` / `$0492` where higan and ares
fail, and by a more physical model than Mesen2's abstraction. That is independent
modeling, not copying.

**A note on the provenance record.** The in-source provenance comments were
audited to make sure they accurately reflect the categories above. A number of
comments in the shipping crates had described hardware-behavior implementations
(CPU unstable stores, the PPU sprite-evaluation and OAM models, and numerous
mapper register decoders) as "ports of" a copyleft reference — Mesen2 (GPLv3), or
FCEUX / puNES (GPLv2) — which overstated the relationship for behaviors that are,
in fact, implemented from public hardware documentation. Those comments were
corrected to cite the public hardware source and to record the copyleft emulator
as a behavioral cross-check rather than a code source; GeraNES (GPLv3) was added
to the disclosed oracle set; and `NOTICE` was extended to state the oracle-versus-
incorporated posture explicitly and to reproduce the MIT notices for the
incorporated components (Section 5.3). These corrections changed only comments and
the attribution file; the emulator's behavior is byte-identical, re-verified
against AccuracyCoin (141/141, including run-ahead), the nestest golden log
(0-diff), and the dual-path differential net. The video shader stack and the
NTSC-decode filters are a separate provenance matter, addressed in Section 5.6.

---

## 5. License compliance

### 5.1 RustyNES's own license

RustyNES is dual-licensed **MIT OR Apache-2.0** (author: DoubleGate), the
conventional permissive dual-license for the Rust ecosystem. This choice is
deliberately compatible with the permissively-licensed components it incorporates
and deliberately does *not* subject the project to the copyleft terms of the
reference emulators it merely consulted.

### 5.2 Reference emulators: oracle use, not code reuse

The projects below were used only as behavioral oracles / accuracy references. No
source code from any of them is incorporated into RustyNES; this is stated in
`NOTICE` and reflected in the in-source comments (Section 4).

| Reference emulator | License | Use in RustyNES |
| --- | --- | --- |
| Mesen2 / MesenCE | GPLv3 | Behavioral oracle / accuracy cross-check only |
| higan | GPLv3 | Accuracy reference for scheduler structure |
| ares | ISC | Accuracy reference for scheduler structure |
| GeraNES | GPLv3 | Behavioral oracle / cross-check for several mapper boards |
| FCEUX | GPLv2 | Behavioral oracle for legacy-compat behaviors |
| Nestopia UE | GPLv2 | Behavioral oracle |
| puNES | GPLv2 | Behavioral oracle |

Using a GPL-licensed program to *observe* hardware behavior, and then implementing
that publicly-documented behavior independently, does not create a derivative work
of that program. The point of the Section 4 audit was to make the source comments
say precisely that, so nothing in the tree could be read as claiming a copyleft
source was translated into this permissive project.

### 5.3 Incorporated third-party components (permissive)

These works are genuinely incorporated and are attributed in `NOTICE` with their
copyright notices and the MIT permission text:

| Component | License | Copyright | Where |
| --- | --- | --- | --- |
| emu2413 v1.5.9 | MIT | 2020 Mitsutaka Okazaki | `crates/rustynes-apu/src/opll.rs` (Rust port; VRC7 audio, ADR 0006) |
| TriCNES (commit 9199870) | MIT | 2025 Chris Siebert | `crates/rustynes-{ppu,cpu,core}` (ported models) + vendored golden oracle |
| rcheevos v12.3.0 | MIT | 2018 RetroAchievements.org | `crates/rustynes-cheevos/vendor/rcheevos/` (optional `retroachievements` feature) |
| Font Awesome Free | its own license | Fonticons, Inc. | `crates/rustynes-frontend/assets/fonts/` (bundled glyphs) |

The emu2413 port is a pure-Rust port of the upstream MIT C source (ADR 0006),
distributed under that MIT license; the upstream MIT notice is now reproduced in
`NOTICE` as that file's own comment claims. TriCNES is both a ported source (its ALE/octal-latch,
OAM-corruption, and DMA-dispatch models) and a vendored golden oracle for the
tests it grounds. rcheevos is compiled only when the RetroAchievements feature is
enabled and keeps its own in-tree `LICENSE`.

### 5.4 Test ROMs

Every ROM committed under `tests/roms/` is a public-domain work released
specifically for validating NES emulators, catalogued per-author in
`tests/roms/LICENSES.md` (blargg's suites, kevtris/AccuracyCoin material, and
others). **No commercial Nintendo software is bundled**, and none ever should be;
users who want to test against commercial dumps they own place them in the
gitignored `tests/roms/external/`. The AccuracyCoin battery itself is MIT-licensed
(Chris Siebert / 100thCoin).

### 5.5 Vendored and immutable trees

RustyNES vendors several third-party source trees whose value depends on their
being byte-identical to upstream (the TriCNES golden oracle, the rcheevos runtime,
upstream test-ROM READMEs, and the `ref-docs/` / `ref-proj/` reference material).
These are protected from accidental reformatting: `.markdownlintignore` exempts
them from markdown linting, a shared `exclude` anchor in the pre-commit
configuration keeps the whitespace-rewriting hooks off content the project did not
author, and `ref-proj/` is gitignored while `ref-docs/` is treated as immutable
(corrections land as dated supplements). This preserves both the integrity of the
oracles and the upstream provenance of the vendored code.

### 5.6 Video shaders and NTSC-decode filters

The optional CRT shader stack (`crates/rustynes-gfx-shaders/`) and the NTSC-decode
filters (`crates/rustynes-frontend/src/ntsc_bisqwit.rs`, `ntsc_lmp88959.rs`)
reproduce the *look* of well-known community shaders and filters — CRT-Royale
(TroggleMonkey, GPLv2+), crt-guest-advanced (guest.r), Sony Megatron
(MajorPainInTheCactus), Bisqwit's NES composite model, and EMMIR's NTSC-CRT
(permissive). These were reviewed at the source level. Each is a single
fullscreen pass built on RustyNES's own uniform / pipeline conventions and is
structurally incompatible with being a translation of the upstream *multi-pass*
shader source. Because copyright protects code expression — not a visual look or
a rendering technique — these are independent reimplementations, not derivative
works of the upstream code, even where an upstream is copyleft; no upstream
shader source is incorporated. The one comment that had implied otherwise (an
NTSC filter reading "ported verbatim from Bisqwit's C ... as implemented by
Mesen2") was corrected: those tables encode the two-level NES composite signal
documented at the NESdev wiki ("NTSC video") — a hardware model, not copied code.
The in-source comments were reworded accordingly, and `NOTICE` now credits each
project as a "visual influence, independently reimplemented (no code
incorporated)". All of these features are optional and default-off; none affects
the deterministic emulation core, its `AccuracyCoin` results, or the base NTSC
framebuffer, which are unchanged.

---

## 6. Conclusion

RustyNES is an independent emulator, not a port. Its scheduler, ownership model,
determinism contract, accuracy-honesty gates, and measured-performance discipline
are its own, recorded as ADRs and enforced in CI rather than asserted. Where it
borrows, it borrows narrowly and openly, under compatible permissive licenses,
with attribution in both source and `NOTICE`. Where it consulted copyleft
references, it used them as oracles to check publicly-documented hardware behavior,
and — as the octal-latch case shows — it was willing to disagree with a leading
reference when the transistor-level evidence pointed the other way.

The strongest evidence for originality is not any single clever routine; it is the
system that surrounds every routine: research before code, a failing test pinned
first, a hard byte-identity contract, honesty gates that fail the build rather than
the reader, and a decision record that documents the rejections alongside the wins.
That system is what makes RustyNES's accuracy claims checkable — and it is what
this project built for itself.
