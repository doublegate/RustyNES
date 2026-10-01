//! Vs. System per-game database (v2.7.0).
//!
//! Closes two gaps in Vs. System support:
//!
//! 1. **PPU palette.** iNES-1.0 dumps carry no NES 2.0 byte-13, so the cartridge
//!    parser defaults every Vs. cart to [`VsPpuType::Rp2C03`]
//!    ([`crate::rustynes_mappers::parse`] §arcade detection). Many Vs. games used a
//!    2C04-000x or RC2C03 PPU whose colour LUT differs from the 2C03's, so an
//!    iNES-1.0 dump renders with the wrong colours. This table supplies the
//!    correct [`VsPpuType`] keyed on the ROM's identity (see "Keys" below); the
//!    frontend applies it
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
//! `SystemBus::vs_overlay_4016` / `vs_overlay_4017`). Each value below is
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
//!   run two CPUs/PPUs, modelled since v2.0.0 beta.5 by
//!   [`crate::VsDualSystem`] (routed via [`crate::Emu::from_rom`], which
//!   consults this table's `dual_system` flag). Their `DSW0` defaults are
//!   `0x00` (Balloon Fight / Tennis / Mahjong) per MAME.
//! - Two titles (Balloon Fight, Wrecking Crew) additionally carry a
//!   **second, COMBINED-dump entry** with a different SHA-256: the
//!   pre-existing 32 KiB-PRG entry (main-CPU program only -- boot cannot
//!   complete, the sub-CPU program is absent) coexists with a new 64 KiB-PRG
//!   entry (main half + sub half concatenated) once the sub-CPU program was
//!   located. See `docs/audit/vs-dualsystem-combined-dumps-2026-07-02.md`. Tennis and
//!   Mahjong remain 32 KiB-only -- no sub-CPU dump has been located for
//!   either.
//!
//! ## Keys (v2.9.8)
//!
//! Each row carries two SHA-256 keys:
//!
//! - an **identity key**: [`crate::Nes::rom_sha256`] of the dump, the SHA-256
//!   of the bytes after its 16-byte iNES header. It is the same identity saves,
//!   states and cheats are keyed by, and it survives any edit to the header: a
//!   re-headered or corrected dump still finds its row, so it keeps its palette
//!   and DIP preset instead of falling back to the 2C03 and DIP 0. Every row
//!   whose dump is staged under `tests/roms/external/` carries one, computed
//!   from that dump; at v2.9.8 that is all 19.
//! - an **image key**: the SHA-256 of the whole file as dumped, header
//!   included ([`crate::Nes::image_sha256`]). Until v2.9.8 it was the only
//!   key. It stays on every row, so a row added from a dump that is not staged
//!   (for which no identity can be computed) is still reachable, and it records
//!   exactly which file each row was verified against.
//!
//! [`lookup`] takes the [`crate::Nes`] and matches the identity key first, then
//! the image key. The table is `&'static` and scanned linearly (19 rows,
//! consulted once per ROM load). It is `no_std`-safe (const data only).

use rustynes_mappers::VsPpuType;

use crate::Nes;

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
    /// [`crate::Nes::rom_sha256`] of the dump: SHA-256 of the bytes after its
    /// 16-byte header. `None` only for a row whose dump was never staged, so
    /// that no identity could be computed for it; such a row is reached by
    /// [`Self::image`] alone.
    identity: Option<[u8; 32]>,
    /// SHA-256 of the whole dump, header included ([`crate::Nes::image_sha256`]).
    /// The pre-v2.9.8 key, kept on every row.
    image: [u8; 32],
    entry: VsDbEntry,
}

const fn entry(
    identity: Option<[u8; 32]>,
    image: [u8; 32],
    vs_dip: u8,
    vs_ppu_type: VsPpuType,
) -> Record {
    Record {
        identity,
        image,
        entry: VsDbEntry {
            vs_dip,
            vs_ppu_type,
            dual_system: false,
        },
    }
}

