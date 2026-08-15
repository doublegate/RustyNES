#![allow(missing_docs)]
//! Criterion bench — `Apu::tick` per-CPU-cycle throughput.
//!
//! **Why this exists.** v2.3.1's per-source-file attribution put the APU at
//! **18.7% of frame time** — `apu.rs` 8.4%, `frame_counter.rs` 2.1%, `blip.rs`
//! 2.1% — the largest core cost that has never been examined. It is invisible in
//! a symbol profile because fat LTO inlines the whole APU into `cpu_clock`,
//! which is precisely why the ten-candidate v2.3.1 hot-path sweep never touched
//! it. Nothing about that cost can be adjudicated without an instrument, and
//! the sibling crates already have one each (`cpu_throughput`,
//! `ppu_throughput`); the APU had only `spectral.rs`, which measures blip
//! *quality*, not per-cycle cost.
//!
//! **Methodology.** Each iteration reproduces `LockstepBus::cpu_clock`'s full
//! per-cycle APU sequence, not just the tick:
//!
//! ```text
//! set_canonical_cycle(cycle)      // the one-clock collapse: the bus owns the counter
//! tick_with_external(sample)      // the mix + timers
//! dmc_tick_end()                  // M-1: DMC byte-timer + reload arm, end-of-cycle
//! promote_dmc_pending_next()      // visibility delay for the next cycle's DMA
//! ```
//!
//! An earlier version of this bench called only `tick()`. That under-measured the
//! per-cycle cost and made the "exactly as the bus drives it" claim false — both
//! review bots caught it independently. The end-of-cycle pair is where the get/put
//! flip lives under M-2, so omitting it skips real work the emulator pays every
//! cycle.
//!
//! A frame is **29,780 CPU cycles** at NTSC (1,789,773 Hz / 60.0988 fps), so a
//! one-frame bench is directly comparable against the `full_frame` core bench
//! that the >3% adoption bar is adjudicated on.
//!
//! Three workloads, because the per-cycle cost is not uniform and a single number
//! would hide which part moved:
//!
//! | bench | what it isolates |
//! | --- | --- |
//! | `apu_tick_silent_frame` | the floor: every channel disabled, so the timers still clock and the mixer still runs but nothing is audible |
//! | `apu_tick_active_frame` | the realistic case: all five channels enabled and running, which is what a game actually costs |
//! | `apu_tick_active_frame_with_external` | the same plus a non-zero external (expansion-audio) sample, the `tick_with_external` path a VRC6/5B/N163 cart drives |
//!
//! `FrameCounter::tick` in isolation (2.1% of frame time on its own) is a
//! deliberate omission, not an oversight: it is C2, to be measured only if C1
//! does not land. Listing a bench here that does not exist would be the same
//! doc-drift this file's own methodology section warns about.
//!
//! **Caveat on the third row.** It varies the external sample INSIDE the timed
//! loop, so it includes that arithmetic. Read it for shape; do not use it in
//! adoption arithmetic.
//!
//! **What this bench is NOT.** It is not an accuracy check. Register writes here
//! are chosen to put channels in a *representative running state*, not to
//! reproduce any particular game's audio. Correctness lives in the unit tests,
//! `blargg_apu_2005`, and the decibel oracle.

use criterion::{Criterion, criterion_group, criterion_main};
use rustynes_apu::{Apu, Region};
use std::hint::black_box;

/// NTSC CPU cycles in one frame: 1,789,773 Hz / 60.0988 fps.
const CPU_CYCLES_PER_FRAME: usize = 29_780;

/// Output sample rate. Fixed rather than taken from a config so the bench does
/// not silently change cost when a default moves.
const SAMPLE_RATE: u32 = 48_000;

