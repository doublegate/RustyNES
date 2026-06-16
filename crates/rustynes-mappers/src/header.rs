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
/// The 4 boolean flags directly mirror the iNES / NES 2.0 wire format and so
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

    // Mapper assembly:
    //   bits 0..=3 from header[6] high nibble,
    //   bits 4..=7 from header[7] high nibble,
    //   bits 8..=11 from header[8] low nibble (NES 2.0 only).
    let mapper_low = u16::from((h[6] >> 4) & 0x0F);
    let mapper_mid = u16::from(h[7] & 0xF0);
    let mapper_id: u16 = if is_nes2 {
        let mapper_hi = u16::from(h[8] & 0x0F) << 8;
        mapper_low | mapper_mid | mapper_hi
    } else {
        mapper_low | mapper_mid
    };
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

/// Re-serialize a [`Header`] back to the canonical 16-byte layout.
///
/// Used by the round-trip test in T-12-002 / T-12-003.
// Serialization performs nibble extraction by mask + cast; the truncation is
// the documented encoding (not a bug), so we allow the cast lints narrowly on
// this function.
#[must_use]
#[allow(clippy::cast_possible_truncation)]
pub fn serialize_header(h: &Header) -> [u8; HEADER_LEN] {
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
    let mut flags7 = ((h.mapper_id >> 4) as u8) & 0xF0;
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
        // Byte 13: Vs. System PPU type (low nibble) when console = Vs. System.
        if h.console_type == ConsoleType::VsSystem {
            out[13] = vs_ppu_type_to_nibble(h.vs_ppu_type);
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
