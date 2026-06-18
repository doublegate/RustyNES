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
//! CPU panel — registers + flags + scrollable disassembly (T-53-002).
//!
//! Read-only. The disassembly takes a 64-byte rolling window of CPU bus
//! peeks per redraw (cheap; 60 Hz it's ~4 KiB/s of work).

use rustynes_core::{EventBpKind, Nes};

use crate::debugger::callstack::{self, CallstackTracker, StepRequest};
use crate::debugger::source_map::SourceMap;
use crate::emu::DebugPoke;
use crate::symbols::SymbolMap;

/// Persistent state of the CPU debugger panel.
#[derive(Debug)]
pub struct CpuPanelState {
    /// `true` when the listing should keep its top aligned to PC.
    pub follow_pc: bool,
    /// User-overridable origin for the disasm listing.
    pub origin: u16,
    /// Number of instructions to render below `origin`.
    pub rows: usize,
    /// Text box for the "go to" field.
    pub goto_text: String,
    /// v1.1.0 beta.2 (Workstream C) — text box for adding an exec breakpoint
    /// (hex address).
    pub bp_text: String,
    /// v1.7.0 "Forge" Workstream A3 — inline 6502 assembler state. Self-contained
    /// for clean merges alongside other CPU-panel work.
    a3: A3Asm,
}

/// v1.7.0 "Forge" Workstream A3 — inline-assembler UI state + the one-shot poke
/// queue. Assembled bytes are queued as [`DebugPoke::CpuRam`] writes through the
/// gated post-frame poke path (work-RAM `$0000-$1FFF` only — the same gated
/// target as the raw RAM cheats; writes elsewhere are core no-ops), so the
/// assembler never touches the running `Nes` directly and determinism + the
/// `emu.write` gate hold.
#[derive(Debug, Default)]
struct A3Asm {
    /// Master enable (off by default → the assembler row is hidden).
    enabled: bool,
    /// Target address hex text (where the first assembled byte lands).
    addr_text: String,
    /// The source line(s) to assemble (one instruction per line).
    src_text: String,
    /// Last assembler status / error line.
    status: String,
    /// Pending writeback bytes, drained by [`CpuPanelState::take_pokes`].
    pending: Vec<DebugPoke>,
}

impl CpuPanelState {
    /// v1.7.0 "Forge" Workstream A3 — drain the queued assembler writeback bytes
    /// for the debugger to forward to the gated post-frame poke path.
    pub fn take_pokes(&mut self) -> Vec<DebugPoke> {
        core::mem::take(&mut self.a3.pending)
    }
}

impl Default for CpuPanelState {
    fn default() -> Self {
        Self {
            follow_pc: true,
            origin: 0xC000,
            rows: 32,
            goto_text: String::new(),
            bp_text: String::new(),
            a3: A3Asm::default(),
        }
    }
}

/// Bit names for the 6502 status register, MSB first.
const FLAG_NAMES: [&str; 8] = ["N", "V", "_", "B", "D", "I", "Z", "C"];

