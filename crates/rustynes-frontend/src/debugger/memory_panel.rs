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
//! `oam_byte` / `debug_peek_rom` API; the only write path is `poke_ram` (CPU RAM),
//! applied like a raw cheat, so the no-edit path is byte-identical and
//! determinism holds.
//!
//! **Live ROM editing (v3.7.0, `T-HACK-BPS-RECORDER`).** Two more domains,
//! **PRG-ROM** and **CHR-ROM**, show the cartridge's ROM by offset within that
//! section of the image (not by CPU / PPU address: one offset is one byte of
//! the file, whatever bank maps it), and are editable. An edit is NOT written
//! from here: it is queued as a [`DebugPoke::Rom`] for the gated post-frame
//! stage (`EmuCore::produce_one_frame`), which refuses it under
//! `RetroAchievements` hardcore, while a movie records or plays and during
//! netplay, and otherwise writes the byte through `Nes::debug_poke_rom` and
//! feeds the same write to the session's `PatchRecorder`. "Save patch..."
//! asks the app (after the egui pass, never under the emu lock inside this
//! closure) to write the recorder's BPS patch and, when RAM pins (enabled raw
//! cheats and freezes) exist, its `.pins.toml` sidecar.

use std::collections::HashMap;

use egui::Color32;
use rustynes_core::{Nes, RomSpace};

use crate::cheats::RawCheat;
use crate::debugger::access_counter::{self, MemoryAccessCounter};
use crate::emu::DebugPoke;

/// Which memory domain the editor is viewing.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum Domain {
    /// The CPU bus (`$0000-$FFFF`); only `$0000-$1FFF` work RAM is writable.
    Cpu,
    /// The PPU bus (`$0000-$3FFF`): CHR, nametables, palette. Read-only.
    Ppu,
    /// Object Attribute Memory (sprite RAM, 256 bytes). Read-only.
    Oam,
    /// v3.7.0 — PRG-ROM by image offset; editable through the gated queue.
    PrgRom,
    /// v3.7.0 — CHR-ROM by image offset; editable through the gated queue.
    ChrRom,
}

impl Domain {
    const ALL: [Self; 5] = [Self::Cpu, Self::Ppu, Self::Oam, Self::PrgRom, Self::ChrRom];

    const fn label(self) -> &'static str {
        match self {
            Self::Cpu => "CPU bus",
            Self::Ppu => "PPU bus",
            Self::Oam => "OAM",
            Self::PrgRom => "PRG-ROM",
            Self::ChrRom => "CHR-ROM",
        }
    }

    /// The ROM section a ROM domain views.
    const fn rom_space(self) -> Option<RomSpace> {
        match self {
            Self::PrgRom => Some(RomSpace::Prg),
            Self::ChrRom => Some(RomSpace::Chr),
            Self::Cpu | Self::Ppu | Self::Oam => None,
        }
    }

    /// The highest valid address in a bus domain (inclusive), as a `u32` so
    /// OAM's 256-byte window is representable distinctly from a 64 KiB bus.
    /// A ROM domain's extent is the loaded board's ([`Self::extent`]).
    const fn max_addr(self) -> u32 {
        match self {
            Self::Cpu => 0xFFFF,
            Self::Ppu => 0x3FFF,
            Self::Oam => 0x00FF,
            Self::PrgRom | Self::ChrRom => 0,
        }
    }

    /// The highest valid address (inclusive) on the loaded console, or `None`
    /// for a ROM domain the board does not offer (`Nes::debug_rom_len`).
    fn extent(self, nes: &mut Nes) -> Option<u32> {
        self.rom_space().map_or_else(
            || Some(self.max_addr()),
            |space| {
                nes.debug_rom_len(space)
                    .and_then(|len| u32::try_from(len.checked_sub(1)?).ok())
            },
        )
    }

    /// Whether this domain has *any* writable region (the CPU bus's work RAM
    /// and, through the gated ROM queue, the two ROM domains). The view picks
    /// its help line by domain directly; the tests pin this summary.
    #[cfg(test)]
    const fn writable(self) -> bool {
        matches!(self, Self::Cpu | Self::PrgRom | Self::ChrRom)
    }

    /// Whether a poke / freeze actually takes effect at `addr` in this domain.
    ///
    /// [`rustynes_core::Nes::poke_ram`] is a no-op outside CPU work RAM
    /// (`$0000-$1FFF`), so only that range is editable on the CPU bus — a
    /// click on ROM / mapper / register space there would otherwise be a
    /// silent, misleading no-op (the ROM is edited in the ROM domains). Every
    /// in-extent offset of a ROM domain is editable.
    const fn addr_writable(self, addr: u32) -> bool {
        match self {
            Self::Cpu => addr <= 0x1FFF,
            Self::PrgRom | Self::ChrRom => true,
            Self::Ppu | Self::Oam => false,
        }
    }

    /// Whether a byte can be frozen (re-applied every frame as a raw cheat):
    /// CPU work RAM only. A ROM edit persists by itself.
    const fn freezable(self, addr: u32) -> bool {
        matches!(self, Self::Cpu) && addr <= 0x1FFF
    }
}

