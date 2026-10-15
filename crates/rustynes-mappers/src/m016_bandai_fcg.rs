// SPDX-License-Identifier: GPL-3.0-or-later
//
// Provenance: the Bandai FCG serial-EEPROM handling (`Eeprom24C01` / `Eeprom24C02`) is derived from Mesen2 (GPL-3.0-or-later), `Core/NES/Mappers/Bandai/`. See docs/originality-and-provenance.md (Section 1)
// and NOTICE for the complete, audited derivation record.
//! Bandai FCG (iNES mappers 16, 153, 157 and 159) implementation.
//!
//! Covers the Bandai FCG-1/FCG-2 and LZ93D50 ASICs. Banking: a 16 KiB
//! switchable PRG bank at `$8000-$BFFF` (last bank fixed at `$C000`), eight
//! independent 1 KiB CHR banks, software mirroring control, and a 16-bit
//! down-counting CPU-cycle (M2) IRQ. Some boards add a serial I²C EEPROM
//! (24C02 on mapper 16 submapper 5, X24C01 on mapper 159).
//!
//! # Register window (`nesdev_wiki/INES_Mapper_016.xhtml`)
//!
//! The register block is the same set of offsets `$0-$D`, but the decode
//! window differs by submapper:
//!
//! - Submapper 4 (FCG-1/2): registers respond at `$6000-$7FFF` (mask
//!   `$E00F`); writes to `$600B/$600C` modify the counter directly.
//! - Submapper 5 (LZ93D50): registers respond at `$8000-$FFFF` (mask
//!   `$800F`); `$800B/$800C` modify a *latch* copied to the counter on a
//!   `$800A` write; an EEPROM read appears in bit 4 of `$6000-$7FFF`.
//! - Submapper 0 (unspecified): respond in both ranges (the union).
//!
//! Mapper 159 is mapper 16 submapper 5 with a 128-byte X24C01 EEPROM
//! (instead of the 256-byte 24C02 on mapper 16).
//!
//! Mapper 153 (v2.9.6, written from `nesdev_wiki/output/INES_Mapper_153.md`)
//! is an LZ93D50 with 8 KiB of battery-backed WRAM and no EEPROM. Its CHR
//! registers `$8000-$8003` become the outer 256 KiB PRG bank (bit 0, "the same
//! value must be written to all four"); CHR is 8 KiB of unbanked CHR-RAM; and
//! `$800D` bit 5 enables the WRAM chip. The page notes that *Famicom Jump II*
//! freezes when it boots with zero-filled WRAM and runs after a soft reset;
//! that is the game, not the board.
//!
//! Mapper 157 (v3.2.0, written from `nesdev_wiki/INES_Mapper_157.xhtml`) is
//! the Datach Joint ROM System: an LZ93D50 game cartridge plugged into the
//! Datach main unit. Banking, mirroring and the IRQ are mapper 16 submapper
//! 5's; CHR is 8 KiB of unbanked CHR-RAM. The main unit holds a 256-byte
//! 24C02 (clocked by `$800D` bit 5) and the barcode reader; *Battle Rush*'s
//! cartridge adds a 128-byte X24C01 clocked by `$8000-$8003` bit 3 (CHR A13
//! OUT; `$8004-$8007` do nothing). Both chips share SDA (`$800D` bit 6) and
//! their outputs are ANDed onto bit 4 of a `$6000-$7FFF` read; bit 3 of the
//! same read is the scanner ([`BarcodeReader`], `datach_barcode.rs`). The variant's code
//! lives outside the derived EEPROM region below, except for the
//! `keeps_word_address` flag on [`Eeprom`], written from the 24C02's
//! random-read protocol (see the field) and on for every 24C02.
//!
//! ## Offsets (relative to the window base, masked to `$x..F`)
//!
//! | Offset | Function                                              |
//! |--------|-------------------------------------------------------|
//! | `$0-7` | 1 KiB CHR bank N at PPU `$N*0x400`                     |
//! | `$8`   | 16 KiB PRG bank at `$8000` (low 4 bits)               |
//! | `$9`   | Mirroring (0 V, 1 H, 2 1scA, 3 1scB)                  |
//! | `$A`   | IRQ control: bit 0 enable; LZ93D50 also latch->counter |
//! | `$B`   | IRQ counter/latch low byte                            |
//! | `$C`   | IRQ counter/latch high byte                           |
//! | `$D`   | EEPROM control (LZ93D50): bit 5 SCL, bit 6 SDA, bit 7 dir |
//!
//! # EEPROM
//!
//! An I²C state machine ([`Eeprom`]) for the X24C01 (159) / 24C02 (16) is
//! implemented below. Provenance: it is **derived from Mesen2's `Eeprom24C01` /
//! `Eeprom24C02`** (`Core/NES/Mappers/Bandai/`, GPL-3.0-or-later); the I2C
//! protocol it models is the published Xicor/Intersil X24C01 / 24C02 datasheet.
//! See NOTICE and docs/originality-and-provenance.md (Section 1). It clocks bits on the SCL
//! **rising** edge and
//! advances the mode/ACK handshake on the **falling** edge, detects
//! START/STOP as SDA transitions while SCL is held high, and honors the two
//! chips' differing bit order (X24C01 LSB-first, 24C02 MSB-first) and
//! addressing (X24C01 combined word-address+R/W byte vs. 24C02
//! device-select + word-address). The X24C01 bit order and rise/fall
//! handshake were the boot-blocker for the mapper-159 games (a blank screen
//! while the game busy-waited on the EEPROM probe). It is **not**
//! datasheet-timing-verified — there are no redistributable behavioral test
//! fixtures for these boards — so it is verified at the unit-test +
//! boot-smoke level against the reference emulators.

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_lossless,
    clippy::struct_excessive_bools,
    clippy::missing_const_for_fn,
    clippy::doc_markdown,
    clippy::option_if_let_else
)]

use crate::cartridge::Mirroring;
use crate::datach_barcode::{Barcode, BarcodeReader};
use crate::mapper::{Mapper, MapperCaps, MapperError};
use alloc::{boxed::Box, vec::Vec};
use alloc::{format, vec};

const PRG_BANK_16K: usize = 0x4000;
const CHR_BANK_1K: usize = 0x0400;
const NAMETABLE_SIZE: usize = 0x0400;
const NAMETABLE_SIZE_U16: u16 = 0x0400;

/// v2 (v2.9.6) appends mapper 153's outer bank, WRAM enable and WRAM. Since
/// v2.9.8 (ADR 0042) a v1 blob is refused; it used to load on the variants
/// that have neither.
///
/// v3 (v3.2.0) adds each EEPROM's protocol state ([`Eeprom::save_lines`],
/// nine bytes after its contents), so a state taken mid-transaction resumes
/// it. v3.2.0 refused a v2 blob, so no v3.1.x save state of mappers 16, 153
/// or 159 loaded; since v3.3.0 one loads again ([`V2_STATE_VERSION`]).
const SAVE_STATE_VERSION: u8 = 3;

/// The v3.1.x layout, still read (`T-FCG-EEPROM-STATE`, v3.3.0): version 3
/// without the protocol record. It never stored one, so the chip is restored
/// idle ([`Eeprom::idle_after`]): its contents are exact, and a game that
/// was in the middle of talking to it when the state was taken sees the
/// transaction end, as it did when v3.1.x restored that state. Mapper 157
/// postdates it and refuses version 2.
const V2_STATE_VERSION: u8 = 2;

/// Mapper 153's WRAM.
const WRAM_153: usize = 0x2000;

/// FCG board / EEPROM variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FcgVariant {
    /// Mapper 16 submapper 0: respond in both `$6000-$7FFF` and
    /// `$8000-$FFFF`; behaves as LZ93D50 (latched counter) with a 24C02.
    Both,
    /// Mapper 16 submapper 4: FCG-1/2, register window `$6000-$7FFF`,
    /// counter written directly, no EEPROM.
    Fcg,
    /// Mapper 16 submapper 5: LZ93D50, register window `$8000-$FFFF`,
    /// latched counter, optional 256-byte 24C02 EEPROM.
    Lz93d50_24c02,
    /// Mapper 159: LZ93D50 with a 128-byte X24C01 EEPROM.
    Lz93d50_24c01,
    /// Mapper 153: LZ93D50 with 8 KiB of battery-backed WRAM and an outer
    /// 256 KiB PRG bank in place of the CHR registers.
    Lz93d50Wram,
    /// Mapper 157: the Datach Joint ROM System. LZ93D50, unbanked CHR-RAM,
    /// the main unit's 256-byte 24C02, an optional 128-byte X24C01 on the
    /// game cartridge, and the barcode reader. Built by
    /// [`BandaiFcg::new_datach`].
    Datach,
}

impl FcgVariant {
    const fn responds_low(self) -> bool {
        matches!(self, Self::Both | Self::Fcg)
    }
    const fn responds_high(self) -> bool {
        matches!(
            self,
            Self::Both
                | Self::Lz93d50_24c02
                | Self::Lz93d50_24c01
                | Self::Lz93d50Wram
                | Self::Datach
        )
    }
    /// LZ93D50 latches the IRQ counter (`$x0B/$x0C` write a latch); FCG-1/2
    /// writes the counter directly.
    const fn latched_counter(self) -> bool {
        !matches!(self, Self::Fcg)
    }
    const fn eeprom_bytes(self) -> usize {
        match self {
            Self::Lz93d50_24c01 => 128,
            Self::Both | Self::Lz93d50_24c02 | Self::Datach => 256,
            Self::Fcg | Self::Lz93d50Wram => 0,
        }
    }
    /// X24C01 uses a combined 7-bit word-address byte; the 24C02 uses a
    /// device-select byte followed by a separate word-address byte.
    const fn is_x24c01(self) -> bool {
        matches!(self, Self::Lz93d50_24c01)
    }
}

/// Serial I²C EEPROM (X24C01 / 24C02) state machine.
///
/// An independent I2C state machine for the X24C01 / 24C02 serial EEPROMs,
/// implemented from the published I2C serial-EEPROM datasheet protocol. The
/// protocol is driven on
/// **both** SCL edges: bits are clocked on the rising edge, and the
/// mode/ACK handshake advances on the falling edge — exactly how the boards
/// drive the line. START / STOP are detected as SDA transitions while SCL is
/// held high.
///
/// The two chips differ in two ways that matter for boot:
///
/// - **Bit order.** The X24C01 (mapper 159) shifts addr/data **LSB-first**
///   (`bit << counter`); the 24C02 (mapper 16) shifts **MSB-first**
///   (`bit << (7 - counter)`).
/// - **Addressing.** The X24C01 takes a single combined byte (7-bit word
///   address + R/W bit); the 24C02 takes a device-select byte (`0xA0 | …`)
///   followed by a separate word-address byte.
///
/// It is not a cycle/timing-accurate datasheet model, but it matches the
/// reference emulators' boot-relevant behavior.
#[derive(Debug, Clone)]
struct Eeprom {
    mem: Box<[u8]>,
    is_x24c01: bool,

    last_scl: u8,
    last_sda: u8,

