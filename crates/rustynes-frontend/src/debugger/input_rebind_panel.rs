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
//! Input rebinding modal (T-52-007; gamepad rows added v1.6.0 Sprint 2).
//!
//! Flow:
//!
//! 1. The user clicks a binding row's "rebind" button.
//! 2. The panel switches into "capturing" mode and shows a modal that
//!    listens for the next input. Keyboard slots capture the next key
//!    press (winit window event); gamepad slots capture the next
//!    `gilrs::Button` press (native only).
//! 3. The captured value writes back into the in-memory [`Config`] and
//!    flags the bindings dirty so the live input maps are rebuilt (the
//!    rebind takes effect immediately, not just after restart).
//! 4. "Save" persists the config to disk via [`Config::save`]. "Reset to
//!    defaults" reverts to [`Config::default`].
//!
//! The TOML-on-disk format is unchanged for keyboard bindings; the
//! `[input.gamepad*]` sections are new but `#[serde(default)]`, so older
//! files keep working.

use winit::event::{ElementState, WindowEvent};
use winit::keyboard::{KeyCode, PhysicalKey};

use crate::config::Config;

/// Which binding the user is currently rebinding (if any). Keyboard and
/// gamepad slots are distinguished so the capture path knows which kind
/// of event to listen for.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum Slot {
    // Player 1 keyboard.
    P1Up,
    P1Down,
    P1Left,
    P1Right,
    P1A,
    P1B,
    P1Select,
    P1Start,
    // Player 2 keyboard.
    P2Up,
    P2Down,
    P2Left,
    P2Right,
    P2A,
    P2B,
    P2Select,
    P2Start,
    // Player 3 keyboard (v1.7.0, Four Score).
    P3Up,
    P3Down,
    P3Left,
    P3Right,
    P3A,
    P3B,
    P3Select,
    P3Start,
    // Player 4 keyboard (v1.7.0, Four Score).
    P4Up,
    P4Down,
    P4Left,
    P4Right,
    P4A,
    P4B,
    P4Select,
    P4Start,
    // System keyboard.
    Quit,
    SaveState,
    LoadState,
    Rewind,
    Reset,
    PowerCycle,
    DebugOverlay,
    // Player 1 gamepad.
    G1Up,
    G1Down,
    G1Left,
    G1Right,
    G1A,
    G1B,
    G1Select,
    G1Start,
    // Player 2 gamepad.
    G2Up,
    G2Down,
    G2Left,
    G2Right,
    G2A,
    G2B,
    G2Select,
    G2Start,
    // Player 3 gamepad (v1.7.0, Four Score).
    G3Up,
    G3Down,
    G3Left,
    G3Right,
    G3A,
    G3B,
    G3Select,
    G3Start,
    // Player 4 gamepad (v1.7.0, Four Score).
    G4Up,
    G4Down,
    G4Left,
    G4Right,
    G4A,
    G4B,
    G4Select,
    G4Start,
}

impl Slot {
    /// `true` if this slot binds a gamepad button (captured from gilrs);
    /// `false` for keyboard slots (captured from winit key events).
    const fn is_gamepad(self) -> bool {
        matches!(
            self,
            Self::G1Up
                | Self::G1Down
                | Self::G1Left
                | Self::G1Right
                | Self::G1A
                | Self::G1B
                | Self::G1Select
                | Self::G1Start
                | Self::G2Up
                | Self::G2Down
                | Self::G2Left
                | Self::G2Right
                | Self::G2A
                | Self::G2B
                | Self::G2Select
                | Self::G2Start
                | Self::G3Up
                | Self::G3Down
                | Self::G3Left
                | Self::G3Right
                | Self::G3A
                | Self::G3B
                | Self::G3Select
                | Self::G3Start
                | Self::G4Up
                | Self::G4Down
                | Self::G4Left
                | Self::G4Right
                | Self::G4A
                | Self::G4B
                | Self::G4Select
                | Self::G4Start
        )
    }
}

