//! v2.9.6 "Roster": every new mapper family, booted through the whole
//! `Nes` (parse -> construct -> run loop -> PPU), on a synthetic CC0 program
//! built here from the register descriptions on each board's `NESdev` page.
//!
//! None of these boards has a redistributable ROM, so this file is the
//! board-level fixture the plan asks for. `rustynes-mappers`' unit tests pin
//! each register decode in isolation; this file checks that the same writes
//! land when a 6502 performs them on the real bus, with the PPU in the loop
//! for pattern-table and nametable effects and the IRQ line wired to the
//! CPU. For each board it:
//!
//! 1. runs a short program of register writes and records what the CPU (or
//!    the PPU, through `$2006`/`$2007`) then sees, into RAM at `$0300`;
//! 2. where the board has an IRQ, arms it, lets it fire, and counts the
//!    handler's runs at `$0200`;
//! 3. snapshots the machine mid-run, runs on, restores, and checks the rerun
//!    is identical (the board's save state is complete enough to replay).
//!
//! **Image layout.** Every 8 KiB PRG bank starts with its own bank number
//! (low, high), and every 1 KiB CHR bank does the same, so a recorded byte
//! names the bank the board selected. Every 8 KiB PRG bank also carries the
//! same program at offset `$1000` and the same vectors, so whichever bank a
//! board maps at `$E000` runs it. That is the "faux fixed bank" the GTROM
//! page recommends, and it makes a bank switch under the running code safe
//! on every board here.

// The emitter splits addresses and bank numbers into their low and high
// bytes with `as u8`, which is the point of the cast, not an accident.
#![allow(clippy::cast_possible_truncation)]

use rustynes_core::Nes;

const PRG_8K: usize = 0x2000;
const CHR_1K: usize = 0x0400;
/// CPU address of the program: offset `$1000` of the bank at `$E000`.
const CODE: u16 = 0xF000;
/// CPU address of the IRQ handler.
const IRQ: u16 = 0xF800;
/// Where recorded results go.
const RESULTS: u16 = 0x0300;
/// The IRQ handler's run counter.
const IRQ_COUNT: u16 = 0x0200;

/// One step of a board's test program.
#[derive(Clone, Copy)]
enum Step {
    /// `LDA #v; STA addr`.
    Write(u16, u8),
    /// `LDA addr; STA $0300+n` (n counts records in order).
    Record(u16),
    /// Write `v` to PPU address `a` through `$2006`/`$2007`.
    WritePpu(u16, u8),
    /// Read PPU address `a` (dummy read, then the real one) into `$0300+n`.
    RecordPpu(u16),
}

/// A tiny 6502 emitter: only the handful of instructions the fixture needs.
#[derive(Default)]
struct Asm(Vec<u8>);

impl Asm {
    fn lda_imm(&mut self, v: u8) {
        self.0.extend_from_slice(&[0xA9, v]);
    }
    fn lda(&mut self, a: u16) {
        self.0.extend_from_slice(&[0xAD, a as u8, (a >> 8) as u8]);
    }
    fn sta(&mut self, a: u16) {
        self.0.extend_from_slice(&[0x8D, a as u8, (a >> 8) as u8]);
    }
    fn write(&mut self, a: u16, v: u8) {
        self.lda_imm(v);
        self.sta(a);
    }
    /// `BIT $2002; BPL *-3`: wait for vblank.
    fn wait_vblank(&mut self) {
        self.0.extend_from_slice(&[0x2C, 0x02, 0x20, 0x10, 0xFB]);
    }
    fn ppu_addr(&mut self, a: u16) {
        self.write(0x2006, (a >> 8) as u8);
        self.write(0x2006, a as u8);
    }
}

