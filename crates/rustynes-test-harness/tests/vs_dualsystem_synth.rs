//! Vs. `DualSystem` synthetic protocol verification (v2.0.0 beta.5).
//!
//! The four commercial `DualSystem` titles cannot serve as CI oracles: the
//! circulating 32 KiB "GVS" dumps are the MAME `maincpu` region ONLY (the
//! sub CPU's program ROMs — e.g. `balonfgt`'s `mds-bf4 a-3.6d`/`.6a`,
//! which differ from the main's `1d`/`1a` by CRC — are absent), so their
//! boot handshake provably cannot complete on ANY emulator. These tests
//! instead build a synthetic NES 2.0 `DualSystem` cart (64 KiB PRG = two
//! DIFFERENT hand-assembled 32 KiB programs) that exercises every wire of
//! the cabinet model:
//!
//! - **Detection**: NES 2.0 byte-13 high nibble = 5 (Vs. `DualSystem`) must
//!   route [`Emu::from_rom`] to the dual constructor;
//! - **Sub PRG banking**: the sub console must execute the SECOND PRG
//!   half (the two halves are different programs — if the sub ran the
//!   main's program, its `$4016` bit-7 identity check would fail-mark);
//! - **`$4016` bit-7 identity**: main reads 0, sub reads `$80`;
//! - **Shared WRAM**: a mailbox exchange (`$11` → `$22`) through
//!   `$6000`/`$6001` proves both consoles see one converged RAM;
//! - **Cross-IRQ**: main asserts the sub's `/IRQ` (bit-1 LOW), the sub's
//!   handler answers through WRAM (`$33`) and pulses the main's `/IRQ`
//!   back (main's handler sets a zero-page flag);
//! - **Snapshot round-trip**: the `RVSD` container restores into a fresh
//!   cabinet and both consoles continue cycle-identically.
//!
//! The final protocol state is four marker bytes in the shared WRAM
//! (`$6000..$6003` = `$11 $22 $33 $44`) plus a clean `$6004 == 0`
//! (either console's identity-failure path writes `$EE` there).

use rustynes_core::{Emu, VsDualSystem};