/// Keyboard rows: player 1, player 2, and the system actions.
const KB_ROWS: &[(Slot, &str)] = &[
    (Slot::P1Up, "Player1 Up"),
    (Slot::P1Down, "Player1 Down"),
    (Slot::P1Left, "Player1 Left"),
    (Slot::P1Right, "Player1 Right"),
    (Slot::P1A, "Player1 A"),
    (Slot::P1B, "Player1 B"),
    (Slot::P1Select, "Player1 Select"),
    (Slot::P1Start, "Player1 Start"),
    (Slot::P2Up, "Player2 Up"),
    (Slot::P2Down, "Player2 Down"),
    (Slot::P2Left, "Player2 Left"),
    (Slot::P2Right, "Player2 Right"),
    (Slot::P2A, "Player2 A"),
    (Slot::P2B, "Player2 B"),
    (Slot::P2Select, "Player2 Select"),
    (Slot::P2Start, "Player2 Start"),
    (Slot::P3Up, "Player3 Up"),
    (Slot::P3Down, "Player3 Down"),
    (Slot::P3Left, "Player3 Left"),
    (Slot::P3Right, "Player3 Right"),
    (Slot::P3A, "Player3 A"),
    (Slot::P3B, "Player3 B"),
    (Slot::P3Select, "Player3 Select"),
    (Slot::P3Start, "Player3 Start"),
    (Slot::P4Up, "Player4 Up"),
    (Slot::P4Down, "Player4 Down"),
    (Slot::P4Left, "Player4 Left"),
    (Slot::P4Right, "Player4 Right"),
    (Slot::P4A, "Player4 A"),
    (Slot::P4B, "Player4 B"),
    (Slot::P4Select, "Player4 Select"),
    (Slot::P4Start, "Player4 Start"),
    (Slot::Quit, "Quit"),
    (Slot::SaveState, "Save state"),
    (Slot::LoadState, "Load state"),
    (Slot::Rewind, "Rewind (hold)"),
    (Slot::Reset, "Reset"),
    (Slot::PowerCycle, "Power cycle"),
    (Slot::DebugOverlay, "Debug overlay"),
];

/// Gamepad rows: player 1 and player 2. Rendered (and captured) only on
/// native builds — wasm32 has no gilrs runtime.
const PAD_ROWS: &[(Slot, &str)] = &[
    (Slot::G1Up, "Pad1 Up"),
    (Slot::G1Down, "Pad1 Down"),
    (Slot::G1Left, "Pad1 Left"),
    (Slot::G1Right, "Pad1 Right"),
    (Slot::G1A, "Pad1 A"),
    (Slot::G1B, "Pad1 B"),
    (Slot::G1Select, "Pad1 Select"),
    (Slot::G1Start, "Pad1 Start"),
    (Slot::G2Up, "Pad2 Up"),
    (Slot::G2Down, "Pad2 Down"),
    (Slot::G2Left, "Pad2 Left"),
    (Slot::G2Right, "Pad2 Right"),
    (Slot::G2A, "Pad2 A"),
    (Slot::G2B, "Pad2 B"),
    (Slot::G2Select, "Pad2 Select"),
    (Slot::G2Start, "Pad2 Start"),
    (Slot::G3Up, "Pad3 Up"),
    (Slot::G3Down, "Pad3 Down"),
    (Slot::G3Left, "Pad3 Left"),
    (Slot::G3Right, "Pad3 Right"),
    (Slot::G3A, "Pad3 A"),
    (Slot::G3B, "Pad3 B"),
    (Slot::G3Select, "Pad3 Select"),
    (Slot::G3Start, "Pad3 Start"),
    (Slot::G4Up, "Pad4 Up"),
    (Slot::G4Down, "Pad4 Down"),
    (Slot::G4Left, "Pad4 Left"),
    (Slot::G4Right, "Pad4 Right"),
    (Slot::G4A, "Pad4 A"),
    (Slot::G4B, "Pad4 B"),
    (Slot::G4Select, "Pad4 Select"),
    (Slot::G4Start, "Pad4 Start"),
];

/// Persistent state of the rebinding panel.
#[derive(Debug, Default)]
pub struct InputPanelState {
    /// `Some(slot)` while listening for the next input.
    pending: Option<Slot>,
    /// `Some((slot, name))` if the next UI tick should apply a captured
    /// rebind (keycode name for keyboard slots, `gilrs::Button` name for
    /// gamepad slots).
    captured: Option<(Slot, String)>,
    /// Status text after save / failure.
    status: String,
    /// Set when a rebind (or reset) changed the config; the app polls
    /// [`Self::take_bindings_dirty`] to rebuild the live input maps.
    bindings_dirty: bool,
}

