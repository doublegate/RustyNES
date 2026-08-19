//! RAM Atlas panel (v2.3.6) — **what is each byte of work RAM for?**
//!
//! The UI over [`rustynes_probe::atlas`]. The classifier is headless and tested
//! independently; this file is presentation plus the two budgeted actions that
//! drive the emulator, and deliberately contains no classification logic of its
//! own — a threshold decided here would be a threshold no test could reach.
//!
//! # Why this is not the memory-compare panel
//!
//! Tools → Analysis already offers RAM Search and RAM Watch, and Debug → Memory
//! offers per-address access counts. Those narrow a set the user already has a
//! hypothesis about, or say an address was touched. Neither says what it is. This
//! panel classifies every address, then lets the user *verify* a candidate by
//! perturbing it — the step that separates cause from coincidence, and the step
//! no RAM search can perform.
//!
//! # The two actions are separate because they cost differently
//!
//! **Observe** runs a window of frames once and classifies all 2048 addresses. It
//! is bounded by [`OBSERVE_FRAMES`] and costs about three seconds of emulation.
//!
//! **Verify** costs two trials per address — a baseline and a perturbed run — so
//! a full 2048-address sweep would be over four thousand trials and tens of
//! minutes. It is therefore offered per-address on demand and as a bounded batch
//! of [`VERIFY_BATCH`], never as "verify everything". A button that quietly takes
//! twenty minutes is a worse affordance than one that admits its limit.
//!
//! # Honesty
//!
//! The panel's job is to not over-claim on the classifier's behalf. Three rules:
//!
//! - `Untested` renders as its own state, never blank and never as `Inert`.
//! - Every verification names the **lens** it used, because liveness is relative
//!   to the observable (a byte is `Live` through work RAM and may be `Inert`
//!   through the framebuffer).
//! - The evidence — change count, direction, range, the threshold that decided it
//!   — is shown beside the label, so a reader can disagree with the label.

use rustynes_core::{Buttons, Nes};
use rustynes_probe::atlas::{
    self, Behaviour, FRAME_TICK_RATIO, Label, Liveness, Observation, SPARSE_MAX_CHANGES, WRAM_LEN,
};
use rustynes_probe::{Budget, Observable, Probe};

use crate::icons::{glyph, label as ic};

/// Frames captured by one Observe.
///
/// About three seconds of NTSC. Long enough for a score to tick, a timer to
/// count and an animation to cycle; short enough that the synchronous pause is
/// comparable to the Latency Oracle's, which users already accept.
const OBSERVE_FRAMES: u32 = 180;

/// Frames each verification trial runs.
///
/// Short on purpose: a poke that matters usually matters immediately, and the
/// cost is paid twice per address. A byte whose effect takes longer than this
/// reads `Inert`, which the panel says is "not observable inside the budget"
/// rather than "dead".
const VERIFY_FRAMES: u32 = 8;

/// Trials one address costs to verify: a baseline plus a perturbed run.
///
/// Named because it appears in three places that must agree — the hover text
/// quoting the cost, the `Budget` sizing in `do_verify`, and
/// `atlas::verify_liveness`'s own up-front affordability check. A bare `2` in the
/// budget with a stale number in the tooltip is the drift this prevents.
/// (PR #392 review.)
const TRIALS_PER_ADDRESS: usize = 2;

/// Addresses one batch verification will attempt.
///
/// Sixteen, so a batch is 32 trials — a fraction of a second of emulation, and
/// small enough that the button's cost is honestly predictable. The alternative,
/// verifying every changed address, is unbounded in the only case that matters:
/// a busy game where hundreds of addresses moved.
const VERIFY_BATCH: usize = 16;

/// Persistent panel state.
pub struct AtlasPanel {
    /// Classified labels; empty until an observation has been made.
    labels: Vec<Label>,
    /// Frames the current labels were derived from.
    frames: u32,
    /// The lens verification uses. Part of the result, not a hidden default.
    lens: Observable,
    /// Hide the (usually overwhelming) untouched addresses.
    hide_untouched: bool,
    /// "Observe" was clicked this frame; the action runs after the render so
    /// `nes` is never captured by the viewport callback.
    observe_requested: bool,
    /// A single address the user asked to verify.
    verify_requested: Option<u16>,
    /// A bounded batch verification was requested.
    verify_batch_requested: bool,
    /// The address whose detail is expanded.
    selected: Option<u16>,
    /// An address the user asked to send to RAM Watch, with the label the atlas
    /// can honestly attach to it. Drained by the overlay after this panel's
    /// render, for the same reason as the two emulator actions: the destination
    /// panel's state is not reachable from here.
    watch_requested: Option<(u16, String)>,
    /// Status / cost line.
    status: String,
}

