#![allow(
    clippy::cast_lossless,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::missing_const_for_fn,
    clippy::suboptimal_flops,
    clippy::items_after_statements,
    clippy::too_many_arguments,
    clippy::too_many_lines,
    clippy::needless_pass_by_ref_mut
)]
//! Game Genie + raw RAM cheat panel (Game Genie: v1.6.0 Sprint 1; raw RAM
//! cheats: v1.7.0).
//!
//! The Game Genie section lists the configured codes with an enabled checkbox,
//! the canonical code, and the decoded address / data (+ compare for
//! 8-character codes). New codes are validated through `rustynes_core::GenieCode::new`;
//! an invalid code shows the `GenieError` `Display` text inline instead of
//! panicking. On any change (add / remove / toggle), the running `Nes` is
//! re-synced — `clear_genie_codes` followed by `add_genie_code` for every
//! ENABLED entry — and the per-ROM file is persisted (native only; see
//! [`crate::cheats`]). Game Genie codes are a PRG-read overlay, so disabling
//! them all restores byte-identical PRG reads.
//!
//! The raw RAM section (GameShark-style) lists `{ address, value, compare }`
//! entries. Unlike Game Genie codes these are NOT pushed into the `Nes`
//! object: they are applied caller-side, once per produced frame, by the app's
//! produce path which reads the enabled list via [`CheatPanelState::enabled_raw_cheats`]
//! (poking RAM mid-`run_frame` would not be deterministic). On any change the
//! per-ROM file is persisted (native); there is no `Nes` re-sync for raw cheats
//! (they have no resident state — an empty enabled list simply pokes nothing,
//! so the no-cheat path is byte-identical).
//!
//! Like Game Genie, raw RAM cheats are a runtime overlay that is NOT captured
//! in the `.rnm` TAS movie stream, so a movie recorded with cheats active will
//! not replay identically with them off (and vice-versa).

use rustynes_core::{GenieCode, Nes};

use crate::cheats::{CheatEntry, RawCheat};

/// Native-only persistence context for the cheat panel: where the per-ROM
/// cheat file lives. Built by the app when a ROM is loaded; `None` on wasm32
/// (no filesystem) or before a ROM is loaded.
#[cfg(not(target_arch = "wasm32"))]
pub struct CheatPersist {
    /// Data directory root (`<data_dir>/cheats/<sha>.toml`).
    pub data_dir: std::path::PathBuf,
    /// SHA-256 of the loaded ROM.
    pub rom_sha256: [u8; 32],
}

/// Persistent state of the cheat panel.
#[derive(Debug, Default)]
pub struct CheatPanelState {
    /// In-memory Game Genie list (source of truth for the UI; mirrored to disk).
    cheats: Vec<CheatEntry>,
    /// Game Genie add-field buffer.
    add_text: String,
    /// Last Game Genie add error (cleared on a successful add).
    error: String,
    /// In-memory raw RAM cheat list (v1.7.0; source of truth for the UI;
    /// mirrored to disk and read by the app's produce path each frame).
    raw: Vec<RawCheat>,
    /// Raw RAM add fields: address (hex), value (hex), compare (hex, optional).
    raw_addr_text: String,
    raw_value_text: String,
    raw_compare_text: String,
    /// Last raw RAM add error (cleared on a successful add).
    raw_error: String,
    /// v1.7.0 H9 — Game Genie ENCODER fields: PRG address (hex, $8000-$FFFF),
    /// data byte (hex), optional compare byte (hex). The inverse of the decoder
    /// (`crate::genie_encode`): produce a code from a known substitution.
    enc_addr_text: String,
    enc_data_text: String,
    enc_compare_text: String,
    /// The last successfully-encoded code (display + "Add to list" source).
    enc_result: String,
    /// Last encoder error (cleared on a successful encode).
    enc_error: String,
}

