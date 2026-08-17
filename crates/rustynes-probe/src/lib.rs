//! Deterministic re-simulation probing: **anchor, replay under variation,
//! locate the first divergence**.
//!
//! # What this is for
//!
//! Several questions about a running game have the same shape:
//!
//! - *How many frames of input lag does this game have?* — replay from one
//!   anchor with a button held and with it never pressed, and see which frame
//!   first differs. That index **is** the game's internal lag.
//! - *What is this RAM byte for?* — replay with the byte perturbed and see what
//!   changes.
//! - *Where do these two configurations disagree?* — replay both and find the
//!   first frame that differs.
//!
//! Each is "take a snapshot, re-simulate under controlled variation, find the
//! first observable difference". This crate is that primitive, written once.
//!
//! # Why `RustyNES` can do this and most emulators cannot
//!
//! The answers are only meaningful if a replay from the same anchor with the
//! same inputs produces the same frames *every time*. That is `RustyNES`'s
//! determinism contract (`docs/testing-strategy.md`), a hard guarantee rather
//! than an aspiration — the same property save-states, TAS replay, and netplay
//! rollback already rely on. A probe result is therefore a property of the ROM,
//! not of the run, and [`Probe::run`] re-asserts it rather than assuming it.
//!
//! # What it deliberately does not do
//!
//! It does not own a [`Nes`]. The caller passes a scratch instance in, so a
//! frontend can reuse one across probes and keep the live emulator untouched.
//! It does not spawn threads, and it does not interpret results — deciding that
//! "frame 1 differed, therefore the lag is 1 frame" belongs to the tool, which
//! knows what it asked.
//!
//! # Example
//!
//! ```no_run
//! use rustynes_core::{Buttons, Nes};
//! use rustynes_probe::{Budget, Observable, Probe};
//!
//! # fn demo(live: &Nes, scratch: &mut Nes) {
//! let mut probe = Probe::anchor(live, Budget::default());
//!
//! // Trial A: hold Right from the first frame. Trial B: never press anything.
//! // `run` is budgeted: `None` means the trial ceiling is spent.
//! let held = probe.run(scratch, 16, Observable::Framebuffer, |_| {
//!     (Buttons::RIGHT, Buttons::empty())
//! }).expect("within budget");
//! let idle = probe.run(scratch, 16, Observable::Framebuffer, |_| {
//!     (Buttons::empty(), Buttons::empty())
//! }).expect("within budget");
//!
//! match Probe::first_divergence(&held, &idle) {
//!     Some(frame) => println!("reacted on frame {frame}"),
//!     None => println!("no reaction inside the budget"),
//! }
//! # }
//! ```

pub mod atlas;
pub mod latency;

use rustynes_core::{Buttons, Nes, ROM_HASH_TAG_LEN};

/// Bounds on what a single probe may spend.
///
/// A probe runs the emulator many times over, from a UI thread in the frontend's
/// case, so it needs a ceiling that does not depend on the game cooperating. A
/// game that never reacts must make the probe *stop and say so* rather than run
/// until something else notices.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Budget {
    /// Hard cap on frames simulated per trial. Reached, [`Probe::run`] returns
    /// the samples it has; the caller sees a short vector and must treat that as
    /// "inconclusive", never as "no divergence".
    pub max_frames_per_trial: u32,
    /// Hard cap on trials a caller may run against one anchor. Enforced by
    /// [`Probe::trials_remaining`]; the engine cannot enforce it alone because
    /// it does not drive the loop.
    pub max_trials: u32,
}

impl Default for Budget {
    fn default() -> Self {
        Self {
            // ~2 s of NTSC. Long enough for any input-lag question (games react
            // within a handful of frames) and short enough that a probe cannot
            // stall a UI frame budget for a noticeable time.
            max_frames_per_trial: 120,
            max_trials: 64,
        }
    }
}

