//! v1.0.0 — the Save-States manager window (native).
//!
//! A grid of the per-ROM save-state slots. Each tile shows the slot's 128x120
//! thumbnail, the slot number, the save timestamp, and Save / Load buttons;
//! the active slot is highlighted.
//!
//! The core already captures the thumbnail in the `THM ` section of every
//! `.rns` blob and exposes
//! [`rustynes_core::Nes::extract_thumbnail`](rustynes_core::Nes::extract_thumbnail)
//! to read it without restoring; this module only SURFACES it.
//!
//! Native-only: the slot files live on the filesystem
//! (`<data_dir>/saves/<rom_sha256_hex>/slotN.rns`). On wasm the slots live in
//! `localStorage` (base64) and this window is not built — the existing wasm
//! save/load (`F1`/`F4`) path is left untouched.
//!
//! Texture lifetime: each thumbnail is decoded into an [`egui::ColorImage`]
//! and uploaded once via `ctx.load_texture`, then CACHED keyed by slot. The
//! cache is invalidated for a slot when it is (re)saved, dropped wholesale on
//! ROM change, and lazily (re)built on window open — never per frame.

use std::path::{Path, PathBuf};

use crate::save_state::{self, NUM_SLOTS};

/// The slot range the manager covers (slots labelled 1-8).
///
/// The File menu's Save/Load-to-Slot submenus and the active-slot selector
/// both range over 0..8, so the manager matches that range (NOT the full
/// `NUM_SLOTS == 10` the on-disk format allows) to stay consistent.
pub const MANAGED_SLOTS: u8 = 8;

const _: () = assert!(MANAGED_SLOTS <= NUM_SLOTS);

/// A Save / Load action the user requested in the manager this frame, drained
/// by the app and routed through its existing `handle_save_state` /
/// `handle_load_state`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveStateRequest {
    /// Overwrite the given slot from the current running state.
    Save(u8),
    /// Restore the given slot into the running `Nes`.
    Load(u8),
}

/// One cached slot tile: the uploaded thumbnail texture (if any) + the
/// human-readable save time, refreshed when the window opens / the slot is
/// saved.
struct SlotTile {
    /// Uploaded thumbnail handle, or `None` for an empty / thumbnail-less slot.
    texture: Option<egui::TextureHandle>,
    /// Whether a slot file exists at all (drives "Empty" vs the placeholder).
    occupied: bool,
    /// Pre-formatted "saved N ago" / absolute time label.
    when: String,
}

/// Save-States manager window state. Held by `App`; rendered inside the egui
/// pass each frame when [`Self::open`] is set.
#[derive(Default)]
pub struct SaveStatesUi {
    /// Whether the window is shown.
    pub open: bool,
    /// Save-mode toggle: when on, a tile click saves to that slot; off = load.
    /// (The explicit per-tile Save / Load buttons work regardless.)
    save_mode: bool,
    /// Per-slot cached tiles (`None` until the cache is (re)built on open).
    tiles: Vec<Option<SlotTile>>,
    /// The ROM hash the cache was built for; a change drops every texture.
    cached_for: Option<[u8; 32]>,
    /// A pending Save / Load the user clicked this frame (drained by the app).
    request: Option<SaveStateRequest>,
}

impl SaveStatesUi {
    /// Open the window and (re)build the slot cache for `rom_sha256`.
    pub fn open(
        &mut self,
        ctx: &egui::Context,
        data_dir: Option<&Path>,
        rom_sha256: Option<[u8; 32]>,
    ) {
        self.open = true;
        self.rebuild(ctx, data_dir, rom_sha256);
    }

    /// Drop every cached texture (called on ROM change to avoid leaking the
    /// old game's thumbnails) — the next open rebuilds.
    pub fn invalidate_all(&mut self) {
        self.tiles.clear();
        self.cached_for = None;
    }

    /// Invalidate a single slot's cached texture (called after that slot is
    /// (re)saved) so the next render reloads its fresh thumbnail.
    pub fn invalidate_slot(&mut self, slot: u8) {
        if let Some(tile) = self.tiles.get_mut(slot as usize) {
            *tile = None;
        }
    }

    /// Return (and clear) the pending Save / Load request, if any.
    pub const fn take_request(&mut self) -> Option<SaveStateRequest> {
        self.request.take()
    }

