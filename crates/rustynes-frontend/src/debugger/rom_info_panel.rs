//! Read-only **ROM Info** browser (v2.2.0 "Capstone").
//!
//! Surfaced from **Tools -> ROM Info**. A purely *observational* companion to
//! the editable **ROM Database** panel: it surfaces, for the currently loaded
//! ROM, the identity + header metadata a user typically wants to confirm at a
//! glance — the two dump-identity CRC32 keys, the SHA-256, the effective
//! per-game database entry (title / mapper / region / mirroring, keyed on the
//! header-excluded CRC), and the cartridge's decoded iNES / NES 2.0 shape
//! (mapper id, region, PRG-ROM / CHR-ROM sizes) read straight off the running
//! [`Nes`].
//!
//! # Two CRC32 keys
//!
//! `RustyNES` keys ROM identity two ways, and both are shown:
//!
//! - **Header-excluded CRC32** — CRC of the PRG+CHR payload only (the iNES
//!   header + any trainer removed). This is the *game-database key*
//!   ([`game_db::entry_for_crc`]) and the canonical way to identify a game
//!   independent of a re-tagged header.
//! - **Full-file CRC32** — CRC of the entire file including the header. This is
//!   the **No-Intro** dump key (No-Intro DATs checksum the whole `.nes` file),
//!   used to recognize a specific dump.
//!
//! No PRG-ROM/bootgod (nescartdb) table is vendored in the repo, so board /
//! chip-level provenance is not surfaced here; the panel is honest about what
//! it knows (the vendored per-game DB + the cartridge header) rather than
//! implying a database it does not carry.
//!
//! This is all frontend-side and read-only — it never mutates the `Nes`, never
//! writes the DB overlay, and the deterministic core never consults any of it.

use rustynes_core::Nes;

use crate::game_db;

/// Panel state: none is required (the view is recomputed from the `Nes` + CRC
/// each frame), but a zero-sized state keeps the panel's wiring uniform with
/// the other tool panels (`show_*` flag + `*_ui` state field).
#[derive(Default)]
pub struct RomInfoPanelState;

/// Format a 32-byte SHA-256 as lowercase hex, wrapped to two 32-char lines so
/// the window stays narrow.
fn sha256_hex(hash: &[u8; 32]) -> (String, String) {
    let mut hi = String::with_capacity(32);
    let mut lo = String::with_capacity(32);
    for (i, b) in hash.iter().enumerate() {
        use std::fmt::Write as _;
        let _ = write!(if i < 16 { &mut hi } else { &mut lo }, "{b:02x}");
    }
    (hi, lo)
}

/// Human-readable size: raw bytes for < 1 KiB, else `<n> KiB (<bytes> bytes)`
/// for any size at or above 1 KiB (the KiB figure is a floored `bytes / 1024`,
/// with the exact byte count always shown alongside for non-multiples).
fn fmt_size(bytes: usize) -> String {
    if bytes < 1024 {
        crate::tf!(HdrBytes, bytes)
    } else {
        crate::tf!(RomInfoKibBytes, bytes / 1024, bytes)
    }
}

/// Render the read-only ROM Info window.
///
/// `crc` is the header-excluded CRC32 (the game-DB key) and `crc_full` the
/// full-file CRC32 (the No-Intro dump key) — both already computed by the app
/// at ROM load. Either may be `None` for an image with no CRC entry (e.g. an
/// FDS / NSF file).
pub fn show(
    ctx: &egui::Context,
    detached: &mut std::collections::HashSet<&'static str>,
    open: &mut bool,
    _state: &mut RomInfoPanelState,
    nes: &Nes,
    crc: Option<u32>,
    crc_full: Option<u32>,
) {
    super::detachable_window(
        ctx,
        detached,
        "rom_info",
        crate::t!(RomInfoTitle),
        super::WindowCfg {
            resizable: Some(false),
            ..Default::default()
        },
        open,
        |ui| {
            // --- Identity / provenance keys ---
            ui.heading(crate::t!(RomInfoIdentity));
            egui::Grid::new("rom_info_identity")
                .num_columns(2)
                .striped(true)
                .show(ui, |ui| {
                    // Title comes from the vendored per-game DB (if listed).
                    let title = crc
                        .and_then(game_db::entry_for_crc)
                        .map(|e| e.title)
                        .filter(|t| !t.is_empty());
                    ui.label(crate::t!(RomInfoTitleDb));
                    ui.label(
                        title
                            .as_deref()
                            .unwrap_or_else(|| crate::t!(RomInfoNotInDb)),
                    );
                    ui.end_row();

                    ui.label(crate::t!(RomInfoCrcDbKey));
                    ui.label(crc.map_or_else(
                        || crate::t!(RomInfoNoCartCrc).to_string(),
                        |c| format!("{c:08X}"),
                    ));
                    ui.end_row();

                    ui.label(crate::t!(RomInfoCrcNoIntro));
                    ui.label(crc_full.map_or_else(
                        || crate::t!(RomInfoUnavailable).to_string(),
                        |c| format!("{c:08X}"),
                    ));
                    ui.end_row();

                    let (hi, lo) = sha256_hex(nes.rom_sha256());
                    ui.label("SHA-256");
                    ui.vertical(|ui| {
                        ui.monospace(hi);
                        ui.monospace(lo);
                    });
                    ui.end_row();
                });

            ui.separator();

            // --- Decoded cartridge header (straight off the running Nes) ---
            ui.heading(crate::t!(RomInfoCartridge));
            egui::Grid::new("rom_info_cart")
                .num_columns(2)
                .striped(true)
                .show(ui, |ui| {
                    ui.label("Mapper");
                    // Show the DB's recorded mapper alongside the active one when
                    // they differ (a header override in effect).
                    let active = nes.mapper_id();
                    let db_mapper = crc.and_then(game_db::entry_for_crc).and_then(|e| e.mapper);
                    match db_mapper {
                        Some(m) if m != active => {
                            ui.label(crate::tf!(RomInfoMapperDb, active, m));
                        }
                        _ => {
                            ui.label(active.to_string());
                        }
                    }
                    ui.end_row();

                    ui.label(crate::t!(HdrRegion));
                    ui.label(format!("{:?}", nes.region()));
                    ui.end_row();

                    ui.label("PRG ROM");
                    ui.label(fmt_size(nes.prg_rom_len()));
                    ui.end_row();

                    let chr = nes.chr_rom_len();
                    ui.label("CHR");
                    ui.label(if chr == 0 {
                        crate::t!(RomInfoChrRamNoRom).to_string()
                    } else {
                        fmt_size(chr)
                    });
                    ui.end_row();

                    // Mirroring / submapper from the DB entry, when present.
                    if let Some(entry) = crc.and_then(game_db::entry_for_crc) {
                        if let Some(m) = entry.mirroring {
                            ui.label(crate::t!(RomInfoMirroringDb));
                            ui.label(format!("{m:?}"));
                            ui.end_row();
                        }
                        if let Some(sm) = entry.submapper {
                            ui.label(crate::t!(RomInfoSubmapperDb));
                            ui.label(sm.to_string());
                            ui.end_row();
                        }
                    }
                });

            ui.separator();
            ui.label(egui::RichText::new(crate::t!(RomInfoFooter)).small().weak());
        },
    );
}
