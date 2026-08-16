//! Measuring a game's **own** input lag.
//!
//! Most NES titles sample the controller in their NMI handler and act on it one
//! or more frames later. That delay is real on hardware, and run-ahead removes
//! it by simulating those frames in advance — but only if you tell it how many.
//!
//! Every emulator makes finding that number a manual ritual: hold a direction,
//! frame-advance until the sprite moves, subtract one. `RetroArch` documents
//! exactly that procedure; `RustyNES`'s own settings panel says only "1 fits most
//! games". Nothing measures it.
//!
//! This does, by asking the question directly: replay the same anchor twice,
//! once with a button held and once with nothing pressed, and find the first
//! frame at which the two runs differ. That index **is** the game's internal
//! lag, because it is the first frame on which pressing the button could have
//! changed anything.
//!
//! # Being honest is the hard part
//!
//! A latency number is acted on — it sets run-ahead depth, which costs real
//! frame budget. So this module is built to **decline** rather than guess:
//!
//! - It probes several buttons, because a given game may ignore most of them,
//!   and requires them to *agree* before reporting a value.
//! - It falls back across observables, because a reaction may be audible or
//!   internal before it is visible — a menu that commits a highlight to a
//!   variable one frame before drawing it would otherwise read as slower than
//!   it is.
//! - It reports [`Confidence`] alongside the number, and returns
//!   [`LatencyReport::frames`] as `None` whenever the trials disagree or nothing
//!   reacted inside the budget.
//!
//! "I could not tell" is a valid, useful answer. A wrong depth silently spends
//! frame budget the host may not have.

use rustynes_core::{Buttons, Nes};

use crate::{Budget, Observable, Probe};

/// Buttons worth probing, in the order tried.
///
/// Directions first: they are what most games act on soonest and what a player
/// is holding when latency matters. `A`/`B` next, since action games respond to
/// them. `START` last and deliberately — it pauses many games, which is a
/// reaction, but a reaction to a *menu*, not to gameplay input, and treating a
/// pause as gameplay latency would over-report.
const PROBE_BUTTONS: [Buttons; 6] = [
    Buttons::RIGHT,
    Buttons::LEFT,
    Buttons::DOWN,
    Buttons::UP,
    Buttons::A,
    Buttons::B,
];

/// Observables tried in order until one produces agreeing answers.
///
/// Framebuffer first — a visible reaction is what a player perceives as latency.
/// Audio next: a sound effect often fires the same frame the input is accepted,
/// before anything is drawn. Work RAM last, because it detects a reaction the
/// player cannot yet perceive; useful as evidence the game read the pad at all,
/// but the least representative of *felt* latency.
const OBSERVABLE_ORDER: [Observable; 3] = [
    Observable::Framebuffer,
    Observable::AudioEnergy,
    Observable::Wram,
];

/// How much to trust a measurement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Confidence {
    /// Every button that reacted agreed on the same frame.
    Unanimous,
    /// A majority agreed; at least one reacting button disagreed. Usable, but a
    /// caller applying it automatically should say it is approximate.
    Majority,
    /// No value: nothing reacted inside the budget, or the reacting buttons did
    /// not agree closely enough to pick one.
    Inconclusive,
}

/// The result of a measurement.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LatencyReport {
    /// Measured internal lag in frames, or `None` when inconclusive.
    ///
    /// `None` and `Some(0)` are **different answers** and must not be collapsed:
    /// `Some(0)` means the game reacted on the very next frame, `None` means the
    /// probe could not tell. Reporting the second as the first is how a latency
    /// tool starts lying.
    pub frames: Option<u32>,
    /// How much to trust [`Self::frames`].
    pub confidence: Confidence,
    /// Buttons that produced any divergence at all.
    pub reacting_buttons: u32,
    /// Buttons probed.
    pub probed_buttons: u32,
    /// Which observable produced the answer, when there is one.
    pub observable: Option<Observable>,
    /// Per-button first-divergence frames, in `PROBE_BUTTONS` order (a plain
    /// code span: that item is private and `rustdoc::private_intra_doc_links` is
    /// denied), for a UI
    /// that wants to show the evidence rather than just the conclusion.
    pub per_button: Vec<Option<u32>>,
}

impl LatencyReport {
    /// An inconclusive report, with the evidence that produced it.
    fn inconclusive(per_button: Vec<Option<u32>>, probed: u32) -> Self {
        let reacting =
            u32::try_from(per_button.iter().filter(|d| d.is_some()).count()).unwrap_or(u32::MAX);
        Self {
            frames: None,
            confidence: Confidence::Inconclusive,
            reacting_buttons: reacting,
            probed_buttons: probed,
            observable: None,
            per_button,
        }
    }

