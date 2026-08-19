//! RAM Atlas: **what is each byte of work RAM for?**
//!
//! # The gap this fills
//!
//! Every emulator ships a RAM search — `TASVideos`' `RamSearch` is the canonical
//! one, and this project already has RAM Search plus RAM Watch in the memory
//! compare panel, and per-address access counts in the access counter. All of
//! them *narrow a set the user already has a hypothesis about*. None of them says
//! what an address is.
//!
//! The reason none of them does is that you cannot tell from observation alone.
//! An address that counts up while the score counts up might be the score, or
//! might be a frame counter that happens to be running. Distinguishing them
//! requires changing the byte and seeing whether anything downstream actually
//! moves — which requires re-simulating the same interval twice under a
//! controlled difference. `RustyNES` can do that soundly because its determinism
//! contract is a hard guarantee (see the crate docs), so this module can.
//!
//! # Two stages, with deliberately different epistemic status
//!
//! **Stage 1, [`observe`] then [`classify`]: correlation only.** Capture work RAM
//! once per frame over a window and describe how each address behaved — untouched,
//! ticking every frame, counting monotonically up or down, changing rarely,
//! churning. Every result is a [`Behaviour`], and a `Behaviour` is a
//! **hypothesis**. Nothing here can distinguish cause from coincidence, and the
//! type does not pretend otherwise.
//!
//! **Stage 2, [`verify_liveness`]: a fact, and a narrow one.** Poke the byte,
//! re-simulate from the same anchor, and compare against an unpoked baseline. If
//! the screen diverges, that byte demonstrably drives something visible:
//! [`Liveness::Live`]. If not, [`Liveness::Inert`].
//!
//! # What a label does NOT mean
//!
//! This matters more than what it does mean, because the failure mode of a tool
//! like this is a confident wrong label that a user then builds a cheat or an
//! achievement on:
//!
//! - [`Liveness::Inert`] means "perturbing it changed nothing observable **inside
//!   the budget**". It does *not* mean unused. An address the game rewrites from
//!   a master copy every frame reads `Inert` because the poke is overwritten
//!   before it can matter — and that is a genuinely interesting fact about the
//!   address, but it is not "dead".
//! - [`Liveness::Live`] means "changing it changed the framebuffer". It does
//!   *not* say the address *is* a coordinate, a score, or health. It says the
//!   byte participates in what you see.
//! - A [`Behaviour`] is never upgraded by verification. `RisingCounter` +
//!   `Live` means "counts up, and drives something visible" — two separate
//!   observations, not a conclusion that it is the score.
//!
//! Every threshold below is a named **public** constant with the reasoning
//! attached. Public deliberately: a classifier whose cutoffs are unexplained
//! magic numbers cannot be argued with, and one whose cutoffs are documented but
//! unreachable cannot be displayed next to the labels they produced. A UI showing
//! "changed on 91% of frames, threshold 90%" is checkable; one showing
//! `FrameTick` is not.

use rustynes_core::{Buttons, Nes};

use crate::{Observable, Probe};

/// Bytes of work RAM. The NES has 2 KiB at `$0000-$07FF`, mirrored to `$1FFF`.
pub const WRAM_LEN: usize = 2048;

/// A change on at least this fraction of frames reads as "every frame".
///
/// Not 100%: a frame counter incremented in the NMI handler can miss a frame
/// during a lag frame or a scene transition, and a strict test would then
/// misfile it as [`Behaviour::Volatile`]. 90% tolerates that without admitting
/// anything that merely changes often.
pub const FRAME_TICK_RATIO: f64 = 0.9;

/// At most this many changes over the window reads as event-driven rather than
/// continuous.
///
/// Three, because the interesting cases — a life lost, a door opened, a state
/// transition — happen a handful of times in a several-second window, while
/// anything continuous changes tens of times.
pub const SPARSE_MAX_CHANGES: u32 = 3;

/// A monotonic run needs at least this many steps in one direction before it is
/// called a counter.
///
/// Two, not one. A single step is indistinguishable from any other one-off
/// change, and calling it a "counter" would put a confident label on the
/// weakest possible evidence.
pub const COUNTER_MIN_STEPS: u32 = 2;

/// A transition from at least this high to at most [`WRAP_LOW`] is treated as a
/// wrap rather than a decrease.
pub const WRAP_HIGH: u8 = 0xF0;

