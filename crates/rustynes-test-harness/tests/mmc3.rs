//! MMC3 (mapper 4) test ROM coverage.
//!
//! Two suites by Shay Green (blargg) / kevtris:
//!
//! - `mmc3_test_2/rom_singles/*.nes` — clocking, details, A12 detection,
//!   scanline timing, and Sharp/NEC revision differences. Sub-ROMs 1, 2,
//!   3, 5 pass strictly as of v0.9.0. Sub-ROM 4 fails on sub-test #2
//!   ("Scanline 0 IRQ should occur later when `$2000=$08`") — this is
//!   part of the IRQ-timing residual tracked in CHANGELOG. Sub-ROM 6
//!   (NEC rev B) is mutually exclusive with sub-ROM 5 (Sharp rev A); we
//!   default to Sharp so 6 fails by design.
//! - `mmc3_irq_tests/*.nes` — older counterpart that uses a visual-only
//!   protocol (no blargg `$6000` status byte). These run for diagnostics
//!   only and confirm the emulator doesn't crash on the ROM.
//!
//! Per `docs/testing-strategy.md` §Layer 3.

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
    let path = rom_path(name);
    let bytes = fs::read(&path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e));
    let r = run_nes_blargg(&bytes, max_frames).expect("rom must parse + run");
    (r.status, r.message, r.frames)
}

// ---------- mmc3_test_2 ----------

#[test]
fn mmc3_test_2_1_clocking() {
    let (s, m, _) = run("blargg/mmc3_test_2/1-clocking.nes", 600);
    assert_eq!(s, 0, "mmc3_test_2 1-clocking failed: {m}");
}

#[test]
fn mmc3_test_2_2_details() {
    let (s, m, _) = run("blargg/mmc3_test_2/2-details.nes", 600);
    assert_eq!(s, 0, "mmc3_test_2 2-details failed: {m}");
}

#[test]
fn mmc3_test_2_3_a12_clocking() {
    let (s, m, _) = run("blargg/mmc3_test_2/3-A12_clocking.nes", 600);
    assert_eq!(s, 0, "mmc3_test_2 3-A12_clocking failed: {m}");
}

#[test]
#[ignore = "expected-fail: sub-test #3 (1-CPU-cycle bracket residual after C1 step B4); see CHANGELOG '[Unreleased]' + docs/adr/0002"]
fn mmc3_test_2_4_scanline_timing_strict() {
    let (s, m, _) = run("blargg/mmc3_test_2/4-scanline_timing.nes", 600);
    assert_eq!(s, 0, "mmc3_test_2 4-scanline_timing: {m}");
}

#[test]
fn mmc3_test_2_4_scanline_timing_currently_fails() {
    let (s, m, _) = run("blargg/mmc3_test_2/4-scanline_timing.nes", 600);
    assert_ne!(
        s, 0,
        "mmc3_test_2/4 unexpectedly PASSES — please flip the `_strict` test to non-ignored \
         and delete this probe; msg={m}"
    );
    // Post-C1-step-B4: sub-tests #1 and #2 now PASS.  The residual is at
    // sub-test #3 ("Scanline 0 IRQ should occur SOONER when $2000=$08"),
    // a 1-CPU-cycle bracket distinct from the structural reload-pending
    // discriminator that step B4 closed.  See ADR-0002 →
    // "Empirical refinement (post-step B4 success, 2026-05-14)".
    assert!(
        m.contains("Scanline 0 IRQ should occur sooner") || m.contains("Failed #3"),
        "mmc3_test_2/4 failure shape changed (was sub-test #3 after C1 step B4) — \
         please re-diagnose; got: {m}"
    );
}

#[test]
fn mmc3_test_2_5_mmc3() {
    // Sharp (rev A) behavior — the project default.
    let (s, m, _) = run("blargg/mmc3_test_2/5-MMC3.nes", 600);
    assert_eq!(s, 0, "mmc3_test_2 5-MMC3 failed: {m}");
}

#[test]
#[ignore = "by-design fail: sub-ROM 6 is NEC rev B; project defaults to Sharp rev A (sub-ROM 5)"]
fn mmc3_test_2_6_mmc3_alt_strict() {
    let (s, m, _) = run("blargg/mmc3_test_2/6-MMC3_alt.nes", 600);
    assert_eq!(s, 0, "mmc3_test_2 6-MMC3_alt: {m}");
}

