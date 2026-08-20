//! Rolling per-cycle hash checkpoints -- the rung-0 comparison surface.
//!
//! # Why a hash and not a trace
//!
//! The constraint nobody budgets for in co-simulation is trace *volume*, not
//! simulation time. A 4200-frame `AccuracyCoin` run is roughly 125 million CPU
//! cycles; as per-cycle CSV that is about **7.5 GB** per side, which is not a
//! diff, it is a disk-space problem wearing a diff's clothes. The same run as
//! 4096-cycle checkpoints is **244 KB**.
//!
//! So the protocol is **hash first, capture on divergence**: both sides chain a
//! 64-bit hash over the per-cycle observable tuple and emit it every
//! [`DEFAULT_INTERVAL`] cycles. A comparison of the two checkpoint streams
//! answers "do these agree, and if not, in which 4096-cycle window did they
//! stop agreeing" -- which is exactly the input a full-capture re-run needs, and
//! nothing more. The re-run then costs one window, not one campaign.
//!
//! # What is hashed, and what deliberately is not
//!
//! This is the load-bearing decision in the module, and it is a decision about
//! *hardware*, not about convenience.
//!
//! `rustynes-core`'s [`CycleRecord`] carries 29 fields. Most of them are
//! `RustyNES`'s **model-internal** state -- `dmc_abort_delay_post`,
//! `apu_phase_post`, `dma_cycles_owed` and the rest describe how *this*
//! emulator chose to represent DMC and DMA bookkeeping. They are not hardware
//! facts. Gating on them would force an independent implementation to
//! transliterate a Rust data structure, which is bad hardware and, given that
//! the whole programme is built on never reading a reference implementation, an
//! odd form of self-derivation.
//!
//! [`Observable`] is therefore the subset an external device-under-test can
//! genuinely produce, and every field in it has a reason:
//!
//! | field | why it is in |
//! |---|---|
//! | `cpu_cycle` | the axis both sides count on; a desync here is the finding |
//! | `bus_access` | the R/W and DMA character of the cycle -- pin-visible |
//! | `bus_addr`, `bus_data` | the address and data buses -- pin-visible |
//! | `put_cycle` | the phase half of the M2 cycle -- pin-visible |
//! | `nmi_line` | a pin |
//! | `irq_line_at_low`, `irq_line_at_high` | the /IRQ pin, sampled twice per cycle |
//! | `pc` | **not** pin-visible; see below |
//!
//! Two of those need their caveats stated rather than buried.
//!
//! **The IRQ line is one wire.** `CycleRecord` splits its IRQ samples into
//! `irq_pending_mapper_*` and `irq_pending_apu_*`, which is `RustyNES`
//! *attributing* the assertion to a source. Real hardware has a single
//! wire-OR'd /IRQ input and cannot make that distinction, so [`Observable`]
//! folds the pair with OR before hashing. Hashing them separately would fail a
//! correct DUT for disagreeing about something it has no way to observe.
//!
//! **`pc` is DUT-observable, not pin-observable.** The 6502 does not expose its
//! program counter; the value is recoverable only indirectly, from the address
//! bus during an opcode fetch. It is included because the testbench wrapper
//! *can* expose the internal register and because rung 1 compares it directly
//! -- but a `pc`-only mismatch means something weaker than a bus mismatch, and
//! [`Divergence`] reports which fields differ so the two are never confused.
//!
//! PPU fields (`ppu_scanline`, `ppu_dot`, `ppu_frame`, `a12_events`) are
//! excluded on purpose: they belong to the PPU rung and its own pre-palette
//! framebuffer gate, not to a CPU-level bus comparison. `a12_events` is the
//! sharpest case -- A12 transitions are genuinely observable on the cartridge
//! connector, so it is excluded for *scope*, not for observability, and it
//! becomes a gate when the PPU rung opens rather than being dropped.
//!
//! # The hash must be reimplementable in ten lines of C++
//!
//! The top risk at rung 0 is a **format-packing mismatch masquerading as an RTL
//! bug** -- the testbench packs a field differently, the streams disagree, and
//! the disagreement is read as a hardware defect. Two things guard against it.
//!
//! First, the hash is **FNV-1a 64**, chosen for exactly one property: a C++
//! testbench can reimplement it correctly from the constants below without a
//! library, and be checked against a fixed vector. Nothing here needs
//! cryptographic strength; it needs to be trivially portable and to detect a
//! flipped bit.
//!
//! Second, [`Observable::encode`] defines a **fixed 16-byte little-endian
//! layout** and `tests::the_wire_encoding_is_pinned_to_a_fixed_vector` pins it
//! to a hardcoded expected hash. If a future refactor reorders a field, that
//! test fails rather than the next co-simulation run reporting a phantom RTL
//! defect.
//!
//! [`CycleRecord`]: rustynes_core::irq_trace::CycleRecord