/// The board's program: init, the steps, then the IRQ arming, then a spin.
fn program(steps: &[Step], arm_irq: &[(u16, u8)]) -> Vec<u8> {
    let mut a = Asm::default();
    // SEI; CLD; LDX #$FF; TXS
    a.0.extend_from_slice(&[0x78, 0xD8, 0xA2, 0xFF, 0x9A]);
    // Inhibit the APU frame IRQ. Without this the frame counter's IRQ, which
    // the handler never acknowledges, re-enters the handler continuously and
    // every "the board's IRQ fired" check passes vacuously; the first draft
    // of this file did exactly that (211 handler runs for a one-shot IRQ).
    a.write(0x4017, 0x40);
    // The PPU ignores $2006 for its first ~29,658 cycles: two vblanks.
    a.wait_vblank();
    a.wait_vblank();
    let mut n = 0u16;
    for &s in steps {
        match s {
            Step::Write(addr, v) => a.write(addr, v),
            Step::Record(addr) => {
                a.lda(addr);
                a.sta(RESULTS + n);
                n += 1;
            }
            Step::WritePpu(addr, v) => {
                a.ppu_addr(addr);
                a.write(0x2007, v);
            }
            Step::RecordPpu(addr) => {
                a.ppu_addr(addr);
                a.lda(0x2007);
                a.lda(0x2007);
                a.sta(RESULTS + n);
                n += 1;
            }
        }
    }
    for &(addr, v) in arm_irq {
        a.write(addr, v);
    }
    if !arm_irq.is_empty() {
        a.0.push(0x58); // CLI
    }
    // JMP * (spin).
    let here = CODE + a.0.len() as u16;
    a.0.extend_from_slice(&[0x4C, here as u8, (here >> 8) as u8]);
    a.0
}

/// The IRQ handler: `PHA; INC $0200; <ack writes>; PLA; RTI`.
fn handler(ack: &[(u16, u8)]) -> Vec<u8> {
    let mut a = Asm::default();
    a.0.push(0x48);
    a.0.extend_from_slice(&[0xEE, IRQ_COUNT as u8, (IRQ_COUNT >> 8) as u8]);
    for &(addr, v) in ack {
        a.write(addr, v);
    }
    a.0.extend_from_slice(&[0x68, 0x40]);
    a.0
}

/// An iNES / NES 2.0 image for `mapper`.
struct Board<'a> {
    mapper: u16,
    submapper: u8,
    prg_8k: usize,
    chr_1k: usize,
    steps: &'a [Step],
    arm_irq: &'a [(u16, u8)],
    ack_irq: &'a [(u16, u8)],
}

