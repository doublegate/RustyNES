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
//! Memory hex editor — CPU / PPU / OAM domains (v1.6.0 "Studio" Workstream C, C2).
//!
//! Originally (T-53-006) a read-only hex viewer. v1.6.0 turns it into the
//! Mesen2-class **hex editor**:
//!
//! - **In-place poke** — click a byte, type a new hex value, Enter writes it.
//!   Only the CPU work-RAM (`$0000-$1FFF`) is writable (via [`Nes::poke_ram`]);
//!   other regions (PPU bus, OAM, ROM) are read-only because the core exposes
//!   no deterministic poke for them.
//! - **Freeze** — mark a writable byte frozen at its current value; the panel
//!   emits it as a [`RawCheat`] that the app re-applies after every frame
//!   (routed through the existing raw-cheat overlay, like Mesen / FCEUX — see
//!   [`MemoryPanelState::freeze_cheats`]).
//! - **Access-type heatmap** — when enabled, the panel arms the core's
//!   `debug-hooks` per-frame access log and tints each byte by whether it was
//!   read (blue) or written (red) in the last frame. Output-only telemetry; it
//!   never perturbs the deterministic run.
//! - **Find** — search the visible domain for a byte sequence (`DE AD BE EF`)
//!   and jump to the first match at/after the cursor.
//!
//! All reads go through the side-effect-free `cpu_bus_peek` / `ppu_bus_peek` /
//! `oam_byte` API; the only write path is `poke_ram` (CPU RAM), applied like a
//! raw cheat, so the no-edit path is byte-identical and determinism holds.

use std::collections::HashMap;

use egui::Color32;
use rustynes_core::Nes;

use crate::cheats::RawCheat;

/// Which memory domain the editor is viewing.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum Domain {
    /// The CPU bus (`$0000-$FFFF`); only `$0000-$1FFF` work RAM is writable.
    Cpu,
    /// The PPU bus (`$0000-$3FFF`): CHR, nametables, palette. Read-only.
    Ppu,
    /// Object Attribute Memory (sprite RAM, 256 bytes). Read-only.
    Oam,
}

impl Domain {
    const fn label(self) -> &'static str {
        match self {
            Self::Cpu => "CPU bus",
            Self::Ppu => "PPU bus",
            Self::Oam => "OAM",
        }
    }

    /// The highest valid address in this domain (inclusive), as a `u32` so OAM's
    /// 256-byte window is representable distinctly from a 64 KiB bus.
    const fn max_addr(self) -> u32 {
        match self {
            Self::Cpu => 0xFFFF,
            Self::Ppu => 0x3FFF,
            Self::Oam => 0x00FF,
        }
    }

    /// Whether this domain has *any* writable region (only the CPU bus does;
    /// see [`Domain::addr_writable`] for the per-address gate).
    const fn writable(self) -> bool {
        matches!(self, Self::Cpu)
    }

    /// Whether a poke / freeze actually takes effect at `addr` in this domain.
    ///
    /// [`rustynes_core::Nes::poke_ram`] is a no-op outside CPU work RAM
    /// (`$0000-$1FFF`), so only that range is editable — a click on ROM /
    /// mapper / register space would otherwise be a silent, misleading no-op.
    const fn addr_writable(self, addr: u16) -> bool {
        matches!(self, Self::Cpu) && addr <= 0x1FFF
    }
}

/// Heatmap recency for one address: was it read / written in the last frame.
#[derive(Default, Clone, Copy)]
struct AccessFlags {
    read: bool,
    write: bool,
}

/// Persistent state of the memory hex editor.
pub struct MemoryPanelState {
    domain: Domain,
    /// Current origin (16-byte aligned).
    origin: u16,
    /// Text input for go-to.
    goto_text: String,
    /// The address currently being edited (CPU domain only), plus its edit
    /// buffer; `None` when no cell is open for editing.
    editing: Option<(u16, String)>,
    /// Frozen CPU work-RAM addresses → the value to hold. Re-applied each frame
    /// as raw cheats via [`Self::freeze_cheats`].
    frozen: HashMap<u16, u8>,
    /// Whether the access-type heatmap is enabled (arms the core access log).
    heatmap: bool,
    /// Last frame's per-address access flags (CPU domain; `$0000-$FFFF`).
    access: HashMap<u16, AccessFlags>,
    /// Find: the hex byte-sequence search box + the last status line.
    find_text: String,
    find_status: Option<String>,
}

