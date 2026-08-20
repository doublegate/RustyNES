//! `RustyNES` as a co-simulation oracle for an external HDL device-under-test.
//!
//! # What this is for
//!
//! v2.5.0 "Fabric" builds a NES core in `SystemVerilog`, written from public
//! hardware documentation, and verifies it against this emulator. This crate is
//! the boundary between the two: a narrow C ABI a Verilator C++ testbench can
//! link, plus the safe Rust API a golden-export binary uses.
//!
//! # Replay, not lockstep — and why that is not a compromise
//!
//! The obvious design is to step the DUT and the oracle together, one cycle at a
//! time. It is not available and should not be built: [`Nes`] exposes
//! `run_frame()` and `step_instruction()` and nothing finer, so cycle-lockstep
//! would mean new core API on the hot path — putting the determinism contract and
//! the ~3.9 ms/frame budget at risk to gain nothing.
//!
//! It gains nothing because **the determinism contract already makes replay
//! equivalent to lockstep**: same seed + ROM + input ⇒ bit-identical framebuffer
//! and audio, so a trace recorded now is exactly the trace a lockstep run would
//! have produced. Replay is additionally *better* in two ways — the goldens are
//! re-diffable without re-simulating, and the oracle and the DUT can run on
//! different machines at different times.
//!
//! So: `RustyNES` emits goldens ahead of time, the testbench writes the **same byte
//! formats** from the DUT, and the diff CLIs that already exist
//! (`cpu_boot_trace_diff`, `ppu_trace_diff`) compare them. The FPGA testbench
//! becomes the *third* writer of `cpu_boot_trace`, after this emulator and
//! `scripts/mesen2_cpu_boot_trace.lua` — a format with an existing foreign
//! producer is a format worth reusing.
//!
//! # The features are mandatory, deliberately
//!
//! `Cargo.toml` enables `cpu-boot-trace` and `irq-timing-trace` unconditionally
//! rather than exposing them as this crate's own optional features. A build
//! without them would compile, link, run, and export **empty** goldens — an
//! absence of signal that reads exactly like agreement. Making them mandatory
//! turns that into a compile error.
//!
//! # Safety
//!
//! Every `extern "C"` entry point is `unsafe` by necessity and documents the
//! invariant its caller must uphold. The handle is an opaque owned pointer:
//! created by [`rn_open`], destroyed by [`rn_close`], and never valid across
//! those. Borrowed slices returned by the accessors point into the emulator and
//! are invalidated by the next call that advances or mutates it — the C side must
//! copy before stepping, which the header comments state at each site.

// `unsafe_code` is allowed crate-wide because this crate IS an FFI boundary --
// the same rationale `rustynes-libretro`, `rustynes-cheevos` and
// `rustynes-frontend` carry. Every raw pointer here originates in a Verilator
// C++ testbench.
//
// This is a narrowing, not a waiver: `clippy::undocumented_unsafe_blocks` (a
// v2.3.9 gate) still applies, so every `unsafe` block must carry a `// SAFETY:`
// comment naming the invariant and who guarantees it. The allow silences the
// blanket "you used unsafe" warning; it does not silence the requirement to
// justify each use.
#![allow(unsafe_code)]

use core::ffi::{c_char, c_int, c_uchar, c_uint, c_ulonglong, c_void};

use rustynes_core::Nes;
use rustynes_core::cpu_boot_trace::{CpuBootTrace, CpuBootTraceConfig};

/// Opaque handle handed to C. One live `Nes` plus the buffers its accessors
/// borrow from.
pub struct Oracle {
    nes: Nes,
}

impl Oracle {
    /// Construct from ROM bytes and the power-on seed.
    ///
    /// The seed is not optional and not defaulted. It is the "same seed" half of
    /// the determinism contract this whole design rests on: CPU/PPU phase
    /// alignment and power-on RAM are seeded from it, so a DUT compared against a
    /// golden generated with a different seed will diverge from cycle zero for
    /// reasons that have nothing to do with its RTL.
    ///
    /// # Errors
    ///
    /// Returns `Err` if the ROM does not parse.
    pub fn new(rom: &[u8], seed: u64) -> Result<Self, String> {
        Nes::from_rom_with_power_on_seed(rom, seed)
            .map(|nes| Self { nes })
            .map_err(|e| format!("{e:?}"))
    }