/// What a trial observes at the end of each frame.
///
/// Every variant reduces its observation to one `u64` so trials compare cheaply
/// and a divergence search is a linear scan of two slices. Hashing loses the
/// ability to say *how* two frames differ — deliberately: this engine answers
/// *when*, and the caller that wants *what* already has Pixel Provenance and the
/// debugger for that.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Observable {
    /// The RGBA framebuffer. The default question — "did the screen change?".
    Framebuffer,
    /// The palette-index framebuffer. Cheaper than [`Self::Framebuffer`] and
    /// immune to a palette or filter change that leaves the rendered indices
    /// identical.
    IndexFramebuffer,
    /// The 2 KiB work RAM. Catches a game that reacted internally without
    /// drawing anything yet — a menu highlight committed to a variable one frame
    /// before it is rendered, for instance.
    Wram,
    /// Coarse energy of the audio produced this frame. The fallback for a
    /// reaction that is audible before it is visible; quantised, because exact
    /// float equality across a resample would be noise, not signal.
    AudioEnergy,
}

/// An anchor plus the budget probes taken from it must respect.
///
/// Cloning is cheap relative to re-deriving the state, but the snapshot is a
/// real allocation — hold one anchor and run many trials against it rather than
/// re-anchoring per trial.
#[derive(Clone, Debug)]
pub struct Probe {
    snapshot: Vec<u8>,
    rom_tag: [u8; ROM_HASH_TAG_LEN],
    budget: Budget,
    trials_used: u32,
}

impl Probe {
    /// Capture the anchor: the emulator's full state plus the identity of the
    /// ROM it is running.
    ///
    /// The ROM tag is recorded so [`Self::run`] can refuse to replay into an
    /// instance running a different game — restoring a snapshot across ROMs
    /// would produce confident nonsense rather than an error.
    #[must_use]
    pub fn anchor(nes: &Nes, budget: Budget) -> Self {
        Self {
            snapshot: nes.snapshot(),
            rom_tag: nes.rom_hash_tag(),
            budget,
            trials_used: 0,
        }
    }

    /// Trials still allowed against this anchor under its [`Budget`].
    #[must_use]
    pub const fn trials_remaining(&self) -> u32 {
        self.budget.max_trials.saturating_sub(self.trials_used)
    }

    /// Trials spent through [`Self::run`] so far.
    ///
    /// Reported so a caller can say how much work a measurement cost, and so a
    /// test can assert that a budget is actually *binding* rather than merely
    /// declared — the two are easy to confuse, and a test that cannot tell them
    /// apart is decoration.
    #[must_use]
    pub const fn trials_used(&self) -> u32 {
        self.trials_used
    }

    /// The budget this anchor was taken under.
    #[must_use]
    pub const fn budget(&self) -> Budget {
        self.budget
    }