impl Board<'_> {
    fn rom(&self) -> Vec<u8> {
        let code = program(self.steps, self.arm_irq);
        let irq = handler(self.ack_irq);
        assert!(code.len() < 0x800, "program overlaps the handler");
        let mut prg = vec![0xFFu8; self.prg_8k * PRG_8K];
        for b in 0..self.prg_8k {
            let base = b * PRG_8K;
            prg[base] = b as u8;
            prg[base + 1] = (b >> 8) as u8;
            prg[base + 0x1000..base + 0x1000 + code.len()].copy_from_slice(&code);
            prg[base + 0x1800..base + 0x1800 + irq.len()].copy_from_slice(&irq);
            for (off, v) in [(0x1FFA, 0xF000u16), (0x1FFC, CODE), (0x1FFE, IRQ)] {
                prg[base + off] = v as u8;
                prg[base + off + 1] = (v >> 8) as u8;
            }
        }
        let mut chr = vec![0u8; self.chr_1k * CHR_1K];
        for b in 0..self.chr_1k {
            chr[b * CHR_1K] = b as u8;
            chr[b * CHR_1K + 1] = (b >> 8) as u8;
        }
        // Always NES 2.0: the submapper and the exact sizes matter here.
        let prg_16k = (self.prg_8k / 2) as u16;
        let chr_8k = (self.chr_1k / 8) as u16;
        let mut h = [0u8; 16];
        h[0..4].copy_from_slice(b"NES\x1A");
        h[4] = prg_16k as u8;
        h[5] = chr_8k as u8;
        h[6] = ((self.mapper & 0x0F) << 4) as u8 | 0x01; // vertical
        h[7] = (self.mapper & 0xF0) as u8 | 0x08; // NES 2.0
        h[8] = ((self.mapper >> 8) as u8 & 0x0F) | (self.submapper << 4);
        h[9] = ((chr_8k >> 8) as u8) << 4 | (prg_16k >> 8) as u8;
        // No PRG-RAM declared; a CHR-RAM board declares 8 KiB.
        h[11] = if self.chr_1k == 0 { 0x07 } else { 0 };
        let mut rom = h.to_vec();
        rom.extend_from_slice(&prg);
        rom.extend_from_slice(&chr);
        rom
    }

    /// Boot, run, and return `(results, irq_count)`; also checks that a
    /// mid-run snapshot replays identically.
    fn run(&self, records: usize) -> (Vec<u8>, u8) {
        let rom = self.rom();
        let mut nes = Nes::from_rom(&rom).unwrap_or_else(|e| {
            panic!(
                "mapper {}.{} must parse: {e:?}",
                self.mapper, self.submapper
            )
        });
        for _ in 0..4 {
            nes.run_frame();
        }
        let snap = nes.snapshot();
        for _ in 0..3 {
            nes.run_frame();
        }
        let after = nes.wram().to_vec();
        nes.restore(&snap).expect("restore");
        for _ in 0..3 {
            nes.run_frame();
        }
        assert_eq!(
            nes.wram(),
            &after[..],
            "mapper {}.{}: a restored snapshot must replay identically",
            self.mapper,
            self.submapper
        );
        let base = usize::from(RESULTS);
        (
            nes.wram()[base..base + records].to_vec(),
            nes.wram()[usize::from(IRQ_COUNT)],
        )
    }
}

use Step::{Record, RecordPpu, Write, WritePpu};

/// Rendering on, sprites from `$1000`: PPU A12 rises once per scanline, which
/// is what an MMC3's scanline counter needs.
const RENDER: [(u16, u8); 2] = [(0x2000, 0x08), (0x2001, 0x18)];

/// MMC3 IRQ at scanline 10, plus rendering.
const MMC3_IRQ: [(u16, u8); 5] = [
    (0xC000, 10),
    (0xC001, 0),
    (0xE001, 0),
    (0x2000, 0x08),
    (0x2001, 0x18),
];
/// The MMC3 acknowledge: disable, then re-enable.
const MMC3_ACK: [(u16, u8); 2] = [(0xE000, 0), (0xE001, 0)];

const fn mmc3_board(mapper: u16, sub: u8, prg: usize, chr: usize, steps: &[Step]) -> Board<'_> {
    Board {
        mapper,
        submapper: sub,
        prg_8k: prg,
        chr_1k: chr,
        steps,
        arm_irq: &MMC3_IRQ,
        ack_irq: &MMC3_ACK,
    }
}

/// The control for every IRQ check below: a board that arms nothing takes no
/// IRQ at all, so a non-zero count can only come from the board.
#[test]
fn no_irq_without_arming() {
    let board = Board {
        mapper: 37,
        submapper: 0,
        prg_8k: 32,
        chr_1k: 256,
        steps: &[],
        arm_irq: &[(0x2000, 0x08), (0x2001, 0x18)],
        ack_irq: &MMC3_ACK,
    };
    let (_, irqs) = board.run(0);
    assert_eq!(irqs, 0);
}

#[test]
fn m12_chr_a18_per_pattern_table() {
    let steps = [
        Write(0x4100, 0x10),
        Write(0x8000, 0),
        Write(0x8001, 4),
        Write(0x8000, 2),
        Write(0x8001, 9),
        RecordPpu(0x0000),
        RecordPpu(0x1000),
        RecordPpu(0x1001),
    ];
    let (r, irqs) = mmc3_board(12, 0, 16, 512, &steps).run(3);
    assert_eq!(r, [0x04, 0x09, 0x01], "$0000 bank 4; $1000 bank $109");
    assert!(
        irqs > 0,
        "the MMC3A counter still fires at a non-zero latch"
    );
}

