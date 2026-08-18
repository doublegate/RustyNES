// SPDX-License-Identifier: GPL-3.0-or-later
//! Audio provenance (v2.3.7 "Overtone") — why does this moment sound like that?
//!
//! The APU analogue of the PPU's `rustynes_ppu::provenance` (a plain code span,
//! not an intra-doc link: `rustynes-apu` does not depend on `rustynes-ppu`, and a
//! bracketed link to a crate outside the graph fails `RUSTDOCFLAGS=-D warnings`
//! while clippy stays green), and deliberately
//! built to the same shape: a **register-attribution** half that answers "what
//! wrote this, and from which instruction", and a **per-cycle mix trace** that
//! answers "what were the channels actually doing".
//!
//! # What was missing before this
//!
//! Every other ingredient already shipped. The frontend's audio scope plots the
//! per-channel waveforms, the audio mixer exposes per-channel gain,
//! [`crate::Apu::pulse1_out`] and its siblings expose live channel outputs, and
//! the trace logger has PC and cycle. What did not exist anywhere is the **link
//! between a sample and the instruction that caused it** — exactly the gap the
//! pixel-provenance design identified for video.
//!
//! # Cadence, and why it is per CPU cycle rather than per output sample
//!
//! The mix is computed **once per CPU cycle** (1.789 MHz NTSC) and handed to the
//! band-limited `blip` decimator, which produces output samples at 44.1 kHz —
//! about one per 40.6 CPU cycles. Recording at *output* rate would therefore
//! require choosing which of those 40 mixes "is" the sample, and band-limited
//! synthesis makes that choice ill-posed: an output sample is a weighted sum of
//! transitions across the filter kernel, not a copy of one instant.
//!
//! So this records what was genuinely mixed, at the cadence it was mixed. The
//! panel maps a clicked output sample back to its CPU-cycle window; the doc says
//! plainly that the window is a kernel width, not a point. **A provenance tool
//! that answers a question it cannot actually answer is worse than one that
//! declines** — the whole argument v2.3.6 was built on.
//!
//! The cost is smaller than it sounds: 29,781 records per NTSC frame against the
//! pixel store's 61,440 — **0.48x the record count of the video side**.
//!
//! # Determinism
//!
//! Output-only. Nothing here is read back into synthesis, none of it is part of
//! the save state, and every store is behind a runtime arm that is off by
//! default. With the arm off the cost is one `Option` discriminant test per
//! register write and per mixed cycle.

use alloc::boxed::Box;
use alloc::vec::Vec;

/// First APU/IO register address covered by [`RegisterAttribution`].
pub const REG_BASE: u16 = 0x4000;

/// Number of register slots tracked: `$4000-$4017` inclusive.
///
/// `$4014` (OAM DMA) and `$4016` (controller strobe) are inside the range and
/// are NOT APU registers. They are tracked anyway rather than punched out: the
/// range is what the bus already classifies as [`EventKind::ApuWrite`], keeping
/// one contiguous index space costs two slots, and a hole would be a permanent
/// invitation to off-by-one arithmetic at every call site.
///
/// [`EventKind::ApuWrite`]: https://docs.rs/rustynes-core
pub const REG_COUNT: usize = 0x18;

/// [`REG_COUNT`] as a `u16`, so address arithmetic never needs a cast.
pub const REG_COUNT_U16: u16 = 0x18;

/// Largest CPU-cycle count in one frame across every supported region, which is
/// what [`MixTrace`] is sized for.
///
/// Dendy is the worst case, not NTSC: 1,773,448 Hz / 50.007 fps = **35,464**
/// cycles per frame, against PAL's 33,247 and NTSC's 29,781. Sizing this from
/// the NTSC number — the one that comes to mind first — would silently truncate
/// the last 16% of every Dendy frame.
pub const MIX_CAP: usize = 36_864;

// ---------------------------------------------------------------------------
// Register attribution — "what wrote $4003, and from where?"
// ---------------------------------------------------------------------------

/// One register write: the byte, the CPU cycle, and the instruction that did it.
///
/// `cycle` is the CPU cycle counter at the writing instruction, which is what
/// makes a record comparable against the Trace Logger and the Event Viewer for
/// the same frame. Mirrors `rustynes_ppu::provenance::WriteAttrib`, which
/// carries the same three fields for VRAM/OAM/palette bytes.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct RegWrite {
    /// CPU cycle count at the writing instruction.
    pub cycle: u64,
    /// Program counter of the writing instruction.
    pub pc: u16,
    /// The byte written, as the CPU put it on the bus.
    pub value: u8,
}

/// Storage-side mirror of [`RegWrite`] with a `Default`, so a slot array can be
/// built without inventing a meaningless public default PC.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
struct RegWriteInner {
    cycle: u64,
    pc: u16,
    value: u8,
}