    /// The emulator, for the safe Rust callers (the golden-export binary).
    pub const fn nes(&self) -> &Nes {
        &self.nes
    }

    /// The emulator, mutably.
    pub const fn nes_mut(&mut self) -> &mut Nes {
        &mut self.nes
    }

    /// Advance until the emulator's frame counter has risen by `frames`.
    ///
    /// **Not** the same as calling `run_frame()` `frames` times. The PPU is
    /// constructed at dot 340 of the pre-render line, so the 7-cycle reset
    /// sequence ticks past the frame wrap and latches `frame_complete`; the
    /// FIRST `run_frame()` after construction then consumes that latch and
    /// returns without stepping a single cycle. Measured, not inferred: frame 0
    /// advances the cycle counter by 0, frames 1..3 by ~29,780 each.
    ///
    /// Every existing caller runs thousands of frames, so one lost frame is
    /// invisible to them. It is not invisible here: `--frames 60` would emit a
    /// 59-frame golden under a manifest claiming 60 -- a provenance record that
    /// is wrong in the one direction that matters, since a DUT compared against
    /// it would be off by a frame for reasons nothing in the record explains.
    ///
    /// Gating on the counter rather than the call count is also robust to the
    /// quirk being fixed in the core later: this loop's meaning does not change.
    ///
    /// Returns the number of `run_frame()` calls actually made.
    pub fn advance_frames(&mut self, frames: u64) -> u64 {
        let target = self.nes.frame().saturating_add(frames);
        let mut calls = 0u64;
        while self.nes.frame() < target {
            self.nes.run_frame();
            calls += 1;
            // A jammed CPU (JAM/KIL/STP) stops advancing frames; without this
            // the loop would spin forever on a ROM that halts.
            if self.nes.is_jammed() {
                break;
            }
        }
        calls
    }

    /// Arm the per-instruction boot trace over a cycle window.
    pub fn enable_cpu_boot_trace(&mut self, capacity: usize, start: u64, end: u64) {
        let cfg = CpuBootTraceConfig::cycles(start..=end);
        self.nes
            .enable_cpu_boot_trace(CpuBootTrace::with_capacity(capacity, cfg));
    }

    /// Arm the per-cycle IRQ/bus trace.
    pub fn enable_irq_trace(&mut self, capacity: usize) {
        self.nes.bus_mut().enable_irq_trace(capacity);
    }

    /// The boot trace in its binary interchange format, or `None` if unarmed.
    ///
    /// This is the byte stream `cpu_boot_trace_diff` consumes and
    /// `scripts/mesen2_cpu_boot_trace.lua` also produces — 12-byte magic
    /// `RUSTYNES_CPU`, schema version, then packed records.
    pub fn take_cpu_boot_trace_binary(&mut self) -> Option<Vec<u8>> {
        self.nes.take_cpu_boot_trace().map(|t| t.to_binary())
    }

    /// The per-cycle IRQ/bus trace as CSV, or `None` if unarmed.
    pub fn take_irq_trace_csv(&mut self) -> Option<String> {
        self.nes.bus_mut().take_irq_trace().map(|t| t.to_csv())
    }
}

// ---------------------------------------------------------------------------
// C ABI
// ---------------------------------------------------------------------------

/// Create an oracle from ROM bytes.
///
/// Returns `NULL` if `rom` is null, `len` is zero, or the ROM does not parse.
///
/// # Safety
///
/// `rom` must point to at least `len` readable bytes. The returned pointer must
/// be released with [`rn_close`] exactly once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rn_open(
    rom: *const c_uchar,
    len: usize,
    seed: c_ulonglong,
) -> *mut c_void {
    if rom.is_null() || len == 0 {
        return core::ptr::null_mut();
    }
    // SAFETY: the caller guarantees `rom` points to `len` readable bytes, which
    // is the documented contract above; the slice does not outlive this call.
    let bytes = unsafe { core::slice::from_raw_parts(rom, len) };
    Oracle::new(bytes, seed).map_or(core::ptr::null_mut(), |o| {
        Box::into_raw(Box::new(o)).cast::<c_void>()
    })
}

