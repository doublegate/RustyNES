//! `Cartridge` value type and supporting enums plus the `RomError` returned
//! by [`crate::parse`].
//!
//! The shape of the public type follows `docs/cartridge-format.md` §Public API
//! and `docs/mappers.md` §Interfaces.

use alloc::{boxed::Box, string::String};
use thiserror::Error;

/// Nametable mirroring layout selected by the cartridge wiring.
///
/// Per `docs/mappers.md` §Mirroring, this is *initial* mirroring for any
/// non-trivial mapper; mappers that expose runtime mirroring control will
/// override this from their internal state.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum Mirroring {
    /// Horizontal arrangement (vertical mirroring on the address line).
    Horizontal,
    /// Vertical arrangement (horizontal mirroring on the address line).
    Vertical,
    /// Both nametables fetch from physical bank A.
    SingleScreenA,
    /// Both nametables fetch from physical bank B.
    SingleScreenB,
    /// Four-screen mode: cartridge supplies extra 2 KiB VRAM.
    FourScreen,
    /// Mapper supplies a runtime mirroring table; defer to the mapper.
    MapperControlled,
}

impl Mirroring {
    /// Resolve a logical nametable index (0..=3, in `$2000` / `$2400` /
    /// `$2800` / `$2C00` order) to a 2 KiB-VRAM physical bank index (0 or 1)
    /// under this mirroring mode. Used by every mapper's `nametable_offset`
    /// helper to keep the mirroring table in one place.
    #[must_use]
    pub const fn physical_bank(self, logical_table: u8) -> usize {
        match self {
            // Horizontal arrangement: tables 0/1 -> bank 0, 2/3 -> bank 1.
            Self::Horizontal => (logical_table >> 1) as usize & 1,
            // Vertical arrangement: tables 0/2 -> bank 0, 1/3 -> bank 1.
            // Also the fallback for FourScreen / MapperControlled headers
            // that show up on mappers that don't actually support those.
            Self::Vertical | Self::FourScreen | Self::MapperControlled => {
                logical_table as usize & 1
            }
            // Single-screen: every logical table aliases to one physical bank.
            Self::SingleScreenA => 0,
            Self::SingleScreenB => 1,
        }
    }
}

/// Region governing CPU/PPU dividers, scanline counts, audio rate tables.
///
/// Mirrors `rustynes_core::Region` but lives in `rustynes-mappers` so the cartridge can
/// surface region from the NES 2.0 header without depending on `rustynes-core`.
/// `rustynes-core` re-exports a public alias so callers see one type.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum Region {
    /// NTSC (Japan, North America, Australia). 60 Hz, 262 scanlines.
    Ntsc,
    /// PAL (Europe). 50 Hz, 312 scanlines.
    Pal,
    /// Multi-region: cartridge runs on either NTSC or PAL hardware.
    Multi,
    /// Dendy (Russian PAL famiclone). 50 Hz, PAL pixel clock + NTSC PPU layout.
    Dendy,
}

/// Console type from NES 2.0 header byte 7 (bits 0-1).
///
/// iNES 1.0 cartridges always parse as [`ConsoleType::Nes`].
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum ConsoleType {
    /// Standard NES / Famicom.
    Nes,
    /// Nintendo Vs. System arcade hardware.
    VsSystem,
    /// Nintendo PlayChoice-10 arcade hardware.
    Playchoice10,
    /// Extended console family (see NES 2.0 byte 13 for the specific variant).
    Extended,
}

/// Hardware RGB palette selected by a Vs. System / PlayChoice-10 PPU.
///
/// This mirrors `rustynes_ppu::PpuPalette` but lives in `rustynes-mappers` so the
/// cartridge layer can surface the resolved palette from the NES 2.0 header
/// without a dependency on `rustynes-ppu` (the workspace edge is `rustynes-ppu ->
/// rustynes-mappers`, never the reverse). `rustynes-core` maps this to the PPU enum.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Default)]
pub enum VsPpuPalette {
    /// Standard 2C02 composite palette (default NES/Famicom; never a Vs. PPU).
    #[default]
    Composite2C02,
    /// 2C03 RGB PPU.
    Rgb2C03,
    /// RP2C04-0001 RGB PPU.
    Rgb2C04_0001,
    /// RP2C04-0002 RGB PPU.
    Rgb2C04_0002,
    /// RP2C04-0003 RGB PPU.
    Rgb2C04_0003,
    /// RP2C04-0004 RGB PPU.
    Rgb2C04_0004,
    /// 2C05 RGB PPU (shares the 2C03 master palette; adds register quirks).
    Rgb2C05,
}

