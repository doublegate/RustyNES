//! Save-state container format for `RustyNES` v2.
//!
//! Per `CLAUDE.md` "Open questions worth knowing": tagged-section per chip
//! with version byte; cross-version compatibility is best-effort, not
//! guaranteed.
//!
//! # On-wire layout
//!
//! ```text
//! HEADER (16 bytes):
//!     magic       : "RUSTYNES"  (8 bytes)
//!     format ver  : u16 little-endian  (currently 1)
//!     rom sha-256 : truncated to 6 bytes (sanity tag, not authoritative)
//!
//! BODY (sections in any order, each):
//!     tag         : [u8; 4]  e.g. b"CPU ", b"PPU ", b"APU ", b"MAP "
//!     version     : u8       per-section schema version
//!     length      : u32 little-endian (body bytes after this length field)
//!     body        : `length` bytes
//! ```
//!
//! Determinism: every `snapshot()` for a given `(seed, ROM, input sequence)`
//! produces bit-identical bytes. Loading is order-independent.

use alloc::{string::String, vec::Vec};
use thiserror::Error;

/// Magic header bytes — first 8 bytes of every `.rns` file.
pub const MAGIC: &[u8; 8] = b"RUSTYNES";

/// Current container-format version.
pub const FORMAT_VERSION: u16 = 1;

/// Length of the truncated ROM SHA-256 we embed in the header (sanity tag).
pub const ROM_HASH_TAG_LEN: usize = 6;

/// Header byte length.
pub const HEADER_LEN: usize = 8 + 2 + ROM_HASH_TAG_LEN;

/// Section tags. The fixed-width 4-byte format keeps parsing trivial and
/// avoids string allocation on the hot path.
pub mod tag {
    /// CPU (2A03 / 6502) state.
    pub const CPU: [u8; 4] = *b"CPU ";
    /// PPU (2C02) state.
    pub const PPU: [u8; 4] = *b"PPU ";
    /// APU (2A03 audio) state.
    pub const APU: [u8; 4] = *b"APU ";
    /// Mapper state (delegates to `Mapper::save_state`).
    pub const MAP: [u8; 4] = *b"MAP ";
    /// Bus / scheduler state (RAM, DMA, NMI edge latches, cycle counter).
    pub const BUS: [u8; 4] = *b"BUS ";
    /// Optional UI thumbnail (128x120 RGBA8 nearest-neighbor of the current
    /// framebuffer). NOT part of the deterministic save-state contract --
    /// frontends use it for slot pickers. See ADR 0003.
    pub const THM: [u8; 4] = *b"THM ";
}

/// Thumbnail width in pixels (1/2 native NES width).
pub const THUMBNAIL_WIDTH: usize = 128;
/// Thumbnail height in pixels (1/2 native NES height).
pub const THUMBNAIL_HEIGHT: usize = 120;
/// Thumbnail byte length (`THUMBNAIL_WIDTH * THUMBNAIL_HEIGHT * 4`, RGBA8).
pub const THUMBNAIL_LEN: usize = THUMBNAIL_WIDTH * THUMBNAIL_HEIGHT * 4;
/// Body version byte for the `THM ` section.
pub const THUMBNAIL_VERSION: u8 = 1;

/// Errors produced by save-state encode / decode.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum SnapshotError {
    /// The blob is shorter than the header.
    #[error("save state truncated: header needs {expected} bytes, got {got}")]
    HeaderTruncated {
        /// Expected byte count.
        expected: usize,
        /// Actual byte count.
        got: usize,
    },

    /// The magic prefix is wrong.
    #[error("save state magic mismatch: expected {:?}, got {got:?}", MAGIC)]
    BadMagic {
        /// Bytes observed at the magic offset.
        got: [u8; 8],
    },

    /// The container format version is outside the range we understand.
    #[error("save state container format version {got} not supported (max {max})")]
    UnsupportedFormat {
        /// Version we read.
        got: u16,
        /// Highest version we accept.
        max: u16,
    },

    /// A section body is shorter than its declared length.
    #[error("save state section {tag} body truncated: declared {declared} bytes, got {got}")]
    SectionTruncated {
        /// 4-byte tag (printable ASCII).
        tag: String,
        /// Declared length.
        declared: usize,
        /// Bytes actually available.
        got: usize,
    },

    /// A section had a version this build does not handle.
    #[error(
        "save state section {tag} version {file_version} not supported (chip supports {chip_supports})"
    )]
    VersionMismatch {
        /// 4-byte tag (printable ASCII).
        tag: String,
        /// Version recorded in the file.
        file_version: u8,
        /// Highest version the running chip accepts.
        chip_supports: u8,
    },

    /// A section's body failed internal consistency checks.
    #[error("save state section {tag}: {reason}")]
    SectionInvalid {
        /// 4-byte tag (printable ASCII).
        tag: String,
        /// Free-form reason.
        reason: String,
    },

    /// A required section was missing.
    #[error("save state missing required section {0}")]
    MissingSection(String),

    /// A section blob ran past EOF.
    #[error("save state truncated mid-section at offset {0}")]
    Eof(usize),
}

