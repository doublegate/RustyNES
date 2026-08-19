//! v2.3.8 "Parallax" — the Divergence Lens panel.
//!
//! The UI over [`rustynes_probe::divergence`]. The engine is headless and tested
//! there; this file is the surface, and deliberately thin — the v2.3.6 lesson is
//! that untested frontend wiring is where a working core goes to die, so the
//! panel does no analysis of its own.
//!
//! # What it asks
//!
//! "If this work-RAM byte were a different value right now, what on screen — or
//! in the audio — would change?"
//!
//! It composes with the RAM Atlas rather than duplicating it. The Atlas answers
//! *is this address live*; the Lens answers *what does it actually change*, down
//! to a pixel set or a CPU cycle. A work-RAM byte is the perturbation because it
//! is the one the Atlas already hands you, and because it is exactly reversible:
//! the trial engine restores the anchor, so nothing survives the measurement.
//!
//! # Declining rather than guessing
//!
//! Three outcomes are rendered as three visibly different things, and the panel
//! never collapses them:
//!
//! - **differs** — here is where;
//! - **identical** — the two configurations really did produce the same output;
//! - **inconclusive** — the budget ran out, or the coarse detector and the fine
//!   localiser disagreed. This is "I stopped looking", NOT "no divergence", and
//!   rendering it as the latter would be the failure the Latency Oracle's
//!   `None`-vs-`Some(0)` split exists to prevent.

use rustynes_core::{Buttons, Nes};
use rustynes_probe::Probe;
use rustynes_probe::divergence::{
    self, AudioLocalisation, Localisation, PixelCause, PixelDivergence, budget_for,
};

/// Frames each configuration is replayed for.
///
/// Long enough that a byte whose effect is delayed by a few frames still shows
/// up, short enough that four trials stay under a second. Not user-tunable yet:
/// a knob whose effect nobody has measured is a knob that invites cargo-culting.
const WINDOW_FRAMES: u32 = 30;

/// Which lens the user asked for.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Lens {
    Video,
    Audio,
}

/// Persistent panel state.
pub struct DivergencePanel {
    /// Work-RAM address to perturb, `$0000-$07FF`.
    addr: u16,
    /// Value written at the perturbed address before frame 0.
    value: u8,
    /// The most recent video result, if one was run for this ROM.
    video: Option<Localisation>,
    /// Why the located pixel differs, when the trials could record it.
    ///
    /// Separate from `video` rather than folded into it, because `None` here is
    /// "no provenance record for that pixel" and NOT "no difference" — the two
    /// would be indistinguishable inside a single optional.
    cause: Option<PixelCause>,
    /// The most recent audio result, if one was run for this ROM.
    audio: Option<AudioLocalisation>,
    /// A run the user asked for; drained after the render so `nes` is never
    /// captured by the viewport callback.
    requested: Option<Lens>,
    /// Status / error line.
    status: String,
}

impl Default for DivergencePanel {
    fn default() -> Self {
        Self {
            addr: 0,
            value: 0xFF,
            video: None,
            cause: None,
            audio: None,
            requested: None,
            status: String::new(),
        }
    }
}