    /// A run-ahead depth this measurement supports, clamped to `max_depth`.
    ///
    /// Returns `None` for an inconclusive report: **leave the user's setting
    /// alone rather than guess**. Run-ahead depth is linear in the core's frame
    /// cost (roughly 34% / 52% / 78% of the NTSC budget at depth 0 / 1 / 2), so
    /// applying a fabricated depth spends real budget for nothing.
    #[must_use]
    pub fn suggested_run_ahead(&self, max_depth: u32) -> Option<u32> {
        self.frames.map(|f| f.min(max_depth))
    }
}

/// How a measurement is run.
#[derive(Clone, Copy, Debug)]
pub struct LatencyConfig {
    /// Frames to simulate per trial. A game that has not reacted within this
    /// many frames is reported as inconclusive rather than as zero-lag.
    pub frames_per_trial: u32,
    /// Largest lag treated as a real measurement. A divergence beyond this is
    /// far more likely to be the game's own animation or a timer than a reaction
    /// to input, so it is discarded rather than reported.
    pub max_plausible_lag: u32,
}

impl Default for LatencyConfig {
    fn default() -> Self {
        Self {
            frames_per_trial: 20,
            // Games buffer input by a frame or three. Ten is generous; past it,
            // "the screen changed" almost certainly means something else moved.
            max_plausible_lag: 10,
        }
    }
}

/// Measure the game's internal input lag from the emulator's current state.
///
/// `nes` is a scratch instance the probe replays into — it is rewound to the
/// anchor repeatedly and left wherever the last trial ended, so do not pass the
/// live emulator.
///
/// Returns as soon as an observable yields agreeing answers, so the common case
/// costs one observable's worth of trials rather than all three.
pub fn measure(nes: &mut Nes, anchor: &Nes, cfg: LatencyConfig) -> LatencyReport {
    let budget = Budget {
        max_frames_per_trial: cfg.frames_per_trial,
        // Two trials per button per observable, plus headroom.
        max_trials: u32::try_from((PROBE_BUTTONS.len() + 1) * 2 * OBSERVABLE_ORDER.len())
            .unwrap_or(u32::MAX),
    };
    let probe = Probe::anchor(anchor, budget);
    let probed = u32::try_from(PROBE_BUTTONS.len()).unwrap_or(u32::MAX);
    let mut last_evidence = vec![None; PROBE_BUTTONS.len()];

    for observable in OBSERVABLE_ORDER {
        // The idle baseline is the same for every button under one observable,
        // so it is run once rather than per button.
        let idle = probe.run(nes, cfg.frames_per_trial, observable, |_| {
            (Buttons::empty(), Buttons::empty())
        });

        let mut per_button = Vec::with_capacity(PROBE_BUTTONS.len());
        for button in PROBE_BUTTONS {
            let held = probe.run(nes, cfg.frames_per_trial, observable, move |_| {
                (button, Buttons::empty())
            });
            let d = Probe::first_divergence(&held, &idle).filter(|f| *f <= cfg.max_plausible_lag);
            per_button.push(d);
        }

        if let Some(report) = conclude(&per_button, probed, observable) {
            return report;
        }
        last_evidence = per_button;
    }

    LatencyReport::inconclusive(last_evidence, probed)
}

