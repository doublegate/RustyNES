//! Header parser shared between iNES 1.0 and NES 2.0 paths.
//!
//! Encoding rules follow `docs/cartridge-format.md` §Header layout.

use crate::cartridge::{ConsoleType, Mirroring, Region, RomError, VsPpuType};
use alloc::format;

/// Magic bytes of an iNES / NES 2.0 file: `"NES\x1A"`.
pub const MAGIC: [u8; 4] = [b'N', b'E', b'S', 0x1A];

/// Header length in bytes.
pub const HEADER_LEN: usize = 16;

/// 16 KiB PRG-ROM unit size.
pub const PRG_UNIT: usize = 16 * 1024;

/// 8 KiB CHR-ROM unit size.
pub const CHR_UNIT: usize = 8 * 1024;

/// 512-byte trainer block size (when present).
pub const TRAINER_LEN: usize = 512;

/// Parsed header view, format-detected.
///
/// The 5 boolean flags directly mirror the iNES / NES 2.0 wire format and so
/// are not refactorable into an enum without losing parser fidelity.
#[derive(Debug, Clone, Copy)]
#[allow(clippy::struct_excessive_bools)]
pub struct Header {
    /// True if the file is NES 2.0 (header byte 7 bits 2-3 == `10`).
    pub is_nes2: bool,
    /// 12-bit mapper id (iNES 1.0 fills only the low 8 bits).
    pub mapper_id: u16,
    /// 4-bit submapper id (NES 2.0 only; 0 on iNES 1.0).
    pub submapper: u8,
    /// PRG-ROM size in bytes.
    pub prg_size: usize,
    /// CHR-ROM size in bytes (0 if cart uses CHR-RAM).
    pub chr_size: usize,
    /// Effective initial mirroring.
    pub mirroring: Mirroring,
    /// Region from NES 2.0 byte 12; defaults to NTSC for iNES 1.0.
    pub region: Region,
    /// Console type from NES 2.0 byte 7; always [`ConsoleType::Nes`] for iNES 1.0.
    pub console_type: ConsoleType,
    /// Vs. System PPU type from NES 2.0 byte 13 low nibble, valid only when
    /// `console_type == ConsoleType::VsSystem` (otherwise [`VsPpuType::None`]).
    /// Resolves to the output palette + 2C05 quirks via [`VsPpuType::ppu_palette`]
    /// / [`VsPpuType::is_2c05`].
    pub vs_ppu_type: VsPpuType,
    /// True when the NES 2.0 byte-13 **high** nibble marks a Vs. `DualSystem`
    /// board (Vs. hardware types 5/6), valid only when
    /// `console_type == ConsoleType::VsSystem`. Drives DUAL-system *detection*
    /// only; the two-CPU/two-PPU emulation is a documented v2.0 deferral
    /// (`docs/audit/vs-dualsystem-design-2026-06-11.md`).
    pub vs_dual_system: bool,
    /// PRG-RAM size in bytes (NES 2.0 byte 10 low nibble; heuristic for iNES 1.0).
    pub prg_ram_size: u32,
    /// CHR-RAM size in bytes (NES 2.0 byte 11 low nibble; heuristic for iNES 1.0).
    pub chr_ram_size: u32,
    /// True when battery-backed PRG-RAM is present (`header[6]` bit 1).
    pub has_battery: bool,
    /// True when a 512-byte trainer follows the header (`header[6]` bit 2).
    pub has_trainer: bool,
    /// True when bit 3 of `header[6]` forces four-screen mode.
    pub four_screen: bool,
}

/// Assemble the mapper number from header bytes 6-8 (see the comment
/// inside for the iNES 1.0 dirty-tail rule).
fn mapper_number(h: &[u8; HEADER_LEN], is_nes2: bool) -> u16 {
    // Mapper assembly:
    //   bits 0..=3 from header[6] high nibble,
    //   bits 4..=7 from header[7] high nibble,
    //   bits 8..=11 from header[8] low nibble (NES 2.0 only).
    //
    // iNES 1.0 only: old ROM tools wrote signatures ("DiskDude!" and its
    // variants) into bytes 7-15, which the original iNES emulator ignored.
    // Byte 7's high nibble then reads as mapper bits 4-7 and adds 64 (for
    // 'D' = 0x44) to the mapper number. The NESdev "iNES" page gives the rule
    // applied here: if bytes 12-15 are not all zero and the header is not NES
    // 2.0, mask off the upper four bits of the mapper number. A clean iNES 1.0
    // header always has zeros there, so a well-formed dump of any mapper from
    // 16 to 255 is unaffected; NES 2.0 headers are exempt because bytes 12-15
    // carry real fields. Byte 7's low nibble is already ignored on the iNES
    // 1.0 path (console type and the NES 2.0 marker), so nothing else in the
    // tail is read.
    let ines1_dirty_tail = !is_nes2 && h[12..16].iter().any(|&b| b != 0);
    let mapper_low = u16::from((h[6] >> 4) & 0x0F);
    let mapper_mid = if ines1_dirty_tail {
        0
    } else {
        u16::from(h[7] & 0xF0)
    };
    if is_nes2 {
        let mapper_hi = u16::from(h[8] & 0x0F) << 8;
        mapper_low | mapper_mid | mapper_hi
    } else {
        mapper_low | mapper_mid
    }
}

