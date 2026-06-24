//! `BasicBot` control panel (v1.8.9) — configure and run the save-state-anchored
//! input search ([`crate::basic_bot`]) against the live emulator, and show the
//! best result. The search runs synchronously under the emu lock when "Run" is
//! clicked; it restores the anchor afterwards, so the live timeline is untouched.

use crate::basic_bot::{self, BotConfig};
use rustynes_core::Nes;

/// Persistent state of the `BasicBot` panel.
pub struct BasicBotPanel {
    /// Target CPU address, entered as hex.
    addr_hex: String,
    /// Read the target as a little-endian 16-bit value.
    two_byte: bool,
    /// Frames per attempt.
    frames: usize,
    /// Number of attempts.
    attempts: usize,
    /// PRNG seed (reproducible).
    seed: u64,
    /// A status / error line.
    status: String,
    /// v1.8.9 — detached into its own OS window (egui multi-viewport).
    detached: bool,
    /// v1.8.9 — "Run search" was clicked this frame; the caller runs it after the
    /// render (so `nes` never has to be captured by the viewport callback).
    run_requested: bool,
}

impl Default for BasicBotPanel {
    fn default() -> Self {
        Self {
            addr_hex: "0000".to_owned(),
            two_byte: false,
            frames: 60,
            attempts: 200,
            seed: 0x1234_5678,
            status: String::new(),
            detached: false,
            run_requested: false,
        }
    }
}

/// Draw the `BasicBot` window. `nes` is `Some` only when a ROM is loaded under the
/// held lock; the search is disabled otherwise.
pub fn show(
    ctx: &egui::Context,
    open: &mut bool,
    state: &mut BasicBotPanel,
    nes: Option<&mut Nes>,
) {
    let can_run = nes.is_some();
    if state.detached {
        // v1.8.9 multi-viewport — render in a real OS window via egui's
        // `show_viewport_immediate`. The body takes only `can_run` (a `Copy` bool),
        // never `nes`, so the FnMut callback captures nothing that has to move.
        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("rustynes_basic_bot"),
            egui::ViewportBuilder::default()
                .with_title("BasicBot")
                .with_inner_size([340.0, 320.0]),
            |vctx, _class| {
                // A full-window Area hosts the body without the deprecated
                // context-level `CentralPanel::show`.
                egui::Area::new(egui::Id::new("basic_bot_detached"))
                    .show(vctx, |ui| body(ui, state, can_run));
                // The OS window's close button reattaches to the docked panel.
                if vctx.input(|i| i.viewport().close_requested()) {
                    state.detached = false;
                }
            },
        );
    } else {
        egui::Window::new("BasicBot")
            .open(open)
            .default_width(320.0)
            .show(ctx, |ui| body(ui, state, can_run));
    }
    // Run the search AFTER the render — `nes` is free here (not captured by any
    // closure), so it moves into `run_search` directly, no reborrow needed.
    if std::mem::take(&mut state.run_requested) {
        run_search(state, nes);
    }
}

/// The panel body (config + Run), shared by the docked window and the detached OS
/// viewport. A click sets `run_requested`; [`show`] runs the search after rendering.
fn body(ui: &mut egui::Ui, state: &mut BasicBotPanel, can_run: bool) {
    ui.checkbox(&mut state.detached, "Detach to its own window");
    ui.separator();
    ui.horizontal(|ui| {
        ui.label("Target $");
        ui.add(egui::TextEdit::singleline(&mut state.addr_hex).desired_width(60.0));
        ui.checkbox(&mut state.two_byte, "16-bit");
    });
    ui.add(egui::Slider::new(&mut state.frames, 1..=600).text("frames / attempt"));
    ui.add(egui::Slider::new(&mut state.attempts, 1..=5000).text("attempts"));
    ui.horizontal(|ui| {
        ui.label("seed");
        ui.add(egui::DragValue::new(&mut state.seed).hexadecimal(8, false, true));
    });
    ui.weak("Maximizes the target value over random player-1 inputs.");

    if ui
        .add_enabled(can_run, egui::Button::new("\u{25B6} Run search"))
        .clicked()
    {
        state.run_requested = true;
    }
    if !can_run {
        ui.weak("Load a ROM to run the bot.");
    }

    if !state.status.is_empty() {
        ui.separator();
        ui.label(&state.status);
    }
}

/// Parse the address + run the search, recording a status line.
fn run_search(state: &mut BasicBotPanel, nes: Option<&mut Nes>) {
    let Some(nes) = nes else { return };
    let hex = state
        .addr_hex
        .trim()
        .trim_start_matches("0x")
        .trim_start_matches("0X");
    let Ok(addr) = u16::from_str_radix(hex, 16) else {
        "Invalid target address (enter hex, e.g. 0075).".clone_into(&mut state.status);
        return;
    };
    let cfg = BotConfig {
        target_addr: addr,
        two_byte: state.two_byte,
        frames: state.frames,
        attempts: state.attempts,
        pool: BotConfig::default().pool,
        seed: state.seed,
    };
    let r = basic_bot::search(nes, &cfg, None);
    state.status = format!(
        "Best ${addr:04X} = {} over {} attempts ({} input frames).",
        r.best_score,
        r.attempts_run,
        r.best_inputs.len()
    );
}
