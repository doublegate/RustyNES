//! Save-state encoding / decoding for the [`Cpu`].
//!
//! Per `CLAUDE.md` §Open questions: tagged-section per chip, version byte
//! up front, best-effort cross-version compatibility. This module owns the
//! version-1 schema for the CPU section.
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
/// - v2 (W3-Stage-4 promotion, 2026-06-10): appends the R1 master-clock
///   substrate pipeline — `master_clock` (u64) + the `mc_need_nmi` /
///   `mc_prev_need_nmi` / `mc_run_irq` / `mc_prev_run_irq` /
///   `mc_prev_nmi_line` latches (1 byte each). Written unconditionally
///   (zeros when `mc-r1-substrate` is off) so the layout is identical
///   across feature builds. v1 blobs upconvert best-effort: under
///   `mc-r1-substrate`, `master_clock` is re-derived as `cycles * 12`
///   (the NTSC master-clock rate; preserves the load-bearing phase
///   parity) and the pipeline latches default to quiescent `false`.
pub const CPU_SNAPSHOT_VERSION: u8 = 2;

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
        if data.is_empty() {
            return Err(CpuSnapshotError::Truncated {
                expected: ENCODED_LEN,
                got: 0,
            });
        }
        let version = data[0];
        if version != CPU_SNAPSHOT_VERSION && version != 1 {
            return Err(CpuSnapshotError::UnsupportedVersion(version));
        }
        let expected = if version >= 2 {
            ENCODED_LEN
        } else {
            ENCODED_LEN_V1
        };
        if data.len() != expected {
            return Err(CpuSnapshotError::Truncated {
                expected,
                got: data.len(),
            });
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
        // v2 (W3-Stage-4): the R1 master-clock substrate pipeline; v1 blobs
        // upconvert best-effort (master_clock re-derived from `cycles`, the
        // pipeline latches quiescent). When `mc-r1-substrate` is off the v2
        // bytes are consumed and discarded.
        if version >= 2 {
            {
                let mut mc = [0u8; 8];
                mc.copy_from_slice(&data[p..p + 8]);
                self.master_clock = u64::from_le_bytes(mc);
                self.mc_need_nmi = data[p + 8] != 0;
                self.mc_prev_need_nmi = data[p + 9] != 0;
                self.mc_run_irq = data[p + 10] != 0;
                self.mc_prev_run_irq = data[p + 11] != 0;
                self.mc_prev_nmi_line = data[p + 12] != 0;
            }
        } else {
            {
                self.master_clock = self.cycles.wrapping_mul(12);
                self.mc_need_nmi = false;
                self.mc_prev_need_nmi = false;
                self.mc_run_irq = false;
                self.mc_prev_run_irq = false;
                self.mc_prev_nmi_line = false;
            }
        }
        let _ = p;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn snapshot_is_deterministic() {
        let mut cpu = Cpu::new();
        cpu.a = 0x42;
        let a = cpu.snapshot();
        let b = cpu.snapshot();
        assert_eq!(a, b);
    }
}
