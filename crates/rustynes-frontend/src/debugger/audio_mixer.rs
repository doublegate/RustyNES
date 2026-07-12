//! Audio Mixer panel (v2.1.6 "Expansion Audio", Workstream B7).
//!
//! A per-source **mix-balance** console plus **per-channel visualization** for
//! the five base 2A03 channels (pulse 1/2, triangle, noise, DMC) and the on-cart
//! **expansion-audio** contribution (VRC6/VRC7/FDS/MMC5/Namco 163/Sunsoft 5B).
//! It generalizes the NSF player's scope/VU eye-candy ([`super::nsf_panel`]) into
//! a first-class tool available for *any* ROM — including cartridge audio, not
//! just `.nsf` tunes — and adds interactive balance sliders + Mesen-style presets.
//!
//! ## Where the mix lives — and why determinism is preserved
//!
//! The balance is applied as the **frontend UI mixing overlay** that already
//! existed since v1.4.0: [`rustynes_core::Nes::set_apu_channel_gain`] (a `[f32;
//! 6]`, index 0..=4 = the base channels, index 5 = the lumped external/mapper
//! audio) and [`rustynes_core::Nes::set_apu_channel_mask`] (per-channel mute).
//! Both are documented, save-state-excluded *UI preferences* — at the unity
//! default (`[1.0; 6]`, mask all-on) the mixer takes the exact integer-gate path
//! and the deterministic per-frame audio is **byte-identical** to a build with no
//! mixer at all. Because the gains/mask are never serialized into the `.rns`
//! save state or the `.rnm` movie, a save-state / TAS / netplay replay is
//! byte-identical **regardless of the slider positions** — the recorded sound is
//! the core's own output; the mixer only re-weights it for the local speakers.
//!
//! The scopes/VU meters read the same read-only DAC taps the debugger APU panel
//! uses ([`rustynes_core::Nes::apu_snapshot`], including the v2.1.6 `external`
//! expansion tap): a copy is sampled once per redraw and nothing writes back into
//! the emulator, so the visualization is likewise determinism-neutral.

use rustynes_core::Nes;

use super::audio_scope::{ScopeRing, scope, vu_meter};
use crate::config::Config;

/// Per-channel descriptor: display label, `channel_mask` bit, `channel_gain`
/// index, and the trace/VU colour. The order matches the base-channel bit
/// layout; the expansion row (index 5) is appended and only enabled when the
/// loaded board actually has on-cart audio.
struct ChannelDesc {
    label: &'static str,
    short: &'static str,
    bit: u8,
    color: egui::Color32,
}

/// The five base 2A03 channels, in `channel_gain` / `channel_mask` index order.
const BASE_CHANNELS: [ChannelDesc; 5] = [
    ChannelDesc {
        label: "Pulse 1",
        short: "P1 ",
        bit: 0,
        color: egui::Color32::LIGHT_BLUE,
    },
    ChannelDesc {
        label: "Pulse 2",
        short: "P2 ",
        bit: 1,
        color: egui::Color32::LIGHT_GREEN,
    },
    ChannelDesc {
        label: "Triangle",
        short: "Tri",
        bit: 2,
        color: egui::Color32::LIGHT_YELLOW,
    },
    ChannelDesc {
        label: "Noise",
        short: "Noi",
        bit: 3,
        color: egui::Color32::LIGHT_RED,
    },
    ChannelDesc {
        label: "DMC",
        short: "DMC",
        bit: 4,
        color: egui::Color32::WHITE,
    },
];

/// The expansion-audio (external/mapper) mixer row (index 5). Its label is
/// overridden at runtime with the detected chip family.
const EXPANSION: ChannelDesc = ChannelDesc {
    label: "Expansion",
    short: "Ext",
    bit: 5,
    color: egui::Color32::from_rgb(0xC0, 0x90, 0xF0),
};

