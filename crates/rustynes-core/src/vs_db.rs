//! Vs. System per-game database (v2.7.0).
//!
//! Closes two gaps in Vs. System support:
//!
//! 1. **PPU palette.** iNES-1.0 dumps carry no NES 2.0 byte-13, so the cartridge
//!    parser defaults every Vs. cart to [`VsPpuType::Rp2C03`]
//!    ([`crate::rustynes_mappers::parse`] §arcade detection). Many Vs. games used a
//!    2C04-000x or RC2C03 PPU whose colour LUT differs from the 2C03's, so an
//!    iNES-1.0 dump renders with the wrong colours. This table supplies the
//!    correct [`VsPpuType`] keyed on the ROM SHA-256; the frontend applies it
//!    via [`crate::Nes::set_vs_ppu_type`] (always — the DB is authoritative for
//!    the palette).
//!
//! 2. **DIP-switch presets.** Vs. arcade games read an 8-bit DIP bank through
//!    the upper bits of `$4016`/`$4017` (coinage, difficulty, lives, PPU-type,
//!    etc.). The frontend's default DIP is `0`, which is not always the
//!    factory-shipment setting; this table supplies each game's documented
//!    default so the frontend can apply it when the user has not set an explicit
//!    `[vs] dip` (precedence: explicit config dip > DB > 0).
//!
//! ## DIP-byte encoding
//!
//! The `vs_dip` byte here is in **this emulator's** encoding: DIP switch 1 =
//! bit 0 .. DIP switch 8 = bit 7. The bus overlay maps DIP1 -> `$4016` bit 3,
//! DIP2 -> `$4016` bit 4, and DIP3..8 -> `$4017` bits 2..7 (see
//! `LockstepBus::vs_overlay_4016` / `vs_overlay_4017`). Each value below is
//! MAME's documented `DSW0` factory default for the corresponding game
//! (the bitwise-OR of the per-field `PORT_DIPNAME` defaults in MAME's
//! `src/mame/nintendo/vsnes.cpp`), which is exactly the `vs_dip` byte the
//! overlay consumes (switch 1 = bit 0). On the real dual-system boards there is
//! a second DIP bank (`DSW1`, for the sub-CPU); this single-CPU model only
//! exposes `DSW0`.
//!
//! ## Sources
//!
//! - DIP defaults: MAME `src/mame/nintendo/vsnes.cpp` `INPUT_PORTS_START`
//!   blocks (`PORT_DIPNAME(<mask>, <default>, ...)`).
//! - PPU types: MAME `src/mame/nintendo/vsnes.cpp` — the per-game `ROM_START`
//!   block's `PALETTE_2C04_000x` / `PALETTE_STANDARD` macro is the authoritative
//!   PPU assignment (each `.pal` ROM is dumped from real hardware). The fceux
//!   `src/vsuni.cpp` "Games/PPU list. Information copied from MAME" table is a
//!   secondary cross-check; both agree for every game below. Verified
//!   2026-06-11 against MAME `master`. (For dual-system carts the `ppu1`
//!   master-CPU PALETTE is used.)
//!
//! ## Caveats
//!
//! - The dual-system games (Balloon Fight / Tennis / Mahjong / Wrecking Crew)
//!   run two CPUs/PPUs; this single-CPU model does not boot them past the attract
//!   screen regardless of the DIP. Their entries are kept for palette correctness
//!   and for forward use once dual-system is modelled. Their `DSW0` defaults are
//!   `0x00` (Balloon Fight / Tennis / Mahjong) per MAME.
//!
//! The table is `&'static`, sorted by SHA-256, and binary-searched. It is
//! `no_std`-safe (const data only).

use rustynes_mappers::VsPpuType;

/// A single Vs. System per-game database entry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VsDbEntry {
    /// The game's factory-default DIP-switch bank, in this emulator's encoding
    /// (switch 1 = bit 0 .. switch 8 = bit 7). MAME `DSW0` default.
    pub vs_dip: u8,
    /// The correct Vs. System PPU type (output palette + 2C05 quirks). Supplies
    /// the right colour LUT for iNES-1.0 dumps that default to the 2C03.
    pub vs_ppu_type: VsPpuType,
    /// `true` for a Vs. **`DualSystem`** cart (two CPUs + two PPUs sharing an
    /// inter-CPU latch — Tennis / Mahjong / Wrecking Crew / Balloon Fight).
    /// v2.0.0 beta.5: [`crate::Emu::from_rom`] routes these to the full
    /// two-console [`crate::VsDualSystem`] wrapper. This flag is the
    /// load-bearing detection source for the circulating iNES-1.0 dumps,
    /// whose headers carry no NES 2.0 byte-13 Vs. hardware type (see
    /// `docs/audit/vs-dualsystem-design-2026-06-11.md`).
    pub dual_system: bool,
}

