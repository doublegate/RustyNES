//! Famicom Disk System (FDS) — Stage 1 foundation (v2.2.0).
//!
//! This module implements the `.fds` container parser and the FDS RAM-adapter
//! device (modelled as a [`Mapper`]). Stage 1 covers:
//!
//! - **Container parsing** ([`parse_fds`]): the fwNES 16-byte header form
//!   (`"FDS\x1A"` + side count) and the headerless raw form (first side opens
//!   with `\x01*NINTENDO-HVC*`). Each side is a 65500-byte block.
//! - **Memory map**: 32 KiB PRG-RAM at `$6000-$DFFF` (read/write), an 8 KiB
//!   user-supplied BIOS at `$E000-$FFFF` (read-only), and 8 KiB CHR-RAM.
//! - **Registers** `$4020-$4026` (write) / `$4030-$4033` (read).
//! - **Timer IRQ**: a 16-bit down-counter clocked every CPU cycle.
//! - **Disk read**: a head-position state machine delivering disk bytes at a
//!   ~149-CPU-cycle cadence with the byte-transfer flag (and optional IRQ).
//!
//! Stage 2b (v2.2.0) extends Stage 1 with:
//!
//! - **Disk write path**: with `$4025` in write mode (bit 2 clear), each
//!   ~149-CPU-cycle byte-transfer tick stores the byte last written to `$4024`
//!   into the inserted side at the head position, advances the head, raises the
//!   byte-transfer flag/IRQ, and marks the disk image **dirty**.
//! - **Multi-side eject/insert**: the inserted side is now an
//!   `Option<usize>` (`None` = ejected). [`Fds::set_disk_side`] swaps sides; an
//!   eject sets `$4032` bit 0 (disk not inserted) and bit 1 (not ready); an
//!   insert resets the head and opens a short not-ready window.
//! - **Persistence hooks**: [`Fds::disk_image_bytes`] re-serializes the
//!   (possibly-modified) sides to the headerless `.fds` byte layout, and
//!   [`Fds::disk_is_dirty`] / [`Fds::clear_disk_dirty`] let a host save it back.
//! - **Per-disk write-protect**: [`Fds::set_write_protected`] toggles the
//!   `$4032` bit-2 write-protect flag (default writable).
//!
//! FDS-proper (v1.6.0 Workstream F) extends the drive model, after `puNES`
//! `fds.c`, while staying cycle-count-based (NOT the v2.0 master-clock axis):
//!
//! - **Timed disk-head position**: a motor restart after the cold spin-up
//!   rewinds the belt-driven disk to the disk-start gap and the head must
//!   re-seek to track 0, so a short [`HEAD_RESEEK_CYCLES`] not-ready window
//!   opens (rather than the head teleporting to track 0 instantly). This is
//!   what the BIOS re-read loop observes as the not-ready -> ready transition,
//!   and it closes the **Kid Icarus side-B post-registration** replay.
//! - **`$4032` drive status / auto-insert**: `$4032.1` (not-ready) is driven by
//!   the spin-up / re-seek windows above, the motor state, and end-of-head, so
//!   the disk re-presents itself to the loader on each re-read without manual
//!   re-insertion (the "auto-insert" behaviour).
//! - **Per-game CRC quirk table** ([`quirk_for_crc`] / [`FdsQuirk`]): a curated,
//!   growable list keyed off the disk-image CRC-32 for titles whose replay
//!   timing the nominal model does not satisfy (extra re-seek slack today). It
//!   ships **empty** — entries are added only from real, maintainer-measured
//!   dumps; the Kid Icarus side-B fix above is title-independent (the timed
//!   head model) and needs no entry.
//!
//! Still simplified (matching Stage 1): CRC/gap bytes are not synthesized on
//! write (the BIOS's CRC is recomputed in its own RAM, not on the medium), and
//! the seek windows are short deterministic fixed cycle counts, not an analog
//! seek-time model. BIOS-driven writing is unverified without a real BIOS.
//! The frontend wiring (BIOS prompt, side-swap keybind, `.fds.sav` file I/O)
//! is a later stage; this module only provides the core API + state.
//!
//! References (nesdev wiki, committed under `nesdev_wiki/`):
//! - `Family_Computer_Disk_System.xhtml` — register map + IRQ + banks.
//! - `FDS_disk_format.xhtml` / `FDS_file_format.xhtml` — the `.fds` container.
//! - `FDS_BIOS.xhtml` — the 8 KiB `disksys.rom` BIOS.

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_lossless,
    // The FDS sound channel's mod counter is a signed 7-bit value whose
    // hardware semantics are exactly two's-complement wrap; the audio register
    // packing genuinely round-trips signed<->unsigned bytes.
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::missing_const_for_fn,
    clippy::doc_markdown,
    // The FDS register set is genuinely a bag of independent hardware bits;
    // packing them into enums would obscure the 1:1 mapping to the documented
    // $4025/$4030 bit layout.
    clippy::struct_excessive_bools
)]

use alloc::{boxed::Box, format, string::String, vec, vec::Vec};

use crate::cartridge::{Mirroring, RomError};
use crate::mapper::{Mapper, MapperCaps, MapperDebugInfo, MapperError};

/// Bytes per disk side in the common `.fds` / fwNES file format (no CRCs, no
/// gaps). The on-disk physical capacity and the QD (65536) variant are larger;
/// see `FDS_disk_format.xhtml` §True disk capacity.
pub const FDS_SIDE_LEN: usize = 65500;

/// QD-file side length (65536). Detected so QD images degrade gracefully to the
/// 65500-byte read window rather than mis-parsing.
const QD_SIDE_LEN: usize = 65536;

/// fwNES optional header length.
const FWNES_HEADER_LEN: usize = 16;

/// PRG-RAM size: 32 KiB mapped at `$6000-$DFFF`.
const PRG_RAM_LEN: usize = 0x8000;

/// BIOS ROM size: 8 KiB mapped at `$E000-$FFFF`.
const BIOS_LEN: usize = 0x2000;

/// CHR-RAM size: 8 KiB at PPU `$0000-$1FFF`.
const CHR_RAM_LEN: usize = 0x2000;

/// Nominal CPU cycles between disk byte transfers.
///
/// Real hardware streams bits at roughly 96.4 kHz; with 8 bits + framing this
/// works out to ~149 CPU cycles (≈1.789773 MHz / 96.4 kHz / 8 ≈ 149) per
/// transferred byte. Emulators converge on this value (Mesen2, FCEUX). Stage 1
/// uses the fixed nominal cadence; the BIOS only requires bytes to arrive in
/// order at a rate it can service.
pub const DISK_BYTE_CYCLES: u32 = 149;

/// Deterministic "not ready" window (CPU cycles) opened after a side is
/// inserted. Real hardware spins the disk up + seeks the head, which is not
/// modelled cycle-exactly here; this short fixed window keeps `$4032` bit 1
/// set briefly after insert (closer to hardware than an instantly-ready drive)
/// without introducing any non-deterministic timing. Roughly one byte-transfer
/// period — enough that BIOS polling sees the not-ready transition.
pub const INSERT_NOT_READY_CYCLES: u32 = DISK_BYTE_CYCLES;

/// Drive spin-up time (CPU cycles) after the motor turns on: the disk spins up
/// and the head seeks to the disk start, during which `$4032.1` reports
/// not-ready. The BIOS reset disk-check waits for the not-ready -> ready
/// transition, so this must be long enough to be observed across its poll loop
/// (a few NMI frames). ~50000 cycles ≈ 28 ms, comfortably within one frame of
/// BIOS polling while still being far shorter than the real ~1 s spin-up so the
/// boot stays snappy and deterministic.
pub const MOTOR_SPIN_UP_CYCLES: u32 = 50_000;

/// Head re-seek time (CPU cycles) modelled on a motor restart.
///
/// When the motor restarts, the belt-driven drive physically rewinds the disk
/// back to the disk-start gap (the FDS-proper "timed disk-head position" —
/// `puNES` `fds.c` rewinds the `disk_position` and waits out a seek before the
/// first byte streams again, rather than the head teleporting to track 0
/// instantly).
///
/// This window opens on every motor off->on edge after the cold spin-up, while
/// `$4032.1` reports not-ready, so the BIOS observes the not-ready -> ready
/// transition before each re-read (the same handshake it waits for at boot).
/// This is what closes the **Kid Icarus side-B post-registration** replay: the
/// game stops the motor, writes the save, then re-reads later files — and the
/// BIOS re-read loop expects the drive to report not-ready while the head
/// returns. With an instant rewind the loader never sees the transition and the
/// post-registration screen never streams its blocks.
///
/// ~8000 cycles ≈ 4.5 ms — far shorter than the cold spin-up (the disk is
/// already turning), long enough to be observed across the BIOS poll loop, and
/// deterministic (no analog seek-time model). Per-game quirks ([`FdsQuirk`])
/// may extend it for titles whose timing the nominal value does not satisfy.
pub const HEAD_RESEEK_CYCLES: u32 = 8_000;

/// Per-game FDS timing quirk, modelled on `puNES` `fds.c`'s per-CRC drive table.
///
/// A small, additive set of knobs keyed off the disk-image CRC-32 (see
/// [`quirk_for_crc`]). Most titles run on the nominal timing and have no entry;
/// the table exists so a game whose BIOS replay loop needs a different
/// not-ready cadence (typically extra head-seek slack) can be tuned without
/// touching the general transfer engine. All fields are deterministic
/// cycle-count adjustments — never an analog/seek-time model — so the
/// determinism contract holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FdsQuirk {
    /// Extra CPU cycles added to the head re-seek not-ready window
    /// ([`HEAD_RESEEK_CYCLES`]) on each motor-restart rewind. Tunes titles whose
    /// BIOS replay loop needs the drive to report not-ready a little longer
    /// before a re-read.
    pub extra_reseek_cycles: u32,
}

impl FdsQuirk {
    /// The no-adjustment default applied to every title without a table entry.
    pub const NONE: Self = Self {
        extra_reseek_cycles: 0,
    };
}

/// Look up the per-game [`FdsQuirk`] for a disk image by its CRC-32.
///
/// Returns [`FdsQuirk::NONE`] for any disk not in the table — the overwhelming
/// majority. The table is the FDS analogue of the per-game mirroring / mapper
/// overrides: a curated, growable list of titles whose drive timing the nominal
/// model does not satisfy. CRC-32s are over the **headerless** side bytes (what
/// [`FdsDisk::to_bytes`] produces and [`Fds::new`] hashes), so a fwNES-headered
/// and a headerless dump of the same disk resolve to the same quirk.
#[must_use]
pub fn quirk_for_crc(crc: u32) -> FdsQuirk {
    // Per-game entries. Each is `(crc32, FdsQuirk { .. })`, the CRC-32 taken
    // over the headerless side bytes ([`FdsDisk::to_bytes`]).
    //
    // NOTE on the canonical Kid Icarus case: the general timed disk-head
    // position model (the [`HEAD_RESEEK_CYCLES`] re-seek window opened on every
    // motor-restart rewind) is what actually closes the Kid Icarus side-B
    // post-registration replay — that fix is title-independent and needs no
    // table entry. This table is the puNES-`fds.c`-style *framework* for the
    // residual minority of titles whose replay loop wants extra not-ready slack
    // beyond the nominal window.
    //
    // The table is intentionally EMPTY. Every concrete CRC-32 entry must be
    // measured from a real (never-committed) dump and verified against that
    // title's actual replay loop before it ships — a fabricated/placeholder CRC
    // key is unacceptable because a real disk that happens to hash to it would
    // silently receive unverified timing slack. Maintainers add measured
    // entries here as `(crc32, FdsQuirk { .. })`, the CRC-32 taken over the
    // headerless side bytes ([`FdsDisk::to_bytes`]). The lookup mechanism +
    // slack application + save-state independence are exercised by the unit
    // tests independently of any specific entry, so the framework cannot rot.
    const TABLE: &[(u32, FdsQuirk)] = &[];
    let mut i = 0;
    while i < TABLE.len() {
        if TABLE[i].0 == crc {
            return TABLE[i].1;
        }
        i += 1;
    }
    FdsQuirk::NONE
}

/// CRC-32/ISO-HDLC (the standard zlib/PNG "CRC-32") over `data`.
///
/// `no_std`-friendly (no lookup-table allocation — the polynomial is applied
/// bitwise); used only at construction time to key the per-game quirk table, so
/// the per-byte cost is irrelevant to the hot paths. Deterministic by
/// construction.
#[must_use]
pub fn fds_crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

/// Lead-in gap length (in `$00` bytes) synthesized before the first block of a
/// side. Hardware uses a long disk-start gap (≈26150-28300 bits ≈ 3300-3500
/// bytes); the BIOS only requires "enough" zero bytes before the first `$80`
/// start mark to settle its block-scan loop. A modest lead-in keeps the wire
/// image small while still giving the loader its expected pre-disk gap.
const WIRE_LEAD_IN_GAP: usize = 200;

/// Inter-block gap length (in `$00` bytes) synthesized between consecutive
/// blocks. Hardware uses ≥480 bits (≈60 bytes), 976 bits typical; the loader
/// accepts a much smaller minimum (a few hundred bits). This value sits
/// comfortably above the minimum the BIOS needs to re-detect a block start.
const WIRE_BLOCK_GAP: usize = 100;

/// The FDS block start mark. On the medium each gap is terminated by a single
/// `1` bit; in byte terms (little-endian) that is `$80`. The BIOS scans the bit
/// stream for this mark to find the start of every block.
const WIRE_START_MARK: u8 = 0x80;

/// Synthesized FDS block descriptor: where a block's payload lives in the raw
/// `.fds` side and where it (and its surrounding gap/mark/CRC) lands in the
/// synthesized wire image. The mapping lets the write path translate a wire
/// head position back to a raw side offset so persistence (`disk_image_bytes`)
/// keeps working.
#[derive(Debug, Clone, Copy)]
struct WireBlock {
    /// Offset of the block's first byte (its block-code byte) in the raw side.
    raw_start: usize,
    /// Block payload length in bytes (block-code byte included; CRC excluded —
    /// it is not stored in the `.fds` form).
    len: usize,
    /// Offset in the wire image of this block's first payload byte (i.e. just
    /// after its `$80` start mark).
    wire_payload_start: usize,
}

/// CRC-16/KERMIT (a.k.a. CRC-16/CCITT, reflected, poly 0x8408) over the start
/// mark + block bytes, matching the FDS RP2C33 block CRC. The BIOS does not
/// verify it for the standard load path, but synthesizing a correct value keeps
/// the wire image faithful and avoids tripping `$4030.D4` on stricter loaders.
fn fds_block_crc(start_mark: u8, block: &[u8]) -> u16 {
    let mut crc: u16 = 0;
    let update = |byte: u8, crc: &mut u16| {
        *crc ^= u16::from(byte);
        for _ in 0..8 {
            let carry = *crc & 1 != 0;
            *crc >>= 1;
            if carry {
                *crc ^= 0x8408;
            }
        }
    };
    update(start_mark, &mut crc);
    for &b in block {
        update(b, &mut crc);
    }
    crc
}

/// Walk a raw `.fds` side and return its block descriptors in stream order.
///
/// The `.fds` form stores only the block payloads concatenated (no gaps, no
/// start marks, no CRCs). Blocks are self-describing via their leading
/// block-code byte and a known/derivable length:
/// - `$01` disk-info: 56 bytes.
/// - `$02` file-amount: 2 bytes (byte 1 = file count).
/// - `$03` file-header: 16 bytes (file size = LE u16 at offset 13).
/// - `$04` file-data: `1 + size` bytes (size from the preceding header).
///
/// Parsing stops at the first byte that is not a valid next block code (the
/// trailing `$00` padding terminates the walk).
fn parse_side_blocks(side: &[u8]) -> Vec<(usize, usize)> {
    let mut blocks = Vec::new();
    let mut pos = 0usize;
    let mut pending_file_size: Option<usize> = None;
    loop {
        if pos >= side.len() {
            break;
        }
        let code = side[pos];
        let len = match code {
            0x01 => 56,
            0x02 => 2,
            0x03 => {
                // File size lives at header offset 13-14 (LE u16); remember it
                // for the file-data block that must follow.
                let size = if pos + 15 <= side.len() {
                    usize::from(side[pos + 13]) | (usize::from(side[pos + 14]) << 8)
                } else {
                    0
                };
                pending_file_size = Some(size);
                16
            }
            0x04 => {
                let size = pending_file_size.take().unwrap_or(0);
                1 + size
            }
            // Any other leading byte (most commonly $00 trailing padding) ends
            // the structured region of the side.
            _ => break,
        };
        if pos + len > side.len() {
            // A declared block runs past the end of the side: clamp and stop.
            blocks.push((pos, side.len() - pos));
            break;
        }
        blocks.push((pos, len));
        pos += len;
    }
    blocks
}

/// Synthesize the on-disk **wire image** for one raw `.fds` side: the gap /
/// start-mark / block / CRC structure the BIOS read routine scans for.
///
/// The layout is, for each parsed block:
/// `[gap $00 × G] [$80 start mark] [block bytes] [crc_lo] [crc_hi]`
/// with a long lead-in gap before block 1 and shorter inter-block gaps, then a
/// run of trailing `$00` so the wire image always ends in a gap (so the head
/// sitting at the inner track reads `$00`).
///
/// Returns the wire bytes plus the [`WireBlock`] descriptors mapping each
/// block's wire payload region back to its raw side offset (used by the write
/// path to persist BIOS-written blocks into the raw `.fds` side).
fn build_side_wire(side: &[u8]) -> (Vec<u8>, Vec<WireBlock>) {
    let raw_blocks = parse_side_blocks(side);
    let mut wire = Vec::with_capacity(side.len() + raw_blocks.len() * 32 + WIRE_LEAD_IN_GAP);
    let mut map = Vec::with_capacity(raw_blocks.len());
    for (i, &(raw_start, len)) in raw_blocks.iter().enumerate() {
        let gap = if i == 0 {
            WIRE_LEAD_IN_GAP
        } else {
            WIRE_BLOCK_GAP
        };
        wire.resize(wire.len() + gap, 0x00);
        wire.push(WIRE_START_MARK);
        let wire_payload_start = wire.len();
        let block = &side[raw_start..raw_start + len];
        wire.extend_from_slice(block);
        let crc = fds_block_crc(WIRE_START_MARK, block);
        wire.push((crc & 0xFF) as u8);
        wire.push((crc >> 8) as u8);
        map.push(WireBlock {
            raw_start,
            len,
            wire_payload_start,
        });
    }
    // Trailing gap so the head reads `$00` past the last block instead of
    // running straight off the end (a small, fixed tail is enough).
    wire.resize(wire.len() + WIRE_BLOCK_GAP, 0x00);
    (wire, map)
}

/// A parsed FDS disk image: an ordered list of disk sides.
#[derive(Debug, Clone)]
pub struct FdsDisk {
    /// One entry per disk side; each is exactly [`FDS_SIDE_LEN`] bytes (longer
    /// raw sides are truncated, shorter ones zero-padded, so the read window is
    /// always well-defined). Heap-allocated to keep the 64 KiB-per-side payload
    /// off the stack.
    sides: Vec<Box<[u8]>>,
    /// Side count declared by the fwNES header, when present (else derived from
    /// the file length).
    declared_side_count: u8,
}