impl SnapshotError {
    /// Construct a [`Self::SectionInvalid`] with a borrowed tag.
    pub fn invalid(tag: [u8; 4], reason: impl Into<String>) -> Self {
        Self::SectionInvalid {
            tag: tag_string(tag),
            reason: reason.into(),
        }
    }
}

/// Render a 4-byte tag back into a `String` (lossy — non-ASCII becomes `?`).
#[must_use]
pub fn tag_string(t: [u8; 4]) -> String {
    let mut s = String::with_capacity(4);
    for b in t {
        s.push(if (0x20..=0x7E).contains(&b) {
            char::from(b)
        } else {
            '?'
        });
    }
    s
}

/// Cursor-style binary writer used by chip snapshot encoders.
///
/// Little-endian for all multi-byte integers; bools are 1 byte (`0` / `1`);
/// optional values are tagged with a presence byte.
#[derive(Debug, Default)]
pub struct BinWriter {
    buf: Vec<u8>,
}

impl BinWriter {
    /// Empty writer.
    #[must_use]
    pub const fn new() -> Self {
        Self { buf: Vec::new() }
    }

    /// Pre-sized writer.
    #[must_use]
    pub fn with_capacity(cap: usize) -> Self {
        Self {
            buf: Vec::with_capacity(cap),
        }
    }

    /// Take the inner buffer.
    #[must_use]
    pub fn into_vec(self) -> Vec<u8> {
        self.buf
    }

    /// Currently-accumulated byte count.
    #[must_use]
    pub fn len(&self) -> usize {
        self.buf.len()
    }

    /// `true` if no bytes have been written.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    /// Append one byte.
    pub fn u8(&mut self, v: u8) {
        self.buf.push(v);
    }

    /// Append a u16 little-endian.
    pub fn u16(&mut self, v: u16) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    /// Append a u32 little-endian.
    pub fn u32(&mut self, v: u32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    /// Append a u64 little-endian.
    pub fn u64(&mut self, v: u64) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    /// Append an i16 little-endian.
    pub fn i16(&mut self, v: i16) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    /// Append a bool as 1 byte.
    pub fn bool(&mut self, v: bool) {
        self.buf.push(u8::from(v));
    }

    /// Append a raw byte slice.
    pub fn bytes(&mut self, v: &[u8]) {
        self.buf.extend_from_slice(v);
    }

    /// Append a length-prefixed byte slice (u32 le length).
    pub fn lp_bytes(&mut self, v: &[u8]) {
        self.u32(u32::try_from(v.len()).expect("slice too large for save state"));
        self.bytes(v);
    }
}

/// Cursor-style binary reader, the inverse of [`BinWriter`].
#[derive(Debug)]
pub struct BinReader<'a> {
    src: &'a [u8],
    pos: usize,
}

impl<'a> BinReader<'a> {
    /// New reader.
    #[must_use]
    pub const fn new(src: &'a [u8]) -> Self {
        Self { src, pos: 0 }
    }

    /// Bytes remaining.
    #[must_use]
    pub const fn remaining(&self) -> usize {
        self.src.len() - self.pos
    }

