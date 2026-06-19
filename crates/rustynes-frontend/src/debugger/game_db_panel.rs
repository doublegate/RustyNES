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
    /// v1.7.0 "Forge" Workstream H4 — whether a per-game DIP override is set for
    /// this ROM (loaded from the `<rom>.json` overlay; `false` = follow the
    /// global `[vs] dip` / per-game-DB precedence).
    dip_override: bool,
    /// v1.7.0 H4 — the 8 DIP-switch bits being edited (switch 1 = index 0).
    dip_bits: [bool; 8],
    /// v1.7.0 H4 — last per-game-overlay (DIP) action result.
    per_game_status: Option<String>,
}

/// Pack the 8 edited DIP bits (switch 1 = index 0 = bit 0) into a byte.
fn dip_byte(bits: [bool; 8]) -> u8 {
    let mut v = 0u8;
    for (i, &on) in bits.iter().enumerate() {
        if on {
            v |= 1 << i;
        }
    }
    v
}

/// Unpack a DIP byte into the 8 edited bits (switch 1 = index 0 = bit 0).
// Only the native load path reads a persisted DIP from the `<rom>.json` overlay;
// the wasm build has no filesystem overlay (the editor applies DIPs live only),
// so this unpacker is unused there.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
fn dip_bits_from(byte: u8) -> [bool; 8] {
    let mut bits = [false; 8];
    for (i, b) in bits.iter_mut().enumerate() {
        *b = byte & (1 << i) != 0;
    }
    bits
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
        // v1.7.0 "Forge" Workstream H4 — load the per-game DIP override (if any)
        // from the `<rom>.json` overlay. No sibling path here (the editor only
        // ever reads/writes the config-dir overlay), so pass `None`.
        #[cfg(not(target_arch = "wasm32"))]
        {
            let dip = crate::per_game::resolve(crc, None).and_then(|c| c.dip_switches);
            self.dip_override = dip.is_some();
            self.dip_bits = dip_bits_from(dip.unwrap_or(0));
        }
        self.loaded_crc = Some(crc);
        self.status = None;
        self.per_game_status = None;
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

            // v1.7.0 "Forge" Workstream H4 — Vs. System / arcade DIP-switch
            // editor. Only meaningful for a Vs. cart; for a normal NES game the
            // section is hidden (DIPs read through `$4016`/`$4017`'s upper bits
            // are inert on a standard controller). Edits persist into the
            // per-game `<rom>.json` overlay (config-dir, keyed by CRC) and apply
            // live via the same `set_vs_dip` core setter the load path uses.
            dip_switch_section(ui, state, nes, crc);
        });
    *open = win_open;
}

/// Render the Vs. System DIP-switch editor for the loaded ROM (no-op for a
/// non-Vs. cart). v1.7.0 "Forge" Workstream H4.
fn dip_switch_section(ui: &mut egui::Ui, state: &mut GameDbPanelState, nes: &mut Nes, crc: u32) {
    if !nes.is_vs_system() {
        return;
    }
    ui.separator();
    ui.heading("Vs. System DIP Switches");
    ui.label(
        egui::RichText::new(
            "Per-game DIP switches (difficulty / lives / coinage / free-play — \
             see the game's manual). Saved into this ROM's per-game overlay and \
             applied immediately.",
        )
        .small()
        .weak(),
    );

    let mut changed = false;
    let was_override = state.dip_override;
    changed |= ui
        .checkbox(
            &mut state.dip_override,
            "Override DIP switches for this game",
        )
        .changed();
    // On the OFF -> ON transition, seed the edit bits from the currently-effective
    // DIP byte so enabling the override is a no-op until the user edits a switch.
    // Without this, `dip_bits` may be stale/zeroed (no prior overlay), so merely
    // ticking the box would force the DIP byte to its uninitialized value and
    // perturb behaviour before any actual edit.
    if state.dip_override && !was_override {
        state.dip_bits = dip_bits_from(nes.vs_dip());
    }

    ui.add_enabled_ui(state.dip_override, |ui| {
        egui::Grid::new("vs_dip_grid")
            .num_columns(2)
            .show(ui, |ui| {
                for (i, bit) in state.dip_bits.iter_mut().enumerate() {
                    // Per-game DB labels are not modeled yet, so show numbered
                    // switches (switch 1 = least-significant DIP bit). A known-label
                    // table can replace this `format!` when one is vendored.
                    changed |= ui.checkbox(bit, format!("Switch {}", i + 1)).changed();
                    if i % 2 == 1 {
                        ui.end_row();
                    }
                }
            });
        ui.label(
            egui::RichText::new(format!("DIP byte: {:02X}", dip_byte(state.dip_bits)))
                .small()
                .weak(),
        );
    });

    // Apply live whenever the edited value changes (cheap; `set_vs_dip` is a
    // no-op for a non-Vs. cart). When the override is off we still push the
    // edited byte so toggling "off" reverts to the global precedence on the
    // next reload, but leave the running value as-is to avoid a surprise jump.
    if changed && state.dip_override {
        nes.set_vs_dip(dip_byte(state.dip_bits));
    }

    ui.horizontal(|ui| {
        if ui.button("Save DIP to Per-Game File").clicked() {
            state.per_game_status = Some(persist_dip(crc, state));
        }
        if ui.button("Clear Per-Game DIP").clicked() {
            state.dip_override = false;
            state.per_game_status = Some(persist_dip(crc, state));
        }
    });
    if let Some(msg) = &state.per_game_status {
        ui.label(egui::RichText::new(msg).small());
    }
}

/// Persist the edited DIP into this ROM's per-game `<rom>.json` overlay (merging
/// with any existing overrides so saving the DIP never drops them). Returns a
/// short user-facing status string. v1.7.0 "Forge" Workstream H4.
#[cfg(not(target_arch = "wasm32"))]
fn persist_dip(crc: u32, state: &GameDbPanelState) -> String {
    // Merge over any existing overlay so we don't clobber `overrides` / `notes`.
    let mut cfg = crate::per_game::resolve(crc, None).unwrap_or_default();
    cfg.dip_switches = if state.dip_override {
        Some(dip_byte(state.dip_bits))
    } else {
        None
    };
    match crate::per_game::save_overlay(crc, &cfg) {
        Ok(()) if state.dip_override => "Saved DIP to the per-game overlay.".to_string(),
        Ok(()) => "Cleared the per-game DIP override.".to_string(),
        Err(e) => format!("Save failed: {e}"),
    }
}

/// On wasm there is no filesystem overlay; the edit applies live only.
#[cfg(target_arch = "wasm32")]
fn persist_dip(_crc: u32, _state: &GameDbPanelState) -> String {
    "Per-game files are unavailable in the browser build (applied live only).".to_string()
}
