//! v2.3.8 "Parallax" — the Divergence Lens: *where* two configurations differ.
//!
//! [`Probe`] already answers **whether** two configurations of the
//! same ROM diverge and **at which frame**: a trial reduces each frame to one
//! `u64`, and [`Probe::first_divergence`](crate::Probe::first_divergence) scans
//! the two sample vectors. That reduction is the right shape for detection and
//! the wrong shape for explanation — a hash says frame 412 differs and cannot
//! say which pixel, so it cannot hand anything to Pixel Provenance, which is
//! where an actual answer lives.
//!
//! This module closes that gap and nothing more: given a detected frame, it
//! re-runs both configurations to exactly that frame, keeps the **full** output
//! rather than its hash, and reports the shape of the difference.
//!
//! # Why the index framebuffer rather than the RGBA one
//!
//! [`Nes::index_framebuffer`] is 256x240 `u16`s, each `(emphasis << 6) | colour`
//! — the PPU's own per-pixel output, before the palette lookup that produces
//! RGBA. Localising on it is both cheaper (2 bytes per pixel rather than 4) and
//! at least as sensitive, because the RGBA buffer is a pure function of this one
//! **given the same palette**.
//!
//! That proviso is stated rather than assumed. It holds here because both trials
//! run on the same `Nes` instance, so they share whatever palette — stock,
//! generated, or user-edited — is loaded. It would NOT hold when comparing two
//! separate instances configured differently, and a future two-instance lens
//! must revisit this rather than inherit it.
//!
//! # What this deliberately does not do
//!
//! It does not localise **within** a frame. Frame `N` is where the two differ by
//! the end of the frame; the PPU rendered it over 89,342 cycles, and narrowing
//! to the cycle is a separate mechanism with its own cost argument. See
//! `to-dos/plans/v2.3.8-parallax-plan.md` item B.

use rustynes_core::Nes;
use rustynes_core::rustynes_ppu::{FRAMEBUFFER_PIXELS, SCREEN_WIDTH};

use crate::{Budget, Observable, Probe};
use rustynes_core::Buttons;

/// Where, within one frame, two configurations' output differs.
///
/// Never empty: a value of this type means at least one pixel differs, so
/// "they agree" is [`Localisation::Identical`] rather than a count of zero.
/// Making the empty case unrepresentable is deliberate — a zero-population
/// divergence and an agreement are the same fact, and offering two encodings of
/// it invites a caller to test the wrong one.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PixelDivergence {
    /// Zero-based frame index, as returned by
    /// [`Probe::first_divergence`](crate::Probe::first_divergence).
    pub frame: u32,
    /// How many of the 61,440 pixels differ.
    ///
    /// The cheapest signal that separates kinds of bug: one pixel is a sprite or
    /// a palette entry, 256 in a row is a scanline, tens of thousands is a
    /// scroll or a mode change. A caller that only wants somewhere to point uses
    /// [`Self::first`]; a caller asking *what kind of difference is this* wants
    /// this and [`Self::bounding_box`].
    pub count: u32,
    /// First differing pixel in raster order, as `(x, y)`.
    pub first: (u16, u16),
    /// Inclusive bounding box of every differing pixel, as
    /// `(min_x, min_y, max_x, max_y)`.
    pub bbox: (u16, u16, u16, u16),
}

impl PixelDivergence {
    /// Inclusive bounding box, `(min_x, min_y, max_x, max_y)`.
    #[must_use]
    pub const fn bounding_box(&self) -> (u16, u16, u16, u16) {
        self.bbox
    }

    /// Whether every differing pixel lies on one scanline.
    ///
    /// Offered because it is the distinction a caller actually acts on — a
    /// single-scanline difference points at mid-frame register timing, a
    /// multi-scanline one does not — and computing it from the bounding box at
    /// each call site would invite getting the inclusive comparison wrong.
    #[must_use]
    pub const fn is_single_scanline(&self) -> bool {
        self.bbox.1 == self.bbox.3
    }
}

