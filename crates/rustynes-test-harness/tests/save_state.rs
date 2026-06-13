//! End-to-end save state + rewind tests.
//!
//! Phase 5 Sprint 2 (T-52-001..-005). Driven by synthetic NROM ROMs so
//! the suite can run without committing commercial dumps.

use rustynes_core::Nes;

fn synth_nrom(prg_kib: usize, chr_kib: usize) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(16 + prg_kib * 1024 + chr_kib * 1024);
    bytes.extend_from_slice(b"NES\x1A");
    bytes.push(u8::try_from(prg_kib / 16).unwrap());
    bytes.push(u8::try_from(chr_kib / 8).unwrap());
    bytes.push(0); // flags6
    bytes.push(0); // flags7
    bytes.extend_from_slice(&[0u8; 8]);

    let mut prg = vec![0u8; prg_kib * 1024];
    if prg_kib >= 16 {
        // JMP $C000 -> 4C 00 C0
        prg[0] = 0x4C;
        prg[1] = 0x00;
        prg[2] = 0xC0;
        let len = prg.len();
        prg[len - 4] = 0x00;
        prg[len - 3] = 0xC0;
        prg[len - 6] = 0x00;
        prg[len - 5] = 0xC0;
        prg[len - 2] = 0x00;
        prg[len - 1] = 0xC0;
    }
    bytes.extend_from_slice(&prg);
    bytes.extend_from_slice(&vec![0u8; chr_kib * 1024]);
    bytes
}

