//! `T-PS-dual-runahead` (v3.1.0, FE-09, D27): rewind in Vs. `DualSystem` mode.
//!
//! ADR 0032's 2026-10-07 amendment lifts rewind and run-ahead into the
//! two-console cabinet, built on the `RVSD` container that already snapshots
//! both consoles and the latch wiring them. Its gate, pinned here for rewind
//! (run-ahead is pinned in `rustynes-frontend`, where it lives):
//!
//! * a rewind across a cabinet frame restores BOTH framebuffers
//!   byte-identically;
//! * the cabinet then replays the same frames it produced the first time.
//!
//! The stimulus is a synthetic `DualSystem` cart built for the purpose. The
//! protocol cart in `vs_dualsystem_synth.rs` leaves the PPU off, so both of
//! its screens are one unchanging colour and a framebuffer comparison against
//! it would pass whatever a restore did to the pictures. This cart enables
//! background rendering and, in each console's NMI, writes a per-frame counter
//! into the backdrop entry `$3F00`. Its CHR-RAM is blank, so every pixel shows
//! the backdrop: each screen changes colour every frame, and the two screens
//! differ (the main cycles `$10-$17`, the sub `$20-$27`), so a restore that
//! swapped, dropped or staled either framebuffer is visible. The eight colours
//! avoid the palette's blacks (`$xD-$xF`): a full 64-entry counter walks
//! through three of them in a row, and consecutive frames are then identical.

use rustynes_core::{Emu, VsDualSystem};

/// FNV-1a 64, as the other harness tests hash framebuffers.
fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325_u64, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// One console's program: wait for the PPU to warm up, enable background
/// rendering and the NMI, then spin. The NMI increments `$00` and writes
/// `base | ($00 & 7)` into `$3F00`.
#[rustfmt::skip]
fn program(base: u8) -> Vec<u8> {
    vec![
        /* 8000 */ 0x78,             // SEI
        /* 8001 */ 0xD8,             // CLD
        /* 8002 */ 0xA2, 0xFF,       // LDX #$FF
        /* 8004 */ 0x9A,             // TXS
        /* 8005 */ 0x2C, 0x02, 0x20, // BIT $2002   (first vblank)
        /* 8008 */ 0x10, 0xFB,       // BPL $8005
        /* 800A */ 0x2C, 0x02, 0x20, // BIT $2002   (second vblank)
        /* 800D */ 0x10, 0xFB,       // BPL $800A
        /* 800F */ 0xA9, 0x0A,       // LDA #$0A    (background on)
        /* 8011 */ 0x8D, 0x01, 0x20, // STA $2001
        /* 8014 */ 0xA9, 0x80,       // LDA #$80    (NMI on)
        /* 8016 */ 0x8D, 0x00, 0x20, // STA $2000
        /* 8019 */ 0x4C, 0x19, 0x80, // JMP $8019
        // NMI at $801C.
        /* 801C */ 0xE6, 0x00,       // INC $00
        /* 801E */ 0xA5, 0x00,       // LDA $00
        /* 8020 */ 0x29, 0x07,       // AND #$07
        /* 8022 */ 0x09, base,       // ORA #base
        /* 8024 */ 0xEA,             // NOP       (keeps the layout below)
        /* 8025 */ 0xA2, 0x3F,       // LDX #$3F
        /* 8027 */ 0x8E, 0x06, 0x20, // STX $2006
        /* 802A */ 0xA2, 0x00,       // LDX #$00
        /* 802C */ 0x8E, 0x06, 0x20, // STX $2006
        /* 802F */ 0x8D, 0x07, 0x20, // STA $2007   ($3F00 = the backdrop)
        /* 8032 */ 0x8E, 0x06, 0x20, // STX $2006   (v back to $0000)
        /* 8035 */ 0x8E, 0x06, 0x20, // STX $2006
        /* 8038 */ 0x40,             // RTI         (also the IRQ vector)
    ]
}

/// A NES 2.0 `DualSystem` cart (mapper 99, Vs. hardware type 5), 64 KiB PRG:
/// the main program in the first half, the sub program in the second, each
/// with its own vectors. CHR-RAM, left blank.
fn build_flashing_cabinet() -> Vec<u8> {
    let mut rom = vec![0u8; 16 + 0x10000];
    rom[0..4].copy_from_slice(b"NES\x1a");
    rom[4] = 0x04; // 4 x 16 KiB PRG
    rom[6] = 0x30; // mapper 99, low nibble
    rom[7] = 0x69; // mapper 99 high nibble | NES 2.0 | Vs. System
    rom[11] = 0x07; // 8 KiB CHR-RAM
    rom[13] = 0x50; // Vs. hardware type 5 (DualSystem), PPU type 0
    let prg = &mut rom[16..];
    for (half, base) in [(0usize, 0x10u8), (0x8000, 0x20)] {
        let code = program(base);
        prg[half..half + code.len()].copy_from_slice(&code);
        // NMI = $801C, RESET = $8000, IRQ = $8038 (an RTI).
        prg[half + 0x7FFA..half + 0x8000].copy_from_slice(&[0x1C, 0x80, 0x00, 0x80, 0x38, 0x80]);
    }
    rom
}