/// The Lens's answer, with "I could not tell" kept distinct from "they agree".
///
/// The Latency Oracle's precedent applies directly: `None` and `Some(0)` are
/// different answers and were never collapsed there, so an exhausted budget must
/// not arrive here wearing the same shape as a genuine agreement. A tool that
/// reports "no divergence" when it merely stopped looking is worse than one that
/// reports nothing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Localisation {
    /// Both configurations produced identical output over every frame compared.
    Identical,
    /// They differ, at this frame and these pixels.
    Differs(PixelDivergence),
    /// The trial budget ran out, or the two trials could not be compared.
    ///
    /// **Not** a synonym for [`Self::Identical`].
    Inconclusive,
}

/// Compare two index framebuffers and describe the difference.
///
/// `None` means byte-identical. Pure and emulator-free, so both branches can be
/// tested without a ROM — which matters because the "they agree" branch is the
/// one a Lens that always reports divergence would still pass.
///
/// Frames of differing or unexpected length yield `None` rather than a partial
/// comparison: a truncated buffer is a caller error, and reporting the pixels
/// that happen to line up would be a confident answer to a question that was not
/// asked.
#[must_use]
pub fn diff_index_frames(frame: u32, a: &[u16], b: &[u16]) -> Option<PixelDivergence> {
    if a.len() != b.len() || a.len() != FRAMEBUFFER_PIXELS {
        return None;
    }
    let mut count: u32 = 0;
    let mut first: Option<(u16, u16)> = None;
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (u16::MAX, u16::MAX, 0u16, 0u16);

    for (i, (pa, pb)) in a.iter().zip(b.iter()).enumerate() {
        if pa == pb {
            continue;
        }
        // `SCREEN_W` is 256 and the buffer is exactly `SCREEN_W * SCREEN_H`, so
        // both fit `u16` by construction; the casts cannot truncate.
        #[expect(
            clippy::cast_possible_truncation,
            reason = "i < FRAMEBUFFER_PIXELS = 61,440, so both coordinates fit u16"
        )]
        let (x, y) = ((i % SCREEN_WIDTH) as u16, (i / SCREEN_WIDTH) as u16);
        count += 1;
        if first.is_none() {
            first = Some((x, y));
        }
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x);
        max_y = max_y.max(y);
    }

    first.map(|first| PixelDivergence {
        frame,
        count,
        first,
        bbox: (min_x, min_y, max_x, max_y),
    })
}

