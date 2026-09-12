//! v2.6.18 — derive the rendering-enable pipeline depth AGAINST the phi2 commit.
//!
//! Diagnostic, not a gate.
//!
//! # The question
//!
//! A `$2001` write's observable effect time is the commit offset PLUS the
//! per-consumer delay depth. Those are one quantity split across two places,
//! and every delay constant in this PPU was fitted against the SHIPPED commit
//! point. v2.6.17 swept `rendering_enabled_delayed` 1..4 and found 1 optimal --
//! but only at that commit point, which answers a different question.
//!
//! Under phi2 the commit is half a dot later, so the depth that reproduces the
//! same effect time is one LESS. That is not a guess: `mask_for_skip_check`
//! needed exactly that correction (2 -> 1) and produced the identical
//! `08 08 09 07`.
//!
//! `rendering_enabled_delayed` is the constant worth deriving because it gates
//! BOTH remaining failures -- the OAM2 63/255/339 reset (`Frozen OAM2
//! Increment`) and the sprite-counter re-arm (`Stale Sprite Shift Regs` test 5).
//!
//! # What a result means
//!
//! If one `(write, lag)` pair passes all three of `Frozen OAM2 Increment`,
//! `Stale Sprite Shift Regs` and `Arbitrary Sprite zero` without losing
//! anything else, the compensation network resolves and 144/144 is reachable.
//! If no pair does, the line closes honestly at 143/144 -- which is a result,
//! not a failure, and is why the controls below are run rather than assumed.
#![cfg(feature = "phi2-write-sweep")]
use core::sync::atomic::Ordering::Relaxed;
use rustynes_test_harness::accuracy_coin_catalog as cat;

/// The three entries the write placement is known to trade against.
const WATCHED: [&str; 3] = [
    "Frozen OAM2 Increment",
    "Stale Sprite Shift Regs",
    "Arbitrary Sprite zero",
];

fn run() -> Vec<cat::TestStatus> {
    let (_r, ram) = rustynes_test_harness::accuracy_coin::run_battery_capturing_ram(7_000);
    cat::decode_results(&ram).expect("decode")
}

#[test]
fn derive_render_gate_lag_under_phi2() {
    // Control FIRST, and at the shipped settings, so a harness fault shows up
    // as a wrong baseline rather than as a wrong conclusion.
    rustynes_core::rustynes_cpu::WRITE_PHI_OFFSET.store(0, Relaxed);
    rustynes_core::rustynes_cpu::READ_PHI_OFFSET.store(0, Relaxed);
    rustynes_core::rustynes_ppu::RENDER_GATE_LAG.store(1, Relaxed);
    let base = run();
    let bs = cat::summarise(&base);
    let baseline = bs.pass + bs.pass_with_code;
    eprintln!(
        "CONTROL write=0 lag=1 (shipped): passed={baseline}/{}",
        bs.assigned()
    );
    assert_eq!(
        baseline, 143,
        "the control does not reproduce the shipped 143/144 -- fix the harness \
         before reading anything into the sweep"
    );

    // (write offset, gate lag). Both directions around the hypothesis, because a
    // sweep that only probes where it expects to win cannot report a null.
    for (woff, lag) in [
        (0u8, 0u8), // shipped commit, no delay -- is the lag doing anything alone?
        (0, 2),     // shipped commit, deeper   -- v2.6.17 measured this as worse
        (2, 0),     // THE HYPOTHESIS: phi2 commit, one less delay
        (2, 1),     // phi2 alone (known 141/144)
        (2, 2),     // phi2, deeper -- the wrong direction, as a control
        (4, 0),     // a full dot later, one less delay
        // The grid's last two cells. Added after the depth-2 pipeline defect
        // was fixed: a claim that 144/144 is unreachable ACROSS this space has
        // to have measured the space, and write=4 had only ever been probed at
        // one depth.
        (4, 1),
        (4, 2),
    ] {
        rustynes_core::rustynes_cpu::WRITE_PHI_OFFSET.store(woff, Relaxed);
        rustynes_core::rustynes_ppu::RENDER_GATE_LAG.store(lag, Relaxed);
        let now = run();
        let s = cat::summarise(&now);
        let passed = s.pass + s.pass_with_code;

        let mut gained = Vec::new();
        let mut lost = Vec::new();
        for ((e, b), n) in cat::catalog().iter().zip(base.iter()).zip(now.iter()) {
            if !b.is_pass() && n.is_pass() {
                gained.push(e.name.clone());
            } else if b.is_pass() && !n.is_pass() {
                lost.push(e.name.clone());
            }
        }
        // Name the three watched entries explicitly: the headline count hides
        // which tests moved, which is the mistake this whole line keeps making.
        let watched: Vec<String> = cat::catalog()
            .iter()
            .zip(now.iter())
            .filter(|(e, _)| WATCHED.contains(&e.name.as_str()))
            .map(|(e, st)| format!("{}={}", e.name, if st.is_pass() { "pass" } else { "FAIL" }))
            .collect();
        eprintln!(
            "write={woff} lag={lag}  passed={passed}/{}  [{}]  GAINED={gained:?} LOST={lost:?}",
            s.assigned(),
            watched.join(", ")
        );
    }

    // Leave the knobs as found.
    rustynes_core::rustynes_cpu::WRITE_PHI_OFFSET.store(0, Relaxed);
    rustynes_core::rustynes_ppu::RENDER_GATE_LAG.store(1, Relaxed);
}
