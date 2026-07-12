//! `TAStudio` piano-roll editor panel (v1.6.0 "Studio" Workstream A2).
//!
//! The graphical face of the [`TasEditor`](crate::tastudio::TasEditor) model
//! (A1/A3/A4): a vertically-scrolling **piano-roll** where each row is one
//! frame and each column is one controller button, modelled on `BizHawk`'s
//! `TAStudio` and FCEUX's TAS Editor.
//!
//! Like every other tool window (`replay_panel` / `netplay_panel` / …) this is
//! a **control + read-out surface only**. It never touches the emulator inside
//! the egui closure — the [`TasEditor`] needs `&mut Nes` for seek / branch /
//! record, which the app holds under the emu lock. Every edit is recorded as a
//! [`TasRequest`] that the app drains after the egui pass and applies under the
//! lock (the same `take_*` pattern the other panels use). Because the editor's
//! seek re-derives state by replaying inputs, the result stays bit-identical —
//! no new determinism surface (see `CLAUDE.md`).
//!
//! The grid renders only the visible rows via [`egui::ScrollArea::show_rows`],
//! so a 100k-frame movie costs the same per frame as a 100-frame one.

use crate::tastudio::TasEditor;
use rustynes_core::{Buttons, FrameInput};

/// The eight standard-controller buttons in wire / display order
/// (`A, B, Select, Start, Up, Down, Left, Right`) with a one-glyph column
/// label. Matches [`Buttons`]'s bit order (LSB first).
const BUTTONS: [(Buttons, &str); 8] = [
    (Buttons::A, "A"),
    (Buttons::B, "B"),
    (Buttons::SELECT, "s"),
    (Buttons::START, "S"),
    (Buttons::UP, "U"),
    (Buttons::DOWN, "D"),
    (Buttons::LEFT, "L"),
    (Buttons::RIGHT, "R"),
];

/// A user action requested from the `TAStudio` window, drained by the app after
/// the egui pass and applied to the `TasEditor` + `Nes` under the emu lock.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TasRequest {
    /// Move the edit/playback cursor to this frame (deterministic re-derive).
    Seek(usize),
    /// Replace the input at this frame with the given value (button edit).
    SetInput {
        /// Target frame index.
        frame: usize,
        /// The new full per-frame input.
        input: FrameInput,
    },
    /// Set or rename a marker label at this frame.
    SetMarker {
        /// Target frame index.
        frame: usize,
        /// The marker label.
        label: String,
    },
    /// Remove the marker at this frame.
    RemoveMarker(usize),
    /// Insert a blank frame at the cursor (shifts later frames down).
    InsertFrame(usize),
    /// Delete the frame at the cursor (shifts later frames up).
    DeleteFrame(usize),
    /// Fork the current timeline into a new branch.
    CreateBranch,
    /// Restore branch `idx`.
    LoadBranch(usize),
    /// Delete branch `idx`.
    DeleteBranch(usize),
    /// Write the project to a `.rnmproj` file (app opens the save dialog).
    SaveProject,
    /// Load a project from a `.rnmproj` file (app opens the open dialog).
    LoadProject,
    /// v1.8.9 — stamp an input-macro pattern into the log starting at `start`
    /// (one `set_input` per frame).
    StampMacro {
        /// First frame to write.
        start: usize,
        /// The per-frame pattern.
        frames: Vec<FrameInput>,
    },
    /// v2.1.10 "Creator Tools" (B8) — enable / move (`Some((start, end))`) or
    /// disable (`None`) the force-greenzone range: guarantee a cached save-state
    /// at every frame in the range so scrubbing there is instant.
    SetForcedGreenzone(Option<(usize, usize)>),
}

/// An in-progress left-drag that paints one button column to a single value,
/// so dragging down a column sets/clears a run of frames in one gesture
/// (`BizHawk`'s "draw input" behaviour). The painted value is fixed at
/// drag-start (`!current` of the first cell) so the drag is idempotent.
#[derive(Clone, Copy)]
struct PaintDrag {
    /// Controller port being painted (0 = P1, 1 = P2).
    port: u8,
    /// The button column being painted.
    button: Buttons,
    /// `true` = press the button on every dragged frame, `false` = release.
    set: bool,
    /// Frames already painted this drag (so we don't re-emit per egui frame).
    last_frame: usize,
}