/// Assemble the synthetic `DualSystem` cart: NES 2.0 header (mapper 99,
/// Vs. System console, byte-13 hardware type 5 = `DualSystem`), 64 KiB PRG
/// (main program in the first half, sub program in the second), CHR-RAM.
fn build_dual_rom() -> Vec<u8> {
    let mut rom = vec![0u8; 16 + 0x10000];
    // --- NES 2.0 header ---
    rom[0..4].copy_from_slice(b"NES\x1a");
    rom[4] = 0x04; // 4 x 16 KiB PRG = 64 KiB
    rom[5] = 0x00; // CHR-RAM
    rom[6] = 0x30; // mapper 99 low nibble (0x63 & 0x0F = 3) << 4
    rom[7] = 0x69; // mapper high nibble 6 | NES 2.0 id (0x08) | Vs. System (0x01)
    rom[11] = 0x07; // CHR-RAM: 64 << 7 = 8 KiB
    rom[13] = 0x50; // Vs. hardware type 5 (DualSystem) << 4, PPU type 0

    // --- MAIN program (PRG offset 0, CPU $8000) ---
    #[rustfmt::skip]
    let main_prog: &[u8] = &[
        /* 8000 */ 0x78,             // SEI
        /* 8001 */ 0xD8,             // CLD
        /* 8002 */ 0xA2, 0xFF,       // LDX #$FF
        /* 8004 */ 0x9A,             // TXS
        /* 8005 */ 0xAD, 0x16, 0x40, // LDA $4016   (identity: bit 7 must be 0)
        /* 8008 */ 0x30, 0x24,       // BMI $802E   (fail)
        /* 800A */ 0xA9, 0x11,       // LDA #$11
        /* 800C */ 0x8D, 0x00, 0x60, // STA $6000   (mailbox: main -> sub)
        /* 800F */ 0xAD, 0x01, 0x60, // LDA $6001   (wait for the sub's ack)
        /* 8012 */ 0xC9, 0x22,       // CMP #$22
        /* 8014 */ 0xD0, 0xF9,       // BNE $800F
        /* 8016 */ 0xA9, 0x00,       // LDA #$00
        /* 8018 */ 0x85, 0x05,       // STA $05     (IRQ-seen flag)
        /* 801A */ 0x58,             // CLI
        /* 801B */ 0xA9, 0x00,       // LDA #$00
        /* 801D */ 0x8D, 0x16, 0x40, // STA $4016   (bit1 LOW: assert sub /IRQ)
        /* 8020 */ 0xAD, 0x02, 0x60, // LDA $6002   (wait for the sub's answer)
        /* 8023 */ 0xC9, 0x33,       // CMP #$33
        /* 8025 */ 0xD0, 0xF9,       // BNE $8020
        /* 8027 */ 0xA5, 0x05,       // LDA $05     (wait for our own IRQ flag)
        /* 8029 */ 0xF0, 0xFC,       // BEQ $8027
        /* 802B */ 0x4C, 0x34, 0x80, // JMP $8034   (finish)
        /* 802E */ 0xA9, 0xEE,       // LDA #$EE    (identity FAIL marker)
        /* 8030 */ 0x8D, 0x04, 0x60, // STA $6004
        /* 8033 */ 0xEA,             // NOP
        /* 8034 */ 0xA9, 0x02,       // LDA #$02    (raise bit1: stop sub storm)
        /* 8036 */ 0x8D, 0x16, 0x40, // STA $4016
        /* 8039 */ 0xA9, 0x44,       // LDA #$44
        /* 803B */ 0x8D, 0x03, 0x60, // STA $6003   (final success marker)
        /* 803E */ 0x4C, 0x3E, 0x80, // JMP $803E   (done)
    ];
    // Main IRQ handler at $8050: set the flag, return.
    #[rustfmt::skip]
    let main_irq: &[u8] = &[
        /* 8050 */ 0x48,             // PHA
        /* 8051 */ 0xA9, 0x01,       // LDA #$01
        /* 8053 */ 0x85, 0x05,       // STA $05
        /* 8055 */ 0x68,             // PLA
        /* 8056 */ 0x40,             // RTI
    ];
    let prg = &mut rom[16..16 + 0x10000];
    prg[0..main_prog.len()].copy_from_slice(main_prog);
    prg[0x0050..0x0050 + main_irq.len()].copy_from_slice(main_irq);
    // Main vectors (PRG offset 0x7FFA): NMI=$803E, RESET=$8000, IRQ=$8050.
    prg[0x7FFA..0x8000].copy_from_slice(&[0x3E, 0x80, 0x00, 0x80, 0x50, 0x80]);

    // --- SUB program (PRG offset 0x8000, CPU $8000 on the sub console) ---
    #[rustfmt::skip]
    let sub_prog: &[u8] = &[
        /* 8000 */ 0x78,             // SEI
        /* 8001 */ 0xD8,             // CLD
        /* 8002 */ 0xA2, 0xFF,       // LDX #$FF
        /* 8004 */ 0x9A,             // TXS
        /* 8005 */ 0xAD, 0x16, 0x40, // LDA $4016   (identity: bit 7 must be 1)
        /* 8008 */ 0x10, 0x24,       // BPL $802E   (fail)
        /* 800A */ 0xAD, 0x00, 0x60, // LDA $6000   (wait for the main's mailbox)
        /* 800D */ 0xC9, 0x11,       // CMP #$11
        /* 800F */ 0xD0, 0xF9,       // BNE $800A
        /* 8011 */ 0xA9, 0x22,       // LDA #$22
        /* 8013 */ 0x8D, 0x01, 0x60, // STA $6001   (ack)
        /* 8016 */ 0x58,             // CLI         (IRQ answers from here on)
        /* 8017 */ 0x4C, 0x17, 0x80, // JMP $8017   (idle)
        /* 801A */ 0xEA, 0xEA, 0xEA, 0xEA,
        /* 801E */ 0xEA, 0xEA, 0xEA, 0xEA,
        /* 8022 */ 0xEA, 0xEA, 0xEA, 0xEA,
        /* 8026 */ 0xEA, 0xEA, 0xEA, 0xEA,
        /* 802A */ 0xEA, 0xEA, 0xEA, 0xEA,
        /* 802E */ 0xA9, 0xEE,       // LDA #$EE    (identity FAIL marker)
        /* 8030 */ 0x8D, 0x04, 0x60, // STA $6004
        /* 8033 */ 0x4C, 0x33, 0x80, // JMP $8033
    ];
    // Sub IRQ handler at $8050: answer through WRAM, pulse the main's /IRQ.
    #[rustfmt::skip]
    let sub_irq: &[u8] = &[
        /* 8050 */ 0x48,             // PHA
        /* 8051 */ 0xA9, 0x33,       // LDA #$33
        /* 8053 */ 0x8D, 0x02, 0x60, // STA $6002
        /* 8056 */ 0xA9, 0x00,       // LDA #$00
        /* 8058 */ 0x8D, 0x16, 0x40, // STA $4016   (bit1 LOW: assert main /IRQ)
        /* 805B */ 0xEA, 0xEA, 0xEA, 0xEA, // (hold the pulse a few cycles)
        /* 805F */ 0xEA, 0xEA, 0xEA, 0xEA,
        /* 8063 */ 0xA9, 0x02,       // LDA #$02
        /* 8065 */ 0x8D, 0x16, 0x40, // STA $4016   (raise: release main /IRQ)
        /* 8068 */ 0x68,             // PLA
        /* 8069 */ 0x40,             // RTI
    ];
    prg[0x8000..0x8000 + sub_prog.len()].copy_from_slice(sub_prog);
    prg[0x8050..0x8050 + sub_irq.len()].copy_from_slice(sub_irq);
    // Sub vectors (PRG offset 0xFFFA): NMI=$8017, RESET=$8000, IRQ=$8050.
    prg[0xFFFA..0x10000].copy_from_slice(&[0x17, 0x80, 0x00, 0x80, 0x50, 0x80]);
    rom
}