    mode: I2cMode,
    next_mode: I2cMode,
    /// Device-select byte (24C02 only; includes the R/W bit).
    chip_addr: u8,
    /// Word address into `mem`.
    addr: u8,
    /// Byte being shifted in (write) or out (read).
    data: u8,
    /// Bits shifted so far in the current byte (0..=8).
    counter: u8,
    /// The bit currently driven back onto SDA toward the CPU (high = 1).
    output: u8,
    /// v3.2.0, every 24C02: a START keeps the word address instead of
    /// clearing it. A 24C02 "random read" is a dummy write of the word address
    /// followed by a repeated START and a read-mode device select, and the
    /// read then begins at that address: the chip's address counter is not
    /// reset by a START. Clearing it made every random read start at `$00`.
    /// Found on mapper 157: *Ultraman Club - Supokon Fight!* reads its file
    /// table that way, and with the address cleared it reformatted the main
    /// unit's EEPROM and stopped on "no free block" at every boot; keeping it,
    /// the game reaches its title (measured 2026-10-08). Then applied to
    /// mapper 16's 24C02 as well, since it is the chip's behaviour, not the
    /// board's. Written from that protocol description.
    /// Off for the X24C01, which takes its word address after every START, so
    /// the clearing is unobservable there.
    keeps_word_address: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum I2cMode {
    Idle,
    ChipAddress,
    Address,
    Read,
    Write,
    SendAck,
    WaitAck,
}

impl I2cMode {
    /// The save-state tag. Fixed numbers, not `as u8`, so reordering the enum
    /// cannot silently change the format.
    const fn to_byte(self) -> u8 {
        match self {
            Self::Idle => 0,
            Self::ChipAddress => 1,
            Self::Address => 2,
            Self::Read => 3,
            Self::Write => 4,
            Self::SendAck => 5,
            Self::WaitAck => 6,
        }
    }

    const fn from_byte(b: u8) -> Option<Self> {
        Some(match b {
            0 => Self::Idle,
            1 => Self::ChipAddress,
            2 => Self::Address,
            3 => Self::Read,
            4 => Self::Write,
            5 => Self::SendAck,
            6 => Self::WaitAck,
            _ => return None,
        })
    }
}

impl Eeprom {
    fn new(bytes: usize, is_x24c01: bool) -> Self {
        Self {
            mem: vec![0xFFu8; bytes.max(1)].into_boxed_slice(),
            is_x24c01,
            last_scl: 0,
            last_sda: 0,
            mode: I2cMode::Idle,
            next_mode: I2cMode::Idle,
            chip_addr: 0,
            addr: 0,
            data: 0,
            counter: 0,
            output: 1,
            keeps_word_address: !is_x24c01,
        }
    }

    /// Bytes [`Self::save_lines`] writes: the two sampled line levels, both
    /// protocol modes, and the five transaction registers.
    const LINES_LEN: usize = 9;

    /// The chip's PROTOCOL state, for a save state (v3.2.0, version 3). The
    /// contents alone are not enough: a state taken in the middle of a
    /// transaction must resume it, and since 24C02 random reads keep the word
    /// address across a START (`keeps_word_address`), `addr` is live between
    /// transactions too. Before version 3 a restore left all of this at its
    /// live values, so rollback and run-ahead diverged from a straight run
    /// (`CodeRabbit` on #604). This is RustyNES's own serialisation of the
    /// fields above; nothing in it comes from a reference emulator.
    fn save_lines(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&[
            self.last_scl,
            self.last_sda,
            self.mode.to_byte(),
            self.next_mode.to_byte(),
            self.chip_addr,
            self.addr,
            self.data,
            self.counter,
            self.output,
        ]);
    }

    /// Check a [`Self::save_lines`] record without changing anything, so a
    /// caller can validate every part of a state before restoring any of it.
    ///
    /// It refuses what THIS chip cannot reach as well as what no chip can: the
    /// read paths index `mem` by `addr` unmasked, so an X24C01 (128 bytes)
    /// restored with a word address of `$80` or more, or in the 24C02-only
    /// `ChipAddress` mode, would index past its memory and panic (the
    /// commit-time security review of 10d0b027).
    fn check_lines(&self, b: &[u8]) -> Result<(), MapperError> {
        let bad = |what: &str| Err(MapperError::Invalid(format!("EEPROM state: {what}")));
        if b.len() != Self::LINES_LEN {
            return bad("wrong length");
        }
        if b[0] > 1 || b[1] > 1 || b[8] > 1 {
            return bad("a line level above 1");
        }
        if I2cMode::from_byte(b[2]).is_none() || I2cMode::from_byte(b[3]).is_none() {
            return bad("unknown protocol mode");
        }
        if b[7] > 8 {
            return bad("bit counter above 8");
        }
        if usize::from(b[5]) >= self.mem.len() {
            return bad("word address past the chip");
        }
        let chip_address = I2cMode::ChipAddress.to_byte();
        if self.is_x24c01 && (b[2] == chip_address || b[3] == chip_address) {
            return bad("a device-select phase on an X24C01, which has none");
        }
        Ok(())
    }

    /// Restore a record [`Self::check_lines`] accepted.
    fn load_lines(&mut self, b: &[u8]) {
        self.last_scl = b[0];
        self.last_sda = b[1];
        self.mode = I2cMode::from_byte(b[2]).unwrap_or(I2cMode::Idle);
        self.next_mode = I2cMode::from_byte(b[3]).unwrap_or(I2cMode::Idle);
        self.chip_addr = b[4];
        self.addr = b[5];
        self.data = b[6];
        self.counter = b[7];
        self.output = b[8];
    }

    /// The protocol state a version 2 blob restores, which carries none: an
    /// idle chip whose last sampled lines are the ones `$x00D` (`ctrl`, bit 5
    /// SCL, bit 6 SDA) last drove, so the game's next write is judged against
    /// the levels the chip really saw and not against a spurious edge. Every
    /// field is set, so the result does not depend on what the board was
    /// doing before the load.
    fn idle_after(&mut self, ctrl: u8) {
        self.last_scl = u8::from(ctrl & 0x20 != 0);
        self.last_sda = u8::from(ctrl & 0x40 != 0);
        self.mode = I2cMode::Idle;
        self.next_mode = I2cMode::Idle;
        self.chip_addr = 0;
        self.addr = 0;
        self.data = 0;
        self.counter = 0;
        self.output = 1;
    }

    fn addr_mask(&self) -> u8 {
        if self.is_x24c01 { 0x7F } else { 0xFF }
    }

    /// Shift one bit into `dest` at the current `counter` position, honoring
    /// the chip's bit order, and advance the counter.
    fn write_bit(&mut self, dest: &mut u8, value: u8) {
        if self.counter < 8 {
            let shift = if self.is_x24c01 {
                self.counter
            } else {
                7 - self.counter
            };
            let mask = !(1u8 << shift);
            *dest = (*dest & mask) | (value << shift);
            self.counter += 1;
        }
    }

    /// Drive `output` from the current `data` bit at `counter`, honoring the
    /// chip's bit order, and advance the counter.
    fn read_bit(&mut self) {
        if self.counter < 8 {
            let shift = if self.is_x24c01 {
                self.counter
            } else {
                7 - self.counter
            };
            self.output = u8::from((self.data & (1u8 << shift)) != 0);
            self.counter += 1;
        }
    }

    /// Drive the EEPROM lines from the `$x00D` register. `scl`/`sda` are the
    /// host-driven clock/data levels; the device's response appears on
    /// [`Self::read_sda`].
    fn write_lines(&mut self, scl_b: bool, sda_b: bool) {
        let scl = u8::from(scl_b);
        let sda = u8::from(sda_b);

        if self.last_scl != 0 && scl != 0 && sda < self.last_sda {
            // START: SDA high->low while SCL stable high.
            self.mode = if self.is_x24c01 {
                I2cMode::Address
            } else {
                I2cMode::ChipAddress
            };
            if !self.keeps_word_address {
                self.addr = 0;
            }
            self.counter = 0;
            self.output = 1;
        } else if self.last_scl != 0 && scl != 0 && sda > self.last_sda {
            // STOP: SDA low->high while SCL stable high.
            self.mode = I2cMode::Idle;
            self.output = 1;
        } else if scl > self.last_scl {
            self.clock_rise(sda);
        } else if scl < self.last_scl {
            self.clock_fall(sda);
        }

        self.last_scl = scl;
        self.last_sda = sda;
    }

    fn read_sda(&self) -> bool {
        self.output != 0
    }

    fn clock_rise(&mut self, sda: u8) {
        match self.mode {
            I2cMode::ChipAddress => {
                let mut chip = self.chip_addr;
                self.write_bit(&mut chip, sda);
                self.chip_addr = chip;
            }
            I2cMode::Address => {
                if self.is_x24c01 {
                    // X24C01: 7 address bits, then the 8th bit selects R/W.
                    if self.counter < 7 {
                        let mut addr = self.addr;
                        self.write_bit(&mut addr, sda);
                        self.addr = addr;
                    } else if self.counter == 7 {
                        self.counter = 8;
                        if sda != 0 {
                            self.next_mode = I2cMode::Read;
                            self.data = self.mem[(self.addr & 0x7F) as usize];
                        } else {
                            self.next_mode = I2cMode::Write;
                        }
                    }
                } else {
                    let mut addr = self.addr;
                    self.write_bit(&mut addr, sda);
                    self.addr = addr;
                }
            }
            I2cMode::Read => self.read_bit(),
            I2cMode::Write => {
                let mut data = self.data;
                self.write_bit(&mut data, sda);
                self.data = data;
            }
            I2cMode::SendAck => self.output = 0,
            I2cMode::WaitAck => {
                if sda == 0 {
                    if self.is_x24c01 {
                        // X24C01: the master ack ends the read; STOP follows.
                        self.next_mode = I2cMode::Idle;
                    } else {
                        // 24C02: sequential read continues to the next byte.
                        self.next_mode = I2cMode::Read;
                        self.data = self.mem[(self.addr & self.addr_mask()) as usize];
                    }
                }
            }
            I2cMode::Idle => {}
        }
    }

    fn clock_fall(&mut self, _sda: u8) {
        match self.mode {
            I2cMode::ChipAddress => {
                if self.counter == 8 {
                    if (self.chip_addr & 0xA0) == 0xA0 {
                        self.mode = I2cMode::SendAck;
                        self.counter = 0;
                        self.output = 1;
                        if self.chip_addr & 0x01 != 0 {
                            self.next_mode = I2cMode::Read;
                            self.data = self.mem[(self.addr & self.addr_mask()) as usize];
                        } else {
                            self.next_mode = I2cMode::Address;
                        }
                    } else {
                        self.mode = I2cMode::Idle;
                        self.counter = 0;
                        self.output = 1;
                    }
                }
            }
            I2cMode::Address => {
                if self.is_x24c01 {
                    if self.counter == 8 {
                        // Ack the address, then run the queued read/write.
                        self.mode = I2cMode::SendAck;
                        self.output = 1;
                    }
                } else if self.counter == 8 {
                    self.counter = 0;
                    self.mode = I2cMode::SendAck;
                    self.next_mode = I2cMode::Write;
                    self.output = 1;
                }
            }
            I2cMode::SendAck => {
                self.mode = self.next_mode;
                self.counter = 0;
                self.output = 1;
            }
            I2cMode::Read => {
                if self.counter == 8 {
                    self.mode = I2cMode::WaitAck;
                    // The EEPROM byte-address counter is a hardware ring: it advances
                    // within the device and rolls over at the top rather than
                    // faulting. `addr` is a `u8` and `addr_mask()` is 0xFF on every
                    // chip but the X24C01, so the mask cannot express that rollover
                    // on its own -- the add traps in a debug build first. Wrap
                    // explicitly; the mask still confines the X24C01 to 7 bits.
                    self.addr = self.addr.wrapping_add(1) & self.addr_mask();
                }
            }
            I2cMode::Write => {
                if self.counter == 8 {
                    self.mode = I2cMode::SendAck;
                    self.next_mode = if self.is_x24c01 {
                        I2cMode::Idle
                    } else {
                        I2cMode::Write
                    };
                    self.mem[(self.addr & self.addr_mask()) as usize] = self.data;
                    // The EEPROM byte-address counter is a hardware ring: it advances
                    // within the device and rolls over at the top rather than
                    // faulting. `addr` is a `u8` and `addr_mask()` is 0xFF on every
                    // chip but the X24C01, so the mask cannot express that rollover
                    // on its own -- the add traps in a debug build first. Wrap
                    // explicitly; the mask still confines the X24C01 to 7 bits.
                    self.addr = self.addr.wrapping_add(1) & self.addr_mask();
                    self.counter = 0;
                }
            }
            I2cMode::WaitAck => {
                if !self.is_x24c01 {
                    self.mode = self.next_mode;
                    self.counter = 0;
                    self.output = 1;
                }
            }
            I2cMode::Idle => {}
        }
    }
}