impl CheatPanelState {
    /// Replace the in-memory cheat lists (called by the app after loading a
    /// ROM's persisted cheats). Clears the add-fields and errors. Native-only —
    /// the wasm32 panel starts empty (no per-ROM persistence to seed from).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn set_cheats(&mut self, cheats: Vec<CheatEntry>, raw: Vec<RawCheat>) {
        self.cheats = cheats;
        self.raw = raw;
        self.add_text.clear();
        self.error.clear();
        self.raw_addr_text.clear();
        self.raw_value_text.clear();
        self.raw_compare_text.clear();
        self.raw_error.clear();
        self.enc_addr_text.clear();
        self.enc_data_text.clear();
        self.enc_compare_text.clear();
        self.enc_result.clear();
        self.enc_error.clear();
    }

    /// The currently-ENABLED raw RAM cheats, cloned for the app's produce
    /// path. Pulled once per pacer iteration (like the fps / movie readouts)
    /// so the per-frame poke loop reads the live edited list without threading
    /// the panel through the produce call stack. Empty when nothing is enabled,
    /// so the no-cheat path stays free of any `poke_ram` calls.
    #[must_use]
    pub fn enabled_raw_cheats(&self) -> Vec<RawCheat> {
        self.raw.iter().filter(|c| c.enabled).cloned().collect()
    }

    /// v1.0.0 (UX3 BUG-3) — push the panel's enabled Game Genie codes into
    /// `nes`, clearing any stale ones first. Called by the app after a Reset /
    /// Power-Cycle (and on ROM load) so the live core reflects the configured
    /// codes even while the panel is closed (the every-frame resync in [`show`]
    /// only runs while the panel is open). An empty enabled set is just a
    /// `clear_genie_codes` on an already-empty map — byte-identical no-cheat path.
    pub fn reapply_to_nes(&mut self, nes: &mut Nes) {
        resync_nes(self, nes);
    }
}

/// Render the cheat panel. Re-syncs `nes` and persists (via `persist`, native
/// only) whenever the cheat set changes.
#[cfg(not(target_arch = "wasm32"))]
pub fn show(
    ctx: &egui::Context,
    open: &mut bool,
    state: &mut CheatPanelState,
    nes: &mut Nes,
    persist: Option<&CheatPersist>,
    rom_crc: Option<u32>,
) {
    let mut changed = false;
    egui::Window::new("Cheats (Game Genie)")
        .open(open)
        .default_pos([560.0, 64.0])
        .default_size([420.0, 380.0])
        .resizable(true)
        .show(ctx, |ui| {
            changed = body(ui, state, rom_crc);
        });
    // v1.0.0 (UX3 BUG-3) — re-sync the live core to the panel's enabled set on
    // EVERY frame the panel is open, not just when the list `changed`. The core
    // could have silently lost the codes between edits (a Reset / Power-Cycle, a
    // fresh ROM load that built a new `Nes`, or any future state swap), in which
    // case the `changed`-only resync never re-applied them and the codes "did
    // nothing". An every-frame resync is cheap (a `clear` + a few `BTreeMap`
    // inserts over the typically tiny enabled set) and makes the live core always
    // reflect what the panel shows. With no enabled codes this is just a
    // `clear_genie_codes` on an already-empty map, so the no-cheat PRG-read path
    // stays byte-identical. Persistence still only fires on an actual change.
    resync_nes(state, nes);
    if changed {
        persist_cheats(state, persist);
    }
}

/// wasm32 variant: identical UI + `Nes` re-sync, but no filesystem
/// persistence (the `persist` argument is absent).
#[cfg(target_arch = "wasm32")]
pub fn show(
    ctx: &egui::Context,
    open: &mut bool,
    state: &mut CheatPanelState,
    nes: &mut Nes,
    rom_crc: Option<u32>,
) {
    egui::Window::new("Cheats (Game Genie)")
        .open(open)
        .default_pos([560.0, 64.0])
        .default_size([420.0, 380.0])
        .resizable(true)
        .show(ctx, |ui| {
            let _ = body(ui, state, rom_crc);
        });
    // v1.0.0 (UX3 BUG-3) — every-frame resync (see the native variant above).
    resync_nes(state, nes);
}

