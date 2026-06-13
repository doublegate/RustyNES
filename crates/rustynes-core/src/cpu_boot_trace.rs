//! Per-CPU-instruction boot-trace fixture (Session-12 observability).
//!
//! Records, for every CPU instruction boundary inside a caller-specified
//! `(cycle_range)` window, a [`CpuBootRecord`] that captures the CPU
//! register file plus the bus's cycle counter, PPU `(frame, scanline,
//! dot)`, the opcode about to execute, the CPU's NMI/IRQ pending state,
//! and a side-effect-free peek of the next 2 opcode bytes (so the diff
//! tool can disassemble divergence points without needing a separate
//! ROM dump).
//!
//! The fixture is the CPU-side analogue of the existing `irq_trace` and
//! `ppu-state-trace` infrastructure -- same design pattern (linear
//! buffer with overflow counter, public binary + CSV emitters,
//! integration-test consumer) applied to the boot-time CPU instruction
//! stream.
//!
//! # Why
//!
//! At `AccuracyCoin` cold-boot, `RustyNES` is `>=1` boot frame slower than
//! `Mesen2` at executing the RESET routine (Session-11 finding: frame 1
//! `Mesen2` has `ctrl=$01 v=$27C0 t=$27BF`, `RustyNES` has `ctrl=$00 v=$0001
//! t=$0000`). A per-instruction trace from both sides, diffed at the
//! cycle anchor, surfaces the EXACT instruction where the two emulators
//! first diverge -- a precondition to a load-bearing fix.
//!
//! See `docs/audit/cascade-a-investigation-2026-05-19.md` for the
//! Cascade-A residual context this fixture extends, and
//! `docs/ppu-trace-tooling.md` for the Session-10 PPU analogue.
//!
//! # Feature gating
//!
//! The recording code is gated on the `cpu-boot-trace` cargo feature
//! (off by default). When the feature is disabled, `Nes::run_frame` and
//! `Nes::step_instruction` do NOT call into this module -- every byte
//! of overhead is gone via `#[cfg(feature = "cpu-boot-trace")]` at the
//! call site. This module itself is feature-gated at the crate root so
//! it does not compile into the default build at all.
//!
//! # Output format
//!
//! Two parallel output formats live here:
//!
//! * **Binary** (default): a 12-byte ASCII magic `"RUSTYNES_CPU"`
//!   followed by a 2-byte little-endian schema version, a 2-byte
//!   reserved-for-flags field, and zero or more
//!   [`RECORD_SIZE`]-byte little-endian-packed [`CpuBootRecord`]s.
//!   The companion Lua script (`scripts/mesen2_cpu_boot_trace.lua`)
//!   emits the SAME format so the diff tool can compare both sides
//!   record-for-record.
//! * **CSV**: human-readable, one row per record, header line first.
//!   Same column order as the binary layout for ease of
//!   cross-reference.
//!
//! # Usage
//!
//! ```ignore
//! # use rustynes_core::Nes;
//! # use rustynes_core::cpu_boot_trace::{CpuBootTrace, CpuBootTraceConfig};
//! let mut nes = Nes::from_rom(&rom_bytes)?;
//! let cfg = CpuBootTraceConfig { cycle_range: 0..=200_000 };
//! nes.enable_cpu_boot_trace(CpuBootTrace::with_capacity(1_000_000, cfg));
//! for _ in 0..5 { nes.run_frame(); }
//! let trace = nes.take_cpu_boot_trace().unwrap();
//! std::fs::write("boot.bin", trace.to_binary()).unwrap();
//! # Ok::<(), rustynes_mappers::RomError>(())
//! ```

#![allow(dead_code)] // Most surfaces are only exercised when the feature is on.

use alloc::string::String;
use alloc::vec::Vec;
use core::ops::RangeInclusive;

/// Schema version for the binary trace layout. Bump on any
/// breaking change to [`CpuBootRecord`]'s byte layout or to the
/// magic/header format.
///
/// Version history:
///
/// * `1` (2026-05-20): initial Session-12 schema.
pub const CPU_BOOT_TRACE_SCHEMA_VERSION: u16 = 1;

/// Magic bytes prefixing every binary trace file. ASCII
/// "`RUSTYNES_CPU`" -- distinguishes our format from
/// `state_trace`'s `RUSTYNES_PPU` and `irq_trace`'s CSV files.
pub const BINARY_MAGIC: &[u8; 12] = b"RUSTYNES_CPU";

