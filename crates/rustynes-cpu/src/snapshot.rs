//! Save-state encoding / decoding for the [`Cpu`].
//!
//! Per `CLAUDE.md` §Open questions: tagged-section per chip, version byte
//! up front, best-effort cross-version compatibility. This module owns the
//! CPU section's schema, currently version 3 (ADR 0028) — see
//! [`CPU_SNAPSHOT_VERSION`] for the full version history and the v2.0.0
//! MAJOR-boundary rejection policy.
//!
//! The encoding is hand-rolled little-endian binary so this crate stays
//! free of `serde` / `bincode` (and so `bitflags` doesn't need its
//! `serde` feature). The container format used by the bus to wrap this
//! blob into a tagged section lives in `rustynes_core::save_state`.

use alloc::vec::Vec;
use thiserror::Error;

use crate::cpu::Cpu;
use crate::status::Status;

/// Schema version for the CPU snapshot blob.
///
/// - v1 (v0.9.0 ..): registers + interrupt latches + cycle bookkeeping.
/// - v2 (W3-Stage-4 promotion, 2026-06-10): appends the master-clock
///   substrate pipeline — `master_clock` (u64) + the `mc_need_nmi` /
///   `mc_prev_need_nmi` / `mc_run_irq` / `mc_prev_run_irq` /
///   `mc_prev_nmi_line` latches (1 byte each).
/// - **v3 (v2.0.0 "Timebase" rc.1, ADR 0028)**: the byte layout is
///   IDENTICAL to v2 — `cycles` and `master_clock` are both still
///   written, unchanged. What changes is the *guarantee*: as of the
///   beta.1–beta.4 one-clock promote, `cycles` is no longer an
///   independently-tracked counter (it is assigned from
///   `Bus::cycle_count()` at every `start_cycle`, see `cpu.rs`), so a v3
///   blob's `cycles`/`master_clock` pair is guaranteed internally
///   consistent by construction in a way a pre-promote v1/v2 blob was
///   only *coincidentally* consistent (kept in sync by parallel
///   increments, not derivation). The version bump exists to make that
///   distinction an explicit, checked contract rather than an implicit
///   assumption — see ADR 0028 for the full MAJOR-boundary decision.
///   v1/v2 blobs are no longer upconverted; [`Cpu::restore`] rejects any
///   version other than [`CPU_SNAPSHOT_VERSION`] (the caller-side
///   `Nes::restore_inner` already enforced this via a strict per-section
///   equality check before this bump — the upconvert path removed here
///   was dead code, unreachable through the only real caller).
pub const CPU_SNAPSHOT_VERSION: u8 = 3;

/// Encoded byte length of the version-1 CPU snapshot.
///
/// Layout: `version(1)` + 8 byte-fields for `a`/`x`/`y`/`s`/`p`/flags,
/// 2 bytes for `pc`, 8 bytes for `cycles`, plus `jammed`,
/// `pending_nmi`, `armed_nmi`, `pending_irq`, `armed_irq`,
/// `nmi_first_tick`, `irq_first_tick`, `irq_sample_i_flag`,
/// `cycles_emitted`, `skip_irq_sample` — all 1 byte each.
const ENCODED_LEN_V1: usize = 1 + 1 + 1 + 1 + 2 + 1 + 1 + 8 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1;

/// Encoded byte length of the version-2 CPU snapshot
/// (v1 + `master_clock` u64 + 5 R1 pipeline latches).
const ENCODED_LEN: usize = ENCODED_LEN_V1 + 8 + 5;

/// Errors returned by [`Cpu::restore`].
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum CpuSnapshotError {
    /// Blob length doesn't match the schema for the version tag we read.
    #[error("CPU snapshot truncated: expected {expected} bytes, got {got}")]
    Truncated {
        /// Expected byte count.
        expected: usize,
        /// Actual byte count.
        got: usize,
    },
    /// The blob's version byte is not understood by this build.
    #[error("CPU snapshot unsupported version {0}")]
    UnsupportedVersion(u8),
}

