//! Shared per-channel audio oscilloscope + VU-meter primitives (v2.1.6
//! "Expansion Audio", Workstream B7).
//!
//! Both the NSF player panel ([`super::nsf_panel`]) and the Audio Mixer panel
//! ([`super::audio_mixer`]) plot the live APU output as rolling per-channel
//! waveform traces and peak level meters. The drawing + ring-buffer logic was
//! originally inlined in `nsf_panel` (v1.5.0 "Lens" Workstream C3 scope, v1.8.9
//! VU meters); it is factored out here so the two panels stay pixel-for-pixel
//! consistent and the meter/scope maths lives in one tested place.
//!
//! ## Determinism note
//!
//! Every value plotted here originates from a *read-only copy* of the APU DAC
//! levels ([`rustynes_core::Nes::apu_snapshot`]) — the base-channel `*_out()`
//! taps and the v2.1.6 [`rustynes_core::ApuDebugView::external`] expansion tap.
//! Nothing in this module writes back into the emulator, so sampling for the
//! scope never perturbs the deterministic per-frame audio. The scope is pure
//! display eye-candy over the sound the core already produced.

/// Number of samples retained per channel scope. One column is appended per
/// egui redraw (~60 Hz), so this is roughly the last ~4 s of level history.
pub const SCOPE_LEN: usize = 256;

/// A small rolling sample history for one channel scope / VU meter.
///
/// Values are stored normalized to `0.0..=1.0` (a channel's DAC level relative
/// to its native ceiling), oldest-to-newest via a wrapping `head`. This is
/// display-only state and never touches the save state or the emulator core.
pub struct ScopeRing {
    buf: [f32; SCOPE_LEN],
    head: usize,
}

impl Default for ScopeRing {
    fn default() -> Self {
        Self {
            buf: [0.0; SCOPE_LEN],
            head: 0,
        }
    }
}

impl ScopeRing {
    /// Append one sample (evicting the oldest), advancing the wrapping head.
    pub fn push(&mut self, v: f32) {
        self.buf[self.head] = v;
        self.head = (self.head + 1) % SCOPE_LEN;
    }

    /// The peak (max) magnitude across the ring — drives the per-channel VU bar.
    #[must_use]
    pub fn peak(&self) -> f32 {
        self.buf.iter().copied().fold(0.0_f32, f32::max)
    }

    /// Clear the ring back to silence (used when a new ROM / NSF is loaded so a
    /// stale trace doesn't bleed across the transition).
    pub fn clear(&mut self) {
        self.buf = [0.0; SCOPE_LEN];
        self.head = 0;
    }
}

/// Draw one channel's rolling waveform into a fixed-height strip.
///
/// Samples are expected in `0.0..=1.0` and plotted on the inverted Y axis
/// (louder = taller). The trace is drawn oldest-first (left) to newest (right).
#[allow(clippy::cast_precision_loss)]
pub fn scope(ui: &mut egui::Ui, label: &str, ring: &ScopeRing, color: egui::Color32) {
    ui.label(label);
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 36.0), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 2.0, egui::Color32::from_black_alpha(180));
    let width = rect.width();
    let height = rect.height();
    let mut points = Vec::with_capacity(SCOPE_LEN);
    for i in 0..SCOPE_LEN {
        // chronological order (oldest first): start at head.
        let s = ring.buf[(ring.head + i) % SCOPE_LEN];
        let x = rect.min.x + (i as f32 / SCOPE_LEN as f32) * width;
        // Sample is 0..=1; plot on the inverted Y axis.
        let y = rect.max.y - s.clamp(0.0, 1.0) * height;
        points.push(egui::pos2(x, y));
    }
    painter.add(egui::Shape::line(points, egui::Stroke::new(1.0, color)));
}

/// Draw one channel's peak level as a horizontal VU bar (label + filled bar).
///
/// `peak` is expected in `0.0..=1.0`; the fill width is proportional to it.
pub fn vu_meter(ui: &mut egui::Ui, label: &str, peak: f32, color: egui::Color32) {
    ui.horizontal(|ui| {
        ui.monospace(label);
        let (rect, _) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 12.0), egui::Sense::hover());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 2.0, egui::Color32::from_black_alpha(180));
        let filled = rect.width() * peak.clamp(0.0, 1.0);
        if filled > 0.5 {
            painter.rect_filled(
                egui::Rect::from_min_size(rect.min, egui::vec2(filled, rect.height())),
                2.0,
                color,
            );
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_ring_wraps_and_keeps_latest() {
        let mut r = ScopeRing::default();
        for i in 0..(SCOPE_LEN + 5) {
            r.push(i as f32);
        }
        // After SCOPE_LEN+5 pushes the head wrapped; the newest sample is the
        // one just before the head.
        let newest = r.buf[(r.head + SCOPE_LEN - 1) % SCOPE_LEN];
        assert!((newest - (SCOPE_LEN as f32 + 4.0)).abs() < f32::EPSILON);
    }

    #[test]
    fn peak_reports_ring_maximum_and_clear_resets() {
        let mut r = ScopeRing::default();
        r.push(0.2);
        r.push(0.9);
        r.push(0.4);
        assert!((r.peak() - 0.9).abs() < f32::EPSILON);
        r.clear();
        assert!(r.peak().abs() < f32::EPSILON);
    }
}