/// Preset: **Authentic hardware (HVC-001) levels** — unity gains. This is the
/// determinism-safe default: the mix is byte-identical to a build with no mixer.
const PRESET_AUTHENTIC: [f32; 6] = [1.0, 1.0, 1.0, 1.0, 1.0, 1.0];

/// Preset: **Balanced / rebalanced** — Mesen's "rebalanced VRC6 vs HVC-001"
/// philosophy. Several expansion chips (notably VRC6) run substantially hotter
/// than the 2A03 on real hardware; this tames the lumped expansion contribution
/// (and nudges the DMC down a touch) for a more even blend against the pulses.
const PRESET_BALANCED: [f32; 6] = [1.0, 1.0, 1.0, 1.0, 0.9, 0.65];

/// Preset: **Expansion boost (chiptune)** — the opposite bias, pushing the
/// expansion chip forward for tracks that lean on it melodically.
const PRESET_EXPANSION_BOOST: [f32; 6] = [1.0, 1.0, 1.0, 1.0, 1.0, 1.5];

/// Display + sampling state for the Audio Mixer panel. All fields are output-only
/// scope history; the authoritative mix values live in the [`Config`] audio
/// section and the core APU overlay.
#[derive(Default)]
pub struct AudioMixerState {
    pulse1: ScopeRing,
    pulse2: ScopeRing,
    triangle: ScopeRing,
    noise: ScopeRing,
    dmc: ScopeRing,
    external: ScopeRing,
    /// The combined (averaged) base-channel level, for the master scope.
    master: ScopeRing,
    /// Whether the collapsible per-channel scope section is expanded.
    scopes_open: bool,
}

impl AudioMixerState {
    /// Clear every scope trace (called on ROM load so a stale waveform doesn't
    /// bleed across the transition).
    pub fn reset_traces(&mut self) {
        self.pulse1.clear();
        self.pulse2.clear();
        self.triangle.clear();
        self.noise.clear();
        self.dmc.clear();
        self.external.clear();
        self.master.clear();
    }
}

