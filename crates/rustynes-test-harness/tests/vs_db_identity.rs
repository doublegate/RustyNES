//! The Vs. System database finds a dump by its header-independent identity
//! (v2.9.8).
//!
//! [`rustynes_core::vs_db`] supplies the PPU palette and the factory DIP bank
//! for Vs. arcade dumps, which are iNES 1.0 images whose headers cannot carry
//! either. Until v2.9.8 every row was keyed by the SHA-256 of the whole file,
//! header included, so a dump that had been re-headered -- a corrected mapper
//! nibble, a NES 2.0 conversion, a cleared or dirtied padding byte -- lost its
//! row and fell back to the 2C03 palette and DIP 0: wrong colours, and some
//! games stuck in their attract loop.
//!
//! v2.9.8 keys each row on [`rustynes_core::Nes::rom_sha256`], the SHA-256 of
//! the bytes after the 16-byte header, the same identity saves and states use.
//! This file proves it against every staged dump the table describes:
//!
//! 1. each dump resolves by its identity alone, to the same row its whole-file
//!    hash finds -- so every identity key in the table was computed from the
//!    dump it claims to describe;
//! 2. the same dump with one header byte rewritten (byte 10, which an iNES 1.0
//!    parse ignores) still resolves to that row. Not byte 12: a nonzero byte in
//!    12-15 trips the iNES 1.0 dirty-tail rule, which drops byte 7's mapper
//!    nibble, and the 64 KiB combined dual dumps then fail to parse as CNROM.
//!
//! The dumps live at `tests/roms/external/` (gitignored; never committed), so
//! the file is gated on `commercial-roms`. A missing dump fails the test: the
//! table's rows were re-keyed from exactly these files, and a silent skip
//! would leave the keys unverified.

#![cfg(feature = "commercial-roms")]

mod common;

use common::external::read_rom_bytes;
use rustynes_core::vs_db;
use rustynes_core::{Emu, Nes};

/// Every staged dump the Vs. database has a row for, one per row (19). The
/// mapper-099 copies of the five GVS dual and Golf images are byte-identical
/// to the `vs-system/` ones, so they add no row and are not repeated here.
const VS_DB_DUMPS: &[&str] = &[
    "vs-system/GVS Balloon Fight.nes",
    "vs-system/GVS Balloon Fight (Dual).nes",
    "vs-system/GVS Mahjong.nes",
    "vs-system/GVS Stroke & Match Golf.nes",
    "vs-system/GVS Tennis.nes",
    "vs-system/GVS Wrecking Crew.nes",
    "vs-system/GVS Wrecking Crew (Dual).nes",
    "vs-system/VS Castlevania.nes",
    "vs-system/VS Clu Clu Land.nes",
    "vs-system/VS Duck Hunt Hack.nes",
    "vs-system/VS Excitebike.nes",
    "vs-system/VS Excitebike Hack.nes",
    "vs-system/VS Gradius Hack.nes",
    "vs-system/VS Ice Climber Hack.nes",
    "vs-system/VS Pinball Hack.nes",
    "vs-system/VS Super Mario Bros Version MAME V1.1A.nes",
    "vs-system/VS TKO Boxing Hack.nes",
    "vs-system/VS The Goonies Hack.nes",
    "mapper-151-KonamiVS/Goonies, The (VS).zip",
];

/// A hash no row carries, for probing one key at a time.
const NOWHERE: [u8; 32] = [0u8; 32];

#[test]
fn every_staged_vs_dump_resolves_by_its_identity() {
    for rel in VS_DB_DUMPS {
        let bytes = read_rom_bytes(rel);
        let nes = Nes::from_rom(&bytes).unwrap_or_else(|e| panic!("{rel}: {e}"));
        let row = vs_db::lookup_by_hashes(&NOWHERE, nes.image_sha256())
            .unwrap_or_else(|| panic!("{rel}: no row for its whole-file hash"));
        assert_eq!(
            vs_db::lookup_by_hashes(nes.rom_sha256(), &NOWHERE),
            Some(row),
            "{rel}: its identity does not reach the row its whole-file hash does"
        );
        assert_eq!(vs_db::lookup(&nes), Some(row), "{rel}");
    }
}

#[test]
fn a_reheadered_vs_dump_keeps_its_row() {
    for rel in VS_DB_DUMPS {
        let bytes = read_rom_bytes(rel);
        let nes = Nes::from_rom(&bytes).unwrap_or_else(|e| panic!("{rel}: {e}"));
        let row = vs_db::lookup(&nes).unwrap_or_else(|| panic!("{rel}: no row"));

        let mut reheadered = bytes.clone();
        reheadered[10] ^= 0x01;
        let re = Nes::from_rom(&reheadered).unwrap_or_else(|e| panic!("{rel} re-headered: {e}"));
        // The edit is a real header change and nothing else.
        assert_ne!(re.image_sha256(), nes.image_sha256(), "{rel}");
        assert_eq!(re.rom_sha256(), nes.rom_sha256(), "{rel}");

        assert_eq!(
            vs_db::lookup(&re),
            Some(row),
            "{rel}: a header edit lost the Vs. database row"
        );
        // The row's `dual_system` flag is what routes a cabinet, so a
        // re-headered DualSystem dump must still build both consoles.
        if row.dual_system {
            assert!(
                matches!(Emu::from_rom(&reheadered), Ok(Emu::Dual(_))),
                "{rel}: re-headered DualSystem dump no longer routes to the cabinet"
            );
        }
    }
}