/// Parse a 16-byte header into a [`Header`].
///
/// # Errors
///
/// Returns [`RomError::Truncated`] if `bytes` is < 16 bytes; [`RomError::BadMagic`]
/// if the magic does not match. Header-internal inconsistencies are returned as
/// [`RomError::InvalidConfig`].
pub fn parse_header(bytes: &[u8]) -> Result<Header, RomError> {
    if bytes.len() < HEADER_LEN {
        return Err(RomError::Truncated {
            needed: HEADER_LEN,
            got: bytes.len(),
        });
    }
    if bytes[0..4] != MAGIC {
        return Err(RomError::BadMagic);
    }

    let h: [u8; HEADER_LEN] = bytes[..HEADER_LEN].try_into().expect("checked length");
    let is_nes2 = (h[7] & 0x0C) == 0x08;

    let mapper_id = mapper_number(&h, is_nes2);
    let submapper: u8 = if is_nes2 { (h[8] >> 4) & 0x0F } else { 0 };

    // PRG / CHR sizing.
    let prg_size = if is_nes2 {
        decoded_size(h[4], u16::from(h[9] & 0x0F), PRG_UNIT)?
    } else {
        usize::from(h[4]) * PRG_UNIT
    };
    let chr_size = if is_nes2 {
        decoded_size(h[5], u16::from((h[9] >> 4) & 0x0F), CHR_UNIT)?
    } else {
        usize::from(h[5]) * CHR_UNIT
    };

    // Mirroring.
    let four_screen = (h[6] & 0x08) != 0;
    let mirroring = if four_screen {
        Mirroring::FourScreen
    } else if (h[6] & 0x01) != 0 {
        Mirroring::Vertical
    } else {
        Mirroring::Horizontal
    };

    // Region (NES 2.0 byte 12 bits 0-1).
    let region = if is_nes2 {
        match h[12] & 0x03 {
            0 => Region::Ntsc,
            1 => Region::Pal,
            2 => Region::Multi,
            3 => Region::Dendy,
            _ => unreachable!(),
        }
    } else {
        // iNES 1.0 has only the unreliable byte 9 bit 0; assume NTSC.
        Region::Ntsc
    };

    // Console type (NES 2.0 byte 7 bits 0-1).
    let console_type = if is_nes2 {
        match h[7] & 0x03 {
            0 => ConsoleType::Nes,
            1 => ConsoleType::VsSystem,
            2 => ConsoleType::Playchoice10,
            3 => ConsoleType::Extended,
            _ => unreachable!(),
        }
    } else {
        ConsoleType::Nes
    };

    // Vs. System PPU type (NES 2.0 byte 13 low nibble, only when console = Vs).
    let vs_ppu_type = if is_nes2 && console_type == ConsoleType::VsSystem {
        VsPpuType::from_byte13_low_nibble(h[13] & 0x0F)
    } else {
        VsPpuType::None
    };

    // Vs. hardware type (NES 2.0 byte 13 HIGH nibble): types 5 and 6 are the
    // Vs. DualSystem boards (two CPUs / two PPUs). Detection only — the
    // dual-console emulation is a documented v2.0 deferral.
    let vs_dual_system =
        is_nes2 && console_type == ConsoleType::VsSystem && matches!(h[13] >> 4, 5 | 6);

    // RAM sizes.
    let prg_ram_size = if is_nes2 {
        // NES 2.0 byte 10: low nibble = volatile PRG-RAM shift, high nibble =
        // non-volatile (battery) PRG-NVRAM shift. Some carts (e.g. StarTropics /
        // MMC6) declare their save RAM ONLY in the high (battery) nibble, so a
        // low-nibble-only read leaves them with zero PRG-RAM and the game reads
        // garbage from the $6000-$7FFF window. Allocate a window large enough
        // for whichever nibble is present.
        ram_size_from_shift(h[10] & 0x0F) + ram_size_from_shift(h[10] >> 4)
    } else {
        // iNES 1.0 has no reliable PRG-RAM size. We default to 8 KiB so the
        // common mappers that use save RAM (MMC1, MMC3, MMC5) get a plausible
        // window allocated. Mappers that override this on construction may.
        8 * 1024
    };
    let chr_ram_size = if is_nes2 {
        ram_size_from_shift(h[11] & 0x0F)
    } else if chr_size == 0 {
        8 * 1024
    } else {
        0
    };

    // Battery / trainer.
    let has_battery = (h[6] & 0x02) != 0;
    let has_trainer = (h[6] & 0x04) != 0;

    Ok(Header {
        is_nes2,
        mapper_id,
        submapper,
        prg_size,
        chr_size,
        mirroring,
        region,
        console_type,
        vs_ppu_type,
        vs_dual_system,
        prg_ram_size,
        chr_ram_size,
        has_battery,
        has_trainer,
        four_screen,
    })
}

/// Standard / exponent-multiplier sizing per NES 2.0.
///
/// `lsb` is header byte 4 or 5; `msb_nibble` is the matching nibble of byte 9.
fn decoded_size(lsb: u8, msb_nibble: u16, unit: usize) -> Result<usize, RomError> {
    if msb_nibble == 0x0F {
        // Exponent-multiplier: lsb = EEEEEEMM.
        let exponent = u32::from(lsb >> 2);
        if exponent >= 32 {
            return Err(RomError::InvalidConfig(format!(
                "exponent-multiplier exponent {exponent} overflows usize"
            )));
        }
        let multiplier_code = lsb & 0x03;
        let multiplier = u64::from(multiplier_code) * 2 + 1;
        let bytes = (1u64
            .checked_shl(exponent)
            .ok_or_else(|| RomError::InvalidConfig("exponent shift overflow".into()))?)
        .checked_mul(multiplier)
        .ok_or_else(|| RomError::InvalidConfig("multiplier overflow".into()))?;
        usize::try_from(bytes).map_err(|_| {
            RomError::InvalidConfig("exponent-multiplier size exceeds usize::MAX".into())
        })
    } else {
        let count = (msb_nibble << 8) | u16::from(lsb);
        let bytes = usize::from(count)
            .checked_mul(unit)
            .ok_or_else(|| RomError::InvalidConfig("rom size overflow".into()))?;
        Ok(bytes)
    }
}