/// Render the Audio Mixer window.
///
/// `config` owns the persisted mix (`config.audio.channel_gain` / `channel_mask`);
/// `nes` (when a ROM is loaded) provides both the read-only DAC taps for the
/// scopes and the sink for pushing any changed gain/mask down to the core APU
/// overlay. Any edit is written back to `config` and persisted immediately so the
/// mix survives a restart, exactly like the other audio settings.
#[allow(clippy::too_many_lines)]
pub fn show(
    ctx: &egui::Context,
    open: &mut bool,
    state: &mut AudioMixerState,
    config: &mut Config,
    nes: Option<&mut Nes>,
) {
    // --- Sample the live per-channel DAC levels (read-only copy) ---
    let chip = nes.as_deref().and_then(Nes::expansion_audio_chip);
    if let Some(n) = nes.as_deref() {
        let apu = n.apu_snapshot();
        let p1 = f32::from(apu.pulse1) / 15.0;
        let p2 = f32::from(apu.pulse2) / 15.0;
        let tri = f32::from(apu.triangle) / 15.0;
        let noi = f32::from(apu.noise) / 15.0;
        let dmc = f32::from(apu.dmc) / 127.0;
        // The external sample is a signed, already-mixed-scale value (~[-0.5,
        // 0.5]); fold to magnitude and clamp for the 0..=1 scope/VU convention.
        let ext = (apu.external.abs() * 2.0).clamp(0.0, 1.0);
        state.pulse1.push(p1);
        state.pulse2.push(p2);
        state.triangle.push(tri);
        state.noise.push(noi);
        state.dmc.push(dmc);
        state.external.push(ext);
        state.master.push((p1 + p2 + tri + noi + dmc) / 5.0);
    }

    let mut changed = false;

    egui::Window::new("Audio Mixer")
        .open(open)
        .default_size([360.0, 460.0])
        .resizable(true)
        .show(ctx, |ui| {
            let audio = &mut config.audio;

            // --- Master scope ---
            ui.strong("Master (base mix)");
            scope(
                ui,
                "",
                &state.master,
                egui::Color32::from_rgb(0xFF, 0xC0, 0x40),
            );
            ui.separator();

            // --- Presets ---
            ui.horizontal_wrapped(|ui| {
                ui.label("Preset:");
                if ui
                    .button("Authentic (HVC-001)")
                    .on_hover_text("Unity gains — byte-identical to the raw core mix")
                    .clicked()
                {
                    audio.channel_gain = PRESET_AUTHENTIC;
                    changed = true;
                }
                if ui
                    .button("Balanced")
                    .on_hover_text("Mesen-style rebalance: tames a hot expansion chip vs the 2A03")
                    .clicked()
                {
                    audio.channel_gain = PRESET_BALANCED;
                    changed = true;
                }
                if ui
                    .button("Expansion boost")
                    .on_hover_text("Pushes the on-cart expansion chip forward")
                    .clicked()
                {
                    audio.channel_gain = PRESET_EXPANSION_BOOST;
                    changed = true;
                }
            });
            ui.add_space(4.0);

            // --- Per-channel mix rows: mute | name | gain slider | VU ---
            ui.strong("Mix balance");
            egui::Grid::new("mixer_rows")
                .num_columns(4)
                .spacing([8.0, 4.0])
                .striped(true)
                .show(ui, |ui| {
                    for (i, desc) in BASE_CHANNELS.iter().enumerate() {
                        let peak = base_peak(state, i);
                        changed |= channel_row(ui, desc, audio, peak, true);
                        ui.end_row();
                    }
                    // Expansion row — enabled only when the board has on-cart audio.
                    let label = chip.unwrap_or("Expansion (none loaded)");
                    let ext_desc = ChannelDesc { label, ..EXPANSION };
                    changed |=
                        channel_row(ui, &ext_desc, audio, state.external.peak(), chip.is_some());
                    ui.end_row();
                });

            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if ui.button("Reset to unity").clicked() {
                    audio.channel_gain = PRESET_AUTHENTIC;
                    audio.channel_mask = 0x3F;
                    changed = true;
                }
                ui.weak("Gains 0.0 – 2.0; unity = authentic hardware.");
            });

            ui.separator();

            // --- Collapsible per-channel scopes ---
            egui::CollapsingHeader::new("Per-channel scopes")
                .default_open(state.scopes_open)
                .show(ui, |ui| {
                    scope(
                        ui,
                        BASE_CHANNELS[0].label,
                        &state.pulse1,
                        BASE_CHANNELS[0].color,
                    );
                    scope(
                        ui,
                        BASE_CHANNELS[1].label,
                        &state.pulse2,
                        BASE_CHANNELS[1].color,
                    );
                    scope(
                        ui,
                        BASE_CHANNELS[2].label,
                        &state.triangle,
                        BASE_CHANNELS[2].color,
                    );
                    scope(
                        ui,
                        BASE_CHANNELS[3].label,
                        &state.noise,
                        BASE_CHANNELS[3].color,
                    );
                    scope(
                        ui,
                        BASE_CHANNELS[4].label,
                        &state.dmc,
                        BASE_CHANNELS[4].color,
                    );
                    if let Some(name) = chip {
                        scope(ui, name, &state.external, EXPANSION.color);
                    }
                });

            ui.add_space(4.0);
            ui.weak(
                "The mix is a frontend UI overlay: it re-weights the core's own \
                 samples for your speakers only. Save-states, movies, and netplay \
                 stay byte-identical regardless of these sliders.",
            );

            if nes.as_deref().is_none() {
                ui.weak("Load a ROM or NSF to see live channel levels.");
            }
        });

    // --- Apply + persist any change (after the egui pass, no lock held here) ---
    if changed {
        if let Some(n) = nes {
            n.set_apu_channel_gain(config.audio.channel_gain);
            n.set_apu_channel_mask(config.audio.channel_mask);
        }
        // Persist so the mix survives a restart, like the other audio prefs.
        let _ = config.save();
    }
}

