// SPDX-License-Identifier: GPL-3.0-or-later
//! The SST39SF040 flash chip used as PRG-ROM on self-flashable homebrew boards
//! (v2.9.6 "Roster"): GTROM (mapper 111) and UNROM 512 (mapper 30).
//!
//! Written from the command table in Microchip's SST39SF010A/020A/040
//! datasheet, which `nesdev_wiki/output/GTROM.md` and `UNROM_512.md` both
//! point to. Commands are sequences of writes to fixed chip addresses. The chip
//! decodes address bits A14-A0 for them, so `5555h` and `2AAAh` match whatever
//! is above A14:
//!
//! | command | cycle 1 | cycle 2 | cycle 3 | cycle 4 | cycle 5 | cycle 6 |
//! |---|---|---|---|---|---|---|
//! | byte program | `5555h←AAh` | `2AAAh←55h` | `5555h←A0h` | `addr←data` | | |
//! | sector erase (4 KiB) | `5555h←AAh` | `2AAAh←55h` | `5555h←80h` | `5555h←AAh` | `2AAAh←55h` | `sector←30h` |
//! | chip erase | `5555h←AAh` | `2AAAh←55h` | `5555h←80h` | `5555h←AAh` | `2AAAh←55h` | `5555h←10h` |
//! | software ID entry | `5555h←AAh` | `2AAAh←55h` | `5555h←90h` | | | |
//! | software ID exit | `xxxxh←F0h`, or the three-cycle form ending `5555h←F0h` | | | | | |
//!
//! Programming can only clear bits: a byte program ANDs the data into the
//! cell, and only an erase sets bits back to 1. In software-ID mode, a read
//! of address 0 returns the manufacturer ID `BFh` and address 1 the device ID
//! `B7h`.
//!
//! **Timing is not modelled.** On the chip a byte program takes about 14 µs and
//! a sector erase about 18 ms. During that time reads return status (DQ7 data
//! polling, DQ6 toggling) rather than data. Here every operation completes
//! within the write, so the first status read already returns final data.
//! The completion loops the wiki's example code uses ("read until you get the
//! same value twice") exit on their first pass, as they would on a chip that
//! had just finished. A program that times the operation itself would see it
//! take no time.

/// Command sequence position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    Idle,
    /// `5555h←AAh` seen.
    Unlock1,
    /// `2AAAh←55h` seen.
    Unlock2,
    /// `5555h←A0h`: the next write is the data.
    Program,
    /// `5555h←80h` seen.
    Erase1,
    /// `5555h←AAh` seen after `80h`.
    Erase2,
    /// `2AAAh←55h` seen after that: the next write picks the erase.
    Erase3,
}

impl Step {
    const fn to_byte(self) -> u8 {
        match self {
            Self::Idle => 0,
            Self::Unlock1 => 1,
            Self::Unlock2 => 2,
            Self::Program => 3,
            Self::Erase1 => 4,
            Self::Erase2 => 5,
            Self::Erase3 => 6,
        }
    }

    const fn from_byte(b: u8) -> Self {
        match b {
            1 => Self::Unlock1,
            2 => Self::Unlock2,
            3 => Self::Program,
            4 => Self::Erase1,
            5 => Self::Erase2,
            6 => Self::Erase3,
            _ => Self::Idle,
        }
    }
}

/// Manufacturer ID (SST).
pub(crate) const MANUFACTURER_ID: u8 = 0xBF;
/// Device ID (SST39SF040).
pub(crate) const DEVICE_ID: u8 = 0xB7;
/// Sector size.
pub(crate) const SECTOR: usize = 0x1000;

/// The command state of one SST39SF040. The memory itself belongs to the
/// board (it is the PRG-ROM), and is passed to [`Self::write`].
#[derive(Debug, Clone, Copy)]
pub(crate) struct Sst39sf040 {
    step: Step,
    id_mode: bool,
}

impl Sst39sf040 {
    pub(crate) const fn new() -> Self {
        Self {
            step: Step::Idle,
            id_mode: false,
        }
    }