impl Default for AtlasPanel {
    fn default() -> Self {
        Self {
            labels: Vec::new(),
            frames: 0,
            // The framebuffer is the default question — "does this drive what I
            // see?" — and the one whose `Inert` answer is informative. Work RAM
            // would report almost everything `Live` (a poke is a work-RAM change
            // by construction), which is true and useless.
            lens: Observable::Framebuffer,
            hide_untouched: true,
            observe_requested: false,
            verify_requested: None,
            verify_batch_requested: false,
            selected: None,
            watch_requested: None,
            status: String::new(),
        }
    }
}

impl AtlasPanel {
    /// Discard everything bound to the previous ROM.
    ///
    /// An atlas describes one game's memory map. Left standing across a ROM
    /// change every label becomes a confident statement about a cartridge it was
    /// never derived from — worse than a stale latency number, because there are
    /// two thousand of them and they look like a map.
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// Take a pending RAM Watch export, if one was requested this frame.
    ///
    /// Taken rather than read so a single click seeds a single entry. The
    /// alternative — leaving the request standing and de-duplicating downstream —
    /// makes the panel's behaviour depend on the destination list's contents,
    /// which is exactly the coupling this hand-off exists to avoid.
    pub fn take_watch_request(&mut self) -> Option<(u16, String)> {
        self.watch_requested.take()
    }

    /// Report the outcome of an export back onto the panel's status line.
    ///
    /// The destination window is drawn EARLIER in the overlay's pass than this
    /// one, so a seed made here does not appear over there until the next frame.
    /// Without this line the user's evidence that anything happened is a list
    /// they may not be looking at — an export that silently succeeds and one that
    /// silently does nothing look identical.
    pub fn note_export(&mut self, text: impl Into<String>) {
        self.status = text.into();
    }
}

/// Draw the RAM Atlas window. `nes` is `Some` only when a ROM is loaded under the
/// held lock; both actions are disabled otherwise.
pub fn show(
    ctx: &egui::Context,
    detached: &mut std::collections::HashSet<&'static str>,
    open: &mut bool,
    state: &mut AtlasPanel,
    nes: Option<&mut Nes>,
    writes_locked: bool,
) {
    // Both actions advance the live `Nes`, and Verify pokes work RAM. Under
    // netplay or a TAS record/replay that diverges a timeline other peers are
    // lockstepped to; under RetroAchievements hardcore it is the write the mode
    // exists to forbid. `restore_quiet` puts the state back, but a netplay peer
    // has already consumed the frames. Gated on the same predicate `emu.write`
    // uses rather than on `nes.is_some()` alone. (PR #392 review.)
    let can_run = nes.is_some() && !writes_locked;
    super::detachable_window(
        ctx,
        detached,
        "ram_atlas",
        "RAM Atlas",
        super::WindowCfg {
            default_width: Some(520.0),
            ..Default::default()
        },
        open,
        |ui| body(ui, state, can_run, writes_locked),
    );
    // Actions run AFTER the render — `nes` is free here, not captured by any
    // closure. Same shape as the Latency Oracle and `BasicBot`.
    run_actions(state, nes);
}