/// Peak for the i-th base channel (indexing the fixed set of scope rings).
fn base_peak(state: &AudioMixerState, i: usize) -> f32 {
    match i {
        0 => state.pulse1.peak(),
        1 => state.pulse2.peak(),
        2 => state.triangle.peak(),
        3 => state.noise.peak(),
        _ => state.dmc.peak(),
    }
}

/// Render one mixer row (mute checkbox, label, gain slider, VU bar). Returns
/// `true` if the user changed the mute or gain. `enabled` greys the whole row
/// out (used for the expansion row when no on-cart audio is present).
fn channel_row(
    ui: &mut egui::Ui,
    desc: &ChannelDesc,
    audio: &mut crate::config::AudioConfig,
    peak: f32,
    enabled: bool,
) -> bool {
    let mut changed = false;
    let bit = 1u8 << desc.bit;

    ui.add_enabled_ui(enabled, |ui| {
        // Mute checkbox (an unchecked box = muted, i.e. mask bit cleared).
        let mut audible = audio.channel_mask & bit != 0;
        if ui
            .checkbox(&mut audible, "")
            .on_hover_text("Audible / muted")
            .changed()
        {
            if audible {
                audio.channel_mask |= bit;
            } else {
                audio.channel_mask &= !bit;
            }
            changed = true;
        }
        ui.colored_label(desc.color, desc.label);
    });
    // Gain slider.
    ui.add_enabled_ui(enabled, |ui| {
        let g = &mut audio.channel_gain[desc.bit as usize];
        if ui
            .add(egui::Slider::new(g, 0.0..=2.0).fixed_decimals(2))
            .changed()
        {
            changed = true;
        }
    });
    // VU peak.
    vu_meter(ui, desc.short, peak, desc.color);
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    // These indirections are also a `clippy::assertions_on_constants` barrier:
    // asserting a function's return keeps the check a runtime one even though the
    // presets are `const`.

    /// Are all six gain entries within the slider's clamp range `0.0..=2.0`?
    fn in_range(gains: &[f32; 6]) -> bool {
        gains.iter().all(|&g| (0.0..=2.0).contains(&g))
    }

    /// Is every entry exactly unity (bit-exact, to sidestep `float_cmp`)?
    fn all_unity(gains: &[f32; 6]) -> bool {
        gains.iter().all(|&g| g.to_bits() == 1.0_f32.to_bits())
    }

    fn expansion_gain(gains: &[f32; 6]) -> f32 {
        gains[5]
    }

    #[test]
    fn every_preset_is_within_slider_range() {
        // Presets must never push a gain outside the UI's clamp, or applying one
        // and then nudging a slider would jump the value.
        assert!(in_range(&PRESET_AUTHENTIC));
        assert!(in_range(&PRESET_BALANCED));
        assert!(in_range(&PRESET_EXPANSION_BOOST));
    }

    #[test]
    fn authentic_preset_is_the_determinism_safe_unity() {
        // The authentic/default preset must be exact unity — the byte-identical
        // core-mix path — while the biased presets are NOT all-unity.
        assert!(all_unity(&PRESET_AUTHENTIC));
        assert!(!all_unity(&PRESET_BALANCED));
    }

    #[test]
    fn balanced_and_boost_bias_the_expansion_channel_opposite_ways() {
        // Balanced tames the hot expansion chip; boost pushes it forward. The
        // two presets must bracket unity on the expansion (index 5) channel.
        assert!(expansion_gain(&PRESET_BALANCED) < 1.0);
        assert!(expansion_gain(&PRESET_EXPANSION_BOOST) > 1.0);
    }
}