impl Cpu {
    /// Encode the CPU's mutable state into a versioned binary blob.
    ///
    /// Format is little-endian, version-tagged at offset 0. See
    /// [`CPU_SNAPSHOT_VERSION`] for the current schema number.
    #[must_use]
    pub fn snapshot(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(ENCODED_LEN);
        out.push(CPU_SNAPSHOT_VERSION);
        out.push(self.a);
        out.push(self.x);
        out.push(self.y);
        out.extend_from_slice(&self.pc.to_le_bytes());
        out.push(self.s);
        out.push(self.p.bits());
        out.extend_from_slice(&self.cycles.to_le_bytes());
        out.push(u8::from(self.jammed));
        out.push(u8::from(self.pending_nmi));
        out.push(u8::from(self.armed_nmi));
        out.push(u8::from(self.pending_irq));
        out.push(u8::from(self.armed_irq));
        out.push(self.nmi_first_tick);
        out.push(self.irq_first_tick);
        out.push(u8::from(self.irq_sample_i_flag));
        out.push(self.cycles_emitted);
        out.push(u8::from(self.skip_irq_sample));
        // v2 (W3-Stage-4): the R1 master-clock substrate pipeline. Written
        // unconditionally (zeros when `mc-r1-substrate` is off) so the blob
        // layout is identical across feature builds.
        {
            out.extend_from_slice(&self.master_clock.to_le_bytes());
            out.push(u8::from(self.mc_need_nmi));
            out.push(u8::from(self.mc_prev_need_nmi));
            out.push(u8::from(self.mc_run_irq));
            out.push(u8::from(self.mc_prev_run_irq));
            out.push(u8::from(self.mc_prev_nmi_line));
        }
        out
    }

