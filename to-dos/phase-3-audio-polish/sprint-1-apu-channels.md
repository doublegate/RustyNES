# Sprint 3-1 — APU channels (pulse, triangle, noise)

**Phase:** Phase 3 — Audio + Polish
**Sprint goal:** Implement the four wave channels (pulse 1, pulse 2, triangle, noise) with envelope, sweep (pulses), linear counter (triangle), and length counter. Channels respond correctly to register writes; raw per-cycle channel outputs available for sampling.
**Estimated duration:** 2 weeks

## Tickets

### T-31-001 — `Apu` struct + `ApuBus` trait

**Description:** Define `Apu` in `crates/rustynes-apu/src/apu.rs`. State per `docs/apu-2a03.md` §State. `ApuBus` trait per §Interfaces.

**Acceptance criteria:**
- [x] Compiles, public types documented.
- [x] Region (NTSC/PAL/Dendy) configurable at construction.

**Reference:** `docs/apu-2a03.md` §Interfaces, §State.
**Estimated complexity:** S.

---

### T-31-002 — Pulse 1 + Pulse 2 channels

**Description:** Each pulse channel: 11-bit timer, 4-step duty sequencer, envelope (15→0 decay or constant volume), sweep (with pulse 1's `~target` vs pulse 2's `-target` distinction), length counter.

**Acceptance criteria:**
- [x] Per-cycle output is a 4-bit volume value.
- [x] Sweep mute correctly silences when target > $7FF.
- [x] Pulse 1's one's-complement negation distinguished from pulse 2's two's-complement.
- [x] Unit tests for envelope decay, sweep period, length counter halt.

**Reference:** `docs/apu-2a03.md` §Behavior.
**Estimated complexity:** L.

---

### T-31-003 — Triangle channel

**Description:** 32-step triangle sequencer. Linear counter (with reload bit). Length counter halt. Channel silenced when length OR linear is 0; sequencer holds its current step (no click).

**Acceptance criteria:**
- [x] Per-cycle output is a 4-bit value (one of 32 triangle steps).
- [x] Silenced state holds last step (no click).
- [x] Linear counter reload via `$4008`/`$400B` works.

**Reference:** `docs/apu-2a03.md` §Behavior.
**Estimated complexity:** M.

---

### T-31-004 — Noise channel

**Description:** 15-bit LFSR (mode 0) and 6-bit LFSR (mode 1) with the documented feedback taps. Length counter, envelope. 16-entry period lookup table.

**Acceptance criteria:**
- [x] LFSR taps correct for both modes (verified against the documented bit positions).
- [x] Period lookup table matches NESdev wiki values (per region).

**Reference:** `docs/apu-2a03.md` §Behavior.
**Estimated complexity:** M.

---

### T-31-005 — `$4015` write/read interface

**Description:** Implement `$4015` register write (channel enable; clearing forces length counter to 0) and read (returns IRQ flags + length-counter status; reading clears the frame IRQ flag).

**Acceptance criteria:**
- [x] Channel enable bits gate length counter behavior correctly.
- [x] Reading `$4015` clears frame IRQ flag but not DMC IRQ flag.
- [x] Unit tests.

**Reference:** `docs/apu-2a03.md` §`$4015` semantics.
**Estimated complexity:** S.

---

## Sprint review checklist

- [x] All tickets checked off.
- [x] Per-channel output observable for tests.
- [x] CHANGELOG entry: "APU pulse/triangle/noise channels complete."