use rustynes_core::irq_trace::{BusAccess, CycleRecord};

/// FNV-1a 64-bit offset basis. Public because the C++ testbench needs it.
pub const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV-1a 64-bit prime. Public because the C++ testbench needs it.
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// Cycles between emitted checkpoints.
///
/// 4096 is a compromise with both ends measured rather than a round number:
/// a full `AccuracyCoin` run is ~125 M cycles, so this yields ~30 500
/// checkpoints (244 KB at 8 bytes each), while the window a divergence
/// localises to stays small enough that a full-capture re-run of it is
/// instant.
pub const DEFAULT_INTERVAL: u64 = 4096;

/// Bytes of the fixed wire encoding of one [`Observable`].
pub const ENCODED_LEN: usize = 16;

/// The per-cycle tuple an external implementation can genuinely produce.
///
/// See the module docs for why each field is in, and for the two whose
/// observability is weaker than the rest (`pc`, and the folded IRQ line).
// Four bools, and clippy is right that that is usually a smell. Here each one
// is a distinct hardware line -- R/W phase, /NMI, and the two /IRQ samples --
// so folding them into a bitfield would hide exactly what the struct exists to
// name, and the wire encoding already packs them into one byte.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Observable {
    /// The cycle axis both sides count on.
    pub cpu_cycle: u64,
    /// Program counter. DUT-observable, **not** pin-observable.
    pub pc: u16,
    /// Address bus for this cycle.
    pub bus_addr: u16,
    /// Data bus for this cycle.
    pub bus_data: u8,
    /// Bus character, encoded as [`Self::access_code`].
    pub bus_access: u8,
    /// The R/W phase half of the M2 cycle.
    pub put_cycle: bool,
    /// The /NMI pin.
    pub nmi_line: bool,
    /// The single wire-OR'd /IRQ pin, sampled on the low half of the cycle.
    pub irq_line_at_low: bool,
    /// The same pin, sampled on the high half.
    pub irq_line_at_high: bool,
}

impl Observable {
    /// Stable numeric code for a bus access.
    ///
    /// Deliberately numeric rather than the CSV writer's single letters: the
    /// C++ side packs an integer, and reusing the letters would invite a
    /// signed-char mismatch on the DMA codes, which are the lowercase ones.
    ///
    /// `0`=idle, `1`=read, `2`=write, `3`=DMA read, `4`=DMA write.
    #[must_use]
    pub const fn access_code(idle: bool, write: bool, dma: bool) -> u8 {
        if idle {
            0
        } else if dma {
            if write { 4 } else { 3 }
        } else if write {
            2
        } else {
            1
        }
    }

    /// Project a `RustyNES` [`CycleRecord`] onto the observable subset.
    ///
    /// **This function is where the partition lives.** Every field it drops is
    /// dropped on purpose, and the module docs give the reason; a future
    /// widening of [`Observable`] has to come through here, where
    /// `tests::model_internal_state_cannot_cause_a_divergence` is watching.
    ///
    /// Note the IRQ fold: `CycleRecord` attributes each sample to the mapper or
    /// the APU, and hardware has one wire-OR'd /IRQ pin that cannot make that
    /// distinction, so the pairs are OR'd rather than carried separately.
    #[must_use]
    pub const fn from_cycle_record(r: &CycleRecord) -> Self {
        Self {
            cpu_cycle: r.cpu_cycle,
            pc: r.pc,
            bus_addr: r.bus_addr,
            bus_data: r.bus_data,
            bus_access: match r.bus_access {
                BusAccess::Idle => 0,
                BusAccess::Read => 1,
                BusAccess::Write => 2,
                BusAccess::DmaRead => 3,
                BusAccess::DmaWrite => 4,
            },
            put_cycle: r.put_cycle_post,
            nmi_line: r.nmi_line,
            irq_line_at_low: r.irq_pending_mapper_at_low || r.irq_pending_apu_at_low,
            irq_line_at_high: r.irq_pending_mapper_at_high || r.irq_pending_apu_at_high,
        }
    }