    /// `true` if the cursor is at end-of-input.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.remaining() == 0
    }

    /// Current byte offset.
    #[must_use]
    pub const fn pos(&self) -> usize {
        self.pos
    }

    const fn need(&self, n: usize) -> Result<(), SnapshotError> {
        if self.remaining() < n {
            return Err(SnapshotError::Eof(self.pos));
        }
        Ok(())
    }

    /// Read one byte.
    ///
    /// # Errors
    ///
    /// Returns [`SnapshotError::Eof`] on EOF.
    pub fn u8(&mut self) -> Result<u8, SnapshotError> {
        self.need(1)?;
        let v = self.src[self.pos];
        self.pos += 1;
        Ok(v)
    }

    /// Read a u16 little-endian.
    ///
    /// # Errors
    ///
    /// Returns [`SnapshotError::Eof`] on EOF.
    pub fn u16(&mut self) -> Result<u16, SnapshotError> {
        self.need(2)?;
        let v = u16::from_le_bytes([self.src[self.pos], self.src[self.pos + 1]]);
        self.pos += 2;
        Ok(v)
    }

    /// Read a u32 little-endian.
    ///
    /// # Errors
    ///
    /// Returns [`SnapshotError::Eof`] on EOF.
    pub fn u32(&mut self) -> Result<u32, SnapshotError> {
        self.need(4)?;
        let mut a = [0u8; 4];
        a.copy_from_slice(&self.src[self.pos..self.pos + 4]);
        self.pos += 4;
        Ok(u32::from_le_bytes(a))
    }

    /// Read a u64 little-endian.
    ///
    /// # Errors
    ///
    /// Returns [`SnapshotError::Eof`] on EOF.
    pub fn u64(&mut self) -> Result<u64, SnapshotError> {
        self.need(8)?;
        let mut a = [0u8; 8];
        a.copy_from_slice(&self.src[self.pos..self.pos + 8]);
        self.pos += 8;
        Ok(u64::from_le_bytes(a))
    }

    /// Read an i16 little-endian.
    ///
    /// # Errors
    ///
    /// Returns [`SnapshotError::Eof`] on EOF.
    pub fn i16(&mut self) -> Result<i16, SnapshotError> {
        self.need(2)?;
        let v = i16::from_le_bytes([self.src[self.pos], self.src[self.pos + 1]]);
        self.pos += 2;
        Ok(v)
    }

    /// Read a bool (any non-zero byte counts as true).
    ///
    /// # Errors
    ///
    /// Returns [`SnapshotError::Eof`] on EOF.
    pub fn bool(&mut self) -> Result<bool, SnapshotError> {
        Ok(self.u8()? != 0)
    }

    /// Read `n` bytes (returns a borrowed slice).
    ///
    /// # Errors
    ///
    /// Returns [`SnapshotError::Eof`] on EOF.
    pub fn take(&mut self, n: usize) -> Result<&'a [u8], SnapshotError> {
        self.need(n)?;
        let s = &self.src[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }

    /// Read into a fixed-length destination.
    ///
    /// # Errors
    ///
    /// Returns [`SnapshotError::Eof`] on EOF.
    pub fn read_into(&mut self, dst: &mut [u8]) -> Result<(), SnapshotError> {
        let s = self.take(dst.len())?;
        dst.copy_from_slice(s);
        Ok(())
    }

    /// Read a length-prefixed byte slice (u32 le length followed by the bytes).
    ///
    /// # Errors
    ///
    /// Returns [`SnapshotError::Eof`] on EOF.
    pub fn lp_bytes(&mut self) -> Result<&'a [u8], SnapshotError> {
        let n = self.u32()? as usize;
        self.take(n)
    }
}

/// Encode a section header (`tag` + `version` + `length`) into `out` and
/// then append the body bytes.
pub fn write_section(out: &mut Vec<u8>, tag: [u8; 4], version: u8, body: &[u8]) {
    out.extend_from_slice(&tag);
    out.push(version);
    let len = u32::try_from(body.len()).expect("section body too large");
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(body);
}

/// Decoded view of one section's metadata + body slice.
#[derive(Debug, Clone, Copy)]
pub struct Section<'a> {
    /// Tag bytes (e.g. `b"CPU "`).
    pub tag: [u8; 4],
    /// Per-section schema version.
    pub version: u8,
    /// Body slice (does NOT include the header itself).
    pub body: &'a [u8],
}

/// Header decoded from the start of the blob.
#[derive(Debug, Clone)]
pub struct Header {
    /// Container format version (matches [`FORMAT_VERSION`] when written by
    /// this build).
    pub format_version: u16,
    /// Truncated ROM SHA-256 sanity tag.
    pub rom_hash_tag: [u8; ROM_HASH_TAG_LEN],
}

/// Parse the 16-byte header.
///
/// # Errors
///
/// - [`SnapshotError::HeaderTruncated`] if `bytes` is shorter than 16 bytes.
/// - [`SnapshotError::BadMagic`] on a bad prefix.
/// - [`SnapshotError::UnsupportedFormat`] if the format version is past
///   [`FORMAT_VERSION`].
pub fn parse_header(bytes: &[u8]) -> Result<(Header, usize), SnapshotError> {
    if bytes.len() < HEADER_LEN {
        return Err(SnapshotError::HeaderTruncated {
            expected: HEADER_LEN,
            got: bytes.len(),
        });
    }
    let mut magic = [0u8; 8];
    magic.copy_from_slice(&bytes[..8]);
    if &magic != MAGIC {
        return Err(SnapshotError::BadMagic { got: magic });
    }
    let format_version = u16::from_le_bytes([bytes[8], bytes[9]]);
    if format_version > FORMAT_VERSION {
        return Err(SnapshotError::UnsupportedFormat {
            got: format_version,
            max: FORMAT_VERSION,
        });
    }
    let mut rom_hash_tag = [0u8; ROM_HASH_TAG_LEN];
    rom_hash_tag.copy_from_slice(&bytes[10..16]);
    Ok((
        Header {
            format_version,
            rom_hash_tag,
        },
        HEADER_LEN,
    ))
}

/// Iterate sections starting at `bytes` (which should begin immediately
/// after the header).
pub struct SectionIter<'a> {
    src: &'a [u8],
    pos: usize,
}