fn cabinet() -> VsDualSystem {
    match Emu::from_rom(&build_flashing_cabinet()).expect("the synthetic cart parses") {
        Emu::Dual(d) => *d,
        Emu::Single(_) => panic!("Vs. hardware type 5 must build a cabinet"),
    }
}

/// Both screens' hashes, main first.
fn screens(dual: &VsDualSystem) -> (u64, u64) {
    (fnv(dual.main_framebuffer()), fnv(dual.sub_framebuffer()))
}

#[test]
fn the_stimulus_changes_both_screens_every_frame_and_they_differ() {
    let mut dual = cabinet();
    for _ in 0..10 {
        dual.run_frame();
    }
    let mut seen = Vec::new();
    for _ in 0..4 {
        dual.run_frame();
        let s = screens(&dual);
        assert_ne!(s.0, s.1, "the two screens show different colours");
        seen.push(s);
    }
    for pair in seen.windows(2) {
        assert_ne!(pair[0].0, pair[1].0, "the main screen changes every frame");
        assert_ne!(pair[0].1, pair[1].1, "the sub screen changes every frame");
    }
}

#[test]
fn rewind_is_off_by_default_and_a_step_back_without_it_changes_nothing() {
    let mut dual = cabinet();
    assert!(!dual.rewind_enabled());
    for _ in 0..5 {
        dual.run_frame();
    }
    let before = dual.snapshot();
    assert!(!dual.rewind_step_back(), "nothing to step back to");
    assert_eq!(dual.snapshot(), before, "the cabinet is untouched");
    assert_eq!(dual.rewind_len(), 0);
}

#[test]
fn a_rewind_across_cabinet_frames_restores_both_framebuffers_exactly() {
    let mut dual = cabinet();
    dual.enable_rewind_with(rustynes_core::REWIND_DEFAULT_MAX_BYTES, 4);
    for _ in 0..10 {
        dual.run_frame();
    }
    // Record what frames 11..=20 looked like, and the whole state after each.
    let mut history = Vec::new();
    for _ in 0..10 {
        dual.run_frame();
        history.push((screens(&dual), dual.snapshot(), dual.main().frame()));
    }
    assert_eq!(dual.rewind_len(), 20, "one entry per frame");

    // The first step back pops the newest entry: the frame on screen now.
    assert!(dual.rewind_step_back());
    let (now_screens, now_state, _) = &history[9];
    assert_eq!(screens(&dual), *now_screens);
    assert_eq!(dual.snapshot(), *now_state);

    // Each further step lands on the frame before, both screens and the
    // whole cabinet byte for byte, across keyframes and deltas alike.
    for back in (0..9).rev() {
        assert!(dual.rewind_step_back(), "entry for history[{back}]");
        let (want_screens, want_state, want_frame) = &history[back];
        assert_eq!(dual.main().frame(), *want_frame);
        assert_eq!(
            screens(&dual),
            *want_screens,
            "both framebuffers after stepping back to history[{back}]"
        );
        assert_eq!(
            dual.snapshot(),
            *want_state,
            "the whole cabinet after stepping back to history[{back}]"
        );
    }
}

#[test]
fn play_resumed_after_a_rewind_replays_the_same_frames() {
    let mut dual = cabinet();
    dual.enable_rewind_with(rustynes_core::REWIND_DEFAULT_MAX_BYTES, 4);
    for _ in 0..12 {
        dual.run_frame();
    }
    let mut first = Vec::new();
    for _ in 0..6 {
        dual.run_frame();
        first.push(screens(&dual));
    }
    // Back to the frame before the six: the newest entry, then six more.
    for _ in 0..7 {
        assert!(dual.rewind_step_back());
    }
    let mut second = Vec::new();
    for _ in 0..6 {
        dual.run_frame();
        second.push(screens(&dual));
    }
    assert_eq!(
        first, second,
        "the replay matches the original run, both screens"
    );
}

#[test]
fn capture_off_frames_stay_out_of_the_ring() {
    let mut dual = cabinet();
    dual.enable_rewind();
    dual.run_frame();
    dual.set_rewind_capture(false);
    assert!(!dual.rewind_capture_enabled());
    dual.run_frame();
    dual.run_frame();
    dual.set_rewind_capture(true);
    assert_eq!(
        dual.rewind_len(),
        1,
        "only the frame captured with capture on"
    );
}

#[test]
fn a_loud_restore_and_a_power_cycle_empty_the_ring_and_a_quiet_one_keeps_it() {
    let mut dual = cabinet();
    dual.enable_rewind();
    for _ in 0..3 {
        dual.run_frame();
    }
    let snap = dual.snapshot();
    dual.restore_quiet(&snap).expect("own snapshot");
    assert_eq!(dual.rewind_len(), 3, "a quiet restore keeps the ring");
    dual.restore(&snap).expect("own snapshot");
    assert_eq!(dual.rewind_len(), 0, "a loaded state replaces the timeline");
    dual.run_frame();
    dual.power_cycle();
    assert_eq!(dual.rewind_len(), 0, "a power cycle ends the timeline");
    assert!(dual.rewind_enabled(), "but rewind stays on");
}