impl FdsDisk {
    /// Number of disk sides in this image.
    #[must_use]
    pub fn side_count(&self) -> usize {
        self.sides.len()
    }

    /// Side count declared by the container header (0 when headerless).
    #[must_use]
    pub fn declared_side_count(&self) -> u8 {
        self.declared_side_count
    }

    /// Borrow the raw bytes of side `idx` (panics if out of range — callers use
    /// [`Self::side_count`] to bound).
    #[must_use]
    pub fn side(&self, idx: usize) -> &[u8] {
        &self.sides[idx]
    }

    /// Mutably borrow the raw bytes of side `idx` (panics if out of range).
    /// Used by the disk-write path to store bytes at the head position.
    pub fn side_mut(&mut self, idx: usize) -> &mut [u8] {
        &mut self.sides[idx]
    }

    /// Re-serialize every side back to the headerless `.fds` byte layout
    /// (`side_count` × [`FDS_SIDE_LEN`] bytes, concatenated). This is the form a
    /// host writes to a side-car `.fds.sav` so the modified disk persists. The
    /// fwNES 16-byte header is intentionally omitted — the headerless form
    /// round-trips through [`parse_fds`] (its first side opens with the
    /// `\x01*NINTENDO-HVC*` disk-info signature).
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.sides.len() * FDS_SIDE_LEN);
        for side in &self.sides {
            out.extend_from_slice(side);
        }
        out
    }
}

/// Parse a `.fds` / fwNES disk image into an [`FdsDisk`].
///
/// Accepts:
/// - The fwNES container: a 16-byte header (`"FDS\x1A"` + byte4 = side count),
///   followed by `side_count` × [`FDS_SIDE_LEN`]-byte sides.
/// - The headerless raw form: 1+ concatenated sides, the first of which opens
///   with `\x01*NINTENDO-HVC*` (the disk-info block).
///
/// QD-style 65536-byte sides are accepted and truncated to the 65500-byte read
/// window (Stage 1 does not consume CRC/gap bytes).
///
/// # Errors
///
/// - [`RomError::BadMagic`] if the bytes are neither a fwNES container nor a
///   recognizable raw disk side.
/// - [`RomError::Truncated`] if the declared side count would run past the end
///   of the file, or the file is too short to hold a single side.
pub fn parse_fds(bytes: &[u8]) -> Result<FdsDisk, RomError> {
    // fwNES container form.
    if bytes.len() >= 4 && &bytes[0..4] == b"FDS\x1A" {
        let declared = bytes.get(4).copied().unwrap_or(0);
        let body = &bytes[FWNES_HEADER_LEN.min(bytes.len())..];
        return parse_sides(body, declared, true);
    }

    // Headerless raw form: must open with the disk-info block signature.
    if bytes.len() >= 15 && bytes[0] == 0x01 && &bytes[1..15] == b"*NINTENDO-HVC*" {
        return parse_sides(bytes, 0, false);
    }

    Err(RomError::BadMagic)
}

/// Split the side region into fixed-size sides.
///
/// `declared` is the fwNES header's side count (0 when headerless / unknown).
/// When `has_header` is set and `declared` is non-zero, it is authoritative and
/// the body must be long enough to hold that many sides. Otherwise the side
/// count is derived from the body length, accepting both the 65500 and QD 65536
/// stride (whichever divides evenly; 65500 takes precedence).
fn parse_sides(body: &[u8], declared: u8, has_header: bool) -> Result<FdsDisk, RomError> {
    if body.len() < FDS_SIDE_LEN {
        return Err(RomError::Truncated {
            needed: FDS_SIDE_LEN,
            got: body.len(),
        });
    }

    // Pick the side stride. Prefer 65500 (FDS format); fall back to 65536 (QD)
    // only when the body length is a clean multiple of it but not of 65500.
    let stride = if body.len().is_multiple_of(FDS_SIDE_LEN) {
        FDS_SIDE_LEN
    } else if body.len().is_multiple_of(QD_SIDE_LEN) {
        QD_SIDE_LEN
    } else {
        // Irregular trailing bytes (some dumps pad). Use the FDS stride and
        // accept a partial final side via the truncation below.
        FDS_SIDE_LEN
    };

    let available_sides = body.len() / stride;
    let side_count = if has_header && declared != 0 {
        let n = declared as usize;
        if body.len() < n * stride {
            return Err(RomError::Truncated {
                needed: n * stride,
                got: body.len(),
            });
        }
        n
    } else {
        available_sides.max(1)
    };

    let mut sides = Vec::with_capacity(side_count);
    for i in 0..side_count {
        let start = i * stride;
        let mut side = vec![0u8; FDS_SIDE_LEN].into_boxed_slice();
        let end = (start + FDS_SIDE_LEN).min(body.len());
        let copy_len = end.saturating_sub(start);
        side[..copy_len].copy_from_slice(&body[start..start + copy_len]);
        sides.push(side);
    }

    Ok(FdsDisk {
        sides,
        declared_side_count: declared,
    })
}

/// FDS sound channel (2C33 audio), modelled per `nesdev_wiki/FDS_audio.xhtml`.
///
/// The chip is a wavetable oscillator with a separate frequency-modulation
/// unit. Both the wave output unit and the modulation unit are clocked every
/// 16 CPU cycles; the volume and modulation envelopes tick on a programmable
/// divider derived from `$408A` and the per-envelope speed.
///
/// The audio state is **always present** (so the register decoders and the
/// save-state tail are identical between `mapper-audio` on/off builds); the
/// synthesis (`clock` / `output`) is only driven when the feature is on.
#[derive(Clone)]
pub(crate) struct FdsAudio {
    // --- Wavetable ($4040-$407F) ---
    /// 64-entry, 6-bit wavetable RAM (0..=63 per step).
    wavetable: [u8; 64],
    /// Wave write-enable / hold ($4089 bit 7). While set the channel output is
    /// held and the wavetable RAM is CPU-writable.
    wave_write_enable: bool,
    /// 24-bit wave phase accumulator. Bits 18..=23 (`>> 18 & 0x3F`) index the
    /// wavetable; the low 18 bits are the fraction.
    wave_acc: u32,
    /// Last wavetable value latched at the output (held while write-enabled).
    /// Mirrors the `$4096`-readable "current wavetable position" sample.
    wave_out_latch: u8,

    // --- Frequency ($4082/$4083) ---
    /// 12-bit wave (carrier) pitch.
    wave_pitch: u16,
    /// $4083 bit 7 — disable channel: halts the wave unit + resets its
    /// accumulator (output holds the `$4040` value).
    wave_halt: bool,
    /// $4083 bit 6 — disable volume + mod envelopes (envelopes do not tick).
    env_halt: bool,

    // --- Volume envelope ($4080, read $4090) ---
    /// Volume envelope mode: false = envelope on, true = direct gain (disabled).
    vol_env_disabled: bool,
    /// Volume envelope direction: true = increase, false = decrease.
    vol_env_increase: bool,
    /// Volume envelope speed (0..=63).
    vol_env_speed: u8,
    /// Current volume gain (0..=63; clamped to 32 at the output multiply).
    vol_gain: u8,
    /// Volume-envelope clock timer (counts down CPU cycles).
    vol_timer: u32,

    // --- Mod envelope ($4084, read $4092) ---
    /// Mod envelope mode: false = envelope on, true = direct gain (disabled).
    mod_env_disabled: bool,
    /// Mod envelope direction: true = increase, false = decrease.
    mod_env_increase: bool,
    /// Mod envelope speed (0..=63).
    mod_env_speed: u8,
    /// Current mod gain (0..=63).
    mod_gain: u8,
    /// Mod-envelope clock timer (counts down CPU cycles).
    mod_timer: u32,

    // --- Modulation unit ($4085/$4086/$4087/$4088) ---
    /// Signed 7-bit mod counter (-64..=63).
    mod_counter: i8,
    /// 12-bit mod pitch ($4086/$4087).
    mod_pitch: u16,
    /// $4087 bit 7 — mod unit disabled (accumulator held + reset on set,
    /// table writable via $4088).
    mod_halt: bool,
    /// 18-bit mod phase accumulator. Bits 13..=17 are the 5-bit table address;
    /// bit 12 is the "ghost" bit (each entry steps twice); bits 0..=11 fraction.
    mod_acc: u32,
    /// 32-entry, 3-bit modulation table (ring buffer).
    mod_table: [u8; 32],
    /// Mod-table write position (advances on each `$4088` write; the low bit is
    /// ignored when indexing, so the position is a 64-step value here stored as
    /// the 5-bit entry index used for `$4088` writes).
    mod_write_pos: u8,

    // --- Master volume / envelope speed ($4089/$408A) ---
    /// Master volume select (0..=3 -> divisors 2,3,4,5 over the gain).
    master_volume: u8,
    /// Master envelope speed multiplier ($408A). 0 disables both envelopes.
    env_speed_mult: u8,

    /// 16-CPU-cycle prescaler driving the wave + modulation units. Counts up
    /// from 0; the units tick when it reaches 16 (then resets).
    cycle_prescaler: u8,
}

impl Default for FdsAudio {
    fn default() -> Self {
        Self {
            wavetable: [0; 64],
            wave_write_enable: false,
            wave_acc: 0,
            wave_out_latch: 0,
            wave_pitch: 0,
            wave_halt: false,
            env_halt: false,
            vol_env_disabled: false,
            vol_env_increase: false,
            vol_env_speed: 0,
            vol_gain: 0,
            vol_timer: 0,
            mod_env_disabled: false,
            mod_env_increase: false,
            mod_env_speed: 0,
            mod_gain: 0,
            mod_timer: 0,
            mod_counter: 0,
            mod_pitch: 0,
            mod_halt: false,
            mod_acc: 0,
            mod_table: [0; 32],
            mod_write_pos: 0,
            master_volume: 0,
            env_speed_mult: 0xE8, // BIOS power-on value.
            cycle_prescaler: 0,
        }
    }
}

impl FdsAudio {
    /// Write a sound register in the `$4040-$408A` window.
    ///
    /// Returns nothing; side effects update the channel state. Wavetable RAM
    /// (`$4040-$407F`) is only mutable while wave-write-enable (`$4089` bit 7)
    /// is set.
    pub(crate) fn write_reg(&mut self, addr: u16, value: u8) {
        match addr {
            0x4040..=0x407F => {
                if self.wave_write_enable {
                    self.wavetable[(addr - 0x4040) as usize] = value & 0x3F;
                }
            }
            0x4080 => {
                // MDVV VVVV: M=mode(1=disabled/direct), D=direction, V=speed.
                self.vol_env_disabled = value & 0x80 != 0;
                self.vol_env_increase = value & 0x40 != 0;
                self.vol_env_speed = value & 0x3F;
                if self.vol_env_disabled {
                    // Direct gain: the speed bits double as the gain value.
                    self.vol_gain = value & 0x3F;
                }
                // Writing resets the envelope clock timer (next tick c cycles on).
                self.vol_timer = self.vol_env_period();
            }
            0x4082 => {
                self.wave_pitch = (self.wave_pitch & 0x0F00) | u16::from(value);
            }
            0x4083 => {
                // MExx FFFF: M=halt/4x, E=disable envelopes, F=freq hi.
                self.wave_pitch = (self.wave_pitch & 0x00FF) | (u16::from(value & 0x0F) << 8);
                let new_halt = value & 0x80 != 0;
                self.env_halt = value & 0x40 != 0;
                if new_halt && !self.wave_halt {
                    // Entering halt resets the wave accumulator (wave position
                    // returns to 0 -> outputs the $4040 value).
                    self.wave_acc = 0;
                }
                self.wave_halt = new_halt;
                if self.env_halt {
                    // Bit 6 also resets the envelope timers.
                    self.vol_timer = self.vol_env_period();
                    self.mod_timer = self.mod_env_period();
                }
            }
            0x4084 => {
                self.mod_env_disabled = value & 0x80 != 0;
                self.mod_env_increase = value & 0x40 != 0;
                self.mod_env_speed = value & 0x3F;
                if self.mod_env_disabled {
                    self.mod_gain = value & 0x3F;
                }
                self.mod_timer = self.mod_env_period();
            }
            0x4085 => {
                // Directly set the 7-bit signed mod counter (sign-extend bit 6).
                let raw = value & 0x7F;
                self.mod_counter = if raw & 0x40 != 0 {
                    (raw | 0x80) as i8
                } else {
                    raw as i8
                };
            }
            0x4086 => {
                self.mod_pitch = (self.mod_pitch & 0x0F00) | u16::from(value);
            }
            0x4087 => {
                // HFxx FFFF: H=reset/disable mod, F=force carry (unused here).
                self.mod_pitch = (self.mod_pitch & 0x00FF) | (u16::from(value & 0x0F) << 8);
                let new_halt = value & 0x80 != 0;
                if new_halt {
                    // Reset the mod accumulator's low 13 bits (fraction + ghost),
                    // keeping the 5-bit table address (bits 13-17) intact.
                    self.mod_acc &= 0x0003_E000;
                }
                self.mod_halt = new_halt;
            }
            0x4088 => {
                // Mod table write — only effective while the mod unit is halted.
                if self.mod_halt {
                    let entry = value & 0x07;
                    let pos = (self.mod_write_pos & 0x1F) as usize;
                    self.mod_table[pos] = entry;
                    self.mod_write_pos = (self.mod_write_pos + 1) & 0x1F;
                }
            }
            0x4089 => {
                // Wxxx xxVV: W=wave write enable/hold, V=master volume.
                self.wave_write_enable = value & 0x80 != 0;
                self.master_volume = value & 0x03;
            }
            0x408A => {
                self.env_speed_mult = value;
            }
            _ => {}
        }
    }

    /// Read a sound register in the `$4090-$4097` window. Returns `None` for
    /// addresses the audio unit does not drive (so the caller falls through to
    /// the device / open-bus behaviour).
    fn read_reg(&self, addr: u16) -> Option<u8> {
        match addr {
            // Current volume gain (bits 5-0); bits 7-6 read as 01 (open bus).
            0x4090 => Some(0x40 | (self.vol_gain & 0x3F)),
            // Current mod gain (bits 5-0); bits 7-6 read as 01 (open bus).
            0x4092 => Some(0x40 | (self.mod_gain & 0x3F)),
            // Current wavetable value (held output sample); bits 7-6 read as 01.
            0x4096 => Some(0x40 | (self.wave_out_latch & 0x3F)),
            // Current mod counter (7-bit) in bits 6-0; bit 7 reads 0.
            0x4097 => Some((self.mod_counter as u8) & 0x7F),
            _ => None,
        }
    }

    /// CPU clocks between volume-envelope ticks: `c = 8 * (e + 1) * (m + 1)`
    /// where `e` is the volume envelope speed and `m` the master multiplier
    /// (`$408A`). Halved (4x faster) when `$4083` bit 7 is set — wait, that is
    /// the wave-halt path; per the wiki the 4x speed-up is governed by the same
    /// bit. We keep envelopes at the base rate here (see [`Self::clock_env`]).
    fn vol_env_period(&self) -> u32 {
        8 * (u32::from(self.vol_env_speed) + 1) * (u32::from(self.env_speed_mult) + 1)
    }

    /// CPU clocks between mod-envelope ticks (same shape as the volume one).
    fn mod_env_period(&self) -> u32 {
        8 * (u32::from(self.mod_env_speed) + 1) * (u32::from(self.env_speed_mult) + 1)
    }

    /// The modulated 20-bit wave pitch, per the nesdev `FDS_audio` "Modulation
    /// unit" pseudo-code:
    ///
    /// ```text
    /// temp = counter * gain;
    /// if ((temp & 0x0f) && !(temp & 0x800)) temp += 0x20;  // round up if +ve
    /// temp += 0x400;
    /// temp = (temp >> 4) & 0xff;       // drop 4 bits, center at 0x40
    /// wave_pitch = (pitch * temp) & 0xFFFFF;
    /// ```
    #[cfg(feature = "mapper-audio")]
    fn modulated_pitch(&self) -> u32 {
        let counter = i32::from(self.mod_counter);
        let gain = i32::from(self.mod_gain);
        let mut temp = counter * gain;
        if (temp & 0x0F) != 0 && (temp & 0x800) == 0 {
            temp += 0x20;
        }
        temp += 0x400;
        temp = (temp >> 4) & 0xFF;
        ((u32::from(self.wave_pitch)).wrapping_mul(temp as u32)) & 0x000F_FFFF
    }

    /// Apply one mod-table step to the signed mod counter, per the 3-bit entry
    /// table `0,1,2,4,reset,-4,-2,-1` (see the nesdev "Modulation unit" list).
    #[cfg(feature = "mapper-audio")]
    fn step_mod_counter(&mut self, entry: u8) {
        match entry & 0x07 {
            0 => {}
            1 => self.mod_counter = self.mod_counter.wrapping_add(1),
            2 => self.mod_counter = self.mod_counter.wrapping_add(2),
            3 => self.mod_counter = self.mod_counter.wrapping_add(4),
            4 => self.mod_counter = 0, // reset
            5 => self.mod_counter = self.mod_counter.wrapping_sub(4),
            6 => self.mod_counter = self.mod_counter.wrapping_sub(2),
            7 => self.mod_counter = self.mod_counter.wrapping_sub(1),
            _ => unreachable!(),
        }
        // The mod counter is a signed 7-bit value: wrap into -64..=63.
        let mut v = i16::from(self.mod_counter);
        if v > 63 {
            v -= 128;
        } else if v < -64 {
            v += 128;
        }
        self.mod_counter = v as i8;
    }

    /// Advance the modulation unit one tick (called every 16 CPU cycles). When
    /// the low 12 bits of the mod accumulator carry out (and the mod pitch is
    /// non-zero), advance the table address and apply the next entry.
    #[cfg(feature = "mapper-audio")]
    fn clock_mod(&mut self) {
        if self.mod_halt || self.mod_pitch == 0 {
            return;
        }
        let before = self.mod_acc;
        self.mod_acc = (self.mod_acc + u32::from(self.mod_pitch)) & 0x0003_FFFF;
        // Carry out of bit 11 (the low 12 bits wrapped past 0xFFF).
        if (before & 0x0FFF) + u32::from(self.mod_pitch) > 0x0FFF {
            // The 5-bit table address is bits 13-17; bit 12 is the "ghost" bit
            // that makes each entry step twice. Index the 32-entry table.
            let table_index = ((self.mod_acc >> 13) & 0x1F) as usize;
            let entry = self.mod_table[table_index];
            self.step_mod_counter(entry);
        }
    }

    /// Advance the wave output unit one tick (called every 16 CPU cycles).
    /// Adds the modulated pitch to the 24-bit wave accumulator and latches the
    /// new wavetable sample (unless the channel is halted or held).
    #[cfg(feature = "mapper-audio")]
    fn clock_wave(&mut self) {
        if self.wave_halt || self.wave_write_enable {
            // Halted: hold the $4040 value. Write-enabled: hold current output.
            if self.wave_halt {
                self.wave_out_latch = self.wavetable[0] & 0x3F;
            }
            return;
        }
        let pitch = self.modulated_pitch();
        self.wave_acc = self.wave_acc.wrapping_add(pitch) & 0x00FF_FFFF;
        let index = ((self.wave_acc >> 18) & 0x3F) as usize;
        self.wave_out_latch = self.wavetable[index] & 0x3F;
    }