/// Detect a divergence between a baseline and a perturbed configuration, then
/// localise it to pixels.
///
/// Four trials against `probe`'s budget: two to detect the frame, two more to
/// re-run to it and keep the full output. That is the "detect cheap, localise
/// expensive, and only once" split — retaining two 61,440-pixel frames for every
/// frame of a long trial is exactly the memory blow-up the `u64` reduction
/// exists to avoid.
///
/// `setup` defines the perturbed configuration and runs after the anchor is
/// restored and before any frame, so whatever it changes is the only difference
/// between the two — the same contract, and the same licence to attribute a
/// difference to it, that
/// [`Probe::run_perturbed`](crate::Probe::run_perturbed) documents.
///
/// # Panics
///
/// Panics if `nes` is running a different ROM than `probe`'s anchor, via the
/// assertion in the engine's trial path.
pub fn localise<F, S>(
    probe: &mut Probe,
    nes: &mut Nes,
    frames: u32,
    mut setup: S,
    mut input: F,
) -> Localisation
where
    F: FnMut(u32) -> (Buttons, Buttons),
    S: FnMut(&mut Nes),
{
    // All four trials must be funded, or none of them is meaningful. Checked up
    // front rather than discovered halfway, mirroring `atlas::verify_liveness`:
    // spending two trials on detection and then finding the localisation pair
    // unaffordable returns `Inconclusive` having consumed the budget that would
    // have answered the question on a second attempt.
    if probe.trials_remaining() < LOCALISE_TRIALS {
        return Localisation::Inconclusive;
    }

    // Detect. `IndexFramebuffer` rather than `Framebuffer` so detection and
    // localisation look at the same thing: a divergence the detector found in
    // the RGBA hash but that the index diff cannot see would be a palette-only
    // difference, and reporting "frame 412 differs, zero pixels differ" is the
    // kind of self-contradiction this crate exists to avoid.
    let Some(base) = probe.run(nes, frames, Observable::IndexFramebuffer, &mut input) else {
        return Localisation::Inconclusive;
    };
    let Some(variant) = probe.run_perturbed(
        nes,
        frames,
        Observable::IndexFramebuffer,
        &mut setup,
        &mut input,
    ) else {
        return Localisation::Inconclusive;
    };

    // A short vector means a trial hit `max_frames_per_trial`, and comparing
    // only the common prefix — which `first_divergence` does — is right for
    // detection but leaves "they agreed as far as we looked" indistinguishable
    // from "they agreed". Say so rather than resolve it.
    if base.len() != frames as usize || variant.len() != frames as usize {
        return Localisation::Inconclusive;
    }

    let Some(n) = Probe::first_divergence(&base, &variant) else {
        return Localisation::Identical;
    };

    // Localise. Re-run to exactly frame `n`, inclusive, so the emulator is left
    // holding the first frame that differs.
    //
    // Reading the frame off `nes` after the trial relies on a trial NOT
    // restoring the anchor on its way out — it restores on the way in. That is
    // true and pinned by `a_trial_leaves_the_emulator_at_its_end_state`, which
    // uses a fixture whose output genuinely varies frame to frame; the obvious
    // fixture renders a blank screen, where "left at the anchor" and "left at
    // the end" are byte-identical and the test cannot fail.
    let to = n + 1;
    let Some(_) = probe.run(nes, to, Observable::IndexFramebuffer, &mut input) else {
        return Localisation::Inconclusive;
    };
    let frame_a = nes.index_framebuffer().to_vec();
    let Some(_) = probe.run_perturbed(
        nes,
        to,
        Observable::IndexFramebuffer,
        &mut setup,
        &mut input,
    ) else {
        return Localisation::Inconclusive;
    };
    let frame_b = nes.index_framebuffer();

    diff_index_frames(n, &frame_a, frame_b).map_or(
        // Detection said frame `n` differs and the pixel diff found nothing.
        // Rather than pick one, report neither: the two measurements disagree,
        // and that is a fact about this run, not an answer about the ROM.
        Localisation::Inconclusive,
        Localisation::Differs,
    )
}

/// Trials [`localise`] spends: two to detect, two to localise.
///
/// Exported so a caller can size a [`Budget`] against the real cost instead of
/// guessing, the same way `latency::budget_for` does. A ceiling with slack in it
/// is not a ceiling.
pub const LOCALISE_TRIALS: u32 = 4;

