//! blargg `cpu_reset` corpus (T-71-002, Phase 7 Sprint 1).
//!
//! Two sub-ROMs validating CPU reset semantics:
//! - `registers.nes` — A/X/Y/P/S register state at power and after a RESET.
//! - `ram_after_reset.nes` — that internal RAM is *preserved* across a warm
//!   RESET (only a power-cycle clears it).
//!
//! Both ROMs were already vendored under `tests/roms/assorted/` (see
//! `tests/roms/LICENSES.md`) but were not wired into a test until Phase 7.
//!
//! ## Headless limitation
//!
//! These are **interactive** ROMs: they display "Press reset AFTER this
//! message disappears" and expect an externally-asserted reset at a precise
//! moment relative to the on-screen message. The headless `run_nes_blargg`
//! `0x81`-handler resets on a fixed schedule that does not line up with that
//! window, so the full pass/fail protocol cannot complete headlessly (status
//! stalls at `0x81`). The `_full_protocol` tests below are therefore
//! `#[ignore]`'d and documented.
//!
//! What we *can* assert deterministically is the **power-on register dump**
//! that `registers.nes` prints before requesting the reset. It reports
//! `A X Y P S = 00 00 00 34 FD`, which validates the cold-boot register
//! contract (A/X/Y=0, P=$34, S=$FD) straight from the test author. The
//! reset-decrement (`S -= 3`) and RAM-preservation semantics are additionally
//! covered by `Cpu::power_on` / `Nes::reset` unit tests (Session-13,
//! `crates/rustynes-cpu/tests/opcodes.rs`).
//!
//! Source: blargg's NES test ROMs (public domain).

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

fn run(name: &str, max_frames: u64) -> rustynes_test_harness::NesTestResult {
    let path = rom_path(&format!("assorted/{name}"));
    let bytes = fs::read(&path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e));
    run_nes_blargg(&bytes, max_frames).unwrap_or_else(|e| panic!("rom must parse and run: {e}"))
}

/// Deterministic: the power-on register dump from `registers.nes` must match
/// the cold-boot contract (A/X/Y=$00, P=$34, S=$FD).
#[test]
fn cpu_reset_registers_power_on_state() {
    let result = run("cpu_reset_registers.nes", 200);
    assert!(
        result.message.contains("00 00 00 34 FD"),
        "power-on register dump mismatch (expected A X Y P S = 00 00 00 34 FD)\n\
         got status {:#x}, message:\n{}",
        result.status,
        result.message
    );
}

/// The full interactive reset protocol cannot complete headlessly (see the
/// module docs). Kept as an `#[ignore]`'d record so a future
/// interactive-reset harness can light it up.
#[test]
#[ignore = "interactive: needs externally-timed reset the headless 0x81-handler can't supply; \
            register/RAM semantics covered by Cpu::power_on / Nes::reset unit tests"]
fn cpu_reset_registers_full_protocol() {
    let result = run("cpu_reset_registers.nes", 600);
    assert_eq!(result.status, 0, "registers: {}", result.message);
}

/// Same headless limitation as above.
#[test]
#[ignore = "interactive: needs externally-timed reset the headless 0x81-handler can't supply; \
            warm-reset RAM preservation covered by Nes::reset / save-state unit tests"]
fn cpu_reset_ram_after_reset_full_protocol() {
    let result = run("cpu_reset_ram_after_reset.nes", 600);
    assert_eq!(result.status, 0, "ram_after_reset: {}", result.message);
}
