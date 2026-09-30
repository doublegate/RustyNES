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

    /// `None` for a byte [`Self::to_byte`] never writes.
    const fn from_byte(b: u8) -> Option<Self> {
        Some(match b {
            0 => Self::Idle,
            1 => Self::Unlock1,
            2 => Self::Unlock2,
            3 => Self::Program,
            4 => Self::Erase1,
            5 => Self::Erase2,
            6 => Self::Erase3,
            _ => return None,
        })
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

    /// Inverse of [`Self::to_bytes`]. `None` for bytes it never writes: a
    /// step other than 0-6, or an ID-mode flag other than 0/1. A board's
    /// `load_state` refuses those rather than normalising them
    /// (`docs/mappers.md` gotcha 12).
    pub(crate) const fn from_bytes(b: [u8; 2]) -> Option<Self> {
        let Some(step) = Step::from_byte(b[0]) else {
            return None;
        };
        let id_mode = match b[1] {
            0 => false,
            1 => true,
            _ => return None,
        };
        Some(Self { step, id_mode })
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

/// The bitmap's byte length for a `mem_len`-byte chip. The wire format has no
/// length prefix: both ends derive the bitmap from the chip size, one bit per
/// 4 KiB sector, least significant bit first.
pub(crate) const fn sector_bitmap_len(mem_len: usize) -> usize {
    mem_len.div_ceil(SECTOR).div_ceil(8)
}

/// The exact byte length of the diff at the start of `data` for a
/// `mem_len`-byte chip: the bitmap, then one sector per set bit (the last
/// sector may be short). `None` when `data` does not hold the whole bitmap,
/// or when a padding bit past the last sector is set, which
/// [`encode_sector_diff`] never writes (`docs/mappers.md` gotcha 12).
pub(crate) fn sector_diff_len(mem_len: usize, data: &[u8]) -> Option<usize> {
    let sectors = mem_len.div_ceil(SECTOR);
    let bitmap = data.get(..sector_bitmap_len(mem_len))?;
    let mut len = bitmap.len();
    for (i, &byte) in bitmap.iter().enumerate() {
        for bit in 0..8 {
            if byte & (1 << bit) == 0 {
                continue;
            }
            let s = i * 8 + bit;
            if s >= sectors {
                return None;
            }
            len += ((s + 1) * SECTOR).min(mem_len) - s * SECTOR;
        }
    }
    Some(len)
}

/// Inverse of [`encode_sector_diff`]: rebuild `mem` from `original` plus the
/// flashed sectors. Returns the number of bytes consumed, or `None` when
/// `mem` and `original` differ in length or `data` is not a whole diff
/// ([`sector_diff_len`]). Every check runs before the first write, so `None`
/// leaves `mem` exactly as it was.
pub(crate) fn decode_sector_diff(mem: &mut [u8], original: &[u8], data: &[u8]) -> Option<usize> {
    if mem.len() != original.len() {
        return None;
    }
    let total = sector_diff_len(mem.len(), data)?;
    if data.len() < total {
        return None;
    }
    let sectors = mem.len().div_ceil(SECTOR);
    let bitmap_len = sector_bitmap_len(mem.len());
    let bitmap = &data[..bitmap_len];
    let mut cur = bitmap_len;
    mem.copy_from_slice(original);
    for s in 0..sectors {
        if bitmap[s / 8] & (1 << (s % 8)) != 0 {
            let r = s * SECTOR..((s + 1) * SECTOR).min(mem.len());
            let n = r.len();
            mem[r].copy_from_slice(&data[cur..cur + n]);
            cur += n;
        }
    }
    debug_assert_eq!(cur, total);
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
        let mut g = Sst39sf040::from_bytes(f.to_bytes()).expect("valid bytes");
        assert!(g.write(&mut mem, 0x5555, 0xA0) | g.write(&mut mem, 0x10, 0x00));
        // Every byte pair `to_bytes` can write decodes, and nothing else does.
        for step in 0..=u8::MAX {
            for id in 0..=u8::MAX {
                let ok = step <= 6 && id <= 1;
                assert_eq!(
                    Sst39sf040::from_bytes([step, id]).is_some(),
                    ok,
                    "{step} {id}"
                );
            }
        }
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

    /// The diff's length is exact before anything is copied, so a refusal
    /// leaves `mem` as it was. A padding bit past the last sector is never
    /// written by `encode_sector_diff` and is refused.
    #[test]
    fn sector_diff_length_is_exact_and_refusal_writes_nothing() {
        // 20 KiB + 1: six sectors, the last one byte long; one bitmap byte
        // with two padding bits.
        let len = 5 * SECTOR + 1;
        let original = vec![0x11u8; len];
        let mut mem = original.clone();
        mem[0] = 0;
        mem[len - 1] = 0;
        let mut out = Vec::new();
        encode_sector_diff(&mem, &original, &mut out);
        assert_eq!(sector_bitmap_len(len), 1);
        assert_eq!(sector_diff_len(len, &out), Some(1 + SECTOR + 1));
        assert_eq!(out.len(), 1 + SECTOR + 1);
        assert_eq!(sector_diff_len(len, &[]), None, "no bitmap");
        for bit in [6u8, 7] {
            assert_eq!(sector_diff_len(len, &[1 << bit]), None, "padding bit {bit}");
        }
        let mut target = vec![0xAAu8; len];
        assert_eq!(
            decode_sector_diff(&mut target, &original, &out[..out.len() - 1]),
            None
        );
        assert!(target.iter().all(|&b| b == 0xAA), "nothing written");
    }
}