/// Persistent TAStudio-window UI state (pending requests + view toggles).
pub struct TasStudioPanelState {
    /// Pending actions, drained by the app each frame.
    requests: Vec<TasRequest>,
    /// Active column drag-paint, if any.
    paint: Option<PaintDrag>,
    /// Show the player-2 button columns.
    show_p2: bool,
    /// Keep the cursor row scrolled into view as playback advances.
    follow_cursor: bool,
    /// One-shot: scroll this frame's row into view on the next render.
    scroll_to: Option<usize>,
    /// Last cursor frame observed, so `follow_cursor` can detect advancement
    /// (playback / seek) and auto-scroll without fighting a manual scroll.
    last_cursor: Option<usize>,
    /// v1.8.9 — the input-macro / pattern bank (session-local; record from the
    /// cursor, stamp at the cursor).
    macros: crate::input_macros::MacroBank,
    /// v1.8.9 — number of frames to capture when recording a new macro.
    macro_len: usize,
}

impl Default for TasStudioPanelState {
    fn default() -> Self {
        Self {
            requests: Vec::new(),
            paint: None,
            show_p2: false,
            follow_cursor: true,
            scroll_to: None,
            last_cursor: None,
            macros: crate::input_macros::MacroBank::default(),
            macro_len: 8,
        }
    }
}

impl TasStudioPanelState {
    /// Drain the pending requests (the app applies them under the emu lock).
    pub fn take_requests(&mut self) -> Vec<TasRequest> {
        core::mem::take(&mut self.requests)
    }

    fn emit(&mut self, req: TasRequest) {
        self.requests.push(req);
    }
}

/// Toggle one `button` of `port` in `input`, returning the edited frame.
/// Pure (no egui / no emulator) so the piano-roll's core edit is unit-tested.
#[must_use]
fn toggle_button(mut input: FrameInput, port: u8, button: Buttons) -> FrameInput {
    let pad = if port == 0 {
        &mut input.p1
    } else {
        &mut input.p2
    };
    pad.toggle(button);
    input
}

/// Set or clear one `button` of `port` in `input` (drag-paint), returning the
/// edited frame. Pure, so it is unit-tested alongside [`toggle_button`].
#[must_use]
fn paint_button(mut input: FrameInput, port: u8, button: Buttons, set: bool) -> FrameInput {
    let pad = if port == 0 {
        &mut input.p1
    } else {
        &mut input.p2
    };
    pad.set(button, set);
    input
}

/// Visual classification of a piano-roll row, driving its background tint.
/// Pure function of the editor state so the colour policy is testable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RowKind {
    /// The current edit/playback cursor frame.
    Cursor,
    /// A lag frame (the program polled no controller this frame).
    Lag,
    /// A frame bearing a named marker.
    Marker,
    /// An ordinary frame inside the emulated greenzone.
    Played,
    /// A frame past everything emulated so far (blank, appendable).
    Future,
}

/// Classify `frame` for row tinting. Cursor wins over marker wins over lag.
#[must_use]
fn row_kind(editor: &TasEditor, frame: usize) -> RowKind {
    if frame == editor.cursor() {
        RowKind::Cursor
    } else if editor.marker_at(frame).is_some() {
        RowKind::Marker
    } else if editor.lag_at(frame) == Some(true) {
        RowKind::Lag
    } else if frame < editor.len() {
        RowKind::Played
    } else {
        RowKind::Future
    }
}

fn row_tint(kind: RowKind) -> Option<egui::Color32> {
    match kind {
        RowKind::Cursor => Some(egui::Color32::from_rgb(0x35, 0x55, 0x88)),
        RowKind::Marker => Some(egui::Color32::from_rgb(0x5a, 0x4a, 0x20)),
        RowKind::Lag => Some(egui::Color32::from_rgb(0x4a, 0x22, 0x22)),
        RowKind::Played | RowKind::Future => None,
    }
}

/// How many empty appendable rows to show past the input log so the user can
/// extend the movie by painting into the future.
const FUTURE_PAD: usize = 8;

#[allow(clippy::too_many_lines)]
pub fn show(
    ctx: &egui::Context,
    open: &mut bool,
    state: &mut TasStudioPanelState,
    editor: Option<&TasEditor>,
) {
    egui::Window::new("TAStudio")
        .open(open)
        .default_size([460.0, 520.0])
        .resizable(true)
        .show(ctx, |ui| {
            let Some(editor) = editor else {
                ui.weak("No TAStudio session. Load a ROM and open TAStudio from Tools.");
                return;
            };

            // Follow-cursor: when enabled, auto-scroll to the cursor row only
            // when it actually advances (playback / seek), so a manual scroll
            // isn't yanked back every frame while the cursor sits still.
            if state.follow_cursor && state.last_cursor != Some(editor.cursor()) {
                state.scroll_to = Some(editor.cursor());
            }
            state.last_cursor = Some(editor.cursor());

            header(ui, state, editor);
            ui.separator();
            branches(ui, state, editor);
            ui.separator();
            macros(ui, state, editor);
            ui.separator();
            grid(ui, state, editor);
        });
}

