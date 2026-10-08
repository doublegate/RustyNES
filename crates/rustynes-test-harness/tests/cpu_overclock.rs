//! `T-CPU-OVERCLOCK` (v3.1.0, FE-01, D22): the CPU-multiplier overclock.
//!
//! `Nes::set_cpu_overclock(k)` runs the CPU `k` times faster against an
//! unchanged PPU, by dividing the region's master-clock CPU divider, while the
//! APU, the DMC and every mapper's CPU-cycle hook stay at the stock rate, so
//! a game gets `k` times the CPU time per frame with its pitch, its tempo and
//! its cycle-timed mapper IRQs unchanged. Off by default (`1`), and `1` is
//! byte-identical to stock (pinned separately by the epoch fingerprint gate,
//! whose seven ROMs all run at `1`).
//!
//! What these tests pin:
//! * the default and the clamp (`1..=4`);
//! * the multiplier reaches the core: CPU cycles per frame scale by `k`;
//! * the APU stays at the stock rate: the same number of audio samples per
//!   frame at every `k`;
//! * it rides in [`HardwareOptions`] (capture, encode, decode, and a named
//!   difference), which is what refuses a movie or a netplay peer recorded
//!   with another value;
//! * a snapshot taken mid-run under the overclock restores to the same
//!   continuation, which is what run-ahead and rewind rely on.

#![cfg(feature = "test-roms")]

mod common;

use std::fs;

use common::{fnv1a64, rom_path};
use rustynes_core::save_state::{BinReader, BinWriter};
use rustynes_core::{HardwareOptions, Nes};

const ROM: &str = "nes-test-roms/ny2011/ny2011.nes";
/// Audible from about frame 120 (ny2011 is silent for its first 300 frames,
/// which made the snapshot test below blind to the APU's phase: a restore
/// that dropped the stock-rate position passed it).
const AUDIBLE_ROM: &str = "nes-test-roms/apu_mixer/square.nes";

fn boot_rom(rom: &str) -> Nes {
    let path = rom_path(rom);
    let bytes = fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    Nes::from_rom(&bytes).unwrap_or_else(|e| panic!("parse {rom}: {e}"))
}

fn boot() -> Nes {
    boot_rom(ROM)
}

/// CPU cycles and audio samples over `frames` frames at overclock `k`.
fn measure(k: u8, frames: u32) -> (u64, usize) {
    let mut nes = boot();
    nes.set_cpu_overclock(k);
    // Settle past power-on first, so the frame boundary is a steady one.
    for _ in 0..30 {
        nes.run_frame();
        let _ = nes.drain_audio();
    }
    let start = nes.cycle();
    let mut samples = 0;
    for _ in 0..frames {
        nes.run_frame();
        samples += nes.drain_audio().len();
    }
    (nes.cycle() - start, samples)
}

#[test]
fn the_default_is_stock_and_the_multiplier_is_clamped() {
    let mut nes = boot();
    assert_eq!(nes.cpu_overclock(), 1, "off by default");
    nes.set_cpu_overclock(0);
    assert_eq!(nes.cpu_overclock(), 1, "0 means stock, not a stopped CPU");
    nes.set_cpu_overclock(9);
    assert_eq!(
        nes.cpu_overclock(),
        rustynes_core::MAX_CPU_OVERCLOCK,
        "clamped to the maximum"
    );
}

#[test]
fn cpu_cycles_per_frame_scale_with_the_multiplier() {
    let frames = 60;
    let (stock, _) = measure(1, frames);
    for k in 2..=rustynes_core::MAX_CPU_OVERCLOCK {
        let (cycles, _) = measure(k, frames);
        #[allow(clippy::cast_precision_loss)]
        let ratio = cycles as f64 / stock as f64;
        let want = f64::from(k);
        assert!(
            (ratio - want).abs() < 0.01,
            "x{k}: {cycles} CPU cycles over {frames} frames against {stock} stock \
             is a ratio of {ratio:.4}, not {want}"
        );
    }
}

