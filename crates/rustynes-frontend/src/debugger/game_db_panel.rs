//! In-app per-game ROM-database editor (v1.2.0 Workstream B, B4).
//!
//! Surfaced from **Tools -> ROM Database**. Shows the loaded ROM's effective
//! per-game database entry (user overlay merged over the vendored base, keyed on
//! the ROM CRC32) and lets the user edit the corrections, persisting them to the
//! user overlay (`<data-dir>/game_db_user.txt`) via [`crate::game_db`].
//!
//! The **mirroring** override applies live (through
//! [`rustynes_core::Nes::set_mirroring_override`]); the region / mapper /
//! submapper overrides are applied by rewriting the iNES header at ROM load, so
//! they take effect the next time the ROM is opened. This is all frontend-side —
//! the deterministic core / test suites never consult the DB.

use rustynes_core::Nes;
use rustynes_core::rustynes_mappers::{Mirroring, Region};

use crate::game_db::{self, GameDbEntry};

/// Editor UI state: the edit buffers + which CRC they were loaded for.
#[derive(Default)]
pub struct GameDbPanelState {
    /// CRC the buffers currently reflect (so we reload when the ROM changes).
    loaded_crc: Option<u32>,
    mirroring: Option<Mirroring>,
    region: Option<Region>,
    /// Mapper-id override as text (empty = no override).
    mapper: String,
    /// Submapper override as text (empty = no override).
    submapper: String,
    title: String,
    /// Last action result, shown under the buttons.
    status: Option<String>,
}

impl GameDbPanelState {
    /// Load the edit buffers from the effective DB entry for `crc` (or blank
    /// defaults if the ROM is not listed).
    fn load_from_db(&mut self, crc: u32) {
        let entry = game_db::entry_for_crc(crc);
        self.mirroring = entry.as_ref().and_then(|e| e.mirroring);
        self.region = entry.as_ref().and_then(|e| e.region);
        self.mapper = entry
            .as_ref()
            .and_then(|e| e.mapper)
            .map(|m| m.to_string())
            .unwrap_or_default();
        self.submapper = entry
            .as_ref()
            .and_then(|e| e.submapper)
            .map(|s| s.to_string())
            .unwrap_or_default();
        self.title = entry.map(|e| e.title).unwrap_or_default();
        self.loaded_crc = Some(crc);
        self.status = None;
    }

    /// Build a [`GameDbEntry`] from the current edit buffers.
    fn to_entry(&self, crc: u32) -> GameDbEntry {
        GameDbEntry {
            crc,
            region: self.region,
            mapper: self.mapper.trim().parse::<u16>().ok(),
            submapper: self.submapper.trim().parse::<u8>().ok(),
            mirroring: self.mirroring,
            title: self.title.trim().to_string(),
        }
    }
}

const MIRRORINGS: &[(Option<Mirroring>, &str)] = &[
    (None, "(no override)"),
    (Some(Mirroring::Horizontal), "Horizontal"),
    (Some(Mirroring::Vertical), "Vertical"),
    (Some(Mirroring::FourScreen), "FourScreen"),
    (Some(Mirroring::SingleScreenA), "SingleScreenA"),
    (Some(Mirroring::SingleScreenB), "SingleScreenB"),
];

const REGIONS: &[(Option<Region>, &str)] = &[
    (None, "(no override)"),
    (Some(Region::Ntsc), "NTSC"),
    (Some(Region::Pal), "PAL"),
    (Some(Region::Dendy), "Dendy"),
];

fn mirroring_label(m: Option<Mirroring>) -> &'static str {
    MIRRORINGS
        .iter()
        .find(|(v, _)| *v == m)
        .map_or("(no override)", |(_, l)| l)
}

fn region_label(r: Option<Region>) -> &'static str {
    REGIONS
        .iter()
        .find(|(v, _)| *v == r)
        .map_or("(no override)", |(_, l)| l)
}

/// Render the ROM-database editor window.
pub fn show(
    ctx: &egui::Context,
    open: &mut bool,
    state: &mut GameDbPanelState,
    nes: &mut Nes,
    crc: Option<u32>,
) {
    let mut win_open = *open;
    egui::Window::new("ROM Database")
        .open(&mut win_open)
        .resizable(false)
        .show(ctx, |ui| {
            let Some(crc) = crc else {
                ui.label("No cartridge loaded (FDS / NSF images have no CRC entry).");
                return;
            };
            // Reload the buffers when the loaded ROM changes.
            if state.loaded_crc != Some(crc) {
                state.load_from_db(crc);
            }

            ui.label(format!("ROM CRC32: {crc:08X}"));
            ui.separator();

            egui::Grid::new("game_db_edit")
                .num_columns(2)
                .show(ui, |ui| {
                    ui.label("Title");
                    ui.text_edit_singleline(&mut state.title);
                    ui.end_row();

                    ui.label("Mirroring");
                    egui::ComboBox::from_id_salt("gdb_mirroring")
                        .selected_text(mirroring_label(state.mirroring))
                        .show_ui(ui, |ui| {
                            for (val, label) in MIRRORINGS {
                                ui.selectable_value(&mut state.mirroring, *val, *label);
                            }
                        });
                    ui.end_row();

                    ui.label("Region");
                    egui::ComboBox::from_id_salt("gdb_region")
                        .selected_text(region_label(state.region))
                        .show_ui(ui, |ui| {
                            for (val, label) in REGIONS {
                                ui.selectable_value(&mut state.region, *val, *label);
                            }
                        });
                    ui.end_row();

                    ui.label("Mapper");
                    ui.text_edit_singleline(&mut state.mapper);
                    ui.end_row();

                    ui.label("Submapper");
                    ui.text_edit_singleline(&mut state.submapper);
                    ui.end_row();
                });

            ui.separator();
            ui.label(
                egui::RichText::new(
                    "Mirroring applies immediately. Region / mapper / submapper apply \
                     on the next ROM load (reopen the ROM).",
                )
                .small()
                .weak(),
            );

            ui.horizontal(|ui| {
                if ui.button("Save & Apply").clicked() {
                    let entry = state.to_entry(crc);
                    match game_db::upsert_user_entry(entry.clone()) {
                        Ok(()) => {
                            nes.set_mirroring_override(entry.mirroring);
                            state.status = Some("Saved to user overrides.".to_string());
                        }
                        Err(e) => state.status = Some(format!("Save failed: {e}")),
                    }
                }
                if ui.button("Reset to Default").clicked() {
                    match game_db::remove_user_entry(crc) {
                        Ok(()) => {
                            state.load_from_db(crc);
                            // Re-apply whatever the vendored base specifies (or clear).
                            nes.set_mirroring_override(state.mirroring);
                            state.status = Some("Reverted to the vendored default.".to_string());
                        }
                        Err(e) => state.status = Some(format!("Reset failed: {e}")),
                    }
                }
            });

            if let Some(msg) = &state.status {
                ui.label(egui::RichText::new(msg).small());
            }
        });
    *open = win_open;
}
