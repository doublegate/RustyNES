#![allow(
    clippy::cast_lossless,
    clippy::cast_possible_truncation,
    clippy::too_many_lines
)]
//! v1.7.0 "Forge" Workstream A2 — iNES / NES 2.0 header editor + read-only
//! "Cartridge Info" pane.
//!
//! Inspects (and optionally edits) the 16-byte iNES / NES 2.0 header of a ROM
//! file **on disk** — NOT the running core. The read-only Cartridge Info pane
//! (the small subset: mapper, submapper, mirroring, PRG/CHR sizes, battery,
//! region, console type, RAM sizes) is the default; an explicit "Edit header"
//! toggle reveals the editors and a "Write to file..." action.
//!
//! Decoding and re-encoding both reuse the core's canonical
//! [`rustynes_core::rustynes_mappers::parse_header`] /
//! [`rustynes_core::rustynes_mappers::serialize_header`] round-trip, so the
//! editor can never drift from the loader. Source inspiration: FCEUX
//! `iNesHeaderEditor.cpp`. See `docs/cartridge-format.md`.
//!
//! Native-only: editing a file on disk needs `std::fs` + the `rfd` picker,
//! both native-only deps. The whole module is `cfg`-gated out of the wasm
//! build (no filesystem there).

use std::io::{Read, Seek, SeekFrom, Write};

use rustynes_core::rustynes_mappers::{
    ConsoleType, Header, Mirroring, Region, VsPpuType, parse_header, serialize_header,
};

/// Length of the iNES / NES 2.0 header in bytes.
const HEADER_LEN: usize = 16;

/// Persistent state of the header editor / Cartridge Info panel.
#[derive(Default)]
pub struct HeaderEditorState {
    /// The currently-loaded ROM path + its parsed header. `None` until a file
    /// is opened.
    loaded: Option<Loaded>,
    /// Master edit toggle. Off by default → the pane is read-only.
    editing: bool,
    /// Last status / error line.
    status: String,
}

struct Loaded {
    path: std::path::PathBuf,
    /// The parsed (and editable) header.
    header: Header,
    /// Raw PRG/CHR unit counts shown for editing (16 KiB / 8 KiB units). Only
    /// meaningful when the size is expressible in the standard notation (which
    /// the editor restricts to).
    prg_units: u16,
    chr_units: u16,
}

/// Render the Cartridge Info / header-editor window.
pub fn show(ctx: &egui::Context, open: &mut bool, state: &mut HeaderEditorState) {
    egui::Window::new("Cartridge Info / Header")
        .open(open)
        .default_pos([120.0, 80.0])
        .default_size([420.0, 460.0])
        .resizable(true)
        .show(ctx, |ui| {
            if ui.button("Open ROM file...").clicked() {
                open_file(state);
            }
            let Some(loaded) = state.loaded.as_mut() else {
                ui.separator();
                ui.weak("Open a .nes / NES 2.0 ROM file to inspect its header.");
                return;
            };
            ui.monospace(format!("file: {}", loaded.path.display()));
            ui.separator();
            ui.checkbox(&mut state.editing, "Edit header (writes the file)");
            ui.separator();
            if state.editing {
                editor(ui, loaded);
                ui.separator();
                if ui.button("Write header to file").clicked() {
                    state.status = write_header_to_file(loaded);
                }
            } else {
                info_pane(ui, &loaded.header);
            }
            if !state.status.is_empty() {
                ui.separator();
                ui.weak(&state.status);
            }
        });
}

/// The read-only Cartridge Info pane (the small subset, shipped first).
fn info_pane(ui: &mut egui::Ui, h: &Header) {
    egui::Grid::new("cart-info")
        .num_columns(2)
        .striped(true)
        .show(ui, |ui| {
            row(ui, "Format", if h.is_nes2 { "NES 2.0" } else { "iNES 1.0" });
            row(ui, "Mapper", &format!("{}", h.mapper_id));
            row(ui, "Submapper", &format!("{}", h.submapper));
            row(ui, "Mirroring", &format!("{:?}", h.mirroring));
            row(
                ui,
                "PRG-ROM",
                &format!("{} bytes ({} KiB)", h.prg_size, h.prg_size / 1024),
            );
            row(
                ui,
                "CHR-ROM",
                &format!("{} bytes ({} KiB)", h.chr_size, h.chr_size / 1024),
            );
            row(ui, "PRG-RAM", &format!("{} bytes", h.prg_ram_size));
            row(ui, "CHR-RAM", &format!("{} bytes", h.chr_ram_size));
            row(ui, "Battery", if h.has_battery { "yes" } else { "no" });
            row(ui, "Trainer", if h.has_trainer { "yes" } else { "no" });
            row(ui, "Region", &format!("{:?}", h.region));
            row(ui, "Console", &format!("{:?}", h.console_type));
            if h.console_type == ConsoleType::VsSystem {
                row(ui, "Vs. PPU", &format!("{:?}", h.vs_ppu_type));
                row(
                    ui,
                    "Vs. DualSystem",
                    if h.vs_dual_system { "yes" } else { "no" },
                );
            }
            ui.end_row();
        });
}

fn row(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.label(label);
    ui.monospace(value);
    ui.end_row();
}

