//! Length-counter sub-unit (shared by pulse, triangle, noise).
//!
//! Per `docs/apu-2a03.md` §State and the NESdev wiki "APU Length Counter"
//! page. A 5-bit register selects from a fixed 32-entry lookup table; when
//! non-zero and clocked at half-frame, decrements toward zero. A `halt` bit
//! freezes the counter (also doubles as the envelope-loop bit on pulse and
//! noise channels).
//!
//! ## Halt/reload write ordering vs the half-frame clock (v2.1.5)
//!
//! The 2A03 applies a length-counter **halt** change and a length **reload**
//! (`$4003`/`$4007`/`$400B`/`$400F` load) with a one-step deferral relative to
//! the frame sequencer's half-frame length clock — the behaviour blargg's
//! `pal_apu_tests` `10.len_halt_timing` and `11.len_reload_timing` (and their
//! NTSC `blargg_apu_2005` twins) pin:
//!
//! - **Halt after clock, not before.** A `$4000`-bit-5 write that lands on the
//!   *same* CPU cycle as a half-frame length clock does **not** suppress that
//!   cycle's clock; the halt takes effect for the *next* clock. Modelled by
//!   latching the written value in [`new_halt`](LengthCounter::new_halt) and
//!   promoting it to the effective [`halt`](LengthCounter::halt) in
//!   [`reload`](LengthCounter::reload), which the owning APU calls once per CPU
//!   cycle *after* the half-frame [`clock`](LengthCounter::clock) but *before*
//!   the mixer samples the channel output.
//! - **Reload ignored during a non-zero clock.** A length load that lands on the
//!   half-frame clock cycle is honoured only if the counter was **not** clocked
//!   this cycle (i.e. it was already zero, so the decrement was a no-op).
//!   Modelled by snapshotting the pre-clock count in
//!   [`previous_count`](LengthCounter::previous_count) at load time and, in
//!   [`reload`](LengthCounter::reload), applying the pending
//!   [`reload_val`](LengthCounter::reload_val) only when the (post-clock) count
//!   still equals that snapshot.
//!
//! This mirrors the `TetaNES` `LengthCounter` (`new_halt` / `reload` /
//! `previous_counter`) and Mesen2's `ApuLengthCounter` (`_newHaltValue` +
//! reload-request) mechanisms verbatim. On a write that does **not** coincide
//! with a half-frame clock the deferral is invisible: `reload` runs in the same
//! cycle as the write (after the no-op clock, before the sample), so the count
//! settles to the loaded value / the halt settles to the written value *within
//! the write cycle* — byte-identical to an immediate apply. Only the
//! write-lands-exactly-on-the-clock-cycle coincidence differs, which is the
//! precise edge the test ROMs probe.

/// 32-entry length lookup table (from the NESdev wiki).
pub const LENGTH_TABLE: [u8; 32] = [
    10, 254, 20, 2, 40, 4, 80, 6, 160, 8, 60, 10, 14, 12, 26, 14, 12, 16, 24, 18, 48, 20, 96, 22,
    192, 24, 72, 26, 16, 28, 32, 30,
];

/// Length counter shared by pulse, triangle, noise channels.
#[derive(Debug, Clone, Copy, Default)]
pub struct LengthCounter {
    /// Current count (0..=254). 0 = silenced.
    pub count: u8,
    /// Effective halt flag consulted by [`clock`](Self::clock) (also serves as
    /// envelope-loop on pulse/noise; control on triangle). Updated from
    /// [`new_halt`](Self::new_halt) in [`reload`](Self::reload).
    pub halt: bool,
    /// Latched halt value from the most recent `$4000`/`$4004`/`$4008`/`$400C`
    /// write, promoted to [`halt`](Self::halt) by [`reload`](Self::reload)
    /// *after* the half-frame clock (the "halt change occurs after clocking
    /// length" rule). See the module docs.
    pub new_halt: bool,
    /// Channel-enable flag from `$4015` write.
    pub enabled: bool,
    /// Pending reload value from the most recent length load. `0` = no pending
    /// reload (a real load never selects table entry 0 for index 0 → value 10,
    /// so `0` is an unambiguous "empty" sentinel here; the table has no 0
    /// entry). Consumed by [`reload`](Self::reload).
    pub reload_val: u8,
    /// Snapshot of [`count`](Self::count) captured at load time. If the
    /// half-frame clock decremented the counter this cycle, the post-clock
    /// count differs from this snapshot and the pending reload is dropped (the
    /// "reload ignored during clocking when ctr > 0" rule). See the module docs.
    pub previous_count: u8,
}

impl LengthCounter {
    /// Load a new value from a `$4003`/`$4007`/`$400B`/`$400F` write.
    /// Lookup index = top 5 bits of the value.
    ///
    /// The reload is **deferred**: it latches [`reload_val`](Self::reload_val)
    /// and snapshots the current [`count`](Self::count) into
    /// [`previous_count`](Self::previous_count); [`reload`](Self::reload)
    /// applies it (or drops it, if a same-cycle half-frame clock moved the
    /// count). A disabled channel ignores the load entirely.
    pub fn load(&mut self, raw: u8) {
        if self.enabled {
            self.reload_val = LENGTH_TABLE[(raw >> 3) as usize];
            self.previous_count = self.count;
        }
    }

    /// Latch a halt-flag change from a `$4000`/`$4004`/`$4008`/`$400C` write.
    /// The value takes effect at the next [`reload`](Self::reload) (after the
    /// half-frame clock), not immediately — see the module docs.
    pub const fn set_halt(&mut self, halt: bool) {
        self.new_halt = halt;
    }

    /// Channel-enable update from `$4015` write. Clearing the bit forces
    /// the count to 0 (silences the channel).
    pub const fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        if !enabled {
            self.count = 0;
        }
    }

    /// Half-frame clock.
    pub const fn clock(&mut self) {
        if !self.halt && self.count > 0 {
            self.count -= 1;
        }
    }

    /// Apply the deferred halt and reload. Called by the owning APU once per CPU
    /// cycle, **after** the half-frame [`clock`](Self::clock) and **before** the
    /// mixer samples the channel:
    ///
    /// - A pending reload is honoured only if the post-clock count still equals
    ///   the [`previous_count`](Self::previous_count) snapshot taken at load —
    ///   i.e. a same-cycle half-frame clock did not decrement it (it was already
    ///   zero). Otherwise the reload is dropped.
    /// - The effective [`halt`](Self::halt) is refreshed from
    ///   [`new_halt`](Self::new_halt) unconditionally, so a halt change becomes
    ///   effective for the *next* clock.
    ///
    /// On a cycle with no half-frame clock (the overwhelmingly common case),
    /// the count is untouched between the write and this call, so `count ==
    /// previous_count` holds and the reload applies in-cycle — byte-identical to
    /// an immediate load.
    pub const fn reload(&mut self) {
        if self.reload_val > 0 {
            if self.count == self.previous_count {
                self.count = self.reload_val;
            }
            self.reload_val = 0;
        }
        self.halt = self.new_halt;
    }

    /// `$4015` read — bit set if count > 0.
    #[must_use]
    pub const fn active(&self) -> bool {
        self.count > 0
    }
}
