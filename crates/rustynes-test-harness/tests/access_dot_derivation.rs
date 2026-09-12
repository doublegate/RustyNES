// SPDX-License-Identifier: GPL-3.0-or-later
//! v2.6.18 condition 2 — where in the CPU cycle does a PPU access land?
//!
//! # The reframing this test exists for
//!
//! The write-placement study swept `WRITE_PHI_OFFSET` and `READ_PHI_OFFSET` in
//! MASTER CLOCKS and reasoned about the result in half-dots. That is the wrong
//! unit, and the sweep's own output says so: `READ=0` and `READ=2` produce
//! identical counts AND identical gained/lost sets, as do `WRITE=2` and
//! `WRITE=4`.
//!
//! The reason is in `Bus::run_ppu_to`, which advances the PPU with
//! `while self.ppu_clock + ppu_div <= target` — **whole dots only**. A
//! sub-dot offset that does not cross a dot boundary is a no-op by
//! construction. So the placement space is not a continuum of master clocks:
//! an NTSC CPU cycle is three PPU dots, and the only question is **which of
//! the three** each access lands in.
//!
//! That is physical rather than an artifact. The 2C02 is clocked at the dot
//! rate, so an access lands in whichever dot its strobe falls in; there is no
//! sub-dot state for it to land between.
//!
//! # Why reads and writes are swept together
//!
//! The nesdev `NMI` page gives one instant for a `$2002` read: "Return old
//! status of `vblank_flag` in bit 7, **then** set `vblank_flag` to false", and
//! the race is stated as VBlank-set and the read happening simultaneously.
//! Observation and side effect are the same event, so a read cannot be a
//! sample point plus a separate effect point — which was the leading
//! hypothesis before this measurement, and is refuted by the documentation
//! rather than by a sweep.
//!
//! What remains is that on hardware BOTH accesses strobe in the same phase of
//! the CPU cycle, so a model placing reads in one dot and writes in another is
//! unphysical whichever dots those are. This grid asks what the corpus says
//! about each combination, including the diagonal where they agree.
//!
//! # What the grid could not reach before
//!
//! `READ_PHI_OFFSET` / `WRITE_PHI_OFFSET` only add, so only dots 1 and 2 were
//! reachable and only three of the nine cells had ever been measured. The
//! `*_PHI_BACKOFF` knobs added alongside this test reach dot 0.
//!
//! Diagnostic, not a gate. Run:
//! `cargo test -p rustynes-test-harness --release --features test-roms,phi2-write-sweep --test access_dot_derivation -- --nocapture`
#![cfg(feature = "phi2-write-sweep")]
use core::sync::atomic::Ordering::Relaxed;
use rustynes_test_harness::accuracy_coin_catalog as cat;

/// The three entries the placement is known to trade against, plus the two the
/// read placement governs, so a cell's cost is legible without the full diff.
const WATCHED: [&str; 4] = [
    "Frozen OAM2 Increment",
    "Stale Sprite Shift Regs",
    "Arbitrary Sprite zero",
    "Misaligned OAM2 Address",
];

/// `MASK_WRITE_DELAY` values the coupled sweep visits.
///
/// One entry on purpose: that knob gates the BG-reload freeze and neither
/// outstanding entry is a background test, so this pass holds it at the shipped
/// 4. A named slice rather than an inline literal so widening it is a
/// one-line change here instead of a restructure at the loop.
const MASK_DELAYS: &[u8] = &[4];

/// NTSC: 12 master clocks per CPU cycle, 4 per dot.
const MC_PER_DOT: i32 = 4;
/// The shipped effective advance for a read, in master clocks:
/// `div / 2 - PPU_OFFSET - PPU_OFFSET` = 6 - 1 - 1 = 4 = dot 1.0.
const READ_BASE_MC: i32 = 4;
/// The shipped effective advance for a write: 6 + 1 - 1 = 6 mc, still dot 1.
const WRITE_BASE_MC: i32 = 6;