    /// A CPU write reaching the chip at `chip_addr` (a byte offset into
    /// `mem`). Returns `true` when it changed `mem`.
    pub(crate) fn write(&mut self, mem: &mut [u8], chip_addr: usize, value: u8) -> bool {
        let cmd = chip_addr & 0x7FFF;
        let mut changed = false;
        self.step = match (self.step, cmd, value) {
            (_, _, 0xF0) if !matches!(self.step, Step::Program) => {
                self.id_mode = false;
                Step::Idle
            }
            (Step::Idle, 0x5555, 0xAA) => Step::Unlock1,
            (Step::Unlock1, 0x2AAA, 0x55) => Step::Unlock2,
            (Step::Unlock2, 0x5555, 0xA0) => Step::Program,
            (Step::Unlock2, 0x5555, 0x80) => Step::Erase1,
            (Step::Unlock2, 0x5555, 0x90) => {
                self.id_mode = true;
                Step::Idle
            }
            (Step::Program, _, _) => {
                if let Some(cell) = mem.get_mut(chip_addr) {
                    let new = *cell & value;
                    changed = new != *cell;
                    *cell = new;
                }
                Step::Idle
            }
            (Step::Erase1, 0x5555, 0xAA) => Step::Erase2,
            (Step::Erase2, 0x2AAA, 0x55) => Step::Erase3,
            (Step::Erase3, _, 0x30) => {
                let start = chip_addr & !(SECTOR - 1);
                if let Some(sector) = mem.get_mut(start..start + SECTOR) {
                    changed = sector.iter().any(|&b| b != 0xFF);
                    sector.fill(0xFF);
                }
                Step::Idle
            }
            (Step::Erase3, 0x5555, 0x10) => {
                changed = mem.iter().any(|&b| b != 0xFF);
                mem.fill(0xFF);
                Step::Idle
            }
            _ => Step::Idle,
        };
        changed
    }

    /// The software-ID byte for a read at `chip_addr`, when ID mode is on.
    pub(crate) const fn id_read(self, chip_addr: usize) -> Option<u8> {
        if !self.id_mode {
            None
        } else if chip_addr & 1 == 0 {
            Some(MANUFACTURER_ID)
        } else {
            Some(DEVICE_ID)
        }
    }

    pub(crate) const fn to_bytes(self) -> [u8; 2] {
        [self.step.to_byte(), self.id_mode as u8]
    }

    pub(crate) const fn from_bytes(b: [u8; 2]) -> Self {
        Self {
            step: Step::from_byte(b[0]),
            id_mode: b[1] != 0,
        }
    }
}

/// The 4 KiB sectors of `mem` that differ from `original`, as a bitmap
/// followed by their contents: a save state carries only what was flashed.
pub(crate) fn encode_sector_diff(mem: &[u8], original: &[u8], out: &mut alloc::vec::Vec<u8>) {
    // Both are the same chip: the flash and the image it was loaded from.
    debug_assert_eq!(mem.len(), original.len());
    let sectors = mem.len().div_ceil(SECTOR);
    let mut bitmap = alloc::vec![0u8; sectors.div_ceil(8)];
    for s in 0..sectors {
        let r = s * SECTOR..((s + 1) * SECTOR).min(mem.len());
        if mem[r.clone()] != original[r] {
            bitmap[s / 8] |= 1 << (s % 8);
        }
    }
    out.extend_from_slice(&bitmap);
    for s in 0..sectors {
        if bitmap[s / 8] & (1 << (s % 8)) != 0 {
            out.extend_from_slice(&mem[s * SECTOR..((s + 1) * SECTOR).min(mem.len())]);
        }
    }
}