impl InputPanelState {
    /// `true` if currently waiting for an input capture (keyboard or
    /// gamepad). Used by the tests; the app gates keyboard input on the
    /// narrower [`Self::is_capturing_keyboard`].
    #[allow(dead_code)]
    pub const fn is_capturing(&self) -> bool {
        self.pending.is_some()
    }

    /// `true` if currently waiting for a *keyboard* capture (the app
    /// gates emulator keyboard input on this so the captured key doesn't
    /// also drive the game). A pending *gamepad* capture does not gate
    /// keyboard input.
    pub fn is_capturing_keyboard(&self) -> bool {
        self.pending.is_some_and(|s| !s.is_gamepad())
    }

    /// Return (and clear) whether a rebind changed the config since the
    /// last poll.
    pub fn take_bindings_dirty(&mut self) -> bool {
        core::mem::take(&mut self.bindings_dirty)
    }

    /// Maybe consume a window event as a *keyboard* rebinding capture.
    pub fn maybe_capture(&mut self, event: &WindowEvent) {
        let Some(slot) = self.pending else {
            return;
        };
        if slot.is_gamepad() {
            return;
        }
        let WindowEvent::KeyboardInput { event, .. } = event else {
            return;
        };
        if event.state != ElementState::Pressed {
            return;
        }
        let PhysicalKey::Code(code) = event.physical_key else {
            return;
        };
        if matches!(code, KeyCode::Escape) {
            self.pending = None;
            self.status = "(cancelled)".into();
            return;
        }
        let name = format!("{code:?}");
        self.captured = Some((slot, name));
        self.pending = None;
    }

    /// Maybe consume a `gilrs` event as a *gamepad* rebinding capture.
    /// Binds on the next `ButtonPressed`; ignores axis / release events
    /// and `Button::Unknown`. Native only.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn maybe_capture_gamepad(&mut self, event: &gilrs::EventType) {
        if let gilrs::EventType::ButtonPressed(btn, _) = *event {
            self.capture_button(btn);
        }
    }

    /// Pure capture step: bind `btn` to the pending gamepad slot, if any.
    /// Ignores `Button::Unknown` and non-gamepad pending slots. Factored
    /// out of [`Self::maybe_capture_gamepad`] so it's unit-testable
    /// without constructing an opaque `gilrs::Code`.
    #[cfg(not(target_arch = "wasm32"))]
    fn capture_button(&mut self, btn: gilrs::Button) {
        let Some(slot) = self.pending else {
            return;
        };
        if !slot.is_gamepad() || btn == gilrs::Button::Unknown {
            return;
        }
        self.captured = Some((slot, format!("{btn:?}")));
        self.pending = None;
    }
}

pub fn show(
    ctx: &egui::Context,
    open: &mut bool,
    state: &mut InputPanelState,
    config: &mut Config,
) {
    egui::Window::new("Input bindings")
        .open(open)
        .default_pos([560.0, 64.0])
        .default_size([460.0, 520.0])
        .resizable(true)
        .show(ctx, |ui| {
            body(ui, state, config);
        });
}