#[test]
fn m37_outer_latch_through_the_prg_ram_window() {
    let steps = [
        Write(0xA001, 0x80),
        Write(0x6000, 0x03),
        Write(0x8000, 6),
        Write(0x8001, 0x0F),
        Record(0x8000),
        Write(0x6000, 0x07),
        Record(0x8000),
    ];
    let (r, irqs) = mmc3_board(37, 0, 32, 256, &steps).run(2);
    assert_eq!(r, [15, 31]);
    assert!(irqs > 0);
}

#[test]
fn m45_outer_registers_in_turn() {
    let steps = [
        Write(0x6000, 0x00),
        Write(0x6000, 0x10),
        Write(0x6000, 0x0F),
        Write(0x6000, 0x30),
        Write(0x8000, 6),
        Write(0x8001, 0x23),
        Record(0x8000),
        Record(0x5010),
    ];
    let (r, irqs) = mmc3_board(45, 0, 64, 256, &steps).run(2);
    assert_eq!(r[0], 0x13);
    assert_eq!(r[1] & 1, 1, "DIP switch 0 reads back at $5010");
    assert!(irqs > 0);
}

#[test]
fn m47_block_bit() {
    let steps = [
        Write(0xA001, 0x80),
        Write(0x6000, 1),
        Write(0x8000, 6),
        Write(0x8001, 3),
        Record(0x8000),
    ];
    let (r, _) = mmc3_board(47, 0, 32, 256, &steps).run(1);
    assert_eq!(r, [0x13]);
}

#[test]
fn waixing_chr_ram_overlays() {
    // (mapper, the R2 bank that is RAM, a bank that is ROM)
    for (mapper, ram_bank, rom_bank) in [
        (74u16, 8u8, 7u8),
        (191, 0x80, 0x01),
        (192, 11, 12),
        (194, 1, 2),
    ] {
        let steps = [
            Write(0x8000, 2),
            Write(0x8001, ram_bank),
            WritePpu(0x1002, 0xA5),
            RecordPpu(0x1002),
            Write(0x8001, rom_bank),
            WritePpu(0x1000, 0x5A),
            RecordPpu(0x1000),
            Write(0xA001, 0x80),
            Write(0x6000, 0x3C),
            Record(0x6000),
        ];
        let (r, irqs) = mmc3_board(mapper, 0, 16, 256, &steps).run(3);
        assert_eq!(r, [0xA5, rom_bank, 0x3C], "mapper {mapper}");
        assert!(irqs > 0, "mapper {mapper}");
    }
}

#[test]
fn m195_ppu_write_selects_the_ram_window() {
    let steps = [
        // Power-on: banks $28-$2B are RAM.
        Write(0x8000, 2),
        Write(0x8001, 0x28),
        WritePpu(0x1000, 0x11),
        RecordPpu(0x1000),
        // A write to ROM bank $82 selects $00-$03.
        Write(0x8000, 3),
        Write(0x8001, 0x82),
        WritePpu(0x1400, 0x00),
        Write(0x8000, 2),
        Write(0x8001, 0x00),
        RecordPpu(0x1000),
    ];
    let (r, _) = mmc3_board(195, 0, 16, 256, &steps).run(2);
    assert_eq!(r, [0x11, 0x11], "$80 and $82 share the same 4 KiB of RAM");
}

#[test]
fn m121_protection_and_override() {
    let steps = [
        Write(0x5000, 2),
        Record(0x5000),
        Write(0x8001, 0x02),
        Write(0x8003, 0x28),
        Record(0xC000),
    ];
    let (r, _) = mmc3_board(121, 0, 32, 256, &steps).run(2);
    assert_eq!(r, [0x42, 0x10]);
}