/// Vs. System PPU type from NES 2.0 header byte 13 low nibble.
///
/// Only meaningful when the console type (byte 7) is [`ConsoleType::VsSystem`];
/// otherwise [`VsPpuType::None`]. Per nesdev "NES 2.0" §Vs. System Type and
/// "PPU registers" §2C05 identifier.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Default)]
pub enum VsPpuType {
    /// Not a Vs. System cart (the default for NES/Famicom + PlayChoice-10).
    #[default]
    None,
    /// `$0`: any RP2C03/RC2C03 variant.
    Rp2C03,
    /// `$2`: RP2C04-0001.
    Rp2C04_0001,
    /// `$3`: RP2C04-0002.
    Rp2C04_0002,
    /// `$4`: RP2C04-0003.
    Rp2C04_0003,
    /// `$5`: RP2C04-0004.
    Rp2C04_0004,
    /// `$8`: RC2C05-01 (signature unknown; the sole known game does not check
    /// `$2002`).
    Rc2C05_01,
    /// `$9`: RC2C05-02 (`$2002 AND $3F = $3D`).
    Rc2C05_02,
    /// `$A`: RC2C05-03 (`$2002 AND $1F = $1C`).
    Rc2C05_03,
    /// `$B`: RC2C05-04 (`$2002 AND $1F = $1B`).
    Rc2C05_04,
}

impl VsPpuType {
    /// Decode the NES 2.0 byte-13 low nibble into a Vs. PPU type. Reserved /
    /// unknown nibbles fall back to [`VsPpuType::Rp2C03`] (the most common RGB
    /// PPU), matching how most emulators treat unspecified Vs. carts.
    #[must_use]
    pub const fn from_byte13_low_nibble(nibble: u8) -> Self {
        match nibble & 0x0F {
            0x2 => Self::Rp2C04_0001,
            0x3 => Self::Rp2C04_0002,
            0x4 => Self::Rp2C04_0003,
            0x5 => Self::Rp2C04_0004,
            0x8 => Self::Rc2C05_01,
            0x9 => Self::Rc2C05_02,
            0xA => Self::Rc2C05_03,
            0xB => Self::Rc2C05_04,
            // $0 = any RP2C03/RC2C03 variant; $1, $6, $7, $C-$F are reserved.
            // All resolve to the 2C03 (the most common RGB PPU).
            _ => Self::Rp2C03,
        }
    }

    /// Resolve to the output palette.
    #[must_use]
    pub const fn ppu_palette(self) -> VsPpuPalette {
        match self {
            Self::None => VsPpuPalette::Composite2C02,
            Self::Rp2C03 => VsPpuPalette::Rgb2C03,
            Self::Rp2C04_0001 => VsPpuPalette::Rgb2C04_0001,
            Self::Rp2C04_0002 => VsPpuPalette::Rgb2C04_0002,
            Self::Rp2C04_0003 => VsPpuPalette::Rgb2C04_0003,
            Self::Rp2C04_0004 => VsPpuPalette::Rgb2C04_0004,
            // All 2C05 variants share the 2C03 master palette.
            Self::Rc2C05_01 | Self::Rc2C05_02 | Self::Rc2C05_03 | Self::Rc2C05_04 => {
                VsPpuPalette::Rgb2C05
            }
        }
    }

    /// True for the 2C05 series ($2000/$2001 swap + $2002 identifier).
    #[must_use]
    pub const fn is_2c05(self) -> bool {
        matches!(
            self,
            Self::Rc2C05_01 | Self::Rc2C05_02 | Self::Rc2C05_03 | Self::Rc2C05_04
        )
    }