/// The input-rebind window body, reusable from the always-on UX shell's
/// Settings -> Input tab. Applies any captured rebind from the previous input
/// event, then renders the Four Score / port-2 device controls and the
/// keyboard + gamepad binding grids. `state` is the SAME [`InputPanelState`]
/// the debugger owns, so the rebind-capture flow + `bindings_dirty` polling are
/// shared between the two surfaces.
pub fn body(ui: &mut egui::Ui, state: &mut InputPanelState, config: &mut Config) {
    // Apply any captured rebind from the previous input event.
    if let Some((slot, name)) = state.captured.take() {
        apply_slot(config, slot, &name);
        state.status = format!("Rebound {} -> {name}", slot_label(slot));
        state.bindings_dirty = true;
    }

    {
        if state.pending.is_some() {
            let prompt = if state.pending.is_some_and(Slot::is_gamepad) {
                "Press any gamepad button (rebind again to cancel)"
            } else {
                "Press any key (Esc to cancel)"
            };
            egui::Frame::default()
                .fill(egui::Color32::from_black_alpha(220))
                .show(ui, |ui| {
                    ui.label(egui::RichText::new(prompt).strong());
                });
        } else if !state.status.is_empty() {
            ui.label(state.status.clone());
        }

        ui.horizontal(|ui| {
            if ui.button("Save to disk").clicked() {
                match config.save() {
                    Ok(()) => state.status = "Saved.".into(),
                    Err(e) => state.status = format!("save error: {e}"),
                }
            }
            if ui.button("Reset to defaults").clicked() {
                *config = Config::default();
                state.status = "Defaults restored (unsaved).".into();
                state.bindings_dirty = true;
            }
        });

        // Four Score (v1.7.0): a checkbox toggling the 4-player adapter.
        // Flagging the bindings dirty routes the new value to
        // `nes.set_four_score` via the app's reload path.
        if ui
            .checkbox(&mut config.input.four_score, "Four Score (4-player)")
            .changed()
        {
            state.bindings_dirty = true;
        }

        // v1.0.0 — analog-stick D-pad deadzone (0.05..=0.95). Drives every
        // gamepad's `axis_deadzone` (it is read per player at map rebuild and
        // clamped by `input.rs`); flagging `bindings_dirty` rebuilds the live
        // `InputState` so the change applies without a relaunch. Edits all four
        // pad sections so a single slider covers the common single-pad case.
        ui.horizontal(|ui| {
            ui.label("Gamepad stick deadzone");
            let mut dz = config.input.gamepad1.axis_deadzone.clamp(0.05, 0.95);
            if ui
                .add(egui::Slider::new(&mut dz, 0.05..=0.95).fixed_decimals(2))
                .changed()
            {
                for pad in [
                    &mut config.input.gamepad1,
                    &mut config.input.gamepad2,
                    &mut config.input.gamepad3,
                    &mut config.input.gamepad4,
                ] {
                    pad.axis_deadzone = dz;
                }
                state.bindings_dirty = true;
            }
        });

        // v1.1.0 beta.1 (T-110-B2) — turbo / autofire. Takes effect immediately
        // (frame_inputs reads the live config each frame); off by default. The
        // strobe is applied where input meets the NES keyed on the emulated
        // frame, so it is deterministic + rollback / TAS / netplay-safe.
        ui.separator();
        ui.label(egui::RichText::new("Turbo / autofire").strong());
        ui.horizontal(|ui| {
            ui.checkbox(&mut config.input.turbo_a, "Turbo A");
            ui.checkbox(&mut config.input.turbo_b, "Turbo B");
        });
        ui.horizontal(|ui| {
            ui.label("Turbo speed");
            // Period in frames per on/off half-cycle: 1 = ~30 Hz, 4 = ~7.5 Hz.
            // Normalize the persisted value into range up front, so a config
            // loaded with an out-of-range `turbo_period` (e.g. 0 or 999) is
            // corrected even if the user never touches the slider.
            config.input.turbo_period = config.input.turbo_period.clamp(1, 8);
            let mut period = config.input.turbo_period;
            if ui
                .add(egui::Slider::new(&mut period, 1..=8).text("frames"))
                .changed()
            {
                config.input.turbo_period = period;
            }
        });

        // v2.1.0 — non-standard device on the player-2 port ($4017).
        // Selecting one routes through the app's reload path
        // (`sync_expansion_device`). Mouse drives aim/position; left
        // mouse button = Zapper trigger / Vaus fire.
        {
            use crate::config::ExpansionDevice;
            let mut dev = config.input.expansion_device;
            egui::ComboBox::from_label("Port 2 device ($4017)")
                .selected_text(match dev {
                    ExpansionDevice::None => "Standard controller",
                    ExpansionDevice::Zapper => "Zapper (light gun)",
                    ExpansionDevice::Vaus => "Vaus (Arkanoid paddle)",
                    ExpansionDevice::PowerPad => "Power Pad (mat)",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut dev, ExpansionDevice::None, "Standard controller");
                    ui.selectable_value(&mut dev, ExpansionDevice::Zapper, "Zapper (light gun)");
                    ui.selectable_value(&mut dev, ExpansionDevice::Vaus, "Vaus (Arkanoid paddle)");
                    ui.selectable_value(&mut dev, ExpansionDevice::PowerPad, "Power Pad (mat)");
                });
            if dev != config.input.expansion_device {
                config.input.expansion_device = dev;
                state.bindings_dirty = true;
            }
        }

        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.separator();
            ui.label(egui::RichText::new("Keyboard").strong());
            binding_grid(ui, "kb-grid", "Key", KB_ROWS, state, config);

            // Gamepad rows: native only (no gilrs on wasm32).
            #[cfg(not(target_arch = "wasm32"))]
            {
                ui.separator();
                ui.label(egui::RichText::new("Gamepad").strong());
                binding_grid(ui, "pad-grid", "Button", PAD_ROWS, state, config);
            }
            #[cfg(target_arch = "wasm32")]
            let _ = PAD_ROWS;
        });
    }
}