/// Render the CPU panel.
///
/// `symbols` annotates the disassembly + breakpoint list with loaded labels
/// (v1.4.0 D1); `symbols_status` is the last symbol-load status line.
///
/// v1.7.0 "Forge" Workstream C: `callstack` drives the Call Stack section + step
/// verbs (C1); `source_map` + `source_map_status` annotate the disassembly with
/// the original ca65/cc65 source line (C3). Returns a step verb the user clicked
/// (the caller queues it on the tracker + keeps the emulator running until it is
/// satisfied).
#[must_use]
pub fn show(
    ctx: &egui::Context,
    open: &mut bool,
    state: &mut CpuPanelState,
    nes: &mut Nes,
    symbols: &SymbolMap,
    symbols_status: Option<&str>,
    callstack: &CallstackTracker,
    source_map: &SourceMap,
    source_map_status: Option<&str>,
) -> Option<StepRequest> {
    let mut step_request = None;
    let cpu = nes.cpu_snapshot();
    egui::Window::new("CPU")
        .open(open)
        .default_pos([8.0, 64.0])
        .default_size([320.0, 360.0])
        .resizable(true)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.monospace(format!("A={:02X}", cpu.a));
                ui.monospace(format!("X={:02X}", cpu.x));
                ui.monospace(format!("Y={:02X}", cpu.y));
                ui.monospace(format!("S={:02X}", cpu.s));
                ui.monospace(format!("PC={:04X}", cpu.pc));
            });
            ui.horizontal(|ui| {
                ui.monospace("flags:");
                for (i, name) in FLAG_NAMES.iter().enumerate() {
                    let bit = 7 - i;
                    let set = (cpu.p >> bit) & 1 == 1;
                    let mut t = egui::RichText::new(*name).monospace();
                    if set {
                        t = t.color(egui::Color32::LIGHT_GREEN);
                    } else {
                        t = t.color(egui::Color32::DARK_GRAY);
                    }
                    ui.label(t);
                }
                ui.separator();
                ui.monospace(format!("cyc={}", cpu.cycles));
                if cpu.jammed {
                    ui.colored_label(egui::Color32::LIGHT_RED, "JAMMED");
                }
            });

            ui.separator();
            ui.horizontal(|ui| {
                ui.checkbox(&mut state.follow_pc, "Follow PC");
                ui.label("goto:");
                let response = ui.add(
                    egui::TextEdit::singleline(&mut state.goto_text)
                        .desired_width(64.0)
                        .hint_text("$C000"),
                );
                if response.lost_focus()
                    && ui.input(|i| i.key_pressed(egui::Key::Enter))
                    && let Some(addr) = parse_hex16(&state.goto_text)
                {
                    state.origin = addr;
                    state.follow_pc = false;
                }
            });

            // v1.7.0 "Forge" Workstream A3 — inline 6502 assembler. Off by
            // default; when on, assembled bytes are queued through the gated
            // post-frame poke path (work RAM only, like the raw cheats), so the
            // no-assemble path is byte-identical.
            a3_assembler(ui, &mut state.a3);

            // v1.1.0 beta.2 (Workstream C) — exec/PC breakpoints. Adding one
            // pauses emulation + opens this panel the next time PC reaches it.
            egui::CollapsingHeader::new("Breakpoints")
                .default_open(false)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let mut enabled = nes.breakpoints_enabled();
                        if ui.checkbox(&mut enabled, "Armed").changed() {
                            nes.set_breakpoints_enabled(enabled);
                        }
                        let add = ui.add(
                            egui::TextEdit::singleline(&mut state.bp_text)
                                .desired_width(64.0)
                                .hint_text("$8000"),
                        );
                        let submit = (add.lost_focus()
                            && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                            || ui.button("Add").clicked();
                        if submit && let Some(addr) = parse_hex16(&state.bp_text) {
                            nes.add_breakpoint(addr);
                            state.bp_text.clear();
                        }
                        if !nes.breakpoints().is_empty() && ui.button("Clear").clicked() {
                            nes.clear_breakpoints();
                        }
                    });
                    // Snapshot to a local list so we can mutate `nes` while
                    // rendering the rows (remove buttons).
                    let bps: Vec<u16> = nes.breakpoints().to_vec();
                    for addr in bps {
                        ui.horizontal(|ui| {
                            ui.monospace(format!("${addr:04X}"));
                            // v1.4.0 D1 — annotate with the loaded label, if any.
                            if let Some(label) = symbols.label(addr) {
                                ui.colored_label(egui::Color32::from_rgb(0x90, 0xC0, 0xF0), label);
                            }
                            if ui.small_button("x").clicked() {
                                nes.remove_breakpoint(addr);
                            }
                        });
                    }
                });

            // v1.4.0 Workstream D (D2) — event-driven breakpoints. Arming a
            // category pauses + opens this panel the next time that hardware
            // event fires (NMI/IRQ entry, sprite-0 hit, OAM/DMC DMA, or a
            // PPU/APU/mapper register read/write), reporting frame/cycle/
            // scanline/dot. Output-only in the core (observational taps).
            egui::CollapsingHeader::new("Event breakpoints")
                .default_open(false)
                .show(ui, |ui| {
                    let mut mask = nes.event_breakpoints();
                    let before = mask;
                    ui.horizontal_wrapped(|ui| {
                        for kind in EventBpKind::all() {
                            let mut on = mask & kind.bit() != 0;
                            if ui.checkbox(&mut on, kind.label()).changed() {
                                if on {
                                    mask |= kind.bit();
                                } else {
                                    mask &= !kind.bit();
                                }
                            }
                        }
                    });
                    if mask != before {
                        nes.set_event_breakpoints(mask);
                    }
                    ui.weak(
                        "A hit pauses emulation and reports the frame / CPU \
                         cycle / scanline / dot in the status bar.",
                    );
                });

            // v1.4.0 Workstream D (D1) — loaded-symbol status (set from the
            // Debug menu's Load Symbols action).
            if let Some(s) = symbols_status {
                ui.weak(format!("symbols: {s}"));
            } else if !symbols.is_empty() {
                ui.weak(format!("symbols: {} labels", symbols.len()));
            }

            // v1.7.0 "Forge" Workstream C (C3) — loaded `.dbg` source-map status.
            if let Some(s) = source_map_status {
                ui.weak(format!("source map: {s}"));
            } else if !source_map.is_empty() {
                ui.weak(format!("source map: {} addresses", source_map.len()));
            }

            // v1.7.0 "Forge" Workstream C (C1) — the live call stack + the step
            // verbs (over / out / run-to-NMI/IRQ / scanline / frame). A clicked
            // verb is returned to the caller, which queues it on the tracker.
            if let Some(req) = callstack::show_callstack_section(ui, callstack, symbols) {
                step_request = Some(req);
            }

            if state.follow_pc {
                state.origin = cpu.pc;
            }

            // Walk N instructions forward from origin. Read into a window
            // first so the disasm closure stays `Fn`.
            let window_start = state.origin;
            let mut window = [0u8; 256];
            for (i, slot) in window.iter_mut().enumerate() {
                *slot = nes.cpu_bus_peek(window_start.wrapping_add(i as u16));
            }
            let lines = rustynes_core::rustynes_cpu::disassemble_at(
                |a| {
                    let off = a.wrapping_sub(window_start) as usize;
                    window.get(off).copied().unwrap_or(0)
                },
                window_start,
                state.rows,
            );

            egui::ScrollArea::vertical()
                .max_height(ui.available_height() - 4.0)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let pc = cpu.pc;
                    for line in &lines {
                        // v1.4.0 D1 — if a label maps to this address, print it
                        // on its own line above the instruction (the ca65 /
                        // FCEUX listing convention) so the disasm reads like a
                        // labelled source listing.
                        if let Some(label) = symbols.label(line.addr) {
                            ui.label(
                                egui::RichText::new(format!("{label}:"))
                                    .monospace()
                                    .color(egui::Color32::from_rgb(0x90, 0xC0, 0xF0)),
                            );
                        }
                        // v1.7.0 "Forge" Workstream C (C3) — annotate with the
                        // original ca65/cc65 source line (`file:line`), if a
                        // `.dbg` is loaded and maps this address.
                        if let Some(src) = source_map.annotation(line.addr) {
                            ui.label(
                                egui::RichText::new(format!("; {src}"))
                                    .monospace()
                                    .color(egui::Color32::from_rgb(0x80, 0xA0, 0x80)),
                            );
                        }
                        let bytes = line
                            .bytes
                            .iter()
                            .map(|b| format!("{b:02X}"))
                            .collect::<Vec<_>>()
                            .join(" ");
                        let text = format!(
                            "{:04X}  {:8}  {:4} {}",
                            line.addr, bytes, line.mnemonic, line.operand
                        );
                        let mut rt = egui::RichText::new(text).monospace();
                        if line.addr == pc {
                            rt = rt.color(egui::Color32::YELLOW).strong();
                        }
                        ui.label(rt);
                    }
                });
        });
    step_request
}