/// The low side of the wrap window. See [`WRAP_HIGH`].
pub const WRAP_LOW: u8 = 0x0F;

/// Work RAM sampled once per frame over a window.
///
/// Stored **address-major** (all frames for `$0000`, then all frames for `$0001`)
/// because every consumer walks one address across time, and the transpose makes
/// that a contiguous slice instead of a strided gather over 2 KiB.
#[derive(Clone, Debug)]
pub struct Observation {
    frames: u32,
    /// `WRAM_LEN * frames` bytes, indexed `addr * frames + frame`.
    data: Vec<u8>,
}

impl Observation {
    /// Build an observation from address-major data.
    ///
    /// Public so a caller that captured work RAM by some other route — a movie
    /// replay, a netplay trace — can classify it without going through
    /// [`observe`], and so the classifier can be tested against constructed
    /// series rather than only against whatever a fixture ROM happens to do.
    ///
    /// # Panics
    ///
    /// Panics unless `data.len() == WRAM_LEN * frames`. A silently-misshaped
    /// observation would produce plausible labels for the wrong addresses, which
    /// is the worst outcome available here.
    #[must_use]
    pub fn from_addr_major(frames: u32, data: Vec<u8>) -> Self {
        assert_eq!(
            data.len(),
            WRAM_LEN * frames as usize,
            "observation data must be exactly WRAM_LEN * frames bytes, \
             address-major; a misshaped buffer labels the wrong addresses"
        );
        Self { frames, data }
    }

    /// Frames captured.
    #[must_use]
    pub const fn frames(&self) -> u32 {
        self.frames
    }

    /// This address's values over the window, one per frame, in order.
    ///
    /// # Panics
    ///
    /// Panics if `addr` is outside work RAM.
    #[must_use]
    pub fn series(&self, addr: u16) -> &[u8] {
        let a = addr as usize;
        assert!(a < WRAM_LEN, "address ${addr:04X} is outside work RAM");
        let f = self.frames as usize;
        &self.data[a * f..(a + 1) * f]
    }
}

/// Capture work RAM once per frame, advancing `nes`.
///
/// **Advances the emulator it is given**, exactly like [`crate::latency::measure`]
/// — pass a scratch instance, or restore afterwards. It deliberately does not
/// snapshot-and-restore internally: a caller that wants a repeatable window
/// anchors a [`Probe`] first and owns the restore, and hiding that here would
/// make the cost invisible.
///
/// `input` is called once per frame with the zero-based frame index. Holding a
/// button changes what the game does, so it changes what the atlas describes;
/// that is a feature — an atlas taken while walking right tells you about
/// movement state.
pub fn observe<F>(nes: &mut Nes, frames: u32, mut input: F) -> Observation
where
    F: FnMut(u32) -> (Buttons, Buttons),
{
    // Frame-major while capturing (work RAM is contiguous, so a frame is one
    // copy), transposed once at the end. The alternative — writing directly into
    // address-major layout — would stride 2,048 times per frame across a buffer
    // far larger than cache.
    let mut frame_major: Vec<u8> = Vec::with_capacity(WRAM_LEN * frames as usize);
    // Rewind capture is suppressed for the whole window, by the same guard a
    // trial uses — which also moves the provenance stores out, so an
    // observation's 180 rolled-back frames cannot contribute attributions to a
    // timeline they never happened on (v2.3.7). These frames advance the live emulator and the caller rolls
    // them back, so they never happened on the user's timeline — letting them
    // into the ring would allow rewinding *into an observation*, which is exactly
    // the defect this crate had just fixed for `verify_liveness` and then
    // reintroduced one function away. Caught in review on PR #392.
    //
    // The guard also makes this panic-safe: an observation is 180 frames, and a
    // panic part-way through must not leave capture disabled on a `Nes` the
    // caller keeps using.
    let guard = crate::TrialGuard::enter(nes);
    for f in 0..frames {
        let (p1, p2) = input(f);
        guard.nes.set_buttons(0, p1);
        guard.nes.set_buttons(1, p2);
        guard.nes.run_frame();
        frame_major.extend_from_slice(&guard.nes.wram()[..WRAM_LEN]);
    }
    // Restores the caller's capture setting.
    drop(guard);

    let n = frames as usize;
    let mut data = vec![0u8; WRAM_LEN * n];
    for (f, chunk) in frame_major.chunks_exact(WRAM_LEN).enumerate() {
        for (a, &v) in chunk.iter().enumerate() {
            data[a * n + f] = v;
        }
    }
    Observation::from_addr_major(frames, data)
}