/// Render one labelled grid of binding rows, wiring each "rebind" button
/// to set the pending slot.
fn binding_grid(
    ui: &mut egui::Ui,
    id: &str,
    value_col: &str,
    rows: &[(Slot, &str)],
    state: &mut InputPanelState,
    config: &Config,
) {
    egui::Grid::new(id)
        .num_columns(3)
        .striped(true)
        .show(ui, |ui| {
            ui.label("Action");
            ui.label(value_col);
            ui.label("");
            ui.end_row();
            for &(slot, label) in rows {
                ui.label(label);
                ui.monospace(read_slot(config, slot));
                if ui.button("rebind").clicked() {
                    state.pending = Some(slot);
                    state.status.clear();
                }
                ui.end_row();
            }
        });
}

fn slot_label(slot: Slot) -> &'static str {
    KB_ROWS
        .iter()
        .chain(PAD_ROWS)
        .find(|(s, _)| *s == slot)
        .map_or("?", |(_, l)| *l)
}

fn read_slot(cfg: &Config, slot: Slot) -> String {
    let i = &cfg.input;
    match slot {
        Slot::P1Up => i.player1.up.clone(),
        Slot::P1Down => i.player1.down.clone(),
        Slot::P1Left => i.player1.left.clone(),
        Slot::P1Right => i.player1.right.clone(),
        Slot::P1A => i.player1.a.clone(),
        Slot::P1B => i.player1.b.clone(),
        Slot::P1Select => i.player1.select.clone(),
        Slot::P1Start => i.player1.start.clone(),
        Slot::P2Up => i.player2.up.clone(),
        Slot::P2Down => i.player2.down.clone(),
        Slot::P2Left => i.player2.left.clone(),
        Slot::P2Right => i.player2.right.clone(),
        Slot::P2A => i.player2.a.clone(),
        Slot::P2B => i.player2.b.clone(),
        Slot::P2Select => i.player2.select.clone(),
        Slot::P2Start => i.player2.start.clone(),
        Slot::P3Up => i.player3.up.clone(),
        Slot::P3Down => i.player3.down.clone(),
        Slot::P3Left => i.player3.left.clone(),
        Slot::P3Right => i.player3.right.clone(),
        Slot::P3A => i.player3.a.clone(),
        Slot::P3B => i.player3.b.clone(),
        Slot::P3Select => i.player3.select.clone(),
        Slot::P3Start => i.player3.start.clone(),
        Slot::P4Up => i.player4.up.clone(),
        Slot::P4Down => i.player4.down.clone(),
        Slot::P4Left => i.player4.left.clone(),
        Slot::P4Right => i.player4.right.clone(),
        Slot::P4A => i.player4.a.clone(),
        Slot::P4B => i.player4.b.clone(),
        Slot::P4Select => i.player4.select.clone(),
        Slot::P4Start => i.player4.start.clone(),
        Slot::Quit => i.system.quit.clone(),
        Slot::SaveState => i.system.save_state.clone(),
        Slot::LoadState => i.system.load_state.clone(),
        Slot::Rewind => i.system.rewind.clone(),
        Slot::Reset => i.system.reset.clone(),
        Slot::PowerCycle => i.system.power_cycle.clone(),
        Slot::DebugOverlay => i.system.debug_overlay.clone(),
        Slot::G1Up => i.gamepad1.up.clone(),
        Slot::G1Down => i.gamepad1.down.clone(),
        Slot::G1Left => i.gamepad1.left.clone(),
        Slot::G1Right => i.gamepad1.right.clone(),
        Slot::G1A => i.gamepad1.a.clone(),
        Slot::G1B => i.gamepad1.b.clone(),
        Slot::G1Select => i.gamepad1.select.clone(),
        Slot::G1Start => i.gamepad1.start.clone(),
        Slot::G2Up => i.gamepad2.up.clone(),
        Slot::G2Down => i.gamepad2.down.clone(),
        Slot::G2Left => i.gamepad2.left.clone(),
        Slot::G2Right => i.gamepad2.right.clone(),
        Slot::G2A => i.gamepad2.a.clone(),
        Slot::G2B => i.gamepad2.b.clone(),
        Slot::G2Select => i.gamepad2.select.clone(),
        Slot::G2Start => i.gamepad2.start.clone(),
        Slot::G3Up => i.gamepad3.up.clone(),
        Slot::G3Down => i.gamepad3.down.clone(),
        Slot::G3Left => i.gamepad3.left.clone(),
        Slot::G3Right => i.gamepad3.right.clone(),
        Slot::G3A => i.gamepad3.a.clone(),
        Slot::G3B => i.gamepad3.b.clone(),
        Slot::G3Select => i.gamepad3.select.clone(),
        Slot::G3Start => i.gamepad3.start.clone(),
        Slot::G4Up => i.gamepad4.up.clone(),
        Slot::G4Down => i.gamepad4.down.clone(),
        Slot::G4Left => i.gamepad4.left.clone(),
        Slot::G4Right => i.gamepad4.right.clone(),
        Slot::G4A => i.gamepad4.a.clone(),
        Slot::G4B => i.gamepad4.b.clone(),
        Slot::G4Select => i.gamepad4.select.clone(),
        Slot::G4Start => i.gamepad4.start.clone(),
    }
}

