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
) {
    let mut changed = false;
    egui::Window::new("Cheats (Game Genie)")
        .open(open)
        .default_pos([560.0, 64.0])
        .default_size([420.0, 380.0])
        .resizable(true)
        .show(ctx, |ui| {
            changed = body(ui, state);
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
pub fn show(ctx: &egui::Context, open: &mut bool, state: &mut CheatPanelState, nes: &mut Nes) {
    egui::Window::new("Cheats (Game Genie)")
        .open(open)
        .default_pos([560.0, 64.0])
        .default_size([420.0, 380.0])
        .resizable(true)
        .show(ctx, |ui| {
            let _ = body(ui, state);
        });
    // v1.0.0 (UX3 BUG-3) — every-frame resync (see the native variant above).
    resync_nes(state, nes);
}

/// The window body. Returns `true` if the cheat set changed this frame.
fn body(ui: &mut egui::Ui, state: &mut CheatPanelState) -> bool {
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
    ui.heading("RAM cheats");
    changed |= raw_body(ui, state);

    changed
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
    let raw = state.add_text.trim();
    if raw.is_empty() {
        return false;
    }
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
            state.add_text.clear();
            state.error.clear();
            true
        }
        Err(e) => {
            state.error = e.to_string();
            false
        }
    }
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