/// A [`Budget`] that exactly admits one [`localise`] call over `frames` frames.
#[must_use]
pub const fn budget_for(frames: u32) -> Budget {
    Budget {
        max_frames_per_trial: frames,
        max_trials: LOCALISE_TRIALS,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blank() -> Vec<u16> {
        vec![0u16; FRAMEBUFFER_PIXELS]
    }

    /// The branch a Lens that always reports divergence would still pass.
    /// Required as its own fixture for exactly that reason.
    #[test]
    fn identical_frames_are_not_a_divergence() {
        assert_eq!(diff_index_frames(7, &blank(), &blank()), None);
    }

    #[test]
    fn one_differing_pixel_is_located_exactly() {
        let a = blank();
        let mut b = blank();
        // (x, y) = (3, 2) => index 2 * 256 + 3.
        b[2 * SCREEN_WIDTH + 3] = 0x2A;
        let d = diff_index_frames(11, &a, &b).expect("they differ");
        assert_eq!(d.frame, 11);
        assert_eq!(d.count, 1);
        assert_eq!(d.first, (3, 2));
        assert_eq!(d.bounding_box(), (3, 2, 3, 2));
        assert!(d.is_single_scanline());
    }

    /// A whole scanline and a single pixel are different bugs, and `count` plus
    /// the bounding box is what separates them.
    #[test]
    fn a_full_scanline_reports_its_shape() {
        let a = blank();
        let mut b = blank();
        for x in 0..SCREEN_WIDTH {
            b[100 * SCREEN_WIDTH + x] = 1;
        }
        let d = diff_index_frames(0, &a, &b).expect("they differ");
        assert_eq!(d.count, 256);
        assert_eq!(d.first, (0, 100));
        assert_eq!(d.bounding_box(), (0, 100, 255, 100));
        assert!(d.is_single_scanline(), "one scanline, inclusive bounds");
    }

    /// Two separated pixels: the bounding box must span both, and the population
    /// count must NOT be inferred from the box — a 2-pixel difference inside a
    /// large box is a different fact from a filled rectangle.
    #[test]
    fn the_bounding_box_spans_and_the_count_does_not_fill_it() {
        let a = blank();
        let mut b = blank();
        b[10 * SCREEN_WIDTH + 5] = 1;
        b[200 * SCREEN_WIDTH + 250] = 1;
        let d = diff_index_frames(0, &a, &b).expect("they differ");
        assert_eq!(d.count, 2);
        assert_eq!(d.first, (5, 10));
        assert_eq!(d.bounding_box(), (5, 10, 250, 200));
        assert!(!d.is_single_scanline());
    }

    /// An NROM whose emitted pixels depend on work RAM, so poking one byte in a
    /// `setup` closure genuinely changes the screen.
    ///
    /// ```text
    /// $C000  LDA $0000     ; the perturbable byte
    /// $C003  STA $2001     ; drive PPUMASK emphasis from it
    /// $C006  INC $0000     ; keep it moving, so the screen varies frame to frame
    /// $C009  JMP $C000
    /// ```
    fn wram_driven_nrom() -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"NES\x1A");
        bytes.push(1);
        bytes.push(1);
        bytes.push(0);
        bytes.push(0);
        bytes.extend_from_slice(&[0u8; 8]);
        let mut prg = vec![0u8; 16 * 1024];
        prg[0..12].copy_from_slice(&[
            0xAD, 0x00, 0x00, // LDA $0000
            0x8D, 0x01, 0x20, // STA $2001
            0xEE, 0x00, 0x00, // INC $0000
            0x4C, 0x00, 0xC0, // JMP $C000
        ]);
        let len = prg.len();
        prg[len - 6] = 0x00;
        prg[len - 5] = 0xC0;
        prg[len - 4] = 0x00;
        prg[len - 3] = 0xC0;
        prg[len - 2] = 0x00;
        prg[len - 1] = 0xC0;
        bytes.extend_from_slice(&prg);
        bytes.extend_from_slice(&vec![0u8; 8 * 1024]);
        bytes
    }

    fn warmed() -> Nes {
        let mut n = Nes::from_rom(&wram_driven_nrom()).expect("fixture parses");
        for _ in 0..4 {
            n.run_frame();
        }
        n
    }

    fn idle(_: u32) -> (Buttons, Buttons) {
        (Buttons::empty(), Buttons::empty())
    }

    #[test]
    fn a_perturbation_that_changes_the_screen_is_located() {
        let mut n = warmed();
        let mut probe = Probe::anchor(&n, budget_for(12));

        let got = localise(&mut probe, &mut n, 12, |m| m.wram_mut()[0] ^= 0xFF, idle);

        let Localisation::Differs(d) = got else {
            panic!("expected a located divergence, got {got:?}");
        };
        assert!(d.count > 0, "a divergence with no differing pixels");
        assert!(
            d.first.0 < 256 && d.first.1 < 240,
            "located pixel {:?} is off-screen",
            d.first
        );
        let (min_x, min_y, max_x, max_y) = d.bounding_box();
        assert!(min_x <= d.first.0 && d.first.0 <= max_x);
        assert!(min_y <= d.first.1 && d.first.1 <= max_y);
    }

    /// The Lens's claim must match what the engine independently produces at the
    /// same frame — the whole record, not a plausibility check on part of it.
    ///
    /// Added after a mutation exposed the weakness: replacing the baseline frame
    /// with all zeros left `a_perturbation_that_changes_the_screen_is_located`
    /// passing, because "count > 0, on-screen, inside its own bounding box" is
    /// true of a comparison against garbage. Asserting `count` and `first` and
    /// the box against an independently replayed pair is what makes the frames
    /// the Lens actually read part of the contract.
    ///
    /// This is not a reimplementation of the Lens: it drives `Probe`'s own trial
    /// primitives, which is what the Lens composes, and then calls the pure diff.
    /// The thing under test is the composition.
    #[test]
    fn the_located_pixels_match_an_independent_replay() {
        let perturb = |m: &mut Nes| m.wram_mut()[0] ^= 0xFF;
        let mut n = warmed();
        let mut probe = Probe::anchor(
            &n,
            Budget {
                max_frames_per_trial: 12,
                max_trials: LOCALISE_TRIALS + 2,
            },
        );

        let got = localise(&mut probe, &mut n, 12, perturb, idle);
        let Localisation::Differs(claimed) = got else {
            panic!("expected a located divergence, got {got:?}");
        };

        let to = claimed.frame + 1;
        probe
            .run(&mut n, to, Observable::IndexFramebuffer, idle)
            .expect("within budget");
        let a = n.index_framebuffer().to_vec();
        probe
            .run_perturbed(&mut n, to, Observable::IndexFramebuffer, perturb, idle)
            .expect("within budget");
        let b = n.index_framebuffer().to_vec();

        let truth = diff_index_frames(claimed.frame, &a, &b)
            .expect("the frame the Lens named must actually differ");
        assert_eq!(
            claimed, truth,
            "the Lens reported a divergence that an independent replay of the \
             same two configurations does not produce"
        );
    }

    /// The control. Without it, a Lens that reported a divergence unconditionally
    /// would pass the test above and ship.
    #[test]
    fn no_perturbation_is_identical_not_a_divergence() {
        let mut n = warmed();
        let mut probe = Probe::anchor(&n, budget_for(12));

        let got = localise(&mut probe, &mut n, 12, |_| {}, idle);

        assert_eq!(
            got,
            Localisation::Identical,
            "two configurations differing in nothing must agree; anything else \
             means the engine is not replaying deterministically"
        );
    }

    /// An exhausted budget is "I stopped looking", never "they agree".
    #[test]
    fn an_underfunded_budget_is_inconclusive_not_identical() {
        let mut n = warmed();
        let mut probe = Probe::anchor(
            &n,
            Budget {
                max_frames_per_trial: 12,
                max_trials: LOCALISE_TRIALS - 1,
            },
        );

        let got = localise(&mut probe, &mut n, 12, |m| m.wram_mut()[0] ^= 0xFF, idle);

        assert_eq!(got, Localisation::Inconclusive);
        assert_eq!(
            probe.trials_remaining(),
            LOCALISE_TRIALS - 1,
            "an up-front budget check must not spend trials before declining"
        );
    }

    /// A truncated buffer is a caller error, and comparing the pixels that
    /// happen to line up would answer a question nobody asked.
    #[test]
    fn mismatched_or_wrong_length_frames_decline() {
        let a = blank();
        let short = vec![0u16; FRAMEBUFFER_PIXELS - 1];
        assert_eq!(diff_index_frames(0, &a, &short), None);
        assert_eq!(diff_index_frames(0, &short, &a), None);
        assert_eq!(diff_index_frames(0, &short, &short), None);
    }
}