/// An APU with every channel enabled and running — the state a game holds for
/// most of a frame, and therefore the one whose per-cycle cost matters.
///
/// The register values are ordinary running settings: mid-range timer periods
/// so no channel sits at a degenerate period, non-zero lengths so the length
/// counters do not halt the channels partway through the measured frame, and
/// constant-volume envelopes so the measured cost does not drift as an envelope
/// decays across runs.
fn active_apu() -> Apu {
    let mut apu = Apu::new(Region::Ntsc, SAMPLE_RATE);
    // $4015: enable pulse 1+2, triangle, noise, DMC.
    apu.write_register(0x4015, 0x1F);

    // Pulse 1 — 50% duty, constant volume 15, timer ~$0AA, length index 1.
    apu.write_register(0x4000, 0xBF);
    apu.write_register(0x4002, 0xAA);
    apu.write_register(0x4003, 0x08);
    // Pulse 2 — 25% duty, constant volume 12, a different period so the two
    // pulses do not stay phase-locked for the whole frame.
    apu.write_register(0x4004, 0x7C);
    apu.write_register(0x4006, 0x55);
    apu.write_register(0x4007, 0x08);
    // Triangle — linear counter reload, control clear so it keeps running.
    apu.write_register(0x4008, 0x7F);
    apu.write_register(0x400A, 0x40);
    apu.write_register(0x400B, 0x08);
    // Noise — constant volume, mid period.
    apu.write_register(0x400C, 0x3A);
    apu.write_register(0x400E, 0x07);
    apu.write_register(0x400F, 0x08);
    // DMC — mid rate, a non-zero level so the output is not trivially 0.
    apu.write_register(0x4010, 0x0F);
    apu.write_register(0x4011, 0x40);
    apu
}

/// The floor: channels disabled via `$4015`, so timers still clock and the
/// mixer still runs every cycle but nothing is audible. The gap between this
/// and `active` is the part of the cost that depends on channels doing work;
/// everything below it is unconditional per-cycle overhead.
fn silent_apu() -> Apu {
    let mut apu = Apu::new(Region::Ntsc, SAMPLE_RATE);
    apu.write_register(0x4015, 0x00);
    apu
}

/// One CPU cycle, in the order `LockstepBus::cpu_clock` runs it. `cycle` is the
/// canonical bus counter the APU is handed rather than keeping its own mirror.
#[inline]
fn production_cycle(apu: &mut Apu, cycle: u64, external: f32) {
    apu.set_canonical_cycle(cycle);
    apu.tick_with_external(external);
    apu.dmc_tick_end();
    apu.promote_dmc_pending_next();
}

fn bench_silent(c: &mut Criterion) {
    c.bench_function("apu_tick_silent_frame", |b| {
        b.iter_batched(
            silent_apu,
            |mut apu| {
                for cycle in 0..CPU_CYCLES_PER_FRAME as u64 {
                    production_cycle(&mut apu, cycle, 0.0);
                }
                black_box(&apu);
            },
            criterion::BatchSize::SmallInput,
        );
    });
}

fn bench_active(c: &mut Criterion) {
    c.bench_function("apu_tick_active_frame", |b| {
        b.iter_batched(
            active_apu,
            |mut apu| {
                for cycle in 0..CPU_CYCLES_PER_FRAME as u64 {
                    production_cycle(&mut apu, cycle, 0.0);
                }
                black_box(&apu);
            },
            criterion::BatchSize::SmallInput,
        );
    });
}

fn bench_active_external(c: &mut Criterion) {
    c.bench_function("apu_tick_active_frame_with_external", |b| {
        b.iter_batched(
            active_apu,
            |mut apu| {
                // A non-zero, varying external sample: the expansion-audio path
                // a VRC6 / Sunsoft 5B / Namco 163 cart drives. Constant input
                // would let the compiler hoist work the real path cannot.
                let mut phase = 0.0f32;
                for cycle in 0..CPU_CYCLES_PER_FRAME as u64 {
                    phase = if phase > 0.25 { -0.25 } else { phase + 0.001 };
                    production_cycle(&mut apu, cycle, black_box(phase));
                }
                black_box(&apu);
            },
            criterion::BatchSize::SmallInput,
        );
    });
}

criterion_group!(benches, bench_silent, bench_active, bench_active_external);
criterion_main!(benches);