    /// Rebuild the slot cache (thumbnails + timestamps) for the current ROM.
    /// Drops textures wholesale when the ROM changed.
    fn rebuild(
        &mut self,
        ctx: &egui::Context,
        data_dir: Option<&Path>,
        rom_sha256: Option<[u8; 32]>,
    ) {
        if self.cached_for != rom_sha256 {
            self.tiles.clear();
            self.cached_for = rom_sha256;
        }
        self.tiles.resize_with(MANAGED_SLOTS as usize, || None);
        let (Some(dir), Some(sha)) = (data_dir, rom_sha256.as_ref()) else {
            // No data dir / no ROM: every tile is empty.
            for t in &mut self.tiles {
                *t = Some(SlotTile {
                    texture: None,
                    occupied: false,
                    when: "Empty".into(),
                });
            }
            return;
        };
        for slot in 0..MANAGED_SLOTS {
            // Only (re)build a slot that has no cached tile (keeps the
            // per-frame cost zero on a window that is merely open).
            if self.tiles[slot as usize].is_some() {
                continue;
            }
            let tile = match save_state::slot_meta(dir, sha, slot) {
                Ok(Some(meta)) => {
                    let texture = meta
                        .thumbnail
                        .as_deref()
                        .and_then(|rgba| upload_thumbnail(ctx, slot, rgba));
                    SlotTile {
                        texture,
                        occupied: true,
                        when: format_modified(meta.modified),
                    }
                }
                _ => SlotTile {
                    texture: None,
                    occupied: false,
                    when: "Empty".into(),
                },
            };
            self.tiles[slot as usize] = Some(tile);
        }
    }

    /// Render the window. `active_slot` is highlighted; clicks queue a request
    /// (drained via [`Self::take_request`]) for the app to act on after the
    /// egui pass.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        data_dir: Option<&Path>,
        rom_sha256: Option<[u8; 32]>,
        active_slot: u8,
        rom_loaded: bool,
    ) {
        if !self.open {
            return;
        }
        // Re-resolve the cache if the ROM changed since it was built (e.g. the
        // user loaded a different game with the window left open).
        if self.cached_for != rom_sha256 {
            self.rebuild(ctx, data_dir, rom_sha256);
        }
        let mut open = self.open;
        egui::Window::new("Save States")
            .open(&mut open)
            .resizable(true)
            .default_width(440.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.save_mode, "Save mode");
                    ui.weak(if self.save_mode {
                        "(clicking a tile SAVES to it)"
                    } else {
                        "(clicking a tile LOADS it)"
                    });
                });
                ui.separator();
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        egui::Grid::new("save-states-grid")
                            .num_columns(2)
                            .spacing([12.0, 12.0])
                            .show(ui, |ui| {
                                for slot in 0..MANAGED_SLOTS {
                                    self.tile_ui(ui, slot, active_slot, rom_loaded);
                                    if slot % 2 == 1 {
                                        ui.end_row();
                                    }
                                }
                            });
                    });
            });
        self.open = open;
    }

    /// Render a single slot tile (thumbnail + label + Save/Load buttons).
    fn tile_ui(&mut self, ui: &mut egui::Ui, slot: u8, active_slot: u8, rom_loaded: bool) {
        let tile = self.tiles.get(slot as usize).and_then(Option::as_ref);
        let active = slot == active_slot;
        let frame = egui::Frame::group(ui.style()).stroke(if active {
            egui::Stroke::new(2.0, egui::Color32::from_rgb(240, 200, 100))
        } else {
            ui.visuals().widgets.noninteractive.bg_stroke
        });
        frame.show(ui, |ui| {
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.strong(format!("Slot {}", slot + 1));
                    if active {
                        ui.colored_label(egui::Color32::from_rgb(240, 200, 100), "(active)");
                    }
                });
                // Thumbnail (or a placeholder sized like one so the grid is
                // even). The 128x120 source is shown at 2x for visibility.
                let desired = egui::vec2(128.0, 120.0);
                if let Some(tex) = tile.and_then(|t| t.texture.as_ref()) {
                    ui.add(egui::Image::new(tex).fit_to_exact_size(desired));
                } else {
                    let (rect, _) = ui.allocate_exact_size(desired, egui::Sense::hover());
                    ui.painter()
                        .rect_filled(rect, 4.0, egui::Color32::from_gray(28));
                    let label = if tile.is_some_and(|t| t.occupied) {
                        "(no thumbnail)"
                    } else {
                        "Empty"
                    };
                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        label,
                        egui::FontId::proportional(13.0),
                        egui::Color32::GRAY,
                    );
                }
                ui.weak(tile.map_or("Empty", |t| t.when.as_str()).to_string());
                let occupied = tile.is_some_and(|t| t.occupied);
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(rom_loaded, egui::Button::new("Save"))
                        .clicked()
                    {
                        self.request = Some(SaveStateRequest::Save(slot));
                    }
                    if ui
                        .add_enabled(rom_loaded && occupied, egui::Button::new("Load"))
                        .clicked()
                    {
                        self.request = Some(SaveStateRequest::Load(slot));
                    }
                });
            });
        });
    }
}