/// NES 2.0 RAM-shift encoding: 0 → 0 bytes, otherwise `64 << shift`.
const fn ram_size_from_shift(shift: u8) -> u32 {
    if shift == 0 { 0 } else { 64u32 << shift }
}

/// Re-serialize a [`Header`] into the canonical 16-byte layout, from scratch.
///
/// **Lossy.** Every bit [`Header`] does not model is written as zero: the
/// Vs. hardware type (byte 13 high nibble; types 1-4 and 6 become 0 or 5), the
/// extended console type (byte 13 low nibble), bytes 14-15, the PRG-NVRAM and
/// CHR-NVRAM nibbles of bytes 10-11, the exponent-multiplier size notation,
/// and every byte past 7 of an iNES 1.0 header. Writing its output over a
/// ROM file therefore changes bytes the caller never edited. Use
/// [`serialize_header_preserving`], which starts from the file's own bytes.
#[must_use]
#[deprecated(
    since = "2.9.3",
    note = "writes every header bit `Header` does not model as zero; use `serialize_header_preserving`"
)]
pub fn serialize_header(h: &Header) -> [u8; HEADER_LEN] {
    canonical_header(h)
}

/// Write the edits in `h` over `original`, the 16 header bytes `h` was parsed
/// from, and return the result.
///
/// Only the bits of fields whose value differs from `parse_header(original)`
/// are rewritten; every other bit of `original` comes back unchanged. So an
/// unedited header round-trips byte for byte, whatever it holds -- Vs.
/// hardware types 1-4 and 6, an extended console type, bytes 14-15, NVRAM
/// nibbles, exponent-notation sizes, reserved bits, and the junk some dumpers
/// left in bytes 8-15 of iNES 1.0 headers. This is what the header editor
/// writes to disk.
///
/// An edited field is written in its canonical encoding, exactly as
/// [`serialize_header`] would write it: sizes in the standard notation, a
/// changed PRG-RAM size as a volatile shift in byte 10, and a changed
/// `vs_dual_system` as hardware type 5 (set) or 0 (cleared). Toggling
/// `is_nes2` changes what bytes 7-15 mean, so that edit re-encodes the whole
/// header canonically. So does an `original` that does not parse.
#[must_use]
pub fn serialize_header_preserving(h: &Header, original: &[u8; HEADER_LEN]) -> [u8; HEADER_LEN] {
    let Ok(base) = parse_header(original) else {
        return canonical_header(h);
    };
    if h.is_nes2 != base.is_nes2 {
        return canonical_header(h);
    }
    // Every byte the canonical encoding produces for the edited header; each
    // changed field copies only its own bits from here.
    let c = canonical_header(h);
    let mut out = *original;
    let mut take = |byte: usize, mask: u8| out[byte] = (out[byte] & !mask) | (c[byte] & mask);

    if h.mapper_id != base.mapper_id {
        take(6, 0xF0);
        take(7, 0xF0);
        if h.is_nes2 {
            take(8, 0x0F);
        }
    }
    if h.mirroring != base.mirroring {
        take(6, 0x01);
    }
    if h.has_battery != base.has_battery {
        take(6, 0x02);
    }
    if h.has_trainer != base.has_trainer {
        take(6, 0x04);
    }
    if h.four_screen != base.four_screen {
        take(6, 0x08);
    }
    if h.prg_size != base.prg_size {
        take(4, 0xFF);
        if h.is_nes2 {
            take(9, 0x0F);
        }
    }
    if h.chr_size != base.chr_size {
        take(5, 0xFF);
        if h.is_nes2 {
            take(9, 0xF0);
        }
    }
    // Bytes 7 (console bits) and 8-13 carry these fields in NES 2.0 only;
    // `parse_header` ignores them in iNES 1.0, and so does this.
    if h.is_nes2 {
        if h.submapper != base.submapper {
            take(8, 0xF0);
        }
        if h.prg_ram_size != base.prg_ram_size {
            take(10, 0xFF);
        }
        if h.chr_ram_size != base.chr_ram_size {
            take(11, 0x0F);
        }
        if h.region != base.region {
            take(12, 0x03);
        }
        if h.console_type != base.console_type {
            // Byte 13 means something else for each console type, so a
            // console change re-encodes it.
            take(7, 0x03);
            take(13, 0xFF);
        } else if h.console_type == ConsoleType::VsSystem {
            if h.vs_ppu_type != base.vs_ppu_type {
                take(13, 0x0F);
            }
            if h.vs_dual_system != base.vs_dual_system {
                take(13, 0xF0);
            }
        }
    }
    out
}