/// The panel body, shared by the docked window and the detached OS viewport.
fn body(ui: &mut egui::Ui, state: &mut AtlasPanel, can_run: bool, writes_locked: bool) {
    ui.label("Classifies every byte of the 2 KiB work RAM by how it behaves, then");
    ui.label("verifies a candidate by changing it and re-simulating.");
    ui.separator();

    ui.horizontal(|ui| {
        if ui
            .add_enabled(
                can_run,
                egui::Button::new(ic(glyph::MAGNIFYING_GLASS_PLUS, "Observe")),
            )
            .on_hover_text(format!(
                "Runs {OBSERVE_FRAMES} frames (about 3 seconds) and classifies all \
                 {WRAM_LEN} addresses. Briefly pauses the emulator."
            ))
            .clicked()
        {
            state.observe_requested = true;
        }
        ui.label("lens:");
        // The lens is chosen before verification and reported with every result:
        // liveness is relative to it.
        egui::ComboBox::from_id_salt("atlas_lens")
            .selected_text(lens_name(state.lens))
            .show_ui(ui, |ui| {
                for lens in [
                    Observable::Framebuffer,
                    Observable::AudioEnergy,
                    Observable::Wram,
                ] {
                    ui.selectable_value(&mut state.lens, lens, lens_name(lens));
                }
            });
    });

    if !can_run {
        // Distinguish the two reasons, or a user in a netplay session sees a dead
        // button and concludes the tool is broken.
        if writes_locked {
            ui.weak(
                "Unavailable during netplay, TAS recording or playback, and \
                 RetroAchievements hardcore: this tool advances the emulator and \
                 pokes memory.",
            );
        } else {
            ui.weak("Load a ROM to observe.");
        }
    }

    if state.labels.is_empty() {
        if !state.status.is_empty() {
            ui.separator();
            ui.weak(&state.status);
        }
        return;
    }

    ui.separator();
    summary(ui, state);
    ui.separator();

    ui.horizontal(|ui| {
        ui.checkbox(&mut state.hide_untouched, "Hide untouched");
        // The real count, not the cap: "Verify next 16" when three remain
        // overstates what the button will do. (PR #392 review.)
        let pending = state
            .labels
            .iter()
            .filter(|l| l.liveness == Liveness::Untested && l.behaviour != Behaviour::Untouched)
            .count()
            .min(VERIFY_BATCH);
        if ui
            .add_enabled(
                can_run && pending > 0,
                egui::Button::new(ic(glyph::GAUGE, &format!("Verify next {pending}"))),
            )
            .on_hover_text(format!(
                "Perturbs up to {VERIFY_BATCH} not-yet-tested addresses, \
                 {TRIALS_PER_ADDRESS} trials each. A full sweep of all {WRAM_LEN} \
                 addresses is deliberately not offered: it would be {} trials.",
                WRAM_LEN * TRIALS_PER_ADDRESS
            ))
            .clicked()
        {
            state.verify_batch_requested = true;
        }
    });

    ui.separator();
    table(ui, state, can_run);

    if !state.status.is_empty() {
        ui.separator();
        ui.weak(&state.status);
    }
}

/// Counts per behaviour, plus how much of the atlas has been verified.
fn summary(ui: &mut egui::Ui, state: &AtlasPanel) {
    let mut counts = [0usize; 6];
    let mut tested = 0usize;
    let mut live = 0usize;
    for l in &state.labels {
        counts[behaviour_index(l.behaviour)] += 1;
        match l.liveness {
            Liveness::Untested => {}
            Liveness::Live => {
                tested += 1;
                live += 1;
            }
            Liveness::Inert => tested += 1,
        }
    }
    ui.label(format!(
        "{} frames observed. {} of {WRAM_LEN} addresses changed.",
        state.frames,
        WRAM_LEN - counts[behaviour_index(Behaviour::Untouched)]
    ));
    ui.weak(format!(
        "tick {} · rising {} · falling {} · sparse {} · volatile {} · untouched {}",
        counts[behaviour_index(Behaviour::FrameTick)],
        counts[behaviour_index(Behaviour::RisingCounter)],
        counts[behaviour_index(Behaviour::FallingCounter)],
        counts[behaviour_index(Behaviour::Sparse)],
        counts[behaviour_index(Behaviour::Volatile)],
        counts[behaviour_index(Behaviour::Untouched)],
    ));
    // Stated as a fraction of the whole, so "we verified 16" cannot read as "we
    // verified the atlas".
    ui.weak(format!(
        "verified {tested} of {WRAM_LEN} through {} — {live} live, {} inert, \
         {} untested",
        lens_name(state.lens),
        tested - live,
        WRAM_LEN - tested
    ));
}