/// Destroy an oracle. Passing `NULL` is a no-op.
///
/// # Safety
///
/// `handle` must have come from [`rn_open`] and must not be used afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rn_close(handle: *mut c_void) {
    if handle.is_null() {
        return;
    }
    // SAFETY: the caller guarantees this pointer came from `rn_open`'s
    // `Box::into_raw` and has not already been freed.
    drop(unsafe { Box::from_raw(handle.cast::<Oracle>()) });
}

/// Borrow the handle, or bail out when it is null.
///
/// Two arms: one returning `$default`, one returning nothing. The second exists
/// because `return ();` is a clippy error (`unneeded_unit`), and the alternative
/// -- an `#[allow]` at each of the three `void` entry points -- would spend a
/// lint suppression on a purely syntactic problem.
macro_rules! oracle {
    ($handle:expr) => {{
        if $handle.is_null() {
            return;
        }
        // SAFETY: checked non-null immediately above; the caller's contract is
        // that the pointer came from `rn_open` and is still live.
        unsafe { &mut *$handle.cast::<Oracle>() }
    }};
    ($handle:expr, $default:expr) => {{
        if $handle.is_null() {
            return $default;
        }
        // SAFETY: checked non-null immediately above; the caller's contract is
        // that the pointer came from `rn_open` and is still live.
        unsafe { &mut *$handle.cast::<Oracle>() }
    }};
}

/// Run one frame. Returns the cumulative CPU cycle count afterwards.
///
/// # Safety
///
/// `handle` must be a live pointer from [`rn_open`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rn_run_frame(handle: *mut c_void) -> c_ulonglong {
    let o = oracle!(handle, 0);
    o.nes.run_frame();
    o.nes.bus().cycle()
}

/// Execute one instruction. Returns the cycles it took.
///
/// # Safety
///
/// `handle` must be a live pointer from [`rn_open`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rn_step_instruction(handle: *mut c_void) -> c_uint {
    let o = oracle!(handle, 0);
    c_uint::from(o.nes.step_instruction())
}

/// The cumulative CPU cycle counter — the diff anchor both sides align on.
///
/// # Safety
///
/// `handle` must be a live pointer from [`rn_open`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rn_cycle(handle: *mut c_void) -> c_ulonglong {
    let o = oracle!(handle, 0);
    o.nes.bus().cycle()
}

/// Copy the **index** framebuffer into `out` — 256x240 `u16`s, each
/// `(emphasis << 6) | colour`.
///
/// Returns the number of `u16`s written, or 0 on a null handle or short buffer.
///
/// This, not the RGBA buffer, is the video comparison surface. The RGBA output is
/// a pure function of this given the same palette, so comparing RGBA lets a
/// palette difference masquerade as a rendering difference — the failure v2.3.8
/// "Parallax" exists to prevent. A DUT must therefore expose its palette index
/// and emphasis bits, not its final colour.
///
/// # Safety
///
/// `out` must point to at least `cap` writable `u16`s.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rn_index_framebuffer(
    handle: *mut c_void,
    out: *mut u16,
    cap: usize,
) -> usize {
    let o = oracle!(handle, 0);
    let fb = o.nes.index_framebuffer();
    if out.is_null() || cap < fb.len() {
        return 0;
    }
    // SAFETY: `out` is non-null and the caller guarantees `cap` writable u16s,
    // and `cap >= fb.len()` was just checked. The regions cannot overlap: `fb`
    // borrows emulator-owned memory, `out` is caller-owned.
    unsafe { core::ptr::copy_nonoverlapping(fb.as_ptr(), out, fb.len()) };
    fb.len()
}