/// Bandai FCG mapper (iNES mappers 16, 153, 157 and 159).
pub struct BandaiFcg {
    prg_rom: Box<[u8]>,
    chr: Box<[u8]>,
    vram: Box<[u8]>,
    chr_is_ram: bool,

    variant: FcgVariant,

    prg_bank: u8,
    chr_banks: [u8; 8],
    mirroring: Mirroring,

    // 16-bit down-counting IRQ.
    irq_latch: u16,
    irq_counter: u16,
    irq_enabled: bool,
    irq_pending: bool,

    eeprom: Option<Eeprom>,
    // Last value written to the EEPROM control register (for save-state).
    eeprom_ctrl: u8,

    /// Mapper 153: the outer 256 KiB PRG bank (`$8000-$8003` bit 0).
    outer: u8,
    /// Mapper 153: `$800D` bit 5, the WRAM chip enable.
    wram_enabled: bool,
    /// Mapper 153: 8 KiB of battery-backed WRAM; empty on the other variants.
    wram: Box<[u8]>,

    /// Mapper 157: the game cartridge's own 128-byte X24C01, present when the
    /// NES 2.0 header declares PRG-NVRAM (*Battle Rush*). It shares SDA with
    /// the main unit's 24C02 but has its own clock, `$8000-$8003` bit 3.
    ext_eeprom: Option<Eeprom>,
    /// Mapper 157: the last level written to the external EEPROM's SCL
    /// (`$8000-$8003` bit 3).
    ext_scl: bool,
    /// Mapper 157: the barcode scanner's serial output (bit 3 of a
    /// `$6000-$7FFF` read). Idle on every other variant.
    barcode: BarcodeReader,
}

impl BandaiFcg {
    /// Construct a new Bandai FCG mapper.
    ///
    /// `prg_rom` must be a non-zero multiple of 16 KiB; CHR-ROM must be a
    /// multiple of 1 KiB (CHR-RAM allocated as 8 KiB when empty).
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] on size mismatch.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
        variant: FcgVariant,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_16K) {
            return Err(MapperError::Invalid(format!(
                "Bandai-FCG PRG-ROM size {} is not a non-zero multiple of 16 KiB",
                prg_rom.len()
            )));
        }
        let chr_is_ram = chr_rom.is_empty();
        let chr: Box<[u8]> = if chr_is_ram {
            vec![0u8; 8 * CHR_BANK_1K].into_boxed_slice()
        } else if chr_rom.len().is_multiple_of(CHR_BANK_1K) {
            chr_rom
        } else {
            return Err(MapperError::Invalid(format!(
                "Bandai-FCG CHR-ROM size {} is not a multiple of 1 KiB",
                chr_rom.len()
            )));
        };
        let eeprom = if variant.eeprom_bytes() > 0 {
            Some(Eeprom::new(variant.eeprom_bytes(), variant.is_x24c01()))
        } else {
            None
        };
        Ok(Self {
            prg_rom,
            chr,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            chr_is_ram,
            variant,
            prg_bank: 0,
            chr_banks: [0; 8],
            mirroring,
            irq_latch: 0,
            irq_counter: 0,
            irq_enabled: false,
            irq_pending: false,
            eeprom,
            eeprom_ctrl: 0,
            outer: 0,
            wram_enabled: false,
            wram: if variant == FcgVariant::Lz93d50Wram {
                vec![0u8; WRAM_153].into_boxed_slice()
            } else {
                Box::new([])
            },
            ext_eeprom: None,
            ext_scl: false,
            barcode: BarcodeReader::default(),
        })
    }

    /// Construct mapper 157, the Datach Joint ROM System
    /// (`nesdev_wiki/INES_Mapper_157.xhtml`).
    ///
    /// `external_x24c01` adds the game cartridge's 128-byte X24C01, which the
    /// page says the NES 2.0 header signals with PRG-NVRAM (byte 10 = `$10`)
    /// and which only *Battle Rush: Build up Robot Tournament* carries.
    ///
    /// # Errors
    ///
    /// As [`Self::new`].
    pub fn new_datach(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
        external_x24c01: bool,
    ) -> Result<Self, MapperError> {
        let mut m = Self::new(prg_rom, chr_rom, mirroring, FcgVariant::Datach)?;
        if external_x24c01 {
            m.ext_eeprom = Some(Eeprom::new(128, true));
        }
        Ok(m)
    }

    /// Mapper 157's save-state tail, after the fields every variant writes:
    /// the external SCL level, the external X24C01's 128 bytes when the
    /// cartridge has one (fixed by the header, so not flagged), then the
    /// barcode reader ([`BarcodeReader::save`]). Added with the variant in
    /// v3.2.0, so no earlier blob of this variant exists. The tail itself
    /// needed no version bump; [`SAVE_STATE_VERSION`] is 3 for a different
    /// reason, the EEPROM protocol state every variant with an EEPROM now
    /// saves (an earlier draft of this comment said it "stays 2").
    fn save_datach_tail(&self, out: &mut Vec<u8>) {
        out.push(u8::from(self.ext_scl));
        if let Some(ext) = self.ext_eeprom.as_ref() {
            out.extend_from_slice(&ext.mem);
            ext.save_lines(out);
        }
        self.barcode.save(out);
    }

    /// Restore [`Self::save_datach_tail`]. Validates the whole tail before
    /// changing anything.
    fn load_datach_tail(&mut self, tail: &[u8]) -> Result<(), MapperError> {
        let ext_len = self
            .ext_eeprom
            .as_ref()
            .map_or(0, |e| e.mem.len() + Eeprom::LINES_LEN);
        let fixed = 1 + ext_len;
        let short = || MapperError::Invalid(format!("Datach state tail of {} bytes", tail.len()));
        if tail.len() < fixed {
            return Err(short());
        }
        let mut reader = BarcodeReader::default();
        let used = reader.load(&tail[fixed..]).ok_or_else(short)?;
        if fixed + used != tail.len() {
            return Err(short());
        }
        if let Some(ext) = self.ext_eeprom.as_ref() {
            ext.check_lines(&tail[1 + ext.mem.len()..fixed])?;
        }
        self.ext_scl = tail[0] != 0;
        if let Some(ext) = self.ext_eeprom.as_mut() {
            let n = ext.mem.len();
            ext.mem.copy_from_slice(&tail[1..=n]);
            ext.load_lines(&tail[1 + n..fixed]);
        }
        self.barcode = reader;
        Ok(())
    }

    /// The data line both EEPROMs drive back toward the CPU. "If there is
    /// both an internal and an external EEPROM, the Data out from both
    /// EEPROMs is ANDed (open drain circuit)" (`INES_Mapper_157`).
    fn eeprom_data_out(&self) -> bool {
        self.eeprom.as_ref().is_none_or(Eeprom::read_sda)
            && self.ext_eeprom.as_ref().is_none_or(Eeprom::read_sda)
    }

    /// Mapper 153's outer bank as a 16 KiB bank offset (256 KiB = 16 banks).
    const fn outer_base(&self) -> usize {
        (self.outer as usize & 0x01) << 4
    }

    const fn nametable_offset(&self, addr: u16) -> usize {
        let table = (((addr - 0x2000) / NAMETABLE_SIZE_U16) & 0x03) as u8;
        let local = (addr as usize) & (NAMETABLE_SIZE - 1);
        let physical = self.mirroring.physical_bank(table);
        physical * NAMETABLE_SIZE + local
    }

    fn chr_offset(&self, addr: u16) -> usize {
        // Mapper 153: "8 KiB unbanked CHR-RAM" and "No CHR banking is
        // available" (`INES_Mapper_153`). Its `$8000-$8003` registers drive
        // the outer PRG bank instead, so `chr_banks` is never written there
        // and must not be consulted: routing through it mapped all eight
        // 1 KiB windows onto the first 1 KiB.
        // Mapper 157 likewise: "8 KiB of unbanked CHR-RAM instead of
        // bankswitched CHR-ROM", "No CHR banking is available".
        if matches!(self.variant, FcgVariant::Lz93d50Wram | FcgVariant::Datach) {
            return addr as usize & 0x1FFF;
        }
        let slot = (addr as usize / CHR_BANK_1K) & 0x07;
        let total = (self.chr.len() / CHR_BANK_1K).max(1);
        let bank = (self.chr_banks[slot] as usize) % total;
        bank * CHR_BANK_1K + (addr as usize & (CHR_BANK_1K - 1))
    }

    /// Apply a register write decoded to offset `$0-$F`.
    fn write_reg(&mut self, off: u8, value: u8) {
        match off & 0x0F {
            // Mapper 153: PA12/PA13 are grounded, so `$0-$3` are the outer
            // PRG bank and `$4-$7` do nothing.
            0x0..=0x3 if self.variant == FcgVariant::Lz93d50Wram => self.outer = value & 0x01,
            0x4..=0x7 if self.variant == FcgVariant::Lz93d50Wram => {}
            // Mapper 157: bit 3 of `$8000-$8003` is CHR A13 OUT, wired to
            // the external X24C01's SCL ("External EEPROM Clock"). The ASIC's
            // PA12/PA13 inputs are grounded, so only `$0-$3` respond and
            // `$4-$7` do nothing; no CHR bank is selected by any of them. The
            // SDA the external chip sees is the shared line, `$800D` bit 6.
            0x0..=0x3 if self.variant == FcgVariant::Datach => {
                self.ext_scl = value & 0x08 != 0;
                let sda = self.eeprom_ctrl & 0x40 != 0;
                if let Some(ext) = self.ext_eeprom.as_mut() {
                    ext.write_lines(self.ext_scl, sda);
                }
            }
            0x4..=0x7 if self.variant == FcgVariant::Datach => {}
            0x0..=0x7 => self.chr_banks[(off & 0x07) as usize] = value,
            0x8 => self.prg_bank = value & 0x0F,
            0x9 => {
                self.mirroring = match value & 0x03 {
                    0 => Mirroring::Vertical,
                    1 => Mirroring::Horizontal,
                    2 => Mirroring::SingleScreenA,
                    _ => Mirroring::SingleScreenB,
                };
            }
            0xA => {
                // IRQ control. Bit 0 = enable. Writing acknowledges.
                self.irq_pending = false;
                self.irq_enabled = (value & 0x01) != 0;
                if self.variant.latched_counter() {
                    // LZ93D50: copy latch to counter.
                    self.irq_counter = self.irq_latch;
                }
            }
            0xB => {
                if self.variant.latched_counter() {
                    self.irq_latch = (self.irq_latch & 0xFF00) | value as u16;
                } else {
                    self.irq_counter = (self.irq_counter & 0xFF00) | value as u16;
                }
            }
            0xC => {
                if self.variant.latched_counter() {
                    self.irq_latch = (self.irq_latch & 0x00FF) | ((value as u16) << 8);
                } else {
                    self.irq_counter = (self.irq_counter & 0x00FF) | ((value as u16) << 8);
                }
            }
            0xD if self.variant == FcgVariant::Lz93d50Wram => {
                self.eeprom_ctrl = value;
                self.wram_enabled = value & 0x20 != 0;
            }
            0xD => {
                self.eeprom_ctrl = value;
                if let Some(ee) = self.eeprom.as_mut() {
                    // Bit 5 = SCL, bit 6 = SDA (host-driven), bit 7 = dir.
                    let scl = (value & 0x20) != 0;
                    let sda = (value & 0x40) != 0;
                    ee.write_lines(scl, sda);
                }
                // Mapper 157: bit 6 is "Both EEPROM's I2C SDA"; the external
                // chip keeps the clock last written to `$8000-$8003`.
                if let Some(ext) = self.ext_eeprom.as_mut() {
                    ext.write_lines(self.ext_scl, value & 0x40 != 0);
                }
            }
            _ => {}
        }
    }
}