/// Like [`entry`] but flags the cart as a Vs. **`DualSystem`** title.
const fn entry_dual(
    identity: Option<[u8; 32]>,
    image: [u8; 32],
    vs_dip: u8,
    vs_ppu_type: VsPpuType,
) -> Record {
    Record {
        identity,
        image,
        entry: VsDbEntry {
            vs_dip,
            vs_ppu_type,
            dual_system: true,
        },
    }
}

/// The embedded database. Row order carries no meaning (it is the ascending
/// image-key order the pre-v2.9.8 binary search needed, kept to keep history
/// readable); no two rows share a key (enforced by a unit test).
///
/// Each comment records the game and the MAME `DSW0` default / PPU source.
/// Each row's first array is its identity key, the second its image key.
static DB: &[Record] = &[
    // Vs. Duck Hunt (hack) -- MAME duckhunt DSW0=0x28
    // PPU RC2C03: vsnes.cpp ROM_START(duckhunt) -> PALETTE_STANDARD (rp2c0x.pal).
    entry(
        Some([
            0x6e, 0xe2, 0xbd, 0x97, 0x73, 0x96, 0xba, 0xcd, 0x6c, 0x36, 0x71, 0xa6, 0x5b, 0xd2,
            0xcf, 0x23, 0x5a, 0xaa, 0xe0, 0x0b, 0x91, 0x06, 0x08, 0x3e, 0xc8, 0x62, 0xb8, 0x9a,
            0x8a, 0x75, 0xfc, 0x19,
        ]),
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
        Some([
            0x7f, 0x9f, 0x79, 0x92, 0x20, 0xe2, 0x54, 0xcd, 0x3b, 0x55, 0x27, 0xc6, 0xc8, 0xe5,
            0x09, 0xe5, 0x00, 0x08, 0xf7, 0x38, 0x95, 0xec, 0x0f, 0xd8, 0xc0, 0xf8, 0x77, 0x3c,
            0x0f, 0x5b, 0x75, 0x92,
        ]),
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
        Some([
            0xc1, 0x9d, 0x54, 0x61, 0xfa, 0x40, 0xa7, 0x0c, 0x6e, 0x50, 0xf0, 0x70, 0x96, 0x63,
            0x80, 0x97, 0xfa, 0x85, 0x8d, 0xec, 0x83, 0x0a, 0xce, 0xf9, 0x71, 0xfb, 0x7e, 0xae,
            0xae, 0x5f, 0x9c, 0x00,
        ]),
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
        Some([
            0x85, 0x70, 0xd6, 0xf3, 0xab, 0x7a, 0xfa, 0x49, 0x17, 0x94, 0x60, 0x5b, 0xfd, 0xa5,
            0xd9, 0xb4, 0xa0, 0xa4, 0x38, 0xde, 0xd3, 0xb7, 0x2c, 0x5a, 0xa1, 0x19, 0x25, 0x03,
            0x31, 0x65, 0x90, 0x81,
        ]),
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
        Some([
            0x5c, 0x78, 0x67, 0xaa, 0xf4, 0x59, 0xce, 0x43, 0x49, 0xf1, 0x30, 0x6a, 0xb6, 0x92,
            0xd7, 0xd2, 0xa0, 0xe9, 0xe7, 0xb3, 0x53, 0x4e, 0xc3, 0x86, 0xeb, 0x2d, 0xb2, 0x22,
            0x37, 0x13, 0xca, 0xb2,
        ]),
        [
            0x44, 0xf4, 0x34, 0x07, 0x0b, 0xf9, 0x10, 0xf4, 0xe5, 0x13, 0x5d, 0x22, 0xba, 0x65,
            0xb8, 0xc7, 0x49, 0x2c, 0xca, 0xf3, 0x25, 0xaa, 0xc1, 0x91, 0xd0, 0xab, 0xf9, 0xac,
            0xb3, 0xa3, 0xce, 0xc9,
        ],
        0x01,
        VsPpuType::Rp2C04_0001,
    ),
    // Vs. Wrecking Crew (DualSystem, COMBINED 64 KiB dump: main PRG half +
    // sub-CPU PRG half, MAME `.6d/.6c/.6b/.6a`) -- MAME wrecking DSW0=0xF8.
    // PPU RP2C04-0002: vsnes.cpp ROM_START(wrecking) ppu1 -> PALETTE_2C04_0002.
    // Distinct SHA-256 from the pre-existing 32 KiB-only "GVS Wrecking
    // Crew.nes" entry below (same game, same DIP/PPU -- the dump
    // completeness is what differs). See
    // `docs/audit/vs-dualsystem-combined-dumps-2026-07-02.md` for how this dump was
    // assembled and verified.
    entry_dual(
        Some([
            0x28, 0x91, 0xef, 0x4d, 0x44, 0x50, 0xd2, 0x26, 0x77, 0x94, 0x7e, 0x79, 0x8a, 0x56,
            0x95, 0x47, 0x07, 0x5f, 0x58, 0x5f, 0xb5, 0x05, 0xd7, 0x06, 0x15, 0x4e, 0x85, 0x32,
            0x88, 0x05, 0x08, 0xd9,
        ]),
        [
            0x4c, 0xde, 0x98, 0x3b, 0x9d, 0x03, 0x40, 0x2b, 0x32, 0x4e, 0x28, 0x15, 0x18, 0x92,
            0x68, 0x04, 0x1b, 0x31, 0x3a, 0x93, 0x77, 0xd7, 0xf0, 0x85, 0x6c, 0xf8, 0xd3, 0xa9,
            0x79, 0x07, 0x08, 0xfe,
        ],
        0xF8,
        VsPpuType::Rp2C04_0002,
    ),
    // Vs. Stroke & Match Golf -- MAME smgolf DSW0=0x21
    // PPU RP2C04-0002: vsnes.cpp ROM_START(smgolf) -> PALETTE_2C04_0002.
    entry(
        Some([
            0x4b, 0xda, 0x51, 0xcc, 0xcf, 0x07, 0x46, 0x77, 0xf5, 0xb9, 0x02, 0x89, 0xa8, 0x4b,
            0xd3, 0x41, 0xb2, 0xcb, 0x7c, 0x1b, 0xbe, 0x47, 0xdc, 0x59, 0x37, 0xd6, 0x95, 0x31,
            0x15, 0xe7, 0xbb, 0xf1,
        ]),
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
        Some([
            0x98, 0x10, 0x7b, 0x00, 0x4d, 0x05, 0xe4, 0x5e, 0x5c, 0x5f, 0x7c, 0x62, 0x6c, 0xcf,
            0xbf, 0x19, 0x61, 0xc5, 0x4c, 0xe5, 0x3a, 0x2b, 0x37, 0x5b, 0x2f, 0x21, 0xec, 0xae,
            0x44, 0x9f, 0x01, 0xd0,
        ]),
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
        Some([
            0x63, 0xf8, 0x55, 0x86, 0xea, 0xec, 0x6b, 0x40, 0x0b, 0x04, 0x67, 0x8e, 0xd4, 0xa9,
            0x6c, 0x51, 0xf1, 0xb0, 0x21, 0x13, 0x20, 0x4a, 0x16, 0xab, 0x35, 0xf9, 0x3e, 0xd9,
            0xd3, 0x09, 0xf2, 0xf6,
        ]),
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
        Some([
            0xcd, 0x06, 0x54, 0xbc, 0x20, 0xe4, 0x05, 0x22, 0x5a, 0x7e, 0x23, 0xca, 0xe9, 0x62,
            0xd3, 0xb8, 0x36, 0x6b, 0xc1, 0xa0, 0x4f, 0x19, 0xc9, 0x88, 0x59, 0x7c, 0xd7, 0x2f,
            0xc7, 0xaa, 0x25, 0xa4,
        ]),
        [
            0x85, 0xd5, 0xf1, 0x74, 0xfe, 0x94, 0xcc, 0xba, 0x9d, 0x70, 0x2e, 0x01, 0xc0, 0xf7,
            0x2d, 0xcc, 0x56, 0x9b, 0xc2, 0x44, 0x70, 0xd3, 0x4a, 0x36, 0xbb, 0xd5, 0x9a, 0xed,
            0x9b, 0xb2, 0x9d, 0x7d,
        ],
        0xF8,
        VsPpuType::Rp2C04_0002,
    ),
    // Vs. Balloon Fight (DualSystem, COMBINED 64 KiB dump: main PRG half +
    // sub-CPU PRG half, MAME `.6d/.6c/.6b/.6a`) -- MAME balonfgt DSW0=0x00.
    // PPU RP2C04-0003: vsnes.cpp ROM_START(balonfgt) ppu1 -> PALETTE_2C04_0003.
    // Distinct SHA-256 from the pre-existing 32 KiB-only "GVS Balloon
    // Fight.nes" entry above (same game, same DIP/PPU -- the dump
    // completeness is what differs). See
    // `docs/audit/vs-dualsystem-combined-dumps-2026-07-02.md` for how this dump was
    // assembled and verified.
    entry_dual(
        Some([
            0xc4, 0x6d, 0x51, 0x57, 0x6e, 0x65, 0xae, 0xc7, 0xdf, 0xe8, 0x24, 0x59, 0x65, 0x62,
            0x60, 0x71, 0x99, 0x9b, 0x7a, 0x73, 0x78, 0x78, 0xae, 0xac, 0x36, 0x53, 0x71, 0x7f,
            0x1f, 0xbe, 0x94, 0xcd,
        ]),
        [
            0x8b, 0x32, 0x80, 0xe6, 0x51, 0xf3, 0xf8, 0xd9, 0x9d, 0xe0, 0x46, 0xcd, 0xd2, 0x71,
            0x0e, 0x2f, 0xf5, 0xf9, 0x4b, 0x5d, 0xd4, 0x22, 0x49, 0x82, 0xcc, 0xa4, 0xa9, 0xa7,
            0x26, 0x44, 0x41, 0xcb,
        ],
        0x00,
        VsPpuType::Rp2C04_0003,
    ),
    // Vs. The Goonies (hack) -- MAME goonies DSW0=0x80
    // PPU RP2C04-0003: vsnes.cpp ROM_START(goonies) -> PALETTE_2C04_0003.
    entry(
        Some([
            0x44, 0x7c, 0x57, 0x7d, 0xf5, 0x4c, 0xaf, 0x00, 0xbb, 0x28, 0xb1, 0xc3, 0xda, 0x36,
            0x2a, 0x86, 0x76, 0x14, 0x52, 0x1e, 0x72, 0xfc, 0xd4, 0xf7, 0xdb, 0x3e, 0x58, 0x9b,
            0xd2, 0xf5, 0xd3, 0x6b,
        ]),
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
        Some([
            0xfc, 0xee, 0xbd, 0x7a, 0x79, 0xa2, 0x42, 0x89, 0x87, 0x40, 0xf7, 0x77, 0xd9, 0x1f,
            0x90, 0xa2, 0xe9, 0xfd, 0xd8, 0x6b, 0x51, 0x4e, 0xa9, 0x3a, 0x90, 0x9e, 0x28, 0xe0,
            0xbe, 0xec, 0x0e, 0x0d,
        ]),
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
        Some([
            0x37, 0xca, 0x2b, 0x68, 0x98, 0xe6, 0xc3, 0xcc, 0x96, 0xdc, 0xde, 0x5f, 0x0b, 0xf1,
            0x72, 0x68, 0xb9, 0x07, 0xb4, 0x48, 0xa4, 0x5c, 0x58, 0xa8, 0xf1, 0xea, 0x9d, 0x2c,
            0x34, 0xbd, 0x7c, 0x63,
        ]),
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
        Some([
            0x38, 0xdb, 0x13, 0x56, 0x24, 0x3b, 0xae, 0xac, 0x8a, 0x85, 0x4d, 0x44, 0x7b, 0x5d,
            0x42, 0xc2, 0xd5, 0x73, 0xce, 0x17, 0x52, 0xf5, 0xfc, 0x59, 0x5b, 0xb2, 0x2f, 0x86,
            0x0d, 0xa9, 0x2f, 0x9e,
        ]),
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
        Some([
            0x88, 0x9a, 0x48, 0x95, 0x0d, 0x3c, 0x1d, 0x8d, 0x06, 0x75, 0x58, 0x0d, 0x37, 0x7a,
            0xab, 0x99, 0x5b, 0x57, 0x16, 0x13, 0x4f, 0xe5, 0xdc, 0x17, 0xb8, 0x40, 0xb7, 0x65,
            0x8f, 0x52, 0x9f, 0x40,
        ]),
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
        Some([
            0xc1, 0xa9, 0xb1, 0x3e, 0x18, 0xfb, 0x3a, 0x00, 0xaa, 0x36, 0x09, 0x67, 0x2a, 0x38,
            0xca, 0x8c, 0x24, 0x65, 0x45, 0x51, 0xd9, 0x0d, 0x3d, 0x43, 0x04, 0xe9, 0xb9, 0xf7,
            0xf8, 0x29, 0xc2, 0xd1,
        ]),
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
        Some([
            0xe3, 0x17, 0xd1, 0xdf, 0x4e, 0xd1, 0xb6, 0x41, 0x0f, 0xbd, 0x2d, 0xda, 0x65, 0x12,
            0xc0, 0x6d, 0x39, 0xce, 0x23, 0x85, 0x22, 0x6c, 0x06, 0x43, 0xbd, 0x95, 0xc8, 0x70,
            0xed, 0xd7, 0xfd, 0x97,
        ]),
        [
            0xfd, 0xa8, 0x4d, 0x8d, 0xcd, 0xe6, 0x90, 0xb1, 0x5a, 0xcf, 0x8f, 0x11, 0xb2, 0x7d,
            0x61, 0x3d, 0x57, 0x1a, 0x65, 0xb2, 0xb3, 0x47, 0x19, 0xc6, 0xe0, 0x3e, 0x7f, 0x00,
            0xe3, 0xb7, 0x09, 0x6b,
        ],
        0x00,
        VsPpuType::Rp2C04_0003,
    ),
    // Vs. The Goonies (unpatched Konami dump, `Goonies, The (VS).nes`, iNES
    // mapper 151) -- the same game, board and PPU as the "(hack)" row above,
    // whose PPU and DSW0 default it carries over: RP2C04-0003, DSW0=0x80.
    // The two files differ only in 327 PRG bytes of code. v2.9.8: without
    // this row the dump fell back to the 2C03 and drew its title red on
    // green; with it, the copyright screen is white on black like the hack.
    entry(
        Some([
            0x1f, 0x61, 0x3c, 0xe5, 0xce, 0xb1, 0xa8, 0x1f, 0xeb, 0x55, 0x98, 0xe5, 0x0a, 0x37,
            0x8b, 0xe0, 0x78, 0xde, 0x6c, 0xd6, 0x2b, 0x4c, 0x0c, 0x3b, 0x10, 0xdb, 0x3f, 0x59,
            0xd4, 0x2f, 0xbe, 0x14,
        ]),
        [
            0xff, 0x26, 0x8f, 0xb3, 0xbb, 0x3e, 0xa2, 0x74, 0x3b, 0x6f, 0xda, 0xe0, 0x5e, 0x84,
            0x54, 0x7c, 0xce, 0x0f, 0xbb, 0xa0, 0x94, 0x77, 0x79, 0xab, 0xe9, 0x99, 0x0a, 0x94,
            0x37, 0x08, 0x63, 0x67,
        ],
        0x80,
        VsPpuType::Rp2C04_0003,
    ),
];

