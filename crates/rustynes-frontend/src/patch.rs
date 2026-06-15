//! ROM soft-patching (v1.2.0, Workstream B, ROM ingest).
//!
//! Self-contained appliers for the three common NES ROM soft-patch formats:
//! IPS, UPS, and BPS. Each takes the in-memory base ROM plus a patch byte
//! buffer and returns a freshly-allocated patched ROM, leaving the inputs
//! untouched.
//!
//! This is **frontend-only**: patches are applied to the raw ROM bytes before
//! the file is handed to `rustynes-core`, so the emulation core, the test
//! suites, and the determinism contract are unaffected. A patched ROM is just
//! a different ROM as far as the core is concerned.
//!
//! ## Formats
//!
//! - **IPS** — the classic International Patching System: an ASCII `PATCH`
//!   header, then a stream of records (3-byte big-endian offset + 2-byte
//!   big-endian length, or a length of `0` introducing an RLE run), terminated
//!   by the ASCII marker `EOF`. An optional trailing 3-byte big-endian value
//!   after `EOF` truncates the output.
//! - **UPS** — byuu's format: a `UPS1` magic, variable-width input/output file
//!   sizes, then XOR-diff blocks (a variable-width relative offset followed by
//!   XOR bytes terminated by `0x00`), and finally three little-endian CRC32
//!   footers (input, output, patch). The input CRC is verified before applying
//!   and the output CRC after.
//! - **BPS** — byuu's successor format: a `BPS1` magic, variable-width
//!   source/target/metadata sizes, an action stream of `SourceRead` /
//!   `TargetRead` / `SourceCopy` / `TargetCopy` commands using variable-width
//!   encoding, and three little-endian CRC32 footers (source, target, patch).
//!   The source CRC is verified before applying and the target CRC after.
//!
//! The CRC32 used by UPS and BPS is the standard IEEE / zip CRC-32 (reflected,
//! polynomial `0xEDB8_8320`), the same one `game_db` uses.

use thiserror::Error;

/// Errors raised by the soft-patch appliers.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum PatchError {
    /// The patch did not begin with the expected magic / header bytes.
    #[error("invalid {format} patch: bad magic or header")]
    BadMagic {
        /// Format that was attempted (`IPS` / `UPS` / `BPS`).
        format: &'static str,
    },
    /// The patch ended before a complete record could be read.
    #[error("truncated {format} patch")]
    Truncated {
        /// Format that was attempted.
        format: &'static str,
    },
    /// A record or footer was otherwise malformed (bad varint, bad length).
    #[error("malformed {format} patch: {detail}")]
    Malformed {
        /// Format that was attempted.
        format: &'static str,
        /// Human-readable detail.
        detail: &'static str,
    },
    /// The base ROM's CRC32 did not match the value the patch expects.
    #[error("source CRC32 mismatch: ROM is 0x{actual:08x}, patch expects 0x{expected:08x}")]
    SourceCrcMismatch {
        /// CRC32 the patch records for the source ROM.
        expected: u32,
        /// CRC32 actually computed over the supplied ROM.
        actual: u32,
    },
    /// The produced output's CRC32 did not match the value the patch expects.
    #[error("output CRC32 mismatch: produced 0x{actual:08x}, patch expects 0x{expected:08x}")]
    OutputCrcMismatch {
        /// CRC32 the patch records for the output ROM.
        expected: u32,
        /// CRC32 actually computed over the produced output.
        actual: u32,
    },
    /// A patch record addressed an offset outside the valid range.
    #[error("{format} patch offset out of range")]
    OffsetOutOfRange {
        /// Format that was attempted.
        format: &'static str,
    },
    /// The extension passed to [`detect_and_apply`] is not a known format.
    #[error("unknown patch extension: {0:?}")]
    UnknownExtension(String),
}

/// IEEE CRC-32 (reflected, polynomial `0xEDB8_8320`) — the zip/PNG CRC, matching
/// `game_db`'s `crc32`. Table-less; soft-patching is not a hot path.
#[must_use]
fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