/// The canonical encoding behind [`serialize_header`] and the edited fields of
/// [`serialize_header_preserving`].
// Serialization performs nibble extraction by mask + cast; the truncation is
// the documented encoding (not a bug), so we allow the cast lints narrowly on
// this function.
#[allow(clippy::cast_possible_truncation)]
fn canonical_header(h: &Header) -> [u8; HEADER_LEN] {
    let mut out = [0u8; HEADER_LEN];
    out[0..4].copy_from_slice(&MAGIC);

    // Sizing.
    let (prg_lsb, prg_msb_nibble) = encode_size(h.prg_size, PRG_UNIT, h.is_nes2);
    let (chr_lsb, chr_msb_nibble) = encode_size(h.chr_size, CHR_UNIT, h.is_nes2);
    out[4] = prg_lsb;
    out[5] = chr_lsb;

    // Flags 6.
    let mut flags6 = ((h.mapper_id & 0x0F) as u8) << 4;
    if matches!(h.mirroring, Mirroring::Vertical) {
        flags6 |= 0x01;
    }
    if h.has_battery {
        flags6 |= 0x02;
    }
    if h.has_trainer {
        flags6 |= 0x04;
    }
    if h.four_screen {
        flags6 |= 0x08;
    }
    out[6] = flags6;

    // Flags 7.
    // Mapper bits 4-7 sit in byte 7's high nibble as-is (no shift).
    let mut flags7 = (h.mapper_id as u8) & 0xF0;
    if h.is_nes2 {
        flags7 |= 0x08;
        flags7 |= match h.console_type {
            ConsoleType::Nes => 0,
            ConsoleType::VsSystem => 1,
            ConsoleType::Playchoice10 => 2,
            ConsoleType::Extended => 3,
        };
    }
    out[7] = flags7;

    if h.is_nes2 {
        // Mapper hi nibble + submapper.
        out[8] = (((h.mapper_id >> 8) as u8) & 0x0F) | ((h.submapper & 0x0F) << 4);
        out[9] = (prg_msb_nibble & 0x0F) | ((chr_msb_nibble & 0x0F) << 4);
        out[10] = ram_shift_for(h.prg_ram_size);
        out[11] = ram_shift_for(h.chr_ram_size);
        out[12] = match h.region {
            Region::Ntsc => 0,
            Region::Pal => 1,
            Region::Multi => 2,
            Region::Dendy => 3,
        };
        // Byte 13: Vs. System PPU type (low nibble) + Vs. hardware type (high
        // nibble) when console = Vs. System. We only track the DualSystem bool,
        // so a dual board re-encodes as hardware type 5 (the bool round-trips
        // even though the original 5-vs-6 distinction is not retained).
        if h.console_type == ConsoleType::VsSystem {
            let hi = if h.vs_dual_system { 5 << 4 } else { 0 };
            out[13] = vs_ppu_type_to_nibble(h.vs_ppu_type) | hi;
        }
        // Bytes 14-15 reserved/extended; left zero.
    }

    out
}

// Truncating cast: count is masked to 8 / 4 bits before the cast.
#[allow(clippy::cast_possible_truncation)]
const fn encode_size(bytes: usize, unit: usize, is_nes2: bool) -> (u8, u8) {
    let count = bytes / unit;
    if is_nes2 {
        // Standard notation only; we do not round-trip through exponent
        // encoding (we never emit it).
        let lsb = (count & 0xFF) as u8;
        let msb = ((count >> 8) & 0x0F) as u8;
        (lsb, msb)
    } else {
        ((count & 0xFF) as u8, 0)
    }
}

/// Encode a [`VsPpuType`] back to its NES 2.0 byte-13 low nibble.
const fn vs_ppu_type_to_nibble(t: VsPpuType) -> u8 {
    match t {
        VsPpuType::None | VsPpuType::Rp2C03 => 0x0,
        VsPpuType::Rp2C04_0001 => 0x2,
        VsPpuType::Rp2C04_0002 => 0x3,
        VsPpuType::Rp2C04_0003 => 0x4,
        VsPpuType::Rp2C04_0004 => 0x5,
        VsPpuType::Rc2C05_01 => 0x8,
        VsPpuType::Rc2C05_02 => 0x9,
        VsPpuType::Rc2C05_03 => 0xA,
        VsPpuType::Rc2C05_04 => 0xB,
    }
}

const fn ram_shift_for(size: u32) -> u8 {
    if size == 0 {
        return 0;
    }
    // shift = log2(size / 64).
    let mut shift = 0u8;
    let mut v = size / 64;
    while v > 1 {
        v >>= 1;
        shift += 1;
    }
    shift & 0x0F
}

#[cfg(test)]
// The pre-v2.9.3 round-trip tests below still exercise the deprecated
// canonical `serialize_header`, which stays until v3.0.0 (ADR 0042).
#[allow(deprecated)]
mod tests {
    use super::*;

    fn ines_header(prg_16k_units: u8, chr_8k_units: u8, mapper: u8, flags6: u8) -> [u8; 16] {
        let mut h = [0u8; 16];
        h[..4].copy_from_slice(&MAGIC);
        h[4] = prg_16k_units;
        h[5] = chr_8k_units;
        h[6] = (mapper << 4) | (flags6 & 0x0F);
        h[7] = mapper & 0xF0;
        h
    }

    /// A small deterministic PRNG (xorshift64) for header sweeps, so the
    /// sampled headers are the same on every run.
    fn next(state: &mut u64) -> u8 {
        *state ^= *state << 13;
        *state ^= *state >> 7;
        *state ^= *state << 17;
        (*state >> 24).to_le_bytes()[0]
    }

    fn random_header(state: &mut u64) -> [u8; 16] {
        let mut h = [0u8; 16];
        h[..4].copy_from_slice(&MAGIC);
        for b in &mut h[4..] {
            *b = next(state);
        }
        h
    }

    #[test]
    fn preserving_round_trip_is_byte_identical_for_every_parsable_header() {
        // The header editor writes `serialize_header_preserving` output over
        // the ROM file, so an unedited header must come back exactly -- every
        // bit, modelled or not. Bytes 7 and 13 (format, console type, Vs.
        // PPU / hardware type, extended console type) are swept exhaustively
        // against sampled values of the rest; then 200,000 fully random
        // headers cover exponent sizes, NVRAM nibbles, reserved bits and iNES
        // 1.0 junk in bytes 8-15.
        let mut state = 0x9E37_79B9_7F4A_7C15;
        let mut checked = 0u32;
        let mut check = |h: &[u8; 16]| {
            if let Ok(parsed) = parse_header(h) {
                assert_eq!(&serialize_header_preserving(&parsed, h), h, "{h:02x?}");
                checked += 1;
            }
        };
        for b7 in 0..=255u8 {
            for b13 in 0..=255u8 {
                let mut h = random_header(&mut state);
                h[7] = b7;
                h[13] = b13;
                check(&h);
            }
        }
        for _ in 0..200_000 {
            check(&random_header(&mut state));
        }
        // Most random headers parse; a sweep that silently checked nothing
        // would pass, so say how much it covered.
        assert!(checked > 200_000, "only {checked} headers parsed");
    }

