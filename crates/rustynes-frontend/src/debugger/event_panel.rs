//! Graphical PPU Event Viewer (v1.5.0 "Lens" Workstream A2).
//!
//! Originally (v1.1.0 beta.2, T-110-C3) a write-only scanline x dot scatter.
//! v1.5.0 extends it into the `GeraNES`-class graphical event viewer: a full
//! 341 x 312 (`dot` x `scanline`) per-dot **read/write heatmap** — blue dots for
//! PPU-register reads, red dots for PPU/APU/mapper writes — with hover/click
//! cycle metadata and a synchronized register-access table.
//!
//! Built on the core's `debug-hooks` event log (`Nes::events`), which is
//! output-only and reset per frame, so determinism / `AccuracyCoin` are
//! unaffected and the feature-off core build is byte-identical. The panel is a
//! pure consumer; it never mutates emulator-visible state.

use egui::{Align, Color32, Pos2, Rect, Sense, Stroke, Vec2};
use rustynes_core::{EventKind, Nes};

/// Full NES frame extents for the grid: dots `0..=340` (341 columns) and
/// scanlines `-1..=310` mapped to rows `0..=311` (312 rows — pre-render through
/// the full PAL-tall frame envelope, matching `GeraNES`'s `341 x 312`). NTSC only
/// fills rows up to ~261; the extra height keeps the aspect honest.
const DOTS: f32 = 341.0;
const LINES: f32 = 312.0;

/// Read = blue, write = red (`GeraNES` convention).
const READ_COLOR: Color32 = Color32::from_rgb(0x40, 0xA0, 0xFF);
const WRITE_COLOR: Color32 = Color32::from_rgb(0xF0, 0x50, 0x50);

/// Per-write hue by destination bus (only used to tint the legend chips + the
/// table type column; the heatmap itself is the binary read/write scheme).
const fn write_tint(kind: EventKind) -> Color32 {
    match kind {
        EventKind::PpuWrite => Color32::from_rgb(0xF0, 0x60, 0x60), // PPU write — red
        EventKind::ApuWrite => Color32::from_rgb(0xF0, 0xC0, 0x40), // APU write — amber
        EventKind::MapperWrite => Color32::from_rgb(0xE0, 0x70, 0xE0), // mapper write — magenta
        EventKind::PpuRead => READ_COLOR,
    }
}

/// One captured event flattened out of the borrow of `nes` so the painter
/// closure doesn't hold it.
#[derive(Clone, Copy)]
struct Ev {
    kind: EventKind,
    scanline: i16,
    dot: u16,
    addr: u16,
    value: u8,
}

/// Event-viewer panel state: the click-selected event index (within the current
/// frame's capture).
#[derive(Default)]
pub struct EventPanelState {
    /// Index into the current frame's event list of the selected event, if any.
    selected: Option<usize>,
    /// `selected` as of the previous repaint — so `scroll_to_me` fires only when
    /// the selection actually changes (otherwise it locks the scroll viewport).
    previous_selected: Option<usize>,
    /// The frame the current selection belongs to; when the frame advances the
    /// stale selection is dropped (it would point at an unrelated new event).
    last_frame: Option<u64>,
}

/// Map a `$2000-$2007` (mirrored) PPU register address to its mnemonic.
fn ppu_reg_name(addr: u16) -> &'static str {
    match addr & 0x2007 {
        0x2000 => "PPUCTRL",
        0x2001 => "PPUMASK",
        0x2002 => "PPUSTATUS",
        0x2003 => "OAMADDR",
        0x2004 => "OAMDATA",
        0x2005 => "PPUSCROLL",
        0x2006 => "PPUADDR",
        0x2007 => "PPUDATA",
        _ => "PPU",
    }
}

/// A human label for an event's accessed register / window.
fn reg_label(ev: Ev) -> String {
    match ev.addr {
        0x2000..=0x3FFF => ppu_reg_name(ev.addr).to_string(),
        0x4014 => "OAMDMA".to_string(),
        0x4016 => "JOY1".to_string(),
        0x4017 => "JOY2/FRM".to_string(),
        0x4000..=0x4013 | 0x4015 => "APU".to_string(),
        _ => "MAPPER".to_string(),
    }
}

/// The direction word for the table / tooltip.
const fn dir_word(kind: EventKind) -> &'static str {
    if kind.is_read() { "Read" } else { "Write" }
}