fn fnv(b: &[u8]) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for &b in b {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

#[test]
fn save_state_round_trip_preserves_emulator_state() {
    let rom = synth_nrom(16, 8);
    let mut nes = Nes::from_rom(&rom).unwrap();
    for _ in 0..30 {
        nes.run_frame();
    }
    let cycle = nes.cycle();
    let fb_hash = fnv(nes.framebuffer());
    let blob = nes.snapshot();
    // Drift forward.
    for _ in 0..30 {
        nes.run_frame();
    }
    nes.restore(&blob).unwrap();
    assert_eq!(nes.cycle(), cycle);
    assert_eq!(fnv(nes.framebuffer()), fb_hash);
}

#[test]
fn save_state_blob_starts_with_rustynes_magic() {
    let rom = synth_nrom(16, 8);
    let mut nes = Nes::from_rom(&rom).unwrap();
    nes.run_frame();
    let blob = nes.snapshot();
    assert_eq!(&blob[..8], b"RUSTYNES");
}

#[test]
fn rewind_step_back_returns_prior_frames() {
    let rom = synth_nrom(16, 8);
    let mut nes = Nes::from_rom(&rom).unwrap();
    nes.enable_rewind_with(8 * 1024 * 1024, 1);
    let mut cycles = Vec::new();
    for _ in 0..15 {
        nes.run_frame();
        cycles.push(nes.cycle());
    }
    // Rewind 5 steps. cycles[i] is the state at the END of frame i+1
    // (= run_frame call i, 0-indexed). Each step_back pops the most
    // recent entry and restores to it. After 5 pops (and 5 restorations),
    // the current state matches cycles[15 - 5] = cycles[10].
    for _ in 0..5 {
        assert!(nes.rewind_step_back());
    }
    assert_eq!(nes.cycle(), cycles[15 - 5]);
}

#[test]
fn snapshot_is_deterministic_for_two_emulators() {
    let rom = synth_nrom(16, 8);
    let mut a = Nes::from_rom(&rom).unwrap();
    let mut b = Nes::from_rom(&rom).unwrap();
    for _ in 0..10 {
        a.run_frame();
        b.run_frame();
    }
    assert_eq!(a.snapshot(), b.snapshot());
}

/// Synthetic NROM ROM that keeps the DMC + OAM DMA machinery permanently
/// hot: a looping DMC sample at the fastest rate (so reload DMAs fire every
/// ~432 CPU cycles forever), an OAM DMA spammed every loop iteration, and a
/// `$4015` disable/re-enable toggle each iteration so the W3-Stage-3
/// delayed-`$4015` status machinery (pending slot + countdown + the
/// implicit-abort latches) is repeatedly in flight when frames end.
fn synth_dmc_oam_rom() -> Vec<u8> {
    let mut bytes = Vec::with_capacity(16 + 16 * 1024 + 8 * 1024);
    bytes.extend_from_slice(b"NES\x1A");
    bytes.push(1); // 16 KiB PRG
    bytes.push(1); // 8 KiB CHR
    bytes.push(0); // flags6
    bytes.push(0); // flags7
    bytes.extend_from_slice(&[0u8; 8]);

    let mut prg = vec![0u8; 16 * 1024];
    // PRG loads at $C000 (16 KiB NROM mirrors at $8000/$C000).
    let program: &[u8] = &[
        0xA9, 0x4F, // LDA #$4F      ; DMC: loop=1, rate idx 15 (fastest)
        0x8D, 0x10, 0x40, // STA $4010
        0xA9, 0x00, // LDA #$00
        0x8D, 0x12, 0x40, // STA $4012 ; sample addr $C000
        0xA9, 0x01, // LDA #$01
        0x8D, 0x13, 0x40, // STA $4013 ; sample length 17 bytes
        0xA9, 0x10, // LDA #$10
        0x8D, 0x15, 0x40, // STA $4015 ; enable DMC (load DMA + loop chain)
        // loop (at $C014, byte offset 20):
        0xA9, 0x02, // LDA #$02
        0x8D, 0x14, 0x40, // STA $4014 ; OAM DMA from page 2
        0xA9, 0x00, // LDA #$00
        0x8D, 0x15, 0x40, // STA $4015 ; DMC disable (delayed-status path)
        0xA9, 0x10, // LDA #$10
        0x8D, 0x15, 0x40, // STA $4015 ; DMC re-enable (restart race path)
        0x4C, 0x14, 0xC0, // JMP $C014 ; -> loop
    ];
    prg[..program.len()].copy_from_slice(program);
    // Reset vector -> $C000; NMI/IRQ -> a lone RTI at $C040.
    prg[0x40] = 0x40; // RTI
    let len = prg.len();
    prg[len - 6] = 0x40; // NMI lo -> $C040
    prg[len - 5] = 0xC0;
    prg[len - 4] = 0x00; // RESET -> $C000
    prg[len - 3] = 0xC0;
    prg[len - 2] = 0x40; // IRQ/BRK -> $C040
    prg[len - 1] = 0xC0;
    bytes.extend_from_slice(&prg);
    bytes.extend_from_slice(&vec![0u8; 8 * 1024]);
    bytes
}

/// W3-Stage-4 (2026-06-10): byte-identical EMULATION continuation through a
/// save + restore taken while the DMC reload chain, the OAM DMA latch, and
/// the delayed-`$4015` machinery are hot. Under the promoted `mc-r1-full-cpu`
/// umbrella this exercises every Stage-4-serialized field (the CPU R1
/// pipeline, the APU parity/exclusion/delayed-status tail, the BUS engine
/// state, the PPU W2 countdowns) -- any of them unserialized would shift a
/// DMC/OAM DMA stall and diverge the cumulative cycle count and framebuffer
/// within a frame or two. On the default build it pins the same contract for
/// the lockstep scheduler.
///
/// The audio STREAM is deliberately not bit-compared across the restore
/// boundary: the blip pending-sample queue and the band-limited synthesis
/// accumulator are documented as intentionally NOT preserved (see
/// `rustynes-apu/src/snapshot.rs`), so a restored run has a short, by-design
/// audio transient (and a correspondingly perturbed IIR filter tail) even
/// though the EMULATED state is exact. The re-encode test below covers the
/// serialized-state completeness axis instead.
#[test]
fn save_state_continuation_byte_identical_with_dmc_and_oam_dma_hot() {
    let rom = synth_dmc_oam_rom();
    let mut a = Nes::from_rom(&rom).unwrap();
    for _ in 0..12 {
        a.run_frame();
        // Drain audio like a real frontend does (the pending queue is
        // capped; an undrained run would wedge sample production).
        let _ = a.drain_audio();
    }
    let blob = a.snapshot();
    let mut b = Nes::from_rom(&rom).unwrap();
    b.restore(&blob).unwrap();
    assert_eq!(a.cycle(), b.cycle(), "restore must reproduce the cycle");
    for frame in 0..12 {
        a.run_frame();
        b.run_frame();
        assert_eq!(
            a.cycle(),
            b.cycle(),
            "cycle count diverged at continuation frame {frame}"
        );
        assert_eq!(
            fnv(a.framebuffer()),
            fnv(b.framebuffer()),
            "framebuffer diverged at continuation frame {frame}"
        );
        let _ = a.drain_audio();
        let _ = b.drain_audio();
    }
}

/// W3-Stage-4 (2026-06-10): encode -> decode -> re-encode stability at
/// every frame of the hot-DMA program. A field that is serialized but not
/// restored (or restored but cleared) re-encodes differently and trips
/// this immediately.
#[test]
fn save_state_re_encode_stable_every_frame_with_dmc_machinery_hot() {
    let rom = synth_dmc_oam_rom();
    let mut nes = Nes::from_rom(&rom).unwrap();
    for _ in 0..5 {
        nes.run_frame();
        let _ = nes.drain_audio();
    }
    for frame in 0..10 {
        let blob = nes.snapshot();
        let mut fresh = Nes::from_rom(&rom).unwrap();
        fresh.restore(&blob).unwrap();
        assert_eq!(
            fresh.snapshot(),
            blob,
            "restore + re-encode must reproduce the blob at frame {frame}"
        );
        nes.run_frame();
        let _ = nes.drain_audio();
    }
}

#[test]
fn snapshot_round_trip_preserves_full_byte_stream() {
    let rom = synth_nrom(16, 8);
    let mut nes = Nes::from_rom(&rom).unwrap();
    for _ in 0..5 {
        nes.run_frame();
    }
    let blob_a = nes.snapshot();
    nes.restore(&blob_a).unwrap();
    let blob_b = nes.snapshot();
    assert_eq!(
        blob_a, blob_b,
        "snapshot bytes must be reproducible after restore"
    );
}
