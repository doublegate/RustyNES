//! Cycle trace logger panel (v1.1.0 beta.2, Workstream C, T-110-C2).
//!
//! Toggles the core's `debug-hooks` cycle-trace ring (`Nes::set_trace_enabled`),
//! shows the most-recent instructions (CPU register file + a disassembly of the
//! instruction at each recorded PC), and exports the full ring to a text file.
//! Read-only inspection; the ring is output-only in the core, so determinism is
//! unaffected.

use rustynes_core::{Nes, TraceRec};

/// How many tail records the live view renders (the ring holds far more; the
/// full set is written by Export).
const TAIL_ROWS: usize = 128;

/// Trace panel state.
#[derive(Debug, Default)]
pub struct TracePanelState {
    /// Last export result (path written, or an error), shown under the toolbar.
    pub export_status: Option<String>,
}

/// Disassemble the single instruction at `rec.pc` into `"MNE operand"` (peeking
/// the current bus — accurate for ROM code, the usual trace target).
fn disasm_one(nes: &mut Nes, pc: u16) -> String {
    let mut w = [0u8; 3];
    for (i, slot) in w.iter_mut().enumerate() {
        *slot = nes.cpu_bus_peek(pc.wrapping_add(i as u16));
    }
    // `w` holds only 3 bytes; a 3-byte instruction's disassembler may peek
    // `pc+3` (or wrap to `pc-1`), so use the safe `.get().unwrap_or(0)` pattern
    // — `& 3` would index past the array and panic (gemini/Copilot #42).
    let lines = rustynes_core::rustynes_cpu::disassemble_at(
        |a| {
            let off = a.wrapping_sub(pc) as usize;
            w.get(off).copied().unwrap_or(0)
        },
        pc,
        1,
    );
    lines.first().map_or_else(
        || "???".to_string(),
        |l| format!("{} {}", l.mnemonic, l.operand),
    )
}

/// Format one record as a fixed-width trace line.
fn fmt_rec(disasm: &str, r: &TraceRec) -> String {
    format!(
        "{:04X}  A:{:02X} X:{:02X} Y:{:02X} S:{:02X} P:{:02X}  CYC:{:<12}  {}",
        r.pc, r.a, r.x, r.y, r.s, r.p, r.cycle, disasm
    )
}

/// Render the trace panel.
pub fn show(ctx: &egui::Context, open: &mut bool, state: &mut TracePanelState, nes: &mut Nes) {
    egui::Window::new("Trace")
        .open(open)
        .default_size([460.0, 360.0])
        .resizable(true)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                let mut on = nes.trace_enabled();
                if ui.checkbox(&mut on, "Record").changed() {
                    nes.set_trace_enabled(on);
                }
                if ui.button("Clear").clicked() {
                    nes.clear_trace();
                    state.export_status = None;
                }
                ui.label(format!("{} recs", nes.trace_len()));
                // Export the full ring to a text file (native only — no
                // filesystem on wasm). A one-shot debug dump.
                #[cfg(not(target_arch = "wasm32"))]
                if ui.button("Export…").clicked() {
                    state.export_status = Some(export_trace(nes));
                }
            });
            if let Some(s) = &state.export_status {
                ui.weak(s);
            }
            ui.separator();

            // Live tail: the most-recent TAIL_ROWS records, disassembled.
            let tail = nes.trace_tail_vec(TAIL_ROWS);
            let lines: Vec<String> = tail
                .iter()
                .map(|r| {
                    let d = disasm_one(nes, r.pc);
                    fmt_rec(&d, r)
                })
                .collect();
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    if lines.is_empty() {
                        ui.weak("(no records — enable Record and run a frame)");
                    }
                    for line in &lines {
                        ui.monospace(line);
                    }
                });
        });
}

/// Write the entire trace ring to `<temp>/rustynes-trace.log`. Returns a status
/// string (the path on success, or the error).
#[cfg(not(target_arch = "wasm32"))]
fn export_trace(nes: &mut Nes) -> String {
    let recs = nes.trace_records();
    let mut out = String::with_capacity(recs.len() * 64);
    for r in &recs {
        let d = disasm_one(nes, r.pc);
        out.push_str(&fmt_rec(&d, r));
        out.push('\n');
    }
    let path = std::env::temp_dir().join("rustynes-trace.log");
    match std::fs::write(&path, out) {
        Ok(()) => format!("wrote {} records to {}", recs.len(), path.display()),
        Err(e) => format!("export failed: {e}"),
    }
}
