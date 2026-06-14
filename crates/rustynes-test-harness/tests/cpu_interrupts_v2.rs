//! blargg `cpu_interrupts_v2/rom_singles/*.nes` corpus.  Validates CPU
//! interrupt timing including NMI / IRQ / BRK and cycle-precise effects.
//!
//! As of v1.0.0 — the master-clock-precise scheduler is the default and ONLY
//! core — the ENTIRE suite passes strictly on the default build:
//! `1-cli_latency`, `2-nmi_and_brk`, `3-nmi_and_irq`, `4-irq_and_dma`, and
//! `5-branch_delays_irq` are all plain strict `#[test]`s (no `#[ignore]`).
//!
//! History: through the engine lineage these sub-ROMs failed on the legacy
//! integer-lockstep build and passed only under the opt-in `mc-r1-full-cpu`
//! master-clock umbrella (R1 `end_cycle` `T_last - 1` + R2 on-time VBL/NMI + R3
//! cold-boot alignment closed #2/#3/#4; `mc-r1-branch-poll-points` closed #5).
//! That umbrella was promoted to the default core and the feature flag removed,
//! so the former default-build expected-fail set and its `*_currently_fails`
//! companion probes no longer exist.

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
// 2-nmi_and_brk — PASSES strictly on the default master-clock build
// (closed by the R-phase substrate, now unconditional in the v1.0.0 core).
// ============================================================================

#[test]
fn cpu_interrupts_v2_2_nmi_and_brk_strict() {
    let (s, m, _) = run("2-nmi_and_brk.nes", 1500);
    assert_eq!(s, 0, "2-nmi_and_brk: {m}");
}

// ============================================================================
// 3-nmi_and_irq — PASSES strictly on the default master-clock build
// (closed by the R-phase substrate, now unconditional in the v1.0.0 core).
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
// 5-branch_delays_irq — PASSES strictly on the default master-clock build
// (closed by the taken-branch IRQ poll points, now unconditional in the
// v1.0.0 core).
// ============================================================================

#[test]
fn cpu_interrupts_v2_5_branch_delays_irq_strict() {
    let (s, m, _) = run("5-branch_delays_irq.nes", 1500);
    assert_eq!(s, 0, "5-branch_delays_irq: {m}");
}