/// Render the graphical PPU Event Viewer.
#[allow(clippy::many_single_char_names)] // local geometric coords (w/h/x/y/p).
pub fn show(ctx: &egui::Context, open: &mut bool, state: &mut EventPanelState, nes: &mut Nes) {
    egui::Window::new("Event Viewer")
        .open(open)
        .default_size([700.0, 640.0])
        .resizable(true)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                let mut on = nes.event_logging();
                if ui.checkbox(&mut on, "Record").changed() {
                    nes.set_event_logging(on);
                }
                ui.separator();
                ui.weak("Reads are blue, writes are red. Full PPU frame: 341x312.");
            });
            ui.horizontal(|ui| {
                ui.colored_label(READ_COLOR, "PPU read");
                ui.colored_label(write_tint(EventKind::PpuWrite), "PPU write");
                ui.colored_label(write_tint(EventKind::ApuWrite), "APU write");
                ui.colored_label(write_tint(EventKind::MapperWrite), "mapper write");
            });

            // Flatten the borrow out of `nes` up front.
            let frame = nes.ppu_snapshot().frame;
            let events: Vec<Ev> = nes
                .events()
                .iter()
                .map(|e| Ev {
                    kind: e.kind,
                    scanline: e.scanline,
                    dot: e.dot,
                    addr: e.addr,
                    value: e.value,
                })
                .collect();

            ui.horizontal(|ui| {
                ui.label(format!("Events: {}", events.len()));
                ui.separator();
                ui.label(format!("Frame {frame}"));
            });
            ui.separator();

            if state.last_frame != Some(frame) {
                // The frame advanced: the previous selection indexed a different
                // frame's events, so drop it rather than highlight an unrelated one.
                state.selected = None;
                state.last_frame = Some(frame);
            }

            if events.is_empty() || state.selected.is_some_and(|i| i >= events.len()) {
                // No capture, or the capture changed under us (frame advanced) —
                // drop the stale selection rather than index out of bounds.
                state.selected = None;
            }

            draw_heatmap(ui, state, &events);
            ui.separator();
            event_table(ui, state, &events);

            if !nes.event_logging() {
                ui.weak("(enable Record, then run/step a frame)");
            }
        });
}

/// Draw the read/write heatmap with hover tooltip + click-to-select.
#[allow(clippy::many_single_char_names)]
fn draw_heatmap(ui: &mut egui::Ui, state: &mut EventPanelState, events: &[Ev]) {
    // Keep the 341:312 aspect inside the available width, capped so the table
    // below stays visible.
    let avail = ui.available_size();
    let w = avail.x.max(64.0);
    let h = (w * LINES / DOTS).min(320.0);
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, h), Sense::click());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 2.0, Color32::from_rgb(0x0C, 0x0C, 0x10));
    p.rect_stroke(
        rect,
        2.0,
        Stroke::new(1.0, Color32::from_gray(0x50)),
        egui::StrokeKind::Inside,
    );
    // Faint line at the first visible scanline (row 0 = pre-render, row 1 =
    // scanline 0).
    let vis_y = rect.top() + (1.0 / LINES) * rect.height();
    p.line_segment(
        [
            Pos2::new(rect.left(), vis_y),
            Pos2::new(rect.right(), vis_y),
        ],
        Stroke::new(1.0, Color32::from_gray(0x28)),
    );
    // Faint line at the end of the NTSC visible region (scanline 240 -> row 241).
    let vbl_y = rect.top() + (241.0 / LINES) * rect.height();
    p.line_segment(
        [
            Pos2::new(rect.left(), vbl_y),
            Pos2::new(rect.right(), vbl_y),
        ],
        Stroke::new(1.0, Color32::from_gray(0x28)),
    );

    let cell_to_pos = |dot: u16, scanline: i16| -> Pos2 {
        let row = (f32::from(scanline) + 1.0).clamp(0.0, LINES - 1.0);
        let x = rect.left() + (f32::from(dot) / DOTS) * rect.width();
        let y = rect.top() + (row / LINES) * rect.height();
        Pos2::new(x, y)
    };

    // Plot every event as a 2x2 dot.
    for ev in events {
        let pos = cell_to_pos(ev.dot, ev.scanline);
        let color = if ev.kind.is_read() {
            READ_COLOR
        } else {
            WRITE_COLOR
        };
        p.rect_filled(Rect::from_min_size(pos, Vec2::new(2.0, 2.0)), 0.0, color);
    }

    // Hover / click: find the nearest event within a small pixel radius.
    let hover = resp
        .hover_pos()
        .and_then(|mp| nearest_event(events, mp, &cell_to_pos));
    if resp.clicked() {
        state.selected = hover;
    }

    // A ring around the selected event (drawn last so it sits on top).
    if let Some(i) = state.selected
        && let Some(ev) = events.get(i)
    {
        let pos = cell_to_pos(ev.dot, ev.scanline);
        p.circle_stroke(
            pos + Vec2::new(1.0, 1.0),
            5.0,
            Stroke::new(1.5, Color32::from_rgb(0xF0, 0xD0, 0x40)),
        );
    }

    // Hover tooltip with the nearest event's metadata.
    if let Some(i) = hover
        && let Some(&ev) = events.get(i)
    {
        let pos = cell_to_pos(ev.dot, ev.scanline);
        p.circle_stroke(
            pos + Vec2::new(1.0, 1.0),
            4.0,
            Stroke::new(1.0, Color32::WHITE),
        );
        resp.on_hover_ui(|ui| {
            ui.monospace(format!("#{i}  {}", dir_word(ev.kind)));
            ui.monospace(format!("{} (${:04X})", reg_label(ev), ev.addr));
            ui.monospace(format!("Value: ${:02X}", ev.value));
            ui.monospace(format!("Scanline: {}", ev.scanline));
            ui.monospace(format!("Dot: {}", ev.dot));
        });
    }
}