impl Default for MemoryPanelState {
    fn default() -> Self {
        Self {
            domain: Domain::Cpu,
            origin: 0,
            goto_text: String::new(),
            editing: None,
            frozen: HashMap::new(),
            heatmap: false,
            access: HashMap::new(),
            find_text: String::new(),
            find_status: None,
        }
    }
}

const READ_TINT: Color32 = Color32::from_rgb(0x40, 0x80, 0xC0);
const WRITE_TINT: Color32 = Color32::from_rgb(0xC0, 0x50, 0x50);
const FROZEN_TINT: Color32 = Color32::from_rgb(0x50, 0xB0, 0xF0);

impl MemoryPanelState {
    /// Whether the heatmap wants the core's per-frame access log armed.
    #[must_use]
    pub const fn wants_access_log(&self) -> bool {
        self.heatmap
    }

    /// The frozen CPU-RAM bytes as raw cheats, re-applied after every frame by
    /// the app's produce path (merged with the cheat panel's list). Empty when
    /// nothing is frozen, so the no-freeze path stays byte-identical.
    #[must_use]
    pub fn freeze_cheats(&self) -> Vec<RawCheat> {
        self.frozen
            .iter()
            .map(|(&address, &value)| RawCheat {
                address,
                value,
                compare: None,
                enabled: true,
            })
            .collect()
    }

    /// Refresh the heatmap from the just-finished frame's access log. Called by
    /// the app's per-frame pump under the emu lock (observational; reads only).
    pub fn refresh_heatmap(&mut self, nes: &Nes) {
        if !self.heatmap {
            if !self.access.is_empty() {
                self.access.clear();
            }
            return;
        }
        self.access.clear();
        for rec in nes.accesses() {
            let e = self.access.entry(rec.addr).or_default();
            if rec.write {
                e.write = true;
            } else {
                e.read = true;
            }
        }
    }

    fn read_byte(&self, nes: &mut Nes, addr: u16) -> u8 {
        match self.domain {
            Domain::Cpu => nes.cpu_bus_peek(addr),
            Domain::Ppu => nes.ppu_bus_peek(addr),
            Domain::Oam => nes.oam_byte(addr as u8),
        }
    }

    /// Search the current domain for the byte sequence in `find_text` starting
    /// at the cursor (`origin`), wrapping once. Sets `find_status` + moves the
    /// origin to the match.
    fn run_find(&mut self, nes: &mut Nes) {
        let Some(needle) = parse_byte_seq(&self.find_text) else {
            self.find_status = Some("bad byte sequence".to_string());
            return;
        };
        if needle.is_empty() {
            self.find_status = Some("empty search".to_string());
            return;
        }
        let max = self.domain.max_addr();
        let span = max + 1;
        // Scan every start address once, beginning just past the current origin
        // so repeated Find walks forward through matches.
        let start = (u32::from(self.origin) + 1) % span;
        for off in 0..span {
            let base = (start + off) % span;
            if base + needle.len() as u32 > span {
                continue;
            }
            let hit = needle.iter().enumerate().all(|(k, &b)| {
                let a = (base + k as u32) as u16;
                self.read_byte(nes, a) == b
            });
            if hit {
                self.origin = (base as u16) & 0xFFF0;
                self.find_status = Some(format!("found at ${base:04X}"));
                return;
            }
        }
        self.find_status = Some("not found".to_string());
    }
}