/// Read a UPS/BPS variable-width (base-128, little-endian) integer.
///
/// Each byte contributes 7 bits; the high bit (`0x80`) marks the final byte.
/// After consuming a byte's 7 payload bits the running value has `1` added
/// before the next shift — the canonical beat/byuu varint, so that the value
/// space has no redundant encodings.
///
/// Advances `pos` past the consumed bytes. Returns `None` if the data runs out
/// before a terminating byte is seen.
fn read_vuint(data: &[u8], pos: &mut usize) -> Option<u64> {
    let mut value: u64 = 0;
    let mut shift: u32 = 0;
    loop {
        let byte = *data.get(*pos)?;
        *pos += 1;
        // The low 7 bits are the payload; `value |= (byte & 0x7f) << shift`.
        value = value.wrapping_add(u64::from(byte & 0x7f).wrapping_shl(shift));
        if byte & 0x80 != 0 {
            return Some(value);
        }
        shift += 7;
        // Add 1 << shift after each non-terminal byte (the +1 bias).
        value = value.wrapping_add(1u64.wrapping_shl(shift));
    }
}

/// Apply an IPS soft-patch to `rom`, returning the patched ROM.
///
/// # Errors
///
/// Returns [`PatchError`] if the patch is missing its `PATCH` header, is
/// truncated mid-record, or never reaches the `EOF` terminator.
pub fn apply_ips(rom: &[u8], patch: &[u8]) -> Result<Vec<u8>, PatchError> {
    const FMT: &str = "IPS";
    let truncated = || PatchError::Truncated { format: FMT };

    if patch.len() < 5 || &patch[0..5] != b"PATCH" {
        return Err(PatchError::BadMagic { format: FMT });
    }

    let mut out = rom.to_vec();
    let mut pos = 5usize;

    loop {
        // Need at least 3 bytes for the next offset (or "EOF").
        if pos + 3 > patch.len() {
            return Err(truncated());
        }
        if &patch[pos..pos + 3] == b"EOF" {
            pos += 3;
            break;
        }
        let offset = (usize::from(patch[pos]) << 16)
            | (usize::from(patch[pos + 1]) << 8)
            | usize::from(patch[pos + 2]);
        pos += 3;

        if pos + 2 > patch.len() {
            return Err(truncated());
        }
        let length = (usize::from(patch[pos]) << 8) | usize::from(patch[pos + 1]);
        pos += 2;

        if length == 0 {
            // RLE record: 2-byte run length, then 1 byte to repeat.
            if pos + 3 > patch.len() {
                return Err(truncated());
            }
            let run = (usize::from(patch[pos]) << 8) | usize::from(patch[pos + 1]);
            let value = patch[pos + 2];
            pos += 3;
            let end = offset
                .checked_add(run)
                .ok_or(PatchError::OffsetOutOfRange { format: FMT })?;
            if end > out.len() {
                out.resize(end, 0);
            }
            out[offset..end].fill(value);
        } else {
            // Normal record: `length` literal bytes follow.
            if pos + length > patch.len() {
                return Err(truncated());
            }
            let end = offset
                .checked_add(length)
                .ok_or(PatchError::OffsetOutOfRange { format: FMT })?;
            if end > out.len() {
                out.resize(end, 0);
            }
            out[offset..end].copy_from_slice(&patch[pos..pos + length]);
            pos += length;
        }
    }

    // Optional 3-byte big-endian truncation extension after EOF.
    if pos + 3 <= patch.len() {
        let truncate_to = (usize::from(patch[pos]) << 16)
            | (usize::from(patch[pos + 1]) << 8)
            | usize::from(patch[pos + 2]);
        out.truncate(truncate_to);
    }

    Ok(out)
}