/// The window body. Returns `true` if the cheat set changed this frame.
fn body(ui: &mut egui::Ui, state: &mut CheatPanelState, rom_crc: Option<u32>) -> bool {
    let mut changed = false;

    ui.horizontal(|ui| {
        let r = ui.add(
            egui::TextEdit::singleline(&mut state.add_text)
                .desired_width(140.0)
                .hint_text("SXIOPO"),
        );
        let submit = ui.button("Add").clicked()
            || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
        if submit {
            changed |= try_add(state);
        }
    });

    // v1.2.0 Workstream D (D3) — Game Genie code-name database pick-list for the
    // loaded ROM. Pure frontend: a chosen code is appended through the same
    // validated path as a typed code (`add_code_by_str`), feeding the existing
    // `GenieCode` decode + persistence. The list only appears when the loaded
    // ROM's CRC matches a database entry, so it never clutters an unknown ROM.
    changed |= genie_db_picklist(ui, state, rom_crc);

    if !state.error.is_empty() {
        ui.colored_label(
            egui::Color32::from_rgb(0xE0, 0x40, 0x40),
            state.error.clone(),
        );
    }

    ui.separator();
    if state.cheats.is_empty() {
        ui.label("No Game Genie cheats. Enter a 6- or 8-character code above.");
    } else {
        let mut remove: Option<usize> = None;
        egui::Grid::new("cheat-grid")
            .num_columns(4)
            .striped(true)
            .show(ui, |ui| {
                ui.label("On");
                ui.label("Code");
                ui.label("Effect");
                ui.label("");
                ui.end_row();
                for (i, entry) in state.cheats.iter_mut().enumerate() {
                    if ui.checkbox(&mut entry.enabled, "").changed() {
                        changed = true;
                    }
                    ui.monospace(&entry.code);
                    ui.monospace(decode_label(&entry.code));
                    if ui.button("\u{2715}").clicked() {
                        remove = Some(i);
                    }
                    ui.end_row();
                }
            });
        if let Some(i) = remove {
            state.cheats.remove(i);
            changed = true;
        }
    }

    ui.add_space(8.0);
    egui::CollapsingHeader::new("Game Genie encoder")
        .default_open(false)
        .show(ui, |ui| {
            changed |= encoder_body(ui, state);
        });

    ui.add_space(8.0);
    ui.heading("RAM cheats");
    changed |= raw_body(ui, state);

    changed
}

/// v1.7.0 H9 — the Game Genie ENCODER section: turn a known
/// `(address, data[, compare])` substitution into a canonical code (the
/// inverse of the decoder). Returns `true` if a generated code was added to the
/// Game Genie list. A power user who found the value with a RAM search / hex
/// editor can author the code without a code-book lookup.
fn encoder_body(ui: &mut egui::Ui, state: &mut CheatPanelState) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label("Addr $");
        ui.add(
            egui::TextEdit::singleline(&mut state.enc_addr_text)
                .desired_width(56.0)
                .hint_text("91D9"),
        );
        ui.label("=");
        ui.add(
            egui::TextEdit::singleline(&mut state.enc_data_text)
                .desired_width(40.0)
                .hint_text("AD"),
        );
        ui.label("if");
        ui.add(
            egui::TextEdit::singleline(&mut state.enc_compare_text)
                .desired_width(40.0)
                .hint_text("(any)"),
        );
        if ui.button("Encode").clicked() {
            encode_from_fields(state);
        }
    });
    if !state.enc_error.is_empty() {
        ui.colored_label(
            egui::Color32::from_rgb(0xE0, 0x40, 0x40),
            state.enc_error.clone(),
        );
    }
    if !state.enc_result.is_empty() {
        ui.horizontal(|ui| {
            ui.monospace(state.enc_result.clone());
            if ui.button("Add to list").clicked() {
                state.enc_result.clone_into(&mut state.add_text);
                changed |= try_add(state);
            }
        });
    }
    ui.label(
        egui::RichText::new(
            "Address is the PRG byte the code substitutes ($8000-$FFFF). With a \
             compare byte you get an 8-character (bank-specific) code.",
        )
        .weak(),
    );
    changed
}

/// Parse the encoder fields and produce a code into `state.enc_result` (or an
/// error into `state.enc_error`). Address must be `$8000-$FFFF`; the data +
/// optional compare are single hex bytes.
fn encode_from_fields(state: &mut CheatPanelState) {
    state.enc_error.clear();
    let addr_str = state.enc_addr_text.trim().trim_start_matches('$');
    let data_str = state.enc_data_text.trim();
    let cmp_str = state.enc_compare_text.trim();

    let Ok(addr) = u16::from_str_radix(addr_str, 16) else {
        state.enc_error = "address must be hex".to_string();
        return;
    };
    if addr < 0x8000 {
        state.enc_error = "address must be in $8000-$FFFF".to_string();
        return;
    }
    let Ok(data) = u8::from_str_radix(data_str, 16) else {
        state.enc_error = "data must be a hex byte".to_string();
        return;
    };
    if cmp_str.is_empty() {
        state.enc_result = crate::genie_encode::encode_6(addr, data);
    } else if let Ok(cmp) = u8::from_str_radix(cmp_str, 16) {
        state.enc_result = crate::genie_encode::encode_8(addr, data, cmp);
    } else {
        state.enc_error = "compare must be a hex byte (or blank)".to_string();
        state.enc_result = String::new();
    }
}