/// Decode a 128x120 RGBA8 thumbnail into an egui texture. Returns `None` if
/// the byte count doesn't match the expected dimensions (defensive — the core
/// already validates the section).
fn upload_thumbnail(ctx: &egui::Context, slot: u8, rgba: &[u8]) -> Option<egui::TextureHandle> {
    let w = rustynes_core::THUMBNAIL_WIDTH;
    let h = rustynes_core::THUMBNAIL_HEIGHT;
    if rgba.len() != w * h * 4 {
        return None;
    }
    let image = egui::ColorImage::from_rgba_unmultiplied([w, h], rgba);
    Some(ctx.load_texture(
        format!("save-state-thumb-{slot}"),
        image,
        egui::TextureOptions::NEAREST,
    ))
}

/// Format a slot's modification time as a compact "N min ago" / "N h ago" /
/// "N d ago" relative label (no chrono dependency — `SystemTime` math only).
fn format_modified(modified: Option<std::time::SystemTime>) -> String {
    let Some(modified) = modified else {
        return "Saved".into();
    };
    // A file newer than "now" (clock skew) falls back to "Saved".
    let Ok(elapsed) = std::time::SystemTime::now().duration_since(modified) else {
        return "Saved".into();
    };
    let secs = elapsed.as_secs();
    if secs < 60 {
        "just now".into()
    } else if secs < 3600 {
        format!("{} min ago", secs / 60)
    } else if secs < 86_400 {
        format!("{} h ago", secs / 3600)
    } else {
        format!("{} d ago", secs / 86_400)
    }
}

/// The data-dir slot path, re-exported for the app to log / inspect.
#[allow(dead_code)]
pub fn slot_path_for(data_dir: &Path, rom_sha256: &[u8; 32], slot: u8) -> Option<PathBuf> {
    save_state::slot_path(data_dir, rom_sha256, slot).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn managed_slots_match_the_menu_range() {
        // The File-menu Save/Load-to-Slot submenus range over slots 1-8.
        assert_eq!(MANAGED_SLOTS, 8);
    }

    #[test]
    fn take_request_drains_once() {
        let mut ui = SaveStatesUi {
            request: Some(SaveStateRequest::Load(3)),
            ..Default::default()
        };
        assert_eq!(ui.take_request(), Some(SaveStateRequest::Load(3)));
        assert_eq!(ui.take_request(), None);
    }

    #[test]
    fn invalidate_slot_clears_only_that_tile() {
        let mut ui = SaveStatesUi::default();
        ui.tiles.resize_with(MANAGED_SLOTS as usize, || None);
        // Mark two tiles "present" (texture None is fine for this test).
        for slot in [2usize, 5] {
            ui.tiles[slot] = Some(SlotTile {
                texture: None,
                occupied: true,
                when: "now".into(),
            });
        }
        ui.invalidate_slot(2);
        assert!(ui.tiles[2].is_none(), "slot 2 invalidated");
        assert!(ui.tiles[5].is_some(), "slot 5 untouched");
    }

    #[test]
    fn invalidate_all_drops_cache_and_rom_key() {
        let mut ui = SaveStatesUi {
            cached_for: Some([7u8; 32]),
            ..Default::default()
        };
        ui.tiles.resize_with(3, || None);
        ui.invalidate_all();
        assert!(ui.tiles.is_empty());
        assert!(ui.cached_for.is_none());
    }

    #[test]
    fn format_modified_buckets() {
        let now = std::time::SystemTime::now();
        assert_eq!(format_modified(None), "Saved");
        assert_eq!(
            format_modified(Some(now - std::time::Duration::from_secs(10))),
            "just now"
        );
        assert_eq!(
            format_modified(Some(now - std::time::Duration::from_secs(120))),
            "2 min ago"
        );
        assert_eq!(
            format_modified(Some(now - std::time::Duration::from_secs(7200))),
            "2 h ago"
        );
        assert_eq!(
            format_modified(Some(now - std::time::Duration::from_secs(2 * 86_400))),
            "2 d ago"
        );
    }
}
