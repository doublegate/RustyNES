//! v1.4.0 Workstream E2 — the browser Save-States manager (wasm32).
//!
//! The wasm analogue of the native `save_states_ui` thumbnail grid. Because
//! `IndexedDB` reads are asynchronous (see [`crate::wasm_idb`]), the grid
//! can't synchronously read each slot inside the egui pass the way the native
//! manager reads the filesystem. Instead:
//!
//! 1. Opening the window (File -> Save States) kicks off an async
//!    [`crate::wasm_idb::scan_slots`] task via `spawn_local`.
//! 2. That task writes the per-slot metadata (occupied + 128x120 RGBA
//!    thumbnail) into a thread-local [`STATE`].
//! 3. The egui pass renders tiles from that snapshot every frame, uploading
//!    each thumbnail to an `egui` texture once (cached by slot).
//! 4. Save / Load clicks queue a [`SlotRequest`] the `App` drains after the
//!    egui pass and routes through the same `IndexedDB` save/load path the
//!    F1/F4 hotkeys use.
//!
//! All state is a thread-local (the browser is single-threaded), matching the
//! `wasm_touch` / `wasm_audio` pattern, so the `App` struct stays untouched.
//! Native never compiles this module; the desktop manager is unaffected.

use std::cell::RefCell;

use crate::wasm_idb::SlotMeta;

/// Slots the browser manager exposes (matches the native `MANAGED_SLOTS`).
pub const MANAGED_SLOTS: u8 = 8;

/// A Save / Load action the user clicked in the grid this frame, drained by
/// the `App` and routed through the `IndexedDB` save/load path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotRequest {
    /// Save the current running state to the given slot.
    Save(u8),
    /// Load the given slot into the running `Nes`.
    Load(u8),
}

/// One slot's render state: occupancy, the raw thumbnail (until uploaded),
/// and the cached `egui` texture once uploaded.
#[derive(Default)]
struct SlotView {
    occupied: bool,
    /// Raw 128x120 RGBA thumbnail, consumed once into `texture`.
    thumbnail: Option<Vec<u8>>,
    texture: Option<egui::TextureHandle>,
}

/// Window + grid state.
#[derive(Default)]
struct State {
    open: bool,
    /// Save-mode toggle: clicking a tile saves (on) or loads (off).
    save_mode: bool,
    /// `true` while an async slot scan is in flight (drives a spinner label).
    scanning: bool,
    /// Per-slot views; rebuilt from an async scan on open.
    slots: Vec<SlotView>,
    /// Pending request the app drains after the egui pass.
    request: Option<SlotRequest>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

/// Open the manager and kick off an async `IndexedDB` slot scan for the given
/// ROM. Called from the `OpenSaveStates` menu action. No-op (logs) if no ROM.
pub fn open(rom_sha256: Option<[u8; 32]>) {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.open = true;
        s.slots.clear();
        s.scanning = rom_sha256.is_some();
    });
    let Some(sha) = rom_sha256 else {
        return;
    };
    wasm_bindgen_futures::spawn_local(async move {
        let metas = crate::wasm_idb::scan_slots(sha, MANAGED_SLOTS).await;
        set_scan_result(metas);
    });
}

/// Store the async scan result into the thread-local for the egui pass.
fn set_scan_result(metas: Vec<SlotMeta>) {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.slots = metas
            .into_iter()
            .map(|m| SlotView {
                occupied: m.occupied,
                thumbnail: m.thumbnail,
                texture: None,
            })
            .collect();
        s.scanning = false;
    });
}

/// Drain a pending Save / Load request (called by the `App` after the egui
/// pass). Returns `None` if nothing was clicked.
pub fn take_request() -> Option<SlotRequest> {
    STATE.with(|s| s.borrow_mut().request.take())
}

/// Render the manager window (called from the wasm egui `extra` closure each
/// frame). `active_slot` is highlighted; `rom_loaded` gates the buttons.
pub fn show(ctx: &egui::Context, active_slot: u8, rom_loaded: bool) {
    let is_open = STATE.with(|s| s.borrow().open);
    if !is_open {
        return;
    }
    let mut open = true;
    egui::Window::new("Save States")
        .open(&mut open)
        .resizable(true)
        .default_width(440.0)
        .show(ctx, |ui| {
            STATE.with(|s| {
                let mut s = s.borrow_mut();
                ui.horizontal(|ui| {
                    ui.checkbox(&mut s.save_mode, "Save mode");
                    let hint = if s.save_mode {
                        "(clicking a tile SAVES to it)"
                    } else {
                        "(clicking a tile LOADS it)"
                    };
                    ui.weak(hint);
                });
                if s.scanning {
                    ui.weak("Scanning IndexedDB slots...");
                }
                ui.separator();
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        egui::Grid::new("wasm-save-states-grid")
                            .num_columns(2)
                            .spacing([12.0, 12.0])
                            .show(ui, |ui| {
                                for slot in 0..MANAGED_SLOTS {
                                    tile_ui(ui, ctx, &mut s, slot, active_slot, rom_loaded);
                                    if slot % 2 == 1 {
                                        ui.end_row();
                                    }
                                }
                            });
                    });
            });
        });
    STATE.with(|s| s.borrow_mut().open = open);
}

/// Render one slot tile (thumbnail + label + Save/Load buttons).
fn tile_ui(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    state: &mut State,
    slot: u8,
    active_slot: u8,
    rom_loaded: bool,
) {
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
            // Lazily upload the thumbnail texture once (cached on the view).
            let view = state.slots.get_mut(slot as usize);
            let occupied = view.as_ref().is_some_and(|v| v.occupied);
            if let Some(view) = view
                && view.texture.is_none()
                && let Some(rgba) = view.thumbnail.take()
            {
                view.texture = upload_thumbnail(ctx, slot, &rgba);
            }
            let desired = egui::vec2(128.0, 120.0);
            let texture = state
                .slots
                .get(slot as usize)
                .and_then(|v| v.texture.as_ref());
            if let Some(tex) = texture {
                ui.add(egui::Image::new(tex).fit_to_exact_size(desired));
            } else {
                let (rect, _) = ui.allocate_exact_size(desired, egui::Sense::hover());
                ui.painter()
                    .rect_filled(rect, 4.0, egui::Color32::from_gray(28));
                let label = if occupied { "(no thumbnail)" } else { "Empty" };
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    label,
                    egui::FontId::proportional(13.0),
                    egui::Color32::GRAY,
                );
            }
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(rom_loaded, egui::Button::new("Save"))
                    .clicked()
                {
                    state.request = Some(SlotRequest::Save(slot));
                }
                if ui
                    .add_enabled(rom_loaded && occupied, egui::Button::new("Load"))
                    .clicked()
                {
                    state.request = Some(SlotRequest::Load(slot));
                }
            });
        });
    });
}

/// Decode a 128x120 RGBA8 thumbnail into an `egui` texture. `None` if the
/// byte count doesn't match the expected dimensions.
fn upload_thumbnail(ctx: &egui::Context, slot: u8, rgba: &[u8]) -> Option<egui::TextureHandle> {
    let w = rustynes_core::THUMBNAIL_WIDTH;
    let h = rustynes_core::THUMBNAIL_HEIGHT;
    if rgba.len() != w * h * 4 {
        return None;
    }
    let image = egui::ColorImage::from_rgba_unmultiplied([w, h], rgba);
    Some(ctx.load_texture(
        format!("wasm-save-state-thumb-{slot}"),
        image,
        egui::TextureOptions::NEAREST,
    ))
}