    /// The byte returned in the low 5 bits of `$2002` on a 2C05 (0 otherwise).
    ///
    /// The 2C05-01 signature is unknown (the only known game, Ninja
    /// Jajamaru-kun, never reads it), so it returns 0.
    #[must_use]
    pub const fn ppu_2c05_id(self) -> u8 {
        match self {
            Self::Rc2C05_02 => 0x3D,
            Self::Rc2C05_03 => 0x1C,
            Self::Rc2C05_04 => 0x1B,
            // 2C05-01 signature unknown -> 0; non-2C05 -> 0.
            _ => 0x00,
        }
    }
}

/// Errors returned by [`crate::parse`].
///
/// Marked `#[non_exhaustive]` so new variants can be added without breaking
/// downstream `match` arms.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum RomError {
    /// File ended before the header / declared sections finished loading.
    #[error("rom is truncated: needed at least {needed} bytes, got {got}")]
    Truncated {
        /// Minimum number of bytes required to satisfy the header.
        needed: usize,
        /// Number of bytes actually present.
        got: usize,
    },

    /// Magic bytes did not match `"NES\x1A"`.
    #[error("rom magic bytes do not match \"NES\\x1A\"")]
    BadMagic,

    /// The file is a Famicom Disk System disk image (fwNES `"FDS\x1A"` header
    /// or a raw `"*NINTENDO-HVC*"` disk side). FDS is a separate sub-platform
    /// (disk drive + `disksys.rom` BIOS + disk IRQ/transfer timing + FDS
    /// audio) planned for v2.2.0; it is detected here so the frontend can show
    /// a clear message instead of a generic bad-magic error.
    #[error("Famicom Disk System images are not yet supported (planned for v2.2.0)")]
    FdsUnsupported,

    /// Mapper id is outside the coverage matrix for this build.
    #[error("mapper {0} is not yet implemented")]
    UnsupportedMapper(u16),

    /// The header parsed but encoded an internally inconsistent configuration
    /// (e.g., trainer flag set on a NES 2.0 ROM that has no trainer bytes).
    #[error("rom configuration is invalid: {0}")]
    InvalidConfig(String),
}

/// Concrete iNES / NES 2.0 cartridge value.
///
/// `prg_rom` and `chr_rom` are read-only ROM banks. `prg_ram_size` and
/// `chr_ram_size` are *requested* sizes; mapper implementations allocate the
/// matching RAM buffers in their constructors.
///
/// Field set follows `docs/cartridge-format.md` §Public API. `mapper` (the
/// boxed `dyn Mapper` from `docs/mappers.md`) is constructed by
/// [`crate::parse`] and stored on the cartridge separately from this metadata
/// header so the metadata is cheap to clone.
#[derive(Debug, Clone)]
pub struct Cartridge {
    /// PRG-ROM bytes. Length is a multiple of 16 KiB for standard sizes; may
    /// be irregular when the NES 2.0 exponent-multiplier encoding is used.
    pub prg_rom: Box<[u8]>,
    /// CHR-ROM bytes. Empty when the cartridge uses CHR-RAM (length 0).
    pub chr_rom: Box<[u8]>,
    /// 12-bit mapper id (iNES 1.0 only uses the low 8 bits).
    pub mapper_id: u16,
    /// 4-bit submapper id (NES 2.0 only; always 0 for iNES 1.0).
    pub submapper: u8,
    /// Initial mirroring as selected by the header.
    pub mirroring: Mirroring,
    /// Region from NES 2.0 byte 12; defaults to [`Region::Ntsc`] for iNES 1.0.
    pub region: Region,
    /// Console type from NES 2.0 byte 7; always [`ConsoleType::Nes`] for iNES 1.0.
    pub console_type: ConsoleType,
    /// Vs. System PPU type from NES 2.0 byte 13 (low nibble), valid only when
    /// `console_type == ConsoleType::VsSystem`; [`VsPpuType::None`] otherwise.
    pub vs_ppu_type: VsPpuType,
    /// Requested PRG-RAM size in bytes.
    pub prg_ram_size: u32,
    /// Requested CHR-RAM size in bytes (0 if the cart ships with CHR-ROM only).
    pub chr_ram_size: u32,
    /// True if the PRG-RAM is battery-backed (save RAM).
    pub has_battery: bool,
    /// True if a 512-byte trainer was present in the file (loaded at $7000-$71FF).
    pub has_trainer: bool,
    /// True if the file format is NES 2.0 (vs. iNES 1.0).
    pub is_nes2: bool,
}