/// The per-address rows, virtualized, with the detail pane below them.
///
/// `show_rows` renders only the visible slice. With "hide untouched" off this
/// list is all [`WRAM_LEN`] addresses, and building two thousand selectable
/// labels every frame is a cost the panel has no reason to pay. (PR #392 review.)
///
/// Virtualization requires a uniform row height, which is why the expanded
/// evidence sits in a fixed pane BELOW the scroll area rather than inline under
/// its row. That is the better arrangement anyway: the detail no longer shifts
/// the rows around it when opened, and it stays visible while scrolling.
fn table(ui: &mut egui::Ui, state: &mut AtlasPanel, can_run: bool) {
    // No per-frame allocation. Review suggested caching a filtered `Vec<Label>`
    // and invalidating it when `labels` or the filter changes; this avoids the
    // allocation *without* adding cache state to keep in sync — and stale derived
    // state is the defect class this release has already produced three times.
    //
    // `show_rows` needs a count up front, so the filter is walked twice: once to
    // count (2,048 predicate evaluations at worst, no allocation) and once inside
    // the closure, where `skip`/`take` bound the work to the visible slice.
    // (PR #392 review.)
    let hide_untouched = state.hide_untouched;
    let visible = |l: &&Label| !(hide_untouched && l.behaviour == Behaviour::Untouched);
    let row_count = state.labels.iter().filter(visible).count();
    if row_count == 0 {
        ui.weak("No addresses changed during the window.");
        return;
    }

    let row_h = ui.text_style_height(&egui::TextStyle::Monospace);
    // Copied out so the row loop can mutate `state.selected` without holding a
    // borrow of `state.labels`; `Label` is `Copy` and the slice is one screenful.
    let mut clicked: Option<u16> = None;
    let labels = &state.labels;
    let selected_addr = state.selected;
    egui::ScrollArea::vertical()
        .max_height(240.0)
        .show_rows(ui, row_h, row_count, |ui, range| {
            let start = range.start;
            let len = range.len();
            for l in labels.iter().filter(visible).skip(start).take(len) {
                let selected = selected_addr == Some(l.addr);
                let text = format!(
                    "${:04X}  {:<15} {:<9} {}",
                    l.addr,
                    behaviour_name(l.behaviour),
                    liveness_name(l.liveness),
                    short_evidence(l),
                );
                if ui
                    .selectable_label(selected, egui::RichText::new(text).monospace())
                    .clicked()
                {
                    clicked = Some(l.addr);
                }
            }
        });
    if let Some(addr) = clicked {
        state.selected = if selected_addr == Some(addr) {
            None
        } else {
            Some(addr)
        };
    }

    // The detail pane. Resolved from `rows` rather than `state.labels` so a
    // selection hidden by the current filter stops being shown, instead of
    // lingering as evidence for a row the user can no longer see.
    // Resolved against the FILTERED set, so a selection hidden by the current
    // filter stops showing evidence for a row the user cannot see.
    if let Some(addr) = state.selected
        && let Some(l) = state
            .labels
            .iter()
            .filter(visible)
            .find(|l| l.addr == addr)
            .copied()
    {
        ui.separator();
        detail(ui, state, &l, can_run);
    }
}

/// The expanded evidence for one address, plus its Verify button.
fn detail(ui: &mut egui::Ui, state: &mut AtlasPanel, l: &Label, can_run: bool) {
    ui.indent(l.addr, |ui| {
        let s = &l.stats;
        ui.weak(format!(
            "changed {} times · {} up, {} down, {} wraps · range {:#04X}-{:#04X} · \
             {} distinct values",
            s.changes, s.increases, s.decreases, s.wraps, s.min, s.max, s.distinct
        ));
        if let Some(f) = s.first_change {
            ui.weak(format!("first changed on frame {f}"));
        }
        // The threshold that decided it, so the label can be disagreed with.
        ui.weak(why(l, state.frames));

        match l.liveness {
            Liveness::Live => {
                let at = l
                    .divergence_frame
                    .map_or_else(|| "unknown frame".to_owned(), |f| format!("frame {f}"));
                ui.label(format!(
                    "VERIFIED LIVE through {}: changing this byte changed the \
                     observed output at {at}.",
                    lens_name(state.lens)
                ));
                ui.weak(
                    "This does NOT identify what the byte is — only that it \
                     participates in what the lens observes.",
                );
            }
            Liveness::Inert => {
                ui.label(format!(
                    "VERIFIED INERT through {}: changing this byte changed nothing \
                     the lens observed within {VERIFY_FRAMES} frames.",
                    lens_name(state.lens)
                ));
                ui.weak(
                    "Not the same as unused. A byte the game rewrites from a master \
                     copy each frame reads inert because the change is overwritten \
                     before it can matter.",
                );
            }
            Liveness::Untested => {
                ui.weak("Not verified. Behaviour above is a hypothesis from observation only.");
                if ui
                    .add_enabled(can_run, egui::Button::new("Verify this address"))
                    .clicked()
                {
                    state.verify_requested = Some(l.addr);
                }
            }
        }

        // Offered for EVERY address, including `Untested` and `Inert`.
        //
        // Restricting the export to verified-live addresses would be the
        // paternalistic reading of this panel's honesty rule. The rule is that a
        // claim must carry its evidence, not that unverified addresses are
        // unusable — an `Untested` sparse byte is a perfectly good thing to watch
        // while forming a hypothesis, and `Inert` is documented here as NOT
        // meaning unused. What the rule requires is that the exported entry say
        // which of the three it was, which `watch_label` does.
        //
        // Not gated on `can_run` either: this writes to another panel's list and
        // never touches the emulator, so the netplay / TAS / hardcore predicate
        // that gates Observe and Verify does not apply to it.
        if ui
            .button(ic(glyph::EYE, "Send to RAM Watch"))
            .on_hover_text(
                "Adds this address to Tools -> Analysis -> Memory Compare's RAM \
                 Watch list, labelled with the atlas verdict that produced it.",
            )
            .clicked()
        {
            state.watch_requested = Some((l.addr, watch_label(l, state.lens)));
        }
    });
}

