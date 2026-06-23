//! Desktop on-screen "virtual pad" — a translucent NES controller overlaid on
//! the bottom of the gameplay area (issue #111, the `ControlsOverlay` request).
//!
//! Rather than a separate window (which shrinks the visible game), the controls
//! are drawn as two chromeless [`egui::Area`] overlays anchored to the bottom
//! corners — the D-pad bottom-left, the B / A face buttons + Select / Start
//! bottom-right — with semi-transparent, custom-painted buttons so the full
//! frame stays visible underneath. Held buttons fold into the per-frame input at
//! the SAME late-latch as the keyboard / gamepad (see `App::frame_inputs`), so an
//! on-screen press records and replays identically in TAS movies and netplay.
//! Native-only — the browser build has its own touch overlay (`wasm_touch`).
//!
//! v1.8.9 "Backlog" (creator tooling).

use std::cell::Cell;

use rustynes_core::Buttons;

/// Diameter (px) of a face / direction button.
const BTN: f32 = 46.0;
/// Diameter (px) of the smaller Select / Start pills.
const SYS_W: f32 = 56.0;
const SYS_H: f32 = 24.0;
/// Inset of each overlay from the screen edge.
const MARGIN: f32 = 22.0;

/// On-screen overlay-controller state.
///
/// The mask is rebuilt every frame from which on-screen buttons the pointer is
/// held down on, so releasing the pointer releases the buttons. Hidden ⇒ the
/// mask is empty, so a closed overlay never injects input.
#[derive(Debug, Default)]
pub struct VirtualPad {
    /// Whether the overlay is shown.
    pub visible: bool,
    /// Buttons held this frame (rebuilt by [`Self::show`]).
    mask: Buttons,
}

impl VirtualPad {
    /// The buttons currently held on the overlay (player 1).
    #[must_use]
    pub const fn mask(&self) -> Buttons {
        self.mask
    }

    /// Draw the translucent overlay (when [`Self::visible`]) and recompute the
    /// held mask. Clears the mask and returns early when hidden.
    pub fn show(&mut self, ctx: &egui::Context) {
        if !self.visible {
            self.mask = Buttons::empty();
            return;
        }
        let held = Cell::new(Buttons::empty());
        // D-pad, anchored to the bottom-left corner.
        egui::Area::new(egui::Id::new("virtual_pad_dpad"))
            .anchor(egui::Align2::LEFT_BOTTOM, egui::vec2(MARGIN, -MARGIN))
            .interactable(true)
            .show(ctx, |ui| dpad(ui, &held));
        // Face + system buttons, anchored to the bottom-right corner.
        egui::Area::new(egui::Id::new("virtual_pad_face"))
            .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-MARGIN, -MARGIN))
            .interactable(true)
            .show(ctx, |ui| face_buttons(ui, &held));
        self.mask = held.get();
    }
}

/// Translucent white for the D-pad / outlines (the game shows through).
const WHITE: (u8, u8, u8) = (235, 235, 235);
/// NES-controller red for the A / B face buttons.
const NES_RED: (u8, u8, u8) = (0xE8, 0x18, 0x10);

/// A round translucent button tinted `rgb`. Allocates `diam`×`diam`, ORs `bit`
/// into `held` while the pointer is held down on it, and paints a circle + glyph
/// (more opaque while pressed) so the game shows through at rest.
fn round_button(
    ui: &mut egui::Ui,
    held: &Cell<Buttons>,
    label: &str,
    bit: Buttons,
    diam: f32,
    rgb: (u8, u8, u8),
) {
    let (rect, resp) =
        ui.allocate_exact_size(egui::vec2(diam, diam), egui::Sense::click_and_drag());
    let down = resp.is_pointer_button_down_on();
    if down {
        held.set(held.get() | bit);
    }
    let (r, g, b) = rgb;
    let alpha = if down { 210 } else { 80 };
    let fill = egui::Color32::from_rgba_unmultiplied(r, g, b, alpha);
    // Dark glyph on the light D-pad, light glyph on the red face buttons.
    let fg = if rgb == WHITE {
        egui::Color32::from_black_alpha(220)
    } else {
        egui::Color32::from_white_alpha(235)
    };
    let painter = ui.painter();
    painter.circle_filled(rect.center(), diam * 0.5, fill);
    painter.circle_stroke(
        rect.center(),
        diam * 0.5,
        egui::Stroke::new(1.5, egui::Color32::from_black_alpha(120)),
    );
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        egui::FontId::proportional(diam * 0.42),
        fg,
    );
}

/// A small translucent pill (Select / Start).
fn pill_button(ui: &mut egui::Ui, held: &Cell<Buttons>, label: &str, bit: Buttons) {
    let (rect, resp) =
        ui.allocate_exact_size(egui::vec2(SYS_W, SYS_H), egui::Sense::click_and_drag());
    let down = resp.is_pointer_button_down_on();
    if down {
        held.set(held.get() | bit);
    }
    let fill = if down {
        egui::Color32::from_white_alpha(170)
    } else {
        egui::Color32::from_white_alpha(64)
    };
    let painter = ui.painter();
    // Fully-rounded pill: corner radius = half the height (SYS_H / 2 = 12).
    painter.rect_filled(rect, egui::CornerRadius::same(12), fill);
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        egui::FontId::proportional(SYS_H * 0.55),
        egui::Color32::from_black_alpha(220),
    );
}

/// The four-way D-pad cross.
fn dpad(ui: &mut egui::Ui, held: &Cell<Buttons>) {
    ui.spacing_mut().item_spacing = egui::vec2(2.0, 2.0);
    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            ui.add_space(BTN + 2.0);
            round_button(ui, held, "\u{2191}", Buttons::UP, BTN, WHITE);
        });
        ui.horizontal(|ui| {
            round_button(ui, held, "\u{2190}", Buttons::LEFT, BTN, WHITE);
            ui.add_space(BTN + 2.0);
            round_button(ui, held, "\u{2192}", Buttons::RIGHT, BTN, WHITE);
        });
        ui.horizontal(|ui| {
            ui.add_space(BTN + 2.0);
            round_button(ui, held, "\u{2193}", Buttons::DOWN, BTN, WHITE);
        });
    });
}

/// B / A face buttons + the Select / Start pills beneath them.
fn face_buttons(ui: &mut egui::Ui, held: &Cell<Buttons>) {
    ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
    ui.vertical(|ui| {
        // B sits slightly lower-left of A, like the hardware pad.
        ui.horizontal(|ui| {
            ui.add_space(8.0);
            round_button(ui, held, "B", Buttons::B, BTN, NES_RED);
            round_button(ui, held, "A", Buttons::A, BTN, NES_RED);
        });
        ui.horizontal(|ui| {
            pill_button(ui, held, "SELECT", Buttons::SELECT);
            pill_button(ui, held, "START", Buttons::START);
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hidden_pad_defaults_to_empty_mask() {
        let pad = VirtualPad::default();
        assert!(!pad.visible);
        assert_eq!(pad.mask(), Buttons::empty());
    }

    #[test]
    fn overlay_layouts_accumulate_no_input_without_pointer() {
        // egui's headless test harness: nothing is "pressed" without a pointer,
        // so both layouts accumulate an empty mask — this exercises the layout +
        // Cell plumbing (allocate/paint/interact) without a display.
        let held = Cell::new(Buttons::empty());
        egui::__run_test_ui(|ui| {
            dpad(ui, &held);
            face_buttons(ui, &held);
        });
        assert_eq!(held.get(), Buttons::empty());
    }
}