    /// Restore the anchor into `nes` and run `frames` frames, sampling
    /// `observable` after each one.
    ///
    /// `input` is called once per frame with the zero-based frame index and
    /// returns the buttons for controller 1 and 2. It is the only thing that
    /// varies between trials; everything else is re-derived from the anchor,
    /// which is what makes two trials comparable.
    ///
    /// Returns one sample per frame actually run. A vector shorter than `frames`
    /// means the budget stopped it — treat that as **inconclusive**, not as
    /// evidence of no divergence.
    ///
    /// # Panics
    ///
    /// Panics if `nes` is running a different ROM than the anchor was taken
    /// from, or if the anchor fails to restore. Both are caller errors that
    /// would otherwise yield a plausible, wrong answer, and this engine exists
    /// to produce answers people will act on.
    fn run_uncounted<F, S>(
        &self,
        nes: &mut Nes,
        frames: u32,
        observable: Observable,
        setup: S,
        mut input: F,
    ) -> Vec<u64>
    where
        F: FnMut(u32) -> (Buttons, Buttons),
        S: FnOnce(&mut Nes),
    {
        assert_eq!(
            nes.rom_hash_tag(),
            self.rom_tag,
            "probe anchor belongs to a different ROM than the emulator it was \
             replayed into; restoring across ROMs yields confident nonsense"
        );
        // `restore_quiet`, NOT `restore`. The loud variant additionally clears
        // the rewind ring, on the correct reasoning that a state loaded from
        // elsewhere is unrelated to what was buffered. That reasoning does not
        // hold for a probe: the anchor was snapshotted from this same timeline,
        // and a trial ends by putting it back.
        //
        // This was a real, user-visible bug. `latency::measure_in_place` runs up
        // to 21 trials against the LIVE emulator, so asking "how much input lag
        // does this game have?" silently destroyed the user's rewind history
        // twenty-one times over. PR #385 review caught the symptom and the fix
        // there changed only `measure_in_place`'s FINAL restore — which left
        // every trial's restore, the actual source, untouched. Fixed here at the
        // one site all trials share.
        nes.restore_quiet(&self.snapshot)
            .expect("probe anchor round-trips: it came from Nes::snapshot");

        // A trial's frames are re-simulated: they never happened on the user's
        // timeline. Letting them into the rewind ring would let the user rewind
        // *into a measurement* — the same reason run-ahead suppresses capture
        // around its hidden frames. Suppressed for the trial and restored to
        // whatever the caller had, not to an assumed `true`, so a caller who had
        // deliberately disabled capture does not get it switched back on.
        //
        // Held by a guard rather than restored at the end of the function, so an
        // unwinding panic anywhere in the trial cannot leave capture switched off
        // on a `Nes` the caller keeps using. That failure would be silent and
        // durable: rewind would simply stop recording, with nothing to indicate
        // why. (PR #392 review.)
        let guard = CaptureGuard::suppress(nes);

        // The perturbation, if any: applied to the freshly-restored anchor before
        // frame 0, so it is the ONLY difference between this trial and a
        // baseline one. That is what lets a divergence be attributed to it.
        setup(guard.nes);

        let n = frames.min(self.budget.max_frames_per_trial);
        let mut samples = Vec::with_capacity(n as usize);
        // Generously sized so one frame always fits: an NTSC frame at 192 kHz is
        // ~3,200 samples. Allocated once per trial, not per frame, and reused as
        // the drain target via `drain_audio_into`.
        let mut audio = vec![0.0f32; 8192];
        for f in 0..n {
            let (p1, p2) = input(f);
            guard.nes.set_buttons(0, p1);
            guard.nes.set_buttons(1, p2);
            guard.nes.run_frame();
            // Drain EVERY frame, whatever the observable. `Nes::restore` does
            // drop the blip's pending queue (verified by
            // `tests/restore_audio_pin.rs`), so trials cannot contaminate each
            // other through it — but draining only for `AudioEnergy` made that
            // safety depend on restore's audio semantics staying as they are,
            // and let a 120-frame framebuffer trial pile up ~88k samples for
            // nothing. Raised in review on #384.
            //
            // v2.3.6: this was `audio.clear()` and nothing else — the buffer was
            // emptied and never filled, so `sample` summed an EMPTY slice and
            // `Observable::AudioEnergy` reported zero energy on every frame of
            // every trial. Two trials therefore always agreed under that
            // observable, which made the audio lens incapable of detecting any
            // divergence: `latency`'s audio fallback silently never fired, and the
            // RAM Atlas's audio lens would have reported EVERY address `Inert` —
            // a confident wrong verdict, from a comment that said "drain" beside
            // code that did not. Caught in review on PR #392.
            // Samples actually written by this frame's drain. `audio` keeps its
            // full capacity, so `sample` gets only the populated prefix — passing
            // the whole buffer would mix this frame's samples with stale trailing
            // zeros and make the energy depend on buffer length rather than audio.
            let audio_len = guard.nes.drain_audio_into(&mut audio);

            samples.push(sample(guard.nes, observable, &audio[..audio_len]));
        }
        // `guard` restores the caller's capture setting as it drops here.
        samples
    }

    /// Restore the anchor and run one **budgeted** trial.
    ///
    /// Returns `None` once [`Self::trials_remaining`] reaches zero, so a search
    /// loop terminates on the budget rather than on the caller remembering to
    /// check.
    ///
    /// This is the only public way to run a trial, deliberately. An earlier
    /// version also exposed an uncounted `run`, which made the unbudgeted choice
    /// the convenient default — and `latency::measure` duly used it, so the
    /// budget was computed, documented and never enforced. Raised in review on
    /// #384; the uncounted path is now private.
    pub fn run<F>(
        &mut self,
        nes: &mut Nes,
        frames: u32,
        observable: Observable,
        input: F,
    ) -> Option<Vec<u64>>
    where
        F: FnMut(u32) -> (Buttons, Buttons),
    {
        if self.trials_remaining() == 0 {
            return None;
        }
        self.trials_used += 1;
        Some(self.run_uncounted(nes, frames, observable, |_| {}, input))
    }

