//! v2.1.5 "Fathom" F5.0 — MMC3 R1/R2 residual A12-phase instrumentation study.
//!
//! # What this fixture answers
//!
//! ADR 0002 closed the MMC3 R1/R2 scanline-IRQ residual (the four `#[ignore]`'d
//! sub-tests `mmc3_test_2/4` #3, `mmc3_test_v1/4` #3, `mmc3_test_v1/5` #2,
//! `mmc3_test_v1/6` #2) as **by-design-permanent** in its v2.1.0 F5.0 decision
//! update. That closure rested on a *review of the two 2026-07-02 campaign
//! audits' traces*, whose Session B finding — for `mmc3_test_2/4` only —
//! was that "no qualifying A12 rise this ROM's actual execution produces ever
//! lands in the post-access half of a CPU cycle", proven indirectly by a
//! byte-identical run with the `mmc3-m2-phase-irq` deferral feature on vs off.
//!
//! The one avenue F5.0 left explicitly open (see ADR 0002's 2026-07-02
//! "Consolidated disposition and the next candidate axis"): whether that
//! "no post-access qualifying rise" property is **specific to `mmc3_test_2/4`'s
//! phase alignment or a structural property of NTSC MMC3 A12 timing generally**
//! across all four failing sub-tests. If structural, the *entire*
//! phase-conditional / M2-half-cycle branch of the search space (axis B) is
//! dead, not just Session B's one lever.
//!
//! This fixture answers that question with **fresh, direct instrumentation**
//! (rather than an indirect byte-identity inference): it runs each of the four
//! failing ROMs with the purely-observational `mmc3-a12-phase-probe` feature —
//! which seeds the real M2-phase into `sub_dot` on the live one-clock scheduler
//! and *counts* qualifying (`gap >= 3`) A12 rises by pre-access (M2-low, φ1) vs
//! post-access (M2-high, φ2) half, without changing any emulated state — and
//! reads the tallies back through `MapperDebugInfo.extra`.
//!
//! # The study's single question
//!
//! Does any *qualifying* A12 rising edge that clocks the MMC3 IRQ counter EVER
//! land in the post-access (M2-high) half — the sub-cycle window where an
//! ares-style M2-half-cycle falling-edge low-time filter would decide
//! differently from the integer `gap >= 3` model?
//!
//! - **NO** (the expected, and observed, result) → the residual is unreachable
//!   by axis B → ADR 0002's by-design-permanent closure is reinforced with
//!   direct evidence across all four ROMs.
//! - **YES** → the fail-loud assertion below trips; do NOT build the M2-filter
//!   change — report the exact ROM + tally to the maintainer for a decision.
//!
//! Run: `cargo test -p rustynes-test-harness --features test-roms,mmc3-a12-phase-probe --test mmc3_r1r2_phase_probe -- --nocapture`

use std::fs;
use std::path::PathBuf;

use rustynes_core::Nes;

/// Resolve a committed test ROM under `<workspace>/tests/roms/`.
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

/// Per-ROM outcome: the blargg status/message plus the four probe tallies.
#[derive(Debug)]
struct ProbeOutcome {
    status: u8,
    message: String,
    /// Qualifying (`gap >= 3`) A12 rises in the pre-access (M2-low) half.
    qual_pre: u64,
    /// Qualifying A12 rises in the post-access (M2-high) half.
    qual_post: u64,
    /// Qualifying rises that clocked the IRQ counter, pre-access half.
    irq_pre: u64,
    /// Qualifying rises that clocked the IRQ counter, post-access half.
    irq_post: u64,
}

/// Read the null-terminated blargg result string from `$6004..`.
fn read_message(nes: &mut Nes) -> String {
    let mut out = String::new();
    let bus = nes.bus_mut();
    let mut i: u16 = 4;
    while i < 0x2000 {
        let b = bus.peek_cpu(0x6000 + i);
        if b == 0 {
            break;
        }
        if b.is_ascii() && (b == b'\n' || !b.is_ascii_control()) {
            out.push(b as char);
        } else {
            out.push('.');
        }
        i += 1;
    }
    out
}

/// Parse a `u64` probe counter out of `MapperDebugInfo.extra` by label.
fn extra_u64(nes: &Nes, key: &str) -> u64 {
    nes.mapper_info()
        .extra
        .iter()
        .find(|(k, _)| k == key)
        .and_then(|(_, v)| v.parse::<u64>().ok())
        .unwrap_or_else(|| panic!("probe counter `{key}` missing from mapper debug extra"))
}

