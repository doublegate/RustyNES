//! blargg `cpu_interrupts_v2/rom_singles/*.nes` corpus.  Validates CPU
//! interrupt timing including NMI / IRQ / BRK and cycle-precise effects.
//!
//! As of v0.9.0:
//!
//! - `1-cli_latency` passes strictly.
//! - `2-nmi_and_brk`, `3-nmi_and_irq`, `4-irq_and_dma`, `5-branch_delays_irq`
//!   all fail at the same architectural surface: the CPU's per-cycle IRQ
//!   sample point, the bus's IRQ poll point, and the PPU's A12 emission
//!   dot need to be re-aligned together. See CHANGELOG `[Unreleased]` →
//!   "Investigated and rolled back" for the diagnosis. v1.0.0 milestone.
//!
//! W3-Stage-4 promotion (2026-06-10): under the opt-in `mc-r1-full-cpu`
//! master-clock umbrella the WHOLE suite passes strictly — the R-phase
//! substrate (R1 `end_cycle` `T_last - 1` + R2 on-time VBL/NMI + R3
//! cold-boot alignment) closed #2/#3/#4, and `mc-r1-branch-poll-points`
//! (folded into the umbrella at Stage 4) closed #5. The default lockstep
//! build keeps the documented expected-fail set, so the #2/#3/#5 strict
//! tests are `#[ignore]`'d ONLY on the default build via
//! `#[cfg_attr(not(feature = "mc-r1-full-cpu"), ignore = ...)]`, and the
//! `*_currently_fails` companion probes (which assert the DEFAULT build's
//! failure shape and trip loudly on a surprise pass) are compiled out
//! under the umbrella via `#[cfg(not(feature = "mc-r1-full-cpu"))]`.
//!
//! NOTE: the harness's `mc-r1-full-cpu` feature is a forward to
//! `rustynes-core/mc-r1-full-cpu`; always enable the pair together
//! (`--features test-roms,mc-r1-full-cpu`) so the cfg expectations here
//! match the core actually under test.

#![cfg(feature = "test-roms")]

use std::fs;
use std::path::PathBuf;

use rustynes_test_harness::run_nes_blargg;

fn rom_path(rel: &str) -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .join("tests")
        .join("roms")
        .join(rel)
}

fn run(name: &str, max_frames: u64) -> (u8, String, u64) {
    let path = rom_path(&format!("blargg/cpu_interrupts_v2/{name}"));
    let bytes = fs::read(&path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e));
    let r = run_nes_blargg(&bytes, max_frames).expect("rom must parse + run");
    (r.status, r.message, r.frames)
}

// ============================================================================
// 1-cli_latency — passes strictly.
// ============================================================================

#[test]
fn cpu_interrupts_v2_1_cli_latency() {
    let (s, m, _) = run("1-cli_latency.nes", 1500);
    assert_eq!(s, 0, "1-cli_latency failed: {m}");
}

// ============================================================================
// 2-nmi_and_brk — PASSES strictly under `mc-r1-full-cpu` (W3-Stage-4
// promotion); known fail on the default lockstep build (NMI/BRK timing).
// ============================================================================

#[test]
fn cpu_interrupts_v2_2_nmi_and_brk_strict() {
    let (s, m, _) = run("2-nmi_and_brk.nes", 1500);
    assert_eq!(s, 0, "2-nmi_and_brk: {m}");
}

// ============================================================================
// 3-nmi_and_irq — PASSES strictly under `mc-r1-full-cpu` (W3-Stage-4
// promotion); known fail on the default lockstep build (NMI/IRQ interleave).
// ============================================================================

#[test]
fn cpu_interrupts_v2_3_nmi_and_irq_strict() {
    let (s, m, _) = run("3-nmi_and_irq.nes", 1500);
    assert_eq!(s, 0, "3-nmi_and_irq: {m}");
}

// ============================================================================
// 4-irq_and_dma — PASSES strictly as of C1 Phase 3 (DMA alignment audit,
// 2026-05-15). The OAM DMA alignment parity in `LockstepBus::drain_dma`
// was inverted from `cycle & 1 == 0 => 513` to `cycle & 1 == 0 => 514`
// per nesdev §DMA's get/put alignment rule; combined with Phase 1's
// M2-low IRQ sample, this flipped the test FAIL → PASS.
// ============================================================================

#[test]
fn cpu_interrupts_v2_4_irq_and_dma_strict() {
    let (s, m, _) = run("4-irq_and_dma.nes", 1500);
    assert_eq!(s, 0, "4-irq_and_dma: {m}");
}

// ============================================================================
// 5-branch_delays_irq — PASSES strictly under `mc-r1-full-cpu` (the W1
// `mc-r1-branch-poll-points` taken-branch IRQ poll points, folded into the
// umbrella at W3-Stage-4); known fail on the default lockstep build
// (test_branch_taken_pagecross).
// ============================================================================

#[test]
fn cpu_interrupts_v2_5_branch_delays_irq_strict() {
    let (s, m, _) = run("5-branch_delays_irq.nes", 1500);
    assert_eq!(s, 0, "5-branch_delays_irq: {m}");
}
