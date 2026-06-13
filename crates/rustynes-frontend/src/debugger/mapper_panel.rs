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
//! Mapper panel — bank registers + IRQ counter state (T-53-007).

use rustynes_core::Nes;

/// Persistent state of the mapper panel (currently no scroll state, but
/// keep the type so the parent doesn't reach into specifics).
#[derive(Debug, Default)]
pub struct MapperPanelState {}

pub fn show(ctx: &egui::Context, open: &mut bool, _state: &mut MapperPanelState, nes: &Nes) {
    let info = nes.mapper_info();
    egui::Window::new("Mapper")
        .open(open)
        .default_pos([16.0, 720.0])
        .default_size([420.0, 360.0])
        .resizable(true)
        .show(ctx, |ui| {
            ui.label(format!("Mapper #{} — {}", info.mapper_id, info.name));
            ui.label(format!("Mirroring: {}", info.mirroring));
            egui::ScrollArea::vertical().show(ui, |ui| {
                if !info.prg_banks.is_empty() {
                    ui.separator();
                    ui.label(egui::RichText::new("PRG banks").strong());
                    for (k, v) in &info.prg_banks {
                        ui.monospace(format!("{k:>10} = {v}"));
                    }
                }
                if !info.chr_banks.is_empty() {
                    ui.separator();
                    ui.label(egui::RichText::new("CHR banks").strong());
                    for (k, v) in &info.chr_banks {
                        ui.monospace(format!("{k:>10} = {v}"));
                    }
                }
                if !info.irq_state.is_empty() {
                    ui.separator();
                    ui.label(egui::RichText::new("IRQ counter").strong());
                    for (k, v) in &info.irq_state {
                        ui.monospace(format!("{k:>10} = {v}"));
                    }
                }
                if !info.extra.is_empty() {
                    ui.separator();
                    ui.label(egui::RichText::new("Extra").strong());
                    for (k, v) in &info.extra {
                        ui.monospace(format!("{k:>10} = {v}"));
                    }
                }
            });
        });
}
