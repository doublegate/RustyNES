//! Lockstep-scheduler types shared across the bus, the IRQ-timing trace
//! fixture, and (in Phase B+) the CPU IRQ sampling path.
//!
//! Currently re-exports [`M2Phase`] (canonical reference enum for "which
//! half of the 6502 cycle the bus is currently in") from `rustynes-cpu`.
//! The definition lives in `rustynes-cpu` because the `rustynes_cpu::Bus`
//! trait method `poll_irq_at_phase` was parameterised over it until v2.9.8
//! removed that method (ADR 0042); consumers of `rustynes-core` (the test
//! harness, the `irq_trace` fixture) import the enum from
//! `rustynes_core::scheduler` via this re-export.
//!
//! See `docs/scheduler.md` and `docs/adr/0002-irq-timing-coordination.md`
//! for the surrounding design.

pub use rustynes_cpu::M2Phase;
