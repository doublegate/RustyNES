//! Per-dot capture of the addresses the PPU drives on its own bus.
//!
//! # Why this exists rather than deriving the address
//!
//! A 2C02 drives A0-A13 and /RD on real pins. The sequence of addresses it asks
//! for, dot by dot, is therefore a **hardware fact**: a logic analyser can record
//! it, and two correct implementations may not disagree about it. That makes it a
//! legitimate gate for an independent reimplementation, unlike the internal
//! latches, which are one decomposition among several.
//!
//! The address is *derivable* from [`v`](crate::Ppu) and PPUCTRL, both of which
//! the state trace already carries — and deriving it inside a comparator would
//! reimplement the addressing arithmetic that is the subject of the comparison.
//! A test that reimplements its subject agrees with itself forever, so the
//! address is captured where the PPU actually asks for it.
//!
//! # READS only, and why writes are deliberately absent
//!
//! A real 2C02 drives its address bus on writes too — a `$2007` write asserts an
//! address exactly as a fetch does — so a trace of reads alone is not a complete
//! picture of the bus. That omission was raised in review of #450 as a
//! correctness defect, and it is a scoped decision rather than an oversight.
//!
//! Adding writes today would BREAK the gate, for a reason unrelated to any RTL.
//! The DUT captures a dot only while its background fetch pipeline is driving
//! the bus; it has no write path to capture, because `$2007`-during-rendering is
//! v2.5.8's subject. Recording writes here would put records in the reference
//! that the DUT cannot produce, and the comparator — which requires the two
//! sequences to correspond — would report a divergence caused entirely by the
//! oracle having been made more complete than its counterpart.
//!
//! So the rule is the one this project applies to every narrowing: the scope is
//! stated rather than assumed. This is a trace of VRAM **reads**; the comparator
//! narrows further, to the background-fetch dots, and prints what it excluded on
//! every run. Writes join it when the DUT can drive them.
//!
//! # Cost
//!
//! Behind the `ppu-fetch-trace` feature and off unless a trace is installed. The
//! hook is one `Option` check on the VRAM read path; with the feature disabled
//! the module is not compiled at all.
//!
//! # What it is NOT
//!
//! Not a general-purpose profiler and not a save-state field. The buffer is
//! bounded, and a full buffer stops recording rather than growing or wrapping —
//! a wrapped buffer would silently change which window a comparison covers,
//! which is the kind of quiet coverage loss this project keeps finding.

use alloc::vec::Vec;

/// `RNESFTCH` plus a version byte pair, then the record size. 16 bytes total, so
/// the payload starts aligned and a reader can validate before allocating.
pub const MAGIC: &[u8; 12] = b"RNESFTCH\0\0\0\0";

/// Schema version. Bump on any layout change; a reader must refuse a version it
/// does not know rather than misinterpret the bytes.
pub const SCHEMA_VERSION: u16 = 1;

/// Bytes per encoded record.
pub const RECORD_SIZE: usize = 12;

/// One PPU bus read.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FetchRecord {
    /// Frame counter at the moment of the read.
    pub frame: u32,
    /// Scanline; `i16` carries the `-1` pre-render line some callers use.
    pub scanline: i16,
    /// Dot within the scanline, 0..=340.
    pub dot: u16,
    /// The address driven on the PPU bus, already masked to 14 bits.
    pub addr: u16,
}

impl FetchRecord {
    /// Encode explicitly, byte by byte.
    ///
    /// Never a struct-memory copy: padding bytes are uninitialised, and a golden
    /// file whose spare bytes differ run-to-run is a golden that cannot be
    /// compared.
    #[must_use]
    pub fn to_bytes(self) -> [u8; RECORD_SIZE] {
        let mut out = [0u8; RECORD_SIZE];
        out[0..4].copy_from_slice(&self.frame.to_le_bytes());
        out[4..6].copy_from_slice(&self.scanline.to_le_bytes());
        out[6..8].copy_from_slice(&self.dot.to_le_bytes());
        out[8..10].copy_from_slice(&self.addr.to_le_bytes());
        // out[10..12] stays zero: reserved, and explicitly written so the file
        // is byte-reproducible.
        out
    }

    /// Decode one record, or `None` if the slice is short.
    #[must_use]
    pub fn from_bytes(buf: &[u8]) -> Option<Self> {
        if buf.len() < RECORD_SIZE {
            return None;
        }
        // `try_into().ok()?` rather than hand-indexed byte arrays: the slice
        // lengths come from the field widths themselves, so a field that moves
        // cannot silently read the neighbouring one. It propagates rather than
        // panicking, which is why there is no `unwrap` here even though the
        // length check above already makes every conversion infallible.
        Some(Self {
            frame: u32::from_le_bytes(buf[0..4].try_into().ok()?),
            scanline: i16::from_le_bytes(buf[4..6].try_into().ok()?),
            dot: u16::from_le_bytes(buf[6..8].try_into().ok()?),
            addr: u16::from_le_bytes(buf[8..10].try_into().ok()?),
        })
    }
}

/// A bounded capture of PPU bus reads.
#[derive(Clone, Debug)]
pub struct FetchTrace {
    records: Vec<FetchRecord>,
    capacity: usize,
    /// Reads that arrived after the buffer filled.
    ///
    /// Reported rather than discarded silently: a comparison over a truncated
    /// window that does not know it is truncated claims a coverage it does not
    /// have.
    dropped: u64,
}

