# Audio provenance

**Status:** implemented in v2.3.7 "Overtone". Output-only, default-off, and not
part of the save state — the deterministic audio contract is unaffected whether
it is armed or not.

The APU counterpart of [pixel provenance](pixel-provenance.md), and deliberately
the same shape: a **register-attribution** half that answers *"what wrote this,
and from which instruction"*, and a **mix trace** that answers *"what were the
channels actually doing"*.

## What the feature answers

Pick a moment in the frame and read the causal chain:

- the mixed value handed to the band-limited decimator, and the expansion-audio
  contribution folded into it;
- what each of the five channels was putting out, as a share of its own full
  scale;
- which channel **dominated**;
- for every APU register, the value it holds, the CPU cycle it was written on,
  and **the instruction that wrote it** — symbolised through the source map when
  one is loaded.

## Why this is not just wiring up existing panels

Every ingredient but one already shipped. `audio_scope.rs` plots the per-channel
waveforms. `audio_mixer.rs` exposes per-channel gain. `Apu::pulse1_out()` and its
siblings expose live channel outputs. The Trace Logger has PC and cycle. The
Event Viewer already records `$4000-$4017` writes as `EventKind::ApuWrite`.

What did not exist anywhere is the **link between a sample and the instruction
that caused it**. `EventRec` carries `kind / scanline / dot / addr / value` — no
PC, no CPU cycle, and it is scanline-oriented rather than sample-oriented. So the
event log is the interception *point* this feature reuses; it is not the record.

## Cadence: per CPU cycle, and why that is the honest choice

The mix is computed **once per CPU cycle** (1.789 MHz NTSC) and handed to `blip`,
which decimates to 44.1 kHz — roughly one output sample per **40.6** CPU cycles.

Recording at *output* rate would mean choosing which of those ~40 mixes "is" the
sample. Band-limited synthesis makes that choice ill-posed: an output sample is a
weighted sum of transitions across the filter kernel, not a copy of one instant.
A tool that picked one anyway would be answering a question its own signal chain
cannot answer, and would do it confidently.

So the trace records what was genuinely mixed, at the cadence it was mixed, and
the panel says plainly that one output sample spans ~40.6 of these cycles. This
is the same discipline as the mapper tier gate and the accuracy ledger: state
what is measured, and decline the rest.

**It is also cheaper than it sounds.** 29,781 records per NTSC frame against the
pixel store's 61,440 — **0.48x the record count of the video side**.

| region | CPU cycles/frame |
|---|---|
| NTSC | 29,781 |
| PAL | 33,247 |
| **Dendy** | **35,464** |

`MIX_CAP` is sized from **Dendy**, not NTSC. Sizing it from the number that comes
to mind first would silently truncate the last 16% of every Dendy frame; and when
the cap *is* exceeded the trace reports `truncated()` rather than quietly
returning a short buffer that looks complete.

## Phase 1 — register attribution

One slot per address across `$4000-$4017`, each holding `(value, cpu_cycle, pc)`.

**Last write, not a history.** The question is "what is the register holding, and
who put it there"; a ring would need a retention policy nobody has a principled
value for. The Event Viewer already keeps the per-frame write *sequence* — this
keeps the per-register *cause*, which it does not.

A `written` flag rather than a sentinel cycle, because **cycle 0 is a legitimate
value**: the reset sequence performs real writes, and a sentinel would misreport
the earliest writes in a run as "never written".

`$4014` (OAM DMA) and `$4016` (controller strobe) fall inside the range and are
not APU registers. They are tracked anyway and labelled for what they are: the
range is what the bus already classifies as `ApuWrite`, one contiguous index
space costs two slots, and a hole would invite off-by-one arithmetic at every
call site.

The attribution is **not** cleared per frame — "which instruction last wrote
`$4003`" has an answer that legitimately predates this frame. It is cleared on a
cold boot, where the history it describes genuinely ended.

## Phase 2 — the mix trace

Per CPU cycle: the five channel outputs that went into the mix, the expansion
contribution, and the result. The index **is** the cycle offset from
`first_cycle`, so no per-record timestamp is stored.