/// `(offset, backoff)` reaching the requested dot for a given base, or `None`
/// when the dot is not reachable with non-negative knobs.
fn knobs(base_mc: i32, dot: i32) -> Option<(u8, u8)> {
    // Land on the dot's FIRST master clock: any point inside the dot is
    // equivalent, which is the whole finding this test is built on.
    let delta = dot * MC_PER_DOT - base_mc;
    let (off, back) = if delta >= 0 { (delta, 0) } else { (0, -delta) };
    Some((u8::try_from(off).ok()?, u8::try_from(back).ok()?))
}

fn apply(read_dot: i32, write_dot: i32) {
    let (ro, rb) = knobs(READ_BASE_MC, read_dot).expect("read dot reachable");
    let (wo, wb) = knobs(WRITE_BASE_MC, write_dot).expect("write dot reachable");
    rustynes_core::rustynes_cpu::READ_PHI_OFFSET.store(ro, Relaxed);
    rustynes_core::rustynes_cpu::READ_PHI_BACKOFF.store(rb, Relaxed);
    rustynes_core::rustynes_cpu::WRITE_PHI_OFFSET.store(wo, Relaxed);
    rustynes_core::rustynes_cpu::WRITE_PHI_BACKOFF.store(wb, Relaxed);
}

fn reset_knobs() {
    apply(1, 1);
}

fn run() -> Vec<cat::TestStatus> {
    let (_r, ram) = rustynes_test_harness::accuracy_coin::run_battery_capturing_ram(7_000);
    cat::decode_results(&ram).expect("decode")
}

#[test]
fn derive_the_access_dot_grid() {
    // Control FIRST and at the shipped placement, so a harness fault reads as a
    // wrong baseline rather than as a wrong conclusion.
    reset_knobs();
    let base = run();
    let bs = cat::summarise(&base);
    let baseline = bs.pass + bs.pass_with_code;
    eprintln!(
        "CONTROL read=dot1 write=dot1 (shipped): passed={baseline}/{}",
        bs.assigned()
    );
    assert_eq!(
        baseline, 143,
        "the control does not reproduce the shipped 143/144 -- fix the harness \
         before reading anything into the grid"
    );

    for read_dot in 0..3 {
        for write_dot in 0..3 {
            apply(read_dot, write_dot);
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
            let watched: Vec<String> = cat::catalog()
                .iter()
                .zip(now.iter())
                .filter(|(e, _)| WATCHED.contains(&e.name.as_str()))
                .map(|(e, st)| format!("{}={}", e.name, if st.is_pass() { "pass" } else { "FAIL" }))
                .collect();
            eprintln!(
                "read=dot{read_dot} write=dot{write_dot}  passed={passed}/{}  [{}]  \
                 GAINED={gained:?} LOST={lost:?}",
                s.assigned(),
                watched.join(", ")
            );
        }
    }
    reset_knobs();
}

/// The premise the grid rests on, asserted rather than assumed: two knob values
/// inside the SAME dot must produce the same split, and crossing a dot boundary
/// must not.
///
/// This is a property of the arithmetic in `knobs`, checked here because the
/// expensive grid above is only meaningful if it holds.
#[test]
fn knob_values_within_one_dot_are_the_same_placement() {
    // Read base is 4 mc (dot 1). Offsets 0..=3 all stay inside dot 1.
    for off in 0..MC_PER_DOT {
        assert_eq!(
            (READ_BASE_MC + off) / MC_PER_DOT,
            1,
            "read offset {off} should stay inside dot 1"
        );
    }
    assert_eq!(
        (READ_BASE_MC + MC_PER_DOT) / MC_PER_DOT,
        2,
        "read offset 4 crosses into dot 2"
    );
    // Write base is 6 mc: offsets 0 and 1 are dot 1, 2 and 3 are dot 2.
    assert_eq!((WRITE_BASE_MC) / MC_PER_DOT, 1);
    assert_eq!((WRITE_BASE_MC + 1) / MC_PER_DOT, 1);
    assert_eq!((WRITE_BASE_MC + 2) / MC_PER_DOT, 2);
    // Dot 0 is reachable only through the backoff knobs.
    assert_eq!(knobs(READ_BASE_MC, 0), Some((0, 4)));
    assert_eq!(knobs(WRITE_BASE_MC, 0), Some((0, 6)));
    assert_eq!(knobs(WRITE_BASE_MC, 2), Some((2, 0)));
}