    #[test]
    fn preserving_keeps_byte_13_high_nibble_and_bytes_14_15() {
        // Before v2.9.3 the editor wrote the canonical encoding, which
        // collapsed the Vs. hardware type (byte 13 high nibble) to 5 or 0 --
        // losing UniSystem protection types 1-4 and the 5/6 distinction --
        // and always wrote bytes 14-15 as zero.
        let mut h = ines_header(2, 1, 0, 0);
        h[7] = 0x08 | 0x01; // NES 2.0, Vs. System
        h[14] = 0x02; // two miscellaneous ROMs
        h[15] = 0x2A; // a default expansion device
        for hw in 0..16u8 {
            h[13] = (hw << 4) | 0x2; // PPU type 2 (RP2C04-0001)
            let mut p = parse_header(&h).unwrap();
            p.has_battery = true; // an unrelated edit
            let out = serialize_header_preserving(&p, &h);
            assert_eq!(out[13], h[13], "Vs. hardware type {hw}");
            assert_eq!(out[14..], h[14..], "bytes 14-15, hw {hw}");
            assert_eq!(out[6], h[6] | 0x02);
        }
    }

    #[test]
    fn preserving_keeps_the_extended_console_type() {
        // Console type 3 (Extended): byte 13's LOW nibble is the extended
        // console type (VT01-VT32, EPSM, ...), which `Header` does not model
        // (CodeRabbit on #571).
        let mut h = ines_header(2, 1, 0, 0);
        h[7] = 0x08 | 0x03;
        for ext in 0..16u8 {
            h[13] = ext;
            let mut p = parse_header(&h).unwrap();
            p.mapper_id = 4;
            let out = serialize_header_preserving(&p, &h);
            assert_eq!(out[13], ext, "extended console type {ext}");
            assert_eq!(parse_header(&out).unwrap().mapper_id, 4);
        }
    }

    #[test]
    fn toggling_dual_system_rewrites_only_the_hardware_type() {
        let mut h = ines_header(2, 1, 0, 0);
        h[7] = 0x08 | 0x01;
        h[13] = 0x32; // UniSystem with protection type 3, PPU type 2
        let mut p = parse_header(&h).unwrap();
        assert!(!p.vs_dual_system);
        p.vs_dual_system = true; // the editor's DualSystem checkbox
        assert_eq!(serialize_header_preserving(&p, &h)[13], 0x52);
        h[13] = 0x62; // DualSystem type 6 stays 6 while the flag stays set
        let mut p = parse_header(&h).unwrap();
        assert_eq!(serialize_header_preserving(&p, &h)[13], 0x62);
        p.vs_dual_system = false;
        assert_eq!(serialize_header_preserving(&p, &h)[13], 0x02);
    }

    /// Every bit that differs between `a` and `b`, as a 16-byte mask.
    fn changed(a: &[u8; 16], b: &[u8; 16]) -> [u8; 16] {
        core::array::from_fn(|i| a[i] ^ b[i])
    }

    #[test]
    fn each_edit_rewrites_only_its_own_bits() {
        // A NES 2.0 Vs. System header with every unmodelled bit set, so a
        // stray write anywhere shows up in the changed-bit mask.
        let mut h = [0xFFu8; 16];
        h[..4].copy_from_slice(&MAGIC);
        h[4] = 0x02; // 2 x 16 KiB PRG
        h[5] = 0x01; // 1 x 8 KiB CHR
        h[6] = 0x10; // mapper 1, horizontal
        h[7] = 0xF9; // mapper 0xF1 high nibble, NES 2.0, Vs. System
        h[8] = 0x30; // submapper 3
        h[9] = 0x00; // standard size notation
        h[10] = 0x77; // 8 KiB volatile + 8 KiB NV PRG-RAM
        h[11] = 0x77; // 8 KiB CHR-RAM, 8 KiB CHR-NVRAM
        h[12] = 0xFD; // PAL, reserved bits set
        h[13] = 0x31; // hardware type 3, PPU type 1
        let base = parse_header(&h).unwrap();

        #[allow(clippy::type_complexity)]
        let edits: [(&str, fn(&mut Header), [u8; 16]); 11] = [
            (
                "mapper",
                |p| p.mapper_id = 0x2A4,
                mask(&[(6, 0xF0), (7, 0xF0), (8, 0x0F)]),
            ),
            (
                "mirroring",
                |p| p.mirroring = Mirroring::Vertical,
                mask(&[(6, 0x01)]),
            ),
            ("battery", |p| p.has_battery = true, mask(&[(6, 0x02)])),
            ("trainer", |p| p.has_trainer = true, mask(&[(6, 0x04)])),
            (
                "prg",
                |p| p.prg_size = 0x123 * PRG_UNIT,
                mask(&[(4, 0xFF), (9, 0x0F)]),
            ),
            (
                "chr",
                |p| p.chr_size = 0x201 * CHR_UNIT,
                mask(&[(5, 0xFF), (9, 0xF0)]),
            ),
            ("submapper", |p| p.submapper = 9, mask(&[(8, 0xF0)])),
            ("prg-ram", |p| p.prg_ram_size = 2048, mask(&[(10, 0xFF)])),
            ("chr-ram", |p| p.chr_ram_size = 4096, mask(&[(11, 0x0F)])),
            ("region", |p| p.region = Region::Dendy, mask(&[(12, 0x03)])),
            ("dual", |p| p.vs_dual_system = true, mask(&[(13, 0xF0)])),
        ];
        for (name, edit, allowed) in edits {
            let mut p = base;
            edit(&mut p);
            let out = serialize_header_preserving(&p, &h);
            let diff = changed(&h, &out);
            for i in 0..16 {
                assert_eq!(diff[i] & !allowed[i], 0, "{name}: byte {i} {out:02x?}");
            }
            assert_ne!(diff, [0; 16], "{name}: the edit was not written");
            // And the edit reads back.
            let back = parse_header(&out).unwrap();
            assert_eq!(serialize_header_preserving(&p, &out), out, "{name}");
            assert_eq!(back.mapper_id, p.mapper_id, "{name}");
            assert_eq!(back.prg_size, p.prg_size, "{name}");
            assert_eq!(back.chr_size, p.chr_size, "{name}");
            assert_eq!(back.region, p.region, "{name}");
            assert_eq!(back.vs_dual_system, p.vs_dual_system, "{name}");
        }
    }