/// One attribution slot plus whether anything has been written yet.
///
/// A `written` flag rather than a sentinel cycle, for the reason the PPU side
/// documents: **cycle 0 is a legitimate value** — the reset sequence performs
/// real writes — so a sentinel would silently misreport the earliest writes in a
/// run as "never written".
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
struct Slot {
    rec: RegWriteInner,
    written: bool,
}

impl Slot {
    const fn get(self) -> Option<RegWrite> {
        if self.written {
            Some(RegWrite {
                cycle: self.rec.cycle,
                pc: self.rec.pc,
                value: self.rec.value,
            })
        } else {
            None
        }
    }
}

/// Last write to each of `$4000-$4017`, with its cause.
///
/// **Last write, not a history.** One slot per address rather than a ring,
/// because the question this half answers is "what is the register holding, and
/// who put it there" — and a ring would need a retention policy nobody has a
/// principled value for. The Event Viewer already keeps the per-frame write
/// *sequence*; this keeps the per-register *cause*, which it does not.
#[derive(Clone, Debug)]
pub struct RegisterAttribution {
    slots: [Slot; REG_COUNT],
}

impl RegisterAttribution {
    /// A fresh table with every slot unwritten.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            slots: [Slot {
                rec: RegWriteInner {
                    cycle: 0,
                    pc: 0,
                    value: 0,
                },
                written: false,
            }; REG_COUNT],
        }
    }

    /// Forget every recorded write.
    pub fn clear(&mut self) {
        *self = Self::new();
    }

    /// Record a write to `addr`. Addresses outside `$4000-$4017` are dropped
    /// rather than wrapping into an unrelated slot.
    pub const fn record(&mut self, addr: u16, pc: u16, cycle: u64, value: u8) {
        let Some(idx) = Self::index(addr) else {
            return;
        };
        self.slots[idx] = Slot {
            rec: RegWriteInner { cycle, pc, value },
            written: true,
        };
    }

    /// The last write to `addr`, or `None` if the address is out of range or
    /// nothing has written it since the last [`Self::clear`].
    #[must_use]
    pub const fn get(&self, addr: u16) -> Option<RegWrite> {
        match Self::index(addr) {
            Some(idx) => self.slots[idx].get(),
            None => None,
        }
    }

    /// Map a register address to a slot index, or `None` when out of range.
    const fn index(addr: u16) -> Option<usize> {
        if addr < REG_BASE {
            return None;
        }
        let idx = (addr - REG_BASE) as usize;
        if idx < REG_COUNT { Some(idx) } else { None }
    }
}

impl Default for RegisterAttribution {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Mix trace — "what were the channels doing?"
// ---------------------------------------------------------------------------

/// The five channel outputs that went into one mixed CPU-cycle sample, plus the
/// result.
///
/// Channel values are the raw pre-mix outputs each channel presented — 0-15 for
/// the two pulses, the triangle and the noise, 0-127 for the DMC — which is what
/// the non-linear mixer consumes. They are NOT scaled by the frontend's mixer
/// gains: those are a presentation control, and recording post-gain values would
/// make the record describe the user's slider rather than the chip.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MixRecord {
    /// The mixed sample handed to the band-limited decimator, including any
    /// expansion audio.
    pub mixed: f32,
    /// The expansion-audio contribution (`0.0` on a cartridge without one).
    pub external: f32,
    /// Pulse 1 output, 0-15.
    pub pulse1: u8,
    /// Pulse 2 output, 0-15.
    pub pulse2: u8,
    /// Triangle output, 0-15.
    pub triangle: u8,
    /// Noise output, 0-15.
    pub noise: u8,
    /// DMC output, 0-127.
    pub dmc: u8,
}

impl MixRecord {
    /// Which channel contributed most to this sample, as an index into the
    /// conventional order (0 = pulse 1 … 4 = DMC), or `None` when every channel
    /// is silent.
    ///
    /// Compares each channel's share of the **non-linear** mixer rather than its
    /// raw value, because the raw values are not commensurable: a DMC 127 and a
    /// pulse 15 are both "full scale" on different scales. The comparison is
    /// therefore on normalised share, which is the only ordering that answers
    /// the question a user is actually asking.
    #[must_use]
    pub fn dominant(&self) -> Option<usize> {
        let shares = [
            f32::from(self.pulse1) / 15.0,
            f32::from(self.pulse2) / 15.0,
            f32::from(self.triangle) / 15.0,
            f32::from(self.noise) / 15.0,
            f32::from(self.dmc) / 127.0,
        ];
        let mut best = None;
        let mut best_share = 0.0f32;
        for (i, &s) in shares.iter().enumerate() {
            if s > best_share {
                best_share = s;
                best = Some(i);
            }
        }
        best
    }
}