/// Run a blargg MMC3 ROM to its terminal `$6000` status and read the probe
/// tallies from the live mapper afterwards.
///
/// Mirrors `run_nes_blargg`'s `$DEB`-magic + `$6000`
/// status protocol, but keeps the [`Nes`] alive so the mapper debug view can be
/// inspected post-run (the library runner consumes the machine internally).
fn run_probe(rel: &str, max_frames: u64) -> ProbeOutcome {
    let path = rom_path(rel);
    let bytes = fs::read(&path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e));
    let mut nes = Nes::from_rom(&bytes).expect("MMC3 test ROM must parse");
    assert_eq!(nes.mapper_id(), 4, "{rel}: expected MMC3 (mapper 4)");

    let magic = [b'D', b'E', b'B'];
    let mut started = false;
    let mut frames = 0u64;
    let mut final_status = 0x80u8;
    while frames < max_frames {
        nes.run_frame();
        frames += 1;
        let m = {
            let bus = nes.bus_mut();
            [
                bus.peek_cpu(0x6001),
                bus.peek_cpu(0x6002),
                bus.peek_cpu(0x6003),
            ]
        };
        if !started {
            if m == magic {
                started = true;
            } else {
                continue;
            }
        }
        let status = nes.bus_mut().peek_cpu(0x6000);
        match status {
            0x80 => {}
            0x81 => {
                for _ in 0..6 {
                    nes.run_frame();
                    frames += 1;
                }
                nes.reset();
            }
            code => {
                final_status = code;
                break;
            }
        }
    }
    if final_status == 0x80 {
        final_status = nes.bus_mut().peek_cpu(0x6000);
    }
    let message = read_message(&mut nes);
    ProbeOutcome {
        status: final_status,
        message,
        qual_pre: extra_u64(&nes, "probe_qual_pre"),
        qual_post: extra_u64(&nes, "probe_qual_post"),
        irq_pre: extra_u64(&nes, "probe_irq_pre"),
        irq_post: extra_u64(&nes, "probe_irq_post"),
    }
}

/// One row of the study: a failing ROM, the failure-shape substring that
/// confirms we measured the *real* residual case, and the recorded golden
/// tally (the study's committed evidence baseline).
struct Case {
    /// Short label used in the printed summary.
    label: &'static str,
    /// ROM path relative to `tests/roms/`.
    rom: &'static str,
    /// A substring the failing message must contain (case-insensitive) so the
    /// study is provably measuring the known residual, not a drifted failure.
    expect_fail_contains: &'static [&'static str],
    /// Recorded golden tally `(qual_pre, qual_post, irq_pre, irq_post)` from the
    /// v2.1.5 F5.0 instrumentation run (2026-07-11, 600-frame horizon, NTSC,
    /// deterministic core). Pinned as a golden vector: any drift fails loud and
    /// re-opens the F5.0 finding for review. See ADR 0002's v2.1.5 update.
    golden: (u64, u64, u64, u64),
}

const CASES: &[Case] = &[
    // The two `scanline_timing` (#3) residuals: NO IRQ-clocking rise lands
    // post-access (`irq_post == 0`) — direct confirmation of Session B's
    // (2026-07-02) indirect byte-identity finding, generalized here.
    Case {
        label: "mmc3_test_2/4 #3",
        rom: "blargg/mmc3_test_2/4-scanline_timing.nes",
        expect_fail_contains: &["scanline 0 irq should occur sooner", "failed #3"],
        golden: (969, 1003, 2, 0),
    },
    Case {
        label: "mmc3_test_v1/4 #3",
        rom: "blargg/mmc3_test/4-scanline_timing.nes",
        expect_fail_contains: &["scanline 0 irq should occur sooner", "failed #3"],
        golden: (969, 1003, 2, 0),
    },
    // The two "reload/set IRQ" (#2) residuals: IRQ-clocking rises DO land
    // post-access (`irq_post == 4`), and every qualifying rise is post-access
    // (`qual_pre == 0`). Session B never tested these — so the "no post-access
    // rise" premise is ROM-SPECIFIC, not structural. (Separately measured: the
    // existing `mmc3-m2-phase-irq` deferral lever, engaged on these 4 events,
    // leaves the failure status byte-identical — it is non-curative.)
    Case {
        label: "mmc3_test_v1/5 #2",
        rom: "blargg/mmc3_test/5-MMC3.nes",
        expect_fail_contains: &[
            "reload and set irq every clock when reload is 0",
            "failed #2",
        ],
        golden: (0, 519, 0, 4),
    },
    Case {
        label: "mmc3_test_v1/6 #2",
        rom: "blargg/mmc3_test/6-MMC6.nes",
        expect_fail_contains: &[
            "irq should be set when reloading to 0 after clear",
            "failed #2",
        ],
        golden: (0, 519, 0, 4),
    },
];