/// Length of a single [`CpuBootRecord`] in the packed binary layout.
///
/// Stable for the lifetime of [`CPU_BOOT_TRACE_SCHEMA_VERSION`].
pub const RECORD_SIZE: usize = 32;

/// Header length (magic + 2-byte schema version + 2-byte
/// reserved-for-flags). Records start at this offset.
pub const HEADER_SIZE: usize = BINARY_MAGIC.len() + 2 + 2;

/// One per-CPU-instruction trace record.
///
/// The byte layout matches the binary trace file format: every
/// field is serialized little-endian in declaration order. See
/// [`CpuBootRecord::to_bytes`] for the canonical encoder and
/// `RECORD_SIZE` for the total length.
///
/// Schema version: [`CPU_BOOT_TRACE_SCHEMA_VERSION`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CpuBootRecord {
    /// Cumulative CPU cycle counter at the start of this
    /// instruction (matches `LockstepBus::cycle()`).
    pub cycle: u64,
    /// PPU frame counter at the start of this instruction.
    pub frame: u32,
    /// PPU scanline at the start of this instruction (-1 ..=
    /// 260 NTSC).
    pub scanline: i16,
    /// PPU dot 0..=340 at the start of this instruction.
    pub dot: u16,
    /// Program counter at the start of this instruction.
    pub pc: u16,
    /// Accumulator.
    pub a: u8,
    /// X index register.
    pub x: u8,
    /// Y index register.
    pub y: u8,
    /// Status flags.
    pub p: u8,
    /// Stack pointer.
    pub s: u8,
    /// Opcode byte at PC (peeked side-effect-free via
    /// `debug_peek_cpu`).
    pub opcode: u8,
    /// Opcode operand byte 1 (PC + 1), peeked. Useful for the
    /// diff tool's disassembly when reporting divergences.
    pub op1: u8,
    /// Opcode operand byte 2 (PC + 2), peeked.
    pub op2: u8,
    /// 1 byte of bookkeeping flags, low bits 0..3:
    ///   bit 0 = CPU NMI line latched (`armed_nmi || pending_nmi`)
    ///   bit 1 = CPU IRQ line latched (`armed_irq || pending_irq`)
    ///   bit 2 = `bus.poll_nmi_edge_for_trace()` -- whether the
    ///           PPU has driven an NMI edge that hasn't been
    ///           consumed yet
    ///   bit 3 = `bus.irq_snapshot_mapper_at_high ||
    ///           irq_snapshot_apu_at_high` (the M2-high snapshot
    ///           the CPU samples at the second-to-last cycle)
    pub flags: u8,
}

const _RECORD_SIZE_CHECK: () = assert!(RECORD_SIZE == compute_record_size());

const fn compute_record_size() -> usize {
    // cycle(8) + frame(4) + scanline(2) + dot(2) + pc(2) +
    // a(1) + x(1) + y(1) + p(1) + s(1) + opcode(1) + op1(1) +
    // op2(1) + flags(1) + pad(5) = 32 bytes.
    //
    // Pad is intentional so future flags / accessor fields land
    // at a known offset without changing RECORD_SIZE.
    8 + 4 + 2 + 2 + 2 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 5
}