/// Internal key+value record. Kept private; [`lookup`] returns the value only.
struct Record {
    sha256: [u8; 32],
    entry: VsDbEntry,
}

const fn entry(sha256: [u8; 32], vs_dip: u8, vs_ppu_type: VsPpuType) -> Record {
    Record {
        sha256,
        entry: VsDbEntry {
            vs_dip,
            vs_ppu_type,
            dual_system: false,
        },
    }
}

/// Like [`entry`] but flags the cart as a Vs. **`DualSystem`** title.
const fn entry_dual(sha256: [u8; 32], vs_dip: u8, vs_ppu_type: VsPpuType) -> Record {
    Record {
        sha256,
        entry: VsDbEntry {
            vs_dip,
            vs_ppu_type,
            dual_system: true,
        },
    }
}

/// The embedded database, sorted ascending by SHA-256 (enforced by a unit test).
///
/// Each comment records the game and the MAME `DSW0` default / PPU source.
static DB: &[Record] = &[
    // Vs. Duck Hunt (hack) -- MAME duckhunt DSW0=0x28
    // PPU RC2C03: vsnes.cpp ROM_START(duckhunt) -> PALETTE_STANDARD (rp2c0x.pal).
    entry(
        [
            0x16, 0x0d, 0x43, 0xde, 0x97, 0x7f, 0xbc, 0x8e, 0x24, 0x11, 0x23, 0x54, 0xe7, 0x0e,
            0x40, 0xcf, 0xf8, 0x43, 0x10, 0x16, 0x7b, 0x07, 0xfb, 0x0b, 0x65, 0xeb, 0xc8, 0x19,
            0xfc, 0xf0, 0xca, 0x0c,
        ],
        0x28,
        VsPpuType::Rp2C03,
    ),
    // Vs. Gradius (hack) -- MAME vsgradus DSW0=0x80
    // PPU RP2C04-0001: vsnes.cpp ROM_START(vsgradus) -> PALETTE_2C04_0001.
    entry(
        [
            0x29, 0x01, 0x11, 0x92, 0x9a, 0x34, 0x10, 0x5e, 0x73, 0x56, 0x2d, 0xba, 0x99, 0x69,
            0x01, 0x1d, 0x65, 0x2a, 0xb8, 0x17, 0x67, 0xee, 0x1c, 0x41, 0x74, 0xd2, 0x7b, 0x7c,
            0x33, 0x95, 0xaa, 0x78,
        ],
        0x80,
        VsPpuType::Rp2C04_0001,
    ),
    // Vs. Super Mario Bros. -- MAME suprmrio DSW0=0x10
    // PPU RP2C04-0004: vsnes.cpp ROM_START(suprmrio) -> PALETTE_2C04_0004.
    entry(
        [
            0x2f, 0xaa, 0xb7, 0xa4, 0x83, 0xf9, 0xa3, 0x33, 0x06, 0x6c, 0x54, 0x27, 0xee, 0x78,
            0xb8, 0xf0, 0x0a, 0xe3, 0x00, 0x31, 0x84, 0xbe, 0xd4, 0xad, 0x2f, 0xd0, 0xe0, 0xad,
            0xa3, 0xb0, 0xb1, 0x9b,
        ],
        0x10,
        VsPpuType::Rp2C04_0004,
    ),
    // Vs. Excitebike (hack) -- MAME excitebk DSW0=0x00
    // PPU RP2C04-0003: vsnes.cpp ROM_START(excitebk/excitebko) -> PALETTE_2C04_0003.
    // (US "palette 3" set; the Japanese excitebkj set is RP2C04-0004 -- a
    // different ROM, not staged here.)
    entry(
        [
            0x31, 0xff, 0x4e, 0x40, 0xe0, 0xac, 0x32, 0x9d, 0x20, 0x7b, 0xb6, 0xa0, 0xc3, 0xaa,
            0x65, 0x98, 0x1e, 0xa9, 0xa3, 0x67, 0x63, 0xf6, 0x79, 0x5d, 0x14, 0xf0, 0x3f, 0x87,
            0x4c, 0xb2, 0x35, 0xc2,
        ],
        0x00,
        VsPpuType::Rp2C04_0003,
    ),
    // Vs. Pinball (hack) -- MAME vspinbal DSW0=0x01
    // PPU RP2C04-0001: vsnes.cpp ROM_START(vspinbal) [US set] -> PALETTE_2C04_0001.
    // (The Japanese vspinbalj set is RC2C03B/PALETTE_STANDARD -- not staged here.)
    entry(
        [
            0x44, 0xf4, 0x34, 0x07, 0x0b, 0xf9, 0x10, 0xf4, 0xe5, 0x13, 0x5d, 0x22, 0xba, 0x65,
            0xb8, 0xc7, 0x49, 0x2c, 0xca, 0xf3, 0x25, 0xaa, 0xc1, 0x91, 0xd0, 0xab, 0xf9, 0xac,
            0xb3, 0xa3, 0xce, 0xc9,
        ],
        0x01,
        VsPpuType::Rp2C04_0001,
    ),
    // Vs. Stroke & Match Golf -- MAME smgolf DSW0=0x21
    // PPU RP2C04-0002: vsnes.cpp ROM_START(smgolf) -> PALETTE_2C04_0002.
    entry(
        [
            0x50, 0x65, 0xc6, 0x9c, 0x1e, 0x8b, 0x09, 0x81, 0x8f, 0x37, 0x4d, 0xd5, 0xa3, 0xb3,
            0x43, 0xa8, 0xde, 0x28, 0x36, 0x3a, 0xf5, 0x91, 0x60, 0x91, 0x53, 0x66, 0x33, 0x95,
            0x52, 0x02, 0x01, 0x4c,
        ],
        0x21,
        VsPpuType::Rp2C04_0002,
    ),
    // Vs. Tennis (DualSystem) -- MAME vstennis DSW0=0x00
    // PPU RC2C03: vsnes.cpp ROM_START(vstennis) ppu1 -> PALETTE_STANDARD.
    entry_dual(
        [
            0x52, 0x93, 0x4e, 0x98, 0x16, 0x7d, 0xf4, 0x7d, 0xe5, 0x2a, 0xbc, 0x2c, 0x1f, 0x56,
            0x53, 0xb5, 0x32, 0x93, 0xba, 0x66, 0x7b, 0x91, 0xd2, 0xdf, 0x2d, 0x58, 0x27, 0x41,
            0xf5, 0x0a, 0x45, 0x9c,
        ],
        0x00,
        VsPpuType::Rp2C03,
    ),
    // Vs. Mahjong (DualSystem) -- MAME vsmahjng DSW0=0x00
    // PPU RC2C03: vsnes.cpp ROM_START(vsmahjng) ppu1 -> PALETTE_STANDARD.
    entry_dual(
        [
            0x63, 0x47, 0x05, 0x57, 0xaf, 0xb7, 0xb9, 0xd5, 0x76, 0x63, 0xcc, 0xc6, 0xe9, 0xb4,
            0xd6, 0xcd, 0x70, 0x02, 0x6e, 0xf0, 0x1a, 0x77, 0xdb, 0xb4, 0x66, 0xab, 0xa1, 0xb1,
            0xd3, 0xe4, 0xf5, 0x12,
        ],
        0x00,
        VsPpuType::Rp2C03,
    ),
    // Vs. Wrecking Crew (DualSystem) -- MAME wrecking DSW0=0xF8
    // PPU RP2C04-0002: vsnes.cpp ROM_START(wrecking) ppu1 -> PALETTE_2C04_0002.
    entry_dual(
        [
            0x85, 0xd5, 0xf1, 0x74, 0xfe, 0x94, 0xcc, 0xba, 0x9d, 0x70, 0x2e, 0x01, 0xc0, 0xf7,
            0x2d, 0xcc, 0x56, 0x9b, 0xc2, 0x44, 0x70, 0xd3, 0x4a, 0x36, 0xbb, 0xd5, 0x9a, 0xed,
            0x9b, 0xb2, 0x9d, 0x7d,
        ],
        0xF8,
        VsPpuType::Rp2C04_0002,
    ),
    // Vs. The Goonies (hack) -- MAME goonies DSW0=0x80
    // PPU RP2C04-0003: vsnes.cpp ROM_START(goonies) -> PALETTE_2C04_0003.
    entry(
        [
            0xae, 0xe9, 0x8d, 0xa8, 0x5b, 0xe8, 0x10, 0x2d, 0x41, 0xbd, 0x21, 0x2a, 0xe1, 0x5d,
            0x11, 0x40, 0x35, 0xc0, 0x8d, 0x52, 0x2b, 0xaf, 0x22, 0x2e, 0xdb, 0x12, 0x56, 0xb8,
            0xc9, 0x3f, 0x3b, 0x7d,
        ],
        0x80,
        VsPpuType::Rp2C04_0003,
    ),
    // Vs. T.K.O. Boxing (hack) -- MAME tkoboxng DSW0=0x00
    // PPU RP2C04-0003: vsnes.cpp ROM_START(tkoboxng) -> PALETTE_2C04_0003.
    entry(
        [
            0xb8, 0x15, 0x8a, 0x64, 0xa1, 0xc8, 0xb6, 0x7b, 0x53, 0x0a, 0x01, 0x06, 0x78, 0x77,
            0x2c, 0x43, 0xb0, 0xae, 0xd7, 0x20, 0xbb, 0x28, 0xf2, 0x09, 0x4a, 0xce, 0xe2, 0xf5,
            0x92, 0x78, 0x9c, 0xc9,
        ],
        0x00,
        VsPpuType::Rp2C04_0003,
    ),
    // Vs. Castlevania -- MAME cstlevna DSW0=0x00
    // PPU RP2C04-0002: vsnes.cpp ROM_START(cstlevna) -> PALETTE_2C04_0002.
    entry(
        [
            0xca, 0xbc, 0x23, 0x0a, 0x7f, 0x8c, 0x36, 0x6e, 0xd4, 0x05, 0xfb, 0x83, 0x1e, 0x42,
            0xc3, 0x58, 0xa1, 0xf1, 0x40, 0x8a, 0x42, 0x77, 0x17, 0x16, 0x1c, 0xd1, 0xdd, 0x3d,
            0x86, 0x8d, 0x35, 0x1b,
        ],
        0x00,
        VsPpuType::Rp2C04_0002,
    ),
    // Vs. Ice Climber (hack) -- MAME iceclimb DSW0=0x00
    // PPU RP2C04-0004: vsnes.cpp ROM_START(iceclimb) -> PALETTE_2C04_0004.
    entry(
        [
            0xda, 0x2d, 0x91, 0xc8, 0x47, 0xbf, 0x59, 0x56, 0xeb, 0xe2, 0x6a, 0x0d, 0x64, 0x38,
            0x20, 0x18, 0x9a, 0x3f, 0xa5, 0xe1, 0xdd, 0x71, 0x5d, 0xd7, 0x76, 0x77, 0x0a, 0x61,
            0x5e, 0xf2, 0x69, 0x8e,
        ],
        0x00,
        VsPpuType::Rp2C04_0004,
    ),
    // Vs. Excitebike -- MAME excitebk DSW0=0x00
    // PPU RP2C04-0003: vsnes.cpp ROM_START(excitebk/excitebko) -> PALETTE_2C04_0003.
    // (Staged dump's PRG bank-0 CRC32 = 7e54df1d = MAME `excitebko`, palette 3.)
    entry(
        [
            0xea, 0x27, 0x8a, 0x35, 0xa0, 0x50, 0x17, 0xa8, 0x04, 0x9f, 0x0b, 0xa9, 0x6e, 0x06,
            0x0a, 0x26, 0xf5, 0x50, 0xed, 0x92, 0x02, 0xf8, 0xee, 0x62, 0x50, 0x2b, 0xef, 0x50,
            0xcb, 0x04, 0x5b, 0x23,
        ],
        0x00,
        VsPpuType::Rp2C04_0003,
    ),
    // Vs. Clu Clu Land -- MAME cluclu DSW0=0x10
    // PPU RP2C04-0004: vsnes.cpp ROM_START(cluclu) -> PALETTE_2C04_0004.
    entry(
        [
            0xfb, 0x43, 0x24, 0x81, 0x06, 0xa4, 0x20, 0x25, 0x90, 0x84, 0x0c, 0xca, 0x68, 0x89,
            0x5a, 0xb4, 0xb2, 0xe9, 0x4c, 0x49, 0xf8, 0x2a, 0xa1, 0x5c, 0x7c, 0x23, 0x26, 0x99,
            0xed, 0x7a, 0xb9, 0x0a,
        ],
        0x10,
        VsPpuType::Rp2C04_0004,
    ),
    // Vs. Balloon Fight (DualSystem) -- MAME balonfgt DSW0=0x00
    // PPU RP2C04-0003: vsnes.cpp ROM_START(balonfgt) ppu1 -> PALETTE_2C04_0003.
    // (Not in the fceux vsuni.cpp PPU list -- fceux skips the DualSystem carts;
    // MAME `balonfgt` is the authoritative source.)
    entry_dual(
        [
            0xfd, 0xa8, 0x4d, 0x8d, 0xcd, 0xe6, 0x90, 0xb1, 0x5a, 0xcf, 0x8f, 0x11, 0xb2, 0x7d,
            0x61, 0x3d, 0x57, 0x1a, 0x65, 0xb2, 0xb3, 0x47, 0x19, 0xc6, 0xe0, 0x3e, 0x7f, 0x00,
            0xe3, 0xb7, 0x09, 0x6b,
        ],
        0x00,
        VsPpuType::Rp2C04_0003,
    ),
];