impl Mapper for BandaiFcg {
    // Battery save: the serial EEPROM's contents -- 128 bytes (24C01) or 256
    // (24C02) -- when this variant carries one, and nothing otherwise (core
    // audit IMP-08). The EEPROM is reached through an I2C protocol, not a CPU
    // window, so this accessor is the ONLY way the frontend can persist it.
    fn sram(&self) -> &[u8] {
        if !self.wram.is_empty() {
            return &self.wram;
        }
        self.eeprom.as_ref().map_or(&[], |e| &e.mem)
    }
    fn sram_mut(&mut self) -> &mut [u8] {
        if !self.wram.is_empty() {
            return &mut self.wram;
        }
        match self.eeprom.as_mut() {
            Some(e) => &mut e.mem,
            None => &mut [],
        }
    }

    // v2.8.0 Phase 4 — CPU-cycle hook + IRQ source; no on-cart audio.
    fn caps(&self) -> MapperCaps {
        MapperCaps::CYCLE_IRQ
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x6000..=0x7FFF if !self.wram.is_empty() => {
                if self.wram_enabled {
                    self.wram[usize::from(addr - 0x6000)]
                } else {
                    0
                }
            }
            // Mapper 157 (mask `$E000`): bit 4 "Data out from I2C EEPROMs"
            // (the two chips ANDed), bit 3 "Serial data out from Barcode
            // scanner". The other bits are open bus on the board; like the
            // mapper-16 read below they read 0 here, because the bus consults
            // a partial driven-bit mask only for `$4020-$5FFF`.
            0x6000..=0x7FFF if self.variant == FcgVariant::Datach => {
                (u8::from(self.eeprom_data_out()) << 4) | (u8::from(self.barcode.level()) << 3)
            }
            0x6000..=0x7FFF => {
                // EEPROM read appears in bit 4 (LZ93D50). Otherwise open bus
                // (the bus's open-bus latch handles unmapped reads, but the
                // FCG drives bit 4 here).
                if let Some(ee) = self.eeprom.as_ref() {
                    let bit = u8::from(ee.read_sda());
                    return bit << 4;
                }
                0
            }
            0x8000..=0xBFFF => {
                let total = (self.prg_rom.len() / PRG_BANK_16K).max(1);
                let bank = (self.prg_bank as usize | self.outer_base()) % total;
                self.prg_rom[bank * PRG_BANK_16K + (addr - 0x8000) as usize]
            }
            0xC000..=0xFFFF => {
                let total = (self.prg_rom.len() / PRG_BANK_16K).max(1);
                // Mapper 153: the last bank of the selected 256 KiB.
                let last = if self.wram.is_empty() {
                    total - 1
                } else {
                    (0x0F | self.outer_base()) % total
                };
                self.prg_rom[last * PRG_BANK_16K + (addr - 0xC000) as usize]
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        if !self.wram.is_empty() && self.wram_enabled && (0x6000..=0x7FFF).contains(&addr) {
            self.wram[usize::from(addr - 0x6000)] = value;
        }
        if self.variant.responds_low() && (0x6000..=0x7FFF).contains(&addr) {
            self.write_reg((addr & 0x0F) as u8, value);
        }
        if self.variant.responds_high() && (0x8000..=0xFFFF).contains(&addr) {
            self.write_reg((addr & 0x0F) as u8, value);
        }
    }

    fn cpu_read_unmapped(&self, addr: u16) -> bool {
        // The FCG drives bit 4 of $6000-$7FFF (EEPROM) when an EEPROM is
        // present, so that window is mapped; otherwise default behavior.
        if self.eeprom.is_some() && (0x6000..=0x7FFF).contains(&addr) {
            return false;
        }
        if !self.wram.is_empty() && (0x6000..=0x7FFF).contains(&addr) {
            return !self.wram_enabled;
        }
        (0x4020..=0x5FFF).contains(&addr)
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                let off = self.chr_offset(addr);
                self.chr[off % self.chr.len()]
            }
            0x2000..=0x3EFF => self.vram[self.nametable_offset(addr)],
            _ => 0,
        }
    }

    fn ppu_peek(&self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                let off = self.chr_offset(addr);
                self.chr[off % self.chr.len()]
            }
            0x2000..=0x3EFF => self.vram[self.nametable_offset(addr)],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                if self.chr_is_ram {
                    let off = self.chr_offset(addr);
                    let len = self.chr.len();
                    self.chr[off % len] = value;
                }
            }
            0x2000..=0x3EFF => {
                let off = self.nametable_offset(addr);
                self.vram[off] = value;
            }
            _ => {}
        }
    }

    fn notify_cpu_cycle(&mut self) {
        // Mapper 157: the scanner's stream is timed in CPU cycles (no wall
        // clock), so it advances here, before the IRQ's early return.
        self.barcode.tick();
        if !self.irq_enabled {
            return;
        }
        // Down-counter: IRQ asserts when the counter holds zero, then it
        // wraps to $FFFF and keeps counting (per the wiki: "When it holds a
        // value of zero, an IRQ is generated").
        if self.irq_counter == 0 {
            self.irq_pending = true;
        }
        self.irq_counter = self.irq_counter.wrapping_sub(1);
    }

    fn irq_pending(&self) -> bool {
        self.irq_pending
    }

    fn has_barcode_reader(&self) -> bool {
        self.variant == FcgVariant::Datach
    }

    fn scan_barcode(&mut self, code: &Barcode) -> bool {
        if self.variant != FcgVariant::Datach {
            return false;
        }
        self.barcode.scan(code);
        true
    }

    fn barcode_scan_in_progress(&self) -> bool {
        self.barcode.is_scanning()
    }

    fn current_mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn debug_info(&self) -> crate::mapper::MapperDebugInfo {
        let id = match self.variant {
            FcgVariant::Datach => 157,
            FcgVariant::Lz93d50_24c01 => 159,
            FcgVariant::Lz93d50Wram => 153,
            _ => 16,
        };
        let mut info = crate::mapper::MapperDebugInfo {
            mapper_id: id,
            name: format!("Bandai FCG ({id})"),
            mirroring: crate::mapper::mirroring_name(self.mirroring),
            ..Default::default()
        };
        info.prg_banks
            .push(("PRG".into(), format!("{:#04x}", self.prg_bank)));
        for (i, b) in self.chr_banks.iter().enumerate() {
            info.chr_banks
                .push((format!("CHR{i}"), format!("{b:#04x}")));
        }
        info.irq_state
            .push(("latch".into(), format!("{:#06x}", self.irq_latch)));
        info.irq_state
            .push(("counter".into(), format!("{:#06x}", self.irq_counter)));
        info.irq_state
            .push(("enabled".into(), format!("{}", self.irq_enabled)));
        info.irq_state
            .push(("pending".into(), format!("{}", self.irq_pending)));
        info.extra.push((
            "eeprom".into(),
            match self.eeprom.as_ref() {
                Some(ee) => format!("{} bytes", ee.mem.len()),
                None => "none".into(),
            },
        ));
        info
    }

    fn save_state(&self) -> Vec<u8> {
        let ee_len = self
            .eeprom
            .as_ref()
            .map_or(0, |e| e.mem.len() + Eeprom::LINES_LEN);
        let mut out = Vec::with_capacity(
            18 + self.vram.len() + ee_len + if self.chr_is_ram { self.chr.len() } else { 0 },
        );
        out.push(SAVE_STATE_VERSION);
        out.push(self.prg_bank);
        out.extend_from_slice(&self.chr_banks);
        out.push(self.mirroring as u8);
        out.extend_from_slice(&self.irq_latch.to_le_bytes());
        out.extend_from_slice(&self.irq_counter.to_le_bytes());
        out.push(u8::from(self.irq_enabled));
        out.push(u8::from(self.irq_pending));
        out.push(self.eeprom_ctrl);
        // EEPROM contents and protocol state (if any).
        if let Some(ee) = self.eeprom.as_ref() {
            out.extend_from_slice(&ee.mem);
            ee.save_lines(&mut out);
        }
        out.extend_from_slice(&self.vram);
        if self.chr_is_ram {
            out.extend_from_slice(&self.chr);
        }
        out.push(self.outer);
        out.push(u8::from(self.wram_enabled));
        out.extend_from_slice(&self.wram);
        if self.variant == FcgVariant::Datach {
            self.save_datach_tail(&mut out);
        }
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let ee_len = self
            .eeprom
            .as_ref()
            .map_or(0, |e| e.mem.len() + Eeprom::LINES_LEN);
        let need_chr = if self.chr_is_ram { self.chr.len() } else { 0 };
        // Versions 3 (v3.2.0) and 2 (v3.1.x) are read. Version 2 is version 3
        // without the EEPROM protocol record (`T-FCG-EEPROM-STATE`); mapper
        // 157 first shipped at version 3, so it has no version 2 to read. A
        // v1 blob, which carried no mapper-153 tail, is refused (v2.9.8, ADR
        // 0042).
        let lines = match data.first() {
            // An empty blob fails the length check below.
            Some(&SAVE_STATE_VERSION) | None => true,
            Some(&V2_STATE_VERSION) if self.variant != FcgVariant::Datach => false,
            Some(&v) => return Err(MapperError::UnsupportedVersion(v)),
        };
        let ee_len = if lines {
            ee_len
        } else {
            ee_len.saturating_sub(Eeprom::LINES_LEN)
        };
        let core_len = 18 + self.vram.len() + ee_len + need_chr;
        let expected = if data.is_empty() {
            core_len
        } else {
            core_len + 2 + self.wram.len()
        };
        // Mapper 157 appends a tail whose length depends on the scan in
        // flight, so it is checked (and restored) separately, after the
        // fixed part.
        let (data, datach_tail) = if self.variant == FcgVariant::Datach {
            if data.len() < expected {
                return Err(MapperError::WrongLength {
                    expected,
                    got: data.len(),
                });
            }
            data.split_at(expected)
        } else {
            (data, &[][..])
        };
        if data.len() != expected {
            return Err(MapperError::WrongLength {
                expected,
                got: data.len(),
            });
        }
        // Validate everything before changing anything (CodeRabbit on #607:
        // an unknown mirroring byte used to return Err after the Datach tail,
        // the PRG bank and the CHR banks had already been restored).
        let mirroring = match data[10] {
            0 => Mirroring::Horizontal,
            1 => Mirroring::Vertical,
            2 => Mirroring::SingleScreenA,
            3 => Mirroring::SingleScreenB,
            4 => Mirroring::FourScreen,
            other => return Err(MapperError::Invalid(format!("mirroring {other}"))),
        };
        if let Some(ee) = self.eeprom.as_ref().filter(|_| lines) {
            let at = 18 + ee.mem.len();
            ee.check_lines(&data[at..at + Eeprom::LINES_LEN])?;
        }
        if self.variant == FcgVariant::Datach {
            self.load_datach_tail(datach_tail)?;
        }
        self.prg_bank = data[1];
        self.chr_banks.copy_from_slice(&data[2..10]);
        self.mirroring = mirroring;
        self.irq_latch = u16::from_le_bytes([data[11], data[12]]);
        self.irq_counter = u16::from_le_bytes([data[13], data[14]]);
        self.irq_enabled = data[15] != 0;
        // Bytes 16+ : pending, eeprom_ctrl, eeprom mem, vram, chr.
        // We packed pending + ctrl after the fixed block; recompute cursor.
        // To keep the layout simple, re-derive: indices 16 = pending,
        // 17 = ctrl. (with_capacity sizing already accounts for the +16
        // header containing version+prg+8 chr+mir+2 latch+2 counter+enabled.)
        let mut cursor = 16;
        let pending = data[cursor];
        cursor += 1;
        let ctrl = data[cursor];
        cursor += 1;
        self.irq_pending = pending != 0;
        self.eeprom_ctrl = ctrl;
        if let Some(ee) = self.eeprom.as_mut() {
            let n = ee.mem.len();
            ee.mem.copy_from_slice(&data[cursor..cursor + n]);
            cursor += n;
            if lines {
                ee.load_lines(&data[cursor..cursor + Eeprom::LINES_LEN]);
                cursor += Eeprom::LINES_LEN;
            } else {
                ee.idle_after(ctrl);
            }
        }
        self.vram
            .copy_from_slice(&data[cursor..cursor + self.vram.len()]);
        cursor += self.vram.len();
        if self.chr_is_ram {
            self.chr
                .copy_from_slice(&data[cursor..cursor + self.chr.len()]);
            cursor += self.chr.len();
        }
        self.outer = data[cursor] & 0x01;
        self.wram_enabled = data[cursor + 1] != 0;
        cursor += 2;
        self.wram
            .copy_from_slice(&data[cursor..cursor + self.wram.len()]);
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::cast_possible_truncation)]
mod tests {
    use super::*;

