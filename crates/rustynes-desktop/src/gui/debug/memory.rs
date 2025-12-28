//! Memory viewer debug window.

use egui::Context;
use rustynes_core::Console;

/// Memory viewer state.
#[derive(Debug)]
pub struct MemoryViewerState {
    /// Current view address.
    address: u16,
    /// Address input string.
    address_input: String,
    /// Number of rows to display.
    rows: usize,
}

impl Default for MemoryViewerState {
    fn default() -> Self {
        Self {
            address: 0,
            address_input: String::from("0000"),
            rows: 16,
        }
    }
}

/// Render the memory viewer debug window.
///
/// # Panics
///
/// Panics if the internal mutex is poisoned (should not happen in single-threaded egui context).
#[allow(clippy::too_many_lines)]
pub fn render(ctx: &Context, open: &mut bool, console: &Option<Console>) {
    // Use a static for state since we don't have access to the full state structure
    // This is safe because egui is single-threaded
    static STATE: std::sync::OnceLock<std::sync::Mutex<MemoryViewerState>> =
        std::sync::OnceLock::new();

    let state_mutex = STATE.get_or_init(|| std::sync::Mutex::new(MemoryViewerState::default()));
    let mut state = state_mutex.lock().unwrap();

    egui::Window::new("Memory Viewer")
        .open(open)
        .resizable(true)
        .default_width(550.0)
        .show(ctx, |ui| {
            if let Some(cons) = console {
                // Controls
                ui.horizontal(|ui| {
                    ui.label("CPU Memory");

                    ui.separator();

                    ui.label("Address:");
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut state.address_input)
                            .desired_width(60.0)
                            .font(egui::TextStyle::Monospace),
                    );
                    if response.lost_focus()
                        && ui.input(|i| i.key_pressed(egui::Key::Enter))
                        && let Ok(addr) = u16::from_str_radix(&state.address_input, 16)
                    {
                        state.address = addr;
                    }

                    if ui.button("Go").clicked()
                        && let Ok(addr) = u16::from_str_radix(&state.address_input, 16)
                    {
                        state.address = addr;
                    }
                });

                ui.add_space(5.0);

                // Navigation buttons
                ui.horizontal(|ui| {
                    if ui.button("<<").clicked() {
                        state.address = state.address.saturating_sub(0x100);
                        state.address_input = format!("{:04X}", state.address);
                    }
                    if ui.button("<").clicked() {
                        state.address = state.address.saturating_sub(0x10);
                        state.address_input = format!("{:04X}", state.address);
                    }
                    if ui.button(">").clicked() {
                        state.address = state.address.saturating_add(0x10);
                        state.address_input = format!("{:04X}", state.address);
                    }
                    if ui.button(">>").clicked() {
                        state.address = state.address.saturating_add(0x100);
                        state.address_input = format!("{:04X}", state.address);
                    }

                    ui.separator();

                    // Quick jump buttons
                    if ui.button("$0000").clicked() {
                        state.address = 0x0000;
                        state.address_input = String::from("0000");
                    }
                    if ui.button("$2000").clicked() {
                        state.address = 0x2000;
                        state.address_input = String::from("2000");
                    }
                    if ui.button("$8000").clicked() {
                        state.address = 0x8000;
                        state.address_input = String::from("8000");
                    }
                    if ui.button("$C000").clicked() {
                        state.address = 0xC000;
                        state.address_input = String::from("C000");
                    }
                });

                ui.add_space(10.0);

                // Memory display
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        // Header
                        ui.horizontal(|ui| {
                            ui.monospace("Addr  ");
                            for i in 0..16 {
                                ui.monospace(format!("{i:02X} "));
                            }
                            ui.monospace(" ASCII");
                        });
                        ui.separator();

                        // Memory rows
                        for row in 0..state.rows {
                            let base_addr = state.address.saturating_add((row * 16) as u16);

                            ui.horizontal(|ui| {
                                // Address
                                ui.monospace(format!("{base_addr:04X}: "));

                                // Hex bytes
                                let mut ascii = String::new();
                                for col in 0..16u16 {
                                    let addr = base_addr.saturating_add(col);
                                    let byte = cons.peek_memory(addr);
                                    ui.monospace(format!("{byte:02X} "));

                                    // ASCII representation
                                    if (0x20..=0x7E).contains(&byte) {
                                        ascii.push(byte as char);
                                    } else {
                                        ascii.push('.');
                                    }
                                }

                                // ASCII column
                                ui.monospace(format!(" {ascii}"));
                            });
                        }
                    });

                ui.add_space(10.0);
                ui.separator();
                ui.label("Note: Shows CPU address space only.");
                ui.label("PPU/OAM memory requires additional core API.");
            } else {
                ui.label("No ROM loaded");
            }
        });
}