/// Apply a UPS soft-patch to `rom`, returning the patched ROM.
///
/// Verifies the input CRC32 before applying and the output CRC32 afterwards.
///
/// # Errors
///
/// Returns [`PatchError`] if the patch is missing its `UPS1` magic, is
/// truncated, encodes an out-of-range offset, or fails either CRC32 check.
pub fn apply_ups(rom: &[u8], patch: &[u8]) -> Result<Vec<u8>, PatchError> {
    const FMT: &str = "UPS";
    let truncated = || PatchError::Truncated { format: FMT };

    if patch.len() < 4 || &patch[0..4] != b"UPS1" {
        return Err(PatchError::BadMagic { format: FMT });
    }
    // Three little-endian CRC32s occupy the final 12 bytes.
    if patch.len() < 4 + 12 {
        return Err(truncated());
    }
    let footer_start = patch.len() - 12;
    let in_crc = read_le_u32(&patch[footer_start..footer_start + 4]);
    let out_crc = read_le_u32(&patch[footer_start + 4..footer_start + 8]);
    let patch_crc_expected = read_le_u32(&patch[footer_start + 8..footer_start + 12]);

    // Verify the patch's own integrity (CRC of everything but the last 4 bytes).
    let patch_crc_actual = crc32(&patch[..footer_start + 8]);
    if patch_crc_actual != patch_crc_expected {
        return Err(PatchError::Malformed {
            format: FMT,
            detail: "patch CRC32 self-check failed",
        });
    }

    // Verify the source ROM up front.
    let actual_in = crc32(rom);
    if actual_in != in_crc {
        return Err(PatchError::SourceCrcMismatch {
            expected: in_crc,
            actual: actual_in,
        });
    }

    let mut pos = 4usize;
    let in_size = read_vuint(patch, &mut pos).ok_or_else(truncated)?;
    let out_size = read_vuint(patch, &mut pos).ok_or_else(truncated)?;
    let out_size_usize =
        usize::try_from(out_size).map_err(|_| PatchError::OffsetOutOfRange { format: FMT })?;
    let _ = in_size;

    // Start from the source, padded/truncated to the declared output size.
    let mut out = rom.to_vec();
    out.resize(out_size_usize, 0);

    let mut cursor = 0usize;
    while pos < footer_start {
        let rel = read_vuint(patch, &mut pos).ok_or_else(truncated)?;
        let rel = usize::try_from(rel).map_err(|_| PatchError::OffsetOutOfRange { format: FMT })?;
        cursor = cursor
            .checked_add(rel)
            .ok_or(PatchError::OffsetOutOfRange { format: FMT })?;
        // XOR bytes until a 0x00 terminator (which is itself applied).
        loop {
            let byte = *patch.get(pos).ok_or_else(truncated)?;
            pos += 1;
            if cursor >= out.len() {
                return Err(PatchError::OffsetOutOfRange { format: FMT });
            }
            out[cursor] ^= byte;
            cursor += 1;
            if byte == 0x00 {
                break;
            }
        }
    }

    // Verify the produced output.
    let actual_out = crc32(&out);
    if actual_out != out_crc {
        return Err(PatchError::OutputCrcMismatch {
            expected: out_crc,
            actual: actual_out,
        });
    }

    Ok(out)
}