    fn mask(bits: &[(usize, u8)]) -> [u8; 16] {
        let mut m = [0u8; 16];
        for &(i, b) in bits {
            m[i] |= b;
        }
        m
    }

    #[test]
    fn preserving_leaves_nes2_only_fields_alone_on_ines() {
        // iNES 1.0 bytes 8-15 are not part of the format and often hold a
        // dumper's signature; edits to NES 2.0-only fields must not touch them.
        let mut h = ines_header(2, 1, 1, 0);
        h[7] |= 0x01; // a Vs. bit iNES 1.0 parsing ignores
        h[8..].copy_from_slice(b"DiskDude");
        let mut p = parse_header(&h).unwrap();
        p.region = Region::Pal;
        p.submapper = 5;
        p.console_type = ConsoleType::Playchoice10;
        assert_eq!(serialize_header_preserving(&p, &h), h);
        p.has_battery = true;
        let out = serialize_header_preserving(&p, &h);
        assert_eq!(changed(&h, &out), mask(&[(6, 0x02)]));
    }

    #[test]
    fn a_format_change_or_unparsable_original_encodes_canonically() {
        let h = ines_header(2, 1, 1, 0);
        let mut p = parse_header(&h).unwrap();
        p.is_nes2 = true;
        assert_eq!(serialize_header_preserving(&p, &h), canonical_header(&p));
        let mut bad = h;
        bad[0] = b'X';
        let p = parse_header(&h).unwrap();
        assert_eq!(serialize_header_preserving(&p, &bad), canonical_header(&p));
    }

    #[test]
    fn canonical_encoding_round_trips_every_mapper_id() {
        // Byte 7's high nibble is mapper bits 4-7. Until v2.9.3 the encoder
        // wrote bits 8-11 there (`(mapper >> 4) & 0xF0`), so every mapper
        // from 16 up came back wrong -- mapper 66 (GxROM) as 2 -- and the
        // header editor wrote that to disk. Found by
        // `each_edit_rewrites_only_its_own_bits`.
        for is_nes2 in [false, true] {
            let top = if is_nes2 { 4095 } else { 255 };
            for id in 0..=top {
                let mut h = ines_header(2, 1, 0, 0);
                if is_nes2 {
                    h[7] = 0x08;
                }
                let mut p = parse_header(&h).unwrap();
                p.mapper_id = id;
                let back = parse_header(&canonical_header(&p)).unwrap();
                assert_eq!(back.mapper_id, id, "nes2={is_nes2}");
            }
        }
    }

    #[test]
    fn a_console_change_re_encodes_byte_13() {
        let mut h = ines_header(2, 1, 0, 0);
        h[7] = 0x08 | 0x03; // Extended
        h[13] = 0x0B; // an extended console type
        let mut p = parse_header(&h).unwrap();
        p.console_type = ConsoleType::Nes;
        let out = serialize_header_preserving(&p, &h);
        assert_eq!((out[7] & 0x03, out[13]), (0, 0));
    }

    #[test]
    fn rejects_bad_magic() {
        let mut h = ines_header(2, 1, 0, 0);
        h[0] = b'X';
        assert!(matches!(parse_header(&h), Err(RomError::BadMagic)));
    }

    #[test]
    fn truncated_header() {
        let bytes = [b'N', b'E', b'S'];
        assert!(matches!(
            parse_header(&bytes),
            Err(RomError::Truncated { needed: 16, got: 3 })
        ));
    }

    #[test]
    fn ines_basic_nrom_horizontal() {
        let h = ines_header(2, 1, 0, 0); // 32K PRG, 8K CHR, mapper 0, horizontal
        let p = parse_header(&h).unwrap();
        assert!(!p.is_nes2);
        assert_eq!(p.mapper_id, 0);
        assert_eq!(p.prg_size, 32 * 1024);
        assert_eq!(p.chr_size, 8 * 1024);
        assert_eq!(p.mirroring, Mirroring::Horizontal);
        assert!(!p.has_battery);
        assert!(!p.has_trainer);
        assert_eq!(p.region, Region::Ntsc);
    }

    #[test]
    fn ines_mapper_assembly() {
        // Mapper 1 (MMC1): low nibble 1, high nibble 0.
        let h = ines_header(1, 0, 1, 0);
        assert_eq!(parse_header(&h).unwrap().mapper_id, 1);
        // Mapper 4 (MMC3): low nibble 4, high nibble 0.
        let h = ines_header(1, 0, 4, 0);
        assert_eq!(parse_header(&h).unwrap().mapper_id, 4);
        // Mapper 0xCD: low nibble D, high nibble C.
        let h = ines_header(1, 0, 0xCD, 0);
        assert_eq!(parse_header(&h).unwrap().mapper_id, 0xCD);
    }

