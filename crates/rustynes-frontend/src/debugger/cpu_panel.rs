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

use rustynes_core::Nes;

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
}

impl Default for CpuPanelState {
    fn default() -> Self {
        Self {
            follow_pc: true,
            origin: 0xC000,
            rows: 32,
            goto_text: String::new(),
            bp_text: String::new(),
        }
    }
}

/// Bit names for the 6502 status register, MSB first.
const FLAG_NAMES: [&str; 8] = ["N", "V", "_", "B", "D", "I", "Z", "C"];

/// Render the CPU panel.
pub fn show(ctx: &egui::Context, open: &mut bool, state: &mut CpuPanelState, nes: &mut Nes) {
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
                            if ui.small_button("x").clicked() {
                                nes.remove_breakpoint(addr);
                            }
                        });
                    }
                });

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
}

fn parse_hex16(s: &str) -> Option<u16> {
    let trimmed = s.trim().trim_start_matches('$').trim_start_matches("0x");
    u16::from_str_radix(trimmed, 16).ok()
}