/// The label an exported address carries into RAM Watch.
///
/// Three things, in the order a reader needs them: the address, the behavioural
/// class, and the verification verdict **with its lens**. The lens is not
/// decoration — liveness is relative to the observable, so "live" without it is
/// the over-claim the atlas exists to avoid, and the watch list is precisely
/// where that claim would outlive the panel that qualified it.
///
/// `Untested` is spelled out rather than omitted. An entry with no verdict and
/// an entry that was never tested look the same once the atlas is gone.
fn watch_label(l: &Label, lens: Observable) -> String {
    let verdict = match l.liveness {
        Liveness::Untested => "unverified".to_owned(),
        Liveness::Live => format!("LIVE via {}", lens_name(lens)),
        Liveness::Inert => format!("inert via {}", lens_name(lens)),
    };
    format!(
        "${:04X} atlas: {}, {verdict}",
        l.addr,
        behaviour_name(l.behaviour)
    )
}

/// The threshold that produced this label, in words.
fn why(l: &Label, frames: u32) -> String {
    let transitions = frames.saturating_sub(1);
    match l.behaviour {
        Behaviour::Untouched => "held one value for the whole window".to_owned(),
        Behaviour::FrameTick => format!(
            "changed on {} of {} transitions, at or above the {:.0}% frame-tick \
             threshold",
            l.stats.changes,
            transitions,
            FRAME_TICK_RATIO * 100.0
        ),
        Behaviour::RisingCounter => {
            format!("{} steps up and none down", l.stats.increases)
        }
        Behaviour::FallingCounter => {
            format!("{} steps down and none up", l.stats.decreases)
        }
        Behaviour::Sparse => format!(
            "changed {} times, at or below the sparse threshold of {SPARSE_MAX_CHANGES}",
            l.stats.changes
        ),
        Behaviour::Volatile => format!(
            "changed {} times in both directions, below the frame-tick threshold",
            l.stats.changes
        ),
    }
}

/// One-line evidence for the collapsed row.
fn short_evidence(l: &Label) -> String {
    if l.behaviour == Behaviour::Untouched {
        return format!("={:#04X}", l.stats.min);
    }
    format!(
        "{}ch {:#04X}-{:#04X}",
        l.stats.changes, l.stats.min, l.stats.max
    )
}

const fn behaviour_index(b: Behaviour) -> usize {
    match b {
        Behaviour::Untouched => 0,
        Behaviour::FrameTick => 1,
        Behaviour::RisingCounter => 2,
        Behaviour::FallingCounter => 3,
        Behaviour::Sparse => 4,
        Behaviour::Volatile => 5,
    }
}

const fn behaviour_name(b: Behaviour) -> &'static str {
    match b {
        Behaviour::Untouched => "untouched",
        Behaviour::FrameTick => "frame tick",
        Behaviour::RisingCounter => "rising",
        Behaviour::FallingCounter => "falling",
        Behaviour::Sparse => "sparse",
        Behaviour::Volatile => "volatile",
    }
}

const fn liveness_name(l: Liveness) -> &'static str {
    match l {
        Liveness::Untested => "untested",
        Liveness::Live => "LIVE",
        Liveness::Inert => "inert",
    }
}

const fn lens_name(o: Observable) -> &'static str {
    match o {
        Observable::Framebuffer => "screen",
        Observable::IndexFramebuffer => "screen (indices)",
        Observable::Wram => "work RAM",
        Observable::AudioEnergy => "audio",
    }
}

/// Run whichever action the body requested, against the live emulator.
fn run_actions(state: &mut AtlasPanel, nes: Option<&mut Nes>) {
    // Observe is checked FIRST and the verification flags are left in place if it
    // wins, rather than being taken unconditionally. Taking them up front
    // discarded a Verify click that landed in the same frame as an Observe — and
    // silently, since the request simply vanished. It now survives to the next
    // frame. (PR #392 review.)
    //
    // A fresh observation replaces every label, so a verification queued against
    // the OLD labels must not then be applied to the new ones; the observe branch
    // clears them explicitly for that reason, which is a different thing from
    // dropping them by accident.
    let observe = std::mem::take(&mut state.observe_requested);
    if !observe && state.verify_requested.is_none() && !state.verify_batch_requested {
        return;
    }
    let Some(nes) = nes else {
        "No ROM loaded.".clone_into(&mut state.status);
        return;
    };

    if observe {
        do_observe(state, nes);
        // An observation invalidates any prior verification: the labels are new
        // objects, and carrying old verdicts across would attach a verdict to a
        // hypothesis it was not tested against. Dropped deliberately here, having
        // been preserved above.
        state.verify_requested = None;
        state.verify_batch_requested = false;
        return;
    }

    let single = state.verify_requested.take();
    // Cleared whether or not it is used: the branch below is chosen by `single`.
    state.verify_batch_requested = false;
    let targets: Vec<u16> = if let Some(addr) = single {
        vec![addr]
    } else {
        state
            .labels
            .iter()
            .filter(|l| l.liveness == Liveness::Untested && l.behaviour != Behaviour::Untouched)
            .take(VERIFY_BATCH)
            .map(|l| l.addr)
            .collect()
    };
    if targets.is_empty() {
        "Nothing left to verify.".clone_into(&mut state.status);
        return;
    }
    do_verify(state, nes, &targets);
}