impl CpuBootRecord {
    /// Pack the record into a fixed-size [`RECORD_SIZE`]-byte
    /// little-endian buffer.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; RECORD_SIZE] {
        let mut buf = [0u8; RECORD_SIZE];
        let mut i = 0usize;
        let copy_u8 = |buf: &mut [u8; RECORD_SIZE], i: &mut usize, v: u8| {
            buf[*i] = v;
            *i += 1;
        };
        let copy_u16 = |buf: &mut [u8; RECORD_SIZE], i: &mut usize, v: u16| {
            buf[*i..*i + 2].copy_from_slice(&v.to_le_bytes());
            *i += 2;
        };
        let copy_i16 = |buf: &mut [u8; RECORD_SIZE], i: &mut usize, v: i16| {
            buf[*i..*i + 2].copy_from_slice(&v.to_le_bytes());
            *i += 2;
        };
        let copy_u32 = |buf: &mut [u8; RECORD_SIZE], i: &mut usize, v: u32| {
            buf[*i..*i + 4].copy_from_slice(&v.to_le_bytes());
            *i += 4;
        };
        let copy_u64 = |buf: &mut [u8; RECORD_SIZE], i: &mut usize, v: u64| {
            buf[*i..*i + 8].copy_from_slice(&v.to_le_bytes());
            *i += 8;
        };
        copy_u64(&mut buf, &mut i, self.cycle);
        copy_u32(&mut buf, &mut i, self.frame);
        copy_i16(&mut buf, &mut i, self.scanline);
        copy_u16(&mut buf, &mut i, self.dot);
        copy_u16(&mut buf, &mut i, self.pc);
        copy_u8(&mut buf, &mut i, self.a);
        copy_u8(&mut buf, &mut i, self.x);
        copy_u8(&mut buf, &mut i, self.y);
        copy_u8(&mut buf, &mut i, self.p);
        copy_u8(&mut buf, &mut i, self.s);
        copy_u8(&mut buf, &mut i, self.opcode);
        copy_u8(&mut buf, &mut i, self.op1);
        copy_u8(&mut buf, &mut i, self.op2);
        copy_u8(&mut buf, &mut i, self.flags);
        // 5 bytes of zero pad to round to 32.
        debug_assert_eq!(i + 5, RECORD_SIZE);
        buf
    }

    /// Decode a single record from a [`RECORD_SIZE`]-byte buffer.
    ///
    /// Returns `None` if the slice is too short.
    #[must_use]
    #[allow(clippy::many_single_char_names)] // Field names mirror CPU registers (a, x, y, p, s).
    pub fn from_bytes(buf: &[u8]) -> Option<Self> {
        if buf.len() < RECORD_SIZE {
            return None;
        }
        let mut i = 0usize;
        let read_u8 = |buf: &[u8], i: &mut usize| {
            let v = buf[*i];
            *i += 1;
            v
        };
        let read_u16 = |buf: &[u8], i: &mut usize| {
            let v = u16::from_le_bytes([buf[*i], buf[*i + 1]]);
            *i += 2;
            v
        };
        let read_i16 = |buf: &[u8], i: &mut usize| {
            let v = i16::from_le_bytes([buf[*i], buf[*i + 1]]);
            *i += 2;
            v
        };
        let read_u32 = |buf: &[u8], i: &mut usize| {
            let v = u32::from_le_bytes([buf[*i], buf[*i + 1], buf[*i + 2], buf[*i + 3]]);
            *i += 4;
            v
        };
        let read_u64 = |buf: &[u8], i: &mut usize| {
            let v = u64::from_le_bytes([
                buf[*i],
                buf[*i + 1],
                buf[*i + 2],
                buf[*i + 3],
                buf[*i + 4],
                buf[*i + 5],
                buf[*i + 6],
                buf[*i + 7],
            ]);
            *i += 8;
            v
        };
        let cycle = read_u64(buf, &mut i);
        let frame = read_u32(buf, &mut i);
        let scanline = read_i16(buf, &mut i);
        let dot = read_u16(buf, &mut i);
        let pc = read_u16(buf, &mut i);
        let a = read_u8(buf, &mut i);
        let x = read_u8(buf, &mut i);
        let y = read_u8(buf, &mut i);
        let p = read_u8(buf, &mut i);
        let s = read_u8(buf, &mut i);
        let opcode = read_u8(buf, &mut i);
        let op1 = read_u8(buf, &mut i);
        let op2 = read_u8(buf, &mut i);
        let flags = read_u8(buf, &mut i);
        Some(Self {
            cycle,
            frame,
            scanline,
            dot,
            pc,
            a,
            x,
            y,
            p,
            s,
            opcode,
            op1,
            op2,
            flags,
        })
    }
}

/// Per-trace filter configuration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CpuBootTraceConfig {
    /// Inclusive CPU-cycle range. The default for the `AccuracyCoin`
    /// boot investigation is `0..=200_000` (covers ~5 cold-boot
    /// frames at ~29,780 cycles/frame).
    pub cycle_range: RangeInclusive<u64>,
}

impl CpuBootTraceConfig {
    /// Build a config covering the given CPU-cycle range.
    #[must_use]
    pub const fn cycles(range: RangeInclusive<u64>) -> Self {
        Self { cycle_range: range }
    }

    /// Returns `true` if `cycle` is inside the filter window.
    #[must_use]
    pub fn contains(&self, cycle: u64) -> bool {
        self.cycle_range.contains(&cycle)
    }
}

/// Per-instruction CPU boot trace.
#[derive(Debug)]
pub struct CpuBootTrace {
    records: Vec<CpuBootRecord>,
    capacity: usize,
    overflow: u64,
    config: CpuBootTraceConfig,
}

