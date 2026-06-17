//! Test ROM runner and golden-master comparator.
//!
//! See `docs/testing-strategy.md` §Layer 2 (golden-log compare) and §Layer 3
//! (test ROM corpus) for the design.
//!
//! Houses the nestest golden-log harness, the blargg "report status at
//! `$6000`" runner, and the full `Nes`-based test harness used by the
//! integration tests under `crates/rustynes-test-harness/tests/*`.

#![warn(missing_docs)]

pub mod accuracy_coin;
pub mod accuracy_coin_catalog;
mod blargg;
mod nes_runner;
mod nestest;

/// Shared commercial-ROM boot-coverage primitives.
///
/// The recursive `.nes` walk + the distinct-colour blank-frame health
/// heuristic, used by the `coverage_smoke` / `render_smoke` diagnostic bins
/// and the `external_coverage` integration test. Compiled only under the
/// `commercial-roms` feature.
#[cfg(feature = "commercial-roms")]
pub mod coverage;

pub use blargg::{BlarggBus, BlarggResult, run_blargg_until_complete};
pub use nes_runner::{NesTestResult, run_nes_blargg, run_nes_blargg_pal, run_nes_blargg_reset};
pub use nestest::{LogLine, NestestBus, NestestRunner, format_log_line, parse_log_line};

use rustynes_core::rustynes_cpu::Cpu;

/// Returns the crate version string.
#[must_use]
pub const fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Convenience: build a fresh `Cpu` in nestest "automation" mode (PC=$C000,
/// `S=$FD`, `P=$24`, cycles already accounting for the 7-cycle reset).
#[must_use]
pub const fn cpu_for_nestest() -> Cpu {
    let mut cpu = Cpu::new();
    cpu.set_pc(0xC000);
    cpu.cycles = 7; // Nintendulator starts CYC at 7 after reset.
    cpu
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_is_non_empty() {
        assert!(!version().is_empty());
    }
}