#[test]
fn mmc3_test_2_6_mmc3_alt_currently_fails_by_design() {
    // Sharp rev A vs. NEC rev B are mutually exclusive. With Sharp as the
    // default, sub-ROM 6 must fail. If it ever passes here, the default
    // revision has silently flipped.
    let (s, _, _) = run("blargg/mmc3_test_2/6-MMC3_alt.nes", 600);
    assert_ne!(
        s, 0,
        "mmc3_test_2/6 (NEC rev B) unexpectedly PASSES on a Sharp-default build — \
         the default MMC3 revision may have flipped; please re-check Mapper::default()"
    );
}

// ---------- mmc3_test (v1, the older kevtris/blargg suite) ----------
//
// v1.5.0 Workstream C1 (TASVideos compatibility pass). The original
// `mmc3_test` suite (Shay Green / kevtris) predates `mmc3_test_2` and uses
// the SAME blargg `$6000` status protocol (the `$DE $B0 $G1` signature is
// in its readme). It is a DISTINCT set of ROMs (verified by SHA-256) that
// re-derives the MMC3 scanline-counter + IRQ behavior with stricter,
// older assertions — a good independent gate beyond the 139 AccuracyCoin.
//
// Observed against the cycle-accurate core (pinned 2026-06-16):
//   - 1-clocking      PASS (strict)
//   - 2-details       PASS (strict)
//   - 3-A12_clocking  PASS (strict)
//   - 4-scanline_timing FAIL #3 ("Scanline 0 IRQ should occur sooner when
//     $2000=$08") — the SAME 1-CPU-cycle scanline-IRQ bracket residual as
//     `mmc3_test_2/4` #3, deferred to the v2.0 fractional-master-clock
//     refactor (ADR 0002).
//   - 5-MMC3          FAIL #2 ("Should reload and set IRQ every clock when
//     reload is 0") — MMC3 reload-to-0 IRQ-cadence precision; same ADR-0002
//     scanline-counter-timing axis (the structural reload-to-0 ASSERTION is
//     present — see `Mmc3::clock_irq` path 2 — only the sub-scanline cadence
//     differs). NB: `mmc3_test_2/5-MMC3` PASSES strictly; this older v1
//     sub-test asserts a tighter cadence.
//   - 6-MMC6          FAIL #2 ("IRQ should be set when reloading to 0 after
//     clear") — same MMC6 reload-to-0 cadence axis (ADR 0002).
//
// Per the test-ROM-is-spec discipline these expectations are PINNED: the
// three passing ROMs are strict, and 4/5/6 are documented expected-fail
// probes that flag if the failure SHAPE changes or a ROM unexpectedly
// passes (then flip the strict test on and delete the probe).

#[test]
fn mmc3_test_v1_1_clocking() {
    let (s, m, _) = run("blargg/mmc3_test/1-clocking.nes", 600);
    assert_eq!(s, 0, "mmc3_test v1 1-clocking failed: {m}");
}

#[test]
fn mmc3_test_v1_2_details() {
    let (s, m, _) = run("blargg/mmc3_test/2-details.nes", 600);
    assert_eq!(s, 0, "mmc3_test v1 2-details failed: {m}");
}

#[test]
fn mmc3_test_v1_3_a12_clocking() {
    let (s, m, _) = run("blargg/mmc3_test/3-A12_clocking.nes", 600);
    assert_eq!(s, 0, "mmc3_test v1 3-A12_clocking failed: {m}");
}

#[test]
#[ignore = "expected-fail: sub-test #3 (1-CPU-cycle scanline-IRQ bracket residual); deferred to v2.0 fractional-master-clock refactor (docs/adr/0002)"]
fn mmc3_test_v1_4_scanline_timing_strict() {
    let (s, m, _) = run("blargg/mmc3_test/4-scanline_timing.nes", 600);
    assert_eq!(s, 0, "mmc3_test v1 4-scanline_timing: {m}");
}