    #[test]
    fn nes2_vs_dualsystem_detected_from_byte13_high_nibble() {
        // NES 2.0 (h[7] bits 2-3 = 0b10) + Vs. System console (bits 0-1 = 01).
        let mut h = ines_header(2, 1, 0, 0);
        h[7] = 0x08 | 0x01;
        // byte 13: high nibble = Vs. hardware type, low nibble = Vs. PPU type.
        h[13] = 0x50; // hardware type 5 (DualSystem), 2C03 PPU
        let p = parse_header(&h).unwrap();
        assert!(p.is_nes2);
        assert_eq!(p.console_type, ConsoleType::VsSystem);
        assert!(p.vs_dual_system);
        // Hardware type 6 is also a DualSystem board.
        h[13] = 0x60;
        assert!(parse_header(&h).unwrap().vs_dual_system);
        // A normal Vs. UniSystem (hardware type 0-4) is NOT dual.
        h[13] = 0x00;
        assert!(!parse_header(&h).unwrap().vs_dual_system);
        h[13] = 0x40;
        assert!(!parse_header(&h).unwrap().vs_dual_system);
        // A non-Vs. NES 2.0 cart is never dual, even with a stray high nibble.
        h[7] = 0x08; // NES 2.0, console type = Nes
        h[13] = 0x50;
        assert!(!parse_header(&h).unwrap().vs_dual_system);
        // An iNES-1.0 cart (no NES 2.0 flag) is never dual.
        let mut h1 = ines_header(2, 1, 0, 0);
        h1[13] = 0x50;
        assert!(!parse_header(&h1).unwrap().vs_dual_system);
    }

    #[test]
    fn vs_dualsystem_round_trips_through_serialize() {
        let mut h = ines_header(2, 1, 0, 0);
        h[7] = 0x08 | 0x01; // NES 2.0 + Vs. System
        h[13] = 0x60; // DualSystem hardware type 6 + 2C03 PPU
        let parsed = parse_header(&h).unwrap();
        assert!(parsed.vs_dual_system);
        let out = serialize_header(&parsed);
        let reparsed = parse_header(&out).unwrap();
        // The bool round-trips (type 6 re-encodes as type 5, both DualSystem).
        assert!(reparsed.vs_dual_system);
        assert_eq!(reparsed.console_type, ConsoleType::VsSystem);
    }

    #[test]
    fn ines_vertical_battery_trainer() {
        let h = ines_header(2, 1, 0, 0b0111); // V mirroring + battery + trainer
        let p = parse_header(&h).unwrap();
        assert_eq!(p.mirroring, Mirroring::Vertical);
        assert!(p.has_battery);
        assert!(p.has_trainer);
    }

    #[test]
    fn ines_four_screen_overrides_mirroring_bit() {
        let h = ines_header(2, 1, 0, 0b1001);
        let p = parse_header(&h).unwrap();
        assert_eq!(p.mirroring, Mirroring::FourScreen);
        assert!(p.four_screen);
    }

    #[test]
    fn ines_chr_ram_when_chr_size_zero() {
        let h = ines_header(2, 0, 0, 0);
        let p = parse_header(&h).unwrap();
        assert_eq!(p.chr_size, 0);
        assert_eq!(p.chr_ram_size, 8 * 1024);
    }

    #[test]
    fn nes2_detection_and_extended_fields() {
        let mut h = [0u8; 16];
        h[..4].copy_from_slice(&MAGIC);
        h[4] = 1; // PRG LSB
        h[5] = 1; // CHR LSB
        h[6] = 0x10; // mapper low nibble = 1
        h[7] = 0x08; // NES 2.0 marker, console NES, mapper hi nibble 0
        h[8] = 0x21; // submapper 2, mapper hi 1
        h[9] = 0x00;
        h[10] = 0x07; // PRG RAM shift 7 -> 64<<7 = 8 KiB
        h[11] = 0x00;
        h[12] = 0x01; // PAL
        let p = parse_header(&h).unwrap();
        assert!(p.is_nes2);
        assert_eq!(p.mapper_id, 0x101); // bits: low=1, hi=1<<8
        assert_eq!(p.submapper, 2);
        assert_eq!(p.prg_ram_size, 8 * 1024);
        assert_eq!(p.region, Region::Pal);
        assert_eq!(p.console_type, ConsoleType::Nes);
    }

    #[test]
    fn nes2_exponent_multiplier_sizing() {
        let mut h = [0u8; 16];
        h[..4].copy_from_slice(&MAGIC);
        // exponent = 16, multiplier = 1: 2^16 = 65536 bytes
        h[4] = 16 << 2;
        h[5] = 0;
        h[6] = 0;
        h[7] = 0x08; // NES 2.0
        h[8] = 0;
        h[9] = 0x0F; // PRG MSB nibble = $F (exponent path)
        let p = parse_header(&h).unwrap();
        assert_eq!(p.prg_size, 65536);
    }

    #[test]
    fn round_trip_ines_header() {
        let h = ines_header(2, 1, 4, 0b0011); // mapper 4, V + battery
        let parsed = parse_header(&h).unwrap();
        let again = serialize_header(&parsed);
        assert_eq!(&h[0..8], &again[0..8]);
    }

    #[test]
    fn round_trip_nes2_header() {
        let mut h = [0u8; 16];
        h[..4].copy_from_slice(&MAGIC);
        h[4] = 2;
        h[5] = 1;
        h[6] = 0x41; // mapper low 4, vertical
        h[7] = 0x08; // NES 2.0, console NES, mapper mid 0
        h[8] = 0x10; // submapper 1
        h[9] = 0x00;
        h[10] = 0x07;
        h[11] = 0x00;
        h[12] = 0x01;
        let parsed = parse_header(&h).unwrap();
        let again = serialize_header(&parsed);
        assert_eq!(&h[..13], &again[..13]);
    }