/// Turn per-button divergences into a verdict, or `None` if this observable
/// cannot support one.
fn conclude(
    per_button: &[Option<u32>],
    probed: u32,
    observable: Observable,
) -> Option<LatencyReport> {
    let reacting: Vec<u32> = per_button.iter().filter_map(|d| *d).collect();
    if reacting.is_empty() {
        return None;
    }

    // Pick the most common answer. Ties resolve to the SMALLEST frame, which is
    // the conservative direction: under-reporting lag sets a lower run-ahead
    // depth, which costs the user less frame budget than over-reporting.
    let mut best = (0usize, u32::MAX);
    for &candidate in &reacting {
        let votes = reacting.iter().filter(|f| **f == candidate).count();
        if votes > best.0 || (votes == best.0 && candidate < best.1) {
            best = (votes, candidate);
        }
    }
    let (votes, frames) = best;

    let confidence = if votes == reacting.len() {
        Confidence::Unanimous
    } else if votes * 2 > reacting.len() {
        Confidence::Majority
    } else {
        // A plurality is not agreement. Two buttons saying "1" and two saying
        // "4" is a game doing something this probe does not understand, and the
        // honest output is no number at all.
        return None;
    };

    Some(LatencyReport {
        frames: Some(frames),
        confidence,
        reacting_buttons: u32::try_from(reacting.len()).unwrap_or(u32::MAX),
        probed_buttons: probed,
        observable: Some(observable),
        per_button: per_button.to_vec(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An NROM that never reads the controller — the honesty fixture.
    fn spin_rom() -> Vec<u8> {
        let mut prg = vec![0u8; 16 * 1024];
        prg[0] = 0x4C; // JMP $C000
        prg[1] = 0x00;
        prg[2] = 0xC0;
        wrap_nrom(prg)
    }

    /// An NROM that latches the controller **once per frame in its NMI handler**
    /// — the structure essentially every real NES game uses — and shifts the
    /// eight bits into `$0300`. The main loop does nothing.
    ///
    /// Frame quantisation is the point. An earlier version of this fixture
    /// polled `$4016` continuously from the main loop, and the measurement came
    /// back split 3-3 between frames 0 and 5: sampling at end-of-frame caught
    /// that loop at a different point in its eight-bit shift for buttons read
    /// early (`A`, `B`) versus late (`Right`, `Left`, `Down`), so *which bit
    /// position a button occupies* leaked into the answer. That is an artefact of
    /// a ROM no real game resembles, not a property of the measurement — and the
    /// split correctly produced "inconclusive", which is the algorithm behaving
    /// as designed on nonsense input.
    fn polling_rom() -> Vec<u8> {
        let mut prg = vec![0u8; 16 * 1024];
        // $C000: enable NMI, forever.
        //
        // Re-asserted in the loop rather than written once, because the PPU
        // IGNORES `$2000` writes for roughly its first 29,658 CPU cycles after
        // reset. A single write at reset lands inside that window, is discarded,
        // and NMI never fires — which presented here as every button reporting
        // "no reaction", i.e. a fixture that silently tested nothing.
        let reset: &[u8] = &[
            0xA9, 0x80, // LDA #$80
            0x8D, 0x00, 0x20, // STA $2000   (NMI on VBlank)
            0x4C, 0x00, 0xC0, // JMP $C000
        ];
        prg[..reset.len()].copy_from_slice(reset);

        // $C020: the NMI handler — strobe, read 8 bits into $0300, return.
        let nmi: &[u8] = &[
            0xA9, 0x01, // LDA #$01
            0x8D, 0x16, 0x40, // STA $4016   (strobe on)
            0xA9, 0x00, // LDA #$00
            0x8D, 0x16, 0x40, // STA $4016   (strobe off -> latch)
            0xA2, 0x08, // LDX #$08
            0xA9, 0x00, // LDA #$00
            0x8D, 0x00, 0x03, // STA $0300
            // read loop @ $C02F
            0xAD, 0x16, 0x40, // LDA $4016
            0x4A, // LSR A        (bit 0 -> carry)
            0x2E, 0x00, 0x03, // ROL $0300
            0xCA, // DEX
            0xD0, 0xF6, // BNE -10 -> back to LDA $4016
            0x40, // RTI
        ];
        prg[0x20..0x20 + nmi.len()].copy_from_slice(nmi);
        wrap_nrom_with_nmi(prg, 0xC020)
    }

    fn wrap_nrom(prg: Vec<u8>) -> Vec<u8> {
        wrap_nrom_with_nmi(prg, 0xC000)
    }

    fn wrap_nrom_with_nmi(prg: Vec<u8>, nmi_addr: u16) -> Vec<u8> {
        let mut prg = prg;
        let len = prg.len();
        prg[len - 6] = (nmi_addr & 0xFF) as u8; // NMI
        prg[len - 5] = (nmi_addr >> 8) as u8;
        prg[len - 4] = 0x00; // RESET-> $C000
        prg[len - 3] = 0xC0;
        prg[len - 2] = 0x00; // IRQ  -> $C000
        prg[len - 1] = 0xC0;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"NES\x1A");
        bytes.push(1);
        bytes.push(1);
        bytes.push(0);
        bytes.push(0);
        bytes.extend_from_slice(&[0u8; 8]);
        bytes.extend_from_slice(&prg);
        bytes.extend_from_slice(&vec![0u8; 8 * 1024]);
        bytes
    }

    fn warmed(rom: &[u8], frames: u32) -> Nes {
        let mut nes = Nes::from_rom(rom).expect("fixture parses");
        for _ in 0..frames {
            nes.run_frame();
        }
        nes
    }

    /// THE honesty property. A ROM that never reads the controller must be
    /// reported as **inconclusive**, never as zero-lag.
    ///
    /// `Some(0)` would be acted on: it sets run-ahead to 0 and tells the user the
    /// game has no internal lag, which is a claim the probe has no evidence for.
    /// A latency tool that cannot say "I don't know" is worse than none.
    #[test]
    fn a_game_that_ignores_input_is_inconclusive_not_zero() {
        let rom = spin_rom();
        let anchor = warmed(&rom, 20);
        let mut scratch = Nes::from_rom(&rom).expect("fixture parses");

        let report = measure(&mut scratch, &anchor, LatencyConfig::default());
        assert_eq!(
            report.frames, None,
            "reported a lag it could not have measured"
        );
        assert_eq!(report.confidence, Confidence::Inconclusive);
        assert_eq!(report.reacting_buttons, 0);
        assert_eq!(
            report.probed_buttons,
            u32::try_from(PROBE_BUTTONS.len()).unwrap_or(u32::MAX)
        );
        assert_eq!(
            report.suggested_run_ahead(3),
            None,
            "an inconclusive report must not move the user's run-ahead setting"
        );
    }

    /// The complement: a ROM that DOES read the pad must produce a measurement.
    /// Without this, the honesty test above would also pass on a probe that
    /// always answers "inconclusive".
    #[test]
    fn a_polling_game_is_measured() {
        let rom = polling_rom();
        let anchor = warmed(&rom, 20);
        let mut scratch = Nes::from_rom(&rom).expect("fixture parses");

        let report = measure(&mut scratch, &anchor, LatencyConfig::default());
        assert!(
            report.frames.is_some(),
            "a continuously-polling ROM produced no measurement: {report:?}"
        );
        assert!(report.reacting_buttons > 0);
        assert_ne!(report.confidence, Confidence::Inconclusive);
        assert!(report.observable.is_some());
    }

    /// A divergence past `max_plausible_lag` is discarded: at that distance it is
    /// far likelier to be the game's own animation than a reaction to the pad.
    #[test]
    fn an_implausibly_late_divergence_is_not_a_measurement() {
        let per_button = [Some(40), Some(40), None, None, None, None];
        // `measure` filters before `conclude` sees them, so model that here.
        let filtered: Vec<Option<u32>> = per_button
            .iter()
            .map(|d| d.filter(|f| *f <= LatencyConfig::default().max_plausible_lag))
            .collect();
        assert!(conclude(&filtered, 6, Observable::Framebuffer).is_none());
    }

    /// Unanimity, majority and a bare plurality must be told apart — a plurality
    /// is not agreement, and must yield no number.
    #[test]
    fn agreement_is_graded_and_a_plurality_is_not_agreement() {
        let unanimous = [Some(2), Some(2), None, None, None, None];
        let r = conclude(&unanimous, 6, Observable::Framebuffer).expect("a verdict");
        assert_eq!(r.frames, Some(2));
        assert_eq!(r.confidence, Confidence::Unanimous);

        let majority = [Some(1), Some(1), Some(4), None, None, None];
        let r = conclude(&majority, 6, Observable::Framebuffer).expect("a verdict");
        assert_eq!(r.frames, Some(1));
        assert_eq!(r.confidence, Confidence::Majority);

        // Two against two: no majority, so no number.
        let split = [Some(1), Some(1), Some(4), Some(4), None, None];
        assert!(
            conclude(&split, 6, Observable::Framebuffer).is_none(),
            "a plurality was reported as a measurement"
        );
    }

    /// An even split is inconclusive at every size, not just at four buttons.
    ///
    /// One-vs-one is the case that looks most like "nearly agreed" and is the
    /// easiest to talk oneself into reporting. It carries exactly as much
    /// evidence as two-vs-two: none. The smallest-value tie-break inside
    /// `conclude` exists only to make candidate selection deterministic — it can
    /// never decide a *reported* number, because equal top votes and a majority
    /// are mutually exclusive.
    #[test]
    fn an_even_split_is_inconclusive_at_any_size() {
        let one_v_one = [Some(3), Some(1), None, None, None, None];
        assert!(
            conclude(&one_v_one, 6, Observable::Framebuffer).is_none(),
            "a 1-1 split was reported as a measurement"
        );
        let two_v_two = [Some(3), Some(3), Some(1), Some(1), None, None];
        assert!(conclude(&two_v_two, 6, Observable::Framebuffer).is_none());
    }

    /// The suggested depth is clamped, so a measurement cannot ask for more
    /// run-ahead than the caller is willing to afford.
    #[test]
    fn suggested_run_ahead_is_clamped() {
        let r = LatencyReport {
            frames: Some(7),
            confidence: Confidence::Unanimous,
            reacting_buttons: 6,
            probed_buttons: 6,
            observable: Some(Observable::Framebuffer),
            per_button: vec![Some(7); 6],
        };
        assert_eq!(r.suggested_run_ahead(3), Some(3));
        assert_eq!(r.suggested_run_ahead(0), Some(0));
    }
}