    /// Restore the anchor, apply `setup`, then run one **budgeted** trial.
    ///
    /// `setup` sees the emulator with the anchor freshly restored and before any
    /// frame has run, so whatever it changes is the only difference between this
    /// trial and a baseline [`Self::run`]. That is precisely what licenses
    /// attributing a divergence to it — the RAM Atlas pokes one work-RAM byte
    /// here and asks whether the screen then differs.
    ///
    /// Counts against the same trial budget as [`Self::run`]: a perturbation
    /// sweep is the easiest way to spend an unbounded number of trials, so it
    /// must not have a cheaper path to the emulator than anything else.
    pub fn run_perturbed<F, S>(
        &mut self,
        nes: &mut Nes,
        frames: u32,
        observable: Observable,
        setup: S,
        input: F,
    ) -> Option<Vec<u64>>
    where
        F: FnMut(u32) -> (Buttons, Buttons),
        S: FnOnce(&mut Nes),
    {
        if self.trials_remaining() == 0 {
            return None;
        }
        self.trials_used += 1;
        Some(self.run_uncounted(nes, frames, observable, setup, input))
    }

    /// The first frame index at which two trials differ, or `None` if they agree
    /// over their common length.
    ///
    /// Comparing only the common prefix is deliberate: a shorter trial means its
    /// budget ran out, and "one ran longer" is not a divergence.
    #[must_use]
    pub fn first_divergence(a: &[u64], b: &[u64]) -> Option<u32> {
        a.iter()
            .zip(b.iter())
            .position(|(x, y)| x != y)
            .and_then(|i| u32::try_from(i).ok())
    }

    /// Whether two trials agree over their whole common prefix, and that prefix
    /// is non-empty.
    ///
    /// Distinct from `first_divergence(..).is_none()`, which is also true for
    /// two empty trials — a case that means "nothing ran", not "they agree".
    #[must_use]
    pub fn agree(a: &[u64], b: &[u64]) -> bool {
        let common = a.len().min(b.len());
        common > 0 && Self::first_divergence(a, b).is_none()
    }
}

/// Suppresses rewind capture for the lifetime of a trial and restores the
/// caller's setting on drop — including on an unwinding panic.
///
/// A plain save-and-restore around the trial body is correct on the happy path
/// and wrong on the unwind: a panic inside `Nes::run_frame` or the perturbation
/// closure would skip the restore and leave capture switched off on a `Nes` the
/// caller keeps using. Rewind would then stop recording silently, with nothing
/// to indicate why.
///
/// Worth contrasting with the `Drop` guard declined for
/// `latency::measure_in_place` on PR #385, because the reasoning genuinely
/// differs rather than being applied inconsistently. There, the guard would have
/// had to restore a snapshot — a fallible operation — and `Drop` cannot return a
/// `Result`, so it would have reintroduced the silent failure that review had
/// just asked to remove. Here the restored value is a `bool` and the operation is
/// infallible, so the guard has no downside at all. And the release profile's
/// `panic = "abort"` argument does not rescue this case either: in a build that
/// does unwind, this flag outlives the panic, whereas an advanced timeline in a
/// dying process does not.
pub(crate) struct CaptureGuard<'a> {
    pub(crate) nes: &'a mut Nes,
    restore_to: bool,
}

impl<'a> CaptureGuard<'a> {
    pub(crate) const fn suppress(nes: &'a mut Nes) -> Self {
        // The caller's setting, not an assumed `true`: someone who deliberately
        // disabled capture must not have it switched back on by a probe.
        let restore_to = nes.rewind_capture_enabled();
        nes.set_rewind_capture(false);
        Self { nes, restore_to }
    }
}

impl Drop for CaptureGuard<'_> {
    fn drop(&mut self) {
        self.nes.set_rewind_capture(self.restore_to);
    }
}