/// Apply a BPS soft-patch to `rom`, returning the patched ROM.
///
/// Verifies the source CRC32 before applying and the target CRC32 afterwards.
///
/// # Errors
///
/// Returns [`PatchError`] if the patch is missing its `BPS1` magic, is
/// truncated, encodes an out-of-range action, or fails either CRC32 check.
pub fn apply_bps(rom: &[u8], patch: &[u8]) -> Result<Vec<u8>, PatchError> {
    const FMT: &str = "BPS";
    let truncated = || PatchError::Truncated { format: FMT };
    let oor = || PatchError::OffsetOutOfRange { format: FMT };

    if patch.len() < 4 || &patch[0..4] != b"BPS1" {
        return Err(PatchError::BadMagic { format: FMT });
    }
    if patch.len() < 4 + 12 {
        return Err(truncated());
    }
    let footer_start = patch.len() - 12;
    let src_crc = read_le_u32(&patch[footer_start..footer_start + 4]);
    let tgt_crc = read_le_u32(&patch[footer_start + 4..footer_start + 8]);
    let patch_crc_expected = read_le_u32(&patch[footer_start + 8..footer_start + 12]);

    let patch_crc_actual = crc32(&patch[..footer_start + 8]);
    if patch_crc_actual != patch_crc_expected {
        return Err(PatchError::Malformed {
            format: FMT,
            detail: "patch CRC32 self-check failed",
        });
    }

    let actual_src = crc32(rom);
    if actual_src != src_crc {
        return Err(PatchError::SourceCrcMismatch {
            expected: src_crc,
            actual: actual_src,
        });
    }

    let mut pos = 4usize;
    let _source_size = read_vuint(patch, &mut pos).ok_or_else(truncated)?;
    let target_size = read_vuint(patch, &mut pos).ok_or_else(truncated)?;
    let target_size = usize::try_from(target_size).map_err(|_| oor())?;
    let metadata_size = read_vuint(patch, &mut pos).ok_or_else(truncated)?;
    let metadata_size = usize::try_from(metadata_size).map_err(|_| oor())?;
    // Skip metadata (typically an XML blob; we don't consume it).
    pos = pos.checked_add(metadata_size).ok_or_else(oor)?;
    if pos > footer_start {
        return Err(truncated());
    }

    let mut out: Vec<u8> = Vec::with_capacity(target_size);
    // Relative-copy cursors, signed offsets are decoded from the varint.
    let mut source_rel_offset: usize = 0;
    let mut target_rel_offset: usize = 0;

    while pos < footer_start {
        let data = read_vuint(patch, &mut pos).ok_or_else(truncated)?;
        let command = data & 0b11;
        let length = usize::try_from((data >> 2) + 1).map_err(|_| oor())?;

        match command {
            // SourceRead: copy `length` bytes from the source at the current
            // output position.
            0 => {
                let start = out.len();
                let end = start.checked_add(length).ok_or_else(oor)?;
                if end > rom.len() {
                    return Err(oor());
                }
                out.extend_from_slice(&rom[start..end]);
            }
            // TargetRead: `length` literal bytes follow in the patch stream.
            1 => {
                let end = pos.checked_add(length).ok_or_else(oor)?;
                if end > footer_start {
                    return Err(truncated());
                }
                out.extend_from_slice(&patch[pos..end]);
                pos = end;
            }
            // SourceCopy: a signed relative offset adjusts a source cursor,
            // then `length` bytes are copied from it.
            2 => {
                let raw = read_vuint(patch, &mut pos).ok_or_else(truncated)?;
                apply_signed_offset(&mut source_rel_offset, raw)?;
                let end = source_rel_offset.checked_add(length).ok_or_else(oor)?;
                if end > rom.len() {
                    return Err(oor());
                }
                out.extend_from_slice(&rom[source_rel_offset..end]);
                source_rel_offset = end;
            }
            // TargetCopy: like SourceCopy but the cursor walks the output being
            // built (so it can reference bytes just written; copy one at a time).
            3 => {
                let raw = read_vuint(patch, &mut pos).ok_or_else(truncated)?;
                apply_signed_offset(&mut target_rel_offset, raw)?;
                for _ in 0..length {
                    if target_rel_offset >= out.len() {
                        return Err(oor());
                    }
                    let byte = out[target_rel_offset];
                    out.push(byte);
                    target_rel_offset += 1;
                }
            }
            _ => unreachable!("command is masked to two bits"),
        }
    }

    if out.len() != target_size {
        return Err(PatchError::Malformed {
            format: FMT,
            detail: "action stream produced wrong target size",
        });
    }

    let actual_tgt = crc32(&out);
    if actual_tgt != tgt_crc {
        return Err(PatchError::OutputCrcMismatch {
            expected: tgt_crc,
            actual: actual_tgt,
        });
    }

    Ok(out)
}

/// Apply the BPS signed relative-offset encoding to a cursor.
///
/// BPS encodes a signed delta as `(abs << 1) | sign`, where `sign == 1` means
/// negative. The decoded delta is added to (or subtracted from) `cursor`.
fn apply_signed_offset(cursor: &mut usize, raw: u64) -> Result<(), PatchError> {
    let oor = || PatchError::OffsetOutOfRange { format: "BPS" };
    let magnitude = usize::try_from(raw >> 1).map_err(|_| oor())?;
    if raw & 1 == 0 {
        *cursor = cursor.checked_add(magnitude).ok_or_else(oor)?;
    } else {
        *cursor = cursor.checked_sub(magnitude).ok_or_else(oor)?;
    }
    Ok(())
}