/// v3.7.0 — what the app reports about the session's live-patch recorder,
/// pushed each frame (`DebuggerOverlay::set_rom_patch_info`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RomPatchInfo {
    /// Whether a recorder exists for the loaded image (an iNES cartridge).
    pub recording: bool,
    /// ROM writes that landed and were recorded this session.
    pub writes: usize,
    /// The last ROM write's outcome (`Err`: why it was refused).
    pub last: Option<Result<String, String>>,
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
    /// Current origin (16-byte aligned). A `u32` because a ROM domain spans
    /// the whole PRG- or CHR-ROM, beyond 64 KiB.
    origin: u32,
    /// Text input for go-to.
    goto_text: String,
    /// The address currently being edited (a writable domain only), plus its
    /// edit buffer; `None` when no cell is open for editing.
    editing: Option<(u32, String)>,
    /// v3.7.0 — ROM writes queued this frame, drained by the app into the
    /// gated post-frame poke stage ([`Self::take_pokes`]).
    pending: Vec<DebugPoke>,
    /// v3.7.0 — "Save patch..." was clicked; the app takes it after the egui
    /// pass ([`Self::take_save_patch_request`]).
    save_patch_requested: bool,
    /// v3.7.0 — the recorder's state, as last reported by the app.
    rom_patch: RomPatchInfo,
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
            pending: Vec::new(),
            save_patch_requested: false,
            rom_patch: RomPatchInfo::default(),
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

    /// Drop every freeze on a ROM change.
    ///
    /// A frozen byte is not a display state. [`Self::freeze_cheats`] feeds
    /// `DebuggerOverlay::enabled_raw_cheats`, which the app applies after EVERY
    /// frame, so a freeze set on one cartridge goes on writing into the next
    /// one's memory at an address that almost certainly means something else
    /// there — a silent, continuous modification of a game the user never
    /// applied it to.
    ///
    /// Only the freezes go. The panel's cursor, its view mode and its heatmap
    /// setting are preferences about how to look at memory, not claims about a
    /// particular cartridge, and resetting them would be churn.
    pub fn clear_rom_bound(&mut self) {
        self.frozen.clear();
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

    /// v3.7.0 — drain the ROM writes queued by the PRG-ROM / CHR-ROM views.
    /// The app forwards them to `EmuCore::debug_pokes`, where the gated
    /// post-frame stage applies or refuses each one.
    #[must_use]
    pub fn take_pokes(&mut self) -> Vec<DebugPoke> {
        std::mem::take(&mut self.pending)
    }

    /// v3.7.0 — whether "Save patch..." was clicked since the last call.
    pub const fn take_save_patch_request(&mut self) -> bool {
        std::mem::replace(&mut self.save_patch_requested, false)
    }

    /// v3.7.0 — the recorder's state for the ROM views' status line.
    pub fn set_rom_patch_info(&mut self, info: RomPatchInfo) {
        self.rom_patch = info;
    }

    /// Queue one edit of `addr` in the current domain: a work-RAM poke is
    /// returned for the caller to apply directly (the v1.6.0 path); a ROM
    /// edit is queued for the gated post-frame stage and `None` returned.
    fn queue_edit(&mut self, addr: u32, value: u8) -> Option<(u16, u8)> {
        match self.domain.rom_space() {
            Some(space) => {
                self.pending.push(DebugPoke::Rom {
                    space,
                    offset: addr,
                    value,
                });
                None
            }
            None => u16::try_from(addr).ok().map(|a| (a, value)),
        }
    }

    fn read_byte(&self, nes: &mut Nes, addr: u32) -> u8 {
        match self.domain {
            Domain::Cpu => nes.cpu_bus_peek(addr as u16),
            Domain::Ppu => nes.ppu_bus_peek(addr as u16),
            Domain::Oam => nes.oam_byte(addr as u8),
            Domain::PrgRom => nes
                .debug_peek_rom(RomSpace::Prg, addr as usize)
                .unwrap_or(0),
            Domain::ChrRom => nes
                .debug_peek_rom(RomSpace::Chr, addr as usize)
                .unwrap_or(0),
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
        let Some(max) = self.domain.extent(nes) else {
            self.find_status = Some("nothing to search".to_string());
            return;
        };
        let span = max + 1;
        // Scan every start address once, beginning just past the current origin
        // so repeated Find walks forward through matches.
        let start = (self.origin + 1) % span;
        for off in 0..span {
            let base = (start + off) % span;
            if base + needle.len() as u32 > span {
                continue;
            }
            let hit = needle
                .iter()
                .enumerate()
                .all(|(k, &b)| self.read_byte(nes, base + k as u32) == b);
            if hit {
                self.origin = base & !0xF;
                self.find_status = Some(format!("found at ${base:04X}"));
                return;
            }
        }
        self.find_status = Some("not found".to_string());
    }
}

pub fn show(
    ctx: &egui::Context,
    detached: &mut std::collections::HashSet<&'static str>,
    open: &mut bool,
    state: &mut MemoryPanelState,
    nes: &mut Nes,
    counter: &mut MemoryAccessCounter,
) {
    super::detachable_window(
        ctx,
        detached,
        "memory",
        "Memory",
        super::WindowCfg {
            default_pos: Some([336.0, 480.0]),
            default_size: Some([520.0, 520.0]),
            ..Default::default()
        },
        open,
        |ui| {
            // The visible domain's extent on this console; `None` for a ROM
            // domain the board does not offer (CHR-RAM, a flashable board, FDS,
            // NSF).
            let extent = state.domain.extent(nes);
            ui.horizontal(|ui| {
                for d in Domain::ALL {
                    if ui.selectable_label(state.domain == d, d.label()).clicked()
                        && state.domain != d
                    {
                        state.domain = d;
                        state.editing = None;
                        state.origin = 0;
                    }
                }
            });
            ui.horizontal(|ui| {
                ui.label("goto:");
                let r = ui.add(
                    egui::TextEdit::singleline(&mut state.goto_text)
                        .desired_width(64.0)
                        .hint_text("$1234"),
                );
                if r.lost_focus()
                    && ui.input(|i| i.key_pressed(egui::Key::Enter))
                    && let Some(addr) = parse_hex32(&state.goto_text)
                    && let Some(max) = extent
                {
                    state.origin = (addr & !0xF).min(max & !0xF);
                }
                if ui.button("-").clicked() {
                    state.origin = state.origin.saturating_sub(256);
                }
                if ui.button("+").clicked() {
                    let next = state.origin + 256;
                    if extent.is_some_and(|max| next <= max) {
                        state.origin = next;
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

            match state.domain {
                Domain::Cpu => {
                    ui.weak(
                        "Click a byte in $0000-$1FFF (work RAM) to poke it (Enter to write). \
                         Right-click toggles freeze. Bytes outside work RAM are read-only \
                         here; edit ROM in the PRG-ROM / CHR-ROM views.",
                    );
                }
                Domain::PrgRom | Domain::ChrRom => {
                    rom_patch_bar(ui, state, extent.is_some());
                }
                Domain::Ppu | Domain::Oam => {
                    ui.weak("Read-only domain (no deterministic poke path).");
                }
            }
            ui.separator();

            // Pending edits collected during the immutable-ish render, applied
            // after so we don't fight the `nes` borrow inside the closures.
            let mut poke: Option<(u32, u8)> = None;
            let mut toggle_freeze: Option<u16> = None;

            let Some(max) = extent else {
                // A ROM domain the board does not offer: nothing to list.
                return;
            };
            let wide = state.domain.rom_space().is_some();
            egui::ScrollArea::vertical().show(ui, |ui| {
                let rows: u32 = 16;
                for r in 0..rows {
                    let row_addr = state.origin + r * 16;
                    if row_addr > max {
                        break;
                    }
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 3.0;
                        if wide {
                            ui.monospace(format!("{row_addr:06X} "));
                        } else {
                            ui.monospace(format!("{row_addr:04X} "));
                        }
                        let mut ascii = String::with_capacity(16);
                        for c in 0..16u32 {
                            let addr = row_addr + c;
                            if addr > max {
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
                            let cpu_addr = u16::try_from(addr).ok();
                            let frozen = state.domain == Domain::Cpu
                                && cpu_addr.is_some_and(|a| state.frozen.contains_key(&a));
                            let mut text = egui::RichText::new(format!("{byte:02X}")).monospace();
                            if frozen {
                                text = text.background_color(FROZEN_TINT).color(Color32::BLACK);
                            } else if state.heatmap
                                && state.domain == Domain::Cpu
                                && let Some(f) = cpu_addr.and_then(|a| state.access.get(&a))
                            {
                                if f.write {
                                    text = text.color(WRITE_TINT);
                                } else if f.read {
                                    text = text.color(READ_TINT);
                                }
                            }
                            // Only work RAM and the ROM domains are pokeable;
                            // a click elsewhere would be a silent no-op, so it
                            // is not made editable / freezable.
                            let editable = state.domain.addr_writable(addr);
                            let resp = ui.add(egui::Label::new(text).sense(egui::Sense::click()));
                            if resp.clicked() && editable {
                                state.editing = Some((addr, format!("{byte:02X}")));
                            }
                            if resp.secondary_clicked() && state.domain.freezable(addr) {
                                toggle_freeze = cpu_addr;
                            }
                        }
                        ui.monospace(format!("  {ascii}"));
                    });
                }
            });

            // Apply the deferred edits (borrow of `nes` is free here). A ROM
            // edit is only queued; the gated post-frame stage writes it.
            if let Some((addr, v)) = poke
                && let Some((addr, v)) = state.queue_edit(addr, v)
            {
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

            // v1.7.0 "Forge" Workstream C (C2) — the per-address read/write/exec
            // access-counter + uninitialized-read detector, shown for the 16
            // addresses currently in view. Self-contained so it merges cleanly.
            // It counts CPU-bus accesses, so a ROM view (offsets, not
            // addresses) does not show it.
            if state.domain.rom_space().is_none() {
                ui.separator();
                access_counter::show_access_counter_section(ui, counter, state.origin as u16);
            }
        },
    );
}

#[cfg(test)]
fn parse_hex16(s: &str) -> Option<u16> {
    parse_hex32(s).and_then(|v| u16::try_from(v).ok())
}

/// A `$`/`0x`/bare-hex address or ROM offset.
fn parse_hex32(s: &str) -> Option<u32> {
    let trimmed = s.trim().trim_start_matches('$').trim_start_matches("0x");
    u32::from_str_radix(trimmed, 16).ok()
}

/// v3.7.0 — the ROM views' status line: whether edits are recorded, how many
/// landed, the last outcome (a refusal shows its reason), and "Save patch...".
///
/// The button only raises a request ([`MemoryPanelState::take_save_patch_request`]);
/// the app writes the files after the egui pass, so nothing here takes the
/// emu lock or touches the file system.
fn rom_patch_bar(ui: &mut egui::Ui, state: &mut MemoryPanelState, offered: bool) {
    if !offered {
        ui.weak(
            "This board does not offer this ROM as editable bytes in image order \
             (CHR-RAM, a self-flashing board, a copier, FDS or NSF).",
        );
        return;
    }
    ui.weak(
        "Offsets are within this section of the ROM file, not CPU/PPU addresses. \
         Click a byte, type a value, Enter: it is written after the next frame and \
         recorded for the patch. Refused in RA hardcore, while a movie records or \
         plays, and during netplay.",
    );
    ui.horizontal(|ui| {
        let info = &state.rom_patch;
        if info.recording {
            ui.label(format!("{} ROM write(s) recorded", info.writes));
        } else {
            ui.colored_label(
                Color32::from_rgb(230, 160, 60),
                "not recording (no iNES image)",
            );
        }
        let save = ui
            .add_enabled(info.recording, egui::Button::new("Save patch..."))
            .on_hover_text(
                "Write the edits as a BPS patch of the clean ROM, and the enabled raw \
                 cheats / freezes as a .pins.toml sidecar the patch names.",
            );
        if save.clicked() {
            state.save_patch_requested = true;
        }
        // v3.7.0 — back to the clean image. Queued like an edit, under the
        // same gate; the way out of "recording refused: live ROM edits".
        let reset = ui
            .add_enabled(
                info.recording && info.writes > 0,
                egui::Button::new("Reset edits"),
            )
            .on_hover_text(
                "Write the clean ROM's bytes back over every edit and start the \
                 recording of edits again. Movies cannot be recorded while edits exist.",
            );
        if reset.clicked() {
            state.pending.push(DebugPoke::RomReset);
        }
    });
    match &state.rom_patch.last {
        Some(Ok(done)) => {
            ui.weak(format!("last write: {done}"));
        }
        Some(Err(why)) => {
            ui.colored_label(Color32::from_rgb(230, 90, 90), format!("refused: {why}"));
        }
        None => {}
    }
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

    /// The hex editor's freezes are the same hazard as the watch panel's: they
    /// are re-applied after every frame, so one set on a previous cartridge goes
    /// on writing into the next.
    #[test]
    fn a_rom_change_drops_every_hex_editor_freeze() {
        let mut s = MemoryPanelState {
            frozen: [(0x0071u16, 9u8), (0x0300, 3)].into_iter().collect(),
            ..MemoryPanelState::default()
        };
        assert_eq!(s.freeze_cheats().len(), 2);
        s.clear_rom_bound();
        assert!(
            s.freeze_cheats().is_empty(),
            "a freeze from the previous cartridge is still being written"
        );
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
        assert_eq!(s.freeze_cheats(), [] as [crate::cheats::RawCheat; 0]);
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
        assert!(Domain::PrgRom.writable() && Domain::ChrRom.writable());
        assert!(Domain::PrgRom.addr_writable(0x7_FFFF));
        assert!(!Domain::PrgRom.freezable(0x10), "a ROM byte is not frozen");
    }

    /// v3.7.0 — an edit in a ROM view is queued as a `DebugPoke::Rom` by
    /// image offset (beyond 64 KiB too), never written in the panel; a
    /// work-RAM edit is still returned for the direct poke.
    #[test]
    fn a_rom_view_edit_is_queued_not_written() {
        let mut s = MemoryPanelState {
            domain: Domain::PrgRom,
            ..MemoryPanelState::default()
        };
        assert_eq!(s.queue_edit(0x1_2345, 0xEA), None);
        s.domain = Domain::ChrRom;
        assert_eq!(s.queue_edit(0x10, 0x55), None);
        assert_eq!(
            s.take_pokes(),
            vec![
                DebugPoke::Rom {
                    space: RomSpace::Prg,
                    offset: 0x1_2345,
                    value: 0xEA
                },
                DebugPoke::Rom {
                    space: RomSpace::Chr,
                    offset: 0x10,
                    value: 0x55
                },
            ]
        );
        assert!(s.take_pokes().is_empty(), "drained");
        s.domain = Domain::Cpu;
        assert_eq!(s.queue_edit(0x0300, 7), Some((0x0300, 7)));
        assert_eq!(
            s.take_pokes(),
            Vec::<DebugPoke>::new(),
            "a RAM edit is not queued"
        );
        assert_eq!(parse_hex32("$12345"), Some(0x1_2345));
    }
}