/// The raw RAM cheat section of the window body. Returns `true` if the raw
/// cheat set changed this frame.
fn raw_body(ui: &mut egui::Ui, state: &mut CheatPanelState) -> bool {
    let mut changed = false;

    ui.horizontal(|ui| {
        ui.label("Addr");
        ui.add(
            egui::TextEdit::singleline(&mut state.raw_addr_text)
                .desired_width(56.0)
                .hint_text("0042"),
        );
        ui.label("=");
        ui.add(
            egui::TextEdit::singleline(&mut state.raw_value_text)
                .desired_width(40.0)
                .hint_text("0A"),
        );
        ui.label("if");
        ui.add(
            egui::TextEdit::singleline(&mut state.raw_compare_text)
                .desired_width(40.0)
                .hint_text("(any)"),
        );
        if ui.button("Add").clicked() {
            changed |= try_add_raw(state);
        }
    });
    if !state.raw_error.is_empty() {
        ui.colored_label(
            egui::Color32::from_rgb(0xE0, 0x40, 0x40),
            state.raw_error.clone(),
        );
    }

    ui.separator();
    if state.raw.is_empty() {
        ui.label("No RAM cheats. Enter a hex address ($0000-$1FFF) and value above.");
        return changed;
    }

    let mut remove: Option<usize> = None;
    egui::Grid::new("raw-cheat-grid")
        .num_columns(3)
        .striped(true)
        .show(ui, |ui| {
            ui.label("On");
            ui.label("Effect");
            ui.label("");
            ui.end_row();
            for (i, entry) in state.raw.iter_mut().enumerate() {
                if ui.checkbox(&mut entry.enabled, "").changed() {
                    changed = true;
                }
                ui.monospace(raw_label(entry));
                if ui.button("\u{2715}").clicked() {
                    remove = Some(i);
                }
                ui.end_row();
            }
        });
    if let Some(i) = remove {
        state.raw.remove(i);
        changed = true;
    }

    changed
}

/// Validate the add-field code via [`GenieCode::new`] and append it. Returns
/// `true` on success (caller re-syncs + persists). On failure, sets the inline
/// error text and returns `false`.
fn try_add(state: &mut CheatPanelState) -> bool {
    let raw = state.add_text.trim().to_string();
    if raw.is_empty() {
        return false;
    }
    let added = add_code_by_str(state, &raw);
    if added {
        state.add_text.clear();
    }
    added
}

/// Validate `raw` via [`GenieCode::new`] and append it (canonical form),
/// deduplicating against the existing list. Returns `true` if added. On failure
/// (invalid or duplicate) sets the inline error and returns `false`. Shared by
/// the typed-code add field and the database pick-list.
fn add_code_by_str(state: &mut CheatPanelState, raw: &str) -> bool {
    match GenieCode::new(raw) {
        Ok(code) => {
            let canonical = code.code().to_string();
            if state.cheats.iter().any(|c| c.code == canonical) {
                state.error = format!("{canonical} is already in the list");
                return false;
            }
            state.cheats.push(CheatEntry {
                code: canonical,
                enabled: true,
            });
            state.error.clear();
            true
        }
        Err(e) => {
            state.error = e.to_string();
            false
        }
    }
}

/// v1.2.0 Workstream D (D3) — render the Game Genie code-name database pick-list
/// for the loaded ROM (by CRC32). Shows nothing when the ROM has no CRC or no DB
/// match. Selecting a row appends that code through [`add_code_by_str`] (the
/// same validated path as a typed code). Returns `true` if the cheat set
/// changed.
fn genie_db_picklist(ui: &mut egui::Ui, state: &mut CheatPanelState, rom_crc: Option<u32>) -> bool {
    let Some(crc) = rom_crc else {
        return false;
    };
    let codes = crate::genie_db::codes_for_crc(crc);
    if codes.is_empty() {
        return false;
    }
    let mut changed = false;
    let game = crate::genie_db::game_for_crc(crc).unwrap_or_default();
    ui.horizontal(|ui| {
        ui.label("Known codes:");
        egui::ComboBox::from_id_salt("genie-db-picklist")
            .selected_text(if game.is_empty() {
                "Pick a code…".to_string()
            } else {
                format!("{game} — pick a code…")
            })
            .show_ui(ui, |ui| {
                for entry in &codes {
                    // Mark already-added codes so the list reads as a checklist.
                    let already = state.cheats.iter().any(|c| c.code == entry.code);
                    let label = if already {
                        format!("\u{2713} {} ({})", entry.name, entry.code)
                    } else {
                        format!("{} ({})", entry.name, entry.code)
                    };
                    if ui.selectable_label(already, label).clicked() && !already {
                        changed |= add_code_by_str(state, &entry.code);
                    }
                }
            });
    });
    changed
}