/// v1.7.0 "Forge" Workstream A3 — the inline-assembler UI: an address field +
/// a multi-line source box + Assemble. Each source line is assembled in
/// sequence (advancing the target address by each instruction's length), and
/// every byte is queued as a gated `DebugPoke::CpuRam` write. On the first
/// per-line error nothing is queued (atomic per assemble click).
fn a3_assembler(ui: &mut egui::Ui, a3: &mut A3Asm) {
    egui::CollapsingHeader::new("Assemble (6502)")
        .default_open(false)
        .show(ui, |ui| {
            ui.checkbox(&mut a3.enabled, "Enable inline assembler (writeback)");
            if !a3.enabled {
                ui.weak(
                    "Assembled bytes are poked into work RAM ($0000-$1FFF) \
                         after the next frame, via the same gated path as cheats.",
                );
                return;
            }
            ui.horizontal(|ui| {
                ui.label("addr $");
                ui.add(
                    egui::TextEdit::singleline(&mut a3.addr_text)
                        .desired_width(56.0)
                        .hint_text("0200"),
                );
            });
            ui.add(
                egui::TextEdit::multiline(&mut a3.src_text)
                    .desired_rows(3)
                    .font(egui::TextStyle::Monospace)
                    .hint_text("LDA #$42\nSTA $0200\nRTS"),
            );
            if ui.button("Assemble + queue").clicked() {
                a3.status = assemble_into(a3);
            }
            if !a3.status.is_empty() {
                ui.weak(&a3.status);
            }
        });
}

/// Assemble every source line into the pending poke queue. Returns a status
/// string. Atomic: a parse error on any line queues nothing.
fn assemble_into(a3: &mut A3Asm) -> String {
    let Some(mut addr) = parse_hex16(&a3.addr_text) else {
        return "bad target address".into();
    };
    let mut bytes_out: Vec<(u16, u8)> = Vec::new();
    for (n, line) in a3.src_text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match super::assembler::assemble_line(line, addr) {
            Ok(bytes) => {
                for b in bytes {
                    bytes_out.push((addr, b));
                    addr = addr.wrapping_add(1);
                }
            }
            Err(e) => return format!("line {}: {e}", n + 1),
        }
    }
    if bytes_out.is_empty() {
        return "nothing to assemble".into();
    }
    let count = bytes_out.len();
    for (a, v) in bytes_out {
        a3.pending.push(DebugPoke::CpuRam { addr: a, value: v });
    }
    format!("queued {count} byte(s) (work RAM only; applied after next frame)")
}

fn parse_hex16(s: &str) -> Option<u16> {
    let trimmed = s.trim().trim_start_matches('$').trim_start_matches("0x");
    u16::from_str_radix(trimmed, 16).ok()
}