/// How an address behaved over the window. **A hypothesis, never a conclusion.**
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Behaviour {
    /// Held one value for the whole window.
    ///
    /// The largest class by far in practice, and the useful one to be able to
    /// exclude: it is most of work RAM on any given screen.
    Untouched,
    /// Changed on nearly every frame — an animation phase, a frame counter, a
    /// scroll position.
    FrameTick,
    /// Only ever went up (wraps allowed), at least [`COUNTER_MIN_STEPS`] times.
    RisingCounter,
    /// Only ever went down (wraps allowed), at least [`COUNTER_MIN_STEPS`] times.
    FallingCounter,
    /// Changed at most [`SPARSE_MAX_CHANGES`] times — event-driven state.
    Sparse,
    /// Changed often, in both directions, but not every frame.
    Volatile,
}

/// Raw per-address statistics, kept alongside the label as its evidence.
///
/// Carried so a UI can show *why* an address was classified as it was. A label
/// without its evidence cannot be checked, and this tool's whole claim is that
/// its output can be.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct AddressStats {
    /// Frames on which the value differed from the previous frame.
    pub changes: u32,
    /// Distinct values observed.
    pub distinct: u32,
    /// Transitions strictly upward, excluding wraps.
    pub increases: u32,
    /// Transitions strictly downward, excluding wraps.
    pub decreases: u32,
    /// Transitions that looked like an 8-bit wrap in either direction.
    pub wraps: u32,
    /// Lowest value observed.
    pub min: u8,
    /// Highest value observed.
    pub max: u8,
    /// Index of the first frame that differed from frame 0, if any.
    pub first_change: Option<u32>,
}

/// Whether perturbing an address demonstrably changes what the screen shows.
///
/// Read the caveats in the module docs before acting on this — particularly that
/// [`Self::Inert`] is not "unused".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Liveness {
    /// Not tested: verification was not requested, or the trial budget ran out
    /// before reaching this address.
    ///
    /// Distinct from [`Self::Inert`] on purpose. "We did not look" and "we looked
    /// and saw nothing" are different claims, and collapsing them is how a
    /// budget-limited sweep starts reporting untested addresses as dead.
    Untested,
    /// Poking the byte changed the framebuffer within the window.
    Live,
    /// Poking the byte changed nothing observable within the window.
    Inert,
}

/// One address's entry in the atlas: what it did, the evidence, and whether
/// perturbing it mattered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Label {
    /// Work-RAM address, `$0000-$07FF`.
    pub addr: u16,
    /// The behavioural hypothesis from observation.
    pub behaviour: Behaviour,
    /// The evidence behind [`Self::behaviour`].
    pub stats: AddressStats,
    /// The verification verdict, if verification ran.
    pub liveness: Liveness,
    /// Frame at which the perturbed run first diverged from the baseline, when
    /// [`Self::liveness`] is [`Liveness::Live`].
    ///
    /// Useful on its own: a byte that diverges on frame 0 is read the very next
    /// frame, while one that diverges on frame 30 feeds something slower.
    pub divergence_frame: Option<u32>,
}