fn apply_slot(cfg: &mut Config, slot: Slot, name: &str) {
    let i = &mut cfg.input;
    let target: &mut String = match slot {
        Slot::P1Up => &mut i.player1.up,
        Slot::P1Down => &mut i.player1.down,
        Slot::P1Left => &mut i.player1.left,
        Slot::P1Right => &mut i.player1.right,
        Slot::P1A => &mut i.player1.a,
        Slot::P1B => &mut i.player1.b,
        Slot::P1Select => &mut i.player1.select,
        Slot::P1Start => &mut i.player1.start,
        Slot::P2Up => &mut i.player2.up,
        Slot::P2Down => &mut i.player2.down,
        Slot::P2Left => &mut i.player2.left,
        Slot::P2Right => &mut i.player2.right,
        Slot::P2A => &mut i.player2.a,
        Slot::P2B => &mut i.player2.b,
        Slot::P2Select => &mut i.player2.select,
        Slot::P2Start => &mut i.player2.start,
        Slot::P3Up => &mut i.player3.up,
        Slot::P3Down => &mut i.player3.down,
        Slot::P3Left => &mut i.player3.left,
        Slot::P3Right => &mut i.player3.right,
        Slot::P3A => &mut i.player3.a,
        Slot::P3B => &mut i.player3.b,
        Slot::P3Select => &mut i.player3.select,
        Slot::P3Start => &mut i.player3.start,
        Slot::P4Up => &mut i.player4.up,
        Slot::P4Down => &mut i.player4.down,
        Slot::P4Left => &mut i.player4.left,
        Slot::P4Right => &mut i.player4.right,
        Slot::P4A => &mut i.player4.a,
        Slot::P4B => &mut i.player4.b,
        Slot::P4Select => &mut i.player4.select,
        Slot::P4Start => &mut i.player4.start,
        Slot::Quit => &mut i.system.quit,
        Slot::SaveState => &mut i.system.save_state,
        Slot::LoadState => &mut i.system.load_state,
        Slot::Rewind => &mut i.system.rewind,
        Slot::Reset => &mut i.system.reset,
        Slot::PowerCycle => &mut i.system.power_cycle,
        Slot::DebugOverlay => &mut i.system.debug_overlay,
        Slot::G1Up => &mut i.gamepad1.up,
        Slot::G1Down => &mut i.gamepad1.down,
        Slot::G1Left => &mut i.gamepad1.left,
        Slot::G1Right => &mut i.gamepad1.right,
        Slot::G1A => &mut i.gamepad1.a,
        Slot::G1B => &mut i.gamepad1.b,
        Slot::G1Select => &mut i.gamepad1.select,
        Slot::G1Start => &mut i.gamepad1.start,
        Slot::G2Up => &mut i.gamepad2.up,
        Slot::G2Down => &mut i.gamepad2.down,
        Slot::G2Left => &mut i.gamepad2.left,
        Slot::G2Right => &mut i.gamepad2.right,
        Slot::G2A => &mut i.gamepad2.a,
        Slot::G2B => &mut i.gamepad2.b,
        Slot::G2Select => &mut i.gamepad2.select,
        Slot::G2Start => &mut i.gamepad2.start,
        Slot::G3Up => &mut i.gamepad3.up,
        Slot::G3Down => &mut i.gamepad3.down,
        Slot::G3Left => &mut i.gamepad3.left,
        Slot::G3Right => &mut i.gamepad3.right,
        Slot::G3A => &mut i.gamepad3.a,
        Slot::G3B => &mut i.gamepad3.b,
        Slot::G3Select => &mut i.gamepad3.select,
        Slot::G3Start => &mut i.gamepad3.start,
        Slot::G4Up => &mut i.gamepad4.up,
        Slot::G4Down => &mut i.gamepad4.down,
        Slot::G4Left => &mut i.gamepad4.left,
        Slot::G4Right => &mut i.gamepad4.right,
        Slot::G4A => &mut i.gamepad4.a,
        Slot::G4B => &mut i.gamepad4.b,
        Slot::G4Select => &mut i.gamepad4.select,
        Slot::G4Start => &mut i.gamepad4.start,
    };
    target.clear();
    target.push_str(name);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyboard_slots_are_not_gamepad() {
        assert!(!Slot::P1Up.is_gamepad());
        assert!(!Slot::P2Start.is_gamepad());
        assert!(!Slot::Quit.is_gamepad());
    }

    #[test]
    fn gamepad_slots_report_gamepad() {
        assert!(Slot::G1A.is_gamepad());
        assert!(Slot::G2Start.is_gamepad());
    }

    #[test]
    fn apply_gamepad_slot_writes_config_and_label_resolves() {
        let mut cfg = Config::default();
        apply_slot(&mut cfg, Slot::G1A, "North");
        assert_eq!(cfg.input.gamepad1.a, "North");
        assert_eq!(read_slot(&cfg, Slot::G1A), "North");
        assert_eq!(slot_label(Slot::G1A), "Pad1 A");
    }

    #[test]
    fn apply_p2_keyboard_slot_writes_config() {
        let mut cfg = Config::default();
        apply_slot(&mut cfg, Slot::P2A, "KeyJ");
        assert_eq!(cfg.input.player2.a, "KeyJ");
        assert_eq!(read_slot(&cfg, Slot::P2A), "KeyJ");
    }

    #[test]
    fn p3_p4_slots_roundtrip_keyboard_and_gamepad() {
        // v1.7.0 Four Score rows: P3/P4 keyboard + G3/G4 gamepad slots
        // read/write the right config fields and resolve labels.
        let mut cfg = Config::default();
        apply_slot(&mut cfg, Slot::P3A, "KeyU");
        assert_eq!(cfg.input.player3.a, "KeyU");
        assert_eq!(read_slot(&cfg, Slot::P3A), "KeyU");
        assert_eq!(slot_label(Slot::P3A), "Player3 A");
        apply_slot(&mut cfg, Slot::P4Start, "Numpad3");
        assert_eq!(cfg.input.player4.start, "Numpad3");
        apply_slot(&mut cfg, Slot::G3B, "North");
        assert_eq!(cfg.input.gamepad3.b, "North");
        assert!(Slot::G4Up.is_gamepad());
        assert!(!Slot::P3Up.is_gamepad());
        assert_eq!(slot_label(Slot::G4Start), "Pad4 Start");
    }

    #[test]
    fn is_capturing_keyboard_distinguishes_slot_kind() {
        let mut s = InputPanelState {
            pending: Some(Slot::P1Up),
            ..Default::default()
        };
        assert!(s.is_capturing());
        assert!(s.is_capturing_keyboard());
        s.pending = Some(Slot::G1A);
        assert!(s.is_capturing());
        assert!(!s.is_capturing_keyboard());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn gamepad_capture_binds_next_button_press() {
        let mut s = InputPanelState {
            pending: Some(Slot::G1A),
            ..Default::default()
        };
        s.capture_button(gilrs::Button::North);
        assert_eq!(s.captured, Some((Slot::G1A, "North".to_string())));
        assert!(s.pending.is_none());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn gamepad_capture_ignores_unknown_and_keyboard_slots() {
        // Unknown button: no capture.
        let mut s = InputPanelState {
            pending: Some(Slot::G1A),
            ..Default::default()
        };
        s.capture_button(gilrs::Button::Unknown);
        assert!(s.captured.is_none());
        assert!(s.pending.is_some());
        // Pending on a keyboard slot: a pad press must not capture it.
        let mut s = InputPanelState {
            pending: Some(Slot::P1A),
            ..Default::default()
        };
        s.capture_button(gilrs::Button::North);
        assert!(s.captured.is_none());
        assert!(s.pending.is_some());
    }
}