/// v1.8.9 — the input-macro / pattern bank: record a pattern from the cursor and
/// stamp a saved pattern at the cursor (the FCEUX / `BizHawk` pattern-paint).
fn macros(ui: &mut egui::Ui, state: &mut TasStudioPanelState, editor: &TasEditor) {
    let cursor = editor.cursor();
    ui.collapsing("Macros", |ui| {
        ui.horizontal(|ui| {
            ui.label("Record");
            ui.add(
                egui::DragValue::new(&mut state.macro_len)
                    .range(1..=600)
                    .suffix(" fr"),
            );
            if ui
                .button("\u{23fa} from cursor")
                .on_hover_text("Capture this many frames starting at the cursor as a macro")
                .clicked()
            {
                let frames = editor.extract_macro(cursor, state.macro_len.max(1));
                let name = format!("macro {}", state.macros.macros.len() + 1);
                state
                    .macros
                    .macros
                    .push(crate::input_macros::InputMacro { name, frames });
            }
        });
        // Collect actions during the immutable iter, apply after.
        let mut stamp: Option<Vec<FrameInput>> = None;
        let mut remove: Option<usize> = None;
        for (i, m) in state.macros.macros.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.label(format!("{} ({} fr)", m.name, m.frames.len()));
                if ui
                    .button("Stamp")
                    .on_hover_text("Stamp this pattern at the cursor")
                    .clicked()
                {
                    stamp = Some(m.frames.clone());
                }
                if ui.button("\u{2716}").on_hover_text("Delete").clicked() {
                    remove = Some(i);
                }
            });
        }
        if let Some(frames) = stamp {
            state.emit(TasRequest::StampMacro {
                start: cursor,
                frames,
            });
        }
        if let Some(i) = remove {
            state.macros.macros.remove(i);
        }
    });
}

fn header(ui: &mut egui::Ui, state: &mut TasStudioPanelState, editor: &TasEditor) {
    ui.horizontal(|ui| {
        ui.strong("Frame:");
        ui.monospace(format!("{} / {}", editor.cursor(), editor.len()));
        ui.separator();
        ui.strong("Lag:");
        ui.monospace(format!("{}", editor.lag_count()));
    });
    ui.horizontal(|ui| {
        if ui
            .button("⑂ Branch")
            .on_hover_text("Fork the current timeline into a new branch")
            .clicked()
        {
            state.emit(TasRequest::CreateBranch);
        }
        if ui
            .button("＋ Frame")
            .on_hover_text("Insert a blank frame at the cursor")
            .clicked()
        {
            state.emit(TasRequest::InsertFrame(editor.cursor()));
        }
        if ui
            .button("－ Frame")
            .on_hover_text("Delete the frame at the cursor")
            .clicked()
        {
            state.emit(TasRequest::DeleteFrame(editor.cursor()));
        }
        // `.rnmproj` file I/O uses the native file dialog; the browser build
        // has no equivalent yet, so these buttons are native-only (showing
        // them on wasm would be misleading no-ops).
        #[cfg(not(target_arch = "wasm32"))]
        {
            ui.separator();
            if ui
                .button("💾 Save")
                .on_hover_text("Save project (.rnmproj)")
                .clicked()
            {
                state.emit(TasRequest::SaveProject);
            }
            if ui
                .button("📂 Load")
                .on_hover_text("Load project (.rnmproj)")
                .clicked()
            {
                state.emit(TasRequest::LoadProject);
            }
        }
    });
    ui.horizontal(|ui| {
        ui.checkbox(&mut state.show_p2, "Show P2");
        ui.checkbox(&mut state.follow_cursor, "Follow cursor");
        if ui
            .button("⏷ Cursor")
            .on_hover_text("Scroll the cursor row into view")
            .clicked()
        {
            state.scroll_to = Some(editor.cursor());
        }
        // v2.1.10 "Creator Tools" (B8) — force-greenzone toggle. When enabled it
        // guarantees a cached state at every frame in the recent window ending at
        // the cursor (capped span), so scrubbing / rewinding there is instant. A
        // pure caching optimisation — it never alters the deterministic timeline.
        ui.separator();
        let mut forced = editor.forced_greenzone_range().is_some();
        if ui
            .checkbox(&mut forced, "Force GZ")
            .on_hover_text(
                "Force-cache a save-state at every frame in the recent window \
                 (up to ~3 min) for instant scrubbing",
            )
            .changed()
        {
            if forced {
                let cursor = editor.cursor();
                let start = cursor.saturating_sub(crate::tastudio::MAX_FORCED_GREENZONE_FRAMES - 1);
                state.emit(TasRequest::SetForcedGreenzone(Some((start, cursor))));
            } else {
                state.emit(TasRequest::SetForcedGreenzone(None));
            }
        }
    });
}