/// Run the handshake to completion (well under a frame; a few frames give
/// slack for the power-on alignment) and return the cabinet.
fn run_handshake() -> VsDualSystem {
    let rom = build_dual_rom();
    let emu = Emu::from_rom(&rom).expect("synthetic dual cart must parse");
    let mut dual = match emu {
        Emu::Dual(d) => *d,
        Emu::Single(_) => {
            panic!("NES 2.0 byte-13 hardware type 5 must construct a VsDualSystem")
        }
    };
    for _ in 0..5 {
        dual.run_frame();
    }
    dual
}

/// Read the five protocol markers as seen from one console.
fn markers(nes: &mut rustynes_core::Nes) -> [u8; 5] {
    let b = nes.bus_mut();
    [
        b.debug_peek_cpu(0x6000),
        b.debug_peek_cpu(0x6001),
        b.debug_peek_cpu(0x6002),
        b.debug_peek_cpu(0x6003),
        b.debug_peek_cpu(0x6004),
    ]
}

#[test]
fn dual_handshake_completes_on_synthetic_cart() {
    let mut dual = run_handshake();
    assert!(!dual.main().is_jammed(), "main CPU jammed");
    assert!(!dual.sub().is_jammed(), "sub CPU jammed");
    // Both consoles must see the fully converged shared WRAM: the main's
    // mailbox, the sub's ack, the sub's IRQ-driven answer, the main's
    // final marker — and NO identity-failure marker (which would flag a
    // bit-7 or PRG-half-banking regression).
    let (main, sub) = dual.split_mut();
    let m = markers(main);
    let s = markers(sub);
    assert_eq!(
        m,
        [0x11, 0x22, 0x33, 0x44, 0x00],
        "main console's view of the protocol markers"
    );
    assert_eq!(
        s,
        [0x11, 0x22, 0x33, 0x44, 0x00],
        "sub console's view of the protocol markers"
    );
}

#[test]
fn dual_consoles_run_different_prg_halves() {
    let dual = run_handshake();
    // The two programs park in DIFFERENT idle loops: main at $803E, sub at
    // $8017. If the sub had banked the main's PRG half, the exchange above
    // could never complete and neither PC would sit in its idle loop.
    let main_pc = dual.main().cpu().pc;
    let sub_pc = dual.sub().cpu().pc;
    assert!(
        (0x803E..=0x8040).contains(&main_pc),
        "main must park in its done loop, got {main_pc:04X}"
    );
    // The sub idles at $8017 but may momentarily sit inside its IRQ
    // handler ($8050..$8069) if the main's release write races the sample.
    assert!(
        (0x8017..=0x8019).contains(&sub_pc) || (0x8050..=0x806A).contains(&sub_pc),
        "sub must park in its idle loop (or IRQ tail), got {sub_pc:04X}"
    );
}

#[test]
fn dual_snapshot_round_trips_on_synthetic_cart() {
    let mut a = run_handshake();
    let snap = a.snapshot();

    let rom = build_dual_rom();
    let mut b = VsDualSystem::from_rom(&rom).expect("fresh cabinet must construct");
    b.restore(&snap).expect("dual snapshot must restore");

    // The restored cabinet must carry the converged WRAM markers...
    let (bm, bs) = b.split_mut();
    assert_eq!(markers(bm), [0x11, 0x22, 0x33, 0x44, 0x00]);
    assert_eq!(markers(bs), [0x11, 0x22, 0x33, 0x44, 0x00]);
    // ...and continue cycle-identically with the original.
    for _ in 0..30 {
        a.run_frame();
        b.run_frame();
    }
    assert_eq!(a.main().cycle(), b.main().cycle(), "main cycle diverged");
    assert_eq!(a.sub().cycle(), b.sub().cycle(), "sub cycle diverged");
    assert_eq!(
        a.main_framebuffer(),
        b.main_framebuffer(),
        "main framebuffers diverged after restore"
    );
    assert_eq!(
        a.sub_framebuffer(),
        b.sub_framebuffer(),
        "sub framebuffers diverged after restore"
    );
}