/// Look up the Vs. System per-game database entry for a loaded ROM.
///
/// Matches the ROM's header-independent identity ([`Nes::rom_sha256`]) first,
/// then its whole-image hash ([`Nes::image_sha256`]); see the module's "Keys"
/// section. Returns `None` for a ROM the table does not describe, including
/// every non-Vs. ROM. `no_std`-safe.
///
/// v2.9.8 changed the parameter from a bare hash to the [`Nes`]: the old
/// `lookup(&[u8; 32])` could only be handed one of the two hashes, and handing
/// it the wrong one compiled and silently missed. Use [`lookup_by_hashes`]
/// when the two hashes are available without a [`Nes`].
#[must_use]
pub fn lookup(nes: &Nes) -> Option<VsDbEntry> {
    lookup_by_hashes(nes.rom_sha256(), nes.image_sha256())
}

/// [`lookup`] with the two hashes supplied directly.
///
/// `identity` is [`Nes::rom_sha256`] (SHA-256 of an iNES image's bytes after
/// its 16-byte header) and `image` is [`Nes::image_sha256`] (SHA-256 of the
/// whole image). The identity key is matched first.
#[must_use]
pub fn lookup_by_hashes(identity: &[u8; 32], image: &[u8; 32]) -> Option<VsDbEntry> {
    lookup_in(DB, identity, image)
}

