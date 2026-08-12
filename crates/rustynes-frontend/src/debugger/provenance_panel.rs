//! Pixel provenance inspector (v2.3.2 "Lucid" phase 3).
//!
//! Pick a screen pixel and read the full causal chain that produced it: the PPU
//! dot and scanline that emitted it, which layer won the priority decision, the
//! nametable / attribute / pattern addresses of the tile actually on screen, the
//! palette entry the color came from — and, for the bytes that have one, **the
//! CPU instruction and cycle that last wrote them**.
//!
//! # Relationship to the other inspectors
//!
//! This is deliberately not a replacement for anything. The Trace Logger answers
//! "what did the CPU do"; the Event Viewer answers "when did this register write
//! land"; the HD Pixel Inspector answers "which replacement rule matched". This
//! panel answers "why is THIS pixel this color", which none of them can, because
//! the edge from a byte in PPU memory back to the instruction that stored it did
//! not exist until `rustynes-ppu`'s `provenance` module recorded it.
//!
//! # Read-only
//!
//! Takes `&Nes`, mutates nothing, and reads only records the core produced as
//! output-only telemetry. Arming and disarming the two stores is the panel's one
//! side effect on the emulator, and both are documented as determinism-neutral:
//! the framebuffer, audio, and cycle counts are bit-identical either way.

use egui::{Color32, Sense, Vec2};

use rustynes_core::Nes;
use rustynes_core::rustynes_ppu::{
    PATTERN_ADDR_NONE, PixelLayer, PixelProvenance, SPRITE_SLOT_NONE, WriteAttrib,
};

use crate::debugger::source_map::SourceMap;
use crate::gfx::{NES_H, NES_W};

/// Persistent panel state.
pub struct ProvenancePanelState {
    /// Pinned screen coordinate the report describes.
    px: u32,
    py: u32,
    /// Mirrors the core's "provenance armed" state so the checkbox is stateful
    /// across frames without querying the `Nes` before it exists.
    prov_armed: bool,
    /// Mirrors the core's "write attribution armed" state.
    attrib_armed: bool,
}

impl Default for ProvenancePanelState {
    fn default() -> Self {
        Self {
            // Screen centre: a pixel that is interesting in most games, and far
            // from the left-edge mask that would otherwise make the first thing
            // a user sees a backdrop pixel with nothing to say.
            px: 128,
            py: 120,
            prov_armed: false,
            attrib_armed: false,
        }
    }
}

/// A small colour swatch plus its NES palette value.
///
/// The swatch is the RAW palette colour. Grayscale / emphasis are reported as
/// text on their own row rather than baked in, because the point of the row is
/// to name the palette entry — a swatch silently darkened by emphasis would not
/// match the entry the user is about to look up.
fn nes_swatch(ui: &mut egui::Ui, color: u8) {
    let rgba = rustynes_core::rustynes_ppu::nes_color_to_rgba(color);
    let (rect, _r) = ui.allocate_exact_size(Vec2::new(18.0, 18.0), Sense::hover());
    ui.painter().rect_filled(
        rect,
        2.0,
        Color32::from_rgba_unmultiplied(rgba[0], rgba[1], rgba[2], rgba[3]),
    );
    ui.monospace(format!("${color:02X}"));
}

/// Render one attribution line: who wrote a byte, and where that instruction is.
///
/// `None` is rendered as an explicit "no record" rather than being skipped —
/// an absent attribution is information (the byte predates arming, or the mapper
/// owns it), and silently omitting the row would read as "nothing wrote this".
fn attribution_row(ui: &mut egui::Ui, label: &str, rec: Option<WriteAttrib>, map: &SourceMap) {
    ui.label(label);
    match rec {
        Some(w) => {
            use std::fmt::Write as _;
            let mut text = format!(
                "PC ${:04X}  cycle {}  wrote ${:02X}",
                w.pc, w.cycle, w.value
            );
            // A source location is a bonus, not a requirement: it only exists
            // when the user has loaded a `.dbg` file for this ROM.
            if let Some(loc) = map.loc(w.pc)
                && let Some(file) = map.file_name(loc.file)
            {
                let _ = write!(text, "  [{file}:{}]", loc.line);
            }
            ui.monospace(text);
        }
        None => {
            ui.weak("no record (written before arming, or mapper-owned)");
        }
    }
    ui.end_row();
}