    /// The fixed little-endian wire encoding, pinned by a test.
    ///
    /// Layout, byte offsets: `cpu_cycle` 0..8, `pc` 8..10, `bus_addr` 10..12,
    /// `bus_data` 12, `bus_access` 13, flags 14, pad 15. The trailing pad byte
    /// is explicit and always zero so the C++ side cannot accidentally hash
    /// uninitialised struct padding -- the classic way two correct
    /// implementations of the same layout disagree.
    #[must_use]
    pub const fn encode(&self) -> [u8; ENCODED_LEN] {
        let c = self.cpu_cycle.to_le_bytes();
        let p = self.pc.to_le_bytes();
        let a = self.bus_addr.to_le_bytes();
        let flags = (self.put_cycle as u8)
            | ((self.nmi_line as u8) << 1)
            | ((self.irq_line_at_low as u8) << 2)
            | ((self.irq_line_at_high as u8) << 3);
        [
            c[0],
            c[1],
            c[2],
            c[3],
            c[4],
            c[5],
            c[6],
            c[7],
            p[0],
            p[1],
            a[0],
            a[1],
            self.bus_data,
            self.bus_access,
            flags,
            0,
        ]
    }
}

/// One emitted checkpoint: the hash of every cycle up to and including
/// `through_cycle`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Checkpoint {
    /// The last cycle folded into `hash`.
    pub through_cycle: u64,
    /// The chained FNV-1a state at that point.
    pub hash: u64,
}

/// Chains the per-cycle hash and emits a [`Checkpoint`] every `interval`
/// cycles.
#[derive(Debug, Clone)]
pub struct Hasher {
    state: u64,
    interval: u64,
    counted: u64,
    last_cycle: u64,
    checkpoints: Vec<Checkpoint>,
}

impl Hasher {
    /// A hasher emitting every `interval` cycles.
    ///
    /// # Panics
    ///
    /// If `interval` is zero. A zero interval would emit a checkpoint per cycle
    /// and reproduce the 7.5 GB problem this module exists to avoid, so it is
    /// rejected loudly rather than silently clamped.
    #[must_use]
    pub fn new(interval: u64) -> Self {
        assert!(interval > 0, "checkpoint interval must be non-zero");
        Self {
            state: FNV_OFFSET_BASIS,
            interval,
            counted: 0,
            last_cycle: 0,
            checkpoints: Vec::new(),
        }
    }

    /// Fold one cycle in, emitting a checkpoint when the interval is reached.
    pub fn push(&mut self, o: &Observable) {
        for b in o.encode() {
            self.state ^= u64::from(b);
            self.state = self.state.wrapping_mul(FNV_PRIME);
        }
        self.last_cycle = o.cpu_cycle;
        self.counted += 1;
        if self.counted.is_multiple_of(self.interval) {
            self.checkpoints.push(Checkpoint {
                through_cycle: o.cpu_cycle,
                hash: self.state,
            });
        }
    }

    /// Emit a final checkpoint for a partial trailing window.
    ///
    /// Without this, a run whose length is not a multiple of the interval
    /// silently drops its tail -- and the tail is where a run that was stopped
    /// early because something went wrong actually differs. Returns the
    /// complete stream.
    #[must_use]
    pub fn finish(mut self) -> Vec<Checkpoint> {
        if !self.counted.is_multiple_of(self.interval) && self.counted > 0 {
            self.checkpoints.push(Checkpoint {
                through_cycle: self.last_cycle,
                hash: self.state,
            });
        }
        self.checkpoints
    }

    /// Cycles folded so far.
    #[must_use]
    pub const fn counted(&self) -> u64 {
        self.counted
    }
}

/// The result of comparing two checkpoint streams.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Comparison {
    /// Every checkpoint agreed, and both streams were the same length.
    Identical {
        /// How many checkpoints were compared.
        checkpoints: usize,
    },
    /// The streams diverged. The window to re-run with full capture is
    /// `(after_cycle, through_cycle]`.
    Diverged(Divergence),
    /// The streams cannot be compared, and this is **not** agreement.
    ///
    /// A truncated run, a DUT that stopped early, or two runs at different
    /// intervals all land here. Reporting them as `Identical` would be the
    /// project's recurring failure -- an absence of signal read as a pass.
    Inconclusive {
        /// Why the comparison could not be made.
        reason: InconclusiveReason,
    },
}