/// One frame's worth of per-CPU-cycle mix records.
///
/// The index **is** the cycle offset from [`Self::first_cycle`], so no per-record
/// timestamp is stored — that is what keeps the record at 16 bytes and the frame
/// at ~465 KiB.
#[derive(Clone, Debug)]
pub struct MixTrace {
    recs: Vec<MixRecord>,
    first_cycle: u64,
    /// Set when a frame produced more cycles than [`MIX_CAP`] and records were
    /// dropped. Surfaced rather than silent: a truncated trace that looks
    /// complete is the failure mode this whole subsystem exists to avoid.
    truncated: bool,
}

impl MixTrace {
    /// An empty trace with capacity for the worst-case region.
    #[must_use]
    pub fn new() -> Self {
        Self {
            recs: Vec::with_capacity(MIX_CAP),
            first_cycle: 0,
            truncated: false,
        }
    }

    /// Drop every record and re-anchor the trace at `first_cycle`.
    pub fn clear(&mut self, first_cycle: u64) {
        self.recs.clear();
        self.first_cycle = first_cycle;
        self.truncated = false;
    }

    /// Append one mixed cycle. Beyond [`MIX_CAP`] the record is dropped and the
    /// trace is flagged [`Self::truncated`].
    pub fn push(&mut self, rec: MixRecord) {
        if self.recs.len() >= MIX_CAP {
            self.truncated = true;
            return;
        }
        self.recs.push(rec);
    }

    /// CPU cycle the first record corresponds to.
    #[must_use]
    pub const fn first_cycle(&self) -> u64 {
        self.first_cycle
    }

    /// Whether records were dropped for exceeding [`MIX_CAP`].
    #[must_use]
    pub const fn truncated(&self) -> bool {
        self.truncated
    }

    /// Every record, oldest first.
    #[must_use]
    pub fn records(&self) -> &[MixRecord] {
        &self.recs
    }

    /// The record for absolute CPU `cycle`, or `None` if it is outside the
    /// trace.
    #[must_use]
    pub fn at_cycle(&self, cycle: u64) -> Option<MixRecord> {
        let idx = usize::try_from(cycle.checked_sub(self.first_cycle)?).ok()?;
        self.recs.get(idx).copied()
    }
}

impl Default for MixTrace {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Run-ahead carry
// ---------------------------------------------------------------------------

/// Both audio stores, lifted out of the APU so a same-timeline restore can put
/// them back.
///
/// **This exists because of a shipped bug, not a hypothetical one.** Pixel
/// Provenance was non-functional from v2.3.2 to v2.3.6 because run-ahead's
/// per-frame rollback cleared the provenance store *after* the visible frame was
/// produced and *before* the UI could read it — so the panel could never observe
/// a populated record, and a comment two lines above the clear asserted the
/// opposite. Audio Provenance rides the identical rollback.
///
/// The frontend takes this before `restore_quiet` and puts it back after, which
/// leaves the restore's own reasoning completely intact: a save-state load and a
/// netplay rollback still clear, because those are genuine timeline changes.
/// Run-ahead's rollback is not — it returns to the timeline it just left.
#[derive(Debug, Default)]
pub struct AudioProvenanceStash {
    pub(crate) state: Option<Box<AudioProvenance>>,
}

impl AudioProvenanceStash {
    /// Whether the store was armed when this stash was taken, so the caller can
    /// skip the put-back on the common path where nothing is armed.
    #[must_use]
    pub const fn is_armed(&self) -> bool {
        self.state.is_some()
    }
}

/// Everything audio provenance owns, behind ONE pointer.
///
/// **Consolidated after measurement, not for tidiness.** The first shape put
/// four fields directly on `Apu` — two `Option<Box<..>>` plus the `u16`/`u64`
/// attribution context. That grew the struct on the hot path and the
/// `apu_throughput` bench read **+9%** on two of three workloads with the arm
/// OFF, which is the configuration the shipped frontend runs. One `Option<Box>`
/// costs eight bytes and one null test when disarmed, and everything else moves
/// behind the allocation where only an armed session pays for it.
#[derive(Clone, Debug)]
pub struct AudioProvenance {
    /// Last write to each of `$4000-$4017`, with its cause.
    pub reg_attrib: RegisterAttribution,
    /// This frame's per-CPU-cycle mix records.
    pub mix_trace: MixTrace,
    /// PC of the instruction currently executing, pushed down once per
    /// instruction by the core so `write_register` can attribute a write.
    pub attrib_pc: u16,
    /// CPU-cycle counterpart of [`Self::attrib_pc`].
    pub attrib_cycle: u64,
}

impl AudioProvenance {
    /// A freshly-armed store.
    #[must_use]
    pub fn new() -> Self {
        Self {
            reg_attrib: RegisterAttribution::new(),
            mix_trace: MixTrace::new(),
            attrib_pc: 0,
            attrib_cycle: 0,
        }
    }
}

impl Default for AudioProvenance {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unwritten_slots_report_none() {
        let a = RegisterAttribution::new();
        for addr in REG_BASE..REG_BASE + REG_COUNT_U16 {
            assert_eq!(a.get(addr), None, "{addr:#06X} should be unwritten");
        }
    }