/// Validate the raw-cheat add fields and append the entry. Returns `true` on
/// success (caller persists). On failure, sets the inline error text and
/// returns `false`.
///
/// Address and value are parsed as hex; the compare field is optional (blank =
/// no compare). The address must be `< 0x2000` (CPU work RAM; the core no-ops
/// outside that range but we reject it up-front so the user gets feedback).
fn try_add_raw(state: &mut CheatPanelState) -> bool {
    let addr_text = state.raw_addr_text.trim();
    let value_text = state.raw_value_text.trim();
    let compare_text = state.raw_compare_text.trim();

    let Ok(address) = u16::from_str_radix(addr_text, 16) else {
        state.raw_error = format!("invalid hex address '{addr_text}'");
        return false;
    };
    if address >= 0x2000 {
        state.raw_error = format!("address ${address:04X} out of range (must be $0000-$1FFF)");
        return false;
    }
    let Ok(value) = u8::from_str_radix(value_text, 16) else {
        state.raw_error = format!("invalid hex value '{value_text}'");
        return false;
    };
    let compare = if compare_text.is_empty() {
        None
    } else if let Ok(c) = u8::from_str_radix(compare_text, 16) {
        Some(c)
    } else {
        state.raw_error = format!("invalid hex compare '{compare_text}'");
        return false;
    };

    state.raw.push(RawCheat {
        address,
        value,
        compare,
        enabled: true,
    });
    state.raw_addr_text.clear();
    state.raw_value_text.clear();
    state.raw_compare_text.clear();
    state.raw_error.clear();
    true
}

/// A read-only "$ADDR = $VV" (or "$ADDR = $VV if $CC") label for a raw cheat.
fn raw_label(entry: &RawCheat) -> String {
    entry.compare.map_or_else(
        || format!("${:04X} = ${:02X}", entry.address, entry.value),
        |cmp| {
            format!(
                "${:04X} = ${:02X} if ${cmp:02X}",
                entry.address, entry.value
            )
        },
    )
}

/// A read-only "addr=data" (or "addr=data cmp" for 8-char) label decoded from
/// `code`. Falls back to a dash if the code no longer decodes (shouldn't
/// happen — entries are only stored after a successful decode).
fn decode_label(code: &str) -> String {
    GenieCode::new(code).map_or_else(
        |_| "-".to_string(),
        |g| {
            g.compare().map_or_else(
                || format!("${:04X} = ${:02X}", g.addr(), g.data()),
                |cmp| format!("${:04X} = ${:02X} if ${cmp:02X}", g.addr(), g.data()),
            )
        },
    )
}

/// Re-sync the running `Nes` to the in-memory cheat list: clear all codes,
/// then add back every ENABLED entry. A failed `add_genie_code` (shouldn't
/// happen — entries are pre-validated) disables that entry and surfaces the
/// error inline.
fn resync_nes(state: &mut CheatPanelState, nes: &mut Nes) {
    nes.clear_genie_codes();
    for entry in &mut state.cheats {
        if !entry.enabled {
            continue;
        }
        if let Err(e) = nes.add_genie_code(&entry.code) {
            state.error = format!("{}: {e}", entry.code);
            entry.enabled = false;
        }
    }
}

/// Persist the in-memory cheat lists (Game Genie + raw RAM) to the per-ROM
/// file (native only).
#[cfg(not(target_arch = "wasm32"))]
fn persist_cheats(state: &CheatPanelState, persist: Option<&CheatPersist>) {
    if let Some(p) = persist {
        crate::cheats::save(&p.data_dir, &p.rom_sha256, &state.cheats, &state.raw);
    }
}