/// Compute one address's statistics from its series.
#[must_use]
pub fn stats_for(series: &[u8]) -> AddressStats {
    let mut s = AddressStats::default();
    let Some(&first) = series.first() else {
        return s;
    };
    s.min = first;
    s.max = first;

    // Distinct values via a 256-bit set rather than sorting or hashing: the
    // domain is one byte, so this is exact and allocation-free.
    let mut seen = [false; 256];
    seen[first as usize] = true;

    for (i, w) in series.windows(2).enumerate() {
        let (prev, next) = (w[0], w[1]);
        s.min = s.min.min(next);
        s.max = s.max.max(next);
        seen[next as usize] = true;
        if prev == next {
            continue;
        }
        s.changes += 1;
        if s.first_change.is_none() {
            // `i` indexes the window, so the CHANGED frame is `i + 1`.
            s.first_change = u32::try_from(i + 1).ok();
        }
        // A wrap is counted as neither an increase nor a decrease, so a counter
        // that rolls over stays monotonic. Judged by the two values straddling
        // the byte boundary rather than by the arithmetic difference, which
        // cannot tell 0xFF -> 0x00 from 0xFF -> 0x00 by subtraction alone.
        let wrapped_up = prev >= WRAP_HIGH && next <= WRAP_LOW;
        let wrapped_down = prev <= WRAP_LOW && next >= WRAP_HIGH;
        if wrapped_up || wrapped_down {
            s.wraps += 1;
        } else if next > prev {
            s.increases += 1;
        } else {
            s.decreases += 1;
        }
    }
    // `seen` is exactly 256 entries, so the count cannot exceed 256. `expect`
    // rather than `unwrap_or(u32::MAX)`: the saturating form implied the value
    // could plausibly be huge, which misdescribes a one-byte domain.
    // (PR #392 review.)
    s.distinct = u32::try_from(seen.iter().filter(|&&b| b).count())
        .expect("at most 256 distinct byte values");
    s
}

/// Classify one address from its statistics.
///
/// Order matters and is deliberate: `Untouched` and `FrameTick` are decided
/// before the counter tests, so a frame counter is reported as a frame tick
/// rather than as a `RisingCounter` — it is monotonic, but "ticks every frame" is
/// the more specific and more useful statement.
#[must_use]
pub fn classify_stats(stats: &AddressStats, frames: u32) -> Behaviour {
    if stats.changes == 0 {
        return Behaviour::Untouched;
    }
    // Transitions available, not frames: an N-frame window has N-1 of them.
    let transitions = f64::from(frames.saturating_sub(1));
    if transitions > 0.0 && f64::from(stats.changes) / transitions >= FRAME_TICK_RATIO {
        return Behaviour::FrameTick;
    }
    let rising = stats.decreases == 0 && stats.increases >= COUNTER_MIN_STEPS;
    let falling = stats.increases == 0 && stats.decreases >= COUNTER_MIN_STEPS;
    if rising {
        return Behaviour::RisingCounter;
    }
    if falling {
        return Behaviour::FallingCounter;
    }
    if stats.changes <= SPARSE_MAX_CHANGES {
        return Behaviour::Sparse;
    }
    Behaviour::Volatile
}

/// Classify every work-RAM address in an observation.
///
/// Returns all [`WRAM_LEN`] labels, including [`Behaviour::Untouched`] ones, with
/// [`Liveness::Untested`] throughout — verification is a separate, budgeted step.
/// Returning the untouched addresses rather than filtering them is deliberate:
/// "this address did nothing during the window" is an answer, and a caller that
/// wants only the interesting ones can filter, whereas one that wants the full
/// map cannot recover what was dropped.
#[must_use]
pub fn classify(obs: &Observation) -> Vec<Label> {
    (0..WRAM_LEN)
        .map(|a| {
            let addr = u16::try_from(a).expect("WRAM_LEN fits u16");
            let stats = stats_for(obs.series(addr));
            Label {
                addr,
                behaviour: classify_stats(&stats, obs.frames()),
                stats,
                liveness: Liveness::Untested,
                divergence_frame: None,
            }
        })
        .collect()
}

/// The value a perturbation writes: the bitwise complement of what is there.
///
/// Guaranteed to differ from the current value for every input, which a
/// `wrapping_add(1)` also achieves — but the complement is a *large* change, and
/// a large change is far more likely to produce an observable difference. A
/// coordinate nudged by one may move a sprite inside the same pixel; the same
/// coordinate complemented moves it across the screen.
#[must_use]
pub const fn perturbed_value(current: u8) -> u8 {
    !current
}