pub fn show(ctx: &egui::Context, open: &mut bool, state: &mut MemoryPanelState, nes: &mut Nes) {
    egui::Window::new("Memory")
        .open(open)
        .default_pos([336.0, 480.0])
        .default_size([520.0, 520.0])
        .resizable(true)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                for d in [Domain::Cpu, Domain::Ppu, Domain::Oam] {
                    if ui.selectable_label(state.domain == d, d.label()).clicked()
                        && state.domain != d
                    {
                        state.domain = d;
                        state.editing = None;
                        state.origin = 0;
                    }
                }
                ui.separator();
                ui.label("goto:");
                let r = ui.add(
                    egui::TextEdit::singleline(&mut state.goto_text)
                        .desired_width(56.0)
                        .hint_text("$1234"),
                );
                if r.lost_focus()
                    && ui.input(|i| i.key_pressed(egui::Key::Enter))
                    && let Some(addr) = parse_hex16(&state.goto_text)
                {
                    state.origin = (addr & 0xFFF0).min((state.domain.max_addr() as u16) & 0xFFF0);
                }
                if ui.button("-").clicked() {
                    state.origin = state.origin.wrapping_sub(256);
                }
                if ui.button("+").clicked() {
                    let next = u32::from(state.origin) + 256;
                    if next <= state.domain.max_addr() {
                        state.origin = next as u16;
                    }
                }
            });

            ui.horizontal(|ui| {
                ui.checkbox(&mut state.heatmap, "Access heatmap")
                    .on_hover_text(
                        "Tint bytes by read (blue) / write (red) in the last frame \
                         (CPU bus; arms the debug-hooks access log).",
                    );
                ui.separator();
                ui.label("find:");
                let fr = ui.add(
                    egui::TextEdit::singleline(&mut state.find_text)
                        .desired_width(120.0)
                        .hint_text("DE AD BE EF"),
                );
                let go = (fr.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                    || ui.button("Find").clicked();
                if go {
                    state.run_find(nes);
                }
                if let Some(s) = &state.find_status {
                    ui.weak(s);
                }
            });

            if state.domain.writable() {
                ui.weak(
                    "Click a byte in $0000-$1FFF (work RAM) to poke it (Enter to write). \
                     Right-click toggles freeze. Bytes outside work RAM are read-only.",
                );
            } else {
                ui.weak("Read-only domain (no deterministic poke path).");
            }
            ui.separator();

            // Pending edits collected during the immutable-ish render, applied
            // after so we don't fight the `nes` borrow inside the closures.
            let mut poke: Option<(u16, u8)> = None;
            let mut toggle_freeze: Option<u16> = None;

            egui::ScrollArea::vertical().show(ui, |ui| {
                let rows: u16 = 16;
                let max = state.domain.max_addr();
                for r in 0..rows {
                    let row_addr = state.origin.wrapping_add(r * 16);
                    if u32::from(row_addr) > max {
                        break;
                    }
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 3.0;
                        ui.monospace(format!("{row_addr:04X} "));
                        let mut ascii = String::with_capacity(16);
                        for c in 0..16u16 {
                            let addr = row_addr.wrapping_add(c);
                            if u32::from(addr) > max {
                                break;
                            }
                            let byte = state.read_byte(nes, addr);
                            ascii.push(if (0x20..0x7F).contains(&byte) {
                                byte as char
                            } else {
                                '.'
                            });

                            // If this cell is being edited, draw the text box.
                            if let Some((eaddr, buf)) = state.editing.as_mut()
                                && *eaddr == addr
                            {
                                let resp = ui.add(
                                    egui::TextEdit::singleline(buf)
                                        .desired_width(22.0)
                                        .font(egui::TextStyle::Monospace),
                                );
                                resp.request_focus();
                                if resp.lost_focus() {
                                    if ui.input(|i| i.key_pressed(egui::Key::Enter))
                                        && let Some(v) = parse_byte(buf)
                                    {
                                        poke = Some((addr, v));
                                    }
                                    state.editing = None;
                                }
                                continue;
                            }

                            // Otherwise a clickable label, tinted by freeze /
                            // heatmap state.
                            let frozen = state.frozen.contains_key(&addr);
                            let mut text = egui::RichText::new(format!("{byte:02X}")).monospace();
                            if frozen {
                                text = text.background_color(FROZEN_TINT).color(Color32::BLACK);
                            } else if state.heatmap
                                && state.domain == Domain::Cpu
                                && let Some(f) = state.access.get(&addr)
                            {
                                if f.write {
                                    text = text.color(WRITE_TINT);
                                } else if f.read {
                                    text = text.color(READ_TINT);
                                }
                            }
                            // Only $0000-$1FFF work RAM is actually pokeable;
                            // a click elsewhere would be a silent no-op, so it
                            // is not made editable / freezable.
                            let editable = state.domain.addr_writable(addr);
                            let resp = ui.add(egui::Label::new(text).sense(egui::Sense::click()));
                            if resp.clicked() && editable {
                                state.editing = Some((addr, format!("{byte:02X}")));
                            }
                            if resp.secondary_clicked() && editable {
                                toggle_freeze = Some(addr);
                            }
                        }
                        ui.monospace(format!("  {ascii}"));
                    });
                }
            });

            // Apply the deferred edits (borrow of `nes` is free here).
            if let Some((addr, v)) = poke {
                nes.poke_ram(addr, v);
                // Keep a freeze in sync if this byte is frozen.
                if let Some(slot) = state.frozen.get_mut(&addr) {
                    *slot = v;
                }
            }
            if let Some(addr) = toggle_freeze
                && state.frozen.remove(&addr).is_none()
            {
                let v = nes.cpu_bus_peek(addr);
                state.frozen.insert(addr, v);
            }

            if !state.frozen.is_empty() {
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label(format!("frozen: {}", state.frozen.len()));
                    if ui.small_button("clear frozen").clicked() {
                        state.frozen.clear();
                    }
                });
            }
        });
}