/// The most reads one trace will ever hold, however large a capacity is asked
/// for: 1,048,576 records, 12 MiB at [`RECORD_SIZE`].
///
/// The cap is on the STORED capacity and not merely on the initial allocation.
/// Clamping only the allocation, as an earlier version did, still lets a caller
/// grow the buffer without bound — a `--fetch-trace 1000000000` would have
/// reallocated its way to twelve gigabytes rather than being refused. A
/// co-simulation window that needs more than a million reads wants a shorter
/// window, not a larger buffer: three full frames of a rendering ROM is under
/// ten thousand.
///
/// Exceeding it is not silent. Records past the cap are counted as `dropped`,
/// and `nes_golden_export` exits non-zero on a non-zero drop count — so a
/// clamped run fails loudly instead of writing a golden that covers less than
/// the run it claims to.
pub const MAX_CAPACITY: usize = 1 << 20;

impl FetchTrace {
    /// A trace that records at most `capacity` reads, clamped to
    /// [`MAX_CAPACITY`].
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        let capacity = capacity.min(MAX_CAPACITY);
        Self {
            records: Vec::with_capacity(capacity),
            capacity,
            dropped: 0,
        }
    }

    /// The number of reads this trace will hold before dropping — the requested
    /// capacity after clamping, which is what a caller must check against its
    /// own request to know whether it was clamped.
    #[must_use]
    pub const fn capacity(&self) -> usize {
        self.capacity
    }

    /// Record one read, or count it as dropped once full.
    pub fn push(&mut self, rec: FetchRecord) {
        if self.records.len() < self.capacity {
            self.records.push(rec);
        } else {
            self.dropped = self.dropped.saturating_add(1);
        }
    }

    /// The captured reads, in order.
    #[must_use]
    pub fn records(&self) -> &[FetchRecord] {
        &self.records
    }

    /// How many reads did not fit.
    #[must_use]
    pub const fn dropped(&self) -> u64 {
        self.dropped
    }

    /// The 16-byte header followed by the encoded records.
    #[must_use]
    pub fn to_binary(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(16 + self.records.len() * RECORD_SIZE);
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&SCHEMA_VERSION.to_le_bytes());
        out.extend_from_slice(&(RECORD_SIZE as u16).to_le_bytes());
        for r in &self.records {
            out.extend_from_slice(&r.to_bytes());
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The encoding is a WIRE FORMAT: a Verilator testbench in another
    /// repository writes these bytes and `fetch_diff.py` reads them. A field
    /// silently changing width or order would be caught only by a divergence in
    /// a co-simulation run, which is several steps from the cause.
    ///
    /// The expected bytes are written out by hand rather than produced by
    /// calling `to_bytes`. A test that builds its expectation with the function
    /// under test agrees with itself forever — the failure mode recorded in
    /// `AGENTS.md` after a v2.4.0 test reimplemented its own subject.
    #[test]
    fn record_encoding_is_the_documented_byte_layout() {
        let rec = FetchRecord {
            frame: 0x1234_5678,
            scanline: -1,
            dot: 0x0155,
            addr: 0x23C0,
        };
        let bytes = rec.to_bytes();
        assert_eq!(bytes.len(), RECORD_SIZE);
        assert_eq!(
            bytes,
            [
                0x78, 0x56, 0x34, 0x12, // frame,    u32 LE
                0xFF, 0xFF, //             scanline, i16 LE (-1)
                0x55, 0x01, //             dot,      u16 LE
                0xC0, 0x23, //             addr,     u16 LE
                0x00, 0x00, //             reserved
            ]
        );
    }

    #[test]
    fn record_round_trips_including_a_negative_scanline() {
        // -1 is the pre-render line and the only negative value this field
        // carries, so it is the case a wrong signedness would break.
        for rec in [
            FetchRecord {
                frame: 0,
                scanline: -1,
                dot: 0,
                addr: 0,
            },
            FetchRecord {
                frame: u32::MAX,
                scanline: 260,
                dot: 340,
                addr: 0x3FFF,
            },
            FetchRecord {
                frame: 3,
                scanline: 35,
                dot: 130,
                addr: 0x2000,
            },
        ] {
            let back =
                FetchRecord::from_bytes(&rec.to_bytes()).expect("a full-length record must decode");
            assert_eq!(back, rec);
        }
    }

    #[test]
    fn a_short_buffer_decodes_to_none_rather_than_panicking() {
        let full = FetchRecord {
            frame: 1,
            scanline: 2,
            dot: 3,
            addr: 4,
        }
        .to_bytes();
        for n in 0..RECORD_SIZE {
            assert!(
                FetchRecord::from_bytes(&full[..n]).is_none(),
                "{n} bytes must not decode"
            );
        }
        assert!(FetchRecord::from_bytes(&full).is_some());
    }

    #[test]
    fn capacity_is_clamped_and_the_clamp_is_observable() {
        // The observable part is what makes the clamp safe to rely on: a caller
        // that asked for more can compare its request against `capacity()`.
        let t = FetchTrace::with_capacity(usize::MAX);
        assert_eq!(t.capacity(), MAX_CAPACITY);
        let t = FetchTrace::with_capacity(10);
        assert_eq!(t.capacity(), 10);
    }

    #[test]
    fn pushes_past_capacity_are_counted_not_wrapped() {
        let mut t = FetchTrace::with_capacity(2);
        for dot in 0..5u16 {
            t.push(FetchRecord {
                frame: 0,
                scanline: 0,
                dot,
                addr: dot,
            });
        }
        assert_eq!(t.records().len(), 2);
        assert_eq!(t.dropped(), 3);
        // The records KEPT are the first ones, not the last: a ring buffer here
        // would silently answer a question about the end of a run with data from
        // its middle.
        assert_eq!(t.records()[0].dot, 0);
        assert_eq!(t.records()[1].dot, 1);
    }
}