    #[test]
    fn nes_cart_has_no_vs_ppu_type() {
        // A standard NES 2.0 cart (console type Nes) parses to VsPpuType::None.
        let mut h = [0u8; 16];
        h[..4].copy_from_slice(&MAGIC);
        h[4] = 1;
        h[5] = 1;
        h[7] = 0x08; // NES 2.0, console = Nes
        h[13] = 0x09; // would be 2C05-02 IF this were a Vs. cart
        let p = parse_header(&h).unwrap();
        assert_eq!(p.console_type, ConsoleType::Nes);
        assert_eq!(p.vs_ppu_type, VsPpuType::None);
    }

    #[test]
    fn vs_byte13_parses_ppu_type() {
        // Console type Vs. System (byte 7 bits 0-1 = 1) + byte 13 low nibble.
        let mk = |nibble: u8| {
            let mut h = [0u8; 16];
            h[..4].copy_from_slice(&MAGIC);
            h[4] = 1;
            h[5] = 1;
            h[7] = 0x09; // NES 2.0 (bits 2-3 = 10) + console Vs (bits 0-1 = 01)
            h[13] = nibble;
            parse_header(&h).unwrap()
        };
        assert_eq!(mk(0x0).vs_ppu_type, VsPpuType::Rp2C03);
        assert_eq!(mk(0x2).vs_ppu_type, VsPpuType::Rp2C04_0001);
        assert_eq!(mk(0x5).vs_ppu_type, VsPpuType::Rp2C04_0004);
        assert_eq!(mk(0x9).vs_ppu_type, VsPpuType::Rc2C05_02);
        // The 2C05-02 resolves to the 2C03 palette + 2C05 quirks + $3D id.
        let t = mk(0x9).vs_ppu_type;
        assert_eq!(t.ppu_palette(), crate::cartridge::VsPpuPalette::Rgb2C05);
        assert!(t.is_2c05());
        assert_eq!(t.ppu_2c05_id(), 0x3D);
        // High nibble (Vs. hardware type) does not change the PPU type.
        let mut h = [0u8; 16];
        h[..4].copy_from_slice(&MAGIC);
        h[7] = 0x09;
        h[13] = 0x52; // hw type 5 (Dual System), PPU type 2 = 2C04-0001
        assert_eq!(
            parse_header(&h).unwrap().vs_ppu_type,
            VsPpuType::Rp2C04_0001
        );
    }

    #[test]
    fn vs_byte13_round_trips() {
        let mut h = [0u8; 16];
        h[..4].copy_from_slice(&MAGIC);
        h[4] = 1;
        h[5] = 1;
        h[7] = 0x09; // NES 2.0 + Vs. System
        h[13] = 0x0A; // 2C05-03
        let parsed = parse_header(&h).unwrap();
        assert_eq!(parsed.vs_ppu_type, VsPpuType::Rc2C05_03);
        let again = serialize_header(&parsed);
        assert_eq!(again[13] & 0x0F, 0x0A);
    }

    #[test]
    fn ines1_garbage_tail_masks_the_mapper_high_nibble() {
        // NESdev "iNES", Flags 7-15: old tools wrote signatures such as
        // "DiskDude!" into bytes 7-15, which adds 64 to the mapper number,
        // and "if the last 4 bytes are not all zero, and the header is not
        // marked for NES 2.0 format, an emulator should either mask off the
        // upper 4 bits of the mapper number or simply refuse to load the ROM."
        //
        // The staged Russian Balloon Fight translation carries "@iskDude!"
        // (byte 7 = 0x40): an NROM image that loaded as mapper 64 and filled
        // the sky with RAMBO-1-banked tiles.
        let mut h = ines_header(1, 1, 0, 0x01);
        h[7..16].copy_from_slice(b"@iskDude!");
        assert_eq!(parse_header(&h).unwrap().mapper_id, 0);
        // The textbook "DiskDude!" form on an MMC1 image: 65 -> 1.
        let mut h = ines_header(8, 16, 1, 0);
        h[7..16].copy_from_slice(b"DiskDude!");
        assert_eq!(parse_header(&h).unwrap().mapper_id, 1);
        // A clean iNES 1.0 tail keeps the full 8-bit mapper number...
        let h = ines_header(8, 8, 64, 0);
        assert_eq!(parse_header(&h).unwrap().mapper_id, 64);
        // ...and garbage confined to bytes 8-11 does not trigger the rule,
        // which keys on bytes 12-15 only.
        let mut h = ines_header(8, 8, 64, 0);
        h[8..12].copy_from_slice(b"junk");
        assert_eq!(parse_header(&h).unwrap().mapper_id, 64);
        // NES 2.0 headers are exempt: bytes 12-15 are real fields there.
        let mut h = ines_header(8, 8, 64, 0);
        h[7] |= 0x08;
        h[12] = 0x01; // PAL
        h[15] = 0x01; // default expansion device
        assert_eq!(parse_header(&h).unwrap().mapper_id, 64);
    }

    #[test]
    fn ram_shift_helper_zero_returns_zero() {
        assert_eq!(ram_size_from_shift(0), 0);
    }

    #[test]
    fn ram_shift_helper_round_trip() {
        // shift=7 => 8 KiB
        assert_eq!(ram_size_from_shift(7), 8192);
        assert_eq!(ram_shift_for(8192), 7);
        // shift=10 => 64 KiB
        assert_eq!(ram_size_from_shift(10), 65536);
        assert_eq!(ram_shift_for(65536), 10);
    }
}