/// Restores a snapshot into the live emulator on drop, including on an unwind.
///
/// Both actions here advance the live `Nes` by hundreds of frames and put it back
/// afterwards. A plain restore at the end of the function is correct on the happy
/// path and wrong on a panic: the user would be left mid-analysis, several hundred
/// frames from where they were, with no indication why. `Drop` cannot return a
/// `Result`, so the unwind path is best-effort — but best-effort recovery beats
/// none, and the success path still checks. (PR #392 review.)
struct TimelineGuard<'a> {
    nes: &'a mut Nes,
    snapshot: Vec<u8>,
    restored: bool,
    /// v2.3.7 — the pixel- and audio-provenance stores, held across the restore.
    ///
    /// The same reasoning as `rustynes_probe::TrialGuard`, and it has to be
    /// repeated here because this guard owns its own restore: both stores live
    /// OUTSIDE the save state, and `Nes::restore_quiet` clears them, so putting
    /// the timeline back also emptied whatever the Pixel Provenance and Audio
    /// Provenance panels had accumulated. Cumulative attribution is not
    /// rebuilt by the next frame — a palette byte or a `$4008` write from level
    /// load does not happen again — so the loss was permanent for the session.
    #[cfg(feature = "debug-hooks")]
    provenance: (
        rustynes_core::rustynes_ppu::ProvenanceStash,
        rustynes_core::rustynes_apu::provenance::AudioProvenanceStash,
    ),
}

impl<'a> TimelineGuard<'a> {
    fn capture(nes: &'a mut Nes) -> Self {
        let snapshot = nes.snapshot();
        #[cfg(feature = "debug-hooks")]
        let provenance = (nes.take_provenance(), nes.take_audio_provenance());
        Self {
            nes,
            snapshot,
            restored: false,
            #[cfg(feature = "debug-hooks")]
            provenance,
        }
    }

    /// Restore on the success path, where a failure can still be surfaced.
    fn restore(&mut self) {
        self.nes
            .restore_quiet(&self.snapshot)
            .expect("a snapshot taken from this instance restores to it");
        self.restored = true;
    }
}

impl Drop for TimelineGuard<'_> {
    fn drop(&mut self) {
        if !self.restored {
            // Unwind path only. Cannot report, so it must not panic here either —
            // a panic during unwinding aborts.
            let _ = self.nes.restore_quiet(&self.snapshot);
        }
        // AFTER any restore, on both paths, so the stores are put back into a
        // timeline that has already stopped being rewritten.
        #[cfg(feature = "debug-hooks")]
        {
            // `mem::take` because `Drop` has only `&mut self`. The guard is
            // being destroyed, so leaving default stashes behind is invisible —
            // the same pattern, for the same reason, as `TrialGuard::drop`.
            let (pixel, audio) = core::mem::take(&mut self.provenance);
            self.nes.put_provenance(pixel);
            self.nes.put_audio_provenance(audio);
        }
    }
}

/// Observe and classify, restoring the live timeline afterwards.
fn do_observe(state: &mut AtlasPanel, nes: &mut Nes) {
    let mut guard = TimelineGuard::capture(nes);
    // Idle input: an atlas taken while a button is held describes a different
    // situation, which is a useful thing to offer later but a confusing default.
    let obs: Observation = atlas::observe(guard.nes, OBSERVE_FRAMES, |_| {
        (Buttons::empty(), Buttons::empty())
    });
    // `restore_quiet`, not `restore`: this is the same timeline, snapshotted
    // moments ago on this instance, so the rewind ring must survive.
    guard.restore();

    state.labels = atlas::classify(&obs);
    state.frames = obs.frames();
    state.selected = None;
    state.status = format!(
        "Observed {OBSERVE_FRAMES} frames. Nothing is verified yet — every \
         behaviour below is a hypothesis."
    );
}

