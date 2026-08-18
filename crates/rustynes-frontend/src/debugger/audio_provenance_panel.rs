// SPDX-License-Identifier: GPL-3.0-or-later
//! Audio provenance inspector (v2.3.7 "Overtone").
//!
//! Pick a moment in the frame and read why it sounds like that: what each
//! channel was putting out, which one dominated the mix, and — for every APU
//! register — the value it holds, the CPU cycle it was written on, and **the
//! instruction that wrote it**.
//!
//! # Relationship to the other audio panels
//!
//! Deliberately not a replacement for either. The Audio Scope plots *what the
//! waveform looks like*; the Audio Mixer sets *how loud each channel is*. This
//! answers *why* — which neither can, because neither has the link from a
//! sample back to the instruction responsible for it.
//!
//! # What it does not claim
//!
//! The trace is per **CPU cycle**, which is the cadence the mix is actually
//! computed at. Output samples come out of a band-limited decimator at ~40.6
//! CPU cycles each, and an output sample is a weighted sum of transitions across
//! the filter kernel — not a copy of one instant. So this panel reports the
//! cycle window, and says so, rather than pretending a sample has a single
//! originating cycle.

use crate::debugger::source_map::SourceMap;
use rustynes_core::Nes;

/// The APU register names, indexed from `$4000`.
///
/// `$4014` and `$4016` are inside the traced range and are not APU registers;
/// they are labelled for what they are rather than blanked, so a user who sees
/// a write to one is told why it is there.
const REG_NAMES: [&str; 0x18] = [
    "$4000 Pulse1 ctrl",
    "$4001 Pulse1 sweep",
    "$4002 Pulse1 timer lo",
    "$4003 Pulse1 timer hi",
    "$4004 Pulse2 ctrl",
    "$4005 Pulse2 sweep",
    "$4006 Pulse2 timer lo",
    "$4007 Pulse2 timer hi",
    "$4008 Tri linear",
    "$4009 (unused)",
    "$400A Tri timer lo",
    "$400B Tri timer hi",
    "$400C Noise ctrl",
    "$400D (unused)",
    "$400E Noise period",
    "$400F Noise length",
    "$4010 DMC ctrl",
    "$4011 DMC DAC",
    "$4012 DMC addr",
    "$4013 DMC length",
    "$4014 OAM DMA (not APU)",
    "$4015 Status",
    "$4016 Controller (not APU)",
    "$4017 Frame counter",
];

/// Channel labels in the order [`rustynes_apu::provenance::MixRecord::dominant`]
/// indexes them.
const CHANNELS: [&str; 5] = ["Pulse 1", "Pulse 2", "Triangle", "Noise", "DMC"];

/// Side-band effects a write carries beyond the obvious one.
///
/// **This is the research obligation the plan recorded, made visible.** A naive
/// reading of "last write to `$4003`" says "the period changed" — but that one
/// write ALSO loads the length counter, resets the duty sequencer, and restarts
/// the envelope. A provenance tool that names the right instruction and then
/// describes the wrong effect is precisely the failure this project keeps
/// paying for, so the extra effects are spelled out at the point of use.
///
/// Confirmed against this emulator's own implementation rather than from memory:
/// `Pulse::write_timer_hi`, `Triangle::write_linear`, `Apu::write_status`, and
/// the `$4017` alignment comment in `Apu::write_register`.
const fn side_effects(idx: usize) -> Option<&'static str> {
    match idx {
        // $4003 / $4007
        0x03 | 0x07 => Some(
            "also loads the length counter, resets the duty sequencer, and restarts the envelope",
        ),
        // $4008
        0x08 => {
            Some("length-counter halt is DEFERRED — applied after the same-cycle half-frame clock")
        }
        // $400B
        0x0B => Some("also loads the length counter and sets the linear-counter reload flag"),
        // $400F
        0x0F => Some("also loads the length counter and restarts the envelope"),
        // $4015
        0x15 => Some(
            "enables/disables length counters; the DMC enable is latched and applied with a delay",
        ),
        // $4017
        0x17 => Some("effects land 3 CPU cycles after the write on an APU clock, 4 otherwise"),
        _ => None,
    }
}

/// Panel state: the pinned cycle, and nothing else the core already knows.
///
/// **No mirror of the core's armed flag.** The v2.3.6 pixel panel kept one and
/// edge-detected on it, which desynced permanently the moment a ROM load
/// installed a fresh `Nes`: the checkbox stayed ticked over an unarmed core,
/// with no way back but unticking and re-ticking. The core is re-read every
/// frame here instead.
#[derive(Default)]
pub struct AudioProvenancePanelState {
    /// Offset into the current frame's mix trace, in CPU cycles.
    pin: usize,
    /// Follow the newest recorded cycle instead of holding `pin`.
    follow: bool,
}