#[test]
fn t9552_mapper_249_and_submapper_5() {
    let steps = [
        Write(0x5000, 2),
        Write(0x8000, 6),
        Write(0x8001, 2),
        Record(0x8000),
    ];
    let (r, _) = mmc3_board(249, 0, 32, 256, &steps).run(1);
    assert_eq!(r, [0x08], "the page's worked example, on the bus");
    let (r, irqs) = mmc3_board(4, 5, 32, 256, &steps).run(1);
    assert_eq!(r, [0x02], "submapper 5 files are stored in the $02 order");
    assert!(irqs > 0);
}

#[test]
fn mmc6_ram_on_submapper_1() {
    let steps = [
        Write(0x8000, 0x20),
        Write(0xA001, 0x30),
        Write(0x7001, 0x5A),
        Record(0x7001),
        Record(0x7201),
    ];
    let (r, irqs) = mmc3_board(4, 1, 16, 128, &steps).run(2);
    assert_eq!(r, [0x5A, 0x00], "the unreadable half reads zero");
    assert!(irqs > 0);
}

#[test]
fn m83_cony_prg_mode_2_and_m2_irq() {
    let board = Board {
        mapper: 83,
        submapper: 0,
        prg_8k: 32,
        chr_1k: 256,
        steps: &[Write(0x8100, 0x10), Write(0x8300, 3), Record(0x8000)],
        // Count down from 1000; enable latch copied at $8201.
        arm_irq: &[(0x8100, 0xD0), (0x8200, 0xE8), (0x8201, 0x03)],
        ack_irq: &[(0x8200, 0x00)],
    };
    let (r, irqs) = board.run(1);
    assert_eq!(r, [3]);
    assert_eq!(irqs, 1, "fires once, then disables itself");
}

#[test]
fn m91_jy_banks_outer_and_a12_irq() {
    let board = Board {
        mapper: 91,
        submapper: 0,
        prg_8k: 64,
        chr_1k: 512,
        steps: &[
            Write(0x7000, 3),
            Record(0x8000),
            Write(0x8006, 0),
            Record(0x8000),
            Write(0x6002, 0x07),
            RecordPpu(0x1000),
        ],
        arm_irq: &[(0x7007, 0), (0x2000, 0x08), (0x2001, 0x18)],
        ack_irq: &[(0x7006, 0), (0x7007, 0)],
    };
    let (r, irqs) = board.run(3);
    assert_eq!(r, [3, 0x33, 14], "2 KiB CHR bank 7 = 1 KiB bank 14");
    assert!(irqs > 0);
}

#[test]
fn m105_nes_event_unlock() {
    // Five serial writes per register, LSB first.
    let serial =
        |addr: u16, v: u8| -> [Step; 5] { [0, 1, 2, 3, 4].map(|i| Write(addr, (v >> i) & 1)) };
    let mut steps = Vec::new();
    steps.push(Record(0x8000)); // locked: the first 32 KiB
    steps.extend(serial(0xA000, 0x00)); // I=0
    steps.extend(serial(0xA000, 0x16)); // I=1: unlocked, O=0, AA=3
    steps.push(Record(0x8000));
    let board = Board {
        mapper: 105,
        submapper: 0,
        prg_8k: 32,
        chr_1k: 0,
        steps: &steps,
        arm_irq: &[],
        ack_irq: &[],
    };
    let (r, _) = board.run(2);
    assert_eq!(r, [0, 12], "32 KiB bank 3 of chip 0 = 8 KiB bank 12");
}