/// Is the six-entry NMI loss at `read = dot2` a PURE one-dot relative shift
/// between the `$2002` read and VBL-set, or a defect in the read placement?
///
/// The grid above shows `read=dot2, write=dot2` — the physically unified
/// placement, both accesses strobing in the same phase as they do on hardware —
/// passing `Stale Sprite Shift Regs`, `Arbitrary Sprite zero` AND
/// `Misaligned OAM2 Address`, and losing exactly the six NMI entries. If those
/// six come back when VBL-set moves by the same one dot, then the whole
/// disagreement is CPU/PPU ALIGNMENT and not access placement, and the six are
/// not evidence against a unified access point.
///
/// If they do NOT come back, the read placement is independently constrained
/// and the unified point is refuted on its own terms.
///
/// `VBL_SET_DOT` is a diagnostic. nesdev specifies scanline 241 dot 1 and that
/// is what ships; nothing here proposes changing it.
#[test]
fn is_the_nmi_loss_a_pure_alignment_shift() {
    use rustynes_core::rustynes_ppu::VBL_SET_DOT;

    const NMI_SIX: [&str; 6] = [
        "NMI Overlap BRK",
        "NMI Overlap IRQ",
        "NMI Timing",
        "NMI Suppression",
        "NMI at VBlank end",
        "NMI disabled at VBlank",
    ];

    reset_knobs();
    VBL_SET_DOT.store(1, Relaxed);
    let base = run();
    let bs = cat::summarise(&base);
    assert_eq!(
        bs.pass + bs.pass_with_code,
        143,
        "control must reproduce the shipped 143/144 first"
    );

    for vbl_dot in [0u8, 1, 2] {
        apply(2, 2);
        VBL_SET_DOT.store(vbl_dot, Relaxed);
        let now = run();
        let s = cat::summarise(&now);
        let nmi_ok = cat::catalog()
            .iter()
            .zip(now.iter())
            .filter(|(e, _)| NMI_SIX.contains(&e.name.as_str()))
            .filter(|(_, st)| st.is_pass())
            .count();
        let mut lost = Vec::new();
        for ((e, b), n) in cat::catalog().iter().zip(base.iter()).zip(now.iter()) {
            if b.is_pass() && !n.is_pass() {
                lost.push(e.name.clone());
            }
        }
        eprintln!(
            "read=dot2 write=dot2 VBL_SET_DOT={vbl_dot}  passed={}/{}  NMI six passing={nmi_ok}/6  LOST={lost:?}",
            s.pass + s.pass_with_code,
            s.assigned()
        );
    }

    VBL_SET_DOT.store(1, Relaxed);
    reset_knobs();
}

