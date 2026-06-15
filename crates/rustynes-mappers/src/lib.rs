//! Cartridge file format (iNES + NES 2.0) parsing and mapper implementations.
//!
//! See `docs/mappers.md` and `docs/cartridge-format.md` for the implementation
//! specs, and `ref-docs/research-report.md` §Cartridge for the source material.
//!
//! Supports a broad set of mapper families covering the great majority of the
//! licensed NES library: `NROM`, `MMC1`, `MMC2`/`MMC4`, `MMC3` (Sharp default,
//! NEC submapper), `MMC5` (with vertical split-screen, `ExGrafix`, 4-byte fill
//! mode, dual sprite/BG CHR for 8×16 sprites), `UxROM`, `CNROM`, `AxROM`,
//! `GxROM`, Color Dreams, `CPROM`, `BNROM`/`NINA` (mapper 34 variants),
//! Camerica `BF9093`, `VRC1`, `VRC2`/`VRC4` (shared superset,
//! submapper-dispatched), `VRC3`, `VRC6`, `VRC7` (banking and IRQ; FM audio
//! deferred per `docs/adr/0004-vrc7-audio-deferred.md`), Sunsoft `FME-7`,
//! Namco 163, the Bandai discrete and FCG family, Jaleco SS88006, plus the
//! v2.1.0 Tier 2 boards Tengen RAMBO-1 (64), Irem H3001 (65), Sunsoft-3 (67),
//! Sunsoft-4 (68, CHR-ROM nametables), Holy Diver / Cosmo Carrier (78),
//! TxSROM/TLSROM (118, per-bank nametable mirroring), and Namco 175/340 (210),
//! and the v2.6.0 boards including the Nintendo Vs. System (99), the Taito
//! X1-005 (80, on-cart battery RAM) / X1-017 (82, CHR A12-inversion mode), and
//! Konami VS / VRC1-on-Vs. (151).

#![no_std]
#![warn(missing_docs)]

extern crate alloc;

#[cfg(test)]
extern crate std;

use alloc::{boxed::Box, string::ToString};

mod axrom;
mod bandai152;
mod bandai74;
mod bandai_fcg;
mod cartridge;
mod cnrom;
mod fds;
mod gxrom;
mod header;
mod irem_g101;
mod irem_h3001;
mod jaleco87;
mod jaleco_ss88006;
mod konami_vs;
mod m78;
mod mapper;
mod mmc1;
mod mmc3;
mod mmc5;
mod namco118;
mod namco175;
mod nrom;
mod nsf;
mod rambo1;
mod sprint2;
mod sprint3;
mod sprint5;
mod sunsoft1;
mod sunsoft2;
mod sunsoft3;
mod sunsoft3r;
mod sunsoft4;
mod taito_tc0190;
mod taito_tc0690;
mod taito_x1_005;
mod taito_x1_017;
mod tier;
mod tqrom;
mod txsrom;
mod uxrom;
mod vrc3;
mod vs_system;

pub use axrom::AxRom;
pub use bandai152::Bandai152;
pub use bandai74::Bandai74;
pub use bandai_fcg::{BandaiFcg, FcgVariant};
pub use cartridge::{Cartridge, ConsoleType, Mirroring, Region, RomError, VsPpuPalette, VsPpuType};
pub use cnrom::CnRom;
pub use fds::{parse_fds, Fds, FdsDisk, FdsTraceRec, DISK_BYTE_CYCLES, FDS_SIDE_LEN};
pub use gxrom::GxRom;
pub use header::{parse_header, serialize_header, Header};
pub use irem_g101::IremG101;
pub use irem_h3001::IremH3001;
pub use jaleco87::Jaleco87;
pub use jaleco_ss88006::JalecoSs88006;
pub use konami_vs::KonamiVs;
pub use m78::{M78Variant, M78};
pub use mapper::{
    mirroring_name, BgSplitState, ExAttribute, Mapper, MapperCaps, MapperDebugInfo, MapperError,
    MapperFrameEvents,
};
pub use mmc1::Mmc1;
pub use mmc3::{Mmc3, Mmc3Revision};
pub use mmc5::Mmc5;
pub use namco118::{Namco118, Namco118Board};
pub use namco175::{Namco175, Namco175Board};
pub use nrom::Nrom;
pub use nsf::{is_nsf, parse_nsf, Nsf, NsfMapper};
pub use rambo1::Rambo1;
pub use sprint2::{Camerica, ColorDreams, Cprom, M34Variant, Mmc2, Mmc4, Vrc1, M34};
pub use sprint3::{Fme7, Namco163, Vrc2, Vrc4, Vrc6, Vrc7};
pub use sprint5::{
    Bitcorp38, Bxrom241, Caltron41, Camerica232, Cne240, Jaleco140, Jaleco86, Nina006M113, Nina0379,
};
pub use sunsoft1::Sunsoft1;
pub use sunsoft2::Sunsoft2;
pub use sunsoft3::Sunsoft3;
pub use sunsoft3r::Sunsoft3r;
pub use sunsoft4::Sunsoft4;
pub use taito_tc0190::TaitoTc0190;
pub use taito_tc0690::TaitoTc0690;
pub use taito_x1_005::TaitoX1005;
pub use taito_x1_017::TaitoX1017;
pub use tier::{mapper_tier, MapperTier};
pub use tqrom::Tqrom;
pub use txsrom::TxSrom;
pub use uxrom::UxRom;
pub use vrc3::Vrc3;
pub use vs_system::VsSystem;