#[test]
fn m153_outer_bank_wram_and_irq() {
    let board = Board {
        mapper: 153,
        submapper: 0,
        prg_8k: 64,
        chr_1k: 0,
        steps: &[
            Write(0x8000, 1),
            Write(0x8001, 1),
            Write(0x8002, 1),
            Write(0x8003, 1),
            Write(0x8008, 3),
            Record(0x8000),
            Write(0x800D, 0x20),
            Write(0x6000, 0x5A),
            Record(0x6000),
        ],
        arm_irq: &[(0x800B, 0x00), (0x800C, 0x10), (0x800A, 0x01)],
        ack_irq: &[(0x800A, 0x00)],
    };
    let (r, irqs) = board.run(2);
    assert_eq!(r, [38, 0x5A], "16 KiB bank 16+3 = 8 KiB bank 38");
    assert_eq!(irqs, 1);
}

#[test]
fn m163_nanjing_prg() {
    let board = Board {
        mapper: 163,
        submapper: 0,
        prg_8k: 64,
        chr_1k: 0,
        steps: &[
            Record(0x8000),
            Write(0x5300, 4),
            Write(0x5000, 2),
            Record(0x8000),
        ],
        arm_irq: &RENDER,
        ack_irq: &[],
    };
    let (r, _) = board.run(2);
    assert_eq!(r, [12, 8], "boots in 32 KiB bank 3; then bank 2");
}

#[test]
fn m228_action52_address_latch() {
    let board = Board {
        mapper: 228,
        submapper: 0,
        prg_8k: 64,
        chr_1k: 512,
        steps: &[Write(0x8160, 0x02), Record(0x8000), RecordPpu(0x0000)],
        arm_irq: &[],
        ack_irq: &[],
    };
    let (r, _) = board.run(2);
    assert_eq!(r, [10, 16], "16 KiB bank 5; 8 KiB CHR bank 2");
}

#[test]
fn m111_gtrom_window_bonus_ram_and_flash() {
    let board = Board {
        mapper: 111,
        submapper: 0,
        prg_8k: 64,
        chr_1k: 0,
        steps: &[
            Write(0x6000, 0x01), // not decoded
            Record(0x8000),
            Write(0x7000, 0x03),
            Record(0x8000),
            WritePpu(0x2123, 0x11),
            WritePpu(0x3123, 0x22),
            RecordPpu(0x2123),
            RecordPpu(0x3123),
            Write(0xD555, 0xAA),
            Write(0xAAAA, 0x55),
            Write(0xD555, 0xA0),
            Write(0x8100, 0x42),
            Record(0x8100),
        ],
        arm_irq: &[],
        ack_irq: &[],
    };
    let (r, _) = board.run(5);
    assert_eq!(r, [0, 12, 0x11, 0x22, 0x42]);
}

#[test]
fn gtrom_flash_is_a_battery_save() {
    let board = Board {
        mapper: 111,
        submapper: 0,
        prg_8k: 64,
        chr_1k: 0,
        steps: &[],
        arm_irq: &[],
        ack_irq: &[],
    };
    let nes = Nes::from_rom(&board.rom()).unwrap();
    assert!(nes.has_battery(), "the flash is non-volatile storage");
    assert_eq!(
        nes.save_data().len(),
        64 * PRG_8K,
        "the save is the flash image"
    );
    assert!(nes.sram().is_empty(), "and there is no RAM at $6000");
}