/// Perturb one address and report whether `observable` then diverged.
///
/// Runs up to two trials against `probe`'s budget: a baseline and a perturbed
/// run. Returns [`Liveness::Untested`] with no divergence frame if the budget
/// cannot fund both — never [`Liveness::Inert`], because an untested address is
/// not an inert one.
///
/// **Liveness is relative to the observable, and that is the point.** The same
/// byte can be [`Liveness::Live`] under [`Observable::Wram`] (the poke reached
/// memory, which it always does) and [`Liveness::Inert`] under
/// [`Observable::Framebuffer`] (nothing drew from it). Answering "does this drive
/// what I see?" is [`Observable::Framebuffer`]; "does this drive what I hear?" is
/// [`Observable::AudioEnergy`]. A caller that reports a verdict without naming
/// the lens it used is over-claiming.
///
/// `nes` is scratch: `probe` restores its anchor into it before each trial.
pub fn verify_liveness(
    probe: &mut Probe,
    nes: &mut Nes,
    addr: u16,
    frames: u32,
    observable: Observable,
) -> (Liveness, Option<u32>) {
    let idle = |_: u32| (Buttons::empty(), Buttons::empty());

    // An address outside work RAM cannot be perturbed, and the old code silently
    // skipped the write and then let the two identical trials agree — reporting
    // `Inert` for an address it never touched. That is the exact failure mode the
    // module docs above spend three paragraphs warning against, produced by this
    // function's own bounds check. `Untested` is the honest answer, and refusing
    // before the budget is spent also stops two trials going on a no-op.
    //
    // The case is reachable: this is public and takes a full `u16`, so a caller
    // passing a CPU-space mirror such as `$0810` is entirely plausible. Folding
    // mirrors to their physical byte is deliberately NOT done here — silently
    // reinterpreting the caller's address would be its own confident guess.
    // (PR #392 review.)
    let a = addr as usize;
    if a >= WRAM_LEN {
        return (Liveness::Untested, None);
    }

    // Both trials must be funded, or neither is meaningful. Checking up front
    // avoids spending a baseline trial that the perturbed run then cannot match.
    if probe.trials_remaining() < 2 {
        return (Liveness::Untested, None);
    }

    let Some(baseline) = probe.run(nes, frames, observable, idle) else {
        return (Liveness::Untested, None);
    };
    let Some(poked) = probe.run_perturbed(
        nes,
        frames,
        observable,
        |n| {
            // `a` is in range: checked above, before any trial was spent.
            let current = n.wram()[a];
            n.wram_mut()[a] = perturbed_value(current);
        },
        idle,
    ) else {
        return (Liveness::Untested, None);
    };

    // A short vector means the budget cut a trial off. Comparing unequal-length
    // runs would silently compare prefixes and could report "no divergence" for
    // a run that simply stopped early.
    if baseline.len() != poked.len() || baseline.is_empty() {
        return (Liveness::Untested, None);
    }

    Probe::first_divergence(&baseline, &poked)
        .map_or((Liveness::Inert, None), |f| (Liveness::Live, Some(f)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a single-address observation: `series` for `$0000`, everything else
    /// held at zero. Keeps the classifier tests readable.
    fn obs_of(series: &[u8]) -> Observation {
        let frames = u32::try_from(series.len()).expect("test window is small");
        let mut data = vec![0u8; WRAM_LEN * series.len()];
        data[..series.len()].copy_from_slice(series);
        Observation::from_addr_major(frames, data)
    }

    fn behaviour_of(series: &[u8]) -> Behaviour {
        let obs = obs_of(series);
        let stats = stats_for(obs.series(0));
        classify_stats(&stats, obs.frames())
    }

    #[test]
    fn a_constant_address_is_untouched() {
        assert_eq!(behaviour_of(&[7, 7, 7, 7, 7, 7]), Behaviour::Untouched);
    }

    #[test]
    fn a_value_changing_every_frame_is_a_frame_tick() {
        assert_eq!(
            behaviour_of(&[0, 1, 2, 3, 4, 5, 6, 7]),
            Behaviour::FrameTick
        );
    }

    /// A frame counter is monotonic, so it would also satisfy the rising-counter
    /// test. `FrameTick` must win, because it is the more specific statement —
    /// this pins the ordering inside `classify_stats`, which is otherwise easy to
    /// "tidy" into the wrong answer.
    #[test]
    fn a_frame_counter_is_a_frame_tick_not_a_rising_counter() {
        let stats = stats_for(&[0, 1, 2, 3, 4, 5, 6, 7]);
        assert_eq!(stats.decreases, 0, "fixture is monotonic");
        assert!(
            stats.increases >= COUNTER_MIN_STEPS,
            "fixture would qualify"
        );
        assert_eq!(
            classify_stats(&stats, 8),
            Behaviour::FrameTick,
            "the more specific label lost to the more general one"
        );
    }

    /// A score that rises in bursts, not every frame.
    #[test]
    fn an_intermittent_rise_is_a_rising_counter() {
        assert_eq!(
            behaviour_of(&[0, 0, 0, 5, 5, 5, 9, 9, 9, 12, 12, 12]),
            Behaviour::RisingCounter
        );
    }

    /// A timer counting down, likewise not every frame.
    #[test]
    fn an_intermittent_fall_is_a_falling_counter() {
        assert_eq!(
            behaviour_of(&[60, 60, 60, 59, 59, 59, 58, 58, 58, 57, 57, 57]),
            Behaviour::FallingCounter
        );
    }

    /// THE case that makes wrap handling load-bearing: a counter rolling
    /// `0xFF -> 0x00` is still counting up. Without the wrap window that single
    /// transition registers as a decrease and the address is demoted from
    /// `RisingCounter` to `Volatile` — a real counter reported as churn.
    #[test]
    fn a_counter_that_wraps_is_still_monotonic() {
        let series = [0xFD, 0xFD, 0xFE, 0xFE, 0xFF, 0xFF, 0x00, 0x00, 0x01, 0x01];
        let stats = stats_for(&series);
        assert_eq!(
            stats.wraps, 1,
            "the 0xFF -> 0x00 step was not seen as a wrap"
        );
        assert_eq!(stats.decreases, 0, "a wrap was miscounted as a decrease");
        assert_eq!(behaviour_of(&series), Behaviour::RisingCounter);
    }

    #[test]
    fn a_rare_change_is_sparse() {
        let mut series = [3u8; 20];
        series[10] = 2;
        assert_eq!(behaviour_of(&series), Behaviour::Sparse);
    }

    /// Changing often in both directions, but not every frame.
    #[test]
    fn two_way_churn_is_volatile() {
        let series = [0, 5, 5, 2, 2, 9, 9, 1, 1, 7, 7, 3, 3, 8, 8, 4];
        let stats = stats_for(&series);
        assert!(stats.increases > 0 && stats.decreases > 0, "fixture churns");
        assert_eq!(behaviour_of(&series), Behaviour::Volatile);
    }

    /// A single step is not a counter. Two is the documented floor, and one step
    /// is indistinguishable from any other one-off change.
    #[test]
    fn one_step_is_not_a_counter() {
        let mut series = [1u8; 20];
        for v in &mut series[10..] {
            *v = 2;
        }
        let stats = stats_for(&series);
        assert_eq!(stats.increases, 1);
        assert_eq!(behaviour_of(&series), Behaviour::Sparse);
    }

    #[test]
    fn stats_record_the_evidence() {
        let stats = stats_for(&[10, 10, 12, 12, 11]);
        assert_eq!(stats.changes, 2);
        assert_eq!(stats.increases, 1);
        assert_eq!(stats.decreases, 1);
        assert_eq!(stats.min, 10);
        assert_eq!(stats.max, 12);
        assert_eq!(stats.distinct, 3);
        assert_eq!(
            stats.first_change,
            Some(2),
            "first_change must index the frame that CHANGED, not the one before it"
        );
    }

    /// A perturbation must always actually perturb, or a "no divergence" result
    /// would mean nothing. Checked across the whole byte domain because the two
    /// fixed points of a naive scheme (`0x00` for multiply, `0xFF` for OR) are
    /// exactly the values a real address is most likely to hold.
    #[test]
    fn perturbation_always_differs_from_the_original() {
        for v in 0..=u8::MAX {
            assert_ne!(perturbed_value(v), v, "perturbing {v:#04X} was a no-op");
        }
    }

    #[test]
    fn classify_labels_every_address_as_untested() {
        let obs = obs_of(&[0, 1, 2, 3]);
        let labels = classify(&obs);
        assert_eq!(labels.len(), WRAM_LEN);
        assert!(
            labels.iter().all(|l| l.liveness == Liveness::Untested),
            "observation alone must never claim liveness"
        );
        assert_eq!(labels[0].behaviour, Behaviour::FrameTick);
        assert_eq!(labels[1].behaviour, Behaviour::Untouched);
    }

    /// Addresses must line up with their series. A transpose off by one row
    /// would produce a fully-populated, entirely wrong atlas — plausible output
    /// being the failure mode this tool most needs to avoid.
    #[test]
    fn the_transpose_keeps_addresses_aligned() {
        let frames = 4usize;
        let mut data = vec![0u8; WRAM_LEN * frames];
        // $0010 counts up; $0011 counts down.
        for f in 0..frames {
            data[0x10 * frames + f] = u8::try_from(f).expect("small");
            data[0x11 * frames + f] = u8::try_from(frames - f).expect("small");
        }
        let obs = Observation::from_addr_major(4, data);
        assert_eq!(obs.series(0x10), &[0, 1, 2, 3]);
        assert_eq!(obs.series(0x11), &[4, 3, 2, 1]);
        let labels = classify(&obs);
        assert_eq!(labels[0x10].behaviour, Behaviour::FrameTick);
        assert_eq!(labels[0x11].behaviour, Behaviour::FrameTick);
    }

    #[test]
    #[should_panic(expected = "address-major")]
    fn a_misshaped_observation_panics() {
        let _ = Observation::from_addr_major(4, vec![0u8; 10]);
    }

    // ---- integration: the whole path against a real `Nes` ------------------
    //
    // The classifier tests above use constructed series, which is right for
    // pinning thresholds but proves nothing about `observe` and
    // `verify_liveness` actually working. This project has repeatedly shipped
    // features whose core logic was tested and whose wiring was not — pixel
    // provenance and the attestation path both — so the wiring is tested here.

    use crate::Budget;

    /// A minimal NROM that spins forever, mirroring the crate-root fixture.
    fn synth_nrom() -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"NES\x1A");
        bytes.push(1);
        bytes.push(1);
        bytes.push(0);
        bytes.push(0);
        bytes.extend_from_slice(&[0u8; 8]);
        let mut prg = vec![0u8; 16 * 1024];
        prg[0] = 0x4C; // JMP $C000
        prg[1] = 0x00;
        prg[2] = 0xC0;
        let len = prg.len();
        prg[len - 6] = 0x00;
        prg[len - 5] = 0xC0;
        prg[len - 4] = 0x00; // RESET -> $C000
        prg[len - 3] = 0xC0;
        prg[len - 2] = 0x00;
        prg[len - 1] = 0xC0;
        bytes.extend_from_slice(&prg);
        bytes.extend_from_slice(&vec![0u8; 8 * 1024]);
        bytes
    }

    fn nes() -> Nes {
        Nes::from_rom(&synth_nrom()).expect("fixture parses")
    }

    fn idle_input(_: u32) -> (Buttons, Buttons) {
        (Buttons::empty(), Buttons::empty())
    }

    /// `observe` must produce a correctly-shaped observation from a real
    /// emulator, and a ROM that only spins must classify as untouched
    /// throughout. If the transpose or the capture were wrong, a do-nothing ROM
    /// is the one case that would still look plausible — hence checking the
    /// shape explicitly rather than only the labels.
    #[test]
    fn observing_a_spinning_rom_yields_an_untouched_atlas() {
        let mut n = nes();
        let obs = observe(&mut n, 6, idle_input);
        assert_eq!(obs.frames(), 6);
        assert_eq!(obs.series(0).len(), 6);
        assert_eq!(
            obs.series(0x7FF).len(),
            6,
            "the last address must be present"
        );

        let labels = classify(&obs);
        assert_eq!(labels.len(), WRAM_LEN);
        assert!(
            labels.iter().all(|l| l.behaviour == Behaviour::Untouched),
            "a ROM that only spins changed work RAM"
        );
        assert!(labels.iter().all(|l| l.liveness == Liveness::Untested));
    }

    /// Observing must not leak its frames into the caller's rewind ring.
    ///
    /// `observe` advances the live emulator and the caller rolls it back, so those
    /// frames never happened on the user's timeline. Letting them into the ring
    /// would allow rewinding *into an observation* — the same defect this crate
    /// fixed for `verify_liveness` and then reintroduced one function away, caught
    /// in review on PR #392.
    ///
    /// Mutation-checked: removing the `TrialGuard` from `observe` fails this
    /// with the ring grown by the window length.
    #[test]
    fn observing_does_not_pollute_the_callers_rewind_ring() {
        let mut n = nes();
        n.enable_rewind();
        for _ in 0..3 {
            n.run_frame();
            n.rewind_capture();
        }
        let before = n.rewind_len();
        assert!(before > 0, "fixture failed to buffer any rewind frames");

        let _ = observe(&mut n, 12, idle_input);

        assert_eq!(
            n.rewind_len(),
            before,
            "an observation leaked its frames into the caller's rewind ring"
        );
        assert!(
            n.rewind_capture_enabled(),
            "observation left rewind capture disabled"
        );
    }

    /// THE positive case for verification: a poke must reach the observable and
    /// the divergence must be detected. Under `Observable::Wram` this is
    /// guaranteed by construction — the poke IS a work-RAM change — which is
    /// what makes it a clean test of the machinery rather than of the game.
    #[test]
    fn a_poke_is_live_under_the_wram_observable() {
        let mut n = nes();
        let mut probe = Probe::anchor(&n, Budget::default());
        let (liveness, frame) = verify_liveness(&mut probe, &mut n, 0x0010, 4, Observable::Wram);
        assert_eq!(
            liveness,
            Liveness::Live,
            "a work-RAM poke did not register as a work-RAM divergence"
        );
        assert_eq!(
            frame,
            Some(0),
            "the poke lands before frame 0, so it must diverge on frame 0"
        );
    }

    /// The matching negative case, and the reason `Liveness` is documented as
    /// relative to its lens: the SAME poke on the SAME ROM is inert through the
    /// framebuffer, because this fixture never draws from work RAM.
    ///
    /// This pair is what proves `Inert` is a real verdict rather than a stuck
    /// default — one perturbation, two observables, opposite answers.
    #[test]
    fn the_same_poke_is_inert_under_the_framebuffer_observable() {
        let mut n = nes();
        let mut probe = Probe::anchor(&n, Budget::default());
        let (liveness, frame) =
            verify_liveness(&mut probe, &mut n, 0x0010, 4, Observable::Framebuffer);
        assert_eq!(liveness, Liveness::Inert);
        assert_eq!(
            frame, None,
            "an inert verdict must carry no divergence frame"
        );
    }

    /// An address outside work RAM must be `Untested`, never `Inert`.
    ///
    /// The old code skipped the perturbation for such an address and then let the
    /// two identical trials agree, reporting `Inert` — a confident verdict for a
    /// byte it never touched, which is precisely what this module documents itself
    /// as never doing. `$0810` is a CPU-space mirror of `$0010` and a plausible
    /// caller input. Raised in review on PR #392.
    #[test]
    fn an_address_outside_work_ram_is_untested_not_inert() {
        let mut n = nes();
        let mut probe = Probe::anchor(&n, Budget::default());
        let (liveness, frame) = verify_liveness(&mut probe, &mut n, 0x0810, 4, Observable::Wram);
        assert_eq!(
            liveness,
            Liveness::Untested,
            "an un-perturbable address was given a verdict"
        );
        assert_eq!(frame, None);
        assert_eq!(
            probe.trials_used(),
            0,
            "two trials were spent on an address that could not be perturbed"
        );
    }

    /// A budget too small to fund both trials must report `Untested`, never
    /// `Inert`. Collapsing those two is how a budget-limited sweep starts
    /// reporting addresses it never looked at as dead.
    #[test]
    fn an_unaffordable_verification_is_untested_not_inert() {
        let mut n = nes();
        let budget = Budget {
            max_trials: 1,
            ..Budget::default()
        };
        let mut probe = Probe::anchor(&n, budget);
        let (liveness, frame) = verify_liveness(&mut probe, &mut n, 0x0010, 4, Observable::Wram);
        assert_eq!(
            liveness,
            Liveness::Untested,
            "a verification that could not afford both trials claimed a verdict"
        );
        assert_eq!(frame, None);
        assert_eq!(
            probe.trials_used(),
            0,
            "an unaffordable verification must not spend a half-measurement"
        );
    }

    /// Verification must not leave the caller's atlas sweep unable to continue:
    /// each address costs exactly two trials, so the budget arithmetic a caller
    /// does to size a sweep has to hold.
    #[test]
    fn verification_costs_exactly_two_trials() {
        let mut n = nes();
        let mut probe = Probe::anchor(&n, Budget::default());
        let before = probe.trials_used();
        let _ = verify_liveness(&mut probe, &mut n, 0x0010, 2, Observable::Wram);
        assert_eq!(
            probe.trials_used() - before,
            2,
            "a caller sizing a sweep by trials-per-address would mis-budget"
        );
    }
}