    /// Decode a previously [`Cpu::snapshot`]ed blob back into `self`.
    ///
    /// # Errors
    ///
    /// Returns [`CpuSnapshotError`] if the blob is the wrong length or
    /// carries an unrecognized version.
    pub fn restore(&mut self, data: &[u8]) -> Result<(), CpuSnapshotError> {
        // Check the full expected length FIRST: a short-and-garbled blob
        // (e.g. truncated mid-write) is a truncation error, not a version
        // error, even if the one byte that happens to be present doesn't
        // match CPU_SNAPSHOT_VERSION -- checking length first makes that
        // the error callers see, which is the more useful diagnosis.
        if data.len() != ENCODED_LEN {
            return Err(CpuSnapshotError::Truncated {
                expected: ENCODED_LEN,
                got: data.len(),
            });
        }
        let version = data[0];
        // ADR 0028 (v2.0.0 rc.1): the v1/v2 upconvert path is retired. The
        // ONLY real caller, `Nes::restore_inner`, already rejects a
        // non-matching CPU section version via a strict equality check
        // before this function is ever reached — so accepting v1 here was
        // dead code. `Cpu::restore` now enforces the same strict-equality
        // contract directly, matching ADR 0003's MAJOR-boundary policy
        // ("no migration code paths are required... a v2.x line ... will
        // define explicit migration" — the explicit decision here IS
        // rejection, not a data transform).
        if version != CPU_SNAPSHOT_VERSION {
            return Err(CpuSnapshotError::UnsupportedVersion(version));
        }
        let mut p = 1;
        self.a = data[p];
        p += 1;
        self.x = data[p];
        p += 1;
        self.y = data[p];
        p += 1;
        self.pc = u16::from_le_bytes([data[p], data[p + 1]]);
        p += 2;
        self.s = data[p];
        p += 1;
        self.p = Status::from_bits_truncate(data[p]);
        p += 1;
        let mut c = [0u8; 8];
        c.copy_from_slice(&data[p..p + 8]);
        self.cycles = u64::from_le_bytes(c);
        p += 8;
        self.jammed = data[p] != 0;
        p += 1;
        self.pending_nmi = data[p] != 0;
        p += 1;
        self.armed_nmi = data[p] != 0;
        p += 1;
        self.pending_irq = data[p] != 0;
        p += 1;
        self.armed_irq = data[p] != 0;
        p += 1;
        self.nmi_first_tick = data[p];
        p += 1;
        self.irq_first_tick = data[p];
        p += 1;
        self.irq_sample_i_flag = data[p] != 0;
        p += 1;
        self.cycles_emitted = data[p];
        p += 1;
        self.skip_irq_sample = data[p] != 0;
        p += 1;
        // The master-clock substrate pipeline (unchanged layout since v2 —
        // see the CPU_SNAPSHOT_VERSION doc for what v3 actually changes).
        let mut mc = [0u8; 8];
        mc.copy_from_slice(&data[p..p + 8]);
        self.master_clock = u64::from_le_bytes(mc);
        self.mc_need_nmi = data[p + 8] != 0;
        self.mc_prev_need_nmi = data[p + 9] != 0;
        self.mc_run_irq = data[p + 10] != 0;
        self.mc_prev_run_irq = data[p + 11] != 0;
        self.mc_prev_nmi_line = data[p + 12] != 0;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    #[test]
    fn snapshot_round_trip() {
        let mut cpu = Cpu::new();
        cpu.a = 0xAB;
        cpu.x = 0x12;
        cpu.y = 0x34;
        cpu.pc = 0xC0DE;
        cpu.s = 0xF7;
        cpu.p = Status::from_bits_truncate(0xA4);
        cpu.cycles = 1_234_567;
        cpu.jammed = true;
        let blob = cpu.snapshot();
        assert_eq!(blob.len(), ENCODED_LEN);

        let mut other = Cpu::new();
        other.restore(&blob).unwrap();
        assert_eq!(other.a, 0xAB);
        assert_eq!(other.x, 0x12);
        assert_eq!(other.y, 0x34);
        assert_eq!(other.pc, 0xC0DE);
        assert_eq!(other.s, 0xF7);
        assert_eq!(other.p.bits(), 0xA4);
        assert_eq!(other.cycles, 1_234_567);
        assert!(other.jammed);
    }

    #[test]
    fn snapshot_rejects_short_blob() {
        let mut cpu = Cpu::new();
        let err = cpu.restore(&[CPU_SNAPSHOT_VERSION]).unwrap_err();
        assert!(matches!(err, CpuSnapshotError::Truncated { .. }));
    }

    #[test]
    fn snapshot_rejects_bad_version() {
        let mut cpu = Cpu::new();
        let err = cpu.restore(&[0xFF; ENCODED_LEN]).unwrap_err();
        assert!(matches!(err, CpuSnapshotError::UnsupportedVersion(0xFF)));
    }

    #[test]
    fn snapshot_rejects_pre_v3_versions() {
        // ADR 0028: the v2.0.0 MAJOR-boundary decision is clean rejection,
        // not an upconvert. A same-length blob tagged v1 or v2 (the two
        // schema versions that predate the one-clock promote) must be
        // rejected, not silently accepted as if it were v3.
        let mut cpu = Cpu::new();
        for old_version in [1u8, 2u8] {
            let mut blob = vec![old_version; ENCODED_LEN];
            blob[0] = old_version;
            let err = cpu.restore(&blob).unwrap_err();
            assert!(
                matches!(err, CpuSnapshotError::UnsupportedVersion(v) if v == old_version),
                "version {old_version} must be rejected, not upconverted"
            );
        }
    }

    #[test]
    fn snapshot_is_deterministic() {
        let mut cpu = Cpu::new();
        cpu.a = 0x42;
        let a = cpu.snapshot();
        let b = cpu.snapshot();
        assert_eq!(a, b);
    }
}