/// Render the inspector.
pub fn show(
    ctx: &egui::Context,
    detached: &mut std::collections::HashSet<&'static str>,
    open: &mut bool,
    state: &mut AudioProvenancePanelState,
    nes: &mut Nes,
    source_map: &SourceMap,
) {
    // The CORE is the source of truth for the arm, re-read every frame, for the
    // reason `AudioProvenancePanelState` documents.
    let armed = nes.audio_provenance_armed();
    let mut want_armed = armed;

    super::detachable_window(
        ctx,
        detached,
        "audio_provenance",
        "Audio Provenance",
        super::WindowCfg {
            default_size: Some([520.0, 600.0]),
            ..Default::default()
        },
        open,
        |ui| {
            ui.horizontal(|ui| {
                ui.checkbox(&mut want_armed, "Enable");
                ui.separator();
                ui.checkbox(&mut state.follow, "Follow newest");
            });
            ui.label(
                egui::RichText::new(
                    "Records the channel outputs mixed on every CPU cycle, and which \
                 instruction last wrote each APU register.",
                )
                .small()
                .weak(),
            );
            ui.separator();

            if !armed {
                ui.label("Not armed — tick Enable, then let a frame run.");
                return;
            }

            let Some(trace) = nes.mix_trace() else {
                ui.label("Armed, but the trace is not allocated yet.");
                return;
            };
            let recs = trace.records();
            if recs.is_empty() {
                // Distinct from "not armed" on purpose. A panel whose only failure
                // mode is a confident blank report cannot be trusted even once it
                // works — the lesson the pixel inspector cost four releases.
                ui.label("Armed, but no cycles recorded for this frame yet.");
                return;
            }

            if trace.truncated() {
                ui.colored_label(
                    egui::Color32::from_rgb(0xE0, 0xA0, 0x30),
                    "⚠ Trace truncated — this frame produced more cycles than the buffer holds.",
                );
            }

            if state.follow {
                state.pin = recs.len() - 1;
            }
            state.pin = state.pin.min(recs.len() - 1);

            ui.horizontal(|ui| {
                ui.label("Cycle offset:");
                ui.add(
                    egui::Slider::new(&mut state.pin, 0..=recs.len() - 1)
                        .clamping(egui::SliderClamping::Always),
                );
            });

            let rec = recs[state.pin];
            let abs_cycle = trace.first_cycle() + state.pin as u64;
            ui.label(format!(
                "CPU cycle {abs_cycle}  ({} of {} this frame)",
                state.pin + 1,
                recs.len()
            ));
            ui.label(
                egui::RichText::new(
                    "One output sample spans ~40.6 of these cycles; a sample is a \
                 weighted sum across the filter kernel, not a copy of one cycle.",
                )
                .small()
                .weak(),
            );
            ui.separator();

            // --- the mix at that cycle ---
            let dominant = rec.dominant();
            egui::Grid::new("audio_prov_mix")
                .num_columns(3)
                .striped(true)
                .show(ui, |ui| {
                    ui.label(egui::RichText::new("Channel").strong());
                    ui.label(egui::RichText::new("Output").strong());
                    ui.label(egui::RichText::new("Share").strong());
                    ui.end_row();

                    let vals = [rec.pulse1, rec.pulse2, rec.triangle, rec.noise, rec.dmc];
                    let fulls = [15.0f32, 15.0, 15.0, 15.0, 127.0];
                    for (i, name) in CHANNELS.iter().enumerate() {
                        let lead = dominant == Some(i);
                        let label = if lead {
                            egui::RichText::new(*name).strong()
                        } else {
                            egui::RichText::new(*name)
                        };
                        ui.label(label);
                        ui.label(format!("{}", vals[i]));
                        ui.label(format!("{:.0}%", f32::from(vals[i]) / fulls[i] * 100.0));
                        ui.end_row();
                    }
                });
            ui.label(format!(
                "Mixed: {:.5}   Expansion: {:.5}",
                rec.mixed, rec.external
            ));
            ui.label(dominant.map_or_else(
                || "Dominant: none — every channel silent".to_owned(),
                |i| format!("Dominant: {}", CHANNELS[i]),
            ));

            ui.separator();
            ui.label(egui::RichText::new("Register writes").strong());

            // --- who wrote each register ---
            let Some(attrib) = nes.register_attribution() else {
                ui.label("No register attribution.");
                return;
            };
            egui::ScrollArea::vertical()
                .max_height(240.0)
                .show(ui, |ui| {
                    egui::Grid::new("audio_prov_regs")
                        .num_columns(4)
                        .striped(true)
                        .show(ui, |ui| {
                            ui.label(egui::RichText::new("Register").strong());
                            ui.label(egui::RichText::new("Value").strong());
                            ui.label(egui::RichText::new("Cycle").strong());
                            ui.label(egui::RichText::new("Written by").strong());
                            ui.end_row();

                            for (i, name) in REG_NAMES.iter().enumerate() {
                                let addr = 0x4000 + u16::try_from(i).unwrap_or(0);
                                let Some(w) = attrib.get(addr) else {
                                    continue;
                                };
                                ui.label(*name);
                                ui.label(format!("{:#04X}", w.value));
                                ui.label(format!("{}", w.cycle));
                                ui.label(source_map.annotation(w.pc).map_or_else(
                                    || format!("{:#06X}", w.pc),
                                    |s| format!("{:#06X}  {s}", w.pc),
                                ));
                                ui.end_row();

                                if let Some(extra) = side_effects(i) {
                                    ui.label("");
                                    ui.label("");
                                    ui.label("");
                                    ui.label(egui::RichText::new(extra).small().weak());
                                    ui.end_row();
                                }
                            }
                        });
                });
            ui.label(
                egui::RichText::new(
                    "Registers with no row have not been written since the last cold boot.",
                )
                .small()
                .weak(),
            );
        },
    );

    // Applied AFTER the body renders, so the core sees one arming change per
    // frame and the panel never reads a store it just freed.
    if want_armed != armed {
        nes.set_audio_provenance(want_armed);
    }
}
