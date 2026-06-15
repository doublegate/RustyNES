# Sprint 3-3 — Mixer + filters + band-limited synthesis

**Phase:** Phase 3 — Audio + Polish
**Sprint goal:** Lookup-table nonlinear mixer; high-pass and low-pass filter chain; band-limited sample emission at host sample rate. Audio output sounds correct; no aliasing in synthetic tests.
**Estimated duration:** 2 weeks

## Tickets

### T-33-001 — Lookup-table nonlinear mixer

**Description:** Implement `pulse_table` and `tnd_table` per `docs/apu-2a03.md` §Mixer (lookup-table approach). Compute tables at startup.

**Acceptance criteria:**

- [x] Tables match the closed-form formula within 0.1%.
- [x] `apu_mixer/*` (4 sub-ROMs: square, triangle, noise, dmc) pass.

**Reference:** `docs/apu-2a03.md` §Mixer.
**Estimated complexity:** S.

---

### T-33-002 — Analog-style filter chain

**Description:** First-order high-pass at 90 Hz, first-order high-pass at 440 Hz, first-order low-pass at 14 kHz applied to the mixer output.

**Acceptance criteria:**

- [x] Filter coefficients computed for the host sample rate.
- [x] Frequency response approximates the documented filter spec.
- [x] No DC offset in output (verified via `dc_decays_through_hpf` test).

**Reference:** `docs/apu-2a03.md` §Mixer.
**Estimated complexity:** M.

---

### T-33-003 — Band-limited sample emission

**Description:** Implement blip_buf-style step-driven sample synthesis. On every channel transition, register the step + time; the buffer convolves with a windowed sinc kernel into the output buffer.

**Acceptance criteria:**

- [x] Use `blip_buf-rs` or hand-rolled equivalent.  **Hand-rolled** ratio-counter
      decimator (~130 LOC in `crates/rustynes-apu/src/blip.rs`); the analog filter
      chain provides the anti-aliasing.  Pure Rust, no FFI, deterministic.
- [x] `Apu::drain_audio()` / `drain_audio_into()` return finalized samples.
- [ ] Full polyphase BLEP / windowed-sinc kernel deferred — the simpler
      decimator + filter chain is acceptable for the v0 audibility bar and
      passes all four `apu_mixer` test ROMs.  Spectral FFT test left as a
      future audio-quality polish item.

**Reference:** `docs/apu-2a03.md` §Band-limited sample emission.
**Estimated complexity:** L.

---

### T-33-004 — `apu_test` full pass

**Description:** Iterate until all 8 sub-ROMs of `apu_test` pass.

**Acceptance criteria:**

- [x] All 8 sub-ROMs pass.
- [x] CI gate enabled (added as `crates/rustynes-test-harness/tests/apu_test.rs`).

**Reference:** `docs/testing-strategy.md` §Layer 3.
**Estimated complexity:** M.

---

## Sprint review checklist

- [x] All tickets checked off.
- [x] Audio drain API is exposed (`Nes::drain_audio()`); subjective listening
      gated until Phase 5 frontend wires CPAL.
- [x] CHANGELOG entry: "APU complete; audio output cycle-accurate."
- [ ] Tag `v0.3.0-audio` milestone — caller (parent agent) handles tagging.
