//! Cycle-accurate Ricoh 2A03 CPU core (6502 derivative without BCD mode).
//!
//! See `docs/cpu-6502.md` in the workspace root for the implementation
//! specification, and `ref-docs/research-report.md` §CPU for the source
//! material this is derived from.
//!
//! All 151 documented 6502 opcodes plus the 105 unofficial / undocumented
//! ones are implemented at per-cycle granularity, including the JAM/KIL/STP
//! halt opcodes, NMI / IRQ / BRK handling, and the page-crossing dummy reads
//! that the test ROMs check for.

#![no_std]
#![warn(missing_docs)]

extern crate alloc;

mod bus;
mod cpu;
pub mod disasm;
pub mod scheduler;
mod snapshot;
mod status;

pub use bus::Bus;
pub use cpu::Cpu;
pub use disasm::{disassemble_at, DisasmLine};
pub use scheduler::M2Phase;
pub use snapshot::{CpuSnapshotError, CPU_SNAPSHOT_VERSION};
pub use status::Status;

/// Returns the crate version string.
#[must_use]
pub const fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_is_non_empty() {
        assert!(!version().is_empty());
    }
}