impl<'a> SectionIter<'a> {
    /// New section iterator at the start of the body.
    #[must_use]
    pub const fn new(body: &'a [u8]) -> Self {
        Self { src: body, pos: 0 }
    }
}

impl<'a> Iterator for SectionIter<'a> {
    type Item = Result<Section<'a>, SnapshotError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.pos >= self.src.len() {
            return None;
        }
        // tag(4) + version(1) + len(4) = 9-byte section header.
        if self.src.len() - self.pos < 9 {
            return Some(Err(SnapshotError::Eof(self.pos)));
        }
        let mut tag = [0u8; 4];
        tag.copy_from_slice(&self.src[self.pos..self.pos + 4]);
        let version = self.src[self.pos + 4];
        let mut len_bytes = [0u8; 4];
        len_bytes.copy_from_slice(&self.src[self.pos + 5..self.pos + 9]);
        let len = u32::from_le_bytes(len_bytes) as usize;
        let body_start = self.pos + 9;
        let body_end = body_start + len;
        if body_end > self.src.len() {
            return Some(Err(SnapshotError::SectionTruncated {
                tag: tag_string(tag),
                declared: len,
                got: self.src.len() - body_start,
            }));
        }
        let body = &self.src[body_start..body_end];
        self.pos = body_end;
        Some(Ok(Section { tag, version, body }))
    }
}

/// Build the 16-byte header into `out`.
pub fn write_header(out: &mut Vec<u8>, rom_hash_tag: [u8; ROM_HASH_TAG_LEN]) {
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
    out.extend_from_slice(&rom_hash_tag);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_round_trip() {
        let mut out = Vec::new();
        write_header(&mut out, [1, 2, 3, 4, 5, 6]);
        assert_eq!(out.len(), HEADER_LEN);
        let (h, off) = parse_header(&out).unwrap();
        assert_eq!(h.format_version, FORMAT_VERSION);
        assert_eq!(h.rom_hash_tag, [1, 2, 3, 4, 5, 6]);
        assert_eq!(off, HEADER_LEN);
    }

    #[test]
    fn header_rejects_bad_magic() {
        let mut out = Vec::new();
        out.extend_from_slice(b"NOTRUSTY");
        out.extend_from_slice(&[0u8; HEADER_LEN - 8]);
        assert!(matches!(
            parse_header(&out),
            Err(SnapshotError::BadMagic { .. })
        ));
    }

    #[test]
    fn header_rejects_too_new_format() {
        let mut out = Vec::new();
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&u16::MAX.to_le_bytes());
        out.extend_from_slice(&[0u8; ROM_HASH_TAG_LEN]);
        assert!(matches!(
            parse_header(&out),
            Err(SnapshotError::UnsupportedFormat { .. })
        ));
    }

    #[test]
    fn section_iter_round_trip() {
        let mut out = Vec::new();
        write_section(&mut out, *b"AAAA", 1, &[1, 2, 3]);
        write_section(&mut out, *b"BBBB", 7, &[4, 5, 6, 7, 8]);
        let mut it = SectionIter::new(&out);
        let s1 = it.next().unwrap().unwrap();
        assert_eq!(&s1.tag, b"AAAA");
        assert_eq!(s1.version, 1);
        assert_eq!(s1.body, &[1, 2, 3]);
        let s2 = it.next().unwrap().unwrap();
        assert_eq!(&s2.tag, b"BBBB");
        assert_eq!(s2.version, 7);
        assert_eq!(s2.body, &[4, 5, 6, 7, 8]);
        assert!(it.next().is_none());
    }

    #[test]
    fn binwriter_round_trip() {
        let mut w = BinWriter::new();
        w.u8(0x12);
        w.u16(0x3456);
        w.u32(0x789A_BCDE);
        w.u64(0xFEED_FACE_CAFE_BEEF);
        w.bool(true);
        w.bool(false);
        w.bytes(&[0xAA, 0xBB]);
        let buf = w.into_vec();
        let mut r = BinReader::new(&buf);
        assert_eq!(r.u8().unwrap(), 0x12);
        assert_eq!(r.u16().unwrap(), 0x3456);
        assert_eq!(r.u32().unwrap(), 0x789A_BCDE);
        assert_eq!(r.u64().unwrap(), 0xFEED_FACE_CAFE_BEEF);
        assert!(r.bool().unwrap());
        assert!(!r.bool().unwrap());
        assert_eq!(r.take(2).unwrap(), &[0xAA, 0xBB]);
        assert!(r.is_empty());
    }

    #[test]
    fn binreader_eof_errors() {
        let buf = [0x12u8];
        let mut r = BinReader::new(&buf);
        assert!(r.u8().is_ok());
        assert!(matches!(r.u8(), Err(SnapshotError::Eof(_))));
    }
}