    /// Advance a single envelope (volume or mod) by one CPU cycle. Returns the
    /// updated `(timer, gain)`. The envelope ticks when its timer reaches 0,
    /// then reloads. Disabled envelopes (mode bit set) or a zero master speed
    /// hold the gain.
    #[cfg(feature = "mapper-audio")]
    fn clock_one_env(timer: &mut u32, gain: &mut u8, disabled: bool, increase: bool, period: u32) {
        if disabled || period == 0 {
            return;
        }
        if *timer == 0 {
            *timer = period;
        }
        *timer -= 1;
        if *timer == 0 {
            *timer = period;
            if increase {
                if *gain < 32 {
                    *gain += 1;
                }
            } else if *gain > 0 {
                *gain -= 1;
            }
        }
    }

    /// Tick the volume + mod envelopes for one CPU cycle.
    #[cfg(feature = "mapper-audio")]
    fn clock_envelopes(&mut self) {
        // Envelopes are halted while the waveform is halted or via $4083 bit 6,
        // and disabled when the master envelope speed ($408A) is 0.
        if self.env_halt || self.wave_halt || self.env_speed_mult == 0 {
            return;
        }
        let vol_period = self.vol_env_period();
        Self::clock_one_env(
            &mut self.vol_timer,
            &mut self.vol_gain,
            self.vol_env_disabled,
            self.vol_env_increase,
            vol_period,
        );
        let mod_period = self.mod_env_period();
        Self::clock_one_env(
            &mut self.mod_timer,
            &mut self.mod_gain,
            self.mod_env_disabled,
            self.mod_env_increase,
            mod_period,
        );
    }

    /// Advance the whole sound channel by one CPU cycle: tick the envelopes
    /// every cycle and the wave + modulation units every 16 CPU cycles.
    #[cfg(feature = "mapper-audio")]
    pub(crate) fn clock(&mut self) {
        self.clock_envelopes();
        self.cycle_prescaler += 1;
        if self.cycle_prescaler >= 16 {
            self.cycle_prescaler = 0;
            self.clock_mod();
            self.clock_wave();
        }
    }

    /// Current channel output sample as `i16`, scaled for the bus's external
    /// mix (`f32::from(sample) / 65536.0`).
    ///
    /// Output = `wave_sample (0..=63) * volume_gain (clamped to 32) * master`.
    /// Per nesdev `FDS_audio` "Mixing", the FDS peak is roughly 2.4x the APU
    /// square. We scale so the peak (`63 * 32` at master=full) lands near
    /// VRC6's loudness ballpark (`~±15k`) — comfortably under `i16::MAX`.
    #[cfg(feature = "mapper-audio")]
    pub(crate) fn output(&self) -> i16 {
        // Volume gain clamps to 32 at the output multiply (PWM duty 32/32).
        let gain = u32::from(self.vol_gain.min(32));
        let sample = u32::from(self.wave_out_latch & 0x3F);
        // Master volume: 0=full(2/2), 1=2/3, 2=2/4, 3=2/5. Numerator 2,
        // denominator (master_volume + 2).
        let num = 2u32;
        let den = u32::from(self.master_volume) + 2;
        // Raw product range: sample(0..=63) * gain(0..=32) = 0..=2016.
        let raw = sample * gain;
        // Apply master volume.
        let scaled = raw * num / den;
        // scaled max (master=full) = 2016 * 2 / 2 = 2016. Center bipolar around
        // the mid-DAC (wave centered at 31.5 * gain * master). We output the raw
        // level minus its midpoint, scaled to use a good chunk of i16 range.
        // Midpoint of sample range is 31.5; approximate the DC bias as
        // 32 * gain * num / den (sample == 32).
        let mid = 32u32 * gain * num / den;
        let centered = scaled as i32 - mid as i32;
        // Scale factor 7: peak |centered| ~= 31 * 32 ~= 992 -> *7 ~= 6944 per
        // full-gain step; the full-scale (gain 32, master full) reaches
        // ~±6.9k, in the APU-square-x2.4 ballpark and well under i16::MAX.
        (centered * 7) as i16
    }

    /// Feature-off shim: with `mapper-audio` disabled the synthesizer does
    /// not advance, so a clock is a no-op (mirrors the gated path so the
    /// shared NSF expansion router can call `clock()` unconditionally).
    #[cfg(not(feature = "mapper-audio"))]
    pub(crate) fn clock(&mut self) {}

    /// Feature-off shim: silence when `mapper-audio` is disabled.
    #[cfg(not(feature = "mapper-audio"))]
    pub(crate) fn output(&self) -> i16 {
        0
    }

    /// Save-state tail (kept lock-step with [`Self::read_tail`]).
    fn write_tail(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.wavetable);
        out.extend_from_slice(&self.mod_table);
        out.extend_from_slice(&self.wave_acc.to_le_bytes());
        out.extend_from_slice(&self.mod_acc.to_le_bytes());
        out.extend_from_slice(&self.wave_pitch.to_le_bytes());
        out.extend_from_slice(&self.mod_pitch.to_le_bytes());
        out.extend_from_slice(&self.vol_timer.to_le_bytes());
        out.extend_from_slice(&self.mod_timer.to_le_bytes());
        out.push(self.vol_gain);
        out.push(self.mod_gain);
        out.push(self.vol_env_speed);
        out.push(self.mod_env_speed);
        out.push(self.mod_counter as u8);
        out.push(self.mod_write_pos);
        out.push(self.master_volume);
        out.push(self.env_speed_mult);
        out.push(self.wave_out_latch);
        out.push(self.cycle_prescaler);
        // Packed booleans.
        let mut flags = 0u16;
        flags |= u16::from(self.wave_write_enable);
        flags |= u16::from(self.wave_halt) << 1;
        flags |= u16::from(self.env_halt) << 2;
        flags |= u16::from(self.vol_env_disabled) << 3;
        flags |= u16::from(self.vol_env_increase) << 4;
        flags |= u16::from(self.mod_env_disabled) << 5;
        flags |= u16::from(self.mod_env_increase) << 6;
        flags |= u16::from(self.mod_halt) << 7;
        out.extend_from_slice(&flags.to_le_bytes());
    }

    /// Tail size in bytes — see [`Self::write_tail`].
    /// 64 (wavetable) + 32 (mod table) + 4 + 4 (accumulators) + 2 + 2 (pitches)
    /// + 4 + 4 (timers) + 10 (single bytes incl. prescaler) + 2 (flags) = 134.
    const TAIL_LEN: usize = 64 + 32 + 4 + 4 + 2 + 2 + 4 + 4 + 10 + 2;

    fn read_tail(&mut self, src: &[u8]) -> Result<(), MapperError> {
        if src.len() < Self::TAIL_LEN {
            return Err(MapperError::Truncated {
                expected: Self::TAIL_LEN,
                got: src.len(),
            });
        }
        let mut off = 0;
        self.wavetable.copy_from_slice(&src[off..off + 64]);
        off += 64;
        self.mod_table.copy_from_slice(&src[off..off + 32]);
        off += 32;
        self.wave_acc = u32::from_le_bytes(src[off..off + 4].try_into().unwrap());
        off += 4;
        self.mod_acc = u32::from_le_bytes(src[off..off + 4].try_into().unwrap());
        off += 4;
        self.wave_pitch = u16::from_le_bytes(src[off..off + 2].try_into().unwrap());
        off += 2;
        self.mod_pitch = u16::from_le_bytes(src[off..off + 2].try_into().unwrap());
        off += 2;
        self.vol_timer = u32::from_le_bytes(src[off..off + 4].try_into().unwrap());
        off += 4;
        self.mod_timer = u32::from_le_bytes(src[off..off + 4].try_into().unwrap());
        off += 4;
        self.vol_gain = src[off];
        off += 1;
        self.mod_gain = src[off];
        off += 1;
        self.vol_env_speed = src[off];
        off += 1;
        self.mod_env_speed = src[off];
        off += 1;
        self.mod_counter = src[off] as i8;
        off += 1;
        self.mod_write_pos = src[off] & 0x1F;
        off += 1;
        self.master_volume = src[off] & 0x03;
        off += 1;
        self.env_speed_mult = src[off];
        off += 1;
        self.wave_out_latch = src[off] & 0x3F;
        off += 1;
        self.cycle_prescaler = src[off];
        off += 1;
        let flags = u16::from_le_bytes(src[off..off + 2].try_into().unwrap());
        self.wave_write_enable = flags & (1 << 0) != 0;
        self.wave_halt = flags & (1 << 1) != 0;
        self.env_halt = flags & (1 << 2) != 0;
        self.vol_env_disabled = flags & (1 << 3) != 0;
        self.vol_env_increase = flags & (1 << 4) != 0;
        self.mod_env_disabled = flags & (1 << 5) != 0;
        self.mod_env_increase = flags & (1 << 6) != 0;
        self.mod_halt = flags & (1 << 7) != 0;
        Ok(())
    }
}

/// Internal disk-transfer state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TransferState {
    /// No transfer in progress (motor off, transfer reset, or ejected).
    Idle,
    /// Streaming bytes from the current side.
    Reading,
    /// Storing bytes to the current side (`$4025` write mode).
    Writing,
}

/// One record in the optional FDS read-stream trace.
///
/// A diagnostic for the disk-read / side-swap path — e.g. the Kid Icarus side-B
/// ERR.07 stall. Recorded only after [`Fds::enable_trace`]; default builds never
/// allocate or record, so the determinism contract is untouched. `kind`: 0 =
/// `$4031` read (the disk byte the BIOS consumed), 1 = `$4025` control write, 2 =
/// side change. `value` is the byte (or the new side index / `0xFF` for eject).
/// `head` is the wire head; `side` is the inserted side (or `-1` ejected);
/// `status` is the live `$4030` bits.
#[derive(Clone, Copy, Debug)]
pub struct FdsTraceRec {
    /// Event kind: 0 = `$4031` read, 1 = `$4025` control write, 2 = side change.
    pub kind: u8,
    /// The byte read/written, or (for a side change) the new side index / `0xFF`.
    pub value: u8,
    /// Wire head position at the time of the event.
    pub head: u32,
    /// Inserted side index, or `-1` when ejected.
    pub side: i8,
    /// Live `$4030` status bits at the time of the event.
    pub status: u8,
}

/// FDS RAM-adapter device, modelled as a [`Mapper`].
///
/// Owns the PRG-RAM, CHR-RAM, BIOS, the inserted disk image, all register
/// state, the timer-IRQ counter, and the disk-read head. Routed by the bus for
/// every CPU access in `$4020-$FFFF`; `$4020-$409F` registers are surfaced via
/// [`Mapper::cpu_read_unmapped`] returning `false` for that window.
pub struct Fds {
    // --- Memory ---
    prg_ram: Box<[u8]>, // 32 KiB at $6000-$DFFF
    chr_ram: Box<[u8]>, // 8 KiB CHR-RAM
    bios: Box<[u8]>,    // 8 KiB at $E000-$FFFF

    // --- Disk ---
    disk: FdsDisk,
    /// Index of the currently inserted side (0-based), or `None` when ejected.
    /// Drives `$4032` bit 0 (disk-not-inserted) when `None`. Default = side 0.
    inserted_side: Option<usize>,
    /// Read/write head position. This is an offset into the **wire image** of
    /// the currently inserted side ([`Fds::wire`]) — the synthesized gap /
    /// start-mark / block / CRC stream the BIOS scans — not a raw `.fds` side
    /// offset. The write path maps it back to a raw side offset via
    /// [`Fds::wire_map`].
    head: usize,
    /// Synthesized wire image of the currently inserted side (gap / `$80` /
    /// block / CRC). Empty when ejected. Rebuilt on every insert/side-swap and
    /// after the disk contents change (a BIOS save), so reads always present the
    /// hardware wire format the loader expects.
    wire: Vec<u8>,
    /// Per-block wire→raw mapping for the currently inserted side's [`Fds::wire`].
    /// Lets [`Fds::store_byte`] translate the wire head position back into the
    /// raw side offset so BIOS-written block payloads persist to `.fds`.
    wire_map: Vec<WireBlock>,
    /// Whether the drive has completed its spin-up since the last insert. The
    /// long spin-up not-ready window opens only on the FIRST motor-on after an
    /// insert (the cold spin-up the BIOS reset disk-check waits for). The BIOS
    /// briefly toggles the motor off between blocks during a multi-block read;
    /// those restarts must NOT re-open the spin-up window (the physical disk
    /// keeps spinning), or the `$4032.1` ready check mid-read would spuriously
    /// trip the BIOS's disk-error path.
    spun_up: bool,
    /// Read-path gap-skip state. The RP2C33 controller does not surface the
    /// inter-block gap (`$00` run) or the gap-terminating `$80` start mark to the
    /// CPU as byte-transfer events — it bit-shifts past them in hardware and only
    /// begins raising the transfer flag with the first real block byte. While
    /// this flag is set the read engine silently advances the head over gap +
    /// mark bytes before delivering data. It is armed whenever a read transfer
    /// (re)starts, so the BIOS, which resets the transfer between blocks, always
    /// re-syncs to the next block's start mark.
    read_skipping_gap: bool,
    /// Set on any disk write so a host can persist the modified image. Cleared
    /// via [`Fds::clear_disk_dirty`].
    disk_dirty: bool,
    /// Per-disk write-protect (the `$4032` bit-2 source). Default writable
    /// (`false`); [`Fds::set_write_protected`] sets it.
    write_protected: bool,
    /// Deterministic "not ready" countdown (CPU cycles) after an insert. While
    /// non-zero, `$4032` bit 1 stays set and no transfer runs — a minimal stand
    /// in for the drive's spin-up/seek, with no analog seek-time model. Also
    /// re-opened (for [`HEAD_RESEEK_CYCLES`] + the per-game quirk slack) on each
    /// motor-restart rewind so the BIOS re-read loop observes the not-ready ->
    /// ready transition (the FDS-proper timed disk-head position).
    insert_not_ready: u32,
    /// Per-game timing quirk resolved from the disk-image CRC-32 at construction
    /// ([`quirk_for_crc`]). [`FdsQuirk::NONE`] for the vast majority of titles.
    /// Derived once from immutable inputs, so it is not part of the save-state.
    quirk: FdsQuirk,

    // --- Registers ---
    /// $4020/$4021 — 16-bit timer IRQ reload value.
    timer_reload: u16,
    /// Live 16-bit timer counter.
    timer_counter: u16,
    /// $4022 bit 1 — timer IRQ enabled.
    timer_irq_enabled: bool,
    /// $4022 bit 0 — timer IRQ repeat (reload on expire).
    timer_irq_repeat: bool,
    /// $4023 bit 0 — master disk I/O register + timer enable.
    disk_io_enabled: bool,
    /// $4023 bit 1 — sound I/O enable (latched; audio is Stage 2).
    sound_io_enabled: bool,
    /// $4024 — last written disk write byte (write path is Stage 2).
    write_data: u8,
    /// $4025 control register (raw last write, for debug + bit decode).
    control: u8,
    /// $4026 external connector output latch.
    ext_output: u8,

    // --- Control-register decoded bits ($4025) ---
    transfer_reset: bool,  // bit 0 (1: reset transfer timing to initial state)
    motor_on: bool,        // bit 1 == 0 -> motor running (0: start, 1: stop)
    read_mode: bool,       // bit 2 (1: read, 0: write)
    mirroring: Mirroring,  // bit 3 (0: Vertical mirroring, 1: Horizontal mirroring)
    crc_control: bool,     // bit 5 (must be set for the byte-transfer flag)
    crc_enabled: bool,     // bit 6 (CRC enable; gates the byte-transfer flag)
    irq_on_transfer: bool, // bit 7

    // --- Status flags ($4030) ---
    timer_irq_flag: bool,     // bit 0
    byte_transfer_flag: bool, // bit 1
    crc_error: bool,          // bit 4
    end_of_head: bool,        // bit 6

    // --- Transfer engine ---
    transfer: TransferState,
    transfer_timer: u32,
    /// Byte most recently latched into the read shift register ($4031).
    read_data: u8,

    // --- IRQ line ---
    irq_pending: bool,

    // --- Sound channel ($4040-$4097) ---
    /// FDS 2C33 sound channel. Always present (register decode + save-state are
    /// build-independent); its synthesis is driven only under `mapper-audio`.
    audio: FdsAudio,

    // --- Diagnostic read-stream trace (runtime opt-in; off by default) ---
    /// When set (via [`Fds::enable_trace`]), disk-read / control / side-change
    /// events are appended to `trace`. Pure observation — never affects emulation,
    /// is not serialized, and defaults off, so the determinism contract holds.
    trace_on: bool,
    /// Accumulated trace records, drained by [`Fds::take_trace`].
    trace: Vec<FdsTraceRec>,
}

impl Fds {
    /// Construct an FDS device from a parsed disk image and an 8 KiB BIOS.
    ///
    /// # Errors
    ///
    /// Returns [`RomError::InvalidConfig`] if the BIOS is not exactly 8 KiB.
    pub fn new(disk: FdsDisk, bios: &[u8]) -> Result<Self, RomError> {
        if bios.len() != BIOS_LEN {
            return Err(RomError::InvalidConfig(format!(
                "FDS BIOS must be exactly {BIOS_LEN} bytes, got {}",
                bios.len()
            )));
        }
        // Synthesize the wire image for the power-on inserted side (side 0).
        let (wire, wire_map) = if disk.side_count() > 0 {
            build_side_wire(disk.side(0))
        } else {
            (Vec::new(), Vec::new())
        };
        // Resolve the per-game timing quirk from the disk-image CRC-32 (over the
        // headerless side bytes, matching `disk_image_bytes`).
        let quirk = quirk_for_crc(fds_crc32(&disk.to_bytes()));
        Ok(Self {
            prg_ram: vec![0u8; PRG_RAM_LEN].into_boxed_slice(),
            chr_ram: vec![0u8; CHR_RAM_LEN].into_boxed_slice(),
            bios: bios.to_vec().into_boxed_slice(),
            disk,
            inserted_side: Some(0),
            head: 0,
            wire,
            wire_map,
            disk_dirty: false,
            write_protected: false,
            timer_reload: 0,
            timer_counter: 0,
            timer_irq_enabled: false,
            timer_irq_repeat: false,
            disk_io_enabled: false,
            sound_io_enabled: false,
            write_data: 0,
            control: 0,
            ext_output: 0,
            motor_on: false,
            transfer_reset: true,
            read_mode: false,
            crc_control: false,
            crc_enabled: false,
            mirroring: Mirroring::Horizontal,
            irq_on_transfer: false,
            timer_irq_flag: false,
            byte_transfer_flag: false,
            crc_error: false,
            end_of_head: false,
            transfer: TransferState::Idle,
            transfer_timer: 0,
            read_data: 0,
            irq_pending: false,
            read_skipping_gap: true,
            spun_up: false,
            // The drive starts spun-down; the spin-up not-ready window opens when
            // the BIOS first turns the motor on (the motor off->on edge in
            // write_control), so the reset disk-check observes the not-ready ->
            // ready transition it waits for.
            insert_not_ready: 0,
            quirk,
            audio: FdsAudio::default(),
            trace_on: false,
            trace: Vec::new(),
        })
    }