impl CpuBootTrace {
    /// Allocate a trace buffer with `capacity` records and the
    /// given filter config.
    #[must_use]
    pub fn with_capacity(capacity: usize, config: CpuBootTraceConfig) -> Self {
        Self {
            records: Vec::with_capacity(capacity),
            capacity,
            overflow: 0,
            config,
        }
    }

    /// Number of records captured so far.
    #[must_use]
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// True if no records have been captured.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// Number of records dropped because the buffer was full.
    #[must_use]
    pub const fn overflow(&self) -> u64 {
        self.overflow
    }

    /// Borrow the filter config.
    #[must_use]
    pub const fn config(&self) -> &CpuBootTraceConfig {
        &self.config
    }

    /// Borrow the records.
    #[must_use]
    #[allow(clippy::missing_const_for_fn)] // Vec::as_slice is not const-stable yet.
    pub fn records(&self) -> &[CpuBootRecord] {
        &self.records
    }

    /// Push a new record IF it passes the filter and the buffer
    /// isn't full. Silently drops otherwise.
    pub fn maybe_push(&mut self, rec: CpuBootRecord) {
        if !self.config.contains(rec.cycle) {
            return;
        }
        if self.records.len() < self.capacity {
            self.records.push(rec);
        } else {
            self.overflow = self.overflow.saturating_add(1);
        }
    }

    /// Render the trace as binary bytes.
    #[must_use]
    pub fn to_binary(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(HEADER_SIZE + self.records.len() * RECORD_SIZE);
        out.extend_from_slice(BINARY_MAGIC);
        out.extend_from_slice(&CPU_BOOT_TRACE_SCHEMA_VERSION.to_le_bytes());
        out.extend_from_slice(&[0u8, 0u8]);
        for r in &self.records {
            out.extend_from_slice(&r.to_bytes());
        }
        out
    }

    /// Decode a binary trace into a [`CpuBootTrace`].
    ///
    /// # Errors
    ///
    /// Returns a string describing the parse failure: too short,
    /// bad magic, schema-version mismatch, or trailing bytes.
    pub fn from_binary(buf: &[u8]) -> Result<Self, String> {
        use core::fmt::Write as _;
        if buf.len() < HEADER_SIZE {
            return Err(alloc::format!(
                "trace too short: {} bytes (need at least {})",
                buf.len(),
                HEADER_SIZE
            ));
        }
        if &buf[..BINARY_MAGIC.len()] != BINARY_MAGIC.as_slice() {
            let mut msg = String::from("bad magic: ");
            for b in &buf[..BINARY_MAGIC.len().min(buf.len())] {
                let _ = write!(&mut msg, "{b:02X} ");
            }
            return Err(msg);
        }
        let ver = u16::from_le_bytes([buf[BINARY_MAGIC.len()], buf[BINARY_MAGIC.len() + 1]]);
        if ver != CPU_BOOT_TRACE_SCHEMA_VERSION {
            return Err(alloc::format!(
                "schema mismatch: file is v{ver}, this build expects v{CPU_BOOT_TRACE_SCHEMA_VERSION}"
            ));
        }
        let body = &buf[HEADER_SIZE..];
        if body.len() % RECORD_SIZE != 0 {
            return Err(alloc::format!(
                "body length {} is not a multiple of RECORD_SIZE={RECORD_SIZE}",
                body.len()
            ));
        }
        let n = body.len() / RECORD_SIZE;
        let mut records = Vec::with_capacity(n);
        for chunk in body.chunks_exact(RECORD_SIZE) {
            let rec = CpuBootRecord::from_bytes(chunk)
                .ok_or_else(|| String::from("CpuBootRecord::from_bytes returned None"))?;
            records.push(rec);
        }
        let cycle_range = records.first().map_or(0..=0, |r| {
            r.cycle..=records.last().map_or(r.cycle, |last| last.cycle)
        });
        Ok(Self {
            records,
            capacity: n,
            overflow: 0,
            config: CpuBootTraceConfig { cycle_range },
        })
    }