/// The lookup rule over an arbitrary table, so the unit tests can exercise a
/// row without an identity key (the shipped table has none).
fn lookup_in(table: &[Record], identity: &[u8; 32], image: &[u8; 32]) -> Option<VsDbEntry> {
    table
        .iter()
        .find(|rec| rec.identity.as_ref() == Some(identity))
        .or_else(|| table.iter().find(|rec| rec.image == *image))
        .map(|rec| rec.entry)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A hash no row carries, for probing one key at a time.
    const NOWHERE: [u8; 32] = [0u8; 32];

    #[test]
    fn no_two_rows_share_a_key() {
        for (i, a) in DB.iter().enumerate() {
            for b in &DB[i + 1..] {
                assert_ne!(a.image, b.image, "two rows share an image key");
                if let (Some(x), Some(y)) = (a.identity, b.identity) {
                    assert_ne!(x, y, "two rows share an identity key");
                }
            }
        }
    }

    /// Enumerates the table: every row is reachable, by its identity key alone
    /// when it has one and by its image key alone in every case, and each key
    /// reaches that row and no other. Nothing in the table is unreachable.
    #[test]
    fn every_row_is_reachable_by_each_of_its_keys() {
        for rec in DB {
            assert_eq!(lookup_by_hashes(&NOWHERE, &rec.image), Some(rec.entry));
            if let Some(identity) = rec.identity {
                assert_eq!(lookup_by_hashes(&identity, &NOWHERE), Some(rec.entry));
            }
        }
    }

    /// Every row in the shipped table was re-keyed from its staged dump in
    /// v2.9.8. A future row added from a dump nobody has staged would carry
    /// `None` here and lower this count, which is allowed; the count records
    /// the state, so a change to it is a deliberate edit.
    #[test]
    fn all_nineteen_rows_carry_an_identity_key() {
        assert_eq!(DB.len(), 19);
        assert_eq!(DB.iter().filter(|r| r.identity.is_some()).count(), 19);
    }

    /// The rule itself, over a table holding one re-keyed row and one row left
    /// on its image key: the identity finds the first whatever the image hash
    /// says, the image hash finds the second, and the identity wins when the
    /// two keys point at different rows.
    #[test]
    fn identity_matches_first_and_image_rows_stay_reachable() {
        let rekeyed = entry(Some([1; 32]), [2; 32], 0x11, VsPpuType::Rp2C03);
        let image_only = entry(None, [3; 32], 0x22, VsPpuType::Rp2C04_0001);
        let table = [rekeyed, image_only];
        let dip = |identity: &[u8; 32], image: &[u8; 32]| {
            lookup_in(&table, identity, image).map(|e| e.vs_dip)
        };
        assert_eq!(
            dip(&[1; 32], &NOWHERE),
            Some(0x11),
            "identity, header edited"
        );
        assert_eq!(
            dip(&[1; 32], &[2; 32]),
            Some(0x11),
            "identity, header as dumped"
        );
        assert_eq!(
            dip(&NOWHERE, &[2; 32]),
            Some(0x11),
            "image key of a re-keyed row"
        );
        assert_eq!(dip(&NOWHERE, &[3; 32]), Some(0x22), "image-only row");
        assert_eq!(dip(&[1; 32], &[3; 32]), Some(0x11), "identity wins");
        assert_eq!(dip(&NOWHERE, &NOWHERE), None);
        // A `None` identity never matches anything, the all-zero hash included.
        assert_eq!(dip(&[0; 32], &[9; 32]), None);
    }

    #[test]
    fn lookup_hit_returns_entry() {
        // Vs. Castlevania, by its image key (the pre-v2.9.8 key).
        let sha = [
            0xca, 0xbc, 0x23, 0x0a, 0x7f, 0x8c, 0x36, 0x6e, 0xd4, 0x05, 0xfb, 0x83, 0x1e, 0x42,
            0xc3, 0x58, 0xa1, 0xf1, 0x40, 0x8a, 0x42, 0x77, 0x17, 0x16, 0x1c, 0xd1, 0xdd, 0x3d,
            0x86, 0x8d, 0x35, 0x1b,
        ];
        let e = lookup_by_hashes(&NOWHERE, &sha).expect("castlevania present");
        assert_eq!(e.vs_dip, 0x00);
        assert_eq!(e.vs_ppu_type, VsPpuType::Rp2C04_0002);
    }

    #[test]
    fn goonies_original_dump_uses_the_2c04_0003_palette() {
        // The unpatched Konami dump of Vs. The Goonies (`Goonies, The
        // (VS).nes`, mapper 151 header). Same game and PPU as the hack row
        // above; without a row it fell back to the 2C03 and drew its title
        // red on green.
        let sha = [
            0xff, 0x26, 0x8f, 0xb3, 0xbb, 0x3e, 0xa2, 0x74, 0x3b, 0x6f, 0xda, 0xe0, 0x5e, 0x84,
            0x54, 0x7c, 0xce, 0x0f, 0xbb, 0xa0, 0x94, 0x77, 0x79, 0xab, 0xe9, 0x99, 0x0a, 0x94,
            0x37, 0x08, 0x63, 0x67,
        ];
        let e = lookup_by_hashes(&NOWHERE, &sha).expect("original Goonies dump present");
        assert_eq!(e.vs_ppu_type, VsPpuType::Rp2C04_0003);
        assert_eq!(e.vs_dip, 0x80);
        assert!(!e.dual_system);
    }

    #[test]
    fn lookup_miss_returns_none() {
        assert_eq!(lookup_by_hashes(&[0u8; 32], &[0u8; 32]), None);
        assert_eq!(lookup_by_hashes(&[0xffu8; 32], &[0xffu8; 32]), None);
    }

    #[test]
    fn exactly_six_dualsystem_rows_across_the_four_carts_are_flagged() {
        // Tennis / Mahjong / Wrecking Crew / Balloon Fight are the four
        // DualSystem *games*; every other entry (single-system) is not
        // flagged. The flag lets the frontend warn instead of
        // black-screening on a two-CPU cart -- and lets `Emu::from_rom`
        // route to the two-console wrapper.
        //
        // The row COUNT is 6, not 4: Tennis and Mahjong have only the
        // original 32 KiB-PRG (main-CPU-only) dump staged (no sub-CPU
        // program has been located for either), while Balloon Fight and
        // Wrecking Crew each additionally have a 64 KiB-PRG COMBINED dump
        // (main half + sub half) once the sub-CPU program was located --
        // see the "Caveats" doc section above and
        // `docs/audit/vs-dualsystem-combined-dumps-2026-07-02.md`. The 32 KiB-only
        // entries for those two titles are kept: loading that specific
        // (incomplete) dump still correctly flags `dual_system` (and thus
        // still routes to the wrapper), it just can't complete the boot
        // handshake -- which is expected/harmless, not a bug.
        let dual = DB.iter().filter(|r| r.entry.dual_system).count();
        assert_eq!(
            dual, 6,
            "expected exactly 6 DualSystem rows (4 games, 2 of which have both an incomplete and a complete dump entry)"
        );
        // And the flag survives a lookup round-trip for every flagged record.
        for rec in DB.iter().filter(|r| r.entry.dual_system) {
            assert!(lookup_by_hashes(&NOWHERE, &rec.image).is_some_and(|e| e.dual_system));
        }
    }
}