fn parse_hex16(s: &str) -> Option<u16> {
    let trimmed = s.trim().trim_start_matches('$').trim_start_matches("0x");
    u16::from_str_radix(trimmed, 16).ok()
}

/// Parse a single `$`/`0x`/bare-hex byte.
fn parse_byte(s: &str) -> Option<u8> {
    let t = s.trim().trim_start_matches('$').trim_start_matches("0x");
    u8::from_str_radix(t, 16).ok()
}

/// Parse a whitespace-separated hex byte sequence (`"DE AD BE EF"`), tolerating
/// `$`/`0x` prefixes per token. Returns `None` if any token is not a valid byte.
fn parse_byte_seq(s: &str) -> Option<Vec<u8>> {
    s.split_whitespace().map(parse_byte).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_byte_seq_forms() {
        assert_eq!(
            parse_byte_seq("DE AD BE EF"),
            Some(vec![0xDE, 0xAD, 0xBE, 0xEF])
        );
        assert_eq!(parse_byte_seq("$10 0x20 30"), Some(vec![0x10, 0x20, 0x30]));
        assert_eq!(parse_byte_seq(""), Some(vec![]));
        assert_eq!(parse_byte_seq("ZZ"), None);
        assert_eq!(parse_byte_seq("10 ZZ"), None);
    }

    #[test]
    fn parse_byte_and_hex16() {
        assert_eq!(parse_byte("$0A"), Some(0x0A));
        assert_eq!(parse_byte("ff"), Some(0xFF));
        assert_eq!(parse_byte("100"), None);
        assert_eq!(parse_hex16("$C000"), Some(0xC000));
        assert_eq!(parse_hex16("1234"), Some(0x1234));
    }

    #[test]
    fn freeze_cheats_map_to_raw_cheats() {
        let mut s = MemoryPanelState::default();
        s.frozen.insert(0x0300, 0x63);
        s.frozen.insert(0x0010, 0x01);
        let mut cheats = s.freeze_cheats();
        cheats.sort_by_key(|c| c.address);
        assert_eq!(cheats.len(), 2);
        assert_eq!(cheats[0].address, 0x0010);
        assert_eq!(cheats[0].value, 0x01);
        assert!(cheats[0].enabled);
        assert_eq!(cheats[0].compare, None);
        assert_eq!(cheats[1].address, 0x0300);
        assert_eq!(cheats[1].value, 0x63);
    }

    #[test]
    fn empty_freeze_is_empty_cheats() {
        let s = MemoryPanelState::default();
        assert!(s.freeze_cheats().is_empty());
        assert!(!s.wants_access_log());
    }

    #[test]
    fn domain_extents_and_writability() {
        assert_eq!(Domain::Cpu.max_addr(), 0xFFFF);
        assert_eq!(Domain::Ppu.max_addr(), 0x3FFF);
        assert_eq!(Domain::Oam.max_addr(), 0x00FF);
        assert!(Domain::Cpu.writable());
        assert!(!Domain::Ppu.writable());
        assert!(!Domain::Oam.writable());
    }
}