    /// Render the trace as a UTF-8 CSV string.
    #[must_use]
    pub fn to_csv(&self) -> String {
        use core::fmt::Write as _;
        let mut out = String::new();
        out.push_str("cycle,frame,scanline,dot,pc,a,x,y,p,s,opcode,op1,op2,flags\n");
        for r in &self.records {
            let _ = writeln!(
                out,
                "{},{},{},{},{:04X},{:02X},{:02X},{:02X},{:02X},{:02X},{:02X},{:02X},{:02X},{:02X}",
                r.cycle,
                r.frame,
                r.scanline,
                r.dot,
                r.pc,
                r.a,
                r.x,
                r.y,
                r.p,
                r.s,
                r.opcode,
                r.op1,
                r.op2,
                r.flags,
            );
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_record() -> CpuBootRecord {
        CpuBootRecord {
            cycle: 0x1234_5678,
            frame: 42,
            scanline: 17,
            dot: 271,
            pc: 0xC123,
            a: 0xAA,
            x: 0x55,
            y: 0x33,
            p: 0x34,
            s: 0xFD,
            opcode: 0x4C, // JMP abs
            op1: 0x00,
            op2: 0x80,
            flags: 0x05,
        }
    }

    #[test]
    fn record_size_constants_agree() {
        let bytes = sample_record().to_bytes();
        assert_eq!(bytes.len(), RECORD_SIZE);
    }

    #[test]
    fn record_roundtrips_through_packed_bytes() {
        let original = sample_record();
        let bytes = original.to_bytes();
        let decoded = CpuBootRecord::from_bytes(&bytes).expect("decode");
        assert_eq!(original, decoded);
    }

    #[test]
    fn binary_roundtrip_one_record() {
        let cfg = CpuBootTraceConfig::cycles(0..=u64::MAX);
        let mut trace = CpuBootTrace::with_capacity(16, cfg);
        trace.maybe_push(sample_record());
        let bytes = trace.to_binary();
        let parsed = CpuBootTrace::from_binary(&bytes).expect("parse");
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed.records()[0], trace.records()[0]);
    }

    #[test]
    fn binary_rejects_bad_magic() {
        let mut bytes = alloc::vec![0u8; HEADER_SIZE];
        bytes[0] = b'X';
        let err = CpuBootTrace::from_binary(&bytes).unwrap_err();
        assert!(err.contains("bad magic"), "expected bad-magic err: {err}");
    }

    #[test]
    fn binary_rejects_schema_mismatch() {
        let mut bytes = alloc::vec![0u8; HEADER_SIZE];
        bytes[..BINARY_MAGIC.len()].copy_from_slice(BINARY_MAGIC);
        let bogus = (CPU_BOOT_TRACE_SCHEMA_VERSION + 1).to_le_bytes();
        bytes[BINARY_MAGIC.len()] = bogus[0];
        bytes[BINARY_MAGIC.len() + 1] = bogus[1];
        let err = CpuBootTrace::from_binary(&bytes).unwrap_err();
        assert!(err.contains("schema mismatch"), "got: {err}");
    }

    #[test]
    fn binary_rejects_misaligned_body() {
        let mut bytes = alloc::vec![0u8; HEADER_SIZE + RECORD_SIZE - 1];
        bytes[..BINARY_MAGIC.len()].copy_from_slice(BINARY_MAGIC);
        let ver = CPU_BOOT_TRACE_SCHEMA_VERSION.to_le_bytes();
        bytes[BINARY_MAGIC.len()] = ver[0];
        bytes[BINARY_MAGIC.len() + 1] = ver[1];
        let err = CpuBootTrace::from_binary(&bytes).unwrap_err();
        assert!(err.contains("not a multiple"), "got: {err}");
    }

    #[test]
    fn maybe_push_respects_filter_and_capacity() {
        let cfg = CpuBootTraceConfig::cycles(0..=100);
        let mut trace = CpuBootTrace::with_capacity(2, cfg);
        for c in 0..5u64 {
            let mut r = sample_record();
            r.cycle = c;
            trace.maybe_push(r);
        }
        assert_eq!(trace.len(), 2);
        assert_eq!(trace.overflow(), 3);
        // Out-of-window record is dropped (not counted as overflow).
        let mut r = sample_record();
        r.cycle = 200;
        trace.maybe_push(r);
        assert_eq!(trace.len(), 2);
        assert_eq!(trace.overflow(), 3);
    }

    #[test]
    fn csv_header_includes_all_columns() {
        let cfg = CpuBootTraceConfig::cycles(0..=0);
        let mut trace = CpuBootTrace::with_capacity(2, cfg);
        trace.maybe_push(sample_record());
        let csv = trace.to_csv();
        let header = csv.lines().next().expect("header");
        for column in [
            "cycle", "frame", "scanline", "dot", "pc", "a", "x", "y", "p", "s", "opcode", "op1",
            "op2", "flags",
        ] {
            assert!(
                header.contains(column),
                "header missing `{column}`: {header}"
            );
        }
    }
}