    fn synth_prg(banks_16k: usize) -> Box<[u8]> {
        let mut v = vec![0u8; banks_16k * PRG_BANK_16K];
        for b in 0..banks_16k {
            v[b * PRG_BANK_16K] = b as u8;
        }
        v.into_boxed_slice()
    }

    fn synth_chr(banks_1k: usize) -> Box<[u8]> {
        let mut v = vec![0u8; banks_1k * CHR_BANK_1K];
        for b in 0..banks_1k {
            v[b * CHR_BANK_1K] = b as u8;
        }
        v.into_boxed_slice()
    }

    #[test]
    fn lz93d50_prg_bank_and_fixed_last() {
        let mut m = BandaiFcg::new(
            synth_prg(8),
            synth_chr(16),
            Mirroring::Vertical,
            FcgVariant::Lz93d50_24c02,
        )
        .unwrap();
        assert_eq!(m.cpu_read(0x8000), 0);
        assert_eq!(m.cpu_read(0xC000), 7);
        m.cpu_write(0x8008, 3);
        assert_eq!(m.cpu_read(0x8000), 3);
        assert_eq!(m.cpu_read(0xC000), 7);
    }

    #[test]
    fn chr_bank_select_per_1k_slot() {
        let mut m = BandaiFcg::new(
            synth_prg(8),
            synth_chr(16),
            Mirroring::Vertical,
            FcgVariant::Lz93d50_24c02,
        )
        .unwrap();
        m.cpu_write(0x8000, 4); // CHR slot 0 -> bank 4
        m.cpu_write(0x8004, 9); // CHR slot 4 ($1000) -> bank 9
        assert_eq!(m.ppu_read(0x0000), 4);
        assert_eq!(m.ppu_read(0x1000), 9);
    }

    #[test]
    fn mirroring_control() {
        let mut m = BandaiFcg::new(
            synth_prg(8),
            synth_chr(16),
            Mirroring::Vertical,
            FcgVariant::Lz93d50_24c02,
        )
        .unwrap();
        m.cpu_write(0x8009, 1);
        assert_eq!(m.current_mirroring(), Mirroring::Horizontal);
        m.cpu_write(0x8009, 2);
        assert_eq!(m.current_mirroring(), Mirroring::SingleScreenA);
    }

    #[test]
    fn lz93d50_latched_irq_counts_down_to_zero() {
        let mut m = BandaiFcg::new(
            synth_prg(8),
            synth_chr(16),
            Mirroring::Vertical,
            FcgVariant::Lz93d50_24c02,
        )
        .unwrap();
        // Latch = 3.
        m.cpu_write(0x800B, 0x03);
        m.cpu_write(0x800C, 0x00);
        // Enable + copy latch->counter.
        m.cpu_write(0x800A, 0x01);
        assert_eq!(m.irq_counter, 3);
        m.notify_cpu_cycle(); // 3 -> 2
        m.notify_cpu_cycle(); // 2 -> 1
        m.notify_cpu_cycle(); // 1 -> 0
        assert!(!m.irq_pending());
        m.notify_cpu_cycle(); // counter holds 0 -> IRQ
        assert!(m.irq_pending());
    }

    #[test]
    fn fcg_writes_counter_directly() {
        let mut m = BandaiFcg::new(
            synth_prg(8),
            synth_chr(16),
            Mirroring::Vertical,
            FcgVariant::Fcg,
        )
        .unwrap();
        // FCG-1/2 responds in $6000-$7FFF and writes the counter directly.
        m.cpu_write(0x600B, 0x02);
        m.cpu_write(0x600C, 0x00);
        assert_eq!(m.irq_counter, 2);
        m.cpu_write(0x600A, 0x01); // enable (no latch copy)
        assert_eq!(m.irq_counter, 2);
    }

    #[test]
    fn irq_acknowledge_on_control_write() {
        let mut m = BandaiFcg::new(
            synth_prg(8),
            synth_chr(16),
            Mirroring::Vertical,
            FcgVariant::Lz93d50_24c02,
        )
        .unwrap();
        m.irq_pending = true;
        m.cpu_write(0x800A, 0x00); // disable + ack
        assert!(!m.irq_pending());
        assert!(!m.irq_enabled);
    }

    #[test]
    fn eeprom_present_for_159_absent_for_fcg() {
        let m159 = BandaiFcg::new(
            synth_prg(8),
            synth_chr(16),
            Mirroring::Vertical,
            FcgVariant::Lz93d50_24c01,
        )
        .unwrap();
        assert!(m159.eeprom.is_some());
        assert_eq!(m159.eeprom.as_ref().unwrap().mem.len(), 128);
        let mfcg = BandaiFcg::new(
            synth_prg(8),
            synth_chr(16),
            Mirroring::Vertical,
            FcgVariant::Fcg,
        )
        .unwrap();
        assert!(mfcg.eeprom.is_none());
    }

    #[test]
    fn eeprom_idle_reads_high() {
        let mut m = BandaiFcg::new(
            synth_prg(8),
            synth_chr(16),
            Mirroring::Vertical,
            FcgVariant::Lz93d50_24c01,
        )
        .unwrap();
        // With no I2C transaction, the device releases SDA (out_bit high) ->
        // bit 4 set.
        assert_eq!(m.cpu_read(0x6000) & 0x10, 0x10);
    }