/// Render the pixel provenance inspector.
pub fn show(
    ctx: &egui::Context,
    detached: &mut std::collections::HashSet<&'static str>,
    open: &mut bool,
    state: &mut ProvenancePanelState,
    nes: &mut Nes,
    source_map: &SourceMap,
) {
    // Arming toggles are applied BEFORE the body renders, so a click takes
    // effect on the frame after next (the core has to run a frame to fill the
    // record). The checkbox therefore reflects intent immediately while the
    // report below still honestly says "waiting for a frame".
    let mut want_prov = state.prov_armed;
    let mut want_attrib = state.attrib_armed;

    super::detachable_window(
        ctx,
        detached,
        "provenance",
        "Pixel Provenance",
        super::WindowCfg {
            default_size: Some([480.0, 560.0]),
            ..Default::default()
        },
        open,
        |ui| {
            ui.horizontal(|ui| {
                ui.checkbox(&mut want_prov, "Per-pixel provenance");
                ui.checkbox(&mut want_attrib, "Write attribution");
            });
            ui.weak(
                "Both are output-only and default off. Arming allocates; emulation \
                 is bit-identical either way.",
            );
            ui.separator();

            ui.horizontal(|ui| {
                ui.label("X");
                ui.add(egui::DragValue::new(&mut state.px).range(0..=(NES_W - 1)));
                ui.label("Y");
                ui.add(egui::DragValue::new(&mut state.py).range(0..=(NES_H - 1)));
            });
            ui.separator();

            let Some(frame) = nes.pixel_provenance() else {
                ui.weak("Enable \"Per-pixel provenance\" above, then run a frame.");
                return;
            };
            let Some(rec) = frame.get(state.px as usize, state.py as usize) else {
                ui.weak("(pixel off-screen)");
                return;
            };
            report(ui, &rec, nes, source_map);
        },
    );

    if want_prov != state.prov_armed {
        state.prov_armed = want_prov;
        nes.set_pixel_provenance(want_prov);
    }
    if want_attrib != state.attrib_armed {
        state.attrib_armed = want_attrib;
        nes.set_write_attribution(want_attrib);
    }
}

/// The per-pixel causal report.
fn report(ui: &mut egui::Ui, rec: &PixelProvenance, nes: &Nes, map: &SourceMap) {
    let attrib = nes.write_attribution();

    // --- What was emitted, and when ---
    ui.heading("Emitted");
    egui::Grid::new("prov_emitted")
        .num_columns(2)
        .striped(true)
        .show(ui, |ui| {
            ui.label("Position");
            ui.monospace(format!(
                "scanline {}  dot {}  (screen x {})",
                rec.scanline,
                rec.dot,
                rec.dot.saturating_sub(1)
            ));
            ui.end_row();

            ui.label("Layer");
            ui.monospace(match rec.layer {
                PixelLayer::Backdrop => "backdrop",
                PixelLayer::Background => "background",
                PixelLayer::Sprite => "sprite",
            });
            ui.end_row();

            ui.label("Colour");
            ui.horizontal(|ui| nes_swatch(ui, rec.color));
            ui.end_row();

            ui.label("Grayscale / emphasis");
            ui.monospace(format!(
                "${:02X}{}",
                rec.color_mask,
                if rec.color_mask & 0x01 != 0 {
                    "  (grayscale)"
                } else {
                    ""
                }
            ));
            ui.end_row();
        });
    ui.separator();

    // --- The palette entry, and who wrote it ---
    ui.heading("Palette");
    egui::Grid::new("prov_palette")
        .num_columns(2)
        .striped(true)
        .show(ui, |ui| {
            ui.label("Address");
            // Pre-mirroring, so the `$3F10` sprite family is shown as written
            // rather than folded onto `$3F00` — the address the program used.
            ui.monospace(format!(
                "${:04X}  (entry {})",
                rec.palette_addr, rec.palette_index
            ));
            ui.end_row();

            attribution_row(
                ui,
                "Last written by",
                attrib.and_then(|a| a.palette(rec.palette_index as usize)),
                map,
            );
        });
    ui.separator();

    // --- The tile that produced the pixel ---
    match rec.layer {
        PixelLayer::Backdrop => {
            ui.weak(
                "Backdrop pixel: neither layer was opaque here, so there is no tile \
                 behind it. The colour above comes straight from the palette.",
            );
        }
        PixelLayer::Background | PixelLayer::Sprite => {
            background_section(ui, rec, nes, map);
        }
    }

    if rec.layer == PixelLayer::Sprite {
        ui.separator();
        sprite_section(ui, rec);
    }
}

