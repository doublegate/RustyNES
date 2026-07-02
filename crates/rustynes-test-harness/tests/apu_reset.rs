//! blargg `apu_reset/*.nes` corpus (6 sub-ROMs).
//!
//! Verifies APU register state at power-on and after a soft reset. These
//! ROMs use the `0x81` "Press RESET" protocol: they report `$6000 = 0x81`
//! at a precise cycle, expect the host to soft-reset the machine, then
//! validate the post-reset register state. The dedicated
//! [`run_nes_blargg_reset`] runner watches for the canonical magic
//! (`$DE $B0 $61`) and issues [`rustynes_core::Nes::reset`] when `0x81` appears —
//! without that the ROM sits at `0x81` forever.
//!
//! Per `docs/testing-strategy.md` §Layer 3.
//!
//! Observed status (v2.1.0 coverage wiring, R1 master-clock default build):
//!
//! - PASS: `4015_cleared`, `irq_flag_cleared`, `works_immediately`,
//!   `4017_timing`.
//! - FAIL #3 (`len_ctrs_enabled`): "At reset, length counters should be
//!   enabled, triangle unaffected" — suspect: APU length-counter state
//!   across the soft-reset path (the reset does not re-enable the length
//!   counters / leaves the triangle channel in the wrong state).
//! - FAIL #3 (`4017_written`): "At reset, $4017 should should be rewritten
//!   with last value written" — suspect: APU frame-counter ($4017) reset
//!   behaviour (the last-written $4017 value is not re-applied on reset).
//!
//! RESEARCHED + DEFERRED (v2.2.x accuracy polish): a spec-faithful fix was
//! prototyped — track the last `$4017` value in the APU, and on warm reset
//! re-apply it through the frame-counter write path with the mode bit (bit 7)
//! retained and the IRQ-inhibit bit (bit 6) force-cleared (per nesdev
//! "$4017 mode is unchanged, but IRQ inhibit flag is sometimes cleared").
//! It did NOT flip either of these two ROMs AND it regressed the
//! previously-passing `4017_timing` (which measures the cycle delay from the
//! effective reset `$4017` write to the frame-IRQ-flag set, expecting 6..=12).
//! Root cause: the harness's `Nes::reset()` is a function-call reset that does
//! not reproduce the cycle-accurate CPU reset delay (the 9-12 clock window the
//! reset vector waits) nor the exact frame-counter re-arm phase these
//! timing-sensitive ROMs bracket — so re-arming at the wrong phase shifts
//! `4017_timing`'s measured count out of range. The prototype held `AccuracyCoin`
//! 100% and the oracle 60/60 byte-identical (the change only touched the reset
//! path), but it was REVERTED because regressing a passing reset ROM without
//! flipping the two targets is a net loss. Closing these needs a
//! cycle-accurate reset-sequence model (the master-clock reset axis), not a
//! frame-granular re-arm.
//!
//! RE-VALIDATED (v2.2.x, independent re-attempt): the prototype was
//! re-implemented and measured. Re-applying the retained `$4017` value through
//! the frame-counter write path on warm reset (a) left `4017_written` at
//! FAIL #3 (status 0x03) and (b) shifted `4017_timing`'s reported "Delay after
//! effective $4017 write" to 12 (status 0x81 -> the ROM's accept window was
//! missed), flipping it FAIL; `len_ctrs_enabled` stayed FAIL #3. Net effect:
//! no target flipped, one passing ROM regressed -> REVERTED. Confirmed: these
//! two close only on the cycle-accurate reset-sequence (master-clock) axis.

#![cfg(feature = "test-roms")]

use std::fs;
use std::path::PathBuf;

use rustynes_test_harness::run_nes_blargg_reset;

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
    let path = rom_path(&format!("nes-test-roms/apu_reset/{name}"));
    let bytes = fs::read(&path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e));
    let r = run_nes_blargg_reset(&bytes, max_frames).expect("rom must parse + run");
    (r.status, r.message, r.frames)
}

#[test]
fn apu_reset_4015_cleared() {
    let (s, m, f) = run("4015_cleared.nes", 1500);
    eprintln!("4015_cleared: status={s:#x} frames={f} msg={m:?}");
    assert_eq!(s, 0, "4015_cleared failed: {m}");
}

#[test]
fn apu_reset_irq_flag_cleared() {
    let (s, m, f) = run("irq_flag_cleared.nes", 1500);
    eprintln!("irq_flag_cleared: status={s:#x} frames={f} msg={m:?}");
    assert_eq!(s, 0, "irq_flag_cleared failed: {m}");
}

#[test]
fn apu_reset_works_immediately() {
    let (s, m, f) = run("works_immediately.nes", 1500);
    eprintln!("works_immediately: status={s:#x} frames={f} msg={m:?}");
    assert_eq!(s, 0, "works_immediately failed: {m}");
}

#[test]
fn apu_reset_4017_timing() {
    let (s, m, f) = run("4017_timing.nes", 1500);
    eprintln!("4017_timing: status={s:#x} frames={f} msg={m:?}");
    assert_eq!(s, 0, "4017_timing failed: {m}");
}

// ---------------------------------------------------------------------------
// CLOSED (v2.0.0 beta.3): `len_ctrs_enabled` FAIL #3 was a HARNESS artifact,
// not a core residual. `run_nes_blargg_reset` re-detected the STALE `$81`
// status byte (WRAM survives a soft reset) immediately after issuing the
// reset, re-resetting the ROM mid-measurement and corrupting its second-pass
// log. With the stale-status guard the ROM passes strictly. Plan-residual R3
// reclassified.
// ---------------------------------------------------------------------------

#[test]
fn apu_reset_len_ctrs_enabled() {
    let (s, m, f) = run("len_ctrs_enabled.nes", 1500);
    eprintln!("len_ctrs_enabled: status={s:#x} frames={f} msg={m:?}");
    assert_eq!(s, 0, "len_ctrs_enabled failed: {m}");
}

// ---------------------------------------------------------------------------
// CLOSED (v2.0.0 beta.3, Workstream A4; shipped as the default in beta.4):
// the cycle-accurate reset sequence retains the last `$4017` value and
// re-issues it (mode bit kept, IRQ-inhibit cleared) 2 clocked cycles into
// the CPU's 8-cycle reset delay — plan-residual R4. (The quoted "should
// should" below is blargg's own string in `4017_written.s`, kept verbatim
// so the exact ROM output greps here.)
// ---------------------------------------------------------------------------

#[test]
fn apu_reset_4017_written() {
    let (s, m, f) = run("4017_written.nes", 1500);
    eprintln!("4017_written: status={s:#x} frames={f} msg={m:?}");
    assert_eq!(s, 0, "4017_written failed: {m}");
}