    /// Re-evaluate whether a disk transfer (read or write) should be running,
    /// from the current control bits + inserted state. Called after any `$4025`
    /// write and after an insert/eject.
    ///
    /// The byte-transfer flag is gated (per the Takuika die-scan reference) on
    /// **CRC enabled (`$4025.D6`) + `$4025.D5` set**, plus — in read mode —
    /// motor on. It is NOT gated on transfer-reset (`$4025.D0`): the BIOS holds
    /// reset asserted while it arms CRC/IRQ and then re-syncs between blocks, yet
    /// still expects byte-transfer IRQs to flow. Transfer-reset only rewinds the
    /// head (handled on its rising edge in `write_control`).
    fn update_transfer_state(&mut self) {
        let inserted = self.inserted_side.is_some();
        let ready = inserted && self.insert_not_ready == 0;
        // CRC enable + bit5 are required for the byte-transfer machinery to run
        // at all (both read and write modes).
        let crc_armed = self.crc_enabled && self.crc_control;
        let desired = if !ready || !crc_armed {
            TransferState::Idle
        } else if self.read_mode {
            // Read also requires the motor running.
            if self.motor_on {
                TransferState::Reading
            } else {
                TransferState::Idle
            }
        } else {
            // Write mode: motor state is irrelevant.
            TransferState::Writing
        };
        if self.transfer != desired {
            self.transfer = desired;
            if desired != TransferState::Idle {
                // Begin transferring from the current head position. The timer
                // is seeded so the first byte lands one cadence later.
                self.transfer_timer = DISK_BYTE_CYCLES;
                // A (re)started read re-arms the controller's gap-skip so the
                // first delivered byte is the next block's first byte (the
                // controller hides the gap + $80 start mark from the CPU).
                if desired == TransferState::Reading {
                    self.read_skipping_gap = true;
                }
            }
        }
    }

    /// Advance the disk-read engine by one byte: latch the next disk byte into
    /// the read register, raise the byte-transfer flag (and IRQ if enabled), and
    /// move the head forward.
    fn deliver_byte(&mut self) {
        if self.inserted_side.is_some() {
            // When re-syncing to a block, the controller bit-shifts past the
            // gap ($00 run) and its terminating $80 start mark in hardware
            // without raising a byte-transfer event; the first event delivers
            // the byte that follows the mark (the block's first byte).
            if self.read_skipping_gap {
                while self.head < self.wire.len() && self.wire[self.head] == 0x00 {
                    self.head += 1;
                }
                if self.head < self.wire.len() && self.wire[self.head] == WIRE_START_MARK {
                    self.head += 1;
                    self.read_skipping_gap = false;
                } else if self.head >= self.wire.len() {
                    // No further start mark before the inner track: the head has
                    // reached the end of the side. Flag end-of-head and deliver
                    // $00; the BIOS uses $4030.D6 to detect "no more blocks".
                    self.read_data = 0;
                    self.end_of_head = true;
                    self.byte_transfer_flag = true;
                    if self.irq_on_transfer {
                        self.irq_pending = true;
                    }
                    return;
                } else {
                    // A non-zero, non-mark byte while skipping (should not occur
                    // for a well-formed wire image): treat it as data and stop
                    // skipping so we never stall.
                    self.read_skipping_gap = false;
                }
            }
            if self.head < self.wire.len() {
                self.read_data = self.wire[self.head];
                self.head += 1;
            } else {
                // The head reached the inner track (end of head): flag it and
                // deliver $00. The BIOS detects "no more data" via $4030.D6.
                self.read_data = 0;
                self.end_of_head = true;
            }
        } else {
            self.read_data = 0;
            self.end_of_head = true;
        }
        self.byte_transfer_flag = true;
        if self.irq_on_transfer {
            self.irq_pending = true;
        }
    }

    /// Rebuild the wire image + block map for the currently inserted side from
    /// the raw `.fds` side contents. Called on insert/side-swap and after a BIOS
    /// write changes the disk, so reads always present the up-to-date wire form.
    fn rebuild_wire(&mut self) {
        match self.inserted_side {
            Some(idx) if idx < self.disk.side_count() => {
                let (wire, map) = build_side_wire(self.disk.side(idx));
                self.wire = wire;
                self.wire_map = map;
            }
            _ => {
                self.wire.clear();
                self.wire_map.clear();
            }
        }
    }

    /// Map a wire head position to the raw side offset it corresponds to, when
    /// the head sits inside a block's payload region. Returns `None` for gap /
    /// start-mark / CRC positions (writes there modify only the synthesized
    /// framing, which is regenerated from the raw side, so they are dropped).
    fn wire_head_to_raw(&self, wire_pos: usize) -> Option<usize> {
        for blk in &self.wire_map {
            if wire_pos >= blk.wire_payload_start && wire_pos < blk.wire_payload_start + blk.len {
                return Some(blk.raw_start + (wire_pos - blk.wire_payload_start));
            }
        }
        None
    }

    /// Advance the disk-write engine by one byte: store the byte last written to
    /// `$4024` into the inserted side at the head position, mark the image
    /// dirty, raise the byte-transfer flag (and IRQ if enabled), and move the
    /// head forward. A write-protected disk drops the byte (the medium is not
    /// modified) but still advances the transfer machinery so timing-dependent
    /// BIOS code is unaffected. CRC/gap bytes are not synthesized (Stage-1
    /// simplification) — only the raw `$4024` byte stream lands on the medium.
    fn store_byte(&mut self) {
        if let Some(idx) = self.inserted_side {
            if self.head < self.wire.len() {
                if !self.write_protected {
                    // The BIOS write stream is itself the wire format (gap,
                    // start mark, block bytes, CRC). Land the byte on the wire
                    // image, and when it falls inside a block payload, mirror it
                    // into the raw `.fds` side so the modified disk persists. The
                    // BIOS writes whole blocks back to the same position it read
                    // them, so the existing block geometry stays valid.
                    let raw_off = self.wire_head_to_raw(self.head);
                    self.wire[self.head] = self.write_data;
                    if let Some(off) = raw_off
                        && off < self.disk.side(idx).len()
                    {
                        self.disk.side_mut(idx)[off] = self.write_data;
                    }
                    self.disk_dirty = true;
                }
                self.head += 1;
            } else {
                self.end_of_head = true;
            }
        } else {
            self.end_of_head = true;
        }
        self.byte_transfer_flag = true;
        if self.irq_on_transfer {
            self.irq_pending = true;
        }
    }

    /// Acknowledge a pending timer IRQ (clears the timer-IRQ status bit and the
    /// shared IRQ line if no disk IRQ remains). Mirrors the three documented ack
    /// paths: read `$4030`, write `$4022`, write `$4023`.
    fn ack_timer_irq(&mut self) {
        self.timer_irq_flag = false;
        self.recompute_irq_line();
    }

    /// Acknowledge a pending disk (byte-transfer) IRQ.
    fn ack_disk_irq(&mut self) {
        self.byte_transfer_flag = false;
        self.recompute_irq_line();
    }

    /// Recompute the shared IRQ line from the two latched flags.
    fn recompute_irq_line(&mut self) {
        // The IRQ line is the OR of the timer IRQ and (when armed) the disk
        // byte-transfer IRQ. Disk IRQs are only asserted while
        // `irq_on_transfer`; once the flag is cleared the line drops.
        self.irq_pending = self.timer_irq_flag || (self.irq_on_transfer && self.byte_transfer_flag);
    }

    /// Decode a `$4025` control write into the individual control bits.
    fn write_control(&mut self, value: u8) {
        self.trace_event(1, value);
        self.control = value;
        // $4025 bit layout (per nesdev / Takuika die-scan):
        //   bit0 = transfer reset (1: reset transfer timing to the initial state)
        //   bit1 = drive motor (0: start, 1: stop)
        //   bit2 = transfer mode (1: read, 0: write)
        //   bit3 = nametable arrangement
        //   bit5 = CRC transfer control (must be set for byte-transfer)
        //   bit6 = CRC enable (gates the byte-transfer flag)
        //   bit7 = byte-transfer IRQ enable
        let was_motor_on = self.motor_on;
        let was_transfer_reset = self.transfer_reset;
        self.transfer_reset = (value & 0x01) != 0; // bit 0
        self.motor_on = (value & 0x02) == 0; // bit 1 (0: start, 1: stop)
        self.read_mode = (value & 0x04) != 0;
        self.crc_control = (value & 0x20) != 0;
        self.crc_enabled = (value & 0x40) != 0;
        if self.motor_on && !was_motor_on {
            if self.spun_up {
                // A motor RESTART after the cold spin-up. The belt-driven drive
                // has physically rewound the disk to the disk-start gap (handled
                // on the preceding motor-off below), and the head must re-seek
                // to track 0 before the first block streams again — the
                // FDS-proper timed disk-head position. Open a short not-ready
                // window (plus any per-game quirk slack) so the BIOS re-read
                // loop observes the not-ready -> ready transition it waits for
                // on every re-read (e.g. Kid Icarus side-B post-registration).
                //
                // A restart with the head NOT at the disk start (mid-read motor
                // toggles the BIOS does between blocks) keeps streaming where it
                // left off and needs no re-seek — the disk never stopped turning
                // in that case. We only re-seek when the head sits at the start
                // (i.e. a true rewind happened), which is what the post-load
                // re-read sequence produces.
                if self.head == 0 {
                    self.insert_not_ready = self
                        .insert_not_ready
                        .max(HEAD_RESEEK_CYCLES + self.quirk.extra_reseek_cycles);
                }
            } else {
                // First motor-on since the disk was inserted: the drive spins up
                // and the head seeks to the disk start, reporting not-ready
                // ($4032.1) for a spin-up period the BIOS reset disk-check waits
                // for.
                self.insert_not_ready = MOTOR_SPIN_UP_CYCLES;
                self.spun_up = true;
            }
        }
        if !self.motor_on && was_motor_on {
            // Motor stop: the FDS drive is belt-driven and physically rewinds the
            // disk back to the start when the motor is turned off. The next
            // motor-on therefore reads from the first block again. This is how the
            // BIOS re-reads a side for a subsequent load (e.g. the game proper
            // after the licence screen, or a second LoadFiles pass): it stops the
            // motor, re-arms, and restarts — expecting block 1 to come back first.
            // Without the rewind the head stays parked at the inner track from the
            // prior read, so the next read delivers only trailing gap (no block) and
            // the load stalls forever. The cold spin-up window is NOT re-opened
            // (`spun_up` stays set) — only the disk position rewinds, matching the
            // mid-session rewind hardware does without a full spin-up delay.
            self.head = 0;
            self.end_of_head = false;
            self.read_skipping_gap = true;
        }
        // Asserting transfer reset (its rising edge) resets the byte-transfer
        // timing and re-arms the read gap-skip so the next delivered byte
        // re-syncs to a block start mark. It does NOT rewind the head — the head
        // tracks the physical disk rotation, which only returns to the start
        // when it reaches the inner track (end of head). The BIOS toggles reset
        // between blocks to re-align to each block's start mark; the head must
        // keep advancing across the inter-block gap so successive blocks are
        // read in order. The flag also does NOT stop byte transfers (they are
        // gated by CRC + bit5 + motor, not reset).
        if self.transfer_reset && !was_transfer_reset {
            self.read_skipping_gap = true;
            self.transfer_timer = DISK_BYTE_CYCLES;
        }
        // $4025 bit 3 = nametable arrangement (FDS naming, nesdev wiki):
        //   0: "Horizontal arrangement" == VERTICAL mirroring == $2000 aliases
        //      $2800 (this codebase's `Mirroring::Vertical`, tables 0/2 -> bank 0).
        //   1: "Vertical arrangement" == HORIZONTAL mirroring == $2000 and $2800
        //      are distinct physical nametables (this codebase's
        //      `Mirroring::Horizontal`, tables 0/1 -> bank 0, 2/3 -> bank 1).
        // The FDS BIOS boot/licence flow loads the KYODAKU approval tilemap to
        // $2800 then clears+rebuilds the message nametable at $2000 (with bit 3
        // set), relying on $2000 and $2800 being separate banks. The previous
        // mapping had these swapped, so clearing $2000 wiped the $2800 licence
        // before the BIOS's self-verify read it back, yielding "DISK TROUBLE
        // ERR.20" on every real-BIOS boot.
        self.mirroring = if (value & 0x08) != 0 {
            Mirroring::Horizontal
        } else {
            Mirroring::Vertical
        };
        self.irq_on_transfer = (value & 0x80) != 0;
        // Writing $4025 acknowledges disk IRQs.
        self.ack_disk_irq();
        self.update_transfer_state();
    }

    /// Read of the disk status register `$4030` (acknowledges timer + disk IRQs).
    fn read_status_4030(&mut self) -> u8 {
        let mut v = 0u8;
        if self.timer_irq_flag {
            v |= 0x01; // bit 0: timer IRQ
        }
        if self.byte_transfer_flag {
            v |= 0x80; // bit 7: byte-transfer flag (NOT bit 1)
        }
        if self.crc_error {
            v |= 0x10; // bit 4: CRC error
        }
        if self.end_of_head {
            v |= 0x40; // bit 6: end of head
        }
        // Reading $4030 acknowledges the timer IRQ but, contrary to older docs,
        // does NOT clear the byte-transfer flag nor acknowledge its IRQ — only a
        // $4024/$4031 service does (Takuika die-scan reference). Leaving the
        // byte-transfer flag latched here is what lets the BIOS poll $4030.D7.
        self.timer_irq_flag = false;
        self.recompute_irq_line();
        v
    }

    /// Read of the read data register `$4031` (consumes the latched disk byte,
    /// clears the byte-transfer flag, acknowledges disk IRQs).
    fn read_data_4031(&mut self) -> u8 {
        let v = self.read_data;
        self.trace_event(0, v);
        self.byte_transfer_flag = false;
        self.recompute_irq_line();
        v
    }

    /// Append one [`FdsTraceRec`] when tracing is enabled (a no-op otherwise, so
    /// default builds carry zero overhead beyond a single bool check on the cold
    /// register paths). See [`Fds::enable_trace`].
    fn trace_event(&mut self, kind: u8, value: u8) {
        // Cap the diagnostic buffer so a long session (or tracing accidentally
        // left on) can't grow it without bound; the early records are the ones
        // that matter for the boot/swap investigations this facility serves.
        const MAX_TRACE_RECORDS: usize = 100_000;
        if self.trace_on && self.trace.len() < MAX_TRACE_RECORDS {
            let status = self.read_status_4030_peek();
            self.trace.push(FdsTraceRec {
                kind,
                value,
                head: self.head as u32,
                side: self.inserted_side.map_or(-1, |s| s as i8),
                status,
            });
        }
    }

    /// Non-mutating snapshot of the `$4030` status bits (for the trace record;
    /// unlike [`Self::read_status_4030`] it does not acknowledge the timer IRQ).
    fn read_status_4030_peek(&self) -> u8 {
        let mut v = 0u8;
        if self.timer_irq_flag {
            v |= 0x01;
        }
        if self.byte_transfer_flag {
            v |= 0x80;
        }
        if self.crc_error {
            v |= 0x10;
        }
        if self.end_of_head {
            v |= 0x40;
        }
        v
    }

    /// The per-game timing quirk resolved from the disk CRC-32 at construction
    /// ([`quirk_for_crc`]). [`FdsQuirk::NONE`] for titles without a table entry.
    #[must_use]
    pub fn quirk(&self) -> FdsQuirk {
        self.quirk
    }

    /// Start recording the diagnostic FDS read-stream trace (see [`FdsTraceRec`]).
    /// Off by default; recording is pure observation and never affects emulation.
    pub fn enable_trace(&mut self) {
        self.trace_on = true;
    }

    /// Drain the accumulated FDS trace records.
    pub fn take_trace(&mut self) -> Vec<FdsTraceRec> {
        core::mem::take(&mut self.trace)
    }

    /// Read of the drive status register `$4032`.
    ///
    /// Models the drive's auto-insert / re-seek presentation to the loader: the
    /// not-ready bit (1) is driven by the spin-up / re-seek windows
    /// ([`MOTOR_SPIN_UP_CYCLES`] / [`HEAD_RESEEK_CYCLES`] + per-game quirk),
    /// the motor state, and end-of-head, so the disk re-presents itself on each
    /// re-read with the not-ready -> ready transition the BIOS waits for.
    fn read_drive_status_4032(&self) -> u8 {
        let inserted = self.inserted_side.is_some();
        let mut v = 0u8;
        // bit 0: disk flag (0: inserted, 1: not inserted).
        if !inserted {
            v |= 0x01;
        }
        // bit 1: ready flag (0: ready, 1: not ready). Set while no disk is
        // inserted, during the post-insert not-ready / re-seek window, when the
        // motor is stopped, or when the head has reached the inner track (end of
        // side).
        if !inserted || self.insert_not_ready > 0 || self.end_of_head || !self.motor_on {
            v |= 0x02;
        }
        // bit 2: write-protect (per-disk flag; also forced when ejected, per
        // the wiki: "Write protected or disk ejected").
        if self.write_protected || !inserted {
            v |= 0x04;
        }
        v
    }

    /// Read of the external connector input `$4033` (bit 7 = battery good).
    fn read_ext_4033(&self) -> u8 {
        // Battery good when a disk is present. The low 7 bits read back the
        // open-collector $4026 output (Stage 1: pass the latched output through,
        // masked to the input bits).
        let battery = if self.inserted_side.is_some() {
            0x80
        } else {
            0x00
        };
        battery | (self.ext_output & 0x7F)
    }

    /// Insert side `i` (`Some`) or eject (`None`). An insert resets the head to
    /// the start of the side and opens a short deterministic not-ready window;
    /// an eject stops any transfer. Out-of-range indices are ignored (the
    /// caller bounds via [`Mapper::disk_side_count`]). Shared helper behind the
    /// [`Mapper::set_disk_side`] trait override.
    fn do_set_disk_side(&mut self, side: Option<usize>) {
        self.trace_event(2, side.map_or(0xFF, |s| s as u8));
        match side {
            Some(i) if i < self.disk.side_count() => {
                self.inserted_side = Some(i);
                self.head = 0;
                self.end_of_head = false;
                self.insert_not_ready = INSERT_NOT_READY_CYCLES;
                // A freshly inserted disk must spin up on the next motor-on.
                self.spun_up = false;
                self.rebuild_wire();
            }
            Some(_) => { /* out of range: ignore */ }
            None => {
                self.inserted_side = None;
                self.insert_not_ready = 0;
                self.end_of_head = false;
                self.spun_up = false;
                self.rebuild_wire();
            }
        }
        // Recompute whether a transfer can run given the new inserted state.
        self.update_transfer_state();
    }

