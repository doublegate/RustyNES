// SPDX-License-Identifier: GPL-3.0-or-later
//! v2.9.7 — MMC3's PPU A12 rise filter, for boards built around an MMC3 core
//! that keep their own IRQ counter.
//!
//! # Why this exists
//!
//! During each rendered scanline the PPU drives A12 high and low many times:
//! in the sprite-fetch window (dots 257-320) every one of the eight slots
//! reads two nametable bytes (`$2xxx`, A12 low) and then the sprite's two
//! pattern bytes (`$1xxx` with sprites at `$1000`, A12 high). The MMC3 counts
//! one scanline per line anyway because it ignores a rise unless A12 had been
//! low for about three M2 (CPU) cycles (`MMC3.md`, "IRQ Specifics"). Only the
//! first rise of the window follows a long low; the other seven follow a
//! four-dot low and are filtered.
//!
//! Until v2.9.7 the PPU never reported the garbage nametable reads, so it
//! delivered one rise per line and a counter that clocked on EVERY rise still
//! counted scanlines. Four MMC3-derived boards (`mmc3_clones`, `m176`, `m268`,
//! `m513`) did exactly that. Once the PPU reports the hardware stream, those
//! counters would clock eight times per line; they use this filter instead,
//! which is the one [`crate::m004_mmc3`] applies inline.
//!
//! # State and serialization
//!
//! The filter needs the A12 level and how many CPU cycles ago A12 last fell.
//! It asks only whether that age has reached [`Self::MIN_LOW_CYCLES`], so an
//! age saturated at 127 reproduces every decision exactly. [`Self::to_byte`]
//! packs both into one byte (level in bit 7, age in bits 0-6), which lets the
//! boards keep the single `last_a12` byte their save states already carried.
//! Every byte value is one the filter can produce, so a restore never has to
//! refuse one.

/// MMC3's A12 rise filter. See the module docs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct A12RiseFilter {
    level: bool,
    /// CPU cycles since A12 last fell, saturated at [`Self::AGE_MAX`].
    age: u8,
}

impl A12RiseFilter {
    /// A rise clocks the counter only after A12 was low this many CPU cycles.
    pub(crate) const MIN_LOW_CYCLES: u8 = 3;
    /// Where the age saturates; any value `>= MIN_LOW_CYCLES` would do, and 127
    /// leaves bit 7 for the level.
    const AGE_MAX: u8 = 0x7F;

    /// Power-on: A12 low for "a long time", so the first real rise counts.
    pub(crate) const fn new() -> Self {
        Self {
            level: false,
            age: Self::AGE_MAX,
        }
    }

    /// One CPU (M2) cycle elapsed.
    pub(crate) const fn tick(&mut self) {
        if self.age < Self::AGE_MAX {
            self.age += 1;
        }
    }

    /// A12 is now at `level`. Returns `true` for a rise the counter clocks on:
    /// low to high after at least [`Self::MIN_LOW_CYCLES`] cycles low.
    pub(crate) const fn edge(&mut self, level: bool) -> bool {
        let qualifies = level && !self.level && self.age >= Self::MIN_LOW_CYCLES;
        if self.level && !level {
            self.age = 0;
        }
        self.level = level;
        qualifies
    }

    /// The A12 level last reported.
    #[cfg(test)]
    pub(crate) const fn level(self) -> bool {
        self.level
    }

    /// The save-state byte: level in bit 7, the saturated age in bits 0-6.
    pub(crate) const fn to_byte(self) -> u8 {
        ((self.level as u8) << 7) | self.age
    }

    /// Inverse of [`Self::to_byte`]; every byte is a producible state.
    pub(crate) const fn from_byte(b: u8) -> Self {
        Self {
            level: b & 0x80 != 0,
            age: b & Self::AGE_MAX,
        }
    }

    /// A state written before v2.9.7, whose byte was the bare level (`0`/`1`).
    /// Its age is unknown; saturating it is what such a state meant, since a
    /// board saved between frames has had A12 settled for thousands of cycles.
    pub(crate) const fn from_legacy_level(level: bool) -> Self {
        Self {
            level,
            age: Self::AGE_MAX,
        }
    }
}