/// Look up a Vs. System per-game database entry by ROM SHA-256.
///
/// Returns `Some(entry)` when the hash is in the embedded table, `None`
/// otherwise. Binary search over the sorted table; `no_std`-safe.
#[must_use]
pub fn lookup(sha256: &[u8; 32]) -> Option<VsDbEntry> {
    DB.binary_search_by(|rec| rec.sha256.cmp(sha256))
        .ok()
        .map(|i| DB[i].entry)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn db_is_sorted_by_sha256() {
        for w in DB.windows(2) {
            assert!(
                w[0].sha256 < w[1].sha256,
                "vs_db must be sorted ascending by SHA-256 (binary search invariant)"
            );
        }
    }

    #[test]
    fn lookup_hit_returns_entry() {
        // Vs. Castlevania -- the first byte 0xca entry.
        let sha = [
            0xca, 0xbc, 0x23, 0x0a, 0x7f, 0x8c, 0x36, 0x6e, 0xd4, 0x05, 0xfb, 0x83, 0x1e, 0x42,
            0xc3, 0x58, 0xa1, 0xf1, 0x40, 0x8a, 0x42, 0x77, 0x17, 0x16, 0x1c, 0xd1, 0xdd, 0x3d,
            0x86, 0x8d, 0x35, 0x1b,
        ];
        let e = lookup(&sha).expect("castlevania present");
        assert_eq!(e.vs_dip, 0x00);
        assert_eq!(e.vs_ppu_type, VsPpuType::Rp2C04_0002);
    }

    #[test]
    fn lookup_miss_returns_none() {
        assert_eq!(lookup(&[0u8; 32]), None);
        assert_eq!(lookup(&[0xffu8; 32]), None);
    }

    #[test]
    fn exactly_the_four_dualsystem_carts_are_flagged() {
        // Tennis / Mahjong / Wrecking Crew / Balloon Fight are flagged
        // dual_system; every other entry (single-system) is not. The flag lets
        // the frontend warn instead of black-screening on a two-CPU cart.
        let dual = DB.iter().filter(|r| r.entry.dual_system).count();
        assert_eq!(dual, 4, "expected exactly 4 DualSystem entries");
        // And the flag survives a lookup round-trip for every flagged record.
        for rec in DB.iter().filter(|r| r.entry.dual_system) {
            assert!(lookup(&rec.sha256).is_some_and(|e| e.dual_system));
        }
    }

    #[test]
    fn every_entry_is_findable() {
        for rec in DB {
            let e = lookup(&rec.sha256).expect("entry findable");
            assert_eq!(e, rec.entry);
        }
    }
}