    /// Parse the v3 disk tail starting at byte `off` (after the audio tail).
    /// `base` is the fixed-prefix length used to validate the total v3 size.
    fn load_disk_tail(
        &mut self,
        data: &[u8],
        mut off: usize,
        base: usize,
    ) -> Result<(), MapperError> {
        let saved_sides = u32::from_le_bytes(data[off..off + 4].try_into().unwrap()) as usize;
        off += 4;
        // Validate the full v3 length now that the side count is known.
        let expected = base + FdsAudio::TAIL_LEN + 4 + saved_sides * FDS_SIDE_LEN + 4 + 4 + 1;
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        // Restore the side contents into the matching local sides. A foreign
        // blob with a different side count restores only the overlap so we never
        // index out of range either way.
        let restore = saved_sides.min(self.disk.side_count());
        for s in 0..restore {
            self.disk
                .side_mut(s)
                .copy_from_slice(&data[off..off + FDS_SIDE_LEN]);
            off += FDS_SIDE_LEN;
        }
        // Skip any extra saved sides we have no local slot for.
        off += (saved_sides - restore) * FDS_SIDE_LEN;
        let inserted = u32::from_le_bytes(data[off..off + 4].try_into().unwrap());
        off += 4;
        self.inserted_side = if inserted == u32::MAX {
            None
        } else {
            Some((inserted as usize).min(self.disk.side_count().saturating_sub(1)))
        };
        self.insert_not_ready = u32::from_le_bytes(data[off..off + 4].try_into().unwrap());
        off += 4;
        let disk_flags = data[off];
        self.disk_dirty = (disk_flags & 0x01) != 0;
        self.write_protected = (disk_flags & 0x02) != 0;
        self.spun_up = (disk_flags & 0x04) != 0;
        Ok(())
    }
}

/// Save-state format version for the FDS device.
///
/// - v1: Stage 1 (memory + disk position + timer/transfer + IRQ). No audio tail.
/// - v2: appends the [`FdsAudio`] sound-channel tail ([`FdsAudio::TAIL_LEN`]).
///   Strictly additive — v1 blobs load with the audio unit left at default.
/// - v3 (Stage 2b): appends a **disk tail** after the audio tail capturing the
///   mutable disk contents (so mid-write rewind/save-state round-trips), the
///   `inserted_side` (`Option`, encoded as `0xFFFF_FFFF` for ejected),
///   `disk_dirty`, `write_protected`, the `insert_not_ready` countdown, and the
///   write-vs-read transfer phase. The `inserted_side` field in the v1 section
///   is retained (clamped) for back-compat; v3 overwrites it from the tail.
///   Loading a v1/v2 blob leaves the disk at its construction contents
///   (un-modified), side 0 inserted, not dirty, writable.
const FDS_SAVE_VERSION: u8 = 3;

impl Mapper for Fds {
    // v2.8.0 Phase 4 — CPU-cycle hook + IRQ source + expansion audio
    // (the audio hook only exists under the `mapper-audio` feature).
    fn caps(&self) -> MapperCaps {
        MapperCaps {
            cpu_cycle_hook: true,
            audio: cfg!(feature = "mapper-audio"),
            frame_event_hook: false,
            irq_source: true,
        }
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            // Disk registers are read-only at $4030-$4033 and gated on disk I/O
            // enable (apart from open-bus behaviour the bus handles elsewhere).
            0x4030 => self.read_status_4030(),
            0x4031 => self.read_data_4031(),
            0x4032 => self.read_drive_status_4032(),
            0x4033 => self.read_ext_4033(),
            // Sound channel reads: volume/mod gain + internal accumulators.
            // When write-protect is active ($4089 bit 7 clear), reading any
            // wavetable byte ($4040-$407F) returns the value at the current
            // wave position; while write-enabled it reads back the RAM.
            0x4040..=0x407F => {
                if self.audio.wave_write_enable {
                    self.audio.wavetable[(addr - 0x4040) as usize]
                } else {
                    self.audio.wave_out_latch & 0x3F
                }
            }
            0x4090..=0x4097 => self.audio.read_reg(addr).unwrap_or(0),
            // PRG-RAM at $6000-$DFFF.
            0x6000..=0xDFFF => self.prg_ram[(addr - 0x6000) as usize],
            // BIOS at $E000-$FFFF.
            0xE000..=0xFFFF => self.bios[(addr - 0xE000) as usize],
            // Everything else in the device window reads as open bus; the bus's
            // floating-latch handling owns the exact value, so return 0 here.
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        match addr {
            // $4020/$4021 reload value — NOT gated on disk I/O enable.
            0x4020 => {
                self.timer_reload = (self.timer_reload & 0xFF00) | u16::from(value);
            }
            0x4021 => {
                self.timer_reload = (self.timer_reload & 0x00FF) | (u16::from(value) << 8);
            }
            // $4022 timer IRQ control — gated on disk I/O enable.
            0x4022 => {
                if self.disk_io_enabled {
                    self.timer_irq_repeat = (value & 0x01) != 0;
                    let enable = (value & 0x02) != 0;
                    if enable {
                        // Enabling copies the reload value into the counter.
                        self.timer_counter = self.timer_reload;
                        self.timer_irq_enabled = true;
                    } else {
                        // Disabling stops the counter and acks pending timer IRQ.
                        self.timer_irq_enabled = false;
                        self.ack_timer_irq();
                    }
                }
            }
            // $4023 master I/O enable.
            0x4023 => {
                self.disk_io_enabled = (value & 0x01) != 0;
                self.sound_io_enabled = (value & 0x02) != 0;
                if !self.disk_io_enabled {
                    // Clearing disk registers immediately stops the timer and
                    // acks pending timer IRQs (also disables disk IRQs).
                    self.timer_irq_enabled = false;
                    self.timer_irq_flag = false;
                    self.recompute_irq_line();
                }
            }
            // $4024 write data register: the byte to load into the shift
            // register on the next byte-transfer tick (stored to the medium in
            // write mode by `store_byte`). Writing $4024 acknowledges disk IRQs.
            0x4024 => {
                self.write_data = value;
                self.ack_disk_irq();
            }
            // $4025 FDS control.
            0x4025 => self.write_control(value),
            // $4026 external connector output.
            0x4026 => self.ext_output = value,
            // Sound channel registers ($4040-$408A). Per the wiki these require
            // the sound I/O enable bit ($4023 bit 1) to be set to take effect.
            0x4040..=0x408A => {
                if self.sound_io_enabled {
                    self.audio.write_reg(addr, value);
                }
            }
            // PRG-RAM at $6000-$DFFF (writable).
            0x6000..=0xDFFF => self.prg_ram[(addr - 0x6000) as usize] = value,
            // BIOS / unmapped: ignore.
            _ => {}
        }
    }

    fn cpu_read_unmapped(&self, addr: u16) -> bool {
        // The FDS registers occupy $4020-$409F (the $4040-$4092 sound block is
        // Stage 2 but still part of the device). Reporting these as MAPPED routes
        // the reads to `cpu_read` instead of the open-bus latch. PRG-RAM and BIOS
        // ($6000-$FFFF) are always mapped. Anything else in $40A0-$5FFF is
        // genuinely unmapped (open bus).
        if (0x4020..=0x409F).contains(&addr) {
            return false;
        }
        (0x40A0..=0x5FFF).contains(&addr)
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x1FFF => self.chr_ram[(addr & 0x1FFF) as usize],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        if addr <= 0x1FFF {
            self.chr_ram[(addr & 0x1FFF) as usize] = value;
        }
    }

    fn notify_cpu_cycle(&mut self) {
        // Sound channel runs every CPU cycle (independent of disk I/O).
        #[cfg(feature = "mapper-audio")]
        self.audio.clock();

        // Timer IRQ: decrement each CPU cycle while enabled.
        if self.timer_irq_enabled {
            if self.timer_counter == 0 {
                // Already at zero: fire and reload/disable.
                self.timer_irq_flag = true;
                self.irq_pending = true;
                if self.timer_irq_repeat {
                    self.timer_counter = self.timer_reload;
                } else {
                    self.timer_irq_enabled = false;
                }
            } else {
                self.timer_counter -= 1;
                if self.timer_counter == 0 {
                    self.timer_irq_flag = true;
                    self.irq_pending = true;
                    if self.timer_irq_repeat {
                        self.timer_counter = self.timer_reload;
                    } else {
                        self.timer_irq_enabled = false;
                    }
                }
            }
        }

        // Post-insert not-ready window: count down, then re-evaluate whether a
        // transfer can begin (so a motor-on read/write started during the
        // window kicks off once the drive reports ready).
        if self.insert_not_ready > 0 {
            self.insert_not_ready -= 1;
            if self.insert_not_ready == 0 {
                self.update_transfer_state();
            }
        }

        // Disk transfer: deliver (read) or store (write) a byte every
        // DISK_BYTE_CYCLES cycles.
        if self.disk_io_enabled {
            match self.transfer {
                TransferState::Reading => {
                    if self.transfer_timer > 0 {
                        self.transfer_timer -= 1;
                    }
                    if self.transfer_timer == 0 {
                        self.deliver_byte();
                        self.transfer_timer = DISK_BYTE_CYCLES;
                    }
                }
                TransferState::Writing => {
                    if self.transfer_timer > 0 {
                        self.transfer_timer -= 1;
                    }
                    if self.transfer_timer == 0 {
                        self.store_byte();
                        self.transfer_timer = DISK_BYTE_CYCLES;
                    }
                }
                TransferState::Idle => {}
            }
        }
    }

    fn irq_pending(&self) -> bool {
        self.irq_pending
    }

    fn irq_acknowledge(&mut self) {
        // The CPU acks the IRQ line; the underlying status flags are cleared by
        // the BIOS reading $4030 / writing $4022/$4023/$4024. Dropping the line
        // here keeps it from re-asserting spuriously between those services.
        self.irq_pending = false;
    }

    #[cfg(feature = "mapper-audio")]
    fn mix_audio(&mut self) -> i16 {
        self.audio.output()
    }

    fn current_mirroring(&self) -> Mirroring {
        self.mirroring
    }

    // --- Stage 2b disk interface (trait overrides) ---

    fn disk_side_count(&self) -> usize {
        self.disk.side_count()
    }

    fn inserted_disk_side(&self) -> Option<usize> {
        self.inserted_side
    }

    fn set_disk_side(&mut self, side: Option<usize>) {
        self.do_set_disk_side(side);
    }

    fn enable_fds_trace(&mut self) {
        self.enable_trace();
    }

    fn take_fds_trace(&mut self) -> Vec<FdsTraceRec> {
        self.take_trace()
    }

    fn disk_image_bytes(&self) -> Vec<u8> {
        self.disk.to_bytes()
    }

    fn disk_is_dirty(&self) -> bool {
        self.disk_dirty
    }

    fn clear_disk_dirty(&mut self) {
        self.disk_dirty = false;
    }

    fn set_disk_write_protected(&mut self, protected: bool) {
        self.write_protected = protected;
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(32 + self.prg_ram.len() + self.chr_ram.len());
        out.push(FDS_SAVE_VERSION);
        // Memory.
        out.extend_from_slice(&self.prg_ram);
        out.extend_from_slice(&self.chr_ram);
        // Disk position. The v1-shaped `inserted_side` u32 holds the side index
        // (0 when ejected) for back-compat; the authoritative Option is in the
        // v3 disk tail below.
        let legacy_side = self.inserted_side.unwrap_or(0) as u32;
        out.extend_from_slice(&legacy_side.to_le_bytes());
        out.extend_from_slice(&(self.head as u32).to_le_bytes());
        // Registers / timer.
        out.extend_from_slice(&self.timer_reload.to_le_bytes());
        out.extend_from_slice(&self.timer_counter.to_le_bytes());
        out.extend_from_slice(&self.transfer_timer.to_le_bytes());
        out.push(self.write_data);
        out.push(self.control);
        out.push(self.ext_output);
        out.push(self.read_data);
        // Packed booleans.
        let mut flags = 0u16;
        flags |= u16::from(self.timer_irq_enabled);
        flags |= u16::from(self.timer_irq_repeat) << 1;
        flags |= u16::from(self.disk_io_enabled) << 2;
        flags |= u16::from(self.sound_io_enabled) << 3;
        flags |= u16::from(self.motor_on) << 4;
        flags |= u16::from(self.transfer_reset) << 5;
        flags |= u16::from(self.read_mode) << 6;
        flags |= u16::from(self.irq_on_transfer) << 7;
        flags |= u16::from(self.timer_irq_flag) << 8;
        flags |= u16::from(self.byte_transfer_flag) << 9;
        flags |= u16::from(self.crc_error) << 10;
        flags |= u16::from(self.end_of_head) << 11;
        flags |= u16::from(self.transfer == TransferState::Reading) << 12;
        flags |= u16::from(self.irq_pending) << 13;
        // Bit 14 distinguishes a write transfer (bit 12 covers read). v1/v2
        // never set bit 14, so they restore as a non-write (Idle/Reading).
        flags |= u16::from(self.transfer == TransferState::Writing) << 14;
        // Bit 15: read-path gap-skip state. v1/v2 leave it clear, which restores
        // as "not skipping" — a mid-block read position, the safe default for a
        // legacy blob (a transfer-reset re-arms it anyway).
        flags |= u16::from(self.read_skipping_gap) << 15;
        out.extend_from_slice(&flags.to_le_bytes());
        // v2 audio tail (strictly additive after the v1 section).
        self.audio.write_tail(&mut out);
        // v3 disk tail: mutable disk contents + insert/write-path state.
        out.extend_from_slice(&(self.disk.side_count() as u32).to_le_bytes());
        for s in 0..self.disk.side_count() {
            out.extend_from_slice(self.disk.side(s));
        }
        // inserted_side Option: 0xFFFF_FFFF sentinel for ejected.
        let inserted = self.inserted_side.map_or(u32::MAX, |i| i as u32);
        out.extend_from_slice(&inserted.to_le_bytes());
        out.extend_from_slice(&self.insert_not_ready.to_le_bytes());
        let mut disk_flags = 0u8;
        disk_flags |= u8::from(self.disk_dirty);
        disk_flags |= u8::from(self.write_protected) << 1;
        disk_flags |= u8::from(self.spun_up) << 2;
        out.push(disk_flags);
        out
    }

    #[allow(clippy::too_many_lines)]
    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        // v1: device state only. v2: + the FdsAudio tail. v3: + the disk tail.
        let base = 1 + self.prg_ram.len() + self.chr_ram.len() + 4 + 4 + 2 + 2 + 4 + 4 + 2;
        let version = data.first().copied().unwrap_or(0);
        // For v3 the disk tail is variable-length (it embeds its own side count
        // as the first u32), so we validate the fixed prefix here and the disk
        // tail length once we know the count below. v1/v2 are fixed-length.
        match version {
            1 => {
                if data.len() != base {
                    return Err(MapperError::Truncated {
                        expected: base,
                        got: data.len(),
                    });
                }
            }
            2 => {
                let expected = base + FdsAudio::TAIL_LEN;
                if data.len() != expected {
                    return Err(MapperError::Truncated {
                        expected,
                        got: data.len(),
                    });
                }
            }
            3 => {
                // Need at least the fixed prefix + audio tail + the disk tail's
                // leading side-count u32 to learn how long the tail is.
                let min = base + FdsAudio::TAIL_LEN + 4;
                if data.len() < min {
                    return Err(MapperError::Truncated {
                        expected: min,
                        got: data.len(),
                    });
                }
            }
            other => return Err(MapperError::UnsupportedVersion(other)),
        }
        let mut off = 1;
        let pl = self.prg_ram.len();
        self.prg_ram.copy_from_slice(&data[off..off + pl]);
        off += pl;
        let cl = self.chr_ram.len();
        self.chr_ram.copy_from_slice(&data[off..off + cl]);
        off += cl;

        let legacy_inserted = u32::from_le_bytes(data[off..off + 4].try_into().unwrap()) as usize;
        off += 4;
        let head = u32::from_le_bytes(data[off..off + 4].try_into().unwrap()) as usize;
        off += 4;
        // Clamp restored positions to valid ranges (a corrupt/foreign blob must
        // not be able to drive an out-of-range index into `side()`). For v1/v2
        // this is the authoritative inserted side; v3 overwrites it from the
        // disk tail below.
        self.inserted_side = Some(legacy_inserted.min(self.disk.side_count().saturating_sub(1)));
        // `head` is a wire-image offset; clamp it after the wire image is rebuilt
        // at the end of restore (the disk tail may change the inserted side).
        self.head = head;

