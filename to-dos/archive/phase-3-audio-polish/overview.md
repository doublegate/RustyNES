# Phase 3 — Audio + Polish

> **Status (v1.0.0): delivered.** The v1.0.0 engine ships the full 2A03 APU —
> all five channels, the frame counter, the lookup-table non-linear mixer, the
> analog-style filter chain, band-limited polyphase-BLEP synthesis, and DMC DMA.
> This overview is retained as development history — see
> [`ROADMAP.md`](../ROADMAP.md) for current status.

## Goal

Implement the 2A03 APU: all five channels, the frame counter, the lookup-table nonlinear mixer, the analog-style filter chain, and band-limited sample emission. Add DMC DMA and the documented register-readout bug. By the end of this phase the emulator produces correct audio for any title using only the on-board APU.

## Exit criteria

- [x] `apu_test/*` (8 sub-ROMs) pass.
- [x] `apu_mixer/*` (4 sub-ROMs) pass.
- [x] `dmc_dma_during_read4/*` (5 sub-ROMs in upstream release) pass.
- [ ] `cpu_interrupts_v2/*` (5 sub-ROMs) pass.  **Status: 0/5; deferred to a
      Phase 4 timing-precision sprint that introduces per-cycle bus
      interleaving — the same architectural wall flagged in Phase 2.**
- [ ] Audio output is band-limited.  **Status: filter-chain anti-aliasing in
      place; full polyphase-BLEP synthesis deferred to a future audio-quality
      polish item.  All four `apu_mixer` ROMs pass.**

## Scope

In-scope:

- All five APU channels (pulse 1, pulse 2, triangle, noise, DMC).
- Frame counter (4-step and 5-step modes; frame IRQ).
- DMC DMA via the scheduler's DMA controller.
- The 2A03 register-readout bug during DMA halt.
- Lookup-table mixer + 90 Hz / 440 Hz HP + 14 kHz LP.
- Band-limited sample emission (blip_buf-style).

Out-of-scope:

- Mapper-extended audio (Phase 4).
- Audio capture / WAV export (Phase 5).

## Sprints

- [Sprint 1 — APU channels (pulse 1, pulse 2, triangle, noise)](sprint-1-apu-channels.md)
- [Sprint 2 — DMC channel + DMC DMA + frame counter](sprint-2-dmc-frame.md)
- [Sprint 3 — Mixer + filters + band-limited synthesis](sprint-3-mixer.md)

## Dependencies

Phase 2 complete (scheduler + DMA controller scaffolding).

## Risks

- **Risk: frame counter timing off-by-one.** Detection: `apu_test/4-jitter`. Mitigation: write the frame counter as a state machine with named events; validate against the documented sequence.
- **Risk: DMC DMA misbehavior breaks CPU timing.** Detection: `dmc_dma_during_read4`. Mitigation: keep the DMA controller in `rustynes-core` (single owner of CPU halt logic); rigorous unit tests.
- **Risk: blip_buf-style synthesis introduces latency.** Detection: subjective + scope visualization. Mitigation: tune kernel width for latency vs. accuracy.

## Reference docs

- [docs/apu-2a03.md](../../docs/apu-2a03.md)
- [docs/scheduler.md](../../docs/scheduler.md) — DMA controller
- [docs/cpu-6502.md](../../docs/cpu-6502.md) — interrupt logic (frame IRQ + DMC IRQ)