/// Reduce the emulator's current state to one comparable value.
fn sample(nes: &Nes, observable: Observable, audio: &[f32]) -> u64 {
    match observable {
        Observable::Framebuffer => fnv1a64(nes.framebuffer()),
        Observable::IndexFramebuffer => {
            // The index framebuffer is `u16` per pixel; fold it through the same
            // byte hash so every variant shares one mixing function.
            let mut h = FNV_OFFSET;
            for px in nes.index_framebuffer() {
                h = fnv1a64_step(h, px.to_le_bytes().as_slice());
            }
            h
        }
        Observable::Wram => fnv1a64(nes.wram()),
        Observable::AudioEnergy => audio_energy(audio),
    }
}

/// Quantised sum of |amplitude| for one frame of drained audio.
///
/// Exact float equality across a resampled stream would compare noise; this asks
/// the coarser question the fallback is for — "did this frame make a meaningfully
/// different sound?".
///
/// Extracted from [`sample`] so its dependence on its input can be tested without
/// a ROM that makes noise. That matters because the trial loop once handed this an
/// always-empty slice, and a lens returning a constant is invisible to every test
/// that only compares trials to each other: constants always agree. (PR #392.)
fn audio_energy(audio: &[f32]) -> u64 {
    let energy: f32 = audio.iter().map(|s| s.abs()).sum();
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let q = (energy * 64.0) as u64;
    q
}

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