/// The COUPLED sweep: access placement together with every `$2001` delay depth
/// that is known to compensate for it.
///
/// Sweeping one parameter at a time cannot find a compensating combination, and
/// this PPU has FOUR rendering-delay depths that were each fitted against the
/// SHIPPED placement:
///
/// | consumer | knob | ships |
/// |---|---|---|
/// | render gate (`rendering_enabled_delayed`) | `RENDER_GATE_LAG` | 1 dot |
/// | odd-frame skip (`mask_for_skip_check`) | `SKIP_GATE_LAG` | 2 stages |
/// | OAM2 counter | `OAM2_GATE_LAG` | **0 -- the live mask** |
/// | BG-reload freeze | `MASK_WRITE_DELAY` | 4 dots |
///
/// The skip gate says the coupling out loud in its own comment -- "lockstep
/// applies the PPUMASK write at the *start* of a CPU cycle, while real hardware
/// latches at phi2" -- so its depth and the write dot are one quantity split
/// across two places. The OAM2 row is the one that matters: it is the only
/// consumer with NO delay, and it is the gate the two mutually-contradictory
/// OAM2 entries share.
///
/// Writes a CSV so the coupling can be analysed rather than eyeballed. Set
/// `ACCESS_SWEEP_CSV` to choose the path.
#[test]
#[ignore = "coupled sweep: ~135 battery runs, tens of minutes"]
fn coupled_placement_and_delay_sweep() {
    use rustynes_core::rustynes_ppu::{
        MASK_WRITE_DELAY, OAM2_GATE_LAG, RENDER_GATE_LAG, SKIP_GATE_LAG,
    };
    use std::io::Write as _;

    let csv_path =
        std::env::var("ACCESS_SWEEP_CSV").unwrap_or_else(|_| "/tmp/access_sweep.csv".to_owned());
    let mut csv =
        std::fs::File::create(&csv_path).unwrap_or_else(|e| panic!("create {csv_path}: {e}"));
    writeln!(
        csv,
        "read_dot,write_dot,oam2_lag,render_lag,skip_lag,mask_delay,passed,\
         frozen_oam2,stale,arbitrary,misaligned_oam2,lost"
    )
    .expect("csv header");

    let shipped = |()| {
        reset_knobs();
        OAM2_GATE_LAG.store(0, Relaxed);
        RENDER_GATE_LAG.store(1, Relaxed);
        SKIP_GATE_LAG.store(2, Relaxed);
        MASK_WRITE_DELAY.store(4, Relaxed);
    };
    shipped(());
    let base = run();
    let bs = cat::summarise(&base);
    assert_eq!(
        bs.pass + bs.pass_with_code,
        143,
        "the control does not reproduce the shipped 143/144"
    );

    // Reads are pinned to dots 0 and 1: dot 2 loses all six NMI entries and the
    // `VBL_SET_DOT` diagnostic showed that is not an alignment artifact, so
    // spending a third of the grid there would measure a settled question.
    let mut best = (143u32, String::from("shipped"));
    let mut hits: Vec<String> = Vec::new();
    // The ranges are narrowed by what the nine-cell placement grid and the
    // targeted OAM2 experiment already settled, so the sweep spends its cells
    // where a 143 could actually be:
    //   * reads only at dot 0 / dot 1 -- dot 2 loses all six NMI entries and
    //     `VBL_SET_DOT` showed that is not an alignment artifact;
    //   * writes only at dot 1 / dot 2 -- `Frozen OAM2 Increment` passes at no
    //     other write dot for those reads;
    //   * `OAM2_GATE_LAG` at 1 / 2 -- depth 0 is the live-mask read whose
    //     absence of delay is the thing under suspicion;
    //   * `MASK_WRITE_DELAY` held at the shipped 4 for this pass, since it
    //     gates the BG-reload freeze and neither outstanding entry is a
    //     background test. Widen it if this pass finds nothing.
    for read_dot in 0..2 {
        for write_dot in 1..3 {
            for oam2_lag in 1u8..3 {
                for render_lag in 0u8..3 {
                    for skip_lag in 0u8..3 {
                        // A one-element list on purpose: `MASK_WRITE_DELAY`
                        // gates the BG-reload freeze and neither outstanding
                        // entry is a background test, so this pass holds it at
                        // the shipped 4. Widen the slice if a pass finds
                        // nothing -- keeping it a slice is what makes that a
                        // one-character change rather than a restructure.
                        for &mask_delay in MASK_DELAYS {
                            apply(read_dot, write_dot);
                            OAM2_GATE_LAG.store(oam2_lag, Relaxed);
                            RENDER_GATE_LAG.store(render_lag, Relaxed);
                            SKIP_GATE_LAG.store(skip_lag, Relaxed);
                            MASK_WRITE_DELAY.store(mask_delay, Relaxed);
                            let now = run();
                            let s = cat::summarise(&now);
                            let passed = s.pass + s.pass_with_code;
                            let st = |name: &str| {
                                cat::catalog()
                                    .iter()
                                    .zip(now.iter())
                                    .find(|(e, _)| e.name == name)
                                    .is_some_and(|(_, x)| x.is_pass())
                            };
                            let lost: Vec<&str> = cat::catalog()
                                .iter()
                                .zip(base.iter())
                                .zip(now.iter())
                                .filter(|((_, b), n)| b.is_pass() && !n.is_pass())
                                .map(|((e, _), _)| e.name.as_str())
                                .collect();
                            let cell = format!(
                                "read=dot{read_dot} write=dot{write_dot} oam2={oam2_lag} \
                                 render={render_lag} skip={skip_lag} mask={mask_delay}"
                            );
                            writeln!(
                                csv,
                                "{read_dot},{write_dot},{oam2_lag},{render_lag},{skip_lag},\
                                 {mask_delay},{passed},{},{},{},{},{}",
                                st("Frozen OAM2 Increment"),
                                st("Stale Sprite Shift Regs"),
                                st("Arbitrary Sprite zero"),
                                st("Misaligned OAM2 Address"),
                                lost.join(" ")
                            )
                            .expect("csv row");
                            if passed >= 143 {
                                hits.push(format!("{passed}/144  {cell}  LOST={lost:?}"));
                            }
                            if passed > best.0 {
                                best = (passed, cell.clone());
                                eprintln!("NEW BEST {passed}/144 at {cell}");
                            }
                        }
                    }
                }
            }
        }
    }
    eprintln!("best={best:?}  csv={csv_path}");
    eprintln!("cells at 143 or better ({}):", hits.len());
    for h in &hits {
        eprintln!("  {h}");
    }
    shipped(());
}