/// The editor: combo boxes + numeric fields over the directly-bit-mapped
/// header fields. Sizes are edited in their unit counts (16 KiB / 8 KiB) so
/// the re-serialization stays in the standard (non-exponent) notation.
fn editor(ui: &mut egui::Ui, loaded: &mut Loaded) {
    let h = &mut loaded.header;
    ui.checkbox(&mut h.is_nes2, "NES 2.0 (vs iNES 1.0)");

    ui.horizontal(|ui| {
        ui.label("Mapper:");
        ui.add(egui::DragValue::new(&mut h.mapper_id).range(0..=4095));
        if h.is_nes2 {
            ui.label("Submapper:");
            ui.add(egui::DragValue::new(&mut h.submapper).range(0..=15));
        }
    });

    egui::ComboBox::from_label("Mirroring")
        .selected_text(format!("{:?}", h.mirroring))
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut h.mirroring, Mirroring::Horizontal, "Horizontal");
            ui.selectable_value(&mut h.mirroring, Mirroring::Vertical, "Vertical");
            ui.selectable_value(&mut h.mirroring, Mirroring::FourScreen, "FourScreen");
        });
    // Keep the four-screen flag consistent with the mirroring choice (the
    // serializer reads both).
    h.four_screen = matches!(h.mirroring, Mirroring::FourScreen);

    ui.horizontal(|ui| {
        ui.label("PRG (16 KiB units):");
        if ui
            .add(egui::DragValue::new(&mut loaded.prg_units).range(0..=4095))
            .changed()
        {
            h.prg_size = usize::from(loaded.prg_units) * 16 * 1024;
        }
    });
    ui.horizontal(|ui| {
        ui.label("CHR (8 KiB units):");
        if ui
            .add(egui::DragValue::new(&mut loaded.chr_units).range(0..=4095))
            .changed()
        {
            h.chr_size = usize::from(loaded.chr_units) * 8 * 1024;
        }
    });

    ui.checkbox(&mut h.has_battery, "Battery-backed save RAM");
    ui.checkbox(&mut h.has_trainer, "512-byte trainer present");

    if h.is_nes2 {
        egui::ComboBox::from_label("Region")
            .selected_text(format!("{:?}", h.region))
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut h.region, Region::Ntsc, "NTSC");
                ui.selectable_value(&mut h.region, Region::Pal, "PAL");
                ui.selectable_value(&mut h.region, Region::Multi, "Multi");
                ui.selectable_value(&mut h.region, Region::Dendy, "Dendy");
            });
        egui::ComboBox::from_label("Console")
            .selected_text(format!("{:?}", h.console_type))
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut h.console_type, ConsoleType::Nes, "NES/Famicom");
                ui.selectable_value(&mut h.console_type, ConsoleType::VsSystem, "Vs. System");
                ui.selectable_value(
                    &mut h.console_type,
                    ConsoleType::Playchoice10,
                    "PlayChoice-10",
                );
                ui.selectable_value(&mut h.console_type, ConsoleType::Extended, "Extended");
            });
        if h.console_type == ConsoleType::VsSystem {
            ui.checkbox(&mut h.vs_dual_system, "Vs. DualSystem board");
        } else {
            h.vs_ppu_type = VsPpuType::None;
            h.vs_dual_system = false;
        }
        ui.horizontal(|ui| {
            ui.label("PRG-RAM (bytes):");
            ui.add(egui::DragValue::new(&mut h.prg_ram_size));
        });
        ui.horizontal(|ui| {
            ui.label("CHR-RAM (bytes):");
            ui.add(egui::DragValue::new(&mut h.chr_ram_size));
        });
    }

    ui.weak(
        "Edits the 16-byte header of the file on disk only (not the running \
         core). Sizes are stored in standard 16K/8K-unit notation.",
    );
}

/// Open a ROM file, parse its header, and seed the editor state.
fn open_file(state: &mut HeaderEditorState) {
    let Some(path) = rfd::FileDialog::new()
        .add_filter("NES ROM", &["nes", "unf", "unif"])
        .pick_file()
    else {
        return;
    };
    // Read only the 16-byte header rather than the whole ROM file.
    match read_header_bytes(&path) {
        Ok(bytes) => match parse_header(&bytes) {
            Ok(header) => {
                state.loaded = Some(Loaded {
                    path,
                    header,
                    prg_units: (header.prg_size / (16 * 1024)) as u16,
                    chr_units: (header.chr_size / (8 * 1024)) as u16,
                });
                state.status = "header loaded".into();
            }
            Err(e) => state.status = format!("not a valid iNES/NES2.0 header: {e:?}"),
        },
        Err(e) => state.status = format!("read failed: {e}"),
    }
}

/// Read just the first [`HEADER_LEN`] bytes of a ROM file (the header).
fn read_header_bytes(path: &std::path::Path) -> std::io::Result<[u8; HEADER_LEN]> {
    let mut file = std::fs::File::open(path)?;
    let mut buf = [0u8; HEADER_LEN];
    file.read_exact(&mut buf)?;
    Ok(buf)
}

/// Re-serialize the edited header and overwrite the first 16 bytes of the file
/// in place (the ROM body is untouched). Seeks + writes only the header — the
/// rest of the file is never read or rewritten. Returns a status string.
fn write_header_to_file(loaded: &Loaded) -> String {
    let new_header = serialize_header(&loaded.header);
    match overwrite_header(&loaded.path, &new_header) {
        Ok(()) => "header written".into(),
        Err(e) => format!("write failed: {e}"),
    }
}

/// Overwrite the first [`HEADER_LEN`] bytes of the file in place via a seek +
/// partial write, leaving the ROM body untouched.
fn overwrite_header(path: &std::path::Path, header: &[u8; HEADER_LEN]) -> std::io::Result<()> {
    let mut file = std::fs::OpenOptions::new().write(true).open(path)?;
    // Guard against truncated files: the body after the header must exist.
    if file.metadata()?.len() < HEADER_LEN as u64 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "file too short to hold a header",
        ));
    }
    file.seek(SeekFrom::Start(0))?;
    file.write_all(header)?;
    Ok(())
}