#[test]
fn mmc3_test_v1_4_scanline_timing_currently_fails() {
    let (s, m, _) = run("blargg/mmc3_test/4-scanline_timing.nes", 600);
    assert_ne!(
        s, 0,
        "mmc3_test v1/4 unexpectedly PASSES — flip the `_strict` test on and delete this probe; msg={m}"
    );
    assert!(
        m.contains("Scanline 0 IRQ should occur sooner") || m.contains("Failed #3"),
        "mmc3_test v1/4 failure shape changed (was sub-test #3, the ADR-0002 axis) — re-diagnose; got: {m}"
    );
}

#[test]
#[ignore = "expected-fail: sub-test #2 (reload-to-0 IRQ-cadence precision); deferred to v2.0 fractional-master-clock refactor (docs/adr/0002)"]
fn mmc3_test_v1_5_mmc3_strict() {
    let (s, m, _) = run("blargg/mmc3_test/5-MMC3.nes", 600);
    assert_eq!(s, 0, "mmc3_test v1 5-MMC3: {m}");
}

#[test]
fn mmc3_test_v1_5_mmc3_currently_fails() {
    let (s, m, _) = run("blargg/mmc3_test/5-MMC3.nes", 600);
    assert_ne!(
        s, 0,
        "mmc3_test v1/5 unexpectedly PASSES — flip the `_strict` test on and delete this probe; msg={m}"
    );
    assert!(
        m.contains("reload and set IRQ every clock when reload is 0") || m.contains("Failed #2"),
        "mmc3_test v1/5 failure shape changed (was sub-test #2, the ADR-0002 axis) — re-diagnose; got: {m}"
    );
}

#[test]
#[ignore = "expected-fail: sub-test #2 (MMC6 reload-to-0 IRQ cadence); deferred to v2.0 fractional-master-clock refactor (docs/adr/0002)"]
fn mmc3_test_v1_6_mmc6_strict() {
    let (s, m, _) = run("blargg/mmc3_test/6-MMC6.nes", 600);
    assert_eq!(s, 0, "mmc3_test v1 6-MMC6: {m}");
}

#[test]
fn mmc3_test_v1_6_mmc6_currently_fails() {
    let (s, m, _) = run("blargg/mmc3_test/6-MMC6.nes", 600);
    assert_ne!(
        s, 0,
        "mmc3_test v1/6 unexpectedly PASSES — flip the `_strict` test on and delete this probe; msg={m}"
    );
    assert!(
        m.contains("IRQ should be set when reloading to 0 after clear") || m.contains("Failed #2"),
        "mmc3_test v1/6 failure shape changed (was sub-test #2, the ADR-0002 axis) — re-diagnose; got: {m}"
    );
}

// ---------- mmc3_irq_tests (visual-only protocol) ----------
//
// These ROMs report failure visually (palette colors / pattern position),
// not through the blargg `$6000` status byte. We can only smoke-test that
// the emulator runs the ROM without panicking. Each test asserts that
// `run_nes_blargg` advances to the frame cap without bus / mapper / PPU
// crashes.

fn smoke_mmc3_irq(rel: &str) {
    let (_, _, frames) = run(rel, 600);
    assert!(
        frames > 0,
        "{rel} produced 0 frames — emulator did not advance"
    );
}

#[test]
fn mmc3_irq_tests_1_clocking_smoke() {
    smoke_mmc3_irq("blargg/mmc3_irq_tests/1.Clocking.nes");
}

#[test]
fn mmc3_irq_tests_2_details_smoke() {
    smoke_mmc3_irq("blargg/mmc3_irq_tests/2.Details.nes");
}

#[test]
fn mmc3_irq_tests_3_a12_clocking_smoke() {
    smoke_mmc3_irq("blargg/mmc3_irq_tests/3.A12_clocking.nes");
}

#[test]
fn mmc3_irq_tests_4_scanline_timing_smoke() {
    smoke_mmc3_irq("blargg/mmc3_irq_tests/4.Scanline_timing.nes");
}

#[test]
fn mmc3_irq_tests_5_rev_a_smoke() {
    smoke_mmc3_irq("blargg/mmc3_irq_tests/5.MMC3_rev_A.nes");
}

#[test]
fn mmc3_irq_tests_6_rev_b_smoke() {
    smoke_mmc3_irq("blargg/mmc3_irq_tests/6.MMC3_rev_B.nes");
}
