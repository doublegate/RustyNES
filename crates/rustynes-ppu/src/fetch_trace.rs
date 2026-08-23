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
        Some(Self {
            frame: u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]),
            scanline: i16::from_le_bytes([buf[4], buf[5]]),
            dot: u16::from_le_bytes([buf[6], buf[7]]),
            addr: u16::from_le_bytes([buf[8], buf[9]]),
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

impl FetchTrace {
    /// A trace that records at most `capacity` reads.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            records: Vec::with_capacity(capacity.min(1 << 20)),
            capacity,
            dropped: 0,
        }
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