/// Returns the crate version string.
#[must_use]
pub const fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Parse an iNES 1.0 / NES 2.0 ROM file.
///
/// Validates the magic, detects the format, applies the appropriate sizing
/// rules, slices out the trainer (if any), PRG-ROM, and CHR-ROM, and returns
/// a fully populated [`Cartridge`] plus a constructed `dyn Mapper` ready for
/// the bus to consume.
///
/// # Errors
///
/// - [`RomError::Truncated`] if the file is shorter than the declared sections.
/// - [`RomError::BadMagic`] if the first 4 bytes are not `"NES\x1A"`.
/// - [`RomError::UnsupportedMapper`] if the mapper id is outside the supported
///   set listed at crate level.
/// - [`RomError::InvalidConfig`] for inconsistent header fields (e.g.,
///   exponent overflow).
#[allow(clippy::too_many_lines)]
pub fn parse(bytes: &[u8]) -> Result<(Cartridge, Box<dyn Mapper>), RomError> {
    // Famicom Disk System: a `.fds` disk image carries no embedded BIOS, so it
    // cannot be constructed through the ordinary cartridge path (which has no
    // `disksys.rom` to hand it). The real disk path is `crate::fds::parse_fds`
    // plus `rustynes_core::Nes::from_disk(disk_bytes, bios_bytes)`. Detect the two
    // on-disk forms here — the fwNES container (`"FDS\x1A"` magic) and a raw
    // 65500-byte disk side whose first block opens with `\x01*NINTENDO-HVC*` —
    // and return [`RomError::FdsUnsupported`] so a frontend that has not yet
    // wired `from_disk` (no BIOS prompt) gets a clear message instead of a
    // generic bad-magic error. FDS Stage 1 (drive + BIOS + IRQ/timing) landed
    // in v2.2.0; audio + write + persistence are Stage 2. See `fds.rs`.
    if bytes.len() >= 4 && &bytes[0..4] == b"FDS\x1A" {
        return Err(RomError::FdsUnsupported);
    }
    if bytes.len() >= 15 && bytes[0] == 0x01 && &bytes[1..15] == b"*NINTENDO-HVC*" {
        return Err(RomError::FdsUnsupported);
    }

    // NSF music files (`"NESM\x1A"`) have no iNES header and no PPU program;
    // they run through the dedicated `rustynes_core::Nes::from_nsf` path (which
    // builds an `NsfMapper`), not this cartridge parser. Detect the magic here
    // and return a clear message so a frontend that hands NSF bytes to `parse`
    // by mistake gets a routing hint instead of a generic bad-magic error.
    if nsf::is_nsf(bytes) {
        return Err(RomError::InvalidConfig(
            "NSF music file — load via Nes::from_nsf, not parse()".into(),
        ));
    }

    let h = parse_header(bytes)?;

    let mut cursor = header::HEADER_LEN;
    if h.has_trainer {
        if bytes.len() < cursor + header::TRAINER_LEN {
            return Err(RomError::Truncated {
                needed: cursor + header::TRAINER_LEN,
                got: bytes.len(),
            });
        }
        cursor += header::TRAINER_LEN;
    }

    if bytes.len() < cursor + h.prg_size {
        return Err(RomError::Truncated {
            needed: cursor + h.prg_size,
            got: bytes.len(),
        });
    }
    let prg_rom: Box<[u8]> = bytes[cursor..cursor + h.prg_size]
        .to_vec()
        .into_boxed_slice();
    cursor += h.prg_size;

    if bytes.len() < cursor + h.chr_size {
        return Err(RomError::Truncated {
            needed: cursor + h.chr_size,
            got: bytes.len(),
        });
    }
    let chr_rom: Box<[u8]> = bytes[cursor..cursor + h.chr_size]
        .to_vec()
        .into_boxed_slice();

    // Tail bytes beyond declared PRG+CHR are ignored for iNES 1.0; in NES 2.0
    // they are the optional misc-ROM block which we do not surface yet.

    // Arcade-platform (Vs. System / PlayChoice-10) detection drives the RGB
    // PPU palette. Two signals, applied in order:
    //
    // 1. **Mapper-driven** (the most robust): mapper 99 (Vs. DualSystem CHR
    //    bank) and mapper 151 (Konami VRC1 on a Vs. board) are Vs.-only — no
    //    licensed home game uses either — so a cart bearing one is forced to
    //    `ConsoleType::VsSystem` + the 2C03 RGB PPU (the most common Vs. PPU)
    //    whenever the header did not already carry a resolved Vs. PPU type.
    //
    // 2. **Clean-byte-7 arcade flag** (real No-Intro arcade dumps): a genuine
    //    Vs./PC10 dump is clean iNES 1.0 with byte 7 EXACTLY `0x01` (Vs.) or
    //    `0x02` (PC10) — NOT NES 2.0, mapper-hi-nibble 0. We accept ONLY those
    //    two exact values on a non-NES-2.0 header. This is the critical guard:
    //    the notorious corruption is byte 7 == `0x0A` (console field = 2,
    //    PlayChoice-10, PLUS the NES-2.0 marker bits 2-3 = `10`), carried by
    //    many home dumps (e.g. the committed `Excitebike.nes`). Because `0x0A`
    //    is NES 2.0 AND is neither `0x01` nor `0x02`, it is ignored and the
    //    cart stays whatever `h.console_type` parsed (it never reaches the RGB
    //    palette: a corrupted console-2 home dump resolves to
    //    `VsPpuType::None` -> `Composite2C02`, byte-for-byte the legacy path).
    //    A survey confirmed no oracle/home ROM carries a clean `0x01`/`0x02`.
    //    Both Vs. AND PC10 use the 2C03 RGB PPU.
    //
    // A true NES-2.0 Vs. dump (console field = 1) keeps the existing behaviour:
    // `h.console_type == VsSystem` already resolved `h.vs_ppu_type` from byte 13
    // in the header parser, so it falls through the final `else`. We do NOT
    // special-case an NES-2.0 PlayChoice-10 console field, precisely because
    // that field is the corrupted-home-dump signal in this library.
    let clean_ines = !h.is_nes2;
    let (console_type, vs_ppu_type) = if h.mapper_id == 99 || h.mapper_id == 151 {
        let vs = if h.vs_ppu_type == VsPpuType::None {
            VsPpuType::Rp2C03
        } else {
            h.vs_ppu_type
        };
        (ConsoleType::VsSystem, vs)
    } else if clean_ines && bytes[7] == 0x01 {
        // Clean iNES Vs. System arcade dump.
        (ConsoleType::VsSystem, VsPpuType::Rp2C03)
    } else if clean_ines && bytes[7] == 0x02 {
        // Clean iNES PlayChoice-10 arcade dump. PC10 used the 2C03 RGB PPU;
        // route it through the 2C03 palette by resolving its (otherwise-`None`)
        // Vs. PPU type to the 2C03.
        (ConsoleType::Playchoice10, VsPpuType::Rp2C03)
    } else {
        (h.console_type, h.vs_ppu_type)
    };

    let cart = Cartridge {
        prg_rom: prg_rom.clone(),
        chr_rom: chr_rom.clone(),
        mapper_id: h.mapper_id,
        submapper: h.submapper,
        mirroring: h.mirroring,
        region: h.region,
        console_type,
        vs_ppu_type,
        prg_ram_size: h.prg_ram_size,
        chr_ram_size: h.chr_ram_size,
        has_battery: h.has_battery,
        has_trainer: h.has_trainer,
        is_nes2: h.is_nes2,
    };

    let mapper: Box<dyn Mapper> = match h.mapper_id {
        0 => {
            let nrom = Nrom::new(prg_rom, chr_rom, h.mirroring)
                .map_err(|e| RomError::InvalidConfig(e.to_string()))?;
            Box::new(nrom)
        }
        1 => {
            // MMC1. Default revision is Sharp (no NES 2.0 submapper). Submapper
            // values (1-5) signal SUROM / SOROM / SXROM / SEROM variants —
            // observationally equivalent at the register-protocol level.
            let prg_ram_bytes = if h.prg_ram_size == 0 {
                0
            } else {
                h.prg_ram_size as usize
            };
            let mmc1 = Mmc1::new(prg_rom, chr_rom, h.mirroring, prg_ram_bytes)
                .map_err(|e| RomError::InvalidConfig(e.to_string()))?;
            Box::new(mmc1)
        }
        2 => {
            let uxrom = UxRom::new(prg_rom, chr_rom, h.mirroring)
                .map_err(|e| RomError::InvalidConfig(e.to_string()))?;
            Box::new(uxrom)
        }
        3 => {
            let cnrom = CnRom::new(prg_rom, chr_rom, h.mirroring)
                .map_err(|e| RomError::InvalidConfig(e.to_string()))?;
            Box::new(cnrom)
        }
        4 => {
            // MMC3 (and MMC6 — Star Tropics — falls under the same iNES
            // mapper number with submapper 1).  Default revision is Sharp
            // (project policy: Star Trek 25th Anniversary requires Sharp
            // behavior).  NES 2.0 submapper byte:
            //   0 — MMC3A (Sharp; default).
            //   1 — MMC3B (NEC; "reload to 0" does NOT assert).
            //   2 — MMC3C (Sharp + minor differences not currently modelled).
            //   3 — MC-ACC (clone; treat as Sharp).
            let revision = if h.is_nes2 && h.submapper == 1 {
                Mmc3Revision::Nec
            } else {
                Mmc3Revision::Sharp
            };
            let prg_ram_bytes = if h.prg_ram_size == 0 {
                0
            } else {
                h.prg_ram_size as usize
            };
            let mmc3 = Mmc3::new(prg_rom, chr_rom, h.mirroring, prg_ram_bytes, revision)
                .map_err(|e| RomError::InvalidConfig(e.to_string()))?;
            Box::new(mmc3)
        }
        5 => {
            // MMC5 v0: banking + ExRAM modes 10/11 + scanline IRQ. Several
            // features deferred (vertical split, dual sprite/BG CHR for
            // 8x16 sprites, ExGrafix attribute injection, audio extension);
            // see `crates/rustynes-mappers/src/mmc5.rs` module docs.
            let prg_ram_bytes = if h.prg_ram_size == 0 {
                0
            } else {
                h.prg_ram_size as usize
            };
            let mmc5 = Mmc5::new(prg_rom, chr_rom, h.mirroring, prg_ram_bytes)
                .map_err(|e| RomError::InvalidConfig(e.to_string()))?;
            Box::new(mmc5)
        }
        7 => {
            let axrom =
                AxRom::new(prg_rom, chr_rom).map_err(|e| RomError::InvalidConfig(e.to_string()))?;
            Box::new(axrom)
        }
        9 => {
            let mmc2 = Mmc2::new(prg_rom, chr_rom, h.mirroring)
                .map_err(|e| RomError::InvalidConfig(e.to_string()))?;
            Box::new(mmc2)
        }
        10 => {
            let mmc4 = Mmc4::new(prg_rom, chr_rom, h.mirroring)
                .map_err(|e| RomError::InvalidConfig(e.to_string()))?;
            Box::new(mmc4)
        }
        11 => {
            let cd = ColorDreams::new(prg_rom, chr_rom, h.mirroring)
                .map_err(|e| RomError::InvalidConfig(e.to_string()))?;
            Box::new(cd)
        }
        13 => {
            let cprom = Cprom::new(prg_rom, h.mirroring)
                .map_err(|e| RomError::InvalidConfig(e.to_string()))?;
            Box::new(cprom)
        }
        // VRC2 / VRC4 share iNES mapper IDs across submapper variants.
        // We dispatch by (mapper, submapper) pair.  Default mapping below
        // is conservative; refer to nesdev wiki for the full table.
        21 | 23 | 25 => {
            // Mapper 21 -> VRC4 only (a/c).
            // Mapper 22 -> VRC2a (no submapper).
            // Mapper 23 -> VRC2b/VRC4e/VRC4f.
            // Mapper 25 -> VRC2c/VRC4b/VRC4d.
            // We model VRC4 with CPU-cycle IRQ as the superset; submapper
            // selects pin-decoder variant.  Banking matches between VRC2
            // and VRC4; the difference is mostly the IRQ counter which
            // VRC2 lacks (we just leave it idle).
            let vrc4 = Vrc4::new(prg_rom, chr_rom, h.mapper_id, h.submapper, h.mirroring)
                .map_err(|e| RomError::InvalidConfig(e.to_string()))?;
            Box::new(vrc4)
        }
        22 => {
            let vrc2 = Vrc2::new(prg_rom, chr_rom, 22, h.submapper, h.mirroring)
                .map_err(|e| RomError::InvalidConfig(e.to_string()))?;
            Box::new(vrc2)
        }
        24 | 26 => {
            let vrc6 = Vrc6::new(prg_rom, chr_rom, h.mapper_id, h.mirroring)
                .map_err(|e| RomError::InvalidConfig(e.to_string()))?;
            Box::new(vrc6)
        }
        34 => {
            let variant = if h.is_nes2 && h.submapper == 1 {
                M34Variant::Nina001
            } else {
                M34Variant::Bnrom
            };
            let m34 = M34::new(prg_rom, chr_rom, h.mirroring, variant)
                .map_err(|e| RomError::InvalidConfig(e.to_string()))?;
            Box::new(m34)
        }
        66 => {
            let gxrom = GxRom::new(prg_rom, chr_rom, h.mirroring)
                .map_err(|e| RomError::InvalidConfig(e.to_string()))?;
            Box::new(gxrom)
        }
        69 => {
            let fme7 = Fme7::new(prg_rom, chr_rom, h.mirroring)
                .map_err(|e| RomError::InvalidConfig(e.to_string()))?;
            Box::new(fme7)
        }
        71 => {
            // Some Camerica boards (BF9097) have $9000 mirroring control
            // (subm 1).  Default off.
            let has_single_screen = h.is_nes2 && h.submapper == 1;
            let cam = Camerica::new(prg_rom, h.mirroring, has_single_screen)
                .map_err(|e| RomError::InvalidConfig(e.to_string()))?;
            Box::new(cam)
        }
        75 => {
            let vrc1 = Vrc1::new(prg_rom, chr_rom, h.mirroring)
                .map_err(|e| RomError::InvalidConfig(e.to_string()))?;
            Box::new(vrc1)
        }
        19 => {
            let n163 = Namco163::new(prg_rom, chr_rom, h.mirroring)
                .map_err(|e| RomError::InvalidConfig(e.to_string()))?;
            Box::new(n163)
        }
        16 => {
            // Bandai FCG family. Submapper selects the register window +
            // counter latching + EEPROM (nesdev INES_Mapper_016 §submappers):
            //   0 — unspecified (emulate both windows; LZ93D50 behaviour).
            //   4 — FCG-1/2 ($6000-$7FFF window, direct counter, no EEPROM).
            //   5 — LZ93D50 ($8000-$FFFF window, latched counter, 24C02).
            // Submappers 1/2/3 are deprecated aliases of mappers 159/157/153;
            // a mapper-16 cart carrying them is treated as the closest FCG
            // behaviour (LZ93D50 + 24C02) here.
            let variant = match h.submapper {
                4 => FcgVariant::Fcg,
                5 => FcgVariant::Lz93d50_24c02,
                _ => FcgVariant::Both,
            };
            let fcg = BandaiFcg::new(prg_rom, chr_rom, h.mirroring, variant)
                .map_err(|e| RomError::InvalidConfig(e.to_string()))?;
            Box::new(fcg)
        }
        159 => {
            // Bandai LZ93D50 with a 128-byte X24C01 serial EEPROM.
            let fcg = BandaiFcg::new(prg_rom, chr_rom, h.mirroring, FcgVariant::Lz93d50_24c01)
                .map_err(|e| RomError::InvalidConfig(e.to_string()))?;
            Box::new(fcg)
        }
        18 => {
            // Jaleco SS88006 (Ganbare Goemon Gaiden, Magical Kids Doropie).
            let ss = JalecoSs88006::new(prg_rom, chr_rom, h.mirroring)
                .map_err(|e| RomError::InvalidConfig(e.to_string()))?;
            Box::new(ss)
        }
        32 => {
            // Irem G-101 (Image Fight, Major League, Kaiketsu Yancha Maru 2,
            // Magical Pop's): two switchable 8 KiB PRG banks with a software
            // swap-mode bit, eight 1 KiB CHR banks, software H/V mirroring.
            // Submapper 1 (Major League) hard-wires single-screen A and ignores
            // the $9000 mirroring bit. Many dumps omit the submapper byte; the
            // $9000 mirroring control still works for the other titles, so we
            // only force one-screen on an explicit submapper-1 flag.
            let force_one_screen = h.is_nes2 && h.submapper == 1;
            let m32 = IremG101::new(prg_rom, chr_rom, h.mirroring, force_one_screen)
                .map_err(|e| RomError::InvalidConfig(e.to_string()))?;
            Box::new(m32)
        }
        33 => {
            // Taito TC0190 / TC0350 (Don Doko Don, Power Blazer): two
            // switchable 8 KiB PRG banks, 2x2 KiB + 4x1 KiB CHR banks, and a
            // software mirroring bit. No IRQ (that is the very-similar
            // mapper 48 / TC0690).
            let m33 = TaitoTc0190::new(prg_rom, chr_rom, h.mirroring)
                .map_err(|e| RomError::InvalidConfig(e.to_string()))?;
            Box::new(m33)
        }
        48 => {
            // Taito TC0690 (Don Doko Don 2, Flintstones 2, Jetsons, Bakushou!!
            // Jinsei Gekijou 3): TC0190 banking plus an MMC3-style A12 scanline
            // IRQ and an $E000 mirroring register.
            let m48 = TaitoTc0690::new(prg_rom, chr_rom, h.mirroring)
                .map_err(|e| RomError::InvalidConfig(e.to_string()))?;
            Box::new(m48)
        }
        70 => {
            // Bandai discrete (UxROM-like): PRG bits 4-7, CHR bits 0-3.
            let m70 = Bandai74::new(prg_rom, chr_rom, h.mirroring)
                .map_err(|e| RomError::InvalidConfig(e.to_string()))?;
            Box::new(m70)
        }
        87 => {
            // Jaleco/Konami CNROM-style (Argus, Choplifter, The Goonies, City
            // Connection): fixed PRG + a single bit-swapped 8 KiB CHR-bank
            // register in the $6000-$7FFF window. No IRQ.
            let m87 = Jaleco87::new(prg_rom, chr_rom, h.mirroring)
                .map_err(|e| RomError::InvalidConfig(e.to_string()))?;
            Box::new(m87)
        }
        89 => {
            // Sunsoft-2 on the Sunsoft-3 board (Tenka no Goikenban: Mito
            // Koumon): one $8000-$FFFF register switches a 16 KiB PRG bank, an
            // 8 KiB CHR bank (with an A16 high bit), and one-screen mirroring.
            // No IRQ.
            let m89 = Sunsoft2::new(prg_rom, chr_rom, h.mirroring)
                .map_err(|e| RomError::InvalidConfig(e.to_string()))?;
            Box::new(m89)
        }
        184 => {
            // Sunsoft-1 (Atlantis no Nazo, The Wing of Madoola, Kid Niki):
            // fixed PRG + two 4 KiB CHR banks selected by a single register in
            // the $6000-$7FFF window. No IRQ.
            let m184 = Sunsoft1::new(prg_rom, chr_rom, h.mirroring)
                .map_err(|e| RomError::InvalidConfig(e.to_string()))?;
            Box::new(m184)
        }
        93 => {
            // Sunsoft-3R / Sunsoft-2 IC (Shanghai, Fantasy Zone): UxROM-like
            // 16 KiB PRG bank (bits 4-6) + CHR-RAM enable (bit 0); last PRG
            // bank fixed. CHR is 8 KiB RAM. No IRQ.
            let m93 = Sunsoft3r::new(prg_rom, h.mirroring)
                .map_err(|e| RomError::InvalidConfig(e.to_string()))?;
            Box::new(m93)
        }
        99 => {
            // Nintendo Vs. System: fixed PRG (8/16/32 KiB) + an 8 KiB CHR bank
            // selected by bit 2 of the $4016 write. Detecting mapper 99 forces
            // ConsoleType::VsSystem + the 2C03 RGB PPU above (mapper-driven, so
            // robust against the byte-7 trap). The bus forwards every $4016
            // write to the mapper for the CHR-select bit.
            let m99 = VsSystem::new(prg_rom, chr_rom, h.mirroring)
                .map_err(|e| RomError::InvalidConfig(e.to_string()))?;
            Box::new(m99)
        }
        152 => {
            // Bandai 74161/161 1-screen (Arkanoid II, Pocket Zaurus): UxROM-like
            // 16 KiB PRG bank (bits 4-6) + 8 KiB CHR bank (bits 0-3) + a bit-7
            // software 1-screen mirroring select; last PRG bank fixed. No IRQ.
            let m152 = Bandai152::new(prg_rom, chr_rom)
                .map_err(|e| RomError::InvalidConfig(e.to_string()))?;
            Box::new(m152)
        }
        73 => {
            // Konami VRC3 (Salamander JP): 16-bit CPU-cycle IRQ, no CHR
            // banking (8 KiB CHR-RAM), 16 KiB PRG bank at $F000.
            let vrc3 = Vrc3::new(prg_rom, h.mirroring)
                .map_err(|e| RomError::InvalidConfig(e.to_string()))?;
            Box::new(vrc3)
        }
        64 => {
            // Tengen RAMBO-1 (Klax, Skull & Crossbones): MMC3-like banking
            // with a third switchable PRG bank, finer CHR banking, and a
            // dual-mode (scanline A12 / CPU-cycle) IRQ.
            let m64 = Rambo1::new(prg_rom, chr_rom, h.mirroring)
                .map_err(|e| RomError::InvalidConfig(e.to_string()))?;
            Box::new(m64)
        }
        65 => {
            // Irem H3001 (Daiku no Gen-san 2, Spartan X 2): 16-bit CPU-cycle
            // down-counter IRQ with a write-high/write-low reload latch.
            let m65 = IremH3001::new(prg_rom, chr_rom, h.mirroring)
                .map_err(|e| RomError::InvalidConfig(e.to_string()))?;
            Box::new(m65)
        }
        67 => {
            // Sunsoft-3 (Fantasy Zone 2): 16-bit CPU-cycle IRQ written as a
            // two-write (high-then-low) toggling counter.
            let m67 = Sunsoft3::new(prg_rom, chr_rom, h.mirroring)
                .map_err(|e| RomError::InvalidConfig(e.to_string()))?;
            Box::new(m67)
        }
        68 => {
            // Sunsoft-4 (After Burner, Maharaja): PRG/CHR banking + CHR-ROM
            // as nametables (two nametable bank registers + enable bit). No
            // IRQ.
            let m68 = Sunsoft4::new(prg_rom, chr_rom, h.mirroring)
                .map_err(|e| RomError::InvalidConfig(e.to_string()))?;
            Box::new(m68)
        }
        78 => {
            // Holy Diver / Uchuusen Cosmo Carrier: UxROM-like 16 KiB PRG +
            // 8 KiB CHR with submapper-selected mirroring. NES 2.0
            // submapper 3 = Holy Diver (H/V switch); submapper 1 = Cosmo
            // Carrier (single-screen A/B). Default to Holy Diver (H/V) when
            // no submapper is present, matching the common iNES "alternative
            // nametables" header convention for Holy Diver.
            let variant = if h.is_nes2 && h.submapper == 1 {
                M78Variant::CosmoCarrier
            } else {
                M78Variant::HolyDiver
            };
            let m78 = M78::new(prg_rom, chr_rom, variant)
                .map_err(|e| RomError::InvalidConfig(e.to_string()))?;
            Box::new(m78)
        }
        118 => {
            // TxSROM / TLSROM (Armadillo, NES Play Action Football, Alien
            // Syndrome): MMC3 banking + IRQ plus per-bank nametable mirroring
            // driven by CHR bank bit 7.
            let prg_ram_bytes = if h.prg_ram_size == 0 {
                0
            } else {
                h.prg_ram_size as usize
            };
            let m118 = TxSrom::new(prg_rom, chr_rom, h.mirroring, prg_ram_bytes)
                .map_err(|e| RomError::InvalidConfig(e.to_string()))?;
            Box::new(m118)
        }
        119 => {
            // TQROM (Pin*Bot, High Speed): MMC3 PRG/IRQ/mirroring plus a mixed
            // CHR address space — 64 KiB CHR-ROM + 8 KiB CHR-RAM, selected per
            // 1 KiB bank by bit 6 of the resolved CHR bank number (set =
            // CHR-RAM). See `crates/rustynes-mappers/src/tqrom.rs`.
            let prg_ram_bytes = if h.prg_ram_size == 0 {
                0
            } else {
                h.prg_ram_size as usize
            };
            let m119 = Tqrom::new(prg_rom, chr_rom, h.mirroring, prg_ram_bytes)
                .map_err(|e| RomError::InvalidConfig(e.to_string()))?;
            Box::new(m119)
        }
        210 => {
            // Namco 175 / 340: Namco-163-board variants without the
            // expansion audio (and without an IRQ on either). NES 2.0
            // submapper 1 = Namco 175 (hardwired H/V mirroring, optional
            // enable-gated WRAM); submapper 2 = Namco 340 (H/V/1sc mirroring
            // control). Without a submapper, the wiki recommends 175 if the
            // header battery bit is set (WRAM present) and 340 otherwise.
            let board = if h.is_nes2 && h.submapper == 2 {
                Namco175Board::N340
            } else if (h.is_nes2 && h.submapper == 1) || h.has_battery {
                Namco175Board::N175
            } else {
                Namco175Board::N340
            };
            let m210 = Namco175::new(prg_rom, chr_rom, h.mirroring, board)
                .map_err(|e| RomError::InvalidConfig(e.to_string()))?;
            Box::new(m210)
        }
        80 => {
            // Taito X1-005 (Kyonshiizu 2, Bakushou!! Jinsei Gekijou): a small
            // $7EF0-$7EFF register window (two 2 KiB + four 1 KiB CHR banks,
            // two switchable 8 KiB PRG banks, software H/V mirroring) plus an
            // on-cart 128-byte battery RAM at $7F00-$7FFF unlocked by writing
            // $A3 to both $7EF8 and $7EF9. No IRQ.
            let m80 = TaitoX1005::new(prg_rom, chr_rom, h.mirroring)
                .map_err(|e| RomError::InvalidConfig(e.to_string()))?;
            Box::new(m80)
        }
        82 => {
            // Taito X1-017 (Kyuukyoku Harikiri Koushien / Stadium III): like the
            // X1-005 but with a CHR A12-inversion mode bit (the non-linear
            // 2 KiB/1 KiB CHR-region swap), three protectable 8 KiB PRG-RAM
            // regions, value-shifted CHR (2 KiB banks >> 1) + PRG ($7EFA-$7EFC
            // banks >> 2) registers. The IRQ surface is decoded but unused by
            // the licensed games.
            let m82 = TaitoX1017::new(prg_rom, chr_rom, h.mirroring)
                .map_err(|e| RomError::InvalidConfig(e.to_string()))?;
            Box::new(m82)
        }
        151 => {
            // Konami VS (Vs. Gradius / GVS VS. TKO Boxing): Konami's VRC1
            // silicon on a Nintendo Vs. System board. Banking is byte-identical
            // to mapper 75 (VRC1); the console type was forced to Vs. System +
            // the 2C03 RGB PPU above (mapper-driven, like mapper 99).
            let m151 = KonamiVs::new(prg_rom, chr_rom, h.mirroring)
                .map_err(|e| RomError::InvalidConfig(e.to_string()))?;
            Box::new(m151)
        }
        88 => {
            // Namco 118 with PPU A12 -> CHR A16 (disjoint 64 KiB halves).
            let m88 = Namco118::new(prg_rom, chr_rom, h.mirroring, Namco118Board::M88)
                .map_err(|e| RomError::InvalidConfig(e.to_string()))?;
            Box::new(m88)
        }
        206 => {
            // DxROM / Namco 118 base: MMC3 banking subset, no IRQ / A12.
            let m206 = Namco118::new(prg_rom, chr_rom, h.mirroring, Namco118Board::Dxrom)
                .map_err(|e| RomError::InvalidConfig(e.to_string()))?;
            Box::new(m206)
        }
        85 => {
            // VRC7 (Mapper 85; Lagrange Point JP).  Banking + IRQ
            // identical to VRC6.  FM audio is deferred per ADR-0004
            // (`docs/adr/0004-vrc7-audio-deferred.md`) — there is no
            // permissively-licensed Rust OPLL crate, and the workspace
            // does not take a C build dependency.  The audio register
            // surface at $9010 / $9030 is still decoded and latched so
            // a future v1.x synthesizer integration can read the byte
            // stream without changing the banking / IRQ / save-state
            // layout.
            let vrc7 = Vrc7::new(prg_rom, chr_rom, h.mirroring)
                .map_err(|e| RomError::InvalidConfig(e.to_string()))?;
            Box::new(vrc7)
        }
        // --- v1.2.0 Workstream A, curated (Tier-1) long-tail boards (sprint5). ---
        // Simple discrete-logic mappers; see `tier.rs` (`MapperTier::Curated`)
        // and `docs/adr/0011-mapper-tiering.md`. Each is register-decode
        // unit-tested in `sprint5.rs`.
        38 => Box::new(
            Bitcorp38::new(prg_rom, chr_rom, h.mirroring)
                .map_err(|e| RomError::InvalidConfig(e.to_string()))?,
        ),
        41 => Box::new(
            Caltron41::new(prg_rom, chr_rom, h.mirroring)
                .map_err(|e| RomError::InvalidConfig(e.to_string()))?,
        ),
        79 => Box::new(
            Nina0379::new(prg_rom, chr_rom, h.mirroring)
                .map_err(|e| RomError::InvalidConfig(e.to_string()))?,
        ),
        86 => Box::new(
            Jaleco86::new(prg_rom, chr_rom, h.mirroring)
                .map_err(|e| RomError::InvalidConfig(e.to_string()))?,
        ),
        // Mapper 113: mirroring is register-controlled (no header arg).
        113 => Box::new(
            Nina006M113::new(prg_rom, chr_rom)
                .map_err(|e| RomError::InvalidConfig(e.to_string()))?,
        ),
        140 => Box::new(
            Jaleco140::new(prg_rom, chr_rom, h.mirroring)
                .map_err(|e| RomError::InvalidConfig(e.to_string()))?,
        ),
        232 => Box::new(
            Camerica232::new(prg_rom, chr_rom, h.mirroring)
                .map_err(|e| RomError::InvalidConfig(e.to_string()))?,
        ),
        240 => Box::new(
            Cne240::new(prg_rom, chr_rom, h.mirroring)
                .map_err(|e| RomError::InvalidConfig(e.to_string()))?,
        ),
        241 => Box::new(
            Bxrom241::new(prg_rom, chr_rom, h.mirroring)
                .map_err(|e| RomError::InvalidConfig(e.to_string()))?,
        ),
        other => return Err(RomError::UnsupportedMapper(other)),
    };

    Ok((cart, mapper))
}