#[test]
fn the_apu_stays_at_the_stock_rate() {
    let frames = 60;
    let (_, stock) = measure(1, frames);
    for k in 2..=rustynes_core::MAX_CPU_OVERCLOCK {
        let (_, samples) = measure(k, frames);
        // One sample of slack per frame for where the frame boundary falls
        // inside a resampler step.
        assert!(
            samples.abs_diff(stock) <= frames as usize,
            "x{k}: {samples} audio samples over {frames} frames against {stock} \
             stock; an APU clocked with the CPU would produce about {k} times as many"
        );
    }
}

#[test]
fn the_multiplier_rides_in_hardware_options() {
    let mut nes = boot();
    nes.set_cpu_overclock(3);
    let opts = HardwareOptions::capture(&nes);
    assert_eq!(opts.cpu_overclock, 3);

    let mut w = BinWriter::with_capacity(64);
    opts.write_to(&mut w);
    let bytes = w.into_vec();
    let back = HardwareOptions::read_from(&mut BinReader::new(&bytes)).expect("decodes");
    assert_eq!(back, opts, "encode/decode round trip");

    assert_eq!(HardwareOptions::default().cpu_overclock, 1);
    // Against the same machine captured at stock, so only the multiplier
    // differs (the default's `vs_ppu_type` is `None`, a capture's is not).
    let stock = HardwareOptions::capture(&boot());
    assert_eq!(stock.cpu_overclock, 1);
    assert_eq!(
        stock.differences(&opts),
        vec!["CPU overclock"],
        "a movie or peer on another value is refused with the option named"
    );

    let mut other = boot();
    opts.apply_live(&mut other).expect("apply");
    assert_eq!(other.cpu_overclock(), 3, "apply reaches the core");
}

#[test]
fn an_out_of_range_multiplier_in_a_file_is_refused() {
    let mut opts = HardwareOptions::default();
    opts.cpu_overclock = 2;
    let mut w = BinWriter::with_capacity(64);
    opts.write_to(&mut w);
    let mut bytes = w.into_vec();
    let at = bytes
        .iter()
        .position(|&b| b == 2)
        .expect("the multiplier byte is in the encoding");
    bytes[at] = rustynes_core::MAX_CPU_OVERCLOCK + 1;
    assert!(
        HardwareOptions::read_from(&mut BinReader::new(&bytes)).is_err(),
        "a multiplier the core would clamp cannot replay as written"
    );
}

#[test]
fn a_snapshot_under_the_overclock_restores_to_the_same_continuation() {
    let mut nes = boot_rom(AUDIBLE_ROM);
    nes.set_cpu_overclock(3);
    for _ in 0..150 {
        nes.run_frame();
    }
    // Audio already produced is host-side output, not machine state: drain
    // it so both runs below start from an empty buffer.
    let _ = nes.drain_audio();
    let snap = nes.snapshot();
    let run = |nes: &mut Nes| {
        let mut h = Vec::new();
        for _ in 0..20 {
            nes.run_frame();
            h.extend_from_slice(&fnv1a64(nes.framebuffer()).to_le_bytes());
            for s in nes.drain_audio() {
                h.extend_from_slice(&s.to_le_bytes());
            }
        }
        (fnv1a64(&h), nes.cycle())
    };
    // The window must be audible, or the APU's phase is not observed.
    {
        let mut probe = boot_rom(AUDIBLE_ROM);
        probe.set_cpu_overclock(3);
        for _ in 0..150 {
            probe.run_frame();
        }
        let _ = probe.drain_audio();
        probe.run_frame();
        let peak = probe.drain_audio().iter().fold(0f32, |m, s| m.max(s.abs()));
        assert!(peak > 0.01, "the snapshot window is silent (peak {peak})");
    }
    let first = run(&mut nes);
    nes.restore(&snap).expect("own snapshot restores");
    let second = run(&mut nes);
    assert_eq!(
        first, second,
        "a restore under the overclock must replay the same frames, samples and cycles"
    );
}