impl DivergencePanel {
    /// Discard everything bound to the previous ROM.
    ///
    /// A located divergence names pixels or a CPU cycle in one game's frame.
    /// Left standing across a ROM change it is a precise statement about a
    /// cartridge it was never measured on — the same seam that has now caught
    /// the Pixel Provenance, Latency Oracle, and RAM Atlas panels, which is why
    /// this registers with the shared hook instead of adding a fourth bespoke
    /// clear.
    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

/// Draw the Divergence Lens window.
///
/// `nes` is `Some` only when a ROM is loaded under the held lock.
/// `writes_locked` is the shared locked-session predicate: this panel advances
/// the live emulator AND pokes work RAM, so it is gated exactly as the RAM Atlas
/// is, and for the same reasons — under netplay or a TAS the replayed frames are
/// consumed by peers before the anchor is restored, and under `RetroAchievements`
/// hardcore a work-RAM write is what the mode exists to forbid.
pub fn show(
    ctx: &egui::Context,
    detached: &mut std::collections::HashSet<&'static str>,
    open: &mut bool,
    state: &mut DivergencePanel,
    nes: Option<&mut Nes>,
    writes_locked: bool,
) {
    let can_run = nes.is_some() && !writes_locked;
    super::detachable_window(
        ctx,
        detached,
        "divergence_lens",
        "Divergence Lens",
        super::WindowCfg {
            default_width: Some(420.0),
            ..Default::default()
        },
        open,
        |ui| body(ui, state, can_run, writes_locked),
    );
    // Run AFTER the render — `nes` is free here, not captured by any closure.
    if let Some(lens) = state.requested.take() {
        run(state, nes, lens);
    }
}

/// The panel body, shared by the docked window and the detached OS viewport.
fn body(ui: &mut egui::Ui, state: &mut DivergencePanel, can_run: bool, writes_locked: bool) {
    ui.label("Shows what a single work-RAM byte actually changes.");
    ui.weak(
        "Replays this moment twice — once as-is, once with the byte set — and \
         reports the first frame that differs, down to the pixels or the CPU \
         cycle. Briefly pauses the emulator and leaves it exactly where it was.",
    );
    ui.separator();

    ui.horizontal(|ui| {
        ui.label("Address");
        let mut addr = i64::from(state.addr);
        if ui
            .add(
                egui::DragValue::new(&mut addr)
                    .range(0..=0x7FF)
                    .hexadecimal(3, false, true),
            )
            .changed()
        {
            // Clamped by the range above; the cast cannot lose information.
            state.addr = u16::try_from(addr.clamp(0, 0x7FF)).unwrap_or(0);
        }
        ui.label("Value");
        ui.add(egui::DragValue::new(&mut state.value).hexadecimal(2, false, true));
    });

    ui.horizontal(|ui| {
        if ui
            .add_enabled(can_run, egui::Button::new("Locate (video)"))
            .clicked()
        {
            state.requested = Some(Lens::Video);
        }
        if ui
            .add_enabled(can_run, egui::Button::new("Locate (audio)"))
            .clicked()
        {
            state.requested = Some(Lens::Audio);
        }
    });

    if !can_run {
        ui.weak(if writes_locked {
            "Unavailable during netplay, TAS record/replay, or RetroAchievements \
             hardcore: this replays and pokes the live emulator."
        } else {
            "Load a ROM to run the lens."
        });
    }
    if !state.status.is_empty() {
        ui.weak(&state.status);
    }

    if let Some(result) = state.video {
        ui.separator();
        ui.strong("Video");
        video_result(ui, result);
        if matches!(result, Localisation::Differs(_)) {
            cause_body(ui, state.cause.as_ref());
        }
    }
    if let Some(result) = state.audio {
        ui.separator();
        ui.strong("Audio");
        audio_result(ui, result);
    }
}

/// One-line verdict for a video result, and the elaboration under it.
///
/// Extracted from the render rather than inlined into the `egui` closure so the
/// requirement that the three outcomes render as three DIFFERENT things is
/// machine-checked instead of eyeballed. A panel whose "inconclusive" and
/// "identical" branches drifted into saying the same thing would look correct in
/// review and be wrong for the user, which is the specific failure the Latency
/// Oracle's `None`-vs-`Some(0)` split was written to avoid.
const fn video_text(result: Localisation) -> (&'static str, &'static str) {
    match result {
        // The headline for a located divergence is rendered by `pixel_body`,
        // which has real numbers to show; this arm exists so the mapping is
        // total and the test can assert all three are distinct.
        Localisation::Differs(_) => (
            "Differs — located.",
            "The first differing frame, the pixels, and the bounding box are below.",
        ),
        Localisation::Identical => (
            "Identical — this byte changed nothing on screen in the window.",
            "A real answer, not a failure: the two runs produced the same frames. \
             It does not mean the byte is unused; see the RAM Atlas on why `Inert` \
             is not `unused`.",
        ),
        Localisation::Inconclusive => (
            "Inconclusive — the lens stopped looking.",
            "NOT the same as 'nothing changed'. The trial budget ran out, or the \
             frame detector and the pixel diff disagreed.",
        ),
    }
}

/// Render a video result. Each of the three outcomes gets its own shape.
fn video_result(ui: &mut egui::Ui, result: Localisation) {
    if let Localisation::Differs(d) = result {
        pixel_body(ui, d);
        return;
    }
    let (headline, detail) = video_text(result);
    ui.label(headline);
    ui.weak(detail);
}

/// The located pixel set.
fn pixel_body(ui: &mut egui::Ui, d: PixelDivergence) {
    let (min_x, min_y, max_x, max_y) = d.bounding_box();
    ui.label(format!("First differing frame: {}", d.frame));
    ui.label(format!(
        "{} pixel{} differ, first at ({}, {})",
        d.count,
        if d.count == 1 { "" } else { "s" },
        d.first.0,
        d.first.1
    ));
    ui.label(format!(
        "Bounding box: ({min_x}, {min_y}) to ({max_x}, {max_y}){}",
        if d.is_single_scanline() {
            " — one scanline"
        } else {
            ""
        }
    ));
    ui.weak("Open Pixel Provenance and click that pixel for the instruction behind it.");
}

/// Render why the located pixel differs.
///
/// `None` means the trials recorded no provenance for that pixel, which is a
/// real and distinct outcome from "nothing differs" — the store is per frame,
/// and a pixel the PPU never emitted has nothing to report. Rendered as such
/// rather than silently omitted, because a missing section reads as an answer.
fn cause_body(ui: &mut egui::Ui, cause: Option<&PixelCause>) {
    let Some(cause) = cause else {
        ui.weak("No provenance record for that pixel, so the cause is unknown.");
        return;
    };
    let fields = cause.differing_fields();
    if fields.is_empty() {
        // Not reachable for a located pixel — an index entry is
        // `(emphasis << 6) | colour`, so a differing entry must differ in
        // `color` or `color_mask`. Surfaced rather than hidden: if it ever
        // appears, the two records did not come from the two configurations,
        // and a blank section would make that invisible.
        ui.weak("Records match — unexpected here; please report it.");
        return;
    }
    ui.label(format!("Differs in: {}", fields.join(", ")));
    let (a, b) = (&cause.baseline, &cause.variant);
    if a.pattern_addr != b.pattern_addr {
        ui.weak(format!(
            "pattern ${:04X} -> ${:04X}  (different tile data fetched)",
            a.pattern_addr, b.pattern_addr
        ));
    }
    if a.nt_addr != b.nt_addr {
        ui.weak(format!(
            "nametable ${:04X} -> ${:04X}",
            a.nt_addr, b.nt_addr
        ));
    }
    if a.palette_addr != b.palette_addr {
        ui.weak(format!(
            "palette ${:04X} -> ${:04X}",
            a.palette_addr, b.palette_addr
        ));
    }
    if a.layer != b.layer {
        ui.weak(format!("layer {:?} -> {:?}", a.layer, b.layer));
    }
    ui.weak(format!("emitted at scanline {} dot {}", a.scanline, a.dot));
}

/// Render an audio result.
fn audio_result(ui: &mut egui::Ui, result: AudioLocalisation) {
    match result {
        AudioLocalisation::Differs(d) => {
            ui.label(format!("First differing frame: {}", d.frame));
            ui.label(format!("First differing CPU cycle: {}", d.cycle));
            ui.label(format!(
                "mixed {:.5} -> {:.5}",
                d.baseline.mixed, d.variant.mixed
            ));
            ui.weak(format!(
                "pulse1 {}->{}  pulse2 {}->{}  tri {}->{}  noise {}->{}  dmc {}->{}",
                d.baseline.pulse1,
                d.variant.pulse1,
                d.baseline.pulse2,
                d.variant.pulse2,
                d.baseline.triangle,
                d.variant.triangle,
                d.baseline.noise,
                d.variant.noise,
                d.baseline.dmc,
                d.variant.dmc,
            ));
        }
        other => {
            let (headline, detail) = audio_text(other);
            ui.label(headline);
            ui.weak(detail);
        }
    }
}

/// Run one lens against the live emulator.
///
/// The anchor is the emulator's current state and every trial restores it, so
/// the live timeline is untouched — the same contract `latency::measure_in_place`
/// offers, and as of v2.3.7 that includes the user's provenance records, which
/// the trial engine now carries across its own restores.
fn run(state: &mut DivergencePanel, nes: Option<&mut Nes>, lens: Lens) {
    let Some(nes) = nes else {
        "No ROM loaded.".clone_into(&mut state.status);
        return;
    };
    let addr = state.addr as usize;
    let value = state.value;
    let setup = move |n: &mut Nes| {
        // In range by the DragValue clamp above, and re-checked rather than
        // indexed on trust: a panic here would take the emulator thread down.
        if let Some(byte) = n.wram_mut().get_mut(addr) {
            *byte = value;
        }
    };
    let idle = |_: u32| (Buttons::empty(), Buttons::empty());
    let mut probe = Probe::anchor(&*nes, budget_for(WINDOW_FRAMES));

    match lens {
        Lens::Video => {
            // The explained variant, which costs the same four trials: the two
            // localisation runs capture their own provenance rather than running
            // unarmed, so the cause arrives with the divergence instead of
            // needing a fifth and sixth run.
            let (result, cause) =
                divergence::localise_explained(&mut probe, nes, WINDOW_FRAMES, setup, idle);
            state.video = Some(result);
            state.cause = cause;
        }
        Lens::Audio => {
            state.audio = Some(divergence::localise_audio(
                &mut probe,
                nes,
                WINDOW_FRAMES,
                setup,
                idle,
            ));
        }
    }
    state.status = format!("Ran {WINDOW_FRAMES} frames against ${addr:04X} = ${value:02X}.");
}

/// One-line verdict for an audio result. Companion to [`video_text`], extracted
/// for the same reason.
const fn audio_text(result: AudioLocalisation) -> (&'static str, &'static str) {
    match result {
        AudioLocalisation::Differs(_) => (
            "Differs — located.",
            "The first differing frame, CPU cycle, and per-channel outputs are below.",
        ),
        AudioLocalisation::Identical => (
            "Identical — this byte changed nothing audible in the window.",
            "A real answer: the two runs mixed the same samples, cycle for cycle.",
        ),
        AudioLocalisation::Inconclusive => (
            "Inconclusive — the lens stopped looking.",
            "NOT 'nothing changed'. The budget ran out, the two mix traces could \
             not be aligned, or the energy detector and the per-cycle records \
             disagreed.",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustynes_core::rustynes_apu::provenance::MixRecord;
    use rustynes_core::rustynes_ppu::provenance::PixelProvenance;
    use rustynes_probe::divergence::AudioDivergence;

    fn a_pixel_divergence() -> PixelDivergence {
        PixelDivergence {
            frame: 3,
            count: 1,
            first: (1, 2),
            bbox: (1, 2, 1, 2),
        }
    }

    fn an_audio_divergence() -> AudioDivergence {
        AudioDivergence {
            frame: 3,
            cycle: 99,
            baseline: MixRecord::default(),
            variant: MixRecord {
                pulse1: 7,
                ..MixRecord::default()
            },
        }
    }

    /// The plan's explicit requirement: "they agree", "nothing ran", and "here is
    /// where" must render as three different things. Asserted rather than
    /// eyeballed, because two of the three are the pair that a careless edit
    /// collapses.
    #[test]
    fn the_three_video_outcomes_read_differently() {
        let differs = video_text(Localisation::Differs(a_pixel_divergence()));
        let identical = video_text(Localisation::Identical);
        let inconclusive = video_text(Localisation::Inconclusive);

        assert_ne!(differs.0, identical.0);
        assert_ne!(differs.0, inconclusive.0);
        assert_ne!(
            identical.0, inconclusive.0,
            "'they agree' and 'I stopped looking' must not read the same"
        );
        assert_ne!(identical.1, inconclusive.1, "nor may their elaborations");
    }

    #[test]
    fn the_three_audio_outcomes_read_differently() {
        let differs = audio_text(AudioLocalisation::Differs(an_audio_divergence()));
        let identical = audio_text(AudioLocalisation::Identical);
        let inconclusive = audio_text(AudioLocalisation::Inconclusive);

        assert_ne!(differs.0, identical.0);
        assert_ne!(differs.0, inconclusive.0);
        assert_ne!(identical.0, inconclusive.0);
        assert_ne!(identical.1, inconclusive.1);
    }

    /// An inconclusive result must not be worded as an absence of divergence.
    /// The wording IS the contract here — the user acts on this sentence.
    #[test]
    fn inconclusive_never_claims_nothing_changed() {
        for (headline, detail) in [
            video_text(Localisation::Inconclusive),
            audio_text(AudioLocalisation::Inconclusive),
        ] {
            assert!(
                headline.contains("Inconclusive"),
                "headline must name the outcome: {headline}"
            );
            assert!(
                detail.contains("NOT"),
                "the elaboration must say what this is not: {detail}"
            );
        }
    }

    /// A located result describes one game's frame, so it must not survive a ROM
    /// change. `clear` is the panel's half of the shared
    /// `DebuggerOverlay::clear_rom_bound_analysis` hook, which cannot itself be
    /// unit-tested — constructing the overlay needs a `wgpu::Device` and a
    /// window.
    #[test]
    fn clear_discards_every_rom_bound_field() {
        let mut panel = DivergencePanel {
            addr: 0x123,
            value: 0x42,
            video: Some(Localisation::Differs(a_pixel_divergence())),
            cause: Some(PixelCause {
                pixel: (1, 2),
                baseline: PixelProvenance::default(),
                variant: PixelProvenance::default(),
            }),
            audio: Some(AudioLocalisation::Identical),
            requested: Some(Lens::Video),
            status: "stale".to_owned(),
        };

        panel.clear();

        assert!(
            panel.video.is_none(),
            "a located pixel set outlived its ROM"
        );
        assert!(
            panel.cause.is_none(),
            "a pixel cause outlived its ROM — it names tile and palette addresses \
             from one game's frame"
        );
        assert!(panel.audio.is_none(), "a located cycle outlived its ROM");
        assert!(panel.requested.is_none(), "a queued run outlived its ROM");
        assert!(panel.status.is_empty());
        assert_eq!(panel.addr, 0, "the perturbation target is ROM-bound too");
    }
}