/// The index of the event nearest `mp` within ~6px, or `None`.
fn nearest_event(
    events: &[Ev],
    mp: Pos2,
    cell_to_pos: &impl Fn(u16, i16) -> Pos2,
) -> Option<usize> {
    let mut best: Option<(usize, f32)> = None;
    for (i, ev) in events.iter().enumerate() {
        let pos = cell_to_pos(ev.dot, ev.scanline) + Vec2::new(1.0, 1.0);
        let d = pos.distance_sq(mp);
        if d <= 36.0 && best.is_none_or(|(_, bd)| d < bd) {
            best = Some((i, d));
        }
    }
    best.map(|(i, _)| i)
}

/// The scrollable register-access table (synchronized with the heatmap
/// selection).
fn event_table(ui: &mut egui::Ui, state: &mut EventPanelState, events: &[Ev]) {
    match state.selected.and_then(|i| events.get(i).map(|e| (i, *e))) {
        Some((i, ev)) => {
            ui.monospace(format!(
                "Selected: #{i} {} {} (${:04X}) value ${:02X} at scanline {} dot {}",
                dir_word(ev.kind),
                reg_label(ev),
                ev.addr,
                ev.value,
                ev.scanline,
                ev.dot,
            ));
        }
        None => {
            ui.weak("Selected: none (click a dot in the heatmap)");
        }
    }
    if !events.is_empty() && ui.button("Clear selection").clicked() {
        state.selected = None;
    }

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            egui::Grid::new("event_table")
                .striped(true)
                .num_columns(6)
                .show(ui, |ui| {
                    ui.strong("#");
                    ui.strong("Type");
                    ui.strong("Reg");
                    ui.strong("Val");
                    ui.strong("SL");
                    ui.strong("Dot");
                    ui.end_row();
                    for (i, ev) in events.iter().enumerate() {
                        let selected = state.selected == Some(i);
                        let resp = ui.selectable_label(selected, format!("{i}"));
                        if resp.clicked() {
                            state.selected = Some(i);
                        }
                        // Only auto-scroll when the selection actually changed —
                        // calling this every frame would lock the scroll viewport.
                        if selected && state.selected != state.previous_selected {
                            resp.scroll_to_me(Some(Align::Center));
                        }
                        ui.colored_label(write_tint(ev.kind), dir_word(ev.kind));
                        ui.monospace(format!("{} ${:04X}", reg_label(*ev), ev.addr));
                        ui.monospace(format!("${:02X}", ev.value));
                        ui.monospace(format!("{}", ev.scanline));
                        ui.monospace(format!("{}", ev.dot));
                        ui.end_row();
                    }
                });
        });
    // Record the selection so the next repaint can detect a change (scroll-on-change).
    state.previous_selected = state.selected;
}