#[cfg(test)]
#[allow(clippy::cast_possible_truncation)]
mod tests {
    use super::*;
    use alloc::{vec, vec::Vec};

    fn synth_nrom_rom(prg_kib: usize, chr_kib: usize) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(16 + prg_kib * 1024 + chr_kib * 1024);
        bytes.extend_from_slice(&header::MAGIC);
        bytes.push((prg_kib / 16) as u8); // PRG units
        bytes.push((chr_kib / 8) as u8); // CHR units
        bytes.push(0x00); // flags6: mapper 0, horizontal
        bytes.push(0x00); // flags7
        bytes.extend_from_slice(&[0u8; 8]); // bytes 8-15

        // PRG payload: byte = lower 8 bits of address.
        for i in 0..(prg_kib * 1024) {
            bytes.push((i & 0xFF) as u8);
        }
        // CHR payload: byte = inverted address.
        for i in 0..(chr_kib * 1024) {
            bytes.push(!(i as u8));
        }
        bytes
    }

    #[test]
    fn version_is_non_empty() {
        assert!(!version().is_empty());
    }

    #[test]
    fn parse_synthetic_nrom_16k() {
        let rom = synth_nrom_rom(16, 8);
        let (cart, mut mapper) = parse(&rom).unwrap();
        assert_eq!(cart.mapper_id, 0);
        assert_eq!(cart.prg_rom.len(), 16 * 1024);
        assert_eq!(cart.chr_rom.len(), 8 * 1024);
        assert_eq!(cart.mirroring, Mirroring::Horizontal);
        // Confirm 16K mirroring through the mapper.
        assert_eq!(mapper.cpu_read(0x8000), mapper.cpu_read(0xC000));
    }

    #[test]
    fn parse_synthetic_nrom_32k() {
        let rom = synth_nrom_rom(32, 8);
        let (cart, _mapper) = parse(&rom).unwrap();
        assert_eq!(cart.prg_rom.len(), 32 * 1024);
    }

    #[test]
    fn parse_truncated_returns_typed_error() {
        let mut rom = synth_nrom_rom(32, 8);
        rom.truncate(16 + 100);
        let err = parse(&rom).err().expect("must error");
        assert!(
            matches!(err, RomError::Truncated { .. }),
            "expected Truncated, got {err:?}"
        );
    }

    #[test]
    fn parse_unsupported_mapper_errors() {
        let mut rom = synth_nrom_rom(32, 8);
        // Set mapper 248 — outside the coverage matrix.
        // iNES encoding: mapper_lo = (byte6 >> 4) & 0xF, mapper_hi = byte7 & 0xF0.
        // 248 = 0xF8 -> lo nibble = 8, hi nibble = 0xF. byte6 = 0x80, byte7 = 0xF0.
        rom[6] = 0x80;
        rom[7] = 0xF0;
        let err = parse(&rom).err().expect("must error");
        assert!(
            matches!(err, RomError::UnsupportedMapper(_)),
            "expected UnsupportedMapper, got {err:?}"
        );
    }

    #[test]
    fn parse_fds_image_reports_fds_unsupported() {
        // fwNES container header.
        let mut fwnes = alloc::vec![0u8; 16 + 65500];
        fwnes[0..4].copy_from_slice(b"FDS\x1A");
        fwnes[4] = 1; // one disk side
        assert!(
            matches!(parse(&fwnes).err(), Some(RomError::FdsUnsupported)),
            "fwNES FDS header must report FdsUnsupported"
        );
        // Raw (headerless) disk side opening with the disk-info block.
        let mut raw = alloc::vec![0u8; 65500];
        raw[0] = 0x01;
        raw[1..15].copy_from_slice(b"*NINTENDO-HVC*");
        assert!(
            matches!(parse(&raw).err(), Some(RomError::FdsUnsupported)),
            "raw FDS disk side must report FdsUnsupported"
        );
    }

    #[test]
    fn parse_mapper_5_dispatches_to_mmc5() {
        // Synthesize a 32 KiB PRG / 8 KiB CHR ROM with mapper id 5 (MMC5).
        // iNES: 5 -> low nibble = 5, byte6 = 0x50.
        let mut rom = synth_nrom_rom(32, 8);
        rom[6] = 0x50;
        let (cart, mut mapper) = parse(&rom).expect("MMC5 ROM must parse");
        assert_eq!(cart.mapper_id, 5);
        // Default PRG mode is 3 (4x8K) and $5117 = last bank ROM, so
        // $E000 reads the last 8 KiB. Just exercise the read path.
        let _ = mapper.cpu_read(0xE000);
    }

    #[test]
    fn parse_random_bytes_does_not_panic() {
        // Cheap smoke fuzz; fixed seeds keep test deterministic.
        let seeds: [u64; 8] = [1, 17, 99, 12345, 0xDEAD_BEEF, 0xCAFE, 0x55AA, 0xFF];
        for &s in &seeds {
            let mut state = s;
            let mut bytes = Vec::with_capacity(64);
            for _ in 0..64 {
                state = state
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1);
                bytes.push((state >> 32) as u8);
            }
            // Should not panic; result is don't-care.
            let _ = parse(&bytes);
        }
    }

    #[test]
    fn parse_with_trainer_skips_trainer_bytes() {
        let mut rom = Vec::new();
        rom.extend_from_slice(&header::MAGIC);
        rom.push(2); // 32 KiB PRG
        rom.push(1); // 8 KiB CHR
        rom.push(0b0000_0100); // trainer bit set
        rom.push(0);
        rom.extend_from_slice(&[0u8; 8]);
        rom.extend_from_slice(&[0xAB; 512]); // trainer
        rom.extend_from_slice(&vec![0u8; 32 * 1024]); // PRG
        rom.extend_from_slice(&vec![0u8; 8 * 1024]); // CHR
        let (cart, _) = parse(&rom).unwrap();
        assert!(cart.has_trainer);
        assert_eq!(cart.prg_rom.len(), 32 * 1024);
    }
}
