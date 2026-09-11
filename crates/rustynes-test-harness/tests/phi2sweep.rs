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
    let base = run();
    let bs = cat::summarise(&base);
    eprintln!(
        "OFFSET=0 (shipped)  passed={}/{}",
        bs.pass + bs.pass_with_code,
        bs.assigned()
    );

    for off in 1u8..=4 {
        rustynes_core::rustynes_cpu::WRITE_PHI_OFFSET.store(off, Relaxed);
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
            "OFFSET={off}  passed={}/{}  GAINED={:?}  LOST={:?}",
            s.pass + s.pass_with_code,
            s.assigned(),
            gained,
            lost
        );
    }
    rustynes_core::rustynes_cpu::WRITE_PHI_OFFSET.store(0, Relaxed);
}