Channel values are the raw pre-mix outputs (0-15 for the pulses, triangle and
noise; 0-127 for the DMC) — what the non-linear mixer consumes. They are **not**
scaled by the frontend's mixer gains: those are a presentation control, and
recording post-gain values would make the record describe the user's slider
rather than the chip.

`dominant()` compares each channel's share of **its own full scale**, not raw
magnitude, because the raw values are not commensurable — a DMC 127 and a pulse
15 are both "full scale" on different scales.

## Phase 3 — attribution plumbing

The split follows the precedent pixel provenance set: the **bus has the PC**, the
**APU has the destination register**, and the PC is pushed down once per
instruction from the existing debug block in `Nes::run_frame`. `rustynes-cpu` is
untouched.

Both push-down sites are mirrored — `run_frame` and `step_instruction` — so
single-stepping through a `$4003` store in the debugger attributes the write to
the stepped instruction rather than to whatever `run_frame` last left latched.

Recording happens in `Apu::write_register` **before** the write dispatches, so
the recorded value is what the CPU put on the bus rather than whatever a channel
decided to keep. Both mix paths record: the default-configuration fast
specialization and the gated general path. A record that existed on only one of
two byte-identical paths would be a trap for whoever next changed the other.

## The trap this feature inherited, and how it was closed up front

**Pixel provenance shipped non-functional for four releases** — v2.3.2 to v2.3.6
— because run-ahead's per-frame rollback cleared the store *after* the visible
frame was harvested and *before* the frontend released the emulator lock. The UI
could never observe a populated record. A comment two lines above the clear
asserted the opposite, and that prose is what stopped anyone checking.

Audio provenance rides the identical rollback. So the carry landed **in the same
change as the feature**, not after a bug report:

- `Nes::take_audio_provenance` / `put_audio_provenance`, called around
  `restore_quiet` in `RunAhead::finish`.
- Save-state loads and netplay rollback still clear, unchanged — those are
  genuine timeline changes. Run-ahead's rollback is not; it returns to the
  timeline it just left.
- `runahead_preserves_audio_provenance` drives the real produce path at
  `run_ahead = 1`, the default, and looks at the first moment the UI could.
  **Mutation-checked**: dropping the stash turns it red.
- A control test proves a plain run populates the trace, so a failure of the
  run-ahead test cannot be misread as a bad assertion.

Both assertions are floored at 20,000 records rather than "non-empty", because
the APU's 8-cycle reset sequence alone produces eight records — a non-emptiness
check would pass on a run that emulated nothing at all.

One further note recorded because it cost time: **a single `run_frame`
immediately after `from_rom` can advance zero cycles**, since the PPU starts at a
frame boundary. The control runs three frames for that reason, exactly as the
pixel-provenance control does.

## Phase 4 — the panel

`Tools → Audio → Audio Provenance`, beside Pixel Provenance in intent.

The panel reads the **core** for the armed state every frame rather than keeping
a mirror. The pixel panel kept one and edge-detected on it, which desynced
permanently the moment a ROM load installed a fresh `Nes` — checkbox ticked, core
unarmed, no way back but unticking and re-ticking.

It distinguishes three empty states rather than rendering one confident blank
report: *not armed*, *armed but nothing recorded yet*, and *trace truncated*.

Register rows carry their **side-band effects**, because naming the right
instruction and then describing the wrong effect is its own failure. A write to
`$4003` does not merely set the period — it also loads the length counter, resets
the duty sequencer, and restarts the envelope. Those annotations were confirmed
against this emulator's own implementation (`Pulse::write_timer_hi`,
`Triangle::write_linear`, `Apu::write_status`, and the `$4017` alignment comment
in `Apu::write_register`), not from memory:

| register | beyond the obvious effect |
|---|---|
| `$4003` / `$4007` | loads length, resets duty sequencer, restarts envelope |
| `$4008` | length-counter halt is **deferred** past the same-cycle half-frame clock |
| `$400B` | loads length, sets the linear-counter reload flag |
| `$400F` | loads length, restarts envelope |
| `$4015` | length enables; the DMC enable is latched and applied with a delay |
| `$4017` | effects land 3 CPU cycles later on an APU clock, 4 otherwise |