/// Read a little-endian `u32` from a 4-byte slice.
fn read_le_u32(bytes: &[u8]) -> u32 {
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

/// Apply a soft-patch, dispatching on the (case-insensitive) file extension.
///
/// `ext` is the patch file's extension without the leading dot, e.g. `"ips"`.
///
/// # Errors
///
/// Returns [`PatchError::UnknownExtension`] for an unrecognized extension, or
/// whatever the format-specific applier returns.
pub fn detect_and_apply(rom: &[u8], patch: &[u8], ext: &str) -> Result<Vec<u8>, PatchError> {
    match ext.to_ascii_lowercase().as_str() {
        "ips" => apply_ips(rom, patch),
        "ups" => apply_ups(rom, patch),
        "bps" => apply_bps(rom, patch),
        other => Err(PatchError::UnknownExtension(other.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- shared varint helper (mirror of the canonical encoder) ------------

    /// Encode a UPS/BPS variable-width integer (inverse of [`read_vuint`]).
    fn write_vuint(value: u64, out: &mut Vec<u8>) {
        let mut value = value;
        loop {
            let byte = u8::try_from(value & 0x7f).expect("masked to 7 bits");
            value >>= 7;
            if value == 0 {
                out.push(byte | 0x80);
                break;
            }
            out.push(byte);
            value -= 1;
        }
    }

    fn push_le_u32(value: u32, out: &mut Vec<u8>) {
        out.extend_from_slice(&value.to_le_bytes());
    }

    #[test]
    fn vuint_round_trips() {
        for &v in &[
            0u64,
            1,
            127,
            128,
            255,
            256,
            16383,
            16384,
            1_000_000,
            u64::from(u32::MAX),
        ] {
            let mut buf = Vec::new();
            write_vuint(v, &mut buf);
            let mut pos = 0;
            assert_eq!(read_vuint(&buf, &mut pos), Some(v), "value {v}");
            assert_eq!(pos, buf.len(), "consumed all bytes for {v}");
        }
    }

    // ---- IPS ---------------------------------------------------------------

    #[test]
    fn ips_normal_and_rle_records() {
        // Base ROM: 16 bytes of 0x00.
        let rom = vec![0u8; 16];

        // Patch: PATCH | normal record (offset 2, len 3 = AA BB CC)
        //              | RLE record (offset 8, len 0, run 4, value 0xFF)
        //              | EOF
        let mut patch = Vec::new();
        patch.extend_from_slice(b"PATCH");
        // Normal record at offset 0x000002, length 0x0003.
        patch.extend_from_slice(&[0x00, 0x00, 0x02]);
        patch.extend_from_slice(&[0x00, 0x03]);
        patch.extend_from_slice(&[0xAA, 0xBB, 0xCC]);
        // RLE record at offset 0x000008, length 0 => run 0x0004 of 0xFF.
        patch.extend_from_slice(&[0x00, 0x00, 0x08]);
        patch.extend_from_slice(&[0x00, 0x00]);
        patch.extend_from_slice(&[0x00, 0x04, 0xFF]);
        patch.extend_from_slice(b"EOF");

        let out = apply_ips(&rom, &patch).expect("ips applies");
        let mut expected = vec![0u8; 16];
        expected[2] = 0xAA;
        expected[3] = 0xBB;
        expected[4] = 0xCC;
        for b in expected.iter_mut().skip(8).take(4) {
            *b = 0xFF;
        }
        assert_eq!(out, expected);
    }

    #[test]
    fn ips_extends_rom_when_record_runs_past_end() {
        let rom = vec![0u8; 4];
        let mut patch = Vec::new();
        patch.extend_from_slice(b"PATCH");
        // Write 2 bytes at offset 6 (past the 4-byte ROM): output must grow.
        patch.extend_from_slice(&[0x00, 0x00, 0x06]);
        patch.extend_from_slice(&[0x00, 0x02]);
        patch.extend_from_slice(&[0x11, 0x22]);
        patch.extend_from_slice(b"EOF");

        let out = apply_ips(&rom, &patch).expect("ips applies");
        assert_eq!(out.len(), 8);
        assert_eq!(&out[6..8], &[0x11, 0x22]);
        assert_eq!(&out[0..4], &[0, 0, 0, 0]);
    }

    #[test]
    fn ips_truncation_extension() {
        let rom = vec![0xAAu8; 16];
        let mut patch = Vec::new();
        patch.extend_from_slice(b"PATCH");
        patch.extend_from_slice(b"EOF");
        // Truncate to 8 bytes.
        patch.extend_from_slice(&[0x00, 0x00, 0x08]);

        let out = apply_ips(&rom, &patch).expect("ips applies");
        assert_eq!(out.len(), 8);
        assert!(out.iter().all(|&b| b == 0xAA));
    }

    #[test]
    fn ips_bad_magic_rejected() {
        let rom = vec![0u8; 4];
        let err = apply_ips(&rom, b"NOPE0EOF").unwrap_err();
        assert!(matches!(err, PatchError::BadMagic { format: "IPS" }));
    }

    #[test]
    fn ips_truncated_rejected() {
        // "PATCH" but no EOF and not enough bytes for a record.
        let rom = vec![0u8; 4];
        let err = apply_ips(&rom, b"PATCH\x00").unwrap_err();
        assert!(matches!(err, PatchError::Truncated { format: "IPS" }));
    }

    /// Encode a length (`usize`) as a varint without a lossy cast.
    fn write_len(len: usize, out: &mut Vec<u8>) {
        write_vuint(u64::try_from(len).expect("test lengths fit u64"), out);
    }

    // ---- UPS ---------------------------------------------------------------

    #[test]
    fn ups_xor_diff_round_trips_with_crcs() {
        // Use a source and dest that differ; pad with a trailing byte so the
        // 0x00 block terminator has a valid landing position.
        let src = vec![0x10u8, 0x20, 0x30, 0x40, 0x00];
        let dst = vec![0x11u8, 0x22, 0x30, 0x44, 0x00];

        // Build the patch by hand for full control of the terminator.
        let mut patch = Vec::new();
        patch.extend_from_slice(b"UPS1");
        write_len(src.len(), &mut patch);
        write_len(dst.len(), &mut patch);
        // In UPS a 0x00 byte terminates the current XOR block, so a block can
        // only span consecutive *differing* bytes; matching bytes (where the
        // XOR diff is 0) must fall on a block terminator. Indices 0,1 differ and
        // index 2 matches (0x30), so block 1 = [diff0, diff1, 0x00-at-index-2];
        // index 3 differs and index 4 matches (0x00), so block 2 = [diff3,
        // 0x00-at-index-4]. The terminators land on matching bytes, leaving them
        // unchanged.
        write_vuint(0, &mut patch); // block 1: rel offset 0
        patch.push(src[0] ^ dst[0]); // index 0 (0x01)
        patch.push(src[1] ^ dst[1]); // index 1 (0x02)
        patch.push(0x00); // index 2: terminator on the matching 0x30
        write_vuint(0, &mut patch); // block 2: rel offset 0 (resume at index 3)
        patch.push(src[3] ^ dst[3]); // index 3 (0x04)
        patch.push(0x00); // index 4: terminator on the matching 0x00

        // Footers: input CRC, output CRC, patch CRC.
        push_le_u32(crc32(&src), &mut patch);
        push_le_u32(crc32(&dst), &mut patch);
        let patch_crc = crc32(&patch); // CRC over everything so far
        push_le_u32(patch_crc, &mut patch);

        let out = apply_ups(&src, &patch).expect("ups applies");
        assert_eq!(out, dst);
    }

    #[test]
    fn ups_source_crc_mismatch_rejected() {
        let src = vec![0x10u8, 0x20, 0x30, 0x40, 0x00];
        let dst = vec![0x11u8, 0x22, 0x30, 0x44, 0x00];
        let mut patch = Vec::new();
        patch.extend_from_slice(b"UPS1");
        write_len(src.len(), &mut patch);
        write_len(dst.len(), &mut patch);
        write_vuint(0, &mut patch);
        patch.push(src[0] ^ dst[0]);
        patch.push(src[1] ^ dst[1]);
        patch.push(src[2] ^ dst[2]);
        patch.push(src[3] ^ dst[3]);
        patch.push(0x00);
        push_le_u32(crc32(&src), &mut patch);
        push_le_u32(crc32(&dst), &mut patch);
        let patch_crc = crc32(&patch);
        push_le_u32(patch_crc, &mut patch);

        // Apply against a different ROM => source CRC mismatch.
        let wrong = vec![0xFFu8; 5];
        let err = apply_ups(&wrong, &patch).unwrap_err();
        assert!(matches!(err, PatchError::SourceCrcMismatch { .. }));
    }

    #[test]
    fn ups_bad_magic_rejected() {
        let err = apply_ups(&[0u8; 4], b"XXX1aaaaaaaaaaaa").unwrap_err();
        assert!(matches!(err, PatchError::BadMagic { format: "UPS" }));
    }

    // ---- BPS ---------------------------------------------------------------

    #[test]
    fn bps_source_target_and_copy_actions_round_trip() {
        // Source: 8 bytes. Target reuses a source span, injects literals, and
        // copies an earlier source span.
        let src = vec![0x00u8, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07];
        // Target layout we will reconstruct:
        //   SourceRead 4 -> [00 01 02 03]
        //   TargetRead 2 -> [AA BB]
        //   SourceCopy from offset 6, len 2 -> [06 07]
        let target = vec![0x00u8, 0x01, 0x02, 0x03, 0xAA, 0xBB, 0x06, 0x07];

        let mut patch = Vec::new();
        patch.extend_from_slice(b"BPS1");
        write_len(src.len(), &mut patch);
        write_len(target.len(), &mut patch);
        write_vuint(0, &mut patch); // metadata size

        // Action: SourceRead (cmd 0), length 4. encoding = ((len-1) << 2) | cmd.
        write_vuint((4u64 - 1) << 2, &mut patch);
        // Action: TargetRead, length 2, then 2 literal bytes.
        write_vuint(((2u64 - 1) << 2) | 1, &mut patch);
        patch.push(0xAA);
        patch.push(0xBB);
        // Action: SourceCopy, length 2. Then a signed offset moving the source
        // cursor from 0 to 6 => +6 => (6 << 1) | 0 = 12.
        write_vuint(((2u64 - 1) << 2) | 2, &mut patch);
        write_vuint(12, &mut patch);

        // Footers: source CRC, target CRC, patch CRC.
        push_le_u32(crc32(&src), &mut patch);
        push_le_u32(crc32(&target), &mut patch);
        let patch_crc = crc32(&patch);
        push_le_u32(patch_crc, &mut patch);

        let out = apply_bps(&src, &patch).expect("bps applies");
        assert_eq!(out, target);
    }

    #[test]
    fn bps_target_copy_action_round_trips() {
        // Exercise TargetCopy (cmd 3): build a run by copying earlier output.
        let src = vec![0x00u8, 0x01];
        // Target: SourceRead 2 -> [00 01], then TargetCopy from output[0] len 2
        // -> [00 01]. Final: [00 01 00 01].
        let target = vec![0x00u8, 0x01, 0x00, 0x01];

        let mut patch = Vec::new();
        patch.extend_from_slice(b"BPS1");
        write_len(src.len(), &mut patch);
        write_len(target.len(), &mut patch);
        write_vuint(0, &mut patch);

        write_vuint((2u64 - 1) << 2, &mut patch); // SourceRead 2 (cmd 0)
        write_vuint(((2u64 - 1) << 2) | 3, &mut patch); // TargetCopy 2
        write_vuint(0, &mut patch); // signed offset 0 => cursor stays at 0

        push_le_u32(crc32(&src), &mut patch);
        push_le_u32(crc32(&target), &mut patch);
        let patch_crc = crc32(&patch);
        push_le_u32(patch_crc, &mut patch);

        let out = apply_bps(&src, &patch).expect("bps applies");
        assert_eq!(out, target);
    }

    #[test]
    fn bps_target_crc_mismatch_rejected() {
        let src = vec![0x00u8, 0x01, 0x02, 0x03];
        let target = vec![0x00u8, 0x01, 0x02, 0x03];

        let mut patch = Vec::new();
        patch.extend_from_slice(b"BPS1");
        write_len(src.len(), &mut patch);
        write_len(target.len(), &mut patch);
        write_vuint(0, &mut patch);
        write_vuint((4u64 - 1) << 2, &mut patch); // SourceRead 4 (cmd 0)

        push_le_u32(crc32(&src), &mut patch);
        // Deliberately wrong target CRC.
        push_le_u32(crc32(&target).wrapping_add(1), &mut patch);
        let patch_crc = crc32(&patch);
        push_le_u32(patch_crc, &mut patch);

        let err = apply_bps(&src, &patch).unwrap_err();
        assert!(matches!(err, PatchError::OutputCrcMismatch { .. }));
    }

    #[test]
    fn bps_bad_magic_rejected() {
        let err = apply_bps(&[0u8; 4], b"XXX1aaaaaaaaaaaa").unwrap_err();
        assert!(matches!(err, PatchError::BadMagic { format: "BPS" }));
    }

    // ---- dispatch ----------------------------------------------------------

    #[test]
    fn detect_and_apply_dispatches_by_extension() {
        let rom = vec![0u8; 4];
        let mut ips = Vec::new();
        ips.extend_from_slice(b"PATCH");
        ips.extend_from_slice(b"EOF");
        assert!(detect_and_apply(&rom, &ips, "IPS").is_ok());
        assert!(detect_and_apply(&rom, &ips, "ips").is_ok());

        let err = detect_and_apply(&rom, &ips, "zip").unwrap_err();
        assert!(matches!(err, PatchError::UnknownExtension(ref e) if e == "zip"));
    }
}