        self.timer_reload = u16::from_le_bytes(data[off..off + 2].try_into().unwrap());
        off += 2;
        self.timer_counter = u16::from_le_bytes(data[off..off + 2].try_into().unwrap());
        off += 2;
        self.transfer_timer = u32::from_le_bytes(data[off..off + 4].try_into().unwrap());
        off += 4;
        self.write_data = data[off];
        off += 1;
        self.control = data[off];
        off += 1;
        self.ext_output = data[off];
        off += 1;
        self.read_data = data[off];
        off += 1;
        let flags = u16::from_le_bytes(data[off..off + 2].try_into().unwrap());
        self.timer_irq_enabled = (flags & (1 << 0)) != 0;
        self.timer_irq_repeat = (flags & (1 << 1)) != 0;
        self.disk_io_enabled = (flags & (1 << 2)) != 0;
        self.sound_io_enabled = (flags & (1 << 3)) != 0;
        self.motor_on = (flags & (1 << 4)) != 0;
        self.transfer_reset = (flags & (1 << 5)) != 0;
        self.read_mode = (flags & (1 << 6)) != 0;
        self.irq_on_transfer = (flags & (1 << 7)) != 0;
        self.timer_irq_flag = (flags & (1 << 8)) != 0;
        self.byte_transfer_flag = (flags & (1 << 9)) != 0;
        self.crc_error = (flags & (1 << 10)) != 0;
        self.end_of_head = (flags & (1 << 11)) != 0;
        self.transfer = if (flags & (1 << 14)) != 0 {
            TransferState::Writing
        } else if (flags & (1 << 12)) != 0 {
            TransferState::Reading
        } else {
            TransferState::Idle
        };
        self.irq_pending = (flags & (1 << 13)) != 0;
        self.read_skipping_gap = (flags & (1 << 15)) != 0;
        off += 2;
        // Re-derive mirroring from the saved control byte for consistency.
        // Must match `write_control`'s corrected $4025.D3 mapping: bit 3 = 1 ->
        // Horizontal mirroring ($2000 != $2800), bit 3 = 0 -> Vertical.
        self.mirroring = if (self.control & 0x08) != 0 {
            Mirroring::Horizontal
        } else {
            Mirroring::Vertical
        };
        // v2 audio tail, or default the sound channel for legacy v1 blobs.
        if version >= 2 {
            self.audio.read_tail(&data[off..off + FdsAudio::TAIL_LEN])?;
            off += FdsAudio::TAIL_LEN;
        } else {
            self.audio = FdsAudio::default();
        }
        // v3 disk tail: mutable disk contents + insert / write-path state.
        // Legacy v1/v2 blobs leave the disk at its construction contents
        // (un-modified), side 0 inserted, not dirty, writable.
        if version >= 3 {
            self.load_disk_tail(data, off, base)?;
        } else {
            self.disk_dirty = false;
            self.write_protected = false;
            self.insert_not_ready = 0;
            // v1/v2 blobs predate the spin-up model: treat the drive as already
            // spun up so a restored mid-game state does not re-trigger a spin-up
            // window (the disk was spinning when the state was captured).
            self.spun_up = true;
        }
        // Rebuild the wire image from the (possibly modified) inserted side and
        // clamp the restored head into it. The wire image is derived state — it
        // is reconstructed from the saved raw side contents rather than stored.
        self.rebuild_wire();
        self.head = self.head.min(self.wire.len());
        Ok(())
    }

    fn debug_info(&self) -> MapperDebugInfo {
        MapperDebugInfo {
            mapper_id: 20,
            name: String::from("FDS (RAM adapter)"),
            mirroring: crate::mapper::mirroring_name(self.mirroring),
            prg_banks: vec![(String::from("PRG-RAM"), String::from("$6000-$DFFF (32K)"))],
            chr_banks: vec![(String::from("CHR-RAM"), String::from("8K"))],
            irq_state: vec![
                (
                    String::from("reload"),
                    format!("{:#06X}", self.timer_reload),
                ),
                (
                    String::from("counter"),
                    format!("{:#06X}", self.timer_counter),
                ),
                (
                    String::from("enabled"),
                    format!("{}", self.timer_irq_enabled),
                ),
                (String::from("repeat"), format!("{}", self.timer_irq_repeat)),
                (String::from("pending"), format!("{}", self.irq_pending)),
            ],
            extra: vec![
                (
                    String::from("side"),
                    self.inserted_side
                        .map_or_else(|| String::from("ejected"), |i| format!("{i}")),
                ),
                (String::from("sides"), format!("{}", self.disk.side_count())),
                (String::from("head"), format!("{}", self.head)),
                (String::from("disk_io"), format!("{}", self.disk_io_enabled)),
                (String::from("motor"), format!("{}", self.motor_on)),
                (String::from("read_mode"), format!("{}", self.read_mode)),
                (String::from("dirty"), format!("{}", self.disk_dirty)),
                (
                    String::from("write_protect"),
                    format!("{}", self.write_protected),
                ),
                (
                    String::from("reseek_slack"),
                    format!("{}", self.quirk.extra_reseek_cycles),
                ),
            ],
            // Cartridge-level metadata is filled by the bus (v1.5.0 I8).
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a synthetic fwNES image with `sides` sides, side `s` filled with a
    /// recognizable pattern keyed on the side index.
    fn synth_fwnes(sides: u8) -> Vec<u8> {
        let mut out = vec![0u8; FWNES_HEADER_LEN + sides as usize * FDS_SIDE_LEN];
        out[0..4].copy_from_slice(b"FDS\x1A");
        out[4] = sides;
        for s in 0..sides as usize {
            let base = FWNES_HEADER_LEN + s * FDS_SIDE_LEN;
            out[base] = 0x01;
            out[base + 1..base + 15].copy_from_slice(b"*NINTENDO-HVC*");
            // Distinctive bytes at the start of the data region.
            out[base + 16] = s as u8;
            out[base + 17] = 0xAA;
            out[base + 18] = 0xBB;
        }
        out
    }

    fn synth_raw_side() -> Vec<u8> {
        let mut out = vec![0u8; FDS_SIDE_LEN];
        out[0] = 0x01;
        out[1..15].copy_from_slice(b"*NINTENDO-HVC*");
        out[15] = 0x42;
        out
    }

    fn dummy_bios() -> Vec<u8> {
        // Distinctive ramp so reads can be checked positionally.
        (0..BIOS_LEN).map(|i| (i & 0xFF) as u8).collect()
    }

    fn make_device(sides: u8) -> Fds {
        let disk = parse_fds(&synth_fwnes(sides)).unwrap();
        Fds::new(disk, &dummy_bios()).unwrap()
    }

    // --- Parser ---

    #[test]
    fn parse_fwnes_single_side() {
        let disk = parse_fds(&synth_fwnes(1)).unwrap();
        assert_eq!(disk.side_count(), 1);
        assert_eq!(disk.declared_side_count(), 1);
        assert_eq!(disk.side(0)[0], 0x01);
        assert_eq!(&disk.side(0)[1..15], b"*NINTENDO-HVC*");
    }

    #[test]
    fn parse_fwnes_multi_side() {
        let disk = parse_fds(&synth_fwnes(3)).unwrap();
        assert_eq!(disk.side_count(), 3);
        assert_eq!(disk.side(0)[16], 0);
        assert_eq!(disk.side(1)[16], 1);
        assert_eq!(disk.side(2)[16], 2);
    }

    #[test]
    fn parse_headerless_raw_side() {
        let disk = parse_fds(&synth_raw_side()).unwrap();
        assert_eq!(disk.side_count(), 1);
        assert_eq!(disk.declared_side_count(), 0);
        assert_eq!(disk.side(0)[15], 0x42);
    }

    #[test]
    fn parse_rejects_garbage() {
        let bytes = vec![0xFFu8; 64];
        assert!(matches!(parse_fds(&bytes), Err(RomError::BadMagic)));
    }

    #[test]
    fn parse_rejects_truncated() {
        // fwNES header claiming 2 sides but only ~1.5 sides of body.
        let mut bytes = vec![0u8; FWNES_HEADER_LEN + FDS_SIDE_LEN + FDS_SIDE_LEN / 2];
        bytes[0..4].copy_from_slice(b"FDS\x1A");
        bytes[4] = 2;
        assert!(matches!(parse_fds(&bytes), Err(RomError::Truncated { .. })));
    }

    #[test]
    fn parse_qd_side_truncated_to_read_window() {
        // QD form: 65536-byte sides, no header. Must degrade to a 65500 read
        // window. Build a raw QD-length side (still opens with the disk-info
        // block so the headerless path accepts it).
        let mut bytes = vec![0u8; QD_SIDE_LEN];
        bytes[0] = 0x01;
        bytes[1..15].copy_from_slice(b"*NINTENDO-HVC*");
        let disk = parse_fds(&bytes).unwrap();
        assert_eq!(disk.side_count(), 1);
        assert_eq!(disk.side(0).len(), FDS_SIDE_LEN);
    }

    #[test]
    fn bios_must_be_8k() {
        let disk = parse_fds(&synth_fwnes(1)).unwrap();
        assert!(matches!(
            Fds::new(disk.clone(), &[0u8; 100]),
            Err(RomError::InvalidConfig(_))
        ));
        assert!(Fds::new(disk, &dummy_bios()).is_ok());
    }

    // --- Memory map ---

    #[test]
    fn prg_ram_read_write() {
        let mut fds = make_device(1);
        fds.cpu_write(0x6000, 0x11);
        fds.cpu_write(0xDFFF, 0x22);
        assert_eq!(fds.cpu_read(0x6000), 0x11);
        assert_eq!(fds.cpu_read(0xDFFF), 0x22);
    }

    #[test]
    fn bios_is_read_only() {
        let mut fds = make_device(1);
        assert_eq!(fds.cpu_read(0xE000), 0x00);
        assert_eq!(fds.cpu_read(0xE001), 0x01);
        assert_eq!(fds.cpu_read(0xFFFF), (0x1FFF & 0xFF) as u8);
        // Writes to BIOS are ignored.
        fds.cpu_write(0xE000, 0xFF);
        assert_eq!(fds.cpu_read(0xE000), 0x00);
    }

    #[test]
    fn chr_ram_read_write() {
        let mut fds = make_device(1);
        fds.ppu_write(0x0000, 0x55);
        fds.ppu_write(0x1FFF, 0x66);
        assert_eq!(fds.ppu_read(0x0000), 0x55);
        assert_eq!(fds.ppu_read(0x1FFF), 0x66);
    }

    #[test]
    fn registers_routed_not_open_bus() {
        let fds = make_device(1);
        // $4020-$409F must report as MAPPED so the bus routes them to cpu_read.
        assert!(!fds.cpu_read_unmapped(0x4020));
        assert!(!fds.cpu_read_unmapped(0x4030));
        assert!(!fds.cpu_read_unmapped(0x409F));
        // $40A0-$5FFF is unmapped (open bus).
        assert!(fds.cpu_read_unmapped(0x40A0));
        assert!(fds.cpu_read_unmapped(0x5000));
        // $6000+ is always mapped.
        assert!(!fds.cpu_read_unmapped(0x6000));
        assert!(!fds.cpu_read_unmapped(0xE000));
    }

    // --- Timer IRQ ---

    fn enable_disk_io(fds: &mut Fds) {
        fds.cpu_write(0x4023, 0x01);
        // Close the power-on spin-up not-ready window so the transfer-level
        // tests (which assume an instantly-ready drive) need not each tick it
        // out. The window itself is covered by the insert/ready tests.
        fds.insert_not_ready = 0;
    }

    #[test]
    fn timer_fires_at_zero() {
        let mut fds = make_device(1);
        enable_disk_io(&mut fds);
        // Reload = 10, enable, no repeat.
        fds.cpu_write(0x4020, 10);
        fds.cpu_write(0x4021, 0);
        fds.cpu_write(0x4022, 0x02); // enable, repeat off
        assert!(!fds.irq_pending());
        // Counter starts at 10; needs 10 cycles to reach 0.
        for _ in 0..9 {
            fds.notify_cpu_cycle();
            assert!(!fds.irq_pending(), "should not fire before reaching 0");
        }
        fds.notify_cpu_cycle();
        assert!(fds.irq_pending(), "timer IRQ must fire when counter hits 0");
    }

    #[test]
    fn timer_disabled_when_disk_io_off() {
        let mut fds = make_device(1);
        // Disk I/O NOT enabled: writing $4022 has no effect.
        fds.cpu_write(0x4020, 2);
        fds.cpu_write(0x4022, 0x02);
        for _ in 0..10 {
            fds.notify_cpu_cycle();
        }
        assert!(
            !fds.irq_pending(),
            "timer must not run with disk I/O disabled"
        );
    }

    #[test]
    fn timer_repeat_reloads() {
        let mut fds = make_device(1);
        enable_disk_io(&mut fds);
        fds.cpu_write(0x4020, 3);
        fds.cpu_write(0x4021, 0);
        fds.cpu_write(0x4022, 0x03); // enable + repeat
        for _ in 0..3 {
            fds.notify_cpu_cycle();
        }
        assert!(fds.irq_pending());
        // Ack via $4030 read.
        let _ = fds.cpu_read(0x4030);
        assert!(!fds.irq_pending());
        // Repeat reloaded the counter: fires again after 3 more cycles.
        for _ in 0..3 {
            fds.notify_cpu_cycle();
        }
        assert!(fds.irq_pending(), "repeat flag must reload and re-fire");
    }

    #[test]
    fn timer_no_repeat_stops() {
        let mut fds = make_device(1);
        enable_disk_io(&mut fds);
        fds.cpu_write(0x4020, 2);
        fds.cpu_write(0x4022, 0x02); // enable, no repeat
        for _ in 0..2 {
            fds.notify_cpu_cycle();
        }
        assert!(fds.irq_pending());
        let _ = fds.cpu_read(0x4030); // ack
        // No repeat: counter is disabled, no further IRQs.
        for _ in 0..20 {
            fds.notify_cpu_cycle();
        }
        assert!(!fds.irq_pending());
    }

    #[test]
    fn reload_zero_fires_immediately() {
        let mut fds = make_device(1);
        enable_disk_io(&mut fds);
        fds.cpu_write(0x4020, 0);
        fds.cpu_write(0x4021, 0);
        fds.cpu_write(0x4022, 0x02); // enable, reload 0
        fds.notify_cpu_cycle();
        assert!(fds.irq_pending(), "reload 0 fires on the next cycle");
    }

    #[test]
    fn read_4030_acks_timer_irq() {
        let mut fds = make_device(1);
        enable_disk_io(&mut fds);
        fds.cpu_write(0x4020, 1);
        fds.cpu_write(0x4022, 0x02);
        fds.notify_cpu_cycle();
        assert!(fds.irq_pending());
        let status = fds.cpu_read(0x4030);
        assert_eq!(status & 0x01, 0x01, "timer IRQ bit set in status");
        assert!(!fds.irq_pending(), "$4030 read acks timer IRQ");
        // Reading again shows the flag already cleared.
        assert_eq!(fds.cpu_read(0x4030) & 0x01, 0x00);
    }

    #[test]
    fn write_4022_disable_acks_timer_irq() {
        let mut fds = make_device(1);
        enable_disk_io(&mut fds);
        fds.cpu_write(0x4020, 1);
        fds.cpu_write(0x4022, 0x02);
        fds.notify_cpu_cycle();
        assert!(fds.irq_pending());
        fds.cpu_write(0x4022, 0x00); // disable
        assert!(!fds.irq_pending(), "$4022 disable acks timer IRQ");
    }

    #[test]
    fn clear_4023_stops_and_acks() {
        let mut fds = make_device(1);
        enable_disk_io(&mut fds);
        fds.cpu_write(0x4020, 1);
        fds.cpu_write(0x4022, 0x02);
        fds.notify_cpu_cycle();
        assert!(fds.irq_pending());
        fds.cpu_write(0x4023, 0x00); // clear disk I/O
        assert!(!fds.irq_pending(), "$4023 clear acks timer IRQ");
        // Timer is stopped: further cycles do nothing.
        fds.notify_cpu_cycle();
        assert!(!fds.irq_pending());
    }

    #[test]
    fn reload_value_writable_with_disk_io_off() {
        let mut fds = make_device(1);
        // $4020/$4021 are NOT gated on $4023.0 per nesdev.
        fds.cpu_write(0x4020, 0x34);
        fds.cpu_write(0x4021, 0x12);
        // Now enable disk I/O and the timer; the reload should be $1234.
        enable_disk_io(&mut fds);
        fds.cpu_write(0x4022, 0x02);
        // Counter was loaded with $1234; verify via debug_info.
        let info = fds.debug_info();
        let counter = info
            .irq_state
            .iter()
            .find(|(k, _)| k == "counter")
            .map(|(_, v)| v.clone())
            .unwrap();
        assert_eq!(counter, "0x1234");
    }

    // --- Registers ---

    #[test]
    fn control_sets_mirroring() {
        // $4025 bit 3 = nametable arrangement. Corrected to match the nesdev
        // FDS register table + real-BIOS boot behaviour: bit 3 = 0 is the
        // "Horizontal arrangement" the wiki names, which is VERTICAL mirroring in
        // this codebase's enum ($2000 aliases $2800), and bit 3 = 1 is the
        // "Vertical arrangement" == HORIZONTAL mirroring ($2000 and $2800 are
        // distinct banks). The old assertions had these swapped, which made the
        // BIOS licence-screen clear of $2000 wipe the $2800 approval tilemap and
        // fail every real-BIOS boot ("DISK TROUBLE ERR.20").
        let mut fds = make_device(1);
        // bit 3 = 0 -> Vertical mirroring ("Horizontal arrangement").
        fds.cpu_write(0x4025, 0b0010_0110); // motor stop, no transfer reset, read
        assert_eq!(fds.current_mirroring(), Mirroring::Vertical);
        // bit 3 = 1 -> Horizontal mirroring ("Vertical arrangement").
        fds.cpu_write(0x4025, 0b0010_1110);
        assert_eq!(fds.current_mirroring(), Mirroring::Horizontal);
    }

    #[test]
    fn drive_status_4032() {
        let mut fds = make_device(1);
        // Disk inserted, motor off -> not-ready set; default disk is writable.
        let v = fds.cpu_read(0x4032);
        assert_eq!(v & 0x01, 0x00, "disk inserted -> bit 0 clear");
        assert_eq!(v & 0x04, 0x00, "stage-2b disks are writable by default");
        // Mark read-only -> write-protect bit set.
        fds.set_disk_write_protected(true);
        assert_eq!(fds.cpu_read(0x4032) & 0x04, 0x04, "write-protect reflected");
        fds.set_disk_write_protected(false);
        // Start the motor + read mode + transfer: ready clears.
        enable_disk_io(&mut fds);
        fds.cpu_write(0x4025, 0b1110_0100); // motor on, no reset, read, IRQ-on-xfer
        settle_drive(&mut fds); // run out the motor spin-up window
        let v = fds.cpu_read(0x4032);
        assert_eq!(v & 0x02, 0x00, "motor on + spun-up -> ready");
    }

    #[test]
    fn ext_4033_battery_bit() {
        let mut fds = make_device(1);
        assert_eq!(fds.cpu_read(0x4033) & 0x80, 0x80, "battery good with disk");
    }

    // --- Disk read ---

    #[test]
    fn disk_read_consumes_bytes_in_order() {
        let mut fds = make_device(1);
        enable_disk_io(&mut fds);
        // reset off (bit0=0), motor on (bit1=0), read (bit2=1).
        fds.cpu_write(0x4025, 0b0110_0100);
        settle_drive(&mut fds);
        // The controller bit-shifts past the synthesized lead-in gap + $80 start
        // mark in hardware, so the first delivered byte is the block's first
        // byte (the disk-info block code $01), then the raw block bytes stream in
        // order — exactly what the BIOS load routine expects.
        let side: Vec<u8> = fds.disk.side(0)[..4].to_vec();
        for (i, expected) in side.iter().enumerate() {
            for _ in 0..DISK_BYTE_CYCLES {
                fds.notify_cpu_cycle();
            }
            assert_eq!(fds.cpu_read(0x4030) & 0x80, 0x80, "byte-transfer flag set");
            assert_eq!(
                fds.cpu_read(0x4031),
                *expected,
                "wire block byte {i} in order"
            );
        }
    }

    #[test]
    fn disk_read_irq_on_transfer() {
        let mut fds = make_device(1);
        enable_disk_io(&mut fds);
        // motor on, read, IRQ-on-transfer (bit7).
        fds.cpu_write(0x4025, 0b1110_0100);
        settle_drive(&mut fds);
        for _ in 0..DISK_BYTE_CYCLES {
            fds.notify_cpu_cycle();
        }
        assert!(fds.irq_pending(), "byte-transfer raises IRQ when bit7 set");
        // Reading $4031 acks the disk IRQ.
        let _ = fds.cpu_read(0x4031);
        assert!(!fds.irq_pending());
    }

    #[test]
    fn transfer_reset_rearms_gap_skip() {
        let mut fds = make_device(1);
        enable_disk_io(&mut fds);
        fds.cpu_write(0x4025, 0b0110_0100); // start reading
        settle_drive(&mut fds);
        for _ in 0..(DISK_BYTE_CYCLES * 3) {
            fds.notify_cpu_cycle();
        }
        assert!(fds.head > 0);
        // Asserting transfer reset (bit 0) re-arms the read gap-skip and resets
        // the byte-transfer timing, but does NOT rewind the head — the head
        // tracks the physical disk rotation (it only returns to the start at the
        // inner track / end of head). The BIOS toggles reset between blocks to
        // re-sync to each block's start mark while the head keeps advancing.
        let head_before = fds.head;
        fds.cpu_write(0x4025, 0b0110_0101); // CRC + read + reset asserted
        assert_eq!(
            fds.head, head_before,
            "transfer reset does not rewind the head"
        );
        assert!(fds.read_skipping_gap, "transfer reset re-arms the gap-skip");
    }

    // --- Save state ---

    #[test]
    fn save_state_round_trip() {
        let mut fds = make_device(2);
        enable_disk_io(&mut fds);
        fds.cpu_write(0x6000, 0xAB);
        fds.cpu_write(0x7FFF, 0xCD);
        fds.ppu_write(0x0100, 0xEF);
        fds.cpu_write(0x4020, 0x99);
        fds.cpu_write(0x4021, 0x01);
        fds.cpu_write(0x4022, 0x03);
        fds.cpu_write(0x4025, 0b1110_0100); // motor + read + irq-on-xfer
        settle_drive(&mut fds);
        for _ in 0..(DISK_BYTE_CYCLES + 5) {
            fds.notify_cpu_cycle();
        }
        let blob = fds.save_state();

        // Restore into a fresh device with the same disk + BIOS.
        let mut fresh = make_device(2);
        fresh.load_state(&blob).unwrap();
        assert_eq!(fresh.cpu_read(0x6000), 0xAB);
        assert_eq!(fresh.cpu_read(0x7FFF), 0xCD);
        assert_eq!(fresh.ppu_read(0x0100), 0xEF);
        assert_eq!(fresh.timer_reload, fds.timer_reload);
        assert_eq!(fresh.timer_counter, fds.timer_counter);
        assert_eq!(fresh.head, fds.head);
        assert_eq!(fresh.control, fds.control);
        assert_eq!(fresh.current_mirroring(), fds.current_mirroring());
        // Re-serialize: byte-identical.
        assert_eq!(fresh.save_state(), blob);
    }

    #[test]
    fn load_state_rejects_truncated() {
        let mut fds = make_device(1);
        assert!(matches!(
            fds.load_state(&[FDS_SAVE_VERSION, 0, 0]),
            Err(MapperError::Truncated { .. })
        ));
    }

    #[test]
    fn load_state_rejects_bad_version() {
        let mut fds = make_device(1);
        let mut blob = fds.save_state();
        blob[0] = 0xFF;
        assert!(matches!(
            fds.load_state(&blob),
            Err(MapperError::UnsupportedVersion(0xFF))
        ));
    }

    // --- FDS audio ($4040-$4097) ---

    /// Enable disk + sound I/O ($4023 = 0b11) so the sound registers function.
    fn enable_sound_io(fds: &mut Fds) {
        fds.cpu_write(0x4023, 0x03);
    }

    #[test]
    fn sound_io_gates_register_writes() {
        let mut fds = make_device(1);
        // Sound I/O disabled: writes to $4080 etc. are ignored.
        fds.cpu_write(0x4080, 0x9F); // direct gain 0x1F, would-be
        assert_eq!(fds.audio.vol_gain, 0, "no effect with sound I/O off");
        enable_sound_io(&mut fds);
        fds.cpu_write(0x4080, 0x9F); // M=1 (direct), gain = 0x1F
        assert_eq!(
            fds.audio.vol_gain, 0x1F,
            "direct gain set with sound I/O on"
        );
    }

    #[test]
    fn wavetable_write_gating() {
        let mut fds = make_device(1);
        enable_sound_io(&mut fds);
        // $4089 bit 7 set -> wave RAM is writable + channel held.
        fds.cpu_write(0x4089, 0x80);
        fds.cpu_write(0x4040, 0x3F);
        fds.cpu_write(0x4041, 0x2A);
        assert_eq!(fds.audio.wavetable[0], 0x3F);
        assert_eq!(fds.audio.wavetable[1], 0x2A);
        // While write-enabled, reads return the RAM byte.
        assert_eq!(fds.cpu_read(0x4040), 0x3F);
        // Disable write-enable; further $4040 writes are ignored.
        fds.cpu_write(0x4089, 0x00);
        fds.cpu_write(0x4040, 0x11);
        assert_eq!(fds.audio.wavetable[0], 0x3F, "RAM write-protected");
        // Wave-RAM read while protected returns the current output latch.
        fds.audio.wave_out_latch = 0x07;
        assert_eq!(fds.cpu_read(0x4040), 0x07);
    }

    #[test]
    #[cfg(feature = "mapper-audio")]
    fn wave_accumulator_steps_table_index() {
        let mut fds = make_device(1);
        enable_sound_io(&mut fds);
        // Fill wavetable with index-keyed values while write-enabled.
        fds.cpu_write(0x4089, 0x80);
        for i in 0..64u16 {
            fds.cpu_write(0x4040 + i, (i as u8) & 0x3F);
        }
        fds.cpu_write(0x4089, 0x00); // disable write -> channel runs.
        // Mod gain 0 (direct) -> unmodulated pitch add = P * 64 per wave tick.
        fds.cpu_write(0x4084, 0x80); // mod env disabled, gain 0
        fds.cpu_write(0x4085, 0x00); // mod counter 0
        // Wave pitch P = 0x400 (1024) -> add 1024*64 = 65536 per 16-cycle tick.
        fds.cpu_write(0x4082, 0x00);
        fds.cpu_write(0x4083, 0x04); // hi nibble 4 -> P = 0x400, not halted
        // After 4 wave ticks (64 CPU cycles), acc = 4*65536 = 2^18 -> index 1.
        for _ in 0..64 {
            fds.notify_cpu_cycle();
        }
        assert_eq!(fds.audio.wave_acc, 262_144);
        assert_eq!(fds.audio.wave_out_latch, 1, "index 1 -> wavetable[1] == 1");
        // $4096 reads the held wave value (bits 7-6 read as 01).
        assert_eq!(fds.cpu_read(0x4096), 0x40 | 1);
    }

    #[test]
    #[cfg(feature = "mapper-audio")]
    fn wave_halt_resets_accumulator_and_holds_4040() {
        let mut fds = make_device(1);
        enable_sound_io(&mut fds);
        fds.cpu_write(0x4089, 0x80);
        fds.cpu_write(0x4040, 0x21); // wavetable[0]
        fds.cpu_write(0x4089, 0x00);
        // Run a bit, then halt via $4083 bit 7.
        fds.cpu_write(0x4082, 0x00);
        fds.cpu_write(0x4083, 0x04);
        for _ in 0..32 {
            fds.notify_cpu_cycle();
        }
        assert!(fds.audio.wave_acc > 0);
        fds.cpu_write(0x4083, 0x84); // halt (bit 7), keep freq hi = 4
        assert_eq!(fds.audio.wave_acc, 0, "halt resets the wave accumulator");
        // While halted, the wave unit holds the $4040 value.
        fds.notify_cpu_cycle();
        // Drive a full prescaler tick so clock_wave runs once.
        for _ in 0..16 {
            fds.notify_cpu_cycle();
        }
        assert_eq!(fds.audio.wave_out_latch, 0x21);
    }

    #[test]
    fn volume_envelope_direct_mode() {
        let mut fds = make_device(1);
        enable_sound_io(&mut fds);
        // M=1 (disabled/direct), gain bits = 0x20 (32).
        fds.cpu_write(0x4080, 0x80 | 0x20);
        assert_eq!(fds.audio.vol_gain, 32);
        // $4090 reads back the gain with bits 7-6 = 01.
        assert_eq!(fds.cpu_read(0x4090), 0x40 | 32);
    }

    #[test]
    #[cfg(feature = "mapper-audio")]
    fn volume_envelope_auto_increase() {
        let mut fds = make_device(1);
        enable_sound_io(&mut fds);
        // Master env speed small so the period is short and deterministic.
        // $408A = 0 disables envelopes, so use 0 multiplier+1 via $408A=0?
        // Period c = 8 * (e+1) * (m+1); pick e=0, m=0 -> c = 8.
        fds.cpu_write(0x408A, 0x00); // m = 0 -> wait: 0 disables. Use m via... set below.
        // m must be non-zero to enable; use $408A = 1 -> m=1, c = 8*1*2 = 16.
        fds.cpu_write(0x408A, 0x01);
        // Volume envelope ON (M=0), direction increase (D=1), speed e=0.
        fds.cpu_write(0x4080, 0x40);
        assert_eq!(fds.audio.vol_gain, 0, "starts at 0");
        // c = 8 * (0+1) * (1+1) = 16 CPU cycles per envelope tick.
        // The $4080 write reset the timer to 16; one tick after 16 cycles.
        for _ in 0..16 {
            fds.notify_cpu_cycle();
        }
        assert_eq!(fds.audio.vol_gain, 1, "auto-increase ticks gain up by 1");
        for _ in 0..16 {
            fds.notify_cpu_cycle();
        }
        assert_eq!(fds.audio.vol_gain, 2);
    }

    #[test]
    #[cfg(feature = "mapper-audio")]
    fn volume_envelope_caps_at_32_on_increase() {
        let mut fds = make_device(1);
        enable_sound_io(&mut fds);
        fds.cpu_write(0x408A, 0x01);
        fds.cpu_write(0x4080, 0x40); // ON, increase, speed 0
        // Drive many ticks; gain must saturate at 32.
        for _ in 0..(16 * 64) {
            fds.notify_cpu_cycle();
        }
        assert_eq!(fds.audio.vol_gain, 32, "auto-increase clamps at 32");
    }

    #[test]
    #[cfg(feature = "mapper-audio")]
    fn mod_table_push_and_counter_step() {
        let mut fds = make_device(1);
        enable_sound_io(&mut fds);
        // Mod unit halted ($4087 bit 7) so $4088 pushes table entries.
        fds.cpu_write(0x4087, 0x80);
        // Push 32 entries: entry value 1 (=> +1 step) everywhere.
        for _ in 0..32 {
            fds.cpu_write(0x4088, 0x01);
        }
        assert!(fds.audio.mod_table.iter().all(|&e| e == 1));
        // Writing 32 entries wraps the write position back to start.
        assert_eq!(fds.audio.mod_write_pos, 0);
        // Direct-test the 3-bit step decode against the nesdev table.
        fds.audio.mod_counter = 0;
        fds.audio.step_mod_counter(3); // +4
        assert_eq!(fds.audio.mod_counter, 4);
        fds.audio.step_mod_counter(5); // -4
        assert_eq!(fds.audio.mod_counter, 0);
        fds.audio.step_mod_counter(4); // reset
        assert_eq!(fds.audio.mod_counter, 0);
        fds.audio.step_mod_counter(7); // -1
        assert_eq!(fds.audio.mod_counter, -1);
    }

    #[test]
    #[cfg(feature = "mapper-audio")]
    #[allow(clippy::field_reassign_with_default)]
    fn mod_pitch_bias_formula() {
        // Worked values from the nesdev FDS_audio "Modulation unit" pseudo-code.
        let mut a = FdsAudio::default();
        // counter = 0, gain = 0: temp=0 -> +0x400 -> >>4 = 0x40 -> *pitch.
        a.mod_counter = 0;
        a.mod_gain = 0;
        a.wave_pitch = 0x100;
        // temp = 0x40 (64); wave_pitch = 0x100 * 64 = 0x4000.
        assert_eq!(a.modulated_pitch(), 0x4000);
        // counter = 1, gain = 16: temp = 16 = 0x10. (0x10 & 0x0F)==0 -> no round.
        // temp += 0x400 = 0x410; >>4 = 0x41; & 0xFF = 0x41 (65).
        // wave_pitch = 0x100 * 0x41 = 0x4100.
        a.mod_counter = 1;
        a.mod_gain = 16;
        assert_eq!(a.modulated_pitch(), 0x100 * 0x41);
        // counter = 1, gain = 1: temp = 1. (1 & 0x0F)!=0 && !(1 & 0x800) -> +0x20
        // temp = 0x21; +0x400 = 0x421; >>4 = 0x42; *pitch.
        a.mod_counter = 1;
        a.mod_gain = 1;
        assert_eq!(a.modulated_pitch(), 0x100 * 0x42);
        // Negative counter: counter = -1, gain = 16 -> temp = -16 = 0xFFFFFFF0.
        // (temp & 0x0F)==0 so no rounding; temp += 0x400 -> 0x3F0; >>4 = 0x3F.
        a.mod_counter = -1;
        a.mod_gain = 16;
        assert_eq!(a.modulated_pitch(), 0x100 * 0x3F);
    }

    #[test]
    #[cfg(feature = "mapper-audio")]
    fn master_volume_scaling() {
        let mut fds = make_device(1);
        enable_sound_io(&mut fds);
        // Direct full gain (32), full master (0 -> 2/2).
        fds.cpu_write(0x4080, 0x80 | 0x20);
        fds.cpu_write(0x4089, 0x00); // master volume = 0 (full)
        // Force a known wave sample at the output.
        fds.audio.wave_out_latch = 63;
        let full = fds.mix_audio();
        // master volume = 3 (2/5) should be quieter than full (2/2).
        fds.cpu_write(0x4089, 0x03);
        let quiet = fds.mix_audio();
        assert!(
            quiet.unsigned_abs() < full.unsigned_abs(),
            "master volume 2/5 ({quiet}) is quieter than full ({full})"
        );
        // Centered output is positive for a top-of-range sample (63 > midpoint).
        assert!(full > 0);
        // A mid sample (32) sits near the DC midpoint -> near-zero output.
        fds.cpu_write(0x4089, 0x00);
        fds.audio.wave_out_latch = 32;
        assert_eq!(fds.mix_audio(), 0, "sample 32 is the centered midpoint");
    }

    #[test]
    fn mod_counter_read_4097() {
        let mut fds = make_device(1);
        enable_sound_io(&mut fds);
        fds.cpu_write(0x4085, 0x7F); // -1 (sign-extended 7-bit)
        assert_eq!(fds.audio.mod_counter, -1);
        // $4097 reads the 7-bit counter in bits 6-0; bit 7 reads 0.
        assert_eq!(fds.cpu_read(0x4097), 0x7F);
        fds.cpu_write(0x4085, 0x10); // +16
        assert_eq!(fds.audio.mod_counter, 16);
        assert_eq!(fds.cpu_read(0x4097), 0x10);
    }

    #[test]
    fn audio_save_state_round_trip() {
        let mut fds = make_device(2);
        enable_sound_io(&mut fds);
        // Exercise the full audio surface.
        fds.cpu_write(0x4089, 0x80);
        for i in 0..64u16 {
            fds.cpu_write(0x4040 + i, ((i * 3) as u8) & 0x3F);
        }
        fds.cpu_write(0x4089, 0x01); // write-disable + master volume 1
        fds.cpu_write(0x4080, 0x80 | 0x18); // direct gain 0x18
        fds.cpu_write(0x4082, 0x34);
        fds.cpu_write(0x4083, 0x05);
        fds.cpu_write(0x4087, 0x80); // halt mod -> table writable
        for k in 0..32 {
            fds.cpu_write(0x4088, (k & 0x07) as u8);
        }
        fds.cpu_write(0x4086, 0x21);
        fds.cpu_write(0x4084, 0x80 | 0x07); // direct mod gain 7
        fds.cpu_write(0x4085, 0x05);
        fds.cpu_write(0x408A, 0x40);
        for _ in 0..100 {
            fds.notify_cpu_cycle();
        }
        let blob = fds.save_state();
        assert_eq!(blob[0], 3, "FDS save version is 3 (Stage 2b)");

        let mut fresh = make_device(2);
        fresh.load_state(&blob).unwrap();
        assert_eq!(fresh.audio.wavetable, fds.audio.wavetable);
        assert_eq!(fresh.audio.mod_table, fds.audio.mod_table);
        assert_eq!(fresh.audio.wave_acc, fds.audio.wave_acc);
        assert_eq!(fresh.audio.mod_acc, fds.audio.mod_acc);
        assert_eq!(fresh.audio.wave_pitch, fds.audio.wave_pitch);
        assert_eq!(fresh.audio.mod_pitch, fds.audio.mod_pitch);
        assert_eq!(fresh.audio.vol_gain, fds.audio.vol_gain);
        assert_eq!(fresh.audio.mod_gain, fds.audio.mod_gain);
        assert_eq!(fresh.audio.master_volume, fds.audio.master_volume);
        assert_eq!(fresh.audio.env_speed_mult, fds.audio.env_speed_mult);
        assert_eq!(fresh.audio.cycle_prescaler, fds.audio.cycle_prescaler);
        // Re-serialize: byte-identical.
        assert_eq!(fresh.save_state(), blob);
    }

    #[test]
    #[cfg(not(feature = "mapper-audio"))]
    fn mix_audio_silent_without_feature() {
        // With `mapper-audio` off, the FDS device must be silent regardless of
        // any sound-register programming (byte-identical floor unchanged).
        let mut fds = make_device(1);
        enable_sound_io(&mut fds);
        fds.cpu_write(0x4089, 0x80);
        for i in 0..64u16 {
            fds.cpu_write(0x4040 + i, (i as u8) & 0x3F);
        }
        fds.cpu_write(0x4089, 0x00);
        fds.cpu_write(0x4080, 0x80 | 0x20); // full direct gain
        fds.cpu_write(0x4082, 0xFF);
        fds.cpu_write(0x4083, 0x0F);
        for _ in 0..1000 {
            fds.notify_cpu_cycle();
        }
        // Goes through the trait default (returns 0) since `mix_audio` is gated.
        assert_eq!(Mapper::mix_audio(&mut fds), 0);
    }

    #[test]
    fn load_state_v1_blob_defaults_audio() {
        // A v1 (Stage-1) blob has neither the audio tail nor the disk tail;
        // loading must succeed and leave the sound channel at its default
        // (silent) state.
        let mut fds = make_device(1);
        enable_sound_io(&mut fds);
        // Build a v1-shaped blob by truncating off the disk + audio tails and
        // stamping version 1.
        let disk_tail = 4 + FDS_SIDE_LEN + 4 + 4 + 1;
        let mut blob = fds.save_state();
        blob.truncate(blob.len() - disk_tail - FdsAudio::TAIL_LEN);
        blob[0] = 1;
        // Dirty the audio so we can see the default reset.
        fds.audio.vol_gain = 31;
        fds.audio.wave_pitch = 0x123;
        fds.load_state(&blob).unwrap();
        assert_eq!(fds.audio.vol_gain, 0);
        assert_eq!(fds.audio.wave_pitch, 0);
        assert_eq!(fds.audio.env_speed_mult, 0xE8, "default power-on $408A");
    }

    // --- Stage 2b: disk write path / eject-insert / persistence ---

    /// Drive the device into write mode and store `bytes` over consecutive
    /// byte-transfer ticks (one byte per `$4024` write + `DISK_BYTE_CYCLES`).
    fn write_bytes(fds: &mut Fds, bytes: &[u8]) {
        // reset off (bit0=0), motor on (bit1=0), WRITE mode (bit2=0), CRC
        // enable (bit6=1) + bit5 (required for the byte-transfer flag).
        fds.cpu_write(0x4025, 0b0110_0000);
        // Clear the motor-on spin-up not-ready window so the transfer engine is
        // active immediately (the spin-up handshake is covered by its own tests).
        fds.insert_not_ready = 0;
        fds.update_transfer_state();
        for &b in bytes {
            fds.cpu_write(0x4024, b); // load the next write byte
            for _ in 0..DISK_BYTE_CYCLES {
                fds.notify_cpu_cycle();
            }
        }
    }

    /// Wire offset of the first block's payload byte for a side whose first
    /// block is preceded only by the lead-in gap + start mark (the synth disks
    /// here always open with the disk-info block).
    const FIRST_BLOCK_WIRE_PAYLOAD: usize = WIRE_LEAD_IN_GAP + 1;

    /// Park the head directly at a wire offset (bypassing the read cadence) so a
    /// write/read test lands inside a block payload region. `head` is a wire
    /// offset since the v2.6.0 wire-format synthesis.
    fn seek_head(fds: &mut Fds, wire_off: usize) {
        fds.head = wire_off;
    }

    /// Close the motor-on spin-up not-ready window and re-evaluate the transfer
    /// state, so a transfer-level test runs immediately after a motor-on `$4025`
    /// write (the spin-up handshake itself is covered by the ready/insert tests).
    fn settle_drive(fds: &mut Fds) {
        fds.insert_not_ready = 0;
        fds.update_transfer_state();
    }

    #[test]
    fn write_mode_round_trips_through_read() {
        let mut fds = make_device(1);
        enable_disk_io(&mut fds);
        // Park the head inside the disk-info block payload (past the synthesized
        // lead-in gap + $80 start mark) so the writes mirror into the raw side.
        seek_head(&mut fds, FIRST_BLOCK_WIRE_PAYLOAD);
        let payload = [0x11u8, 0x22, 0x33, 0x44, 0x55];
        write_bytes(&mut fds, &payload);
        // The bytes landed at the start of the disk-info block in the raw side
        // (wire payload start maps back to raw offset 0).
        assert_eq!(&fds.disk.side(0)[..5], &payload);
        // Rewind to the same payload offset and read them back in order through
        // the wire image.
        seek_head(&mut fds, FIRST_BLOCK_WIRE_PAYLOAD);
        fds.cpu_write(0x4025, 0b0110_0100); // motor on, read mode
        for expected in payload {
            for _ in 0..DISK_BYTE_CYCLES {
                fds.notify_cpu_cycle();
            }
            assert_eq!(fds.cpu_read(0x4031), expected, "read back written byte");
        }
    }

    #[test]
    fn write_sets_and_clears_dirty_flag() {
        let mut fds = make_device(1);
        enable_disk_io(&mut fds);
        assert!(!fds.disk_is_dirty(), "clean at construction");
        write_bytes(&mut fds, &[0xAB]);
        assert!(fds.disk_is_dirty(), "a write marks the image dirty");
        fds.clear_disk_dirty();
        assert!(!fds.disk_is_dirty(), "clear_disk_dirty resets it");
    }

    #[test]
    fn disk_image_bytes_reflect_writes() {
        let mut fds = make_device(1);
        enable_disk_io(&mut fds);
        // Park the head inside the disk-info block payload, past the 16-byte
        // signature (raw offset 16), so the written bytes don't clobber it
        // (keeping the image re-parseable below). The signature is at the start
        // of the block payload, so raw offset 16 == wire payload start + 16.
        seek_head(&mut fds, FIRST_BLOCK_WIRE_PAYLOAD + 16);
        // Switch to write mode (head stays put) and store a payload.
        write_bytes(&mut fds, &[0xDE, 0xAD, 0xBE, 0xEF]);
        let bytes = fds.disk_image_bytes();
        assert_eq!(bytes.len(), FDS_SIDE_LEN, "single side re-serialized");
        assert_eq!(&bytes[16..20], &[0xDE, 0xAD, 0xBE, 0xEF]);
        // The disk-info signature is intact, so the image re-parses to an equal
        // disk that reflects the writes.
        let reparsed = parse_fds(&bytes).unwrap();
        assert_eq!(&reparsed.side(0)[16..20], &[0xDE, 0xAD, 0xBE, 0xEF]);
    }

    #[test]
    fn write_protected_disk_does_not_modify_medium() {
        let mut fds = make_device(1);
        enable_disk_io(&mut fds);
        fds.set_disk_write_protected(true);
        let before: Vec<u8> = fds.disk.side(0)[..4].to_vec();
        write_bytes(&mut fds, &[0xFF, 0xFF, 0xFF, 0xFF]);
        assert_eq!(&fds.disk.side(0)[..4], &before[..], "medium unchanged");
        assert!(!fds.disk_is_dirty(), "write-protected write is not dirty");
        // $4032 bit 2 reflects the protect flag.
        assert_eq!(fds.cpu_read(0x4032) & 0x04, 0x04);
    }

    #[test]
    fn eject_sets_not_inserted_and_reads_not_ready() {
        let mut fds = make_device(1);
        enable_disk_io(&mut fds);
        fds.set_disk_side(None); // eject
        assert_eq!(fds.inserted_disk_side(), None);
        let v = fds.cpu_read(0x4032);
        assert_eq!(v & 0x01, 0x01, "ejected -> disk-not-inserted bit set");
        assert_eq!(v & 0x02, 0x02, "ejected -> not-ready bit set");
        // A read transfer started while ejected delivers no real disk bytes.
        fds.cpu_write(0x4025, 0b0110_0100); // motor on, read
        for _ in 0..DISK_BYTE_CYCLES {
            fds.notify_cpu_cycle();
        }
        // The transfer is Idle (no insert), so no byte-transfer flag set.
        assert_eq!(
            fds.cpu_read(0x4030) & 0x80,
            0x00,
            "no transfer while ejected"
        );
    }

    #[test]
    fn insert_side_reads_from_that_side() {
        let mut fds = make_device(2);
        enable_disk_io(&mut fds);
        // Insert side 1 (its data byte at offset 16 is 1, per synth_fwnes).
        fds.set_disk_side(Some(1));
        assert_eq!(fds.inserted_disk_side(), Some(1));
        // Wait out the post-insert not-ready window before reading.
        for _ in 0..INSERT_NOT_READY_CYCLES {
            fds.notify_cpu_cycle();
        }
        assert_eq!(
            fds.cpu_read(0x4032) & 0x02,
            0x02,
            "motor off -> still not ready"
        );
        fds.cpu_write(0x4025, 0b0110_0100); // motor on, read mode
        settle_drive(&mut fds); // run out the motor spin-up window
        // Park at side 1's first block payload (past the lead-in gap + $80
        // mark); the first block byte is 0x01 (the disk-info block code).
        seek_head(&mut fds, FIRST_BLOCK_WIRE_PAYLOAD);
        for _ in 0..DISK_BYTE_CYCLES {
            fds.notify_cpu_cycle();
        }
        assert_eq!(
            fds.cpu_read(0x4031),
            0x01,
            "reads side 1's disk-info block code"
        );
    }

    #[test]
    fn swap_then_gap_skip_delivers_first_block_byte() {
        // Regression for the Kid Icarus side-B ERR.07 stall (T-101-002): after a
        // real disk swap the BIOS relies on the controller's gap-skip to re-sync
        // to the first block — it does NOT manually seek. This mirrors
        // `insert_side_reads_from_that_side` but WITHOUT the manual `seek_head`,
        // so the read engine must skip the lead-in gap + $80 mark from head 0 and
        // deliver side 1's disk-info block code (0x01) as the first byte.
        let mut fds = make_device(2);
        enable_disk_io(&mut fds);
        fds.set_disk_side(Some(1));
        for _ in 0..INSERT_NOT_READY_CYCLES {
            fds.notify_cpu_cycle();
        }
        fds.cpu_write(0x4025, 0b0110_0100); // motor on, read mode
        settle_drive(&mut fds); // run out the motor spin-up window
        // No seek: the gap-skip must re-sync from head 0 to the first block.
        for _ in 0..DISK_BYTE_CYCLES {
            fds.notify_cpu_cycle();
        }
        assert_eq!(
            fds.cpu_read(0x4031),
            0x01,
            "gap-skip after swap must deliver side 1's disk-info block code"
        );
    }

    #[test]
    fn multi_side_swap_isolates_writes() {
        let mut fds = make_device(2);
        enable_disk_io(&mut fds);
        // Write into side 0's disk-info block payload.
        seek_head(&mut fds, FIRST_BLOCK_WIRE_PAYLOAD);
        write_bytes(&mut fds, &[0xA0, 0xA1]);
        assert_eq!(&fds.disk.side(0)[..2], &[0xA0, 0xA1]);
        // Swap to side 1, wait ready, write different bytes into its block.
        fds.set_disk_side(Some(1));
        for _ in 0..INSERT_NOT_READY_CYCLES {
            fds.notify_cpu_cycle();
        }
        seek_head(&mut fds, FIRST_BLOCK_WIRE_PAYLOAD);
        write_bytes(&mut fds, &[0xB0, 0xB1]);
        assert_eq!(&fds.disk.side(1)[..2], &[0xB0, 0xB1]);
        // Side 0 is untouched by the side-1 writes.
        assert_eq!(&fds.disk.side(0)[..2], &[0xA0, 0xA1]);
        assert_eq!(fds.disk_side_count(), 2);
    }

    #[test]
    fn insert_opens_not_ready_window() {
        let mut fds = make_device(2);
        enable_disk_io(&mut fds);
        fds.cpu_write(0x4025, 0b0110_0100); // motor on, read
        fds.set_disk_side(Some(1)); // insert -> not-ready window opens
        // Immediately after insert: not-ready set even though motor is on.
        assert_eq!(
            fds.cpu_read(0x4032) & 0x02,
            0x02,
            "not ready right after insert"
        );
        // Run out the window; with the motor on, ready clears.
        for _ in 0..INSERT_NOT_READY_CYCLES {
            fds.notify_cpu_cycle();
        }
        assert_eq!(
            fds.cpu_read(0x4032) & 0x02,
            0x00,
            "ready after window + motor on"
        );
    }

    // --- FDS-proper (v1.6.0 Workstream F): CRC quirk table + timed head ---

    #[test]
    fn crc32_matches_known_vectors() {
        // The standard CRC-32/ISO-HDLC check values.
        assert_eq!(fds_crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(fds_crc32(b""), 0x0000_0000);
        assert_eq!(fds_crc32(b"a"), 0xE8B7_BE43);
    }

    #[test]
    fn quirk_lookup_returns_none_for_unknown_disk() {
        // A synthetic disk's CRC is not in the (real-dump-keyed) table.
        let fds = make_device(1);
        assert_eq!(fds.quirk(), FdsQuirk::NONE);
        // The table ships empty (entries are maintainer-measured from real
        // dumps only), so every CRC — synthetic or arbitrary — resolves to NONE
        // and no disk receives unverified timing slack.
        assert_eq!(quirk_for_crc(0xDEAD_BEEF), FdsQuirk::NONE);
        assert_eq!(quirk_for_crc(0x9CC9_C8A0), FdsQuirk::NONE);
        assert_eq!(quirk_for_crc(0x0000_0000), FdsQuirk::NONE);
    }

    #[test]
    fn quirk_keys_off_headerless_disk_crc() {
        // The quirk CRC is taken over the headerless side bytes, so a fwNES
        // dump and a headerless dump of the same disk resolve identically.
        let fwnes = parse_fds(&synth_fwnes(1)).unwrap();
        let headerless_bytes = fwnes.to_bytes();
        let crc = fds_crc32(&headerless_bytes);
        assert_eq!(
            quirk_for_crc(crc),
            quirk_for_crc(fds_crc32(&headerless_bytes))
        );
        // And it matches what the device computed at construction.
        let fds = Fds::new(fwnes, &dummy_bios()).unwrap();
        assert_eq!(fds.quirk(), quirk_for_crc(crc));
    }

    #[test]
    fn motor_restart_after_rewind_opens_reseek_window() {
        // Timed disk-head position: a motor restart after the cold spin-up,
        // with the head rewound to the disk start, must re-open a not-ready
        // window while the head re-seeks (rather than reading instantly).
        let mut fds = make_device(1);
        enable_disk_io(&mut fds);
        // Cold spin-up: first motor-on opens the long spin-up window.
        fds.cpu_write(0x4025, 0b0110_0100); // motor on, read mode
        assert!(fds.insert_not_ready > 0, "cold spin-up window open");
        // Run it out -> ready.
        for _ in 0..MOTOR_SPIN_UP_CYCLES {
            fds.notify_cpu_cycle();
        }
        assert_eq!(fds.cpu_read(0x4032) & 0x02, 0x00, "ready after spin-up");
        // Motor stop rewinds the head to the disk start.
        fds.cpu_write(0x4025, 0b0110_0110); // motor off (bit1 set)
        assert_eq!(fds.head, 0, "motor-off rewinds head to start");
        // Motor restart: the head must re-seek, so not-ready re-opens for the
        // re-seek window (no full cold spin-up — the disk is still turning).
        fds.cpu_write(0x4025, 0b0110_0100); // motor on
        assert_eq!(
            fds.insert_not_ready, HEAD_RESEEK_CYCLES,
            "re-seek window opens on motor-restart rewind"
        );
        assert!(
            fds.insert_not_ready < MOTOR_SPIN_UP_CYCLES,
            "re-seek is shorter than the cold spin-up"
        );
        assert_eq!(
            fds.cpu_read(0x4032) & 0x02,
            0x02,
            "not-ready during the re-seek"
        );
        for _ in 0..HEAD_RESEEK_CYCLES {
            fds.notify_cpu_cycle();
        }
        assert_eq!(
            fds.cpu_read(0x4032) & 0x02,
            0x00,
            "ready -> the BIOS re-read loop observes the not-ready->ready edge"
        );
    }

    #[test]
    fn mid_read_motor_toggle_does_not_re_seek() {
        // A motor toggle while the head is NOT at the disk start (the between-
        // blocks toggles the BIOS does mid-read) must NOT open a re-seek window:
        // the disk never stopped spinning, so streaming continues seamlessly.
        let mut fds = make_device(1);
        enable_disk_io(&mut fds);
        fds.cpu_write(0x4025, 0b0110_0100); // motor on
        settle_drive(&mut fds); // past cold spin-up
        // Advance the head off the disk start (simulate a partial read).
        seek_head(&mut fds, FIRST_BLOCK_WIRE_PAYLOAD + 8);
        // The BIOS does NOT toggle the motor off here (that would rewind); a
        // re-arm via transfer-reset keeps the head where it is. Toggle the motor
        // off then on WITHOUT a rewind by forcing the head back after the off
        // edge to model "disk kept spinning past the start".
        fds.cpu_write(0x4025, 0b0110_0110); // motor off (rewinds head to 0)
        seek_head(&mut fds, FIRST_BLOCK_WIRE_PAYLOAD + 8); // head not at start
        fds.insert_not_ready = 0;
        fds.cpu_write(0x4025, 0b0110_0100); // motor on, head != 0
        assert_eq!(
            fds.insert_not_ready, 0,
            "no re-seek window when the head is not at the disk start"
        );
    }

    #[test]
    fn quirk_is_not_part_of_save_state() {
        // The quirk is derived from immutable construction inputs, so it is not
        // serialized; a restored device recomputes it from its own disk.
        let mut fds = make_device(1);
        enable_disk_io(&mut fds);
        let blob = fds.save_state();
        let mut fresh = make_device(1);
        fresh.load_state(&blob).unwrap();
        assert_eq!(fresh.quirk(), fds.quirk());
    }

    #[test]
    fn save_state_v3_round_trips_mid_write_dirty_disk() {
        let mut fds = make_device(2);
        enable_disk_io(&mut fds);
        // Partially write inside side 0's first block then capture mid-write
        // (transfer still active). Park the head 16 bytes into the disk-info
        // block payload (raw offset 16) so the written byte mirrors there
        // without clobbering the block-code byte (which would break the wire
        // re-parse on restore).
        fds.cpu_write(0x4025, 0b0110_0000); // motor on, write mode
        settle_drive(&mut fds); // run out the motor spin-up window
        seek_head(&mut fds, FIRST_BLOCK_WIRE_PAYLOAD + 16);
        fds.cpu_write(0x4024, 0x77);
        for _ in 0..DISK_BYTE_CYCLES {
            fds.notify_cpu_cycle();
        }
        fds.cpu_write(0x4024, 0x88); // queued for the next tick
        for _ in 0..(DISK_BYTE_CYCLES / 2) {
            fds.notify_cpu_cycle(); // mid-cadence: transfer_timer partway
        }
        assert!(fds.disk_is_dirty());
        assert_eq!(fds.transfer, TransferState::Writing);
        let blob = fds.save_state();
        assert_eq!(blob[0], 3, "FDS save version bumped to 3");

        let mut fresh = make_device(2);
        fresh.load_state(&blob).unwrap();
        assert_eq!(fresh.disk.side(0)[16], 0x77, "written byte restored");
        assert!(fresh.disk_is_dirty(), "dirty flag restored");
        assert_eq!(
            fresh.transfer,
            TransferState::Writing,
            "write phase restored"
        );
        assert_eq!(fresh.write_data, 0x88);
        assert_eq!(fresh.head, fds.head);
        assert_eq!(fresh.transfer_timer, fds.transfer_timer);
        assert_eq!(fresh.inserted_disk_side(), fds.inserted_disk_side());
        // Byte-identical re-serialization.
        assert_eq!(fresh.save_state(), blob);
    }

    #[test]
    fn save_state_v3_round_trips_ejected() {
        let mut fds = make_device(2);
        enable_disk_io(&mut fds);
        fds.set_disk_side(None); // eject
        let blob = fds.save_state();
        let mut fresh = make_device(2);
        fresh.load_state(&blob).unwrap();
        assert_eq!(fresh.inserted_disk_side(), None, "ejected state restored");
        assert_eq!(fresh.save_state(), blob);
    }

    #[test]
    fn save_state_v3_round_trips_write_protect() {
        let mut fds = make_device(1);
        fds.set_disk_write_protected(true);
        let blob = fds.save_state();
        let mut fresh = make_device(1);
        fresh.load_state(&blob).unwrap();
        assert_eq!(
            fresh.cpu_read(0x4032) & 0x04,
            0x04,
            "write-protect restored"
        );
    }

    #[test]
    fn load_state_v2_blob_defaults_disk_clean() {
        // A v2 (Stage-1/2a) blob has no disk tail; loading must succeed, leave
        // the disk un-modified, side 0 inserted, clean, writable.
        let mut fds = make_device(2);
        enable_disk_io(&mut fds);
        // Build a v2-shaped blob by truncating off the v3 disk tail + stamping 2.
        // The disk tail = 4 (side count) + sides*FDS_SIDE_LEN + 4 + 4 + 1.
        let disk_tail = 4 + 2 * FDS_SIDE_LEN + 4 + 4 + 1;
        let mut blob = fds.save_state();
        blob.truncate(blob.len() - disk_tail);
        blob[0] = 2;
        // Dirty + eject + protect so we can see the v2 default reset.
        fds.set_disk_side(None);
        fds.set_disk_write_protected(true);
        write_bytes(&mut fds, &[0x01]); // protected: not dirty, but flip state anyway
        fds.load_state(&blob).unwrap();
        assert_eq!(fds.inserted_disk_side(), Some(0), "v2 defaults to side 0");
        assert!(!fds.disk_is_dirty(), "v2 defaults to clean");
        assert_eq!(fds.cpu_read(0x4032) & 0x04, 0x00, "v2 defaults to writable");
    }

    #[test]
    fn load_state_v1_blob_defaults_disk_clean() {
        // A v1 blob has neither audio nor disk tail; the disk defaults apply.
        let mut fds = make_device(1);
        let blob_v3 = fds.save_state();
        let disk_tail = 4 + FDS_SIDE_LEN + 4 + 4 + 1;
        let mut blob = blob_v3;
        blob.truncate(blob.len() - FdsAudio::TAIL_LEN - disk_tail);
        blob[0] = 1;
        fds.set_disk_write_protected(true);
        fds.load_state(&blob).unwrap();
        assert_eq!(fds.inserted_disk_side(), Some(0));
        assert!(!fds.disk_is_dirty());
        assert_eq!(fds.cpu_read(0x4032) & 0x04, 0x00, "v1 defaults to writable");
    }
}