## Determinism and the save state

Output-only throughout. Nothing recorded is read back into synthesis, and none of
it is serialized. The new `Apu` field is registered in
`snapshot_schema_audit.rs` as output-only with a written reason — the audit
caught it the moment it was added and refused to pass until it was classified.
(It caught **four** fields originally; see "What the bench changed" below.)

The attribution is deliberately **not** carried in a save state for the same
reason the PPU's `write_attrib` is not: a restored state's registers were not
written by any instruction this session ran, so carrying PCs across a restore
would report a timeline that no longer exists.

## What the bench changed

Workstream C is not decoration. The plan required re-running `apu_throughput`
after the plumbing landed, and that re-run reshaped the code **three times**. All
three regressions were invisible in the diff; none would have been found by
reading it.

The configuration that matters throughout is **feature compiled in, arm off** —
what every user runs, because `crates/rustynes-frontend/Cargo.toml` pulls
`rustynes-core` with `debug-hooks` on unconditionally. "Default-off" here means
the runtime arm, not the code.

**First:** `record_mix` built the `MixRecord` before testing whether provenance
was armed, so a disarmed build recomputed all five channel outputs every CPU
cycle — and `Pulse::output` is not free, it calls `muted()`, which calls
`sweep_target()`. Measured **+14% to +23%**. The arm check moved to the top.

**Second:** with the check first, a quiet-host A/B still measured **+9.2% /
−2.0% / +9.7%**. The diagnosis was struct layout — four new inline fields
(`reg_attrib`, `mix_trace`, `attrib_pc`, `attrib_cycle`) sitting among hot
members — and they were consolidated behind a single
`Option<Box<AudioProvenance>>`.

**That diagnosis was wrong, and the bench said so.** Re-measured after the
consolidation: **+7.98% / +2.88% / +11.03%**, order-bias control +0.11% / +0.76%
/ +0.67%. The consolidation is kept because one pointer is the better shape, but
it is not what fixed anything, and the earlier claim that it would is recorded
here rather than deleted.

**Third — the actual cause.** The tell was in the numbers all along: the absolute
costs were **+33 µs, +15 µs, +65 µs**, wildly non-uniform. A per-cycle branch
costs a constant number of cycles and cannot produce that shape. `record_mix` was
still being **inlined into `tick_with_external`** — the arm check skipped the
*work*, but the five `output()` calls were still emitted inside the hot function,
inflating it past the point where the mixer and the channel ticks kept their
register allocation and their I-cache line.

The body is now outlined behind `#[cold] #[inline(never)]`, leaving exactly one
null test on the hot path. Final measurement, disarmed, against a control that
drifted −0.8% to −1.4% over the same interval:

| workload | outlined vs baseline | order-bias control | net |
|---|---|---|---|
| `apu_tick_silent_frame` | −0.63% | −1.41% | +0.8% — within drift |
| `apu_tick_active_frame` | −5.60% | −0.80% | −4.8% |
| `..._with_external` | +0.00% (p = 0.99) | −0.29% | +0.3% — within drift |

The disarmed cost is gone. **The −4.8% is NOT claimed as an optimization**: it is
code-layout luck in the favourable direction, of exactly the same kind that
produced the +11% in the unfavourable one, and an unrelated future change will
erase it. Recording it as a win would be adopting noise.

Three lessons worth carrying:

- **A default-off feature can charge the default path without executing one line
  of its own code.** Twice here, by two different mechanisms.
- **A branch that skips the work does not skip the code.** An early return still
  leaves the body inlined in the caller.
- **Non-uniform absolute deltas rule out a per-cycle cost.** That single
  observation is what redirected the investigation from layout to inlining, after
  the layout fix had already been built and measured.

## Verification

- `cargo test -p rustynes-apu --features debug-hooks provenance` — 9 unit tests.
- `cargo test -p rustynes-frontend audio_provenance` — the control and the
  run-ahead regression.
- `cargo test -p rustynes-test-harness --test snapshot_schema_audit`.
- AccuracyCoin **141/141** and nestest 0-diff, verified rather than assumed: this
  release touches `rustynes-apu`.