/// Why a comparison could not be made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InconclusiveReason {
    /// One side produced no checkpoints at all.
    Empty,
    /// The streams agree as far as the shorter one goes, then one ends.
    LengthMismatch {
        /// Checkpoints in the reference stream.
        reference: usize,
        /// Checkpoints in the candidate stream.
        candidate: usize,
    },
    /// The two streams checkpoint at different cycles, so their hashes cover
    /// different spans and cannot be compared at all.
    UnalignedCycles {
        /// The index at which the cycle axes first disagree.
        index: usize,
    },
}

/// Where two checkpoint streams stopped agreeing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Divergence {
    /// Index of the first differing checkpoint.
    pub index: usize,
    /// Last cycle known to agree. Zero when the very first window differs.
    pub after_cycle: u64,
    /// Last cycle of the first differing window.
    pub through_cycle: u64,
    /// The reference hash at that checkpoint.
    pub reference_hash: u64,
    /// The candidate hash at that checkpoint.
    pub candidate_hash: u64,
}

impl Divergence {
    /// Cycles in the window a full-capture re-run must cover.
    #[must_use]
    pub const fn window_len(&self) -> u64 {
        self.through_cycle - self.after_cycle
    }
}

/// Compare two checkpoint streams.
///
/// Answers three things and never collapses them: they agree, they diverge at a
/// located window, or the question could not be answered.
#[must_use]
pub fn compare(reference: &[Checkpoint], candidate: &[Checkpoint]) -> Comparison {
    if reference.is_empty() || candidate.is_empty() {
        return Comparison::Inconclusive {
            reason: InconclusiveReason::Empty,
        };
    }

    let common = reference.len().min(candidate.len());
    for i in 0..common {
        // Cycle alignment is checked BEFORE the hash. Two streams checkpointing
        // at different cycles cover different spans, so a hash difference
        // between them says nothing -- reporting it as a divergence would send
        // a re-run at a window that is not where the problem is.
        if reference[i].through_cycle != candidate[i].through_cycle {
            return Comparison::Inconclusive {
                reason: InconclusiveReason::UnalignedCycles { index: i },
            };
        }
        if reference[i].hash != candidate[i].hash {
            return Comparison::Diverged(Divergence {
                index: i,
                after_cycle: if i == 0 {
                    0
                } else {
                    reference[i - 1].through_cycle
                },
                through_cycle: reference[i].through_cycle,
                reference_hash: reference[i].hash,
                candidate_hash: candidate[i].hash,
            });
        }
    }

    if reference.len() != candidate.len() {
        return Comparison::Inconclusive {
            reason: InconclusiveReason::LengthMismatch {
                reference: reference.len(),
                candidate: candidate.len(),
            },
        };
    }
    Comparison::Identical {
        checkpoints: common,
    }
}

/// Serialize a checkpoint stream: repeated `(u64 through_cycle, u64 hash)`,
/// little-endian, no header.
///
/// Headerless on purpose. The manifest already records the ROM hash, the seed
/// and the frame count, and a second place to state them is a second place for
/// them to disagree.
#[must_use]
pub fn to_bytes(checkpoints: &[Checkpoint]) -> Vec<u8> {
    let mut out = Vec::with_capacity(checkpoints.len() * 16);
    for c in checkpoints {
        out.extend_from_slice(&c.through_cycle.to_le_bytes());
        out.extend_from_slice(&c.hash.to_le_bytes());
    }
    out
}