/// Depth beyond what the borrowed pipeline could reach.
///
/// The coupled sweep's frontier is at the SHIPPED placement:
/// `read=dot1 write=dot1 oam2=2 render=2` closes `Frozen OAM2 Increment` and
/// costs `Stale Sprite Shift Regs` + `Misaligned OAM2 Address` — and it needs
/// `render=2`, a depth that breaks two entries which want the shipped 1. That
/// shape says the extra dot the frozen-flag machinery wants belongs to the OAM2
/// gate, not to the render gate shared with everything else; the first version
/// of `OAM2_GATE_LAG` simply could not express it, because it borrowed the
/// two-stage odd-frame-skip pipeline.
///
/// With a dedicated four-stage history, this asks the question directly: at the
/// shipped placement and the shipped render depth, is there an OAM2 depth that
/// closes the last entry and costs nothing?
#[test]
fn sweep_the_oam2_gate_depth_at_the_shipped_placement() {
    use rustynes_core::rustynes_ppu::{OAM2_GATE_LAG, RENDER_GATE_LAG};

    reset_knobs();
    OAM2_GATE_LAG.store(0, Relaxed);
    RENDER_GATE_LAG.store(1, Relaxed);
    let base = run();
    let bs = cat::summarise(&base);
    assert_eq!(
        bs.pass + bs.pass_with_code,
        143,
        "control must reproduce the shipped 143/144 first"
    );

    for render_lag in [1u8, 2] {
        for depth in 0u8..5 {
            reset_knobs();
            RENDER_GATE_LAG.store(render_lag, Relaxed);
            OAM2_GATE_LAG.store(depth, Relaxed);
            let now = run();
            let s = cat::summarise(&now);
            let st = |name: &str| {
                cat::catalog()
                    .iter()
                    .zip(now.iter())
                    .find(|(e, _)| e.name == name)
                    .is_some_and(|(_, x)| x.is_pass())
            };
            let lost: Vec<&str> = cat::catalog()
                .iter()
                .zip(base.iter())
                .zip(now.iter())
                .filter(|((_, b), n)| b.is_pass() && !n.is_pass())
                .map(|((e, _), _)| e.name.as_str())
                .collect();
            eprintln!(
                "SHIPPED placement, render={render_lag} oam2_depth={depth}  passed={}/{}  \
                 frozen={} stale={} arbitrary={} misaligned={}  LOST={lost:?}",
                s.pass + s.pass_with_code,
                s.assigned(),
                st("Frozen OAM2 Increment"),
                st("Stale Sprite Shift Regs"),
                st("Arbitrary Sprite zero"),
                st("Misaligned OAM2 Address"),
            );
        }
    }
    reset_knobs();
    OAM2_GATE_LAG.store(0, Relaxed);
    RENDER_GATE_LAG.store(1, Relaxed);
}