    #[test]
    fn cycle_zero_is_a_real_write_not_a_sentinel() {
        // The reset sequence performs writes at cycle 0. A sentinel-cycle design
        // would report these as "never written".
        let mut a = RegisterAttribution::new();
        a.record(0x4000, 0, 0, 0);
        assert_eq!(
            a.get(0x4000),
            Some(RegWrite {
                cycle: 0,
                pc: 0,
                value: 0
            })
        );
    }

    #[test]
    fn out_of_range_addresses_are_dropped_not_wrapped() {
        let mut a = RegisterAttribution::new();
        a.record(0x3FFF, 0x1234, 9, 0xAA);
        a.record(0x4018, 0x1234, 9, 0xBB);
        a.record(0xFFFF, 0x1234, 9, 0xCC);
        for addr in REG_BASE..REG_BASE + REG_COUNT_U16 {
            assert_eq!(a.get(addr), None, "{addr:#06X} was written by a stray addr");
        }
        assert_eq!(a.get(0x4018), None);
    }

    #[test]
    fn last_write_wins_per_address_and_addresses_are_independent() {
        let mut a = RegisterAttribution::new();
        a.record(0x4003, 0x8000, 10, 0x11);
        a.record(0x4003, 0x8100, 20, 0x22);
        a.record(0x4007, 0x8200, 30, 0x33);
        assert_eq!(
            a.get(0x4003).map(|w| (w.pc, w.cycle, w.value)),
            Some((0x8100, 20, 0x22))
        );
        assert_eq!(
            a.get(0x4007).map(|w| (w.pc, w.cycle, w.value)),
            Some((0x8200, 30, 0x33))
        );
    }

    #[test]
    fn mix_trace_index_is_the_cycle_offset() {
        let mut t = MixTrace::new();
        t.clear(1_000);
        for i in 0..4u8 {
            t.push(MixRecord {
                pulse1: i,
                ..MixRecord::default()
            });
        }
        assert_eq!(t.at_cycle(1_000).map(|r| r.pulse1), Some(0));
        assert_eq!(t.at_cycle(1_003).map(|r| r.pulse1), Some(3));
        assert_eq!(t.at_cycle(999), None, "before the anchor");
        assert_eq!(t.at_cycle(1_004), None, "past the end");
    }

    #[test]
    fn truncation_is_reported_rather_than_silent() {
        let mut t = MixTrace::new();
        t.clear(0);
        for _ in 0..MIX_CAP {
            t.push(MixRecord::default());
        }
        assert!(!t.truncated(), "exactly at capacity is not truncation");
        t.push(MixRecord::default());
        assert!(t.truncated(), "over capacity must be visible to the caller");
        assert_eq!(t.records().len(), MIX_CAP);
    }

    #[test]
    fn mix_cap_covers_the_worst_case_region() {
        // Dendy, not NTSC, is the worst case: 1_773_448 / 50.007 = 35,464.
        // Sizing from the NTSC number would truncate 16% of every Dendy frame.
        // Integer arithmetic: a float cast here would trip the truncation
        // lint that this crate denies, and ceil-divide is the exact operation
        // the bound needs anyway.
        let dendy_cycles_per_frame = (1_773_448_000_usize).div_ceil(50_007);
        assert!(
            MIX_CAP >= dendy_cycles_per_frame,
            "MIX_CAP {MIX_CAP} < Dendy {dendy_cycles_per_frame}"
        );
    }

    #[test]
    fn dominant_compares_normalised_share_not_raw_value() {
        // DMC 100/127 (0.787) beats pulse 15/15? No — pulse is 1.0. The point is
        // that raw magnitude would pick the DMC, and share picks the pulse.
        let r = MixRecord {
            pulse1: 15,
            dmc: 100,
            ..MixRecord::default()
        };
        assert_eq!(r.dominant(), Some(0), "pulse at full scale must win");

        let r = MixRecord {
            pulse1: 8,
            dmc: 100,
            ..MixRecord::default()
        };
        assert_eq!(r.dominant(), Some(4), "DMC 0.787 beats pulse 0.533");

        assert_eq!(
            MixRecord::default().dominant(),
            None,
            "silence has no winner"
        );
    }

    #[test]
    fn a_stash_reports_unarmed_when_empty() {
        assert!(!AudioProvenanceStash::default().is_armed());
    }
}