fn fnv1a64_step(mut hash: u64, bytes: &[u8]) -> u64 {
    for &b in bytes {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    fnv1a64_step(FNV_OFFSET, bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal NROM that spins forever, mirroring the core's `synth_nrom`
    /// fixture. Enough to exercise the engine's contract without a real game.
    fn synth_nrom() -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"NES\x1A");
        bytes.push(1); // 16 KiB PRG
        bytes.push(1); // 8 KiB CHR
        bytes.push(0);
        bytes.push(0);
        bytes.extend_from_slice(&[0u8; 8]);
        let mut prg = vec![0u8; 16 * 1024];
        prg[0] = 0x4C; // JMP $C000
        prg[1] = 0x00;
        prg[2] = 0xC0;
        let len = prg.len();
        prg[len - 6] = 0x00; // NMI
        prg[len - 5] = 0xC0;
        prg[len - 4] = 0x00; // RESET
        prg[len - 3] = 0xC0;
        prg[len - 2] = 0x00; // IRQ
        prg[len - 1] = 0xC0;
        bytes.extend_from_slice(&prg);
        bytes.extend_from_slice(&vec![0u8; 8 * 1024]);
        bytes
    }

    fn nes() -> Nes {
        Nes::from_rom(&synth_nrom()).expect("fixture parses")
    }

    fn idle(_: u32) -> (Buttons, Buttons) {
        (Buttons::empty(), Buttons::empty())
    }

    /// THE contract the whole engine rests on: two trials with identical inputs
    /// produce identical samples, frame for frame. If this fails, nothing else
    /// here means anything.
    #[test]
    fn identical_trials_are_identical() {
        let mut n = nes();
        for _ in 0..10 {
            n.run_frame();
        }
        let mut probe = Probe::anchor(&n, Budget::default());

        let a = probe
            .run(&mut n, 20, Observable::Framebuffer, idle)
            .expect("within budget");
        let b = probe
            .run(&mut n, 20, Observable::Framebuffer, idle)
            .expect("within budget");
        assert_eq!(a, b, "the determinism contract failed under replay");
        assert_eq!(Probe::first_divergence(&a, &b), None);
        assert!(Probe::agree(&a, &b));
    }

    /// The anchor must actually restore: a trial run after another trial has
    /// advanced the emulator has to produce the same samples as the first.
    /// Without the restore, trial 2 would start 20 frames later and differ.
    #[test]
    fn each_trial_restarts_from_the_anchor() {
        let mut n = nes();
        let mut probe = Probe::anchor(&n, Budget::default());
        let first = probe
            .run(&mut n, 8, Observable::Wram, idle)
            .expect("within budget");
        // Advance well past the anchor between trials.
        for _ in 0..50 {
            n.run_frame();
        }
        let second = probe
            .run(&mut n, 8, Observable::Wram, idle)
            .expect("within budget");
        assert_eq!(first, second, "the anchor did not restore between trials");
    }

    /// All four observables must be usable and self-consistent.
    #[test]
    fn every_observable_is_deterministic() {
        let mut n = nes();
        let mut probe = Probe::anchor(&n, Budget::default());
        for obs in [
            Observable::Framebuffer,
            Observable::IndexFramebuffer,
            Observable::Wram,
            Observable::AudioEnergy,
        ] {
            let a = probe.run(&mut n, 6, obs, idle).expect("within budget");
            let b = probe.run(&mut n, 6, obs, idle).expect("within budget");
            assert_eq!(a, b, "{obs:?} was not deterministic under replay");
            assert_eq!(a.len(), 6, "{obs:?} produced the wrong sample count");
        }
    }

    /// The budget must cap a trial, and the short result must be distinguishable
    /// from a completed one — the caller has to be able to tell "inconclusive"
    /// from "no divergence".
    #[test]
    fn budget_caps_the_trial_length() {
        let mut n = nes();
        let budget = Budget {
            max_frames_per_trial: 3,
            ..Budget::default()
        };
        let mut probe = Probe::anchor(&n, budget);
        let samples = probe
            .run(&mut n, 100, Observable::Framebuffer, idle)
            .expect("within budget");
        assert_eq!(samples.len(), 3, "budget did not cap the trial");
    }

    /// `run_counted` must stop handing out trials once the budget is spent,
    /// so a search loop terminates on the budget rather than on discipline.
    #[test]
    fn trial_budget_is_enforced_and_then_refuses() {
        let mut n = nes();
        let budget = Budget {
            max_trials: 2,
            ..Budget::default()
        };
        let mut probe = Probe::anchor(&n, budget);
        assert_eq!(probe.trials_remaining(), 2);
        assert!(probe.run(&mut n, 2, Observable::Wram, idle).is_some());
        assert!(probe.run(&mut n, 2, Observable::Wram, idle).is_some());
        assert_eq!(probe.trials_remaining(), 0);
        assert!(
            probe.run(&mut n, 2, Observable::Wram, idle).is_none(),
            "the engine handed out a trial past its budget"
        );
    }

    /// A divergence must be located at the exact frame it first appears, not
    /// merely detected. The fixture is synthetic so the answer is known: two
    /// sample streams that agree for three entries and then differ.
    #[test]
    fn first_divergence_reports_the_exact_frame() {
        let a = [1u64, 2, 3, 4, 5];
        let b = [1u64, 2, 3, 9, 5];
        assert_eq!(Probe::first_divergence(&a, &b), Some(3));
        assert!(!Probe::agree(&a, &b));
    }

    /// Two trials of different length agree if their common prefix does — a
    /// budget-truncated trial is not a divergence.
    #[test]
    fn a_shorter_trial_is_not_a_divergence() {
        let long = [1u64, 2, 3, 4];
        let short = [1u64, 2];
        assert_eq!(Probe::first_divergence(&long, &short), None);
        assert!(Probe::agree(&long, &short));
    }

    /// Two empty trials must NOT read as agreement: nothing ran, so there is
    /// nothing to agree about. This is the distinction that stops a probe
    /// reporting "no reaction" when it in fact never simulated anything.
    #[test]
    fn empty_trials_do_not_count_as_agreement() {
        assert_eq!(Probe::first_divergence(&[], &[]), None);
        assert!(
            !Probe::agree(&[], &[]),
            "empty trials must not report agreement"
        );
    }

    /// Input must actually reach the emulator: a trial that presses buttons and
    /// one that does not must differ in WRAM on a ROM that stores the pad.
    ///
    /// The spin-loop fixture never reads the controller, so this asserts the
    /// weaker but still meaningful property that the input closure is invoked
    /// once per frame with ascending indices — without which two "different"
    /// trials would be silently identical and every probe would answer "no
    /// reaction".
    #[test]
    fn the_input_closure_is_called_once_per_frame_in_order() {
        let mut n = nes();
        let mut probe = Probe::anchor(&n, Budget::default());
        let mut seen = Vec::new();
        let _ = probe.run(&mut n, 5, Observable::Wram, |f| {
            seen.push(f);
            (Buttons::empty(), Buttons::empty())
        });
        assert_eq!(seen, vec![0, 1, 2, 3, 4]);
    }

    /// A real state difference must propagate through the replay into a detected
    /// divergence.
    ///
    /// The tests above prove the comparator and the replay path separately; this
    /// closes the loop. Two anchors differing only in one work-RAM byte must
    /// produce sample streams that diverge at frame 0 under the `Wram`
    /// observable — which is the mechanism every consumer of this crate relies
    /// on, and the one a comparator test alone cannot demonstrate.
    ///
    /// The fixture ROM never reads the controller, so perturbing memory is the
    /// available way to introduce a genuine difference; the Latency Oracle will
    /// introduce its difference through input instead, on ROMs that do read it.
    #[test]
    fn a_real_state_difference_is_detected_end_to_end() {
        let mut n = nes();
        for _ in 0..10 {
            n.run_frame();
        }

        n.poke_ram(0x0200, 0x00);
        let mut probe_a = Probe::anchor(&n, Budget::default());
        let a = probe_a
            .run(&mut n, 4, Observable::Wram, idle)
            .expect("within budget");

        // Restore to the same point, change ONE byte, and re-anchor.
        probe_a
            .run(&mut n, 0, Observable::Wram, idle)
            .expect("within budget"); // restore only
        n.poke_ram(0x0200, 0xA5);
        let mut probe_b = Probe::anchor(&n, Budget::default());
        let b = probe_b
            .run(&mut n, 4, Observable::Wram, idle)
            .expect("within budget");

        assert_eq!(
            Probe::first_divergence(&a, &b),
            Some(0),
            "a one-byte work-RAM difference did not reach the observable"
        );
        assert!(!Probe::agree(&a, &b));
    }

    /// A trial must not destroy the caller's rewind history.
    ///
    /// `latency::measure_in_place` runs up to 21 trials against the **live**
    /// emulator, so a loud `restore` here means asking "how much input lag does
    /// this game have?" wipes the user's rewind buffer twenty-one times over.
    /// PR #385 review caught the symptom; the fix there changed only
    /// `measure_in_place`'s final restore and left every trial's restore — the
    /// actual source — untouched, so the bug survived a fix that claimed it.
    ///
    /// The ring must come back **exactly** as it was: neither cleared nor grown.
    /// Growth is its own defect — a trial's frames are re-simulated and never
    /// happened on the user's timeline, so rewinding into them would be rewinding
    /// into a measurement.
    ///
    /// Mutation-checked in both directions: swapping `restore_quiet` back to
    /// `restore` fails this at `rewind_len()` 0, and removing the
    /// `set_rewind_capture(false)` guard fails it at 10 against 8.
    #[test]
    fn a_trial_preserves_the_callers_rewind_ring() {
        let mut n = nes();
        n.enable_rewind();
        for _ in 0..4 {
            n.run_frame();
            n.rewind_capture();
        }
        let before = n.rewind_len();
        assert!(before > 0, "fixture failed to buffer any rewind frames");

        let mut probe = Probe::anchor(&n, Budget::default());
        let _ = probe
            .run(&mut n, 2, Observable::Wram, idle)
            .expect("within budget");

        assert_eq!(
            n.rewind_len(),
            before,
            "running a probe trial cleared the caller's rewind ring; a trial \
             restores its own anchor onto the same timeline and has no business \
             invalidating history the user recorded"
        );
    }

    /// An unwinding panic inside a trial must still restore the caller's rewind
    /// capture setting.
    ///
    /// Without the RAII guard this leaves capture switched OFF on a `Nes` the
    /// caller keeps using, and rewind then stops recording silently with nothing
    /// to indicate why. Raised in review on PR #392.
    ///
    /// The panic is injected through the perturbation closure, which is the one
    /// caller-supplied hook that runs inside the guarded region. Mutation-checked:
    /// replacing the guard with a save-and-restore around the trial body fails
    /// this test.
    #[test]
    fn a_panic_inside_a_trial_still_restores_rewind_capture() {
        let mut n = nes();
        assert!(
            n.rewind_capture_enabled(),
            "fixture starts with capture armed"
        );

        let mut probe = Probe::anchor(&n, Budget::default());
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = probe.run_perturbed(
                &mut n,
                4,
                Observable::Wram,
                |_| panic!("injected mid-trial failure"),
                idle,
            );
        }));
        assert!(result.is_err(), "the injected panic did not propagate");

        assert!(
            n.rewind_capture_enabled(),
            "a panic inside a trial left rewind capture disabled; the caller's \
             rewind would silently stop recording"
        );
    }

    /// The trial loop must actually DRAIN audio each frame.
    ///
    /// This is the wiring test, and it is the one that was missing. The sibling
    /// test below proves `audio_energy` responds to its input, but it calls that
    /// function directly — so it passes with the drain removed, which is exactly
    /// how the bug survived: core logic tested, plumbing not.
    ///
    /// Asserted through the emulator's own queue rather than through the samples:
    /// this fixture is silent, so drained audio is all zeros and hashes identically
    /// to an empty slice. What *is* observable is the residue. If the loop drains
    /// every frame, the queue is empty when the trial ends; if it never drains, the
    /// queue holds the whole trial's audio.
    ///
    /// Mutation-checked: removing the `drain_audio_into` call fails this with
    /// thousands of samples left pending.
    #[test]
    fn a_trial_drains_audio_every_frame() {
        let mut n = nes();
        let mut probe = Probe::anchor(&n, Budget::default());
        let _ = probe
            .run(&mut n, 8, Observable::Framebuffer, idle)
            .expect("within budget");

        let residue = n.drain_audio().len();
        assert_eq!(
            residue, 0,
            "a trial left {residue} audio samples pending; the per-frame drain \
             did not run, so `Observable::AudioEnergy` would see an empty slice \
             and report the same value on every frame of every trial"
        );
    }

    /// The audio observable's value must depend on the samples it is given.
    ///
    /// Direct on `sample`, because that is where the empty-slice bug lived and it
    /// can be proven without a ROM that makes noise.
    #[test]
    fn the_audio_observable_reflects_drained_samples() {
        let empty = super::audio_energy(&[]);
        let quiet = super::audio_energy(&[0.0, 0.0, 0.0]);
        let loud = super::audio_energy(&[0.4, -0.4, 0.4]);
        assert_ne!(
            loud, empty,
            "audio energy of a non-silent buffer matched an EMPTY buffer; the \
             observable cannot distinguish audio from a missing drain"
        );
        assert_ne!(loud, quiet, "audio energy did not respond to amplitude");
    }

    /// A caller's rewind configuration must not change what a trial MEASURES.
    ///
    /// This is the question that decides whether the rewind fix above
    /// invalidated earlier results. Before it, every trial ran with capture
    /// enabled and the ring being cleared; after it, capture is suppressed and
    /// the ring is left alone. If either had perturbed emulation, samples taken
    /// under the two regimes would differ — and every measurement predating the
    /// fix would need re-running.
    ///
    /// One anchor, two trials, rewind armed between them: the probe restores the
    /// same emulation state both times, so any difference in the sample vectors
    /// is attributable to the rewind machinery alone.
    #[test]
    fn a_trials_samples_are_independent_of_the_callers_rewind_state() {
        let mut n = nes();
        let mut probe = Probe::anchor(&n, Budget::default());

        let without = probe
            .run(&mut n, 8, Observable::Framebuffer, idle)
            .expect("within budget");

        n.enable_rewind();
        for _ in 0..3 {
            n.run_frame();
            n.rewind_capture();
        }
        let with_ring = probe
            .run(&mut n, 8, Observable::Framebuffer, idle)
            .expect("within budget");

        assert_eq!(
            without, with_ring,
            "a trial's samples changed when the caller armed rewind; the rewind \
             machinery is perturbing emulation, and every probe result taken \
             under a different rewind configuration would be suspect"
        );
    }

    /// Replaying an anchor into an emulator running a different ROM must fail
    /// loudly. A snapshot restored across ROMs would produce a confident, wrong
    /// answer, which is worse than no answer for a tool people act on.
    #[test]
    #[should_panic(expected = "different ROM")]
    fn replaying_into_a_different_rom_panics() {
        let n = nes();
        let mut probe = Probe::anchor(&n, Budget::default());

        // A ROM with different PRG contents => a different hash tag.
        let mut other_bytes = synth_nrom();
        let prg_start = 16;
        other_bytes[prg_start + 8] = 0xEA; // NOP somewhere harmless
        let mut other = Nes::from_rom(&other_bytes).expect("fixture parses");
        let _ = probe
            .run(&mut other, 1, Observable::Wram, idle)
            .expect("within budget");
    }
}
