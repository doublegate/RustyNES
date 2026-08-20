//! Rung 0's gate: the harness must recognise agreement when agreement exists.
//!
//! Feed `RustyNES`'s own golden back in as if it were the device-under-test and
//! require zero divergences. Without this, every later red result is ambiguous
//! between "the RTL is wrong" and "my writer packs a field wrong" -- and the
//! second is far likelier early on.
//!
//! The test asserts both directions. A comparator that always reports agreement
//! would pass a self-diff trivially, so a corrupted copy must be **caught**; a
//! gate never shown to fail is not a gate.

use std::path::{Path, PathBuf};

use rustynes_cosim::Oracle;

/// A committed, public-domain test ROM. Not a commercial dump.
const ROM: &str = "../../tests/roms/mmc1_a12/mmc1_a12.nes";

const SEED: u64 = 12345;
const FRAMES: u64 = 5;

/// NTSC CPU cycles per frame, doubled so the .5 is exact.
///
/// Kept in integers deliberately: this test's whole job is an arithmetic check on
/// a frame count, and doing it in floating point would mean two lossy casts and a
/// lint suppression to check five frames' worth of cycles.
const HALF_CYCLES_PER_FRAME: u64 = 59_561;

/// How far from the exact count still counts as right: about 0.05 of a frame.
/// An off-by-one-frame error is ~59,561 half-cycles, so this cannot mask one.
const HALF_CYCLE_TOLERANCE: u64 = 3_000;

fn rom_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(ROM)
}

fn export(seed: u64, frames: u64) -> (Vec<u8>, Vec<u16>, Vec<u8>, u64, u64) {
    let rom = std::fs::read(rom_path()).expect("read test rom");
    let mut o = Oracle::new(&rom, seed).expect("parse test rom");
    o.enable_cpu_boot_trace(64 * 1024, 0, 20_000);
    let calls = o.advance_frames(frames);
    let boot = o
        .take_cpu_boot_trace_binary()
        .expect("boot trace was armed and must be present");
    let fb = o.nes().index_framebuffer().to_vec();
    let ram = o.nes().bus().ram_bytes().to_vec();
    (boot, fb, ram, o.nes().cycle(), calls)
}

/// Two independent exports of the same ROM and seed must be byte-identical.
///
/// This is the determinism contract observed at the boundary this crate exposes.
/// If it fails, a pre-generated golden is not the trace a lockstep run would have
/// produced, and replay-as-oracle is unsound.
#[test]
fn the_null_dut_self_diff_is_zero_across_every_format() {
    let (boot_a, fb_a, ram_a, cyc_a, calls_a) = export(SEED, FRAMES);
    let (boot_b, fb_b, ram_b, cyc_b, calls_b) = export(SEED, FRAMES);

    assert_eq!(boot_a, boot_b, "the boot trace diverged between two runs");
    assert_eq!(
        fb_a, fb_b,
        "the index framebuffer diverged between two runs"
    );
    assert_eq!(ram_a, ram_b, "work RAM diverged between two runs");
    assert_eq!(cyc_a, cyc_b, "the cycle count diverged between two runs");
    assert_eq!(calls_a, calls_b, "the call count diverged between two runs");

    assert!(!boot_a.is_empty(), "an empty boot trace is not a match");
    assert_eq!(fb_a.len(), 256 * 240);
    assert_eq!(ram_a.len(), 2048);
}

/// ...and a corrupted DUT must be **caught**, or the test above proves nothing.
///
/// A comparator that always reports agreement passes a self-diff trivially. This
/// flips one bit in the middle of the trace, which is the smallest divergence an
/// RTL bug could plausibly produce.
#[test]
fn a_single_flipped_bit_is_not_mistaken_for_agreement() {
    let (boot, ..) = export(SEED, FRAMES);
    assert!(boot.len() > 5000, "trace too short to corrupt meaningfully");
    let mut corrupt = boot.clone();
    corrupt[5000] ^= 0x01;
    assert_ne!(
        boot, corrupt,
        "a one-bit corruption compared equal -- the comparison is not comparing"
    );
}

/// The frame count must be checkable **without** trusting the counter that
/// produced it.
///
/// `advance_frames(5)` must simulate five NTSC frames' worth of cycles, and it
/// must take SIX `run_frame()` calls to do it -- the first after power-on is
/// swallowed by the `frame_complete` latch the reset sequence leaves set. A bare
/// `for _ in 0..5` loop lands at 4.0 frames here, under a manifest claiming 5.
#[test]
fn five_frames_requested_is_five_frames_of_cycles() {
    let (_, _, _, cycles, calls) = export(SEED, FRAMES);

    let expected_half_cycles = FRAMES * HALF_CYCLES_PER_FRAME;
    let actual_half_cycles = cycles * 2;
    let delta = actual_half_cycles.abs_diff(expected_half_cycles);
    assert!(
        delta <= HALF_CYCLE_TOLERANCE,
        "requested {FRAMES} frames but simulated {cycles} cycles; expected about \
         {} (off by {delta} half-cycles, tolerance {HALF_CYCLE_TOLERANCE}) -- an \
         off-by-one here means every golden is short by a frame under a manifest \
         that claims otherwise",
        expected_half_cycles / 2
    );
    assert_eq!(
        calls,
        FRAMES + 1,
        "expected one swallowed power-on call plus {FRAMES} real frames"
    );
}