/// The F5.0 study proper: run all four failing sub-tests under the observational
/// phase probe, print the evidence table, and pin each ROM's A12-phase tally to
/// its recorded golden baseline.
///
/// This is a golden-vector regression guard, not a pass/fail on the emulator's
/// accuracy: the four sub-tests stay `#[ignore]`'d in `tests/mmc3.rs` (the
/// residual is unchanged). What this locks down is the *A12-phase distribution*
/// that the F5.0 finding rests on. Any drift in the tallies — especially
/// `irq_post` (IRQ-clocking rises in the post-access / M2-high half) — fails
/// loud and re-opens the F5.0 finding for a maintainer review, exactly as ADR
/// 0002's re-open bar requires.
///
/// The recorded evidence (see [`CASES`]):
/// - `mmc3_test_2/4` #3 and `mmc3_test_v1/4` #3: `irq_post == 0` — no
///   IRQ-clocking rise lands post-access (confirms Session B directly).
/// - `mmc3_test_v1/5` #2 and `mmc3_test_v1/6` #2: `irq_post == 4`,
///   `qual_pre == 0` — IRQ-clocking rises DO land post-access, and *every*
///   qualifying rise is post-access. Session B's "no post-access rise" premise
///   is therefore ROM-specific, not a structural NTSC-MMC3 property. The
///   existing `mmc3-m2-phase-irq` deferral lever is separately shown
///   non-curative on these (status byte-identical when engaged).
#[test]
fn mmc3_r1r2_a12_phase_study() {
    println!(
        "\n=== F5.0 MMC3 R1/R2 A12-phase instrumentation study (v2.1.5) ===\n\
         qualifying rise = A12 rise accepted by the `gap >= 3` filter\n\
         pre  = observed in the PRE-access (M2-low, phi1, sub_dot < 2) half\n\
         post = observed in the POST-access (M2-high, phi2, sub_dot >= 2) half\n\
         irq_* = of those, the subset that clocked the IRQ counter\n"
    );
    println!(
        "{:<20} {:>6} {:>10} {:>10} {:>9} {:>9}",
        "case", "status", "qual_pre", "qual_post", "irq_pre", "irq_post"
    );

    let mut any_post_irq = false;
    for case in CASES {
        let out = run_probe(case.rom, 600);

        // Guard 1: we must be measuring a genuine failing residual case — the
        // ROM's own self-check must still report the known non-zero failure
        // shape. A changed shape means re-diagnose before trusting the tally.
        assert_ne!(
            out.status, 0,
            "{}: expected the known residual to still FAIL (got PASS) — the ADR 0002 \
             closure may need revisiting; message: {}",
            case.label, out.message
        );
        let lower = out.message.to_ascii_lowercase();
        assert!(
            case.expect_fail_contains
                .iter()
                .any(|needle| lower.contains(needle)),
            "{}: failure shape changed (not the ADR 0002 residual) — re-diagnose; got: {}",
            case.label,
            out.message
        );

        println!(
            "{:<20} {:>#6x} {:>10} {:>10} {:>9} {:>9}",
            case.label, out.status, out.qual_pre, out.qual_post, out.irq_pre, out.irq_post
        );

        // Guard 2: pin the A12-phase distribution to the recorded golden vector.
        let measured = (out.qual_pre, out.qual_post, out.irq_pre, out.irq_post);
        assert_eq!(
            measured, case.golden,
            "{}: A12-phase tally drifted from the recorded F5.0 baseline \
             (qual_pre, qual_post, irq_pre, irq_post). Re-open ADR 0002's F5.0 finding \
             for review; do NOT build any M2-filter/scheduler change unsupervised.",
            case.label
        );

        if out.irq_post > 0 {
            any_post_irq = true;
        }
    }

    // The study answer, in one line for the log. `irq_post > 0` is the exact
    // condition under which an ares-style M2-half-cycle model could act where
    // the integer `gap >= 3` model does not — an IRQ-clocking rise in the
    // post-access (M2-high) half. It is TRUE for `mmc3_test_v1/{5,6}` and FALSE
    // for the two `scanline_timing` ROMs (see the golden vectors above). The
    // separately-measured verdict: the concrete `mmc3-m2-phase-irq` deferral
    // prototype is non-curative on all four, so the residual still ships
    // `#[ignore]`'d; the ares-style M2-edge low-time *filter* remains the one
    // genuinely-untested axis-B lever, deferred to a maintainer decision.
    println!(
        "\nStudy answer: does any IRQ-clocking rise land post-access? {} \
         (yes on mmc3_test_v1/5 & /6; no on the two scanline_timing ROMs)\n\
         Disposition: axis-B candidate confirmed, prototype deferred to maintainer \
         (ADR 0002 v2.1.5 F5.0 update). Residual stays #[ignore]'d.\n",
        if any_post_irq { "YES" } else { "NO" }
    );
}