/// Parse a checkpoint stream written by [`to_bytes`].
///
/// # Errors
///
/// If the length is not a multiple of 16. A trailing partial record means the
/// producer was interrupted, and a truncated stream that parses is a truncated
/// comparison that passes.
///
/// # Panics
///
/// Never in practice: the `expect`s convert 8-byte subslices of a
/// `chunks_exact(16)` chunk, which the length check above has already
/// guaranteed. They are `expect` rather than `unwrap_or_default` so a future
/// change to the record width fails loudly instead of silently zero-filling.
pub fn from_bytes(bytes: &[u8]) -> Result<Vec<Checkpoint>, &'static str> {
    if !bytes.len().is_multiple_of(16) {
        return Err("checkpoint stream length is not a multiple of 16 bytes");
    }
    Ok(bytes
        .chunks_exact(16)
        .map(|c| Checkpoint {
            through_cycle: u64::from_le_bytes(c[0..8].try_into().expect("8 bytes")),
            hash: u64::from_le_bytes(c[8..16].try_into().expect("8 bytes")),
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A deterministic `CycleRecord` for the projection tests.
    fn record(cycle: u64) -> CycleRecord {
        CycleRecord {
            cpu_cycle: cycle,
            pc: 0xC000,
            ppu_scanline: 0,
            ppu_dot: 0,
            ppu_frame: 0,
            irq_pending_mapper_at_low: false,
            irq_pending_apu_at_low: false,
            irq_pending_mapper_at_high: false,
            irq_pending_apu_at_high: false,
            nmi_line: false,
            a12_events: Vec::new(),
            dmc_dma_pending_pre: false,
            dmc_dma_pending_post: false,
            dmc_dma_short_post: false,
            dmc_abort_pending_post: false,
            dmc_abort_delay_post: 0,
            dmc_dma_cooldown_post: 0,
            dmc_dma_delay_post: 0,
            apu_phase_post: false,
            in_dmc_dma: false,
            dma_cycles_owed: 0,
            bus_access: BusAccess::Read,
            bus_addr: 0x8000,
            bus_data: 0xEA,
            put_cycle_post: false,
            dmc_timer_post: 0,
            dmc_bits_remaining_post: 0,
            dmc_silence_post: false,
            dmc_buffer_full_post: false,
        }
    }

    fn obs(cycle: u64) -> Observable {
        Observable {
            cpu_cycle: cycle,
            pc: 0xC000u16.wrapping_add(u16::try_from(cycle & 0xFFFF).expect("masked")),
            bus_addr: 0x8000u16.wrapping_add(u16::try_from(cycle & 0xFFFF).expect("masked")),
            bus_data: u8::try_from(cycle & 0xFF).expect("masked"),
            bus_access: Observable::access_code(false, cycle.is_multiple_of(3), false),
            put_cycle: cycle.is_multiple_of(2),
            nmi_line: false,
            irq_line_at_low: cycle.is_multiple_of(7),
            irq_line_at_high: cycle.is_multiple_of(11),
        }
    }

    fn stream(n: u64, interval: u64) -> Vec<Checkpoint> {
        let mut h = Hasher::new(interval);
        for c in 0..n {
            h.push(&obs(c));
        }
        h.finish()
    }

    /// The C++ testbench reimplements this encoding and this hash. If either
    /// moves, the next co-simulation run reports a divergence that is ours, not
    /// the DUT's -- so both are pinned to a fixed value rather than to whatever
    /// the current code happens to produce.
    #[test]
    fn the_wire_encoding_is_pinned_to_a_fixed_vector() {
        let o = Observable {
            cpu_cycle: 0x0102_0304_0506_0708,
            pc: 0xC5F5,
            bus_addr: 0x8123,
            bus_data: 0xA9,
            bus_access: 1,
            put_cycle: true,
            nmi_line: false,
            irq_line_at_low: true,
            irq_line_at_high: false,
        };
        assert_eq!(
            o.encode(),
            [
                0x08,
                0x07,
                0x06,
                0x05,
                0x04,
                0x03,
                0x02,
                0x01,
                0xF5,
                0xC5,
                0x23,
                0x81,
                0xA9,
                0x01,
                0b0000_0101,
                0x00,
            ],
            "wire layout changed; the C++ writer must change with it"
        );

        let mut h = Hasher::new(1);
        h.push(&o);
        let ck = h.finish();
        assert_eq!(ck.len(), 1);
        // Independently recomputed FNV-1a over the 16 bytes above.
        let mut expect = FNV_OFFSET_BASIS;
        for b in [
            0x08u8,
            0x07,
            0x06,
            0x05,
            0x04,
            0x03,
            0x02,
            0x01,
            0xF5,
            0xC5,
            0x23,
            0x81,
            0xA9,
            0x01,
            0b0000_0101,
            0x00,
        ] {
            expect ^= u64::from(b);
            expect = expect.wrapping_mul(FNV_PRIME);
        }
        assert_eq!(ck[0].hash, expect);
    }

    #[test]
    fn identical_streams_compare_identical() {
        let a = stream(10_000, DEFAULT_INTERVAL);
        let b = stream(10_000, DEFAULT_INTERVAL);
        assert_eq!(compare(&a, &b), Comparison::Identical { checkpoints: 3 });
    }

    /// The gate must be shown able to FAIL, not merely to pass.
    #[test]
    fn a_single_flipped_bit_is_localised_to_its_window() {
        let reference = stream(10_000, DEFAULT_INTERVAL);

        let mut h = Hasher::new(DEFAULT_INTERVAL);
        for c in 0..10_000u64 {
            let mut o = obs(c);
            if c == 5000 {
                o.bus_data ^= 0x01;
            }
            h.push(&o);
        }
        let candidate = h.finish();

        match compare(&reference, &candidate) {
            Comparison::Diverged(d) => {
                assert_eq!(d.index, 1, "cycle 5000 falls in the second window");
                assert_eq!(d.after_cycle, 4095);
                assert_eq!(d.through_cycle, 8191);
                assert_eq!(d.window_len(), 4096);
                assert!(
                    (d.after_cycle..=d.through_cycle).contains(&5000),
                    "the reported window must contain the corrupted cycle"
                );
            }
            other => panic!("expected a divergence, got {other:?}"),
        }
    }

    /// The partition is real, not decorative: two records differing ONLY in
    /// state this module deliberately excludes must produce the same hash.
    ///
    /// If this fails, someone widened `Observable` to carry model state, and an
    /// independent implementation will now be failed for not reproducing a Rust
    /// struct rather than for disagreeing about hardware.
    #[test]
    fn model_internal_state_cannot_cause_a_divergence() {
        let a = record(42);
        let mut b = record(42);
        // Every field the projection drops, perturbed at once.
        b.ppu_scanline = 120;
        b.ppu_dot = 200;
        b.ppu_frame = 9;
        b.a12_events.push(rustynes_core::irq_trace::A12Event {
            sub_dot: 1,
            level: true,
        });
        b.dmc_dma_pending_pre = !b.dmc_dma_pending_pre;
        b.dmc_dma_pending_post = !b.dmc_dma_pending_post;
        b.dmc_dma_short_post = !b.dmc_dma_short_post;
        b.dmc_abort_pending_post = !b.dmc_abort_pending_post;
        b.dmc_abort_delay_post = 3;
        b.dmc_dma_cooldown_post = 4;
        b.dmc_dma_delay_post = 5;
        b.apu_phase_post = !b.apu_phase_post;
        b.in_dmc_dma = !b.in_dmc_dma;
        b.dma_cycles_owed = 7;
        b.dmc_timer_post = 400;
        b.dmc_bits_remaining_post = 6;
        b.dmc_silence_post = !b.dmc_silence_post;
        b.dmc_buffer_full_post = !b.dmc_buffer_full_post;

        assert_eq!(
            Observable::from_cycle_record(&a).encode(),
            Observable::from_cycle_record(&b).encode(),
            "a field outside the observable subset changed the hash"
        );
    }

    /// And the converse, so the test above cannot pass by the projection
    /// dropping everything.
    #[test]
    fn an_observable_field_does_cause_a_divergence() {
        let a = record(42);
        let mut b = record(42);
        b.bus_addr ^= 0x0001;
        assert_ne!(
            Observable::from_cycle_record(&a).encode(),
            Observable::from_cycle_record(&b).encode()
        );
    }

    /// The two IRQ sources fold to one wire before hashing, because hardware
    /// has one wire and a DUT cannot tell them apart.
    #[test]
    fn the_irq_sources_fold_to_a_single_line() {
        let mut mapper_only = record(1);
        mapper_only.irq_pending_mapper_at_low = true;
        mapper_only.irq_pending_apu_at_low = false;

        let mut apu_only = record(1);
        apu_only.irq_pending_mapper_at_low = false;
        apu_only.irq_pending_apu_at_low = true;

        assert_eq!(
            Observable::from_cycle_record(&mapper_only).encode(),
            Observable::from_cycle_record(&apu_only).encode(),
            "a DUT cannot tell which source asserted /IRQ, so neither may the hash"
        );

        let mut neither = record(1);
        neither.irq_pending_mapper_at_low = false;
        neither.irq_pending_apu_at_low = false;
        assert_ne!(
            Observable::from_cycle_record(&mapper_only).encode(),
            Observable::from_cycle_record(&neither).encode(),
            "the folded line must still distinguish asserted from not"
        );
    }

    /// A run that stopped short must never read as agreement. Which *kind* of
    /// inconclusive it is depends on where it stopped, and both branches are
    /// exercised because only one of them was reachable by accident.
    ///
    /// Stopping mid-window emits a partial tail checkpoint at a cycle the
    /// reference never checkpoints at, so the axes disagree before the hashes
    /// are ever compared -- which is the more informative answer, and the one
    /// the first version of this test guessed wrong.
    #[test]
    fn a_run_truncated_mid_window_is_inconclusive_not_identical() {
        let reference = stream(10_000, DEFAULT_INTERVAL);
        let candidate = stream(6_000, DEFAULT_INTERVAL);
        assert_eq!(
            compare(&reference, &candidate),
            Comparison::Inconclusive {
                reason: InconclusiveReason::UnalignedCycles { index: 1 }
            },
            "candidate's tail checkpoint is at cycle 5999, reference's is at 8191"
        );
    }

    /// Stopping exactly on an interval boundary produces aligned, agreeing
    /// checkpoints and then simply runs out -- the case `LengthMismatch` exists
    /// for.
    #[test]
    fn a_run_truncated_on_a_boundary_is_inconclusive_not_identical() {
        let reference = stream(10_000, DEFAULT_INTERVAL);
        let candidate = stream(8_192, DEFAULT_INTERVAL);
        assert_eq!(
            compare(&reference, &candidate),
            Comparison::Inconclusive {
                reason: InconclusiveReason::LengthMismatch {
                    reference: 3,
                    candidate: 2
                }
            }
        );
    }

    #[test]
    fn an_empty_stream_is_inconclusive_not_identical() {
        let reference = stream(10_000, DEFAULT_INTERVAL);
        assert_eq!(
            compare(&reference, &[]),
            Comparison::Inconclusive {
                reason: InconclusiveReason::Empty
            }
        );
        assert_eq!(
            compare(&[], &reference),
            Comparison::Inconclusive {
                reason: InconclusiveReason::Empty
            }
        );
    }

    /// Different intervals cover different spans, so the hashes are not
    /// comparable at all -- and saying "diverged" would send a re-run at a
    /// window that is not where any problem is.
    #[test]
    fn streams_at_different_intervals_are_inconclusive_not_diverged() {
        let a = stream(8192, 4096);
        let b = stream(8192, 2048);
        assert_eq!(
            compare(&a, &b),
            Comparison::Inconclusive {
                reason: InconclusiveReason::UnalignedCycles { index: 0 }
            }
        );
    }

    /// A trailing partial window must still be emitted; the tail is where a run
    /// stopped early actually differs.
    #[test]
    fn a_partial_trailing_window_is_still_emitted() {
        let ck = stream(5000, DEFAULT_INTERVAL);
        assert_eq!(ck.len(), 2);
        assert_eq!(ck[0].through_cycle, 4095);
        assert_eq!(ck[1].through_cycle, 4999, "the tail must not be dropped");
    }

    #[test]
    fn the_serialised_form_round_trips() {
        let ck = stream(10_000, DEFAULT_INTERVAL);
        let bytes = to_bytes(&ck);
        assert_eq!(bytes.len(), ck.len() * 16);
        assert_eq!(from_bytes(&bytes).expect("round trip"), ck);
    }

    /// A truncated stream that parses is a truncated comparison that passes.
    #[test]
    fn a_truncated_serialised_stream_is_rejected() {
        let mut bytes = to_bytes(&stream(10_000, DEFAULT_INTERVAL));
        bytes.pop();
        assert!(from_bytes(&bytes).is_err());
    }

    #[test]
    #[should_panic(expected = "checkpoint interval must be non-zero")]
    fn a_zero_interval_is_rejected() {
        let _ = Hasher::new(0);
    }

    #[test]
    fn access_codes_are_stable() {
        assert_eq!(Observable::access_code(true, false, false), 0);
        assert_eq!(Observable::access_code(false, false, false), 1);
        assert_eq!(Observable::access_code(false, true, false), 2);
        assert_eq!(Observable::access_code(false, false, true), 3);
        assert_eq!(Observable::access_code(false, true, true), 4);
    }
}