/// The background tile's addresses and their attribution.
///
/// Shown even for a sprite pixel, because the background tile underneath is part
/// of the causal story — it is what the sprite won priority *over*.
fn background_section(ui: &mut egui::Ui, rec: &PixelProvenance, nes: &Nes, map: &SourceMap) {
    ui.heading(if rec.layer == PixelLayer::Sprite {
        "Background (behind the sprite)"
    } else {
        "Background tile"
    });
    let attrib = nes.write_attribution();
    let ciram = nes.ciram_offset_for_nametable_addr(rec.nt_addr);

    egui::Grid::new("prov_bg")
        .num_columns(2)
        .striped(true)
        .show(ui, |ui| {
            ui.label("Nametable address");
            ui.monospace(ciram.map_or_else(
                || format!("${:04X}", rec.nt_addr),
                |off| format!("${:04X}  -> CIRAM +${off:03X}", rec.nt_addr),
            ));
            ui.end_row();

            attribution_row(
                ui,
                "Tile byte written by",
                attrib.zip(ciram).and_then(|(a, off)| a.ciram(off)),
                map,
            );

            ui.label("Attribute address");
            let at_ciram = nes.ciram_offset_for_nametable_addr(rec.at_addr);
            ui.monospace(at_ciram.map_or_else(
                || format!("${:04X}", rec.at_addr),
                |off| format!("${:04X}  -> CIRAM +${off:03X}", rec.at_addr),
            ));
            ui.end_row();

            attribution_row(
                ui,
                "Attribute written by",
                attrib.zip(at_ciram).and_then(|(a, off)| a.ciram(off)),
                map,
            );

            ui.label("Palette group");
            ui.monospace(format!("{}", rec.bg_pal));
            ui.end_row();

            ui.label("Pattern bits");
            ui.monospace(format!(
                "{}{}",
                rec.bg_idx,
                if rec.bg_idx == 0 {
                    "  (transparent)"
                } else {
                    ""
                }
            ));
            ui.end_row();

            ui.label("Fine scroll");
            ui.monospace(format!("x {}  y {}", rec.fine_x, rec.fine_y));
            ui.end_row();

            if rec.layer == PixelLayer::Background {
                ui.label("Pattern address");
                ui.monospace(pattern_addr_text(rec.pattern_addr));
                ui.end_row();
            }
        });
    ui.weak(
        "The nametable address is the tile ON SCREEN, not the PPU's current `v` — \
         by display time `v` has advanced two tiles past this pixel.",
    );
}

/// The winning sprite's slot, flags, and OAM attribution.
fn sprite_section(ui: &mut egui::Ui, rec: &PixelProvenance) {
    ui.heading("Sprite (winning layer)");
    egui::Grid::new("prov_sprite")
        .num_columns(2)
        .striped(true)
        .show(ui, |ui| {
            ui.label("Slot");
            ui.monospace(if rec.sprite_slot == SPRITE_SLOT_NONE {
                "(none)".to_string()
            } else {
                format!("{}", rec.sprite_slot)
            });
            ui.end_row();

            ui.label("Priority");
            ui.monospace(if rec.sprite_front {
                "front of background"
            } else {
                "behind background"
            });
            ui.end_row();

            ui.label("Pattern bits");
            ui.monospace(format!("{}  (palette group {})", rec.spr_idx, rec.spr_pal));
            ui.end_row();

            ui.label("Pattern address");
            ui.monospace(pattern_addr_text(rec.pattern_addr));
            ui.end_row();

            ui.label("Sprite 0 opaque here");
            ui.monospace(format!("{}", rec.sprite_zero));
            ui.end_row();

            // NO OAM attribution row here, deliberately.
            //
            // `WriteAttribution::oam` is indexed by PRIMARY OAM byte address,
            // while `sprite_slot` is the per-scanline SECONDARY-OAM slot. Sprite
            // evaluation copies only in-range sprites into secondary OAM, so
            // slot N is not primary sprite N whenever any earlier sprite was
            // skipped — and `slot * 4` would confidently name the instruction
            // that wrote a DIFFERENT sprite.
            //
            // The record already documents that the primary index does not exist
            // at emit time. Using the slot as if it did would be exactly the
            // plausible-but-wrong answer this panel exists to avoid, so the row
            // is omitted until the primary index is actually carried. Caught in
            // review after the caveat below was written and then contradicted.
        });
    ui.weak(
        "The slot is the per-scanline secondary-OAM slot, not the primary OAM \
         sprite number: sprite evaluation does not retain the source index, so the \
         PPU never knows it. Match the slot's tile / attributes against the OAM \
         viewer to identify the sprite. For the same reason there is no \
         \"written by\" row here — the attribution store is keyed on primary OAM, \
         and indexing it with a secondary slot would name the wrong sprite.",
    );
}

/// Format a pattern address, spelling out the "no pattern" sentinel rather than
/// printing `$FFFF` as though it were an address.
fn pattern_addr_text(addr: u16) -> String {
    if addr == PATTERN_ADDR_NONE {
        "(none - backdrop pixel)".to_string()
    } else {
        format!("${addr:04X}")
    }
}