/// Inverse of [`encode_sector_diff`]: rebuild `mem` from `original` plus the
/// flashed sectors. Returns the number of bytes consumed, or `None` when
/// `data` is too short or `mem` and `original` differ in length. Callers
/// decode into a scratch buffer, so `None` leaves the board untouched.
pub(crate) fn decode_sector_diff(mem: &mut [u8], original: &[u8], data: &[u8]) -> Option<usize> {
    if mem.len() != original.len() {
        return None;
    }
    let sectors = mem.len().div_ceil(SECTOR);
    let bitmap_len = sectors.div_ceil(8);
    let bitmap = data.get(..bitmap_len)?;
    let mut cur = bitmap_len;
    mem.copy_from_slice(original);
    for s in 0..sectors {
        if bitmap[s / 8] & (1 << (s % 8)) != 0 {
            let r = s * SECTOR..((s + 1) * SECTOR).min(mem.len());
            let n = r.len();
            mem[r].copy_from_slice(data.get(cur..cur + n)?);
            cur += n;
        }
    }
    Some(cur)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::{vec, vec::Vec};

    fn cmd(f: &mut Sst39sf040, mem: &mut [u8], seq: &[(usize, u8)]) -> bool {
        let mut changed = false;
        for &(a, v) in seq {
            changed |= f.write(mem, a, v);
        }
        changed
    }

    const UNLOCK: [(usize, u8); 2] = [(0x5555, 0xAA), (0x2AAA, 0x55)];

    #[test]
    fn byte_program_ands_into_the_cell() {
        let mut f = Sst39sf040::new();
        let mut mem = vec![0xFFu8; 0x10000];
        assert!(cmd(
            &mut f,
            &mut mem,
            &[UNLOCK[0], UNLOCK[1], (0x5555, 0xA0), (0x9123, 0x5A)]
        ));
        assert_eq!(mem[0x9123], 0x5A);
        cmd(
            &mut f,
            &mut mem,
            &[UNLOCK[0], UNLOCK[1], (0x5555, 0xA0), (0x9123, 0xF0)],
        );
        assert_eq!(mem[0x9123], 0x50, "programming only clears bits");
    }

    #[test]
    fn a_write_outside_a_command_changes_nothing() {
        let mut f = Sst39sf040::new();
        let mut mem = vec![0xFFu8; 0x10000];
        assert!(!cmd(&mut f, &mut mem, &[(0x1234, 0x00)]));
        // A broken sequence resets to idle.
        assert!(!cmd(
            &mut f,
            &mut mem,
            &[UNLOCK[0], (0x2AAB, 0x55), (0x5555, 0xA0), (0x10, 0)]
        ));
        assert!(mem.iter().all(|&b| b == 0xFF));
    }

    #[test]
    fn command_addresses_decode_a14_to_a0_only() {
        let mut f = Sst39sf040::new();
        let mut mem = vec![0xFFu8; 0x80000];
        // GTROM / UNROM 512 place 5555h in a higher bank.
        let seq = [
            (0x4_D555, 0xAA),
            (0x6_2AAA, 0x55),
            (0x1_5555, 0xA0),
            (0x7_0001, 0x00),
        ];
        assert!(cmd(&mut f, &mut mem, &seq));
        assert_eq!(mem[0x7_0001], 0);
    }

    #[test]
    fn sector_erase_sets_one_4k_sector() {
        let mut f = Sst39sf040::new();
        let mut mem = vec![0u8; 0x10000];
        let seq = [
            UNLOCK[0],
            UNLOCK[1],
            (0x5555, 0x80),
            UNLOCK[0],
            UNLOCK[1],
            (0x3456, 0x30),
        ];
        assert!(cmd(&mut f, &mut mem, &seq));
        assert!(mem[0x3000..0x4000].iter().all(|&b| b == 0xFF));
        assert_eq!(mem[0x2FFF], 0);
        assert_eq!(mem[0x4000], 0);
    }

    #[test]
    fn chip_erase_and_software_id() {
        let mut f = Sst39sf040::new();
        let mut mem = vec![0u8; 0x8000];
        let erase = [
            UNLOCK[0],
            UNLOCK[1],
            (0x5555, 0x80),
            UNLOCK[0],
            UNLOCK[1],
            (0x5555, 0x10),
        ];
        assert!(cmd(&mut f, &mut mem, &erase));
        assert!(mem.iter().all(|&b| b == 0xFF));
        assert_eq!(f.id_read(0), None);
        cmd(&mut f, &mut mem, &[UNLOCK[0], UNLOCK[1], (0x5555, 0x90)]);
        assert_eq!(f.id_read(0), Some(0xBF));
        assert_eq!(f.id_read(1), Some(0xB7));
        cmd(&mut f, &mut mem, &[(0x1234, 0xF0)]);
        assert_eq!(f.id_read(0), None, "F0h anywhere exits ID mode");
    }

    #[test]
    fn state_bytes_round_trip() {
        let mut f = Sst39sf040::new();
        let mut mem = vec![0xFFu8; 0x8000];
        cmd(&mut f, &mut mem, &[UNLOCK[0], UNLOCK[1]]);
        let g = Sst39sf040::from_bytes(f.to_bytes());
        let mut g2 = g;
        assert!(g2.write(&mut mem, 0x5555, 0xA0) | g2.write(&mut mem, 0x10, 0x00));
    }

    #[test]
    fn sector_diff_carries_only_flashed_sectors() {
        let original = vec![0x11u8; 0x8000];
        let mut mem = original.clone();
        mem[0x1003] = 0;
        mem[0x7FFF] = 0;
        let mut out = Vec::new();
        encode_sector_diff(&mem, &original, &mut out);
        assert_eq!(
            out.len(),
            1 + 2 * SECTOR,
            "a one-byte bitmap and two sectors"
        );
        let mut back = vec![0u8; 0x8000];
        assert_eq!(
            decode_sector_diff(&mut back, &original, &out),
            Some(out.len())
        );
        assert_eq!(back, mem);
        assert_eq!(decode_sector_diff(&mut back, &original, &out[..10]), None);
        let mut short = vec![0u8; 0x4000];
        assert_eq!(
            decode_sector_diff(&mut short, &original, &out),
            None,
            "a length mismatch is refused, not a panic"
        );
    }
}
