//! The fixture ROM, assembled by hand (no ROM file, no licence): a 16 KiB
//! NROM whose main loop waits for vblank, reads controller 1 into `$10`, adds
//! it into a running sum at `$12`, counts frames in `$11`, and paints the
//! backdrop colour from the sum. Shared by the integration tests (through
//! `common/mod.rs`) and the crate's unit tests (by `#[path]`).

#![allow(dead_code)]

/// The main loop, at `$C000`.
const PROGRAM: &[u8] = &[
    0x78, // C000 SEI
    0xD8, // C001 CLD
    0xA2, 0xFF, // C002 LDX #$FF
    0x9A, // C004 TXS
    0x2C, 0x02, 0x20, // C005 wait: BIT $2002
    0x10, 0xFB, // C008 BPL wait
    0xA9, 0x01, // C00A LDA #1
    0x8D, 0x16, 0x40, // C00C STA $4016   strobe on
    0xA9, 0x00, // C00F LDA #0
    0x8D, 0x16, 0x40, // C011 STA $4016   strobe off
    0x85, 0x10, // C014 STA $10      clear the button byte
    0xA2, 0x08, // C016 LDX #8
    0xAD, 0x16, 0x40, // C018 read: LDA $4016
    0x4A, // C01B LSR A        bit 0 -> carry
    0x26, 0x10, // C01C ROL $10
    0xCA, // C01E DEX
    0xD0, 0xF7, // C01F BNE read
    0xA5, 0x10, // C021 LDA $10
    0x18, // C023 CLC
    0x65, 0x12, // C024 ADC $12
    0x85, 0x12, // C026 STA $12      running sum
    0xE6, 0x11, // C028 INC $11      frame count
    0xAD, 0x02, 0x20, // C02A LDA $2002   reset the address latch
    0xA9, 0x3F, // C02D LDA #$3F
    0x8D, 0x06, 0x20, // C02F STA $2006
    0xA9, 0x00, // C032 LDA #0
    0x8D, 0x06, 0x20, // C034 STA $2006
    0xA5, 0x12, // C037 LDA $12
    0x29, 0x3F, // C039 AND #$3F
    0x8D, 0x07, 0x20, // C03B STA $2007   backdrop = sum
    0xA9, 0x00, // C03E LDA #0
    0x8D, 0x06, 0x20, // C040 STA $2006
    0x8D, 0x06, 0x20, // C043 STA $2006   v = 0, so the backdrop shows
    0x4C, 0x05, 0xC0, // C046 JMP wait
];

/// Address of the `LDA $10` just after the controller read loop.
pub const AFTER_READ_PC: u16 = 0xC021;

/// Address of the `JMP wait` at the end of the loop (a breakpoint target).
pub const LOOP_END_PC: u16 = 0xC046;

/// The fixture as an iNES image.
#[must_use]
pub fn input_rom() -> Vec<u8> {
    let mut prg = vec![0xEAu8; 16 * 1024];
    prg[..PROGRAM.len()].copy_from_slice(PROGRAM);
    let n = prg.len();
    for v in [n - 6, n - 4, n - 2] {
        prg[v] = 0x00; // NMI / RESET / IRQ -> $C000
        prg[v + 1] = 0xC0;
    }
    let mut rom = b"NES\x1A".to_vec();
    rom.extend_from_slice(&[1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    rom.extend_from_slice(&prg);
    rom.extend_from_slice(&[0u8; 8 * 1024]);
    rom
}
