//! Scheduler-facing types the CPU crate exposes to its bus host.
//!
//! Currently exposes only [`M2Phase`], the canonical reference enum for
//! "which half of the 6502 cycle the host bus is currently in".  The
//! enum lives in `rustynes-cpu` rather than `rustynes-core` because the [`Bus`]
//! trait method [`Bus::poll_irq_at_phase`] is parameterised by it; the
//! CPU crate stays at the top of the workspace dep graph (`rustynes-core`
//! already depends on `rustynes-cpu`, not the other way round) and any
//! `rustynes-core` consumer continues to import `M2Phase` from
//! `rustynes_core::scheduler` via re-export.
//!
//! See `docs/scheduler.md` and `docs/adr/0002-irq-timing-coordination.md`
//! for the surrounding design.
//!
//! [`Bus`]: crate::Bus
//! [`Bus::poll_irq_at_phase`]: crate::Bus::poll_irq_at_phase

/// Convention for the M2-phase reference relative to the CPU cycle's 3
/// PPU dots.
///
/// In silicon the 6502 cycle has two halves — φ1 (M2 low; address valid;
/// memory access) and φ2 (M2 high; data latch; interrupt sample).  The
/// host scheduler ticks the PPU 3 dots per CPU cycle.  The convention
/// this crate adopts:
///
/// * [`M2Phase::Low`] — the **first** half of the cycle: from the start
///   of the bus's per-cycle tick through the end of PPU sub-dot 1
///   (corresponds to silicon's φ1).
/// * [`M2Phase::High`] — the **second** half of the cycle: from the end
///   of PPU sub-dot 1 through end-of-cycle (corresponds to silicon's
///   φ2).  The M2-rising boundary lives between sub-dot 1 and sub-dot 2.
///
/// At end-of-cycle the bus advances its cycle counter and the phase
/// resets to [`M2Phase::Low`] for the next cycle.
///
/// This is the canonical reference enum used by the docs/ADR, by the
/// IRQ-timing tracing fixture (`rustynes_core::irq_trace`), and by
/// [`Bus::poll_irq_at_phase`].
///
/// [`Bus::poll_irq_at_phase`]: crate::Bus::poll_irq_at_phase
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum M2Phase {
    /// M2 low (φ1): memory access window.
    Low,
    /// M2 high (φ2): IRQ/NMI sample window.
    High,
}

impl M2Phase {
    /// CSV-friendly single-letter abbreviation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Low => "L",
            Self::High => "H",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn m2_phase_as_str_round_trips() {
        assert_eq!(M2Phase::Low.as_str(), "L");
        assert_eq!(M2Phase::High.as_str(), "H");
    }
}