fn branches(ui: &mut egui::Ui, state: &mut TasStudioPanelState, editor: &TasEditor) {
    let count = editor.branch_count();
    if count == 0 {
        ui.weak("No branches. Use ⑂ Branch to fork a timeline.");
        return;
    }
    ui.strong(format!("Branches ({count})"));
    egui::ScrollArea::vertical()
        .id_salt("tas_branches")
        .max_height(80.0)
        .show(ui, |ui| {
            for idx in 0..count {
                let frame = editor.branch(idx).map_or(0, |b| b.frame);
                ui.horizontal(|ui| {
                    ui.monospace(format!("#{idx} @ frame {frame}"));
                    if ui.small_button("Load").clicked() {
                        state.emit(TasRequest::LoadBranch(idx));
                    }
                    if ui
                        .small_button("🗑")
                        .on_hover_text("Delete branch")
                        .clicked()
                    {
                        state.emit(TasRequest::DeleteBranch(idx));
                    }
                });
            }
        });
}

#[allow(clippy::too_many_lines)]
fn grid(ui: &mut egui::Ui, state: &mut TasStudioPanelState, editor: &TasEditor) {
    let ports: &[u8] = if state.show_p2 { &[0, 1] } else { &[0] };
    let row_h = ui.spacing().interact_size.y.max(18.0);
    let total = editor.len() + FUTURE_PAD;
    let pointer_released = ui.input(|i| i.pointer.any_released());

    let mut scroll_area = egui::ScrollArea::vertical()
        .id_salt("tas_grid")
        .auto_shrink([false, false]);
    // The grid is virtualized (`show_rows`), so a target row that is currently
    // off-screen is never instantiated — `Response::scroll_to_me` would never
    // fire for it. Drive the scroll on the builder by row offset instead
    // (row index * row height), which works for any frame, on- or off-screen.
    if let Some(target) = state.scroll_to.take() {
        scroll_area = scroll_area.vertical_scroll_offset(target as f32 * row_h);
    }
    scroll_area.show_rows(ui, row_h, total, |ui, range| {
        for frame in range {
            let kind = row_kind(editor, frame);
            ui.horizontal(|ui| {
                if let Some(tint) = row_tint(kind) {
                    let r = ui.max_rect();
                    ui.painter().rect_filled(r, 0.0, tint);
                }
                // Frame number — click to seek the cursor there.
                if ui
                    .add(
                        egui::Label::new(egui::RichText::new(format!("{frame:>6}")).monospace())
                            .sense(egui::Sense::click()),
                    )
                    .on_hover_text("Click to seek")
                    .clicked()
                {
                    state.emit(TasRequest::Seek(frame));
                }
                // Marker glyph (read-only here; editing is via the menu).
                if let Some(label) = editor.marker_at(frame) {
                    ui.colored_label(egui::Color32::from_rgb(0xE0, 0xC0, 0x40), "●")
                        .on_hover_text(label.to_owned());
                } else {
                    ui.label(" ");
                }
                let input = editor.input_at(frame).unwrap_or_default();
                for &port in ports {
                    ui.separator();
                    for (button, glyph) in BUTTONS {
                        button_cell(ui, state, frame, port, button, glyph, input);
                    }
                }
            });
        }
    });

    if pointer_released {
        state.paint = None;
    }
}

