//! v2.6.18 "Terminus" — sweep the write-commit dot against `AccuracyCoin`.
//!
//! Diagnostic, not a gate. Reports the FULL per-test delta versus the shipped
//! offset, because the headline count hides which tests moved — v2.6.17's
//! rendering-enable-lag sweep is the worked example.
#![cfg(feature = "phi2-write-sweep")]
use core::sync::atomic::Ordering::Relaxed;
use rustynes_test_harness::accuracy_coin_catalog as cat;

fn run() -> Vec<cat::TestStatus> {
    let (_r, ram) = rustynes_test_harness::accuracy_coin::run_battery_capturing_ram(7_000);
    cat::decode_results(&ram).expect("decode")
}

#[test]
fn sweep_write_commit_dot() {
    rustynes_core::rustynes_cpu::WRITE_PHI_OFFSET.store(0, Relaxed);
    rustynes_core::rustynes_cpu::READ_PHI_OFFSET.store(0, Relaxed);
    let base = run();
    let bs = cat::summarise(&base);
    eprintln!(
        "OFFSET=0 (shipped)  passed={}/{}",
        bs.pass + bs.pass_with_code,
        bs.assigned()
    );

    // (read, write) pairs. The first sweep moved writes ALONE, which changes
    // the spacing between a write and a following read instead of moving the
    // access model as a unit. NTSC: read +4 and write +2 both land on dot 2.0
    // = phi2, which is the complete model.
    for (roff, woff) in [(0u8, 2u8), (0, 4), (4, 2), (4, 4), (2, 2)] {
        rustynes_core::rustynes_cpu::READ_PHI_OFFSET.store(roff, Relaxed);
        rustynes_core::rustynes_cpu::WRITE_PHI_OFFSET.store(woff, Relaxed);
        let now = run();
        let s = cat::summarise(&now);
        let mut gained = Vec::new();
        let mut lost = Vec::new();
        for ((e, b), n) in cat::catalog().iter().zip(base.iter()).zip(now.iter()) {
            if !b.is_pass() && n.is_pass() {
                gained.push(e.name.clone());
            } else if b.is_pass() && !n.is_pass() {
                lost.push(e.name.clone());
            }
        }
        eprintln!(
            "READ={roff} WRITE={woff}  passed={}/{}  GAINED={:?}  LOST={:?}",
            s.pass + s.pass_with_code,
            s.assigned(),
            gained,
            lost
        );
    }
    rustynes_core::rustynes_cpu::WRITE_PHI_OFFSET.store(0, Relaxed);
    rustynes_core::rustynes_cpu::READ_PHI_OFFSET.store(0, Relaxed);
}

/// v3.3.0 `T-PHI2-WRITE`: the write at phi2 (`WRITE_PHI_OFFSET = 2`, reads
/// unmoved) on the re-synced 146-test battery, with the odd-frame skip at the
/// one stage v2.6.17 derived for phi2 from `ppu_vbl_nmi/10-even_odd_timing`,
/// sweeping the two `$2001` gate depths a placement move shifts: the dot-339
/// sprite re-arm's own gate and the shared rendering gate. The shipped
/// placement loses only `Stale Sprite Shift Regs` under phi2 (the sweep
/// above); this asks whether any depth recovers it without losing another.
#[test]
fn sweep_phi2_gate_depths() {
    use rustynes_core::rustynes_cpu::{READ_PHI_OFFSET, WRITE_PHI_OFFSET};
    use rustynes_core::rustynes_ppu::{RENDER_GATE_LAG, SKIP_GATE_LAG, SPRITE_REARM_LAG};
    let reset = || {
        READ_PHI_OFFSET.store(0, Relaxed);
        WRITE_PHI_OFFSET.store(0, Relaxed);
        SKIP_GATE_LAG.store(2, Relaxed);
        RENDER_GATE_LAG.store(1, Relaxed);
        SPRITE_REARM_LAG.store(u8::MAX, Relaxed);
    };
    reset();
    let base = run();
    for render in [1u8, 0, 2] {
        for rearm in [u8::MAX, 0, 1, 2, 3] {
            reset();
            WRITE_PHI_OFFSET.store(2, Relaxed);
            SKIP_GATE_LAG.store(1, Relaxed);
            RENDER_GATE_LAG.store(render, Relaxed);
            SPRITE_REARM_LAG.store(rearm, Relaxed);
            let now = run();
            let s = cat::summarise(&now);
            let mut gained = Vec::new();
            let mut lost = Vec::new();
            for ((e, b), n) in cat::catalog().iter().zip(base.iter()).zip(now.iter()) {
                if !b.is_pass() && n.is_pass() {
                    gained.push(e.name.clone());
                } else if b.is_pass() && !n.is_pass() {
                    lost.push(e.name.clone());
                }
            }
            eprintln!(
                "PHI2 render_gate={render} rearm={rearm}  passed={}/{}  GAINED={gained:?}  LOST={lost:?}",
                s.pass + s.pass_with_code,
                s.assigned(),
            );
        }
    }
    reset();
}