/// THE DECISIVE CELL: separate depths for the two consumers of one gate.
///
/// At the shipped placement, `render=2 oam2=3` closes `Frozen OAM2 Increment`
/// -- the last outstanding entry -- and costs exactly one: `Stale Sprite Shift
/// Regs`, whose dot-339 sprite-counter re-arm wants the shipped depth 1. The
/// two consumers share `rendering_enabled_delayed` and want different things
/// from it, which is v2.6.5's finding in a different pair of consumers.
///
/// `SPRITE_REARM_LAG` gives the re-arm its own depth. If some combination
/// passes all four watched entries with nothing lost, the battery reaches
/// 144/144.
#[test]
fn separate_the_sprite_rearm_depth_from_the_render_gate() {
    use rustynes_core::rustynes_ppu::{OAM2_GATE_LAG, RENDER_GATE_LAG, SPRITE_REARM_LAG};

    let shipped = || {
        reset_knobs();
        OAM2_GATE_LAG.store(0, Relaxed);
        RENDER_GATE_LAG.store(1, Relaxed);
        SPRITE_REARM_LAG.store(u8::MAX, Relaxed);
    };
    shipped();
    let base = run();
    let bs = cat::summarise(&base);
    assert_eq!(
        bs.pass + bs.pass_with_code,
        143,
        "control must reproduce the shipped 143/144 first"
    );

    for render_lag in [1u8, 2] {
        for oam2 in [2u8, 3] {
            for rearm in [0u8, 1, 2, 3, u8::MAX] {
                shipped();
                RENDER_GATE_LAG.store(render_lag, Relaxed);
                OAM2_GATE_LAG.store(oam2, Relaxed);
                SPRITE_REARM_LAG.store(rearm, Relaxed);
                let now = run();
                let s = cat::summarise(&now);
                let passed = s.pass + s.pass_with_code;
                let lost: Vec<&str> = cat::catalog()
                    .iter()
                    .zip(base.iter())
                    .zip(now.iter())
                    .filter(|((_, b), n)| b.is_pass() && !n.is_pass())
                    .map(|((e, _), _)| e.name.as_str())
                    .collect();
                let still: Vec<&str> = cat::catalog()
                    .iter()
                    .zip(now.iter())
                    .filter(|(_, n)| !n.is_pass())
                    .map(|(e, _)| e.name.as_str())
                    .collect();
                let tag = if rearm == u8::MAX {
                    "follow".to_owned()
                } else {
                    rearm.to_string()
                };
                eprintln!(
                    "render={render_lag} oam2={oam2} rearm={tag}  passed={passed}/{}  \
                     LOST={lost:?}  STILL-FAILING={still:?}",
                    s.assigned()
                );
            }
        }
    }
    shipped();
}