impl Default for A12RiseFilter {
    fn default() -> Self {
        Self::new()
    }
}

/// Test helper (v2.9.7): drive `m` through `lines` rendered scanlines of the
/// A12 stream the PPU reports (a long low, then eight one-cycle pulses per
/// line, as in the sprite-fetch window), with the MMC3 IRQ latch at 7 and IRQs
/// enabled. Returns how many IRQs fired, acknowledging each. An MMC3-style
/// counter fires once per eight lines; one that clocks on every rise fires
/// every line.
#[cfg(test)]
pub(crate) fn irqs_over_scanlines(m: &mut dyn crate::mapper::Mapper, lines: u32) -> u32 {
    // The bus sends `notify_cpu_cycle` only to boards that declare
    // `cpu_cycle_hook`; without it the filter's clock never runs on a real
    // `Nes` and no rise ever qualifies. Driving the hook by hand below would
    // hide that, which is exactly how v2.9.7's first draft shipped four boards
    // with no IRQ at all (found on *AV Jiu Ji Ma Jiang 2*, mapper 115).
    assert!(
        m.caps().cpu_cycle_hook,
        "a board using the A12 filter must declare cpu_cycle_hook"
    );
    m.cpu_write(0xC000, 7);
    m.cpu_write(0xC001, 0);
    m.cpu_write(0xE001, 0);
    let mut irqs = 0;
    for _ in 0..lines {
        for _ in 0..85 {
            m.notify_cpu_cycle();
        }
        for _ in 0..8 {
            m.notify_a12(true);
            m.notify_cpu_cycle();
            m.notify_a12(false);
            m.notify_cpu_cycle();
        }
        if m.irq_pending() {
            irqs += 1;
            m.irq_acknowledge();
        }
    }
    irqs
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One rendered scanline as the PPU now reports it (sprites at `$1000`):
    /// a long low through the background fetches, then eight pulses whose lows
    /// last about one CPU cycle. Exactly one rise qualifies.
    fn scanline(f: &mut A12RiseFilter) -> u32 {
        let mut clocks = 0;
        for _ in 0..85 {
            f.tick();
        }
        for _ in 0..8 {
            clocks += u32::from(f.edge(true));
            f.tick();
            f.edge(false);
            f.tick();
        }
        clocks
    }

    #[test]
    fn one_clock_per_scanline_from_eight_pulses() {
        let mut f = A12RiseFilter::new();
        for _ in 0..240 {
            assert_eq!(scanline(&mut f), 1);
        }
    }

    #[test]
    fn a_rise_needs_three_cycles_low() {
        let mut f = A12RiseFilter::new();
        assert!(f.edge(true), "power-on counts as a long low");
        f.edge(false);
        f.tick();
        f.tick();
        assert!(!f.edge(true), "two cycles low is filtered");
        f.edge(false);
        for _ in 0..3 {
            f.tick();
        }
        assert!(f.edge(true), "three cycles low qualifies");
        assert!(!f.edge(true), "no rise without a fall");
    }

    #[test]
    fn the_byte_reproduces_every_decision() {
        for b in 0..=u8::MAX {
            let f = A12RiseFilter::from_byte(b);
            assert_eq!(f.to_byte(), b, "{b:#04x} round-trips");
        }
        let mut a = A12RiseFilter::new();
        a.edge(true);
        a.edge(false);
        a.tick();
        let mut b = A12RiseFilter::from_byte(a.to_byte());
        for n in 0..4 {
            let (mut a2, mut b2) = (a, b);
            assert_eq!(a2.edge(true), b2.edge(true), "after {n} more cycles");
            a.tick();
            b.tick();
        }
        assert!(A12RiseFilter::from_legacy_level(true).level());
        assert_eq!(
            A12RiseFilter::from_legacy_level(false),
            A12RiseFilter::new()
        );
    }
}
