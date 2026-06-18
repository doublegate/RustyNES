//! v1.7.0 "Forge" Workstream F2 — sub-v2.0 accuracy audit.
//!
//! F2 names two "genuinely-new sub-v2.0 behaviors":
//!
//!   (a) the APU **length-counter halt/reload race** — writing `$400x` to halt
//!       a channel and reloading the length counter on adjacent CPU cycles
//!       relative to the frame-counter's half-frame clock has a one-cycle race
//!       (blargg's `len_halt_timing` / `len_reload_timing`); and
//!   (b) the **DMC load-DMA even/odd-cycle delay** — a DMC sample-buffer LOAD
//!       DMA that begins on a "get" (odd) CPU cycle is deferred one cycle vs.
//!       one that begins on a "put" (even) cycle.
//!
//! Audit finding (v1.7.0): BOTH behaviors are **already implemented** on the
//! current dot-lockstep scheduler, and their canonical test ROMs already pass.
//! Per the F2 contract ("everything already implemented is verify-only — add an
//! audit test, do NOT reimplement"), this file is the explicit, named regression
//! pin for each, so a future refactor that silently broke either trips here with
//! a clear label instead of only as an opaque drift in the broader APU corpus.
//!
//!   (a) gated by blargg `blargg_apu_2005.07.30/10.len_halt_timing.nes` +
//!       `11.len_reload_timing.nes` — both must report status 0 ("Passed").
//!   (b) gated by the `dmc_dma_defer_load_entry` model in
//!       `rustynes-core/src/bus.rs` (the even/odd `put_cycle` defer for a DMC
//!       LOAD DMA) — exercised end-to-end by `dmc_tests/latency.nes` (the DMC
//!       fetch-latency audio probe) and the `sprdma_and_dmc_dma` alignment ROMs,
//!       all of which the harness already runs. Here we pin the DMC fetch-latency
//!       ROM's deterministic audio signature so a regression in the even/odd
//!       defer surfaces as a labelled F2(b) failure.
//!
//! Per `docs/apu-2a03.md` §"Length counter (halt / reload race, F2a)" and
//! §"DMC DMA (load even/odd-cycle delay, F2b)", and `docs/testing-strategy.md`.

#![cfg(feature = "test-roms")]

mod common;

use common::{fnv1a64, rom_path};
use std::fs;

use rustynes_core::Nes;
use rustynes_test_harness::run_nes_blargg;

// ---------------------------------------------------------------------------
// F2(a) — APU length-counter halt/reload race.
// ---------------------------------------------------------------------------

fn run_blargg_apu_2005(name: &str) -> (u8, String) {
    let path = rom_path(&format!("nes-test-roms/blargg_apu_2005.07.30/{name}"));
    let bytes = fs::read(&path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e));
    let r = run_nes_blargg(&bytes, 2000).expect("rom must parse + run");
    (r.status, r.message)
}

/// F2(a): the length-counter HALT timing race. A `$400x` halt-bit write on the
/// cycle adjacent to the half-frame length clock must (or must not) suppress the
/// length-counter clock per hardware — this ROM brackets the exact cycle.
#[test]
fn f2a_length_counter_halt_timing_race() {
    let (status, msg) = run_blargg_apu_2005("10.len_halt_timing.nes");
    assert_eq!(
        status, 0,
        "F2(a) length-counter halt-timing race regressed: status={status:#x} msg={msg:?}"
    );
}

/// F2(a): the length-counter RELOAD timing race — a length reload on the cycle
/// adjacent to the half-frame clock.
#[test]
fn f2a_length_counter_reload_timing_race() {
    let (status, msg) = run_blargg_apu_2005("11.len_reload_timing.nes");
    assert_eq!(
        status, 0,
        "F2(a) length-counter reload-timing race regressed: status={status:#x} msg={msg:?}"
    );
}

// ---------------------------------------------------------------------------
// F2(b) — DMC load-DMA even/odd-cycle delay.
// ---------------------------------------------------------------------------

/// F2(b): the DMC sample-buffer LOAD-DMA fetch latency, which depends on the
/// even/odd ("put"/"get") CPU-cycle alignment at the moment the DMA begins
/// (modelled by `dmc_dma_defer_load_entry`). The `dmc_tests/latency.nes` ROM
/// reports via APU audio (it renders a blank frame), so we pin its deterministic
/// audio FNV-1a signature: a regression in the even/odd defer changes the DMC
/// fetch cadence and therefore the audio hash, tripping this labelled F2(b) pin.
#[test]
fn f2b_dmc_load_dma_latency_audio_signature() {
    const ROM: &str = "nes-test-roms/dmc_tests/latency.nes";
    const FRAMES: u64 = 240;
    let path = rom_path(ROM);
    let bytes = fs::read(&path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e));
    let mut nes = Nes::from_rom(&bytes).unwrap_or_else(|e| panic!("parse {ROM}: {e}"));
    let mut samples: Vec<f32> = Vec::new();
    for _ in 0..FRAMES {
        nes.run_frame();
        samples.extend(nes.drain_audio());
    }
    assert!(
        !samples.is_empty(),
        "F2(b): latency.nes produced no audio — the DMC path went silent"
    );
    let mut audio_bytes: Vec<u8> = Vec::with_capacity(samples.len() * 4);
    for s in &samples {
        audio_bytes.extend_from_slice(&s.to_le_bytes());
    }
    let snap = format!(
        "rom={ROM} frames={FRAMES} audio_samples={} audio_fnv1a64={:016x}",
        samples.len(),
        fnv1a64(&audio_bytes)
    );
    insta::assert_snapshot!("f2b_dmc_latency_f240", snap);
}

/// F2(b): the `sprdma_and_dmc_dma` alignment ROM strictly passes — this is the
/// end-to-end gate that the OAM-DMA + DMC-DMA cycle-steal alignment (including
/// the load even/odd defer) is correct. It uses the `$6000` protocol without the
/// magic preamble, so we also require the "Passed" marker.
#[test]
fn f2b_sprdma_and_dmc_dma_alignment() {
    let path = rom_path("nes-test-roms/sprdma_and_dmc_dma/sprdma_and_dmc_dma.nes");
    let bytes = fs::read(&path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e));
    let r = run_nes_blargg(&bytes, 1500).expect("rom must parse + run");
    assert_eq!(
        r.status, 0,
        "F2(b) sprdma_and_dmc_dma alignment regressed: status={:#x} msg={:?}",
        r.status, r.message
    );
    assert!(
        r.message.contains("Passed"),
        "F2(b) sprdma_and_dmc_dma did not report Passed (false $6000==0?): {}",
        r.message
    );
}