/// Copy the 2 KiB CPU work RAM into `out`.
///
/// Returns bytes written, or 0 on a null handle or short buffer. This is what
/// `AccuracyCoin`'s RAM decoder reads: each test writes its result to a fixed
/// address in `$0000-$07FF`, so a DUT needs only a RAM dump path — no display
/// decoding — to be scored.
///
/// # Safety
///
/// `out` must point to at least `cap` writable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rn_ram(handle: *mut c_void, out: *mut c_uchar, cap: usize) -> usize {
    let o = oracle!(handle, 0);
    let ram = o.nes.bus().ram_bytes();
    if out.is_null() || cap < ram.len() {
        return 0;
    }
    // SAFETY: non-null and length-checked immediately above; disjoint regions.
    unsafe { core::ptr::copy_nonoverlapping(ram.as_ptr(), out, ram.len()) };
    ram.len()
}

/// Drain queued audio into `out`. Returns samples written.
///
/// # Safety
///
/// `out` must point to at least `cap` writable `f32`s.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rn_drain_audio(handle: *mut c_void, out: *mut f32, cap: usize) -> usize {
    let o = oracle!(handle, 0);
    if out.is_null() || cap == 0 {
        return 0;
    }
    // SAFETY: non-null and non-zero capacity checked above; the caller
    // guarantees `cap` writable f32s.
    let buf = unsafe { core::slice::from_raw_parts_mut(out, cap) };
    o.nes.drain_audio_into(buf)
}

/// Set the controller state for `port` (0..=3) from a bitmask.
///
/// Bit order is A, B, Select, Start, Up, Down, Left, Right — LSB first, matching
/// the shift-register order the hardware reports. The DUT must be driven from a
/// frame-indexed script byte-identical to the one fed here, or the two sides
/// diverge for input reasons rather than RTL reasons.
///
/// # Safety
///
/// `handle` must be a live pointer from [`rn_open`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rn_set_buttons(handle: *mut c_void, port: c_uint, mask: c_uchar) {
    let o = oracle!(handle);
    o.nes.set_buttons(
        port as usize,
        rustynes_core::Buttons::from_bits_truncate(mask),
    );
}

/// Arm the per-instruction boot trace over `[start, end]` CPU cycles.
///
/// # Safety
///
/// `handle` must be a live pointer from [`rn_open`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rn_enable_cpu_boot_trace(
    handle: *mut c_void,
    capacity: usize,
    start: c_ulonglong,
    end: c_ulonglong,
) {
    let o = oracle!(handle);
    o.enable_cpu_boot_trace(capacity, start, end);
}

/// Arm the per-cycle IRQ/bus trace.
///
/// # Safety
///
/// `handle` must be a live pointer from [`rn_open`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rn_enable_irq_trace(handle: *mut c_void, capacity: usize) {
    let o = oracle!(handle);
    o.enable_irq_trace(capacity);
}

/// Write the binary boot trace to `path`. Returns 0 on success, negative on error.
///
/// Writing from Rust rather than handing the bytes across the ABI keeps the
/// lifetime question trivial — there is no borrowed buffer for C to mismanage.
///
/// # Safety
///
/// `path` must be a NUL-terminated UTF-8 C string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rn_write_cpu_boot_trace(
    handle: *mut c_void,
    path: *const c_char,
) -> c_int {
    let o = oracle!(handle, -1);
    // SAFETY: the caller guarantees a NUL-terminated string; the borrow ends
    // before this function returns.
    let Some(p) = (unsafe { cstr_to_path(path) }) else {
        return -2;
    };
    o.take_cpu_boot_trace_binary()
        .map_or(-4, |bytes| match std::fs::write(p, bytes) {
            Ok(()) => 0,
            Err(_) => -3,
        })
}

/// Write the IRQ/bus trace CSV to `path`. Returns 0 on success, negative on error.
///
/// # Safety
///
/// `path` must be a NUL-terminated UTF-8 C string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rn_write_irq_trace_csv(handle: *mut c_void, path: *const c_char) -> c_int {
    let o = oracle!(handle, -1);
    // SAFETY: as above.
    let Some(p) = (unsafe { cstr_to_path(path) }) else {
        return -2;
    };
    o.take_irq_trace_csv()
        .map_or(-4, |csv| match std::fs::write(p, csv) {
            Ok(()) => 0,
            Err(_) => -3,
        })
}