/// NES 2.0 mapper 4 submapper 4 is the NEC MMC3 ("Loading the latch with 0
/// disables IRQ", `NES_2_0_submappers.md`); submapper 0 is the Sharp one, which
/// fires every scanline at a latch of 0. Until v2.9.6 the two were swapped with
/// submapper 1.
///
/// v3.1.0: "disables" is the submapper table's shorthand. `MMC3.md` is exact:
/// the NEC part "generates only a single IRQ when `$C000` is `$00`", and
/// "writing to `$C001` with `$C000` still at `$00` will result in another
/// single IRQ" (blargg's `6-MMC3_alt`: "IRQ should be set when reloading due
/// to clear"). The arm sequence writes `$C001` once, so NEC gives exactly one
/// IRQ and then none; before v3.1.0 this test asserted zero.
#[test]
fn mmc3_submapper_4_is_nec_and_0_is_sharp() {
    let arm = [
        (0xC000, 0),
        (0xC001, 0),
        (0xE001, 0),
        (0x2000, 0x08),
        (0x2001, 0x18),
    ];
    let run = |sub: u8| {
        Board {
            mapper: 4,
            submapper: sub,
            prg_8k: 16,
            chr_1k: 128,
            steps: &[],
            arm_irq: &arm,
            ack_irq: &MMC3_ACK,
        }
        .run(0)
        .1
    };
    assert_eq!(
        run(4),
        1,
        "NEC: a latch of 0 gives the one IRQ the $C001 write produces, then stops"
    );
    assert!(run(0) > 0, "Sharp: a latch of 0 fires every clock");
}

/// GTROM's latch clocks on a READ of its window too, taking whatever floats
/// on the bus (`GTROM.md`). `LDA $5F00` floats `$5F`, the operand's high byte,
/// the last value the CPU fetched, so it selects 32 KiB bank 15, which is bank
/// 7 on a 256 KiB board. This is the only test that reaches the bus's
/// `notify_floating_read` call: a mapper-level test cannot.
#[test]
fn m111_gtrom_a_read_of_the_window_latches_open_bus() {
    let board = Board {
        mapper: 111,
        submapper: 0,
        prg_8k: 32,
        chr_1k: 0,
        steps: &[Record(0x5F00), Record(0x8000)],
        arm_irq: &[],
        ack_irq: &[],
    };
    let (r, _) = board.run(2);
    assert_eq!(r, [0x5F, 28], "open bus $5F; 32 KiB bank 7 = 8 KiB bank 28");
}

/// A mapper-111 image with CHR-ROM is the Ninja Ryukenden MMC1 variant, not
/// GTROM; it is refused with a message instead of running as a broken GTROM.
#[test]
fn m111_with_chr_rom_is_refused_not_misrun() {
    let board = Board {
        mapper: 111,
        submapper: 0,
        prg_8k: 32,
        chr_1k: 256,
        steps: &[],
        arm_irq: &[],
        ack_irq: &[],
    };
    let err = Nes::from_rom(&board.rom()).err().expect("must be refused");
    assert!(format!("{err:?}").contains("Ninja Ryukenden"), "{err:?}");
}

/// The save seam (v2.9.6): a power cycle keeps what a game flashed, as a
/// console keeps its flash; a power-on movie starts from the ROM as loaded,
/// never from a zeroed flash (which would be a ROM with no program in it).
#[test]
fn gtrom_flash_survives_power_cycle_and_movie_start_restores_the_rom() {
    let board = Board {
        mapper: 111,
        submapper: 0,
        prg_8k: 64,
        chr_1k: 0,
        steps: &[
            Write(0xD555, 0xAA),
            Write(0xAAAA, 0x55),
            Write(0xD555, 0xA0),
            Write(0x8100, 0x42),
        ],
        arm_irq: &[],
        ack_irq: &[],
    };
    let mut nes = Nes::from_rom(&board.rom()).unwrap();
    for _ in 0..4 {
        nes.run_frame();
    }
    // 8 KiB bank 0 starts with its bank number (0, 0); byte $100 was $FF.
    assert_eq!(nes.save_data()[0x100], 0x42, "the program ran");
    nes.power_cycle();
    assert_eq!(
        nes.save_data()[0x100],
        0x42,
        "a power cycle keeps the flash"
    );
    rustynes_core::power_on_for_movie(&mut nes);
    assert_eq!(
        nes.save_data()[0x100],
        0xFF,
        "a movie starts from the ROM as loaded"
    );
    assert_eq!(
        nes.save_data()[0x1000],
        0x78,
        "and the program is still there (SEI)"
    );
}
