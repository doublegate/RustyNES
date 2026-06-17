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
//! Mapper panel — identity + ROM/RAM + bank registers + IRQ + audio
//! (T-53-007; depth pass v1.5.0 "Lens" Workstream I8).
//!
//! The panel reads the read-only [`rustynes_core::Nes::mapper_info`] view,
//! enriched by the bus with cartridge-level metadata (submapper, accuracy tier,
//! ROM/RAM sizes, battery, IRQ mechanism, expansion audio), and renders it in
//! `GeraNES` `MapperInfo` / `Mesen2` mapper-state style. Output-only: it never
//! mutates the deterministic core.

use rustynes_core::Nes;

/// Persistent state of the mapper panel (currently no scroll state, but
/// keep the type so the parent doesn't reach into specifics).
#[derive(Debug, Default)]
pub struct MapperPanelState {}

/// Format a byte size as KiB (or bytes for sub-KiB), e.g. `32768 -> "32 KiB"`.
fn fmt_size(bytes: usize) -> String {
    if bytes == 0 {
        "none".to_string()
    } else if bytes.is_multiple_of(1024) {
        format!("{} KiB", bytes / 1024)
    } else {
        format!("{bytes} B")
    }
}

pub fn show(ctx: &egui::Context, open: &mut bool, _state: &mut MapperPanelState, nes: &Nes) {
    let info = nes.mapper_info();
    egui::Window::new("Mapper")
        .open(open)
        .default_pos([16.0, 720.0])
        .default_size([440.0, 460.0])
        .resizable(true)
        .show(ctx, |ui| {
            // --- Identity ---
            let submap = if info.submapper == 0 {
                String::new()
            } else {
                format!(".{}", info.submapper)
            };
            ui.label(
                egui::RichText::new(format!(
                    "Mapper #{}{submap} — {}",
                    info.mapper_id, info.name
                ))
                .strong(),
            );
            ui.horizontal(|ui| {
                if !info.tier.is_empty() {
                    ui.label(format!("Tier: {}", info.tier));
                    ui.separator();
                }
                ui.label(format!("Mirroring: {}", info.mirroring));
            });

            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    // --- ROM / RAM sizes + bank counts ---
                    ui.separator();
                    ui.label(egui::RichText::new("ROM / RAM").strong());
                    egui::Grid::new("mapper-sizes")
                        .num_columns(2)
                        .striped(true)
                        .show(ui, |ui| {
                            // PRG-ROM with its 16 KiB / 8 KiB bank counts.
                            ui.label("PRG-ROM");
                            ui.monospace(format!(
                                "{}  ({} x 16K, {} x 8K)",
                                fmt_size(info.prg_rom_size),
                                info.prg_rom_size / 0x4000,
                                info.prg_rom_size / 0x2000
                            ));
                            ui.end_row();
                            if info.chr_rom_size > 0 {
                                ui.label("CHR-ROM");
                                ui.monospace(format!(
                                    "{}  ({} x 1K)",
                                    fmt_size(info.chr_rom_size),
                                    info.chr_rom_size / 0x400
                                ));
                                ui.end_row();
                            }
                            if info.chr_ram_size > 0 {
                                ui.label("CHR-RAM");
                                ui.monospace(fmt_size(info.chr_ram_size));
                                ui.end_row();
                            }
                            ui.label("PRG-RAM");
                            ui.monospace(format!(
                                "{}{}",
                                fmt_size(info.prg_ram_size),
                                if info.has_battery {
                                    "  (battery / NVRAM)"
                                } else {
                                    ""
                                }
                            ));
                            ui.end_row();
                        });

                    // --- Hardware features (IRQ + expansion audio) ---
                    if !info.irq_kind.is_empty() || info.expansion_audio.is_some() {
                        ui.separator();
                        ui.label(egui::RichText::new("Hardware").strong());
                        if !info.irq_kind.is_empty() {
                            ui.monospace(format!("       IRQ = {}", info.irq_kind));
                        }
                        if let Some(chip) = info.expansion_audio {
                            ui.monospace(format!("     Audio = {chip}"));
                        }
                    }

                    // --- Live bank mapping (PRG window $8000-$FFFF) ---
                    if !info.prg_banks.is_empty() {
                        ui.separator();
                        ui.label(egui::RichText::new("PRG banks ($8000-$FFFF)").strong());
                        for (k, v) in &info.prg_banks {
                            ui.monospace(format!("{k:>10} = {v}"));
                        }
                    }
                    // --- Live bank mapping (CHR window $0000-$1FFF) ---
                    if !info.chr_banks.is_empty() {
                        ui.separator();
                        ui.label(egui::RichText::new("CHR banks ($0000-$1FFF)").strong());
                        for (k, v) in &info.chr_banks {
                            ui.monospace(format!("{k:>10} = {v}"));
                        }
                    }
                    // --- IRQ counter live state ---
                    if !info.irq_state.is_empty() {
                        ui.separator();
                        ui.label(egui::RichText::new("IRQ counter").strong());
                        for (k, v) in &info.irq_state {
                            ui.monospace(format!("{k:>10} = {v}"));
                        }
                    }
                    // --- Extra (register last-write log, mode flags, ...) ---
                    if !info.extra.is_empty() {
                        ui.separator();
                        ui.label(egui::RichText::new("Registers / state").strong());
                        for (k, v) in &info.extra {
                            ui.monospace(format!("{k:>10} = {v}"));
                        }
                    }
                });
        });
}
