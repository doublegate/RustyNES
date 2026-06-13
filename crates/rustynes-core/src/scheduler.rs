//! Lockstep-scheduler types shared across the bus, the IRQ-timing trace
//! fixture, and (in Phase B+) the CPU IRQ sampling path.
//!
//! Currently re-exports [`M2Phase`] (canonical reference enum for "which
//! half of the 6502 cycle the lockstep bus is currently in") from
//! `rustynes-cpu`.  The definition lives in `rustynes-cpu` because the
//! [`rustynes_cpu::Bus`] trait method `poll_irq_at_phase` is parameterised
//! over it; consumers of `rustynes-core` (the frontend, the test harness,
//! the `irq_trace` fixture) continue to import the enum from
//! `rustynes_core::scheduler` via this re-export so existing import paths
//! stay unchanged.
//!
//! See `docs/scheduler.md` and `docs/adr/0002-irq-timing-coordination.md`
//! for the surrounding design.

pub use rustynes_cpu::M2Phase;
