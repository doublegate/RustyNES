//! v1.7.0 "Forge" Workstream F1 — battery-save (PRG-RAM / SRAM) round-trip
//! oracle.
//!
//! No test exercised battery-backed PRG-RAM persistence before this. Battery
//! saves ride the same snapshot/restore path the frontend uses to write a
//! `.sav` sidecar (and the mapper `save_state`/`load_state` blob carries the
//! PRG-RAM), so a round-trip through `Nes::snapshot` → `Nes::restore` is the
//! exact persistence mechanism a battery save uses.
//!
//! Driven by a SYNTHETIC battery-backed NROM (flags6 bit 1 set) running a tiny
//! 6502 program that fills `$6000-$60FF` (the start of the PRG-RAM / SRAM
//! window) with a known, address-derived pattern, then spins. The test then:
//!
//! 1. Runs the program until the pattern is written, and reads it back via the
//!    side-effect-free `peek` (the SRAM window is at `$6000-$7FFF`).
//! 2. Snapshots, restores into a FRESH emulator constructed from the same ROM,
//!    and asserts the SRAM pattern survived byte-for-byte — the battery-save
//!    round-trip guarantee.
//! 3. Asserts the restored emulator is bit-identical (cycle + framebuffer) to
//!    the source, so a battery restore resumes deterministically.
//!
//! Per `docs/testing-strategy.md` and `docs/cartridge-format.md`.

#![cfg(feature = "test-roms")]

use rustynes_core::Nes;

/// Build a 32 KiB battery-backed NROM whose reset vector points at a program
/// that fills `$6000..=$60FF` with `value(addr) = (addr_lo XOR 0xA5)` and then
/// spins forever (`JMP self`).
///
/// 6502 program (loaded at `$8000`, mirrored to `$C000` by NROM-128 logic, but
/// we use a full 32 KiB PRG so `$8000` is the live bank):
/// ```text
///   LDX #$00          ; A2 00
/// loop:
///   TXA               ; 8A
///   EOR #$A5          ; 49 A5
///   STA $6000,X       ; 9D 00 60
///   INX               ; E8
///   BNE loop          ; D0 F7   (back to `loop`)
/// spin:
///   JMP spin          ; 4C <spin_lo> <spin_hi>
/// ```
fn synth_battery_nrom() -> Vec<u8> {
    const PRG_KIB: usize = 32;
    let mut bytes = Vec::with_capacity(16 + PRG_KIB * 1024);
    bytes.extend_from_slice(b"NES\x1A");
    bytes.push(u8::try_from(PRG_KIB / 16).unwrap()); // PRG = 2 × 16 KiB
    bytes.push(0); // CHR = 0 (CHR-RAM)
    bytes.push(0x02); // flags6: bit 1 = battery-backed PRG-RAM
    bytes.push(0); // flags7
    bytes.extend_from_slice(&[0u8; 8]);

    let mut prg = vec![0u8; PRG_KIB * 1024];
    // Program at the start of PRG, which maps to CPU $8000.
    let prog: &[u8] = &[
        0xA2, 0x00, // LDX #$00
        0x8A, // TXA
        0x49, 0xA5, // EOR #$A5
        0x9D, 0x00, 0x60, // STA $6000,X
        0xE8, // INX
        0xD0, 0xF7, // BNE loop (-9 -> back to TXA at $8002)
        // spin at $800B:
        0x4C, 0x0B, 0x80, // JMP $800B
    ];
    prg[..prog.len()].copy_from_slice(prog);

    // Reset vector ($FFFC/$FFFD in CPU space) lives at the very top of the
    // 32 KiB PRG: offset 0x7FFC/0x7FFD. Point it at $8000.
    let len = prg.len();
    prg[len - 4] = 0x00; // reset lo
    prg[len - 3] = 0x80; // reset hi -> $8000
    // NMI ($FFFA) + IRQ ($FFFE) -> $800B (the spin), harmless.
    prg[len - 6] = 0x0B;
    prg[len - 5] = 0x80;
    prg[len - 2] = 0x0B;
    prg[len - 1] = 0x80;

    bytes.extend_from_slice(&prg);
    bytes
}

/// The expected SRAM byte at `$6000 + i`.
const fn expected_sram(i: u8) -> u8 {
    i ^ 0xA5
}

/// Read `$6000..=$60FF` from a (mutable) emulator via the side-effect-free peek.
fn read_sram_page(nes: &mut Nes) -> [u8; 256] {
    let mut out = [0u8; 256];
    for (i, slot) in out.iter_mut().enumerate() {
        *slot = nes.peek(0x6000 + u16::try_from(i).unwrap());
    }
    out
}

/// Run the program long enough to write the whole `$6000` page.
fn run_until_written(nes: &mut Nes) {
    // The fill loop is ~256 × 6 cycles ≈ 1.5 k cycles — well under one frame.
    // A handful of frames is ample headroom (and lets the reset sequence settle).
    for _ in 0..4 {
        nes.run_frame();
    }
}

#[test]
fn battery_sram_is_written_by_the_program() {
    let rom = synth_battery_nrom();
    let mut nes = Nes::from_rom(&rom).expect("synthetic battery ROM must parse");
    run_until_written(&mut nes);
    let sram = read_sram_page(&mut nes);
    for (i, &b) in sram.iter().enumerate() {
        let i = u8::try_from(i).unwrap();
        assert_eq!(
            b,
            expected_sram(i),
            "SRAM $6000+{i:#04X} = {b:#04X}, expected {:#04X}",
            expected_sram(i)
        );
    }
}

#[test]
fn battery_sram_survives_snapshot_restore_round_trip() {
    let rom = synth_battery_nrom();
    let mut src = Nes::from_rom(&rom).expect("ROM must parse");
    run_until_written(&mut src);
    let src_sram = read_sram_page(&mut src);
    let cycle = src.cycle();
    let fb: Vec<u8> = src.framebuffer().to_vec();

    // Persist (the battery-save mechanism) ...
    let blob = src.snapshot();

    // ... and restore into a FRESH emulator built from the same ROM (a cold
    // power-on whose SRAM would otherwise be uninitialised).
    let mut restored = Nes::from_rom(&rom).expect("ROM must parse");
    restored.restore(&blob).expect("battery blob must restore");

    let restored_sram = read_sram_page(&mut restored);
    assert_eq!(
        restored_sram, src_sram,
        "battery PRG-RAM did not survive the snapshot/restore round-trip"
    );
    // And the pattern is still the program's pattern, not zeroes.
    for (i, &b) in restored_sram.iter().enumerate() {
        let i = u8::try_from(i).unwrap();
        assert_eq!(
            b,
            expected_sram(i),
            "restored SRAM $6000+{i:#04X} corrupted"
        );
    }

    // The restore must also resume bit-identically (cycle + framebuffer).
    assert_eq!(
        restored.cycle(),
        cycle,
        "restore must reproduce the CPU cycle"
    );
    assert_eq!(
        restored.framebuffer(),
        fb.as_slice(),
        "restore must reproduce the framebuffer"
    );
}

#[test]
fn battery_round_trip_is_deterministic() {
    // Two independent emulators run + snapshot the same battery ROM produce the
    // same persisted blob (the battery save is reproducible).
    let rom = synth_battery_nrom();
    let mut a = Nes::from_rom(&rom).unwrap();
    let mut b = Nes::from_rom(&rom).unwrap();
    run_until_written(&mut a);
    run_until_written(&mut b);
    assert_eq!(
        a.snapshot(),
        b.snapshot(),
        "two runs of the same battery ROM must persist identical blobs"
    );
}