    /// Drive the X24C01 I²C lines directly through the `$800D` register,
    /// mirroring how a game bit-bangs the bus: SDA is set up while SCL is
    /// low, then SCL is pulsed high and back low to clock the bit.
    struct I2cDriver<'a> {
        m: &'a mut BandaiFcg,
    }

    impl I2cDriver<'_> {
        const SCL: u8 = 0x20;
        const SDA: u8 = 0x40;

        fn lines(&mut self, scl: bool, sda: bool) {
            let v = (u8::from(scl) * Self::SCL) | (u8::from(sda) * Self::SDA);
            self.m.cpu_write(0x800D, v);
        }

        fn start(&mut self) {
            // SDA high, SCL high, then SDA falls while SCL stays high.
            self.lines(true, true);
            self.lines(true, false);
        }

        fn stop(&mut self) {
            // SDA low while SCL high, then SDA rises while SCL stays high.
            self.lines(true, false);
            self.lines(true, true);
        }

        /// Clock one bit out from the master to the device (write direction).
        fn send_bit(&mut self, bit: bool) {
            self.lines(false, bit); // set SDA while SCL low
            self.lines(true, bit); // clock rise (device samples)
            self.lines(false, bit); // clock fall (device advances)
        }

        /// Clock one bit while releasing SDA, returning the device's output.
        fn recv_bit(&mut self) -> bool {
            self.lines(false, true); // release SDA, SCL low
            self.lines(true, true); // clock rise (device drives output)
            let out = self.m.eeprom.as_ref().unwrap().read_sda();
            self.lines(false, true); // clock fall
            out
        }

        /// The X24C01 word-address byte is LSB-first: 7 address bits then R/W.
        fn send_addr_rw(&mut self, addr: u8, read: bool) {
            for i in 0..7 {
                self.send_bit((addr >> i) & 1 != 0);
            }
            self.send_bit(read);
        }

        /// Read the device-driven ack bit (low = ack).
        fn read_ack(&mut self) -> bool {
            !self.recv_bit()
        }

        fn send_data_lsb_first(&mut self, byte: u8) {
            for i in 0..8 {
                self.send_bit((byte >> i) & 1 != 0);
            }
        }

        fn recv_data_lsb_first(&mut self) -> u8 {
            let mut byte = 0u8;
            for i in 0..8 {
                if self.recv_bit() {
                    byte |= 1 << i;
                }
            }
            byte
        }
    }

    /// A save state taken MID-TRANSACTION must restore the protocol, not just
    /// the EEPROM's bytes (`CodeRabbit` on #604). Rollback, run-ahead and rewind
    /// all restore at arbitrary cycles, and a game polling its save chip spans
    /// many frames; before v3.2.0's version 3 the blob carried only `mem` and
    /// `$800D`, so the restored board was Idle where the original was halfway
    /// through a read, and the two diverged.
    #[test]
    fn a_state_saved_mid_read_restores_the_transaction() {
        let variant = FcgVariant::Lz93d50_24c01;
        let mut a =
            BandaiFcg::new(synth_prg(8), synth_chr(16), Mirroring::Vertical, variant).unwrap();
        a.eeprom.as_mut().unwrap().mem[0x05] = 0xAB;
        {
            let mut d = I2cDriver { m: &mut a };
            d.start();
            d.send_addr_rw(0x05, true);
            assert!(d.read_ack(), "the X24C01 acknowledges its address");
        }
        let blob = a.save_state();

        let mut b =
            BandaiFcg::new(synth_prg(8), synth_chr(16), Mirroring::Vertical, variant).unwrap();
        b.eeprom.as_mut().unwrap().mem[0x05] = 0x00; // overwritten by the load
        b.load_state(&blob).unwrap();

        let from_a = I2cDriver { m: &mut a }.recv_data_lsb_first();
        let from_b = I2cDriver { m: &mut b }.recv_data_lsb_first();
        assert_eq!(from_a, 0xAB);
        assert_eq!(from_b, from_a, "the restored board continues the same read");
        assert_eq!(b.save_state(), a.save_state());
    }

    /// The protocol bytes are validated before anything is restored: an
    /// unknown mode tag, a counter past 8 or a line above 1 is refused, and the
    /// board keeps its state. So is an unknown mirroring byte, which used to
    /// return Err only after the banks had been restored (`CodeRabbit` on #607).
    /// A state the X24C01 could never be in is refused: a word address past
    /// its 128 bytes, or the 24C02-only device-select phase. Either would make
    /// the next clocked read index past `mem` and panic.
    #[test]
    fn an_x24c01_state_it_cannot_reach_is_refused() {
        let variant = FcgVariant::Lz93d50_24c01;
        let m = BandaiFcg::new(synth_prg(8), synth_chr(16), Mirroring::Vertical, variant).unwrap();
        let good = m.save_state();
        let lines = 18 + m.eeprom.as_ref().unwrap().mem.len();
        let mut other =
            BandaiFcg::new(synth_prg(8), synth_chr(16), Mirroring::Vertical, variant).unwrap();
        let before = other.save_state();
        for (at, value, what) in [
            (lines + 5, 0x80, "word address $80"),
            (
                lines + 2,
                I2cMode::ChipAddress.to_byte(),
                "ChipAddress mode",
            ),
            (
                lines + 3,
                I2cMode::ChipAddress.to_byte(),
                "ChipAddress next mode",
            ),
        ] {
            let mut bad = good.clone();
            bad[at] = value;
            assert!(other.load_state(&bad).is_err(), "{what} refused");
            assert_eq!(other.save_state(), before, "{what}: nothing restored");
        }
    }

    /// Turn a current (version 3) blob into the version 2 blob v3.1.x wrote
    /// for the same board: the same bytes with the protocol record after the
    /// EEPROM contents left out (v3.1.0's `save_state`, read from the tag).
    fn as_v2(m: &BandaiFcg, v3: &[u8]) -> Vec<u8> {
        let mut v2 = v3.to_vec();
        v2[0] = 2;
        if let Some(ee) = m.eeprom.as_ref() {
            let at = 18 + ee.mem.len();
            v2.drain(at..at + Eeprom::LINES_LEN);
        }
        v2
    }

    /// A v3.1.x save state (version 2) of a Bandai board loads again
    /// (`T-FCG-EEPROM-STATE`). v3.2.0 refused it outright when version 3 added
    /// the chip's protocol record. Version 2 never stored that record, so the
    /// chip comes back idle; and it comes back idle DETERMINISTICALLY, whatever
    /// the board was doing before the load, which v3.1.x itself did not do
    /// (it left the live protocol state in place).
    #[test]
    fn a_v3_1_state_loads_with_the_chip_idle() {
        for variant in [
            FcgVariant::Both,
            FcgVariant::Fcg,
            FcgVariant::Lz93d50_24c02,
            FcgVariant::Lz93d50_24c01,
            FcgVariant::Lz93d50Wram,
        ] {
            let board = || {
                let chr = if variant == FcgVariant::Lz93d50Wram {
                    Box::new([])
                } else {
                    synth_chr(16)
                };
                BandaiFcg::new(synth_prg(32), chr, Mirroring::Vertical, variant).unwrap()
            };
            let mut a = board();
            a.cpu_write(0x8008, 3);
            a.cpu_write(0x6008, 3);
            a.cpu_write(0x800B, 0x34);
            a.cpu_write(0x800D, 0x60); // SCL and SDA high: an idle bus
            if let Some(ee) = a.eeprom.as_mut() {
                ee.mem[0x12] = 0x5A;
            }
            let v2 = as_v2(&a, &a.save_state());

            // Two boards in different protocol states before the load.
            let mut b = board();
            let mut c = board();
            if c.eeprom.is_some() {
                let mut d = I2cDriver { m: &mut c };
                d.start();
                d.send_addr_rw(0x05, true);
            }
            b.load_state(&v2)
                .unwrap_or_else(|e| panic!("{variant:?}: {e}"));
            c.load_state(&v2)
                .unwrap_or_else(|e| panic!("{variant:?}: {e}"));
            assert_eq!(b.save_state(), c.save_state(), "{variant:?}: deterministic");
            assert_eq!(
                b.cpu_read(0x8000),
                a.cpu_read(0x8000),
                "{variant:?}: PRG bank"
            );
            assert_eq!(b.irq_latch, a.irq_latch, "{variant:?}: IRQ latch");
            assert_eq!(b.eeprom_ctrl, a.eeprom_ctrl, "{variant:?}: `$x00D`");
            if let Some(ee) = b.eeprom.as_ref() {
                assert_eq!(ee.mem[0x12], 0x5A, "{variant:?}: contents");
                assert_eq!(ee.mode, I2cMode::Idle, "{variant:?}: idle");
                assert_eq!((ee.last_scl, ee.last_sda), (1, 1), "{variant:?}: lines");
            }
            // The restored chip answers a fresh read (the driver speaks the
            // X24C01's protocol, which has no device-select byte).
            if variant == FcgVariant::Lz93d50_24c01 {
                let mut d = I2cDriver { m: &mut b };
                d.start();
                d.send_addr_rw(0x12, true);
                assert!(d.read_ack(), "{variant:?}: acknowledges");
                assert_eq!(d.recv_data_lsb_first(), 0x5A, "{variant:?}: reads back");
            }
        }
    }

    /// The same, against bytes the v3.1.0 code itself wrote rather than ones
    /// derived here: `tests/fixtures/bandai-v3.1.0/` holds `save_state` blobs
    /// dumped from a v3.1.0 checkout (synthetic PRG and CHR, the register
    /// writes below). Each loads, and the loaded board, saved again and put
    /// back in the version 2 layout, reproduces the fixture byte for byte,
    /// which also shows [`as_v2`] matches the real format.
    #[test]
    fn states_written_by_v3_1_0_load() {
        for (bytes, variant) in [
            (
                &include_bytes!("../tests/fixtures/bandai-v3.1.0/both.bin")[..],
                FcgVariant::Both,
            ),
            (
                &include_bytes!("../tests/fixtures/bandai-v3.1.0/fcg.bin")[..],
                FcgVariant::Fcg,
            ),
            (
                &include_bytes!("../tests/fixtures/bandai-v3.1.0/c02.bin")[..],
                FcgVariant::Lz93d50_24c02,
            ),
            (
                &include_bytes!("../tests/fixtures/bandai-v3.1.0/c01.bin")[..],
                FcgVariant::Lz93d50_24c01,
            ),
            (
                &include_bytes!("../tests/fixtures/bandai-v3.1.0/wram.bin")[..],
                FcgVariant::Lz93d50Wram,
            ),
        ] {
            let board = || {
                let chr: Box<[u8]> = if variant == FcgVariant::Lz93d50Wram {
                    Box::new([])
                } else {
                    (0..16 * 1024_usize).map(|i| (i % 251) as u8).collect()
                };
                let prg: Box<[u8]> = (0..32 * PRG_BANK_16K)
                    .map(|i| (i / PRG_BANK_16K) as u8)
                    .collect();
                BandaiFcg::new(prg, chr, Mirroring::Vertical, variant).unwrap()
            };
            let mut a = board();
            a.cpu_write(0x8008, 3);
            a.cpu_write(0x6008, 3);
            a.cpu_write(0x800B, 0x34);
            a.cpu_write(0x800D, 0x60);
            if let Some(ee) = a.eeprom.as_mut() {
                ee.mem[0x12] = 0x5A;
            }
            assert_eq!(as_v2(&a, &a.save_state()), bytes, "{variant:?}: layout");
            let mut b = board();
            b.load_state(bytes)
                .unwrap_or_else(|e| panic!("{variant:?}: {e}"));
            assert_eq!(as_v2(&b, &b.save_state()), bytes, "{variant:?}: restored");
        }
    }

    /// Mapper 157 first shipped in v3.2.0 (version 3), so no version 2 blob of
    /// it exists to load; one is refused rather than guessed at.
    #[test]
    fn a_version_2_datach_state_is_refused() {
        let m = datach(false);
        let mut v2 = m.save_state();
        v2[0] = 2;
        let mut b = datach(false);
        assert!(matches!(
            b.load_state(&v2),
            Err(MapperError::UnsupportedVersion(2))
        ));
    }

    #[test]
    fn a_corrupt_state_is_refused_before_anything_changes() {
        let variant = FcgVariant::Lz93d50_24c02;
        let mut m =
            BandaiFcg::new(synth_prg(8), synth_chr(16), Mirroring::Vertical, variant).unwrap();
        m.cpu_write(0x8008, 3);
        let good = m.save_state();
        let lines = 18 + m.eeprom.as_ref().unwrap().mem.len();
        let mut other =
            BandaiFcg::new(synth_prg(8), synth_chr(16), Mirroring::Vertical, variant).unwrap();
        other.cpu_write(0x8008, 1);
        let before = other.save_state();
        for (at, value, what) in [
            (lines + 2, 7, "mode tag"),
            (lines + 7, 9, "bit counter"),
            (lines, 2, "SCL level"),
            (10, 9, "mirroring"),
        ] {
            let mut bad = good.clone();
            bad[at] = value;
            assert!(other.load_state(&bad).is_err(), "{what} {value} refused");
            assert_eq!(other.save_state(), before, "{what}: nothing restored");
        }
        other.load_state(&good).unwrap();
        assert_eq!(other.save_state(), good);
    }

    #[test]
    fn x24c01_write_then_read_round_trips() {
        let mut m = BandaiFcg::new(
            synth_prg(8),
            synth_chr(16),
            Mirroring::Vertical,
            FcgVariant::Lz93d50_24c01,
        )
        .unwrap();
        {
            let mut d = I2cDriver { m: &mut m };
            // Write 0x5A to word address 0x12.
            d.start();
            d.send_addr_rw(0x12, false); // write
            assert!(d.read_ack(), "device must ack the address byte");
            d.send_data_lsb_first(0x5A);
            assert!(d.read_ack(), "device must ack the data byte");
            d.stop();

            // Read it back from word address 0x12.
            d.start();
            d.send_addr_rw(0x12, true); // read
            assert!(d.read_ack(), "device must ack the read address");
            let got = d.recv_data_lsb_first();
            assert_eq!(got, 0x5A, "read-back must match the written byte");
            d.stop();
        }
        assert_eq!(m.eeprom.as_ref().unwrap().mem[0x12], 0x5A);
    }

    #[test]
    fn save_state_round_trip_with_eeprom() {
        let mut m = BandaiFcg::new(
            synth_prg(8),
            synth_chr(16),
            Mirroring::Horizontal,
            FcgVariant::Lz93d50_24c02,
        )
        .unwrap();
        m.cpu_write(0x8008, 2);
        m.cpu_write(0x8000, 5);
        m.cpu_write(0x800B, 0x10);
        m.cpu_write(0x800A, 0x01);
        if let Some(ee) = m.eeprom.as_mut() {
            ee.mem[0] = 0x42;
        }
        let blob = m.save_state();
        let mut m2 = BandaiFcg::new(
            synth_prg(8),
            synth_chr(16),
            Mirroring::Horizontal,
            FcgVariant::Lz93d50_24c02,
        )
        .unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m.cpu_read(0x8000), m2.cpu_read(0x8000));
        assert_eq!(m.ppu_read(0x0000), m2.ppu_read(0x0000));
        assert_eq!(m.irq_counter, m2.irq_counter);
        assert_eq!(m2.eeprom.as_ref().unwrap().mem[0], 0x42);
    }

    // ---- EEPROM address-counter rollover ------------------------------------
    //
    // The counter is a `u8` masked to 0xFF (0x7F on the X24C01), so the mask
    // cannot express the wrap on its own and the add has to do it. These pin
    // both chips at the boundary, for the read path and the write path, because
    // the bug was a debug-build panic rather than a wrong value -- a test that
    // only checked the returned byte would have passed before the fix.

    #[test]
    fn eeprom_address_wraps_at_the_top_of_each_chip() {
        for (bytes, is_x24c01, top) in [(256usize, false, 0xFFu8), (128, true, 0x7F)] {
            let mut e = Eeprom::new(bytes, is_x24c01);
            e.addr = top;
            e.data = 0xA5;
            e.counter = 8;
            e.mode = I2cMode::Write;
            // Completing the byte must advance past the top without trapping.
            e.clock_fall(0);
            assert_eq!(e.addr, 0, "x24c01={is_x24c01}: ${top:02X} + 1 wraps to $00");
        }
    }

    #[test]
    fn eeprom_sequential_read_wraps_at_the_top_of_each_chip() {
        for (bytes, is_x24c01, top) in [(256usize, false, 0xFFu8), (128, true, 0x7F)] {
            let mut e = Eeprom::new(bytes, is_x24c01);
            e.addr = top;
            e.counter = 8;
            e.mode = I2cMode::Read;
            e.clock_fall(0);
            assert_eq!(e.addr, 0, "x24c01={is_x24c01}: read rollover reaches $00");
        }
    }

    // ---- Mapper 153 (`INES_Mapper_153.md`) ------------------------------

    fn m153(banks_16k: usize) -> BandaiFcg {
        BandaiFcg::new(
            synth_prg(banks_16k),
            Box::new([]),
            Mirroring::Vertical,
            FcgVariant::Lz93d50Wram,
        )
        .unwrap()
    }

    #[test]
    fn m153_outer_bank_from_8000_to_8003_and_fixed_last_of_the_block() {
        let mut m = m153(32); // 512 KiB
        m.cpu_write(0x8008, 3);
        assert_eq!(m.cpu_read(0x8000), 3);
        assert_eq!(m.cpu_read(0xC000), 15, "last bank of the first 256 KiB");
        for a in 0x8000..=0x8003u16 {
            m.cpu_write(a, 1);
        }
        assert_eq!(m.cpu_read(0x8000), 16 + 3);
        assert_eq!(m.cpu_read(0xC000), 31);
        // `$8004-$8007` do nothing on this board.
        m.cpu_write(0x8004, 0);
        assert_eq!(m.cpu_read(0x8000), 16 + 3);
    }

    #[test]
    fn m153_wram_follows_the_800d_chip_enable() {
        let mut m = m153(32);
        assert_eq!(m.sram().len(), WRAM_153, "the battery save is the WRAM");
        assert!(m.cpu_read_unmapped(0x6000), "disabled at power-on");
        m.cpu_write(0x6000, 0x11);
        m.cpu_write(0x800D, 0x20);
        assert!(!m.cpu_read_unmapped(0x6000));
        assert_eq!(m.cpu_read(0x6000), 0, "the disabled write was dropped");
        m.cpu_write(0x7FFF, 0x22);
        assert_eq!(m.cpu_read(0x7FFF), 0x22);
        m.cpu_write(0x800D, 0x00);
        assert!(m.cpu_read_unmapped(0x7FFF));
    }

    #[test]
    fn m153_chr_is_unbanked_ram_and_irq_is_the_lz93d50s() {
        let mut m = m153(32);
        m.cpu_write(0x8000, 1); // outer bank, not CHR bank 0
        m.ppu_write(0x0005, 0x9A);
        assert_eq!(m.ppu_read(0x0005), 0x9A);
        m.cpu_write(0x800B, 2);
        m.cpu_write(0x800C, 0);
        m.cpu_write(0x800A, 1);
        for _ in 0..3 {
            m.notify_cpu_cycle();
        }
        assert!(m.irq_pending(), "latched counter 2 reaches zero");
    }

    /// `INES_Mapper_153`: "PPU $0000-$1FFF: 8 KiB unbanked CHR-RAM" and "No
    /// CHR banking is available". Every byte of the 8 KiB is its own cell, so
    /// the eight 1 KiB windows must not alias one another. The v2.9.6 model
    /// routed CHR-RAM through the CHR bank registers, which this board never
    /// writes, so all eight windows landed on the first 1 KiB and *Famicom
    /// Jump II*'s pattern tables overwrote each other (the striped title).
    #[test]
    fn m153_chr_ram_is_8k_with_no_aliasing_between_1k_windows() {
        let mut m = m153(32);
        // Writes to the CHR-register offsets are the outer bank on this board
        // and must not reach the CHR mapping either.
        for a in 0x8000..=0x8007u16 {
            m.cpu_write(a, 0x05);
        }
        for slot in 0..8u16 {
            m.ppu_write(slot * 0x400 + 0x123, 0xA0 | slot as u8);
        }
        for slot in 0..8u16 {
            assert_eq!(
                m.ppu_read(slot * 0x400 + 0x123),
                0xA0 | slot as u8,
                "1 KiB window {slot} aliased another window"
            );
        }
        m.ppu_write(0x1FFF, 0x5A);
        assert_eq!(m.ppu_read(0x1FFF), 0x5A);
        assert_eq!(m.ppu_read(0x03FF), 0, "$1FFF must not alias $03FF");
    }

    #[test]
    fn m153_state_round_trips_and_v1_is_refused() {
        let mut a = m153(32);
        a.cpu_write(0x8001, 1);
        a.cpu_write(0x800D, 0x20);
        a.cpu_write(0x6123, 0x44);
        let blob = a.save_state();
        let mut b = m153(32);
        b.load_state(&blob).unwrap();
        assert_eq!(b.cpu_read(0x6123), 0x44);
        assert_eq!(b.cpu_read(0xC000), 31);
        assert_eq!(b.save_state(), blob);
        let mut v1 = blob.clone();
        v1[0] = 1;
        v1.truncate(v1.len() - 2 - WRAM_153);
        assert!(
            b.load_state(&v1).is_err(),
            "a v1 blob has no WRAM to restore"
        );
        // Since v2.9.8 (ADR 0042) a v1 blob is refused on a board with no 153
        // tail too; it used to load there.
        let mut fcg = BandaiFcg::new(
            synth_prg(8),
            synth_chr(128),
            Mirroring::Vertical,
            FcgVariant::Fcg,
        )
        .unwrap();
        let mut v1 = fcg.save_state();
        v1[0] = 1;
        v1.truncate(v1.len() - 2);
        assert!(matches!(
            fcg.load_state(&v1),
            Err(MapperError::UnsupportedVersion(1))
        ));
    }

    // ---- Mapper 157, the Datach Joint ROM System (`INES_Mapper_157`) ------

    /// A NES 2.0 image for mapper 157: `banks_16k` PRG banks numbered in
    /// their first byte, no CHR-ROM (8 KiB CHR-RAM), and NES 2.0 byte 10 set
    /// to `0x10` when the game cartridge carries the external X24C01 (the
    /// page's *Battle Rush* row) or `0x00` when it does not.
    fn m157_rom(banks_16k: usize, external_eeprom: bool) -> Vec<u8> {
        let mut rom = vec![0u8; 16];
        rom[0..4].copy_from_slice(b"NES\x1A");
        rom[4] = banks_16k as u8;
        rom[5] = 0;
        rom[6] = 0xD0; // mapper low nibble $D (157 = $9D)
        rom[7] = 0x98; // mapper high nibble $9, NES 2.0 identifier
        rom[10] = if external_eeprom { 0x10 } else { 0x00 };
        rom[11] = 0x07; // 8 KiB CHR-RAM
        rom.extend_from_slice(&synth_prg(banks_16k));
        rom
    }

    fn m157(external_eeprom: bool) -> Box<dyn Mapper> {
        crate::parse(&m157_rom(16, external_eeprom))
            .expect("mapper 157 must construct")
            .1
    }

    /// Banks: "CPU $8000-$BFFF: 16 KiB switchable PRG ROM bank", "CPU
    /// $C000-$FFFF: 16 KiB PRG ROM bank, fixed to the last bank", and the
    /// `$8008` register that "function[s] the same way as on INES Mapper 016,
    /// submapper 5" (mask `$800F`, so `$FFF8` decodes as `$8008`).
    #[test]
    fn m157_prg_bank_at_8008_and_the_last_bank_fixed() {
        let mut m = m157(false);
        assert_eq!(m.cpu_read(0x8000), 0);
        assert_eq!(m.cpu_read(0xC000), 15);
        m.cpu_write(0x8008, 5);
        assert_eq!(m.cpu_read(0x8000), 5);
        assert_eq!(m.cpu_read(0xBFFF & 0xC000), 5);
        assert_eq!(m.cpu_read(0xC000), 15);
        m.cpu_write(0xFFF8, 9);
        assert_eq!(m.cpu_read(0x8000), 9, "mask $800F: $FFF8 is $8008");
        // The LZ93D50's register window is $8000-$FFFF only.
        m.cpu_write(0x6008, 2);
        assert_eq!(m.cpu_read(0x8000), 9, "$6008 is not a register here");
    }

    /// "PPU $0000-$1FFF: 8 KiB unbanked CHR-RAM" and "No CHR banking is
    /// available": writes to `$8000-$8007` must not move any CHR window, and
    /// the eight 1 KiB windows must not alias.
    #[test]
    fn m157_chr_is_8k_of_unbanked_ram() {
        let mut m = m157(false);
        for a in 0x8000..=0x8007u16 {
            m.cpu_write(a, 0x03);
        }
        for slot in 0..8u16 {
            m.ppu_write(slot * 0x400 + 0x055, 0xB0 | slot as u8);
        }
        for slot in 0..8u16 {
            assert_eq!(m.ppu_read(slot * 0x400 + 0x055), 0xB0 | slot as u8);
        }
    }

    /// `$8009`: "the same way as on INES Mapper 016" (0 V, 1 H, 2 1scA, 3 1scB).
    #[test]
    fn m157_mirroring_at_8009() {
        let mut m = m157(false);
        for (v, want) in [
            (0u8, Mirroring::Vertical),
            (1, Mirroring::Horizontal),
            (2, Mirroring::SingleScreenA),
            (3, Mirroring::SingleScreenB),
        ] {
            m.cpu_write(0x8009, v);
            assert_eq!(m.current_mirroring(), want, "$8009 = {v}");
        }
    }

    /// `$800A-$800C`: the LZ93D50's latched 16-bit M2 down-counter.
    #[test]
    fn m157_irq_is_the_latched_lz93d50_counter() {
        let mut m = m157(false);
        m.cpu_write(0x800B, 0x02);
        m.cpu_write(0x800C, 0x00);
        m.cpu_write(0x800A, 0x01);
        m.notify_cpu_cycle();
        m.notify_cpu_cycle();
        assert!(!m.irq_pending());
        m.notify_cpu_cycle();
        assert!(m.irq_pending(), "latch 2 reaches zero on the third M2");
        m.cpu_write(0x800A, 0x00);
        assert!(!m.irq_pending(), "a $800A write acknowledges");
    }

    /// The main unit's 24C02 is the battery save, 256 bytes, with or without
    /// the cartridge's X24C01.
    #[test]
    fn m157_battery_save_is_the_main_units_24c02() {
        assert_eq!(m157(false).sram().len(), 256);
        assert_eq!(m157(true).sram().len(), 256);
    }

    /// `$6000-$7FFF` read: bit 4 is the EEPROMs' data out (released = high
    /// when idle), bit 3 the barcode scanner, the rest open bus.
    #[test]
    fn m157_6000_drives_bits_4_and_3() {
        let mut m = m157(true);
        assert!(!m.cpu_read_unmapped(0x6000));
        let v = m.cpu_read(0x7FFF);
        assert_eq!(v & 0x10, 0x10, "idle EEPROMs release SDA");
        assert_eq!(v & 0x08, 0, "an empty barcode slot reads dark (0)");
        assert_eq!(v & !0x18, 0, "undriven bits read 0 here");
    }

    fn datach(external: bool) -> BandaiFcg {
        BandaiFcg::new_datach(synth_prg(16), Box::new([]), Mirroring::Vertical, external).unwrap()
    }

    /// Drive the Datach's two I2C buses. The internal 24C02 is clocked by
    /// `$800D` bit 5, the external X24C01 by `$8000-$8003` bit 3; both share
    /// SDA, `$800D` bit 6 (`INES_Mapper_157`, "EEPROM Control" and "External
    /// EEPROM Clock").
    struct DatachI2c<'a> {
        m: &'a mut BandaiFcg,
        external: bool,
    }

    impl DatachI2c<'_> {
        fn lines(&mut self, scl: bool, sda: bool) {
            if self.external {
                // Set SDA with the internal clock held low, then the external
                // clock through CHR A13 OUT.
                self.m.cpu_write(0x800D, u8::from(sda) << 6);
                self.m.cpu_write(0x8000, u8::from(scl) << 3);
            } else {
                self.m
                    .cpu_write(0x800D, (u8::from(sda) << 6) | (u8::from(scl) << 5));
            }
        }
        fn start(&mut self) {
            self.lines(false, true);
            self.lines(true, true);
            self.lines(true, false);
            self.lines(false, false);
        }
        fn stop(&mut self) {
            self.lines(false, false);
            self.lines(true, false);
            self.lines(true, true);
        }
        fn send_bit(&mut self, bit: bool) {
            self.lines(false, bit);
            self.lines(true, bit);
            self.lines(false, bit);
        }
        /// Release SDA, clock once, and sample `$6000` bit 4 while SCL is high.
        fn recv_bit(&mut self) -> bool {
            self.lines(false, true);
            self.lines(true, true);
            let bit = self.m.cpu_read(0x6000) & 0x10 != 0;
            self.lines(false, true);
            bit
        }
        fn send_msb_first(&mut self, byte: u8) -> bool {
            for i in (0..8).rev() {
                self.send_bit(byte >> i & 1 != 0);
            }
            !self.recv_bit()
        }
        fn recv_msb_first(&mut self) -> u8 {
            (0..8).fold(0u8, |acc, _| acc << 1 | u8::from(self.recv_bit()))
        }
    }

    /// The main unit's 24C02 at device address 000b ("the highest device
    /// type bits being 1010b"): a byte write, then a **random read** (dummy
    /// write of the word address, repeated START, read select), which must
    /// begin at the address just written. Before the v3.2.0
    /// `keeps_word_address` change a repeated START cleared the address and
    /// this read returned byte `$00`'s contents.
    #[test]
    fn m157_internal_24c02_random_read_starts_at_the_written_address() {
        let mut m = datach(false);
        {
            let mut d = DatachI2c {
                m: &mut m,
                external: false,
            };
            d.start();
            assert!(d.send_msb_first(0xA0), "device select acked");
            assert!(d.send_msb_first(0x37), "word address acked");
            assert!(d.send_msb_first(0xC4), "data acked");
            d.stop();
            d.start();
            assert!(d.send_msb_first(0xA0));
            assert!(d.send_msb_first(0x37));
            d.start(); // repeated START
            assert!(d.send_msb_first(0xA1), "read select acked");
            assert_eq!(d.recv_msb_first(), 0xC4, "random read of $37");
            d.stop();
        }
        assert_eq!(m.sram()[0x37], 0xC4);
        assert_eq!(m.sram()[0x00], 0xFF, "byte $00 untouched");
    }

    /// v3.2.0: the same random read on mapper 16's LZ93D50 + 24C02 board.
    /// Keeping the word address across a START is a property of the 24C02,
    /// not of the Datach, so it holds on every board that carries one.
    #[test]
    fn m016_24c02_random_read_starts_at_the_written_address() {
        let mut m = BandaiFcg::new(
            synth_prg(8),
            synth_chr(16),
            Mirroring::Vertical,
            FcgVariant::Lz93d50_24c02,
        )
        .unwrap();
        {
            let mut d = DatachI2c {
                m: &mut m,
                external: false,
            };
            d.start();
            assert!(d.send_msb_first(0xA0), "device select acked");
            assert!(d.send_msb_first(0x5A), "word address acked");
            assert!(d.send_msb_first(0x3C), "data acked");
            d.stop();
            d.start();
            assert!(d.send_msb_first(0xA0));
            assert!(d.send_msb_first(0x5A));
            d.start(); // repeated START
            assert!(d.send_msb_first(0xA1), "read select acked");
            assert_eq!(d.recv_msb_first(), 0x3C, "random read of $5A");
            d.stop();
        }
        assert_eq!(m.sram()[0x5A], 0x3C);
    }

    /// The cartridge's X24C01 answers only on its own clock, `$8000-$8003`
    /// bit 3, and (Bandai's convention, modelled by the existing X24C01
    /// state machine) shifts LSB-first. The 24C02 never sees that clock, so
    /// its contents are untouched, and `$8004-$8007` clock nothing.
    #[test]
    fn m157_external_x24c01_is_clocked_by_8000_bit_3_only() {
        let mut m = datach(true);
        {
            let mut d = DatachI2c {
                m: &mut m,
                external: true,
            };
            d.start();
            // X24C01: 7 address bits LSB-first, then R/W = 0.
            for i in 0..7 {
                d.send_bit(0x15 >> i & 1 != 0);
            }
            d.send_bit(false);
            assert!(!d.recv_bit(), "address acked");
            for i in 0..8 {
                d.send_bit(0x6B >> i & 1 != 0);
            }
            assert!(!d.recv_bit(), "data acked");
            d.stop();
        }
        assert_eq!(m.ext_eeprom.as_ref().unwrap().mem[0x15], 0x6B);
        assert!(m.sram().iter().all(|&b| b == 0xFF), "24C02 untouched");
        // `$8004` is not the external clock: toggling it with SDA moving
        // does not form a START on the X24C01.
        m.cpu_write(0x8000, 0x00); // external SCL low
        m.cpu_write(0x800D, 0x40); // SDA high
        let before = m.ext_eeprom.as_ref().unwrap().mode;
        m.cpu_write(0x8004, 0x08); // would raise SCL if it were decoded
        m.cpu_write(0x800D, 0x00); // SDA falls: a START only if SCL were high
        assert_eq!(m.ext_eeprom.as_ref().unwrap().mode, before);
        assert!(!m.ext_scl);
        // The same sequence through `$8003` is a START.
        m.cpu_write(0x800D, 0x40);
        m.cpu_write(0x8003, 0x08);
        m.cpu_write(0x800D, 0x00);
        assert_eq!(m.ext_eeprom.as_ref().unwrap().mode, I2cMode::Address);
    }

    /// "the Data out from both EEPROMs is ANDed": either chip pulling SDA
    /// low pulls bit 4 low.
    #[test]
    fn m157_eeprom_outputs_are_wired_and() {
        let mut m = datach(true);
        assert_eq!(m.cpu_read(0x6000) & 0x10, 0x10);
        m.ext_eeprom.as_mut().unwrap().output = 0;
        assert_eq!(m.cpu_read(0x6000) & 0x10, 0, "external low");
        m.ext_eeprom.as_mut().unwrap().output = 1;
        m.eeprom.as_mut().unwrap().output = 0;
        assert_eq!(m.cpu_read(0x6000) & 0x10, 0, "internal low");
    }

    /// The scanner's stream on bit 3: dark (0) idle, then the card's light
    /// margin, the EAN-8 symbol 96385074 module by module (bar = 0, space =
    /// 1), the light margin, and dark again.
    #[test]
    fn m157_scan_presents_the_symbol_on_bit_3() {
        use crate::datach_barcode::{MODULE_CYCLES, QUIET_MODULES};
        let mut m = datach(false);
        assert!(m.has_barcode_reader());
        let code = Barcode::parse("96385074").unwrap();
        assert!(m.scan_barcode(&code));
        let mut seen = alloc::string::String::new();
        let total = (67 + 2 * QUIET_MODULES) * MODULE_CYCLES;
        for cycle in 0..total {
            if cycle % MODULE_CYCLES == MODULE_CYCLES / 2 {
                seen.push(if m.cpu_read(0x6000) & 0x08 != 0 {
                    '1'
                } else {
                    '0'
                });
            }
            m.notify_cpu_cycle();
        }
        let margin = "1".repeat(QUIET_MODULES as usize);
        // The published EAN-8 modules for 96385074, inverted (bar = 0).
        let symbol: alloc::string::String = concat!(
            "101", "0001011", "0101111", "0111101", "0110111", "01010", "1001110", "1110010",
            "1000100", "1011100", "101",
        )
        .chars()
        .map(|c| if c == '1' { '0' } else { '1' })
        .collect();
        assert_eq!(seen, alloc::format!("{margin}{symbol}{margin}"));
        assert!(!m.barcode_scan_in_progress());
        assert_eq!(m.cpu_read(0x6000) & 0x08, 0, "dark again when done");
    }

    #[test]
    fn only_the_datach_has_a_barcode_reader() {
        let code = Barcode::parse("96385074").unwrap();
        for v in [
            FcgVariant::Both,
            FcgVariant::Fcg,
            FcgVariant::Lz93d50_24c02,
            FcgVariant::Lz93d50_24c01,
        ] {
            let mut m =
                BandaiFcg::new(synth_prg(8), synth_chr(16), Mirroring::Vertical, v).unwrap();
            assert!(!m.has_barcode_reader(), "{v:?}");
            assert!(!m.scan_barcode(&code), "{v:?}");
        }
    }

    /// A save state taken mid-scan restores mid-scan: the remaining stream,
    /// the external EEPROM and its clock all come back, and a truncated or
    /// padded tail is refused.
    #[test]
    fn m157_state_round_trips_mid_scan() {
        let mut a = datach(true);
        a.cpu_write(0x8008, 3);
        a.cpu_write(0x8001, 0x08);
        a.ext_eeprom.as_mut().unwrap().mem[5] = 0x99;
        a.eeprom.as_mut().unwrap().mem[6] = 0x77;
        a.scan_barcode(&Barcode::parse("4006381333931").unwrap());
        for _ in 0..200_000 {
            a.notify_cpu_cycle();
        }
        let blob = a.save_state();
        let mut b = datach(true);
        b.load_state(&blob).unwrap();
        assert_eq!(b.save_state(), blob);
        assert!(b.barcode_scan_in_progress());
        assert!(b.ext_scl);
        assert_eq!(b.ext_eeprom.as_ref().unwrap().mem[5], 0x99);
        assert_eq!(b.sram()[6], 0x77);
        for _ in 0..200_000 {
            assert_eq!(a.cpu_read(0x6000), b.cpu_read(0x6000));
            a.notify_cpu_cycle();
            b.notify_cpu_cycle();
        }
        assert!(b.load_state(&blob[..blob.len() - 1]).is_err());
        let mut long = blob.clone();
        long.push(0);
        assert!(b.load_state(&long).is_err());
        // A board without the external chip has a shorter tail.
        assert!(datach(false).load_state(&blob).is_err());
    }
}