#[allow(clippy::too_many_arguments)]
fn button_cell(
    ui: &mut egui::Ui,
    state: &mut TasStudioPanelState,
    frame: usize,
    port: u8,
    button: Buttons,
    glyph: &str,
    input: FrameInput,
) {
    let pad = if port == 0 { input.p1 } else { input.p2 };
    let pressed = pad.contains(button);
    let fill = if pressed {
        egui::Color32::from_rgb(0x40, 0xA0, 0x40)
    } else {
        egui::Color32::from_gray(0x28)
    };
    let resp = ui.add(
        egui::Button::new(if pressed { glyph } else { " " })
            .fill(fill)
            .min_size(egui::vec2(14.0, 14.0))
            .sense(egui::Sense::click_and_drag()),
    );

    if resp.clicked() {
        state.emit(TasRequest::SetInput {
            frame,
            input: toggle_button(input, port, button),
        });
    } else if resp.drag_started() {
        // Begin a column paint; the painted value is the toggle of this cell.
        let set = !pressed;
        state.paint = Some(PaintDrag {
            port,
            button,
            set,
            last_frame: frame,
        });
        state.emit(TasRequest::SetInput {
            frame,
            input: paint_button(input, port, button, set),
        });
    } else if let Some(p) = state.paint {
        // Extend an active paint to any cell of the same column we drag over.
        if p.port == port && p.button == button && p.last_frame != frame && resp.hovered() {
            state.paint = Some(PaintDrag {
                last_frame: frame,
                ..p
            });
            state.emit(TasRequest::SetInput {
                frame,
                input: paint_button(input, port, button, p.set),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustynes_core::Nes;

    fn pad(b: Buttons) -> FrameInput {
        FrameInput::new(b, Buttons::empty())
    }

    /// Minimal valid NROM image (16 KiB PRG, 8 KiB CHR) so we can build a real
    /// `Nes` for the editor — mirrors the model's own test fixture.
    fn synth_nrom() -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"NES\x1A");
        bytes.extend_from_slice(&[1, 1, 0, 0]);
        bytes.extend_from_slice(&[0u8; 8]);
        let mut prg = vec![0u8; 16 * 1024];
        prg[0] = 0x4C; // JMP $C000
        prg[2] = 0xC0;
        let len = prg.len();
        // reset/nmi/irq vectors all point at $C000
        prg[len - 6] = 0x00;
        prg[len - 5] = 0xC0;
        prg[len - 4] = 0x00;
        prg[len - 3] = 0xC0;
        prg[len - 2] = 0x00;
        prg[len - 1] = 0xC0;
        bytes.extend_from_slice(&prg);
        bytes.extend_from_slice(&[0u8; 8 * 1024]);
        bytes
    }

    #[test]
    fn toggle_flips_one_button_on_one_port() {
        let f = FrameInput::default();
        let f = toggle_button(f, 0, Buttons::A);
        assert!(f.p1.contains(Buttons::A));
        assert!(!f.p2.contains(Buttons::A));
        let f = toggle_button(f, 0, Buttons::A);
        assert!(!f.p1.contains(Buttons::A), "second toggle clears it");
        let f = toggle_button(f, 1, Buttons::START);
        assert!(f.p2.contains(Buttons::START));
        assert!(!f.p1.contains(Buttons::START));
    }

    #[test]
    fn paint_sets_and_clears_idempotently() {
        let f = FrameInput::default();
        let on = paint_button(f, 0, Buttons::RIGHT, true);
        assert!(on.p1.contains(Buttons::RIGHT));
        // Painting the same value again is a no-op (idempotent drag).
        let still_on = paint_button(on, 0, Buttons::RIGHT, true);
        assert_eq!(on, still_on);
        let off = paint_button(on, 0, Buttons::RIGHT, false);
        assert!(!off.p1.contains(Buttons::RIGHT));
    }

    #[test]
    fn button_table_matches_wire_order() {
        // The column order must equal the controller's LSB-first shift order.
        let bits: Vec<u8> = BUTTONS.iter().map(|(b, _)| b.bits()).collect();
        assert_eq!(bits, vec![1, 2, 4, 8, 16, 32, 64, 128]);
    }

    #[test]
    fn row_kind_priority_cursor_over_marker_over_lag() {
        let nes = Nes::from_rom(&synth_nrom()).unwrap();
        let mut ed = TasEditor::new(&nes, 1 << 20, 8);
        // Append a couple of frames so cursor/len differ.
        ed.set_input(0, pad(Buttons::A));
        ed.set_input(1, pad(Buttons::B));
        ed.set_marker(1, "mark");
        // cursor is 0 by construction.
        assert_eq!(row_kind(&ed, 0), RowKind::Cursor);
        assert_eq!(row_kind(&ed, 1), RowKind::Marker);
        // A frame past the log is Future.
        assert_eq!(row_kind(&ed, 999), RowKind::Future);
    }

    #[test]
    fn requests_drain_in_order_then_empty() {
        let mut s = TasStudioPanelState::default();
        assert!(s.take_requests().is_empty());
        s.emit(TasRequest::Seek(5));
        s.emit(TasRequest::DeleteBranch(2));
        let drained = s.take_requests();
        assert_eq!(drained.len(), 2);
        assert_eq!(drained[0], TasRequest::Seek(5));
        assert!(s.take_requests().is_empty(), "drained once, now empty");
    }
}