/// Convert a C string to a path, or `None` if null or not UTF-8.
///
/// # Safety
///
/// `p` must be NUL-terminated, or null.
unsafe fn cstr_to_path(p: *const c_char) -> Option<std::path::PathBuf> {
    if p.is_null() {
        return None;
    }
    // SAFETY: non-null checked above; the caller guarantees NUL termination.
    let c = unsafe { core::ffi::CStr::from_ptr(p) };
    c.to_str().ok().map(std::path::PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nrom() -> Vec<u8> {
        // 16 KiB PRG + 8 KiB CHR NROM with a reset vector into an infinite loop,
        // enough to exercise the oracle without depending on a committed ROM.
        let mut rom = vec![0u8; 16 + 16384 + 8192];
        rom[..4].copy_from_slice(b"NES\x1a");
        rom[4] = 1; // 1 x 16 KiB PRG
        rom[5] = 1; // 1 x 8 KiB CHR
        let prg = 16;
        rom[prg] = 0x4C; // JMP $8000
        rom[prg + 1] = 0x00;
        rom[prg + 2] = 0x80;
        // Vectors live in the last 6 bytes of PRG, mirrored at $FFFA.
        let vec_base = prg + 16384 - 6;
        rom[vec_base] = 0x00; // NMI lo
        rom[vec_base + 1] = 0x80;
        rom[vec_base + 2] = 0x00; // RESET lo
        rom[vec_base + 3] = 0x80;
        rom[vec_base + 4] = 0x00; // IRQ lo
        rom[vec_base + 5] = 0x80;
        rom
    }

    /// The power-on latch, pinned rather than worked around silently.
    ///
    /// If this test ever fails it means the core stopped swallowing the first
    /// `run_frame()`. That is a fine thing to happen -- but `advance_frames`
    /// must be re-read when it does, and a silent change here would otherwise
    /// alter every golden's length with nothing to point at.
    #[test]
    fn the_first_run_frame_after_power_on_advances_nothing() {
        let mut o = Oracle::new(&nrom(), 0).expect("parse");
        let before = o.nes().cycle();
        o.nes_mut().run_frame();
        assert_eq!(
            o.nes().cycle(),
            before,
            "the power-on frame_complete latch is gone -- re-read Oracle::advance_frames"
        );
    }

    /// `advance_frames(n)` must advance the frame counter by exactly `n`,
    /// whatever the call count underneath.
    #[test]
    fn advance_frames_advances_the_counter_not_the_call_count() {
        let mut o = Oracle::new(&nrom(), 0).expect("parse");
        let f0 = o.nes().frame();
        let c0 = o.nes().cycle();
        let calls = o.advance_frames(3);
        assert_eq!(
            o.nes().frame(),
            f0 + 3,
            "frame counter did not advance by 3"
        );
        assert!(
            o.nes().cycle() > c0,
            "three frames advanced no cycles at all"
        );
        assert_eq!(
            calls, 4,
            "expected one wasted call for the power-on latch plus three real frames"
        );
    }

    /// A jammed CPU must not spin `advance_frames` forever.
    #[test]
    fn advance_frames_gives_up_on_a_jammed_cpu() {
        // $02 is a JAM/KIL opcode: the CPU halts and frames stop advancing.
        let mut rom = nrom();
        rom[16] = 0x02;
        let mut o = Oracle::new(&rom, 0).expect("parse");
        let calls = o.advance_frames(1_000_000);
        assert!(
            o.nes().is_jammed(),
            "the JAM opcode did not jam the CPU -- this test proves nothing"
        );
        assert!(calls < 1_000_000, "advance_frames did not bail on a jam");
    }

    /// The determinism contract, at the boundary this crate exposes.
    ///
    /// This is the property replay-instead-of-lockstep rests on: if two oracles
    /// built from the same ROM and seed could diverge, a pre-generated golden
    /// would not be the trace a lockstep run produces, and the whole design
    /// would be unsound. Asserted here rather than assumed.
    #[test]
    fn same_seed_same_rom_gives_an_identical_index_framebuffer() {
        let rom = nrom();
        let (mut a, mut b) = (
            Oracle::new(&rom, 0x1234_5678).expect("a"),
            Oracle::new(&rom, 0x1234_5678).expect("b"),
        );
        for _ in 0..10 {
            a.nes_mut().run_frame();
            b.nes_mut().run_frame();
        }
        assert_eq!(
            a.nes().index_framebuffer(),
            b.nes().index_framebuffer(),
            "same seed + same ROM diverged -- replay-as-oracle is unsound if this fails"
        );
        assert_eq!(a.nes().bus().cycle(), b.nes().bus().cycle());
    }

    /// A different seed must be able to produce a different result, or the seed
    /// parameter is decorative and the previous test proves nothing.
    ///
    /// Deliberately weak: power-on RAM and CPU/PPU phase differ by seed, but a
    /// trivial ROM may not *observe* that difference. The assertion is therefore
    /// on the seed reaching the emulator at all, not on divergence.
    #[test]
    fn the_seed_is_carried_into_the_emulator() {
        let rom = nrom();
        let a = Oracle::new(&rom, 0).expect("a");
        let b = Oracle::new(&rom, u64::MAX).expect("b");
        // Snapshots embed power-on state, so two seeds must not produce byte-
        // identical snapshots even before anything runs.
        assert_ne!(
            a.nes().snapshot(),
            b.nes().snapshot(),
            "the seed did not reach power-on state; goldens would not be reproducible"
        );
    }

    #[test]
    fn a_bad_rom_is_rejected_rather_than_panicking() {
        assert!(Oracle::new(b"not a rom", 0).is_err());
    }

    #[test]
    fn the_c_abi_rejects_null_and_empty_input() {
        // SAFETY: deliberately passing null/zero to exercise the guards.
        unsafe {
            assert!(rn_open(core::ptr::null(), 0, 0).is_null());
            assert!(rn_open(core::ptr::null(), 16, 0).is_null());
            rn_close(core::ptr::null_mut()); // must not fault
            assert_eq!(rn_run_frame(core::ptr::null_mut()), 0);
            assert_eq!(rn_cycle(core::ptr::null_mut()), 0);
        }
    }

    #[test]
    fn the_c_abi_round_trips_a_rom() {
        let rom = nrom();
        // SAFETY: `rom` outlives the call; the handle is closed exactly once.
        unsafe {
            let h = rn_open(rom.as_ptr(), rom.len(), 7);
            assert!(!h.is_null());
            let c = rn_run_frame(h);
            assert!(c > 0);

            let mut fb = vec![0u16; 256 * 240];
            assert_eq!(
                rn_index_framebuffer(h, fb.as_mut_ptr(), fb.len()),
                256 * 240
            );
            // A short buffer must be refused, not partially filled.
            assert_eq!(rn_index_framebuffer(h, fb.as_mut_ptr(), 10), 0);

            let mut ram = vec![0u8; 2048];
            assert_eq!(rn_ram(h, ram.as_mut_ptr(), ram.len()), 2048);
            assert_eq!(rn_ram(h, ram.as_mut_ptr(), 10), 0);

            rn_close(h);
        }
    }

    use std::path::{Path, PathBuf};

    // ---- rung 0's gate ----------------------------------------------------
    //
    // These live HERE rather than in `tests/`, and the reason is a build
    // constraint rather than a preference. This crate declares
    // `crate-type = ["rlib", "staticlib", "cdylib"]` because a Verilator
    // testbench links the C ABI. Under `--release` the workspace profile sets
    // `panic = "abort"`, so the dependency graph is built with that strategy --
    // and an INTEGRATION test in `tests/` is a separate binary that links the
    // rlib and needs `unwind`, which cannot share those dependencies:
    //
    //   error: the crate `sha2` requires panic strategy `abort` which is
    //   incompatible with this crate's strategy of `unwind`
    //
    // It compiles clean in debug, so a local `cargo test` says nothing; CI's
    // `cargo test --workspace --release --features test-roms` is what caught it.
    // `rustynes-libretro` is the same shape and has never hit this, because its
    // tests are `#[cfg(test)]` in `src/` and it declares no `rlib` at all.
    //
    // Nothing is lost by moving them: these drive the crate's public API exactly
    // as an external consumer would. If they ever need to be a separate binary
    // again, the crate-type is what has to change first.

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
}