impl Cartridge {
    /// Returns `true` when this cartridge ships PRG-ROM only (no CHR-ROM bank).
    #[must_use]
    pub fn uses_chr_ram(&self) -> bool {
        self.chr_rom.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vs_ppu_type_decodes_byte13_nibble() {
        assert_eq!(VsPpuType::from_byte13_low_nibble(0x0), VsPpuType::Rp2C03);
        assert_eq!(
            VsPpuType::from_byte13_low_nibble(0x2),
            VsPpuType::Rp2C04_0001
        );
        assert_eq!(
            VsPpuType::from_byte13_low_nibble(0x3),
            VsPpuType::Rp2C04_0002
        );
        assert_eq!(
            VsPpuType::from_byte13_low_nibble(0x4),
            VsPpuType::Rp2C04_0003
        );
        assert_eq!(
            VsPpuType::from_byte13_low_nibble(0x5),
            VsPpuType::Rp2C04_0004
        );
        assert_eq!(VsPpuType::from_byte13_low_nibble(0x8), VsPpuType::Rc2C05_01);
        assert_eq!(VsPpuType::from_byte13_low_nibble(0x9), VsPpuType::Rc2C05_02);
        assert_eq!(VsPpuType::from_byte13_low_nibble(0xA), VsPpuType::Rc2C05_03);
        assert_eq!(VsPpuType::from_byte13_low_nibble(0xB), VsPpuType::Rc2C05_04);
        // Reserved nibbles fall back to a 2C03.
        assert_eq!(VsPpuType::from_byte13_low_nibble(0x1), VsPpuType::Rp2C03);
        assert_eq!(VsPpuType::from_byte13_low_nibble(0xF), VsPpuType::Rp2C03);
    }

    #[test]
    fn vs_ppu_type_resolves_palette_and_quirks() {
        assert_eq!(VsPpuType::None.ppu_palette(), VsPpuPalette::Composite2C02);
        assert!(!VsPpuType::None.is_2c05());
        assert_eq!(VsPpuType::Rp2C03.ppu_palette(), VsPpuPalette::Rgb2C03);
        assert!(!VsPpuType::Rp2C03.is_2c05());
        assert_eq!(
            VsPpuType::Rp2C04_0002.ppu_palette(),
            VsPpuPalette::Rgb2C04_0002
        );
        // All 2C05 variants share the 2C03 palette and are 2C05.
        for t in [
            VsPpuType::Rc2C05_01,
            VsPpuType::Rc2C05_02,
            VsPpuType::Rc2C05_03,
            VsPpuType::Rc2C05_04,
        ] {
            assert_eq!(t.ppu_palette(), VsPpuPalette::Rgb2C05);
            assert!(t.is_2c05());
        }
    }

    #[test]
    fn vs_2c05_signature_ids_match_nesdev() {
        assert_eq!(VsPpuType::Rc2C05_01.ppu_2c05_id(), 0x00); // unknown
        assert_eq!(VsPpuType::Rc2C05_02.ppu_2c05_id(), 0x3D);
        assert_eq!(VsPpuType::Rc2C05_03.ppu_2c05_id(), 0x1C);
        assert_eq!(VsPpuType::Rc2C05_04.ppu_2c05_id(), 0x1B);
        // Non-2C05 PPUs report no id.
        assert_eq!(VsPpuType::Rp2C03.ppu_2c05_id(), 0x00);
        assert_eq!(VsPpuType::None.ppu_2c05_id(), 0x00);
    }
}