/// Verify the given addresses, restoring the live timeline afterwards.
fn do_verify(state: &mut AtlasPanel, nes: &mut Nes, targets: &[u16]) {
    let mut guard = TimelineGuard::capture(nes);
    // Two trials per address, plus nothing else: sized exactly, so a budget that
    // runs out is a bug in this arithmetic rather than a silent truncation.
    let budget = Budget {
        max_frames_per_trial: VERIFY_FRAMES,
        max_trials: u32::try_from(targets.len() * TRIALS_PER_ADDRESS).unwrap_or(u32::MAX),
    };
    let mut probe = Probe::anchor(&*guard.nes, budget);

    let mut live = 0usize;
    let mut inert = 0usize;
    let mut untested = 0usize;
    for &addr in targets {
        let (liveness, frame) =
            atlas::verify_liveness(&mut probe, guard.nes, addr, VERIFY_FRAMES, state.lens);
        match liveness {
            Liveness::Live => live += 1,
            Liveness::Inert => inert += 1,
            Liveness::Untested => untested += 1,
        }
        // Direct index, not a linear scan: `classify` emits one label per
        // address over `0..WRAM_LEN` in order, so the index IS the address. The
        // assertion keeps that from being a silent assumption — if the layout
        // ever changes, this fails loudly instead of writing a verdict onto the
        // wrong address. (PR #392 review.)
        if let Some(l) = state.labels.get_mut(addr as usize) {
            debug_assert_eq!(
                l.addr, addr,
                concat!(
                    "atlas labels are no longer address-indexed; ",
                    "a verdict would have been recorded against the wrong address"
                )
            );
            l.liveness = liveness;
            l.divergence_frame = frame;
        }
    }

    guard.restore();

    // The untested count is reported rather than folded into "inert", so a
    // budget shortfall is visible instead of looking like a finding.
    state.status = if untested > 0 {
        format!(
            "Verified {} through {}: {live} live, {inert} inert. {untested} could \
             not be tested within the budget.",
            targets.len() - untested,
            lens_name(state.lens)
        )
    } else {
        format!(
            "Verified {} through {}: {live} live, {inert} inert.",
            targets.len(),
            lens_name(state.lens)
        )
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustynes_probe::atlas::AddressStats;

    fn label(addr: u16, behaviour: Behaviour, liveness: Liveness) -> Label {
        Label {
            addr,
            behaviour,
            stats: AddressStats::default(),
            liveness,
            divergence_frame: None,
        }
    }

    /// The exported label must name the LENS a verdict came from. Liveness is
    /// relative to the observable — a byte is `Live` through work RAM and may be
    /// `Inert` through the screen — and the watch list is exactly where an
    /// unqualified "LIVE" would outlive the panel that qualified it.
    #[test]
    fn an_exported_verdict_carries_its_lens() {
        let l = label(0x0071, Behaviour::RisingCounter, Liveness::Live);
        let text = watch_label(&l, Observable::Wram);
        assert!(text.contains("0071"), "the address is missing: {text}");
        assert!(text.contains("rising"), "the behaviour is missing: {text}");
        assert!(text.contains("LIVE"), "the verdict is missing: {text}");
        assert!(
            text.contains(lens_name(Observable::Wram)),
            "the lens is missing, so the verdict over-claims: {text}"
        );
        // The same label under a different lens must READ differently. Naming
        // the lens is only worth anything if the name tracks the argument.
        assert_ne!(
            text,
            watch_label(&l, Observable::Framebuffer),
            "the lens name did not change with the lens"
        );
    }

    /// `Untested` is spelled out rather than left blank. Once the atlas is gone,
    /// an entry with no verdict and an entry that was never tested look the same,
    /// which is the exact collapse this panel refuses to make elsewhere.
    #[test]
    fn an_unverified_export_says_so() {
        let l = label(0x0300, Behaviour::Sparse, Liveness::Untested);
        let text = watch_label(&l, Observable::Framebuffer);
        assert!(
            text.contains("unverified"),
            "an untested address exported without saying so: {text}"
        );
        assert!(
            !text.contains("LIVE"),
            "an untested address claimed a verdict: {text}"
        );
        // And it must NOT name a lens: nothing was observed through one, so
        // citing one would dress a hypothesis as a measurement.
        assert!(
            !text.contains(lens_name(Observable::Framebuffer)),
            "an untested address cited a lens it never used: {text}"
        );
    }

    /// The request is TAKEN, so one click seeds one entry. Left standing it would
    /// re-fire every frame until the destination happened to de-duplicate it,
    /// which makes this panel's behaviour depend on another panel's contents.
    #[test]
    fn a_watch_request_is_drained_by_the_first_taker() {
        let mut p = AtlasPanel {
            watch_requested: Some((0x0071, "atlas".to_owned())),
            ..AtlasPanel::default()
        };
        assert_eq!(p.take_watch_request(), Some((0x0071, "atlas".to_owned())));
        assert_eq!(
            p.take_watch_request(),
            None,
            "the request survived being taken and would seed a second entry"
        );
    }

    /// A ROM change must discard a pending export too. An address is a fact about
    /// one cartridge's memory map; seeding it into a watch list after a different
    /// game has loaded is the same class of stale claim as keeping the labels.
    #[test]
    fn clearing_discards_a_pending_export() {
        let mut p = AtlasPanel {
            watch_requested: Some((0x0071, "atlas".to_owned())),
            status: "stale".to_owned(),
            ..AtlasPanel::default()
        };
        p.clear();
        assert_eq!(p.take_watch_request(), None);
        assert!(p.status.is_empty());
    }

    /// A ROM change must discard the whole atlas. Two thousand labels describing
    /// a cartridge that is no longer loaded is a worse lie than one stale number,
    /// because it looks like a map.
    #[test]
    fn clearing_discards_the_whole_atlas() {
        let mut p = AtlasPanel {
            labels: vec![label(0, Behaviour::FrameTick, Liveness::Live)],
            frames: 180,
            selected: Some(0),
            status: "stale".to_owned(),
            ..AtlasPanel::default()
        };
        p.clear();
        assert!(p.labels.is_empty());
        assert_eq!(p.frames, 0);
        assert_eq!(p.selected, None);
        assert!(p.status.is_empty());
    }

    /// A batch must never spend trials on untouched addresses: they are the bulk
    /// of work RAM, and perturbing a byte the game never reads is the one case
    /// guaranteed to be uninformative.
    #[test]
    fn a_batch_skips_untouched_and_already_tested_addresses() {
        let labels = [
            label(0x00, Behaviour::Untouched, Liveness::Untested),
            label(0x01, Behaviour::FrameTick, Liveness::Untested),
            label(0x02, Behaviour::Volatile, Liveness::Live),
            label(0x03, Behaviour::RisingCounter, Liveness::Untested),
        ];
        let picked: Vec<u16> = labels
            .iter()
            .filter(|l| l.liveness == Liveness::Untested && l.behaviour != Behaviour::Untouched)
            .take(VERIFY_BATCH)
            .map(|l| l.addr)
            .collect();
        assert_eq!(
            picked,
            [0x01, 0x03],
            "a batch selected an untouched or already-verified address"
        );
    }

    /// The batch is bounded. An unbounded "verify everything" is 4,096 trials and
    /// tens of minutes of a frozen UI.
    #[test]
    fn a_batch_is_bounded() {
        let labels: Vec<Label> = (0..500u16)
            .map(|a| label(a, Behaviour::Volatile, Liveness::Untested))
            .collect();
        let picked = labels
            .iter()
            .filter(|l| l.liveness == Liveness::Untested && l.behaviour != Behaviour::Untouched)
            .take(VERIFY_BATCH)
            .count();
        assert_eq!(picked, VERIFY_BATCH);
    }

    /// `Untested` must have its own rendering. Sharing a string with `Inert`
    /// would present "we did not look" as "we looked and saw nothing".
    #[test]
    fn untested_is_named_distinctly_from_inert() {
        assert_ne!(
            liveness_name(Liveness::Untested),
            liveness_name(Liveness::Inert)
        );
        assert_ne!(
            liveness_name(Liveness::Live),
            liveness_name(Liveness::Inert)
        );
    }

    /// Every behaviour needs a distinct summary slot, or the counts silently
    /// merge two classes.
    #[test]
    fn behaviour_indices_are_distinct() {
        let all = [
            Behaviour::Untouched,
            Behaviour::FrameTick,
            Behaviour::RisingCounter,
            Behaviour::FallingCounter,
            Behaviour::Sparse,
            Behaviour::Volatile,
        ];
        let mut seen = [false; 6];
        for b in all {
            let i = behaviour_index(b);
            assert!(!seen[i], "two behaviours share summary slot {i}");
            seen[i] = true;
        }
        assert!(seen.iter().all(|&s| s), "a summary slot is unused");
    }

    /// The explanation must name the threshold that decided the label, so a
    /// reader can disagree with it.
    #[test]
    fn the_explanation_cites_its_threshold() {
        let mut l = label(0, Behaviour::Sparse, Liveness::Untested);
        l.stats.changes = 2;
        let text = why(&l, 180);
        assert!(
            text.contains(&SPARSE_MAX_CHANGES.to_string()),
            "the sparse explanation did not cite its threshold: {text}"
        );
    }
}
