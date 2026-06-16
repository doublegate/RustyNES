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
//! Memory hex viewer — CPU bus + PPU bus tabs with go-to-address (T-53-006).
//!
//! Read-only. Per-redraw the viewer samples 16 rows × 16 bytes = 256
//! bytes via the side-effect-free `cpu_bus_peek` / `ppu_bus_peek` API.

use rustynes_core::Nes;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum Bus {
    Cpu,
    Ppu,
}

/// Persistent state of the memory panel.
#[derive(Debug)]
pub struct MemoryPanelState {
    bus: Bus,
    /// Current origin (16-byte aligned).
    origin: u16,
    /// Text input for go-to.
    goto_text: String,
}

impl Default for MemoryPanelState {
    fn default() -> Self {
        Self {
            bus: Bus::Cpu,
            origin: 0,
            goto_text: String::new(),
        }
    }
}

pub fn show(ctx: &egui::Context, open: &mut bool, state: &mut MemoryPanelState, nes: &mut Nes) {
    egui::Window::new("Memory")
        .open(open)
        .default_pos([336.0, 480.0])
        .default_size([460.0, 460.0])
        .resizable(true)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut state.bus, Bus::Cpu, "CPU bus");
                ui.selectable_value(&mut state.bus, Bus::Ppu, "PPU bus");
                ui.separator();
                ui.label("goto:");
                let r = ui.add(
                    egui::TextEdit::singleline(&mut state.goto_text)
                        .desired_width(64.0)
                        .hint_text("$1234"),
                );
                if r.lost_focus()
                    && ui.input(|i| i.key_pressed(egui::Key::Enter))
                    && let Some(addr) = parse_hex16(&state.goto_text)
                {
                    state.origin = addr & 0xFFF0;
                }
                if ui.button("- 256").clicked() {
                    state.origin = state.origin.wrapping_sub(256);
                }
                if ui.button("+ 256").clicked() {
                    state.origin = state.origin.wrapping_add(256);
                }
            });
            ui.separator();
            egui::ScrollArea::vertical().show(ui, |ui| {
                // Render 16 rows of 16 bytes.
                let rows: u16 = 16;
                for r in 0..rows {
                    let row_addr = state.origin.wrapping_add(r * 16);
                    let mut s = String::with_capacity(64);
                    use std::fmt::Write as _;
                    let _ = write!(s, "{row_addr:04X}  ");
                    let mut ascii = String::with_capacity(16);
                    for c in 0..16u16 {
                        let addr = row_addr.wrapping_add(c);
                        let byte = match state.bus {
                            Bus::Cpu => nes.cpu_bus_peek(addr),
                            Bus::Ppu => nes.ppu_bus_peek(addr),
                        };
                        let _ = write!(s, "{byte:02X} ");
                        ascii.push(if (0x20..0x7F).contains(&byte) {
                            byte as char
                        } else {
                            '.'
                        });
                    }
                    let _ = write!(s, "  {ascii}");
                    ui.monospace(s);
                }
            });
        });
}

fn parse_hex16(s: &str) -> Option<u16> {
    let trimmed = s.trim().trim_start_matches('$').trim_start_matches("0x");
    u16::from_str_radix(trimmed, 16).ok()
}
