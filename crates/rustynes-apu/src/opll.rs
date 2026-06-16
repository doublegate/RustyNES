//! Yamaha YM2413 (OPLL) FM synthesizer — pure-Rust port of
//! [`emu2413 v1.5.9`](https://github.com/digital-sound-antiques/emu2413)
//! (MIT, Mitsutaka Okazaki) for the VRC7 mapper.
//!
//! # Status (v1.1.0 sprint 1.1 — scaffolding)
//!
//! This file lands the **foundation** of the OPLL port. The constants,
//! patch ROM tables (YM2413 / VRC7 / YMF281B), exp/sin lookup tables,
//! data structures, and public API surface are in place. `calc()` is
//! a stub returning 0 — matching the v1.0.0 ADR-0004 deferred behavior.
//!
//! The phase generator (PG), envelope generator (EG), per-operator
//! arithmetic, channel update loop, and AM/PM LFO are the next sprints
//! (1.1 PG+EG, 1.2 operator+channel+LFO). Each sub-step lands under a
//! per-fix unit test against emu2413 reference outputs.
//!
//! # Algorithmic reference
//!
//! - `/home/parobek/Code/OSS_Public-Projects/RustyNES/ref-proj/Mesen2/Core/Shared/Utilities/emu2413.{h,cpp}`
//!   — the canonical C source (Mesen2 vendors it verbatim from upstream)
//! - nesdev wiki `VRC7_audio.md` — register surface + chip-level behaviour
//! - nesdev wiki `User_Ben_Boldt_YM2413_Patches.md` — patch ROM analysis
//!
//! # License posture
//!
//! emu2413 is MIT-licensed at upstream; this Rust port is a clean-room
//! reimplementation guided by the C source's algorithm. We preserve
//! the upstream MIT notice in `NOTICE` at the repo root (see ADR-0005).
//!
//! # Determinism
//!
//! The OPLL is fully deterministic: identical input register-write
//! sequences produce bit-identical sample streams. The output is
//! `i16` in the `[-4095, 4095]` range (15-bit signed magnitude per
//! the chip's DAC).

// The OPLL port lands in stages: the v1.1.0-rc scaffolding (this file
// as committed) ships the patch ROM, LUTs, and public API but stubs
// `calc()` to 0 — matching ADR-0004's deferred behavior. The PG/EG/
// operator/LFO DSP fields are referenced by name in subsequent sprints;
// silencing the dead-code lint here keeps the v1.0.0 quality-gate
// `-D warnings` invariant green during the foundation landing.
#![allow(dead_code)]
// The PG/EG ports mirror C semantics (unsigned/signed wrap, narrowing
// casts) byte-for-byte against emu2413.cpp. The arithmetic is bounded
// by the chip's documented register widths; clippy's pedantic cast
// lints flag intentional truncations that match the upstream behavior
// and the reference test outputs.  Allowed at module level rather
// than salting every cast site.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]

extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// Constants — match emu2413.cpp lines 108-134
// ---------------------------------------------------------------------------

/// Phase increment counter width.
const DP_BITS: u32 = 19;
/// Full DP counter range = `1 << DP_BITS`.
const DP_WIDTH: u32 = 1u32 << DP_BITS;
/// Phase generator output bits (1024-length sine table = 2^10).
const PG_BITS: u32 = 10;
/// Phase generator table width.
const PG_WIDTH: usize = 1 << PG_BITS;
/// Number of address bits between DP and PG counters.
const DP_BASE_BITS: u32 = DP_BITS - PG_BITS;

/// Envelope output bits.
const EG_BITS: u32 = 7;
/// Envelope mute level.
const EG_MUTE: u32 = (1 << EG_BITS) - 1;
/// Envelope max level (mute - 4).
const EG_MAX: u32 = EG_MUTE - 4;

/// Total-level bits.
const TL_BITS: u32 = 6;

/// Damper rate (before key-on; key-scale affects this).
const DAMPER_RATE: u8 = 12;

/// Convert TL to EG units (left-shift by 1).
#[inline]
const fn tl_to_eg(d: u32) -> u32 {
    d << 1
}

// ---------------------------------------------------------------------------
// Patch ROM tables — match emu2413.cpp lines 42-104
//
// Each patch is 8 bytes; 16 instrument patches + 3 rhythm patches per chip
// type. VRC7 only uses the 16 instrument patches.
// ---------------------------------------------------------------------------

/// Chip type — selects the patch ROM table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChipType {
    /// YM2413 — original OPLL with 15 melodic patches + percussion.
    Ym2413,
    /// VRC7 — Konami mapper 85's custom OPLL variant.
    Vrc7,
    /// YMF281B — derivative used in some arcade hardware.
    Ymf281b,
}

impl ChipType {
    /// Returns the 19×8 patch dump for this chip type.
    #[inline]
    const fn patch_dump(self) -> &'static [u8; 19 * 8] {
        match self {
            Self::Ym2413 => &DEFAULT_INST_YM2413,
            Self::Vrc7 => &DEFAULT_INST_VRC7,
            Self::Ymf281b => &DEFAULT_INST_YMF281B,
        }
    }
}

/// YM2413 patch dump (16 melodic + 3 rhythm). emu2413 row 0.
const DEFAULT_INST_YM2413: [u8; 19 * 8] = [
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // 0: User
    0x71, 0x61, 0x1e, 0x17, 0xd0, 0x78, 0x00, 0x17, // 1: Violin
    0x13, 0x41, 0x1a, 0x0d, 0xd8, 0xf7, 0x23, 0x13, // 2: Guitar
    0x13, 0x01, 0x99, 0x00, 0xf2, 0xc4, 0x21, 0x23, // 3: Piano
    0x11, 0x61, 0x0e, 0x07, 0x8d, 0x64, 0x70, 0x27, // 4: Flute
    0x32, 0x21, 0x1e, 0x06, 0xe1, 0x76, 0x01, 0x28, // 5: Clarinet
    0x31, 0x22, 0x16, 0x05, 0xe0, 0x71, 0x00, 0x18, // 6: Oboe
    0x21, 0x61, 0x1d, 0x07, 0x82, 0x81, 0x11, 0x07, // 7: Trumpet
    0x33, 0x21, 0x2d, 0x13, 0xb0, 0x70, 0x00, 0x07, // 8: Organ
    0x61, 0x61, 0x1b, 0x06, 0x64, 0x65, 0x10, 0x17, // 9: Horn
    0x41, 0x61, 0x0b, 0x18, 0x85, 0xf0, 0x81, 0x07, // A: Synthesizer
    0x33, 0x01, 0x83, 0x11, 0xea, 0xef, 0x10, 0x04, // B: Harpsichord
    0x17, 0xc1, 0x24, 0x07, 0xf8, 0xf8, 0x22, 0x12, // C: Vibraphone
    0x61, 0x50, 0x0c, 0x05, 0xd2, 0xf5, 0x40, 0x42, // D: Synthsizer Bass
    0x01, 0x01, 0x55, 0x03, 0xe9, 0x90, 0x03, 0x02, // E: Acoustic Bass
    0x41, 0x41, 0x89, 0x03, 0xf1, 0xe4, 0xc0, 0x13, // F: Electric Guitar
    0x01, 0x01, 0x18, 0x0f, 0xdf, 0xf8, 0x6a, 0x6d, // R: Bass Drum
    0x01, 0x01, 0x00, 0x00, 0xc8, 0xd8, 0xa7, 0x68, // R: High-Hat(M) / Snare Drum(C)
    0x05, 0x01, 0x00, 0x00, 0xf8, 0xaa, 0x59, 0x55, // R: Tom-tom(M) / Top Cymbal(C)
];

/// VRC7 patch dump from Nuke.YKT analysis (16 melodic + 3 rhythm, but
/// VRC7 doesn't use rhythm). This is THE table for Lagrange Point.
const DEFAULT_INST_VRC7: [u8; 19 * 8] = [
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // 0: User
    0x03, 0x21, 0x05, 0x06, 0xe8, 0x81, 0x42, 0x27, // 1
    0x13, 0x41, 0x14, 0x0d, 0xd8, 0xf6, 0x23, 0x12, // 2
    0x11, 0x11, 0x08, 0x08, 0xfa, 0xb2, 0x20, 0x12, // 3
    0x31, 0x61, 0x0c, 0x07, 0xa8, 0x64, 0x61, 0x27, // 4
    0x32, 0x21, 0x1e, 0x06, 0xe1, 0x76, 0x01, 0x28, // 5
    0x02, 0x01, 0x06, 0x00, 0xa3, 0xe2, 0xf4, 0xf4, // 6
    0x21, 0x61, 0x1d, 0x07, 0x82, 0x81, 0x11, 0x07, // 7
    0x23, 0x21, 0x22, 0x17, 0xa2, 0x72, 0x01, 0x17, // 8
    0x35, 0x11, 0x25, 0x00, 0x40, 0x73, 0x72, 0x01, // 9
    0xb5, 0x01, 0x0f, 0x0F, 0xa8, 0xa5, 0x51, 0x02, // A
    0x17, 0xc1, 0x24, 0x07, 0xf8, 0xf8, 0x22, 0x12, // B
    0x71, 0x23, 0x11, 0x06, 0x65, 0x74, 0x18, 0x16, // C
    0x01, 0x02, 0xd3, 0x05, 0xc9, 0x95, 0x03, 0x02, // D
    0x61, 0x63, 0x0c, 0x00, 0x94, 0xC0, 0x33, 0xf6, // E
    0x21, 0x72, 0x0d, 0x00, 0xc1, 0xd5, 0x56, 0x06, // F
    0x01, 0x01, 0x18, 0x0f, 0xdf, 0xf8, 0x6a, 0x6d, // R: Bass Drum (unused on VRC7)
    0x01, 0x01, 0x00, 0x00, 0xc8, 0xd8, 0xa7, 0x68, // R: HH/SD
    0x05, 0x01, 0x00, 0x00, 0xf8, 0xaa, 0x59, 0x55, // R: Tom/Cymbal
];

/// YMF281B patch dump (kept for completeness; not used by VRC7).
const DEFAULT_INST_YMF281B: [u8; 19 * 8] = [
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x62, 0x21, 0x1a, 0x07, 0xf0, 0x6f, 0x00, 0x16,
    0x40, 0x10, 0x45, 0x00, 0xf6, 0x83, 0x73, 0x63, 0x13, 0x01, 0x99, 0x00, 0xf2, 0xc3, 0x21, 0x23,
    0x01, 0x61, 0x0b, 0x0f, 0xf9, 0x64, 0x70, 0x17, 0x32, 0x21, 0x1e, 0x06, 0xe1, 0x76, 0x01, 0x28,
    0x60, 0x01, 0x82, 0x0e, 0xf9, 0x61, 0x20, 0x27, 0x21, 0x61, 0x1c, 0x07, 0x84, 0x81, 0x11, 0x07,
    0x37, 0x32, 0xc9, 0x01, 0x66, 0x64, 0x40, 0x28, 0x01, 0x21, 0x07, 0x03, 0xa5, 0x71, 0x51, 0x07,
    0x06, 0x01, 0x5e, 0x07, 0xf3, 0xf3, 0xf6, 0x13, 0x00, 0x00, 0x18, 0x06, 0xf5, 0xf3, 0x20, 0x23,
    0x17, 0xc1, 0x24, 0x07, 0xf8, 0xf8, 0x22, 0x12, 0x35, 0x64, 0x00, 0x00, 0xff, 0xf3, 0x77, 0xf5,
    0x11, 0x31, 0x00, 0x07, 0xdd, 0xf3, 0xff, 0xfb, 0x3a, 0x21, 0x00, 0x07, 0x80, 0x84, 0x0f, 0xf5,
    0x01, 0x01, 0x18, 0x0f, 0xdf, 0xf8, 0x6a, 0x6d, 0x01, 0x01, 0x00, 0x00, 0xc8, 0xd8, 0xa7, 0x68,
    0x05, 0x01, 0x00, 0x00, 0xf8, 0xaa, 0x59, 0x55,
];

// ---------------------------------------------------------------------------
// exp_table[256] — match emu2413.cpp lines 137-154
//
// exp_table[x] = round((exp2((double)x / 256.0) - 1) * 1024)
// Used by the operator output: log-domain volume → linear amplitude.
// ---------------------------------------------------------------------------

const EXP_TABLE: [u16; 256] = [
    0, 3, 6, 8, 11, 14, 17, 20, 22, 25, 28, 31, 34, 37, 40, 42, 45, 48, 51, 54, 57, 60, 63, 66, 69,
    72, 75, 78, 81, 84, 87, 90, 93, 96, 99, 102, 105, 108, 111, 114, 117, 120, 123, 126, 130, 133,
    136, 139, 142, 145, 148, 152, 155, 158, 161, 164, 168, 171, 174, 177, 181, 184, 187, 190, 194,
    197, 200, 204, 207, 210, 214, 217, 220, 224, 227, 231, 234, 237, 241, 244, 248, 251, 255, 258,
    262, 265, 268, 272, 276, 279, 283, 286, 290, 293, 297, 300, 304, 308, 311, 315, 318, 322, 326,
    329, 333, 337, 340, 344, 348, 352, 355, 359, 363, 367, 370, 374, 378, 382, 385, 389, 393, 397,
    401, 405, 409, 412, 416, 420, 424, 428, 432, 436, 440, 444, 448, 452, 456, 460, 464, 468, 472,
    476, 480, 484, 488, 492, 496, 501, 505, 509, 513, 517, 521, 526, 530, 534, 538, 542, 547, 551,
    555, 560, 564, 568, 572, 577, 581, 585, 590, 594, 599, 603, 607, 612, 616, 621, 625, 630, 634,
    639, 643, 648, 652, 657, 661, 666, 670, 675, 680, 684, 689, 693, 698, 703, 708, 712, 717, 722,
    726, 731, 736, 741, 745, 750, 755, 760, 765, 770, 774, 779, 784, 789, 794, 799, 804, 809, 814,
    819, 824, 829, 834, 839, 844, 849, 854, 859, 864, 869, 874, 880, 885, 890, 895, 900, 906, 911,
    916, 921, 927, 932, 937, 942, 948, 953, 959, 964, 969, 975, 980, 986, 991, 996, 1002, 1007,
    1013, 1018,
];

// ---------------------------------------------------------------------------
// fullsin_table[256] — match emu2413.cpp lines 156-173
//
// fullsin_table[x] = round(-log2(sin((x + 0.5) * PI / (PG_WIDTH / 4) / 2)) * 256)
// Quarter-wave log-domain sine. PG generation mirrors across quadrants.
// First 256 entries explicit; remainder zero per the C declaration's
// implicit zero-init.
// ---------------------------------------------------------------------------

const FULLSIN_TABLE_QUARTER: [u16; 256] = [
    2137, 1731, 1543, 1419, 1326, 1252, 1190, 1137, 1091, 1050, 1013, 979, 949, 920, 894, 869, 846,
    825, 804, 785, 767, 749, 732, 717, 701, 687, 672, 659, 646, 633, 621, 609, 598, 587, 576, 566,
    556, 546, 536, 527, 518, 509, 501, 492, 484, 476, 468, 461, 453, 446, 439, 432, 425, 418, 411,
    405, 399, 392, 386, 380, 375, 369, 363, 358, 352, 347, 341, 336, 331, 326, 321, 316, 311, 307,
    302, 297, 293, 289, 284, 280, 276, 271, 267, 263, 259, 255, 251, 248, 244, 240, 236, 233, 229,
    226, 222, 219, 215, 212, 209, 205, 202, 199, 196, 193, 190, 187, 184, 181, 178, 175, 172, 169,
    167, 164, 161, 159, 156, 153, 151, 148, 146, 143, 141, 138, 136, 134, 131, 129, 127, 125, 122,
    120, 118, 116, 114, 112, 110, 108, 106, 104, 102, 100, 98, 96, 94, 92, 91, 89, 87, 85, 83, 82,
    80, 78, 77, 75, 74, 72, 70, 69, 67, 66, 64, 63, 62, 60, 59, 57, 56, 55, 53, 52, 51, 49, 48, 47,
    46, 45, 43, 42, 41, 40, 39, 38, 37, 36, 35, 34, 33, 32, 31, 30, 29, 28, 27, 26, 25, 24, 23, 23,
    22, 21, 20, 20, 19, 18, 17, 17, 16, 15, 15, 14, 13, 13, 12, 12, 11, 10, 10, 9, 9, 8, 8, 7, 7,
    7, 6, 6, 5, 5, 5, 4, 4, 4, 3, 3, 3, 2, 2, 2, 2, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0,
];

// ---------------------------------------------------------------------------
// pm_table[8][8] — pitch-modulation LFO table (emu2413.cpp lines 181-190)
// ---------------------------------------------------------------------------

const PM_TABLE: [[i8; 8]; 8] = [
    [0, 0, 0, 0, 0, 0, 0, 0],    // fnum = 000xxxxxx
    [0, 0, 1, 0, 0, 0, -1, 0],   // fnum = 001xxxxxx
    [0, 1, 2, 1, 0, -1, -2, -1], // fnum = 010xxxxxx
    [0, 1, 3, 1, 0, -1, -3, -1], // fnum = 011xxxxxx
    [0, 2, 4, 2, 0, -2, -4, -2], // fnum = 100xxxxxx
    [0, 2, 5, 2, 0, -2, -5, -2], // fnum = 101xxxxxx
    [0, 3, 6, 3, 0, -3, -6, -3], // fnum = 110xxxxxx
    [0, 3, 7, 3, 0, -3, -7, -3], // fnum = 111xxxxxx
];

// ---------------------------------------------------------------------------
// am_table[210] — amplitude-modulation LFO table (emu2413.cpp lines 195-209)
//
// Verified against real YM2413 hardware. Each element repeats 64 cycles.
// ---------------------------------------------------------------------------

const AM_TABLE: [u8; 210] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, //
    2, 2, 2, 2, 2, 2, 2, 2, 3, 3, 3, 3, 3, 3, 3, 3, //
    4, 4, 4, 4, 4, 4, 4, 4, 5, 5, 5, 5, 5, 5, 5, 5, //
    6, 6, 6, 6, 6, 6, 6, 6, 7, 7, 7, 7, 7, 7, 7, 7, //
    8, 8, 8, 8, 8, 8, 8, 8, 9, 9, 9, 9, 9, 9, 9, 9, //
    10, 10, 10, 10, 10, 10, 10, 10, 11, 11, 11, 11, 11, 11, 11, 11, //
    12, 12, 12, 12, 12, 12, 12, 12, //
    13, 13, 13, //
    12, 12, 12, 12, 12, 12, 12, 12, //
    11, 11, 11, 11, 11, 11, 11, 11, 10, 10, 10, 10, 10, 10, 10, 10, //
    9, 9, 9, 9, 9, 9, 9, 9, 8, 8, 8, 8, 8, 8, 8, 8, //
    7, 7, 7, 7, 7, 7, 7, 7, 6, 6, 6, 6, 6, 6, 6, 6, //
    5, 5, 5, 5, 5, 5, 5, 5, 4, 4, 4, 4, 4, 4, 4, 4, //
    3, 3, 3, 3, 3, 3, 3, 3, 2, 2, 2, 2, 2, 2, 2, 2, //
    1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0,
];

// ---------------------------------------------------------------------------
// EG step tables (emu2413.cpp lines 213-218) — based on andete's research
// ---------------------------------------------------------------------------

const EG_STEP_TABLES: [[u8; 8]; 4] = [
    [0, 1, 0, 1, 0, 1, 0, 1],
    [0, 1, 0, 1, 1, 1, 0, 1],
    [0, 1, 1, 1, 0, 1, 1, 1],
    [0, 1, 1, 1, 1, 1, 1, 1],
];

// ---------------------------------------------------------------------------
// Multiplier table (emu2413.cpp line 222-223). Doubled fixed-point.
// ---------------------------------------------------------------------------

const ML_TABLE: [u32; 16] = [
    1,
    2,
    2 * 2,
    3 * 2,
    4 * 2,
    5 * 2,
    6 * 2,
    7 * 2,
    8 * 2,
    9 * 2,
    10 * 2,
    10 * 2,
    12 * 2,
    12 * 2,
    15 * 2,
    15 * 2,
];

// ---------------------------------------------------------------------------
// Patch parameters (13 fields per patch). Matches OPLL_PATCH in emu2413.h.
// ---------------------------------------------------------------------------

/// One OPLL patch — the 13-field instrument definition.
#[derive(Clone, Copy, Debug, Default)]
pub struct Patch {
    /// Total level (carrier volume; 0-63 in dB units).
    pub tl: u8,
    /// Feedback level (modulator self-feedback).
    pub fb: u8,
    /// Envelope-generator sustain enable.
    pub eg: u8,
    /// Multiplier (frequency ratio).
    pub ml: u8,
    /// Attack rate.
    pub ar: u8,
    /// Decay rate.
    pub dr: u8,
    /// Sustain level.
    pub sl: u8,
    /// Release rate.
    pub rr: u8,
    /// Key-rate scaling.
    pub kr: u8,
    /// Key-level scaling.
    pub kl: u8,
    /// AM enable.
    pub am: u8,
    /// PM enable.
    pub pm: u8,
    /// Wave select (0=full sine, 1=half sine).
    pub ws: u8,
}

impl Patch {
    /// Decode 8 bytes of patch ROM dump into a Patch. Matches
    /// `OPLL_dumpToPatch` in emu2413.cpp lines 366-395.
    pub fn from_dump_modulator(dump: &[u8; 8]) -> Self {
        Self {
            am: (dump[0] >> 7) & 1,
            pm: (dump[0] >> 6) & 1,
            eg: (dump[0] >> 5) & 1,
            kr: (dump[0] >> 4) & 1,
            ml: dump[0] & 0x0f,
            kl: (dump[2] >> 6) & 0x03,
            tl: dump[2] & 0x3f,
            ar: (dump[4] >> 4) & 0x0f,
            dr: dump[4] & 0x0f,
            sl: (dump[6] >> 4) & 0x0f,
            rr: dump[6] & 0x0f,
            fb: dump[3] & 0x07,
            ws: (dump[3] >> 3) & 0x01,
        }
    }

    /// Decode 8 bytes of patch ROM dump into a carrier Patch.
    pub fn from_dump_carrier(dump: &[u8; 8]) -> Self {
        Self {
            am: (dump[1] >> 7) & 1,
            pm: (dump[1] >> 6) & 1,
            eg: (dump[1] >> 5) & 1,
            kr: (dump[1] >> 4) & 1,
            ml: dump[1] & 0x0f,
            kl: (dump[3] >> 6) & 0x03,
            tl: 0, // carrier TL comes from $3x register, not patch
            ar: (dump[5] >> 4) & 0x0f,
            dr: dump[5] & 0x0f,
            sl: (dump[7] >> 4) & 0x0f,
            rr: dump[7] & 0x0f,
            fb: 0,
            ws: (dump[3] >> 4) & 0x01,
        }
    }
}

// ---------------------------------------------------------------------------
// Envelope generator state machine (emu2413.cpp line 220)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
enum EgState {
    Attack,
    Decay,
    Sustain,
    Release,
    Damp,
    #[default]
    Unknown,
}

// ---------------------------------------------------------------------------
// Slot — one operator (modulator or carrier). 18 in YM2413, 12 used in VRC7.
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default)]
struct Slot {
    number: u8,
    /// Bit 0 (M): 0=modulator, 1=carrier. Bit 1 (S): rhythm-only flag.
    type_flags: u8,
    patch: Patch,

    /// Latest and previous output for self-feedback.
    output: [i32; 2],

    // Phase generator
    wave_table_idx: u8,
    pg_phase: u32,
    pg_out: u32,
    pg_keep: u8,
    blk_fnum: u16,
    fnum: u16,
    blk: u8,

    // Envelope generator
    eg_state: EgState,
    volume: i32,
    key_flag: u8,
    sus_flag: u8,
    tll: u16,
    rks: u8,
    eg_rate_h: u8,
    eg_rate_l: u8,
    eg_shift: u32,
    eg_out: u32,

    update_requests: u32,
}

impl Slot {
    /// Initialize a slot to the post-`reset_slot` state (emu2413.cpp:561-582).
    ///
    /// `number` is the slot index (0..18). Even indices are modulators
    /// (`type & 1 == 0`); odd are carriers (`type & 1 == 1`). The slot
    /// enters the `Release` envelope state with maximum attenuation
    /// (`eg_out = EG_MUTE`), ready to be keyed on.
    fn reset_to_release(&mut self, number: u8) {
        self.number = number;
        self.type_flags = number % 2;
        self.pg_keep = 0;
        self.wave_table_idx = 0;
        self.pg_phase = 0;
        self.output = [0, 0];
        self.eg_state = EgState::Release;
        self.eg_shift = 0;
        self.rks = 0;
        self.tll = 0;
        self.key_flag = 0;
        self.sus_flag = 0;
        self.blk_fnum = 0;
        self.blk = 0;
        self.fnum = 0;
        self.volume = 0;
        self.pg_out = 0;
        self.eg_out = EG_MUTE;
        self.eg_rate_h = 0;
        self.eg_rate_l = 0;
        self.update_requests = 0;
        self.patch = Patch::default();
    }

    /// Advance the phase generator by one OPLL clock and refresh `pg_out`.
    ///
    /// Direct port of emu2413.cpp lines 765-773.  The phase increment is
    /// `((fnum_low9 * 2 + pm) * ml_table[ML]) << blk >> 2`, where `pm`
    /// is the pitch-modulation offset from [`PM_TABLE`] when the patch's
    /// PM bit is set.  The DP counter wraps modulo `DP_WIDTH`; `pg_out`
    /// is the upper [`PG_BITS`] of the counter (the index into the
    /// 1024-entry sine table).
    ///
    /// `pm_phase` is the chip's global PM LFO phase (`Opll::pm_phase`).
    /// `reset` is true on key-on for rhythm slots with `pg_keep == 0`
    /// (the carrier of a damped channel re-zeros its phase).
    fn calc_phase(&mut self, pm_phase: i32, reset: bool) {
        let pm = if self.patch.pm != 0 {
            // pm_table[fnum>>6 & 7][pm_phase>>10 & 7] in C; values are i8.
            let fnum_row = (self.fnum as usize >> 6) & 7;
            let phase_col = ((pm_phase >> 10) & 7) as usize;
            i32::from(PM_TABLE[fnum_row][phase_col])
        } else {
            0
        };

        if reset {
            self.pg_phase = 0;
        }

        // fnum_low9 = fnum & 0x1FF; phase increment expression matches C.
        let fnum_low9 = i32::from(self.fnum & 0x1FF);
        let ml = ML_TABLE[self.patch.ml as usize] as i32;
        let increment_pre_shift = (fnum_low9 * 2 + pm) * ml;
        // `<< blk >> 2` in C — wrap with i64 to safely shift then truncate.
        let shifted = (i64::from(increment_pre_shift) << self.blk) >> 2;

        // Cast back to u32 with wrapping_add, then mask to DP range.
        self.pg_phase = self.pg_phase.wrapping_add(shifted as u32) & (DP_WIDTH - 1);
        self.pg_out = self.pg_phase >> DP_BASE_BITS;
    }

    /// Attack-state EG step lookup. Direct port of emu2413.cpp:775-795.
    fn lookup_attack_step(&self, counter: u32) -> u8 {
        match self.eg_rate_h {
            12 => {
                let index = ((counter & 0xc) >> 1) as usize;
                4 - EG_STEP_TABLES[self.eg_rate_l as usize][index]
            }
            13 => {
                let index = ((counter & 0xc) >> 1) as usize;
                3 - EG_STEP_TABLES[self.eg_rate_l as usize][index]
            }
            14 => {
                let index = ((counter & 0xc) >> 1) as usize;
                2 - EG_STEP_TABLES[self.eg_rate_l as usize][index]
            }
            0 | 15 => 0,
            _ => {
                let index = (counter >> self.eg_shift) as usize;
                if EG_STEP_TABLES[self.eg_rate_l as usize][index & 7] != 0 {
                    4
                } else {
                    0
                }
            }
        }
    }

    /// Decay-state EG step lookup. Direct port of emu2413.cpp:797-815.
    fn lookup_decay_step(&self, counter: u32) -> u8 {
        match self.eg_rate_h {
            0 => 0,
            13 => {
                let index = (((counter & 0xc) >> 1) | (counter & 1)) as usize;
                EG_STEP_TABLES[self.eg_rate_l as usize][index]
            }
            14 => {
                let index = ((counter & 0xc) >> 1) as usize;
                EG_STEP_TABLES[self.eg_rate_l as usize][index] + 1
            }
            15 => 2,
            _ => {
                let index = (counter >> self.eg_shift) as usize;
                EG_STEP_TABLES[self.eg_rate_l as usize][index & 7]
            }
        }
    }

    /// Begin envelope from the `Damp` → `Attack`/`Decay` transition
    /// (emu2413.cpp:817-825).  If the effective attack rate saturates
    /// at 15, the operator skips Attack and enters Decay at zero
    /// attenuation (instant attack).
    fn start_envelope(&mut self) {
        // min(15, AR + (rks >> 2)) — saturated effective rate.
        let effective_ar = (self.patch.ar + (self.rks >> 2)).min(15);
        if effective_ar == 15 {
            self.eg_state = EgState::Decay;
            self.eg_out = 0;
        } else {
            self.eg_state = EgState::Attack;
        }
        self.update_requests |= UPDATE_EG;
    }

    /// Run one envelope-generator tick (emu2413.cpp:827-887).
    ///
    /// Returns [`EnvelopeStep::ResetBuddyPhase`] when this carrier slot
    /// just transitioned out of Damp on key-on AND the caller should
    /// also reset the modulator's `pg_phase`.  The caller (Opll) is
    /// responsible for applying the buddy reset — Rust's borrow checker
    /// rules out a buddy `&mut` while we hold `&mut self`.
    ///
    /// `buddy_pg_keep` is the buddy slot's `pg_keep` flag (false for
    /// non-rhythm channels).  `eg_counter` is the chip's global EG
    /// counter (`Opll::eg_counter`).  `test` is bit 1 of register `$0F`
    /// (forces `eg_out` to 0 each tick).
    fn calc_envelope(&mut self, buddy_pg_keep: bool, eg_counter: u32, test: u8) -> EnvelopeStep {
        let mask = (1u32 << self.eg_shift).wrapping_sub(1);
        let mut buddy_reset = EnvelopeStep::Continue;

        if self.eg_state == EgState::Attack {
            if self.eg_out > 0 && self.eg_rate_h > 0 && (eg_counter & mask & !3) == 0 {
                let s = self.lookup_attack_step(eg_counter);
                if s > 0 {
                    let cur = self.eg_out as i32;
                    let next = (cur - (cur >> s) - 1).max(0);
                    self.eg_out = next as u32;
                }
            }
        } else if self.eg_rate_h > 0 && (eg_counter & mask) == 0 {
            self.eg_out =
                (self.eg_out + u32::from(self.lookup_decay_step(eg_counter))).min(EG_MUTE);
        }

        match self.eg_state {
            EgState::Damp => {
                if self.eg_out >= EG_MAX && (eg_counter & mask) == 0 {
                    self.start_envelope();
                    // For carriers (type bit 0 set), the carrier's
                    // pg_phase resets to 0 unless pg_keep is set; the
                    // modulator buddy also resets (caller applies).
                    if self.type_flags & 1 != 0 {
                        if self.pg_keep == 0 {
                            self.pg_phase = 0;
                        }
                        if !buddy_pg_keep {
                            buddy_reset = EnvelopeStep::ResetBuddyPhase;
                        }
                    }
                }
            }
            EgState::Attack => {
                if self.eg_out == 0 {
                    self.eg_state = EgState::Decay;
                    self.update_requests |= UPDATE_EG;
                }
            }
            EgState::Decay => {
                // Decay → Sustain transition is checked every cycle
                // (NOT synchronized with the envelope counter — per
                // upstream comment at emu2413.cpp:871).
                if (self.eg_out >> 3) == u32::from(self.patch.sl) {
                    self.eg_state = EgState::Sustain;
                    self.update_requests |= UPDATE_EG;
                }
            }
            EgState::Sustain | EgState::Release | EgState::Unknown => {}
        }

        if test != 0 {
            self.eg_out = 0;
        }

        buddy_reset
    }
}

/// Result of a single [`Slot::calc_envelope`] tick — signals the caller
/// when to reset the buddy slot's `pg_phase` (the Damp → Attack carrier
/// transition resets both the carrier's and modulator's phase counters).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EnvelopeStep {
    /// No buddy-state mutation required.
    Continue,
    /// Caller must zero the buddy slot's `pg_phase`.
    ResetBuddyPhase,
}

// ---------------------------------------------------------------------------
// Update-request flags — emu2413.cpp:504-510. Set by setters that change
// patch/fnum/volume, consumed by `commit_slot_update` (next sprint).
// ---------------------------------------------------------------------------

const UPDATE_WS: u32 = 1;
const UPDATE_TLL: u32 = 2;
const UPDATE_RKS: u32 = 4;
const UPDATE_EG: u32 = 8;
const UPDATE_ALL: u32 = 255;

// ---------------------------------------------------------------------------
// kl_table — key-level scaling base values (emu2413.cpp:226-228).
// All values are pre-doubled (`dB2(x) = x * 2`) so the raw cell value is
// the 1/2-dB attenuation magnitude.
// ---------------------------------------------------------------------------

const KL_TABLE: [f32; 16] = [
    0.0, 18.0, 24.0, 27.75, 30.0, 32.25, 33.75, 35.25, 36.0, 37.5, 38.25, 39.0, 39.75, 40.5, 41.25,
    42.0,
];

// ---------------------------------------------------------------------------
// Runtime-built lookup tables. Sized for the full OPLL register space.
// ---------------------------------------------------------------------------

/// Full 1024-entry log-domain sine table (extended from
/// [`FULLSIN_TABLE_QUARTER`] per emu2413.cpp:356-372).
///
/// First quarter [0..256): explicit (input data).
/// Second quarter [256..512): mirror of first (descending).
/// Second half [512..1024): first half with sign bit (`0x8000`) set.
#[derive(Clone)]
struct WaveTables {
    fullsin: [u16; PG_WIDTH],
    /// First half = `fullsin[0..512]`; second half = `0xfff` (mute).
    halfsin: [u16; PG_WIDTH],
}

impl WaveTables {
    fn new() -> Self {
        let mut fullsin = [0u16; PG_WIDTH];
        let qw = PG_WIDTH / 4;
        // First quarter: copy from the quarter-wave LUT.
        fullsin[..qw].copy_from_slice(&FULLSIN_TABLE_QUARTER);
        // Second quarter: mirror (descending) from the first.
        for x in 0..qw {
            fullsin[qw + x] = fullsin[qw - x - 1];
        }
        // Second half: set the sign bit on each first-half entry.
        for x in 0..(PG_WIDTH / 2) {
            fullsin[PG_WIDTH / 2 + x] = 0x8000 | fullsin[x];
        }

        let mut halfsin = [0u16; PG_WIDTH];
        halfsin[..(PG_WIDTH / 2)].copy_from_slice(&fullsin[..(PG_WIDTH / 2)]);
        for slot in &mut halfsin[(PG_WIDTH / 2)..] {
            *slot = 0xfff;
        }
        Self { fullsin, halfsin }
    }

    #[inline]
    fn sample(&self, idx: u8, phase: u32) -> u16 {
        let i = (phase as usize) & (PG_WIDTH - 1);
        match idx {
            0 => self.fullsin[i],
            _ => self.halfsin[i],
        }
    }
}

/// Total-Level Lookup. `tll[block_fnum_idx][TL or volume][KL] → EG units`.
///
/// `block_fnum_idx = (block << 4) | (fnum_high_4)` — 7 bits indexing
/// 128 rows. TL ranges 0..64, KL ranges 0..4.
#[derive(Clone)]
struct TllRksTables {
    /// Flat storage of `[block_fnum: 128][TL: 64][KL: 4]` u32s
    /// (32,768 entries × 4 bytes = 128 KiB).  Indexed via
    /// [`TllRksTables::tll_at`].
    tll_flat: alloc::boxed::Box<[u32]>,
    /// `rks[(block << 1) | fnum_top_bit][KR]`.
    rks: [[u8; 2]; 16],
}

impl TllRksTables {
    /// Read TLL[block_fnum][TL][KL]. Bounds are: `block_fnum < 128`,
    /// `tl < 64`, `kl < 4`.
    #[inline]
    fn tll_at(&self, block_fnum: usize, tl: usize, kl: usize) -> u32 {
        self.tll_flat[block_fnum * 64 * 4 + tl * 4 + kl]
    }
}

impl TllRksTables {
    fn new() -> Self {
        // Allocate the 128 KiB TLL table directly on the heap via
        // `vec!` to avoid the 128 KiB stack intermediate that
        // `Box::new([...])` would otherwise require.
        let mut tll_flat: alloc::vec::Vec<u32> = alloc::vec![0u32; 128 * 64 * 4];

        // emu2413.cpp:374-396 — buildTllTable
        for (fnum, &kl_val) in KL_TABLE.iter().enumerate() {
            for block in 0..8usize {
                let idx = (block << 4) | fnum;
                for tl in 0..64usize {
                    for kl in 0..4usize {
                        let pos = idx * 64 * 4 + tl * 4 + kl;
                        if kl == 0 {
                            tll_flat[pos] = tl_to_eg(tl as u32);
                        } else {
                            // tmp = (int32_t)(kl_table[fnum] - dB2(3.0) * (7 - block))
                            let tmp = (kl_val - 6.0 * (7 - block) as f32) as i32;
                            if tmp <= 0 {
                                tll_flat[pos] = tl_to_eg(tl as u32);
                            } else {
                                let shifted = tmp >> (3 - kl as u32);
                                // EG_STEP = 0.375 → division by 0.375 = multiplication by 8/3
                                let scaled = (shifted as f32 / 0.375) as u32;
                                tll_flat[pos] = scaled + tl_to_eg(tl as u32);
                            }
                        }
                    }
                }
            }
        }
        let tll_flat = tll_flat.into_boxed_slice();

        // emu2413.cpp:398-405 — buildRksTable
        let mut rks = [[0u8; 2]; 16];
        for fnum8 in 0..2usize {
            for block in 0..8usize {
                let idx = (block << 1) | fnum8;
                rks[idx][1] = ((block << 1) + fnum8) as u8;
                rks[idx][0] = (block >> 1) as u8;
            }
        }

        Self { tll_flat, rks }
    }
}

// ---------------------------------------------------------------------------
// Operator output stage — emu2413.cpp:911-925
// ---------------------------------------------------------------------------

/// Decode a 16-bit log-domain magnitude into a 13-bit linear sample
/// (-4095..=4095). Direct port of emu2413.cpp:911-916.
///
/// Layout of `i`:
/// - bit 15: sign
/// - bits 14-8: exponent (right-shift amount)
/// - bits 7-0: mantissa index into [`EXP_TABLE`]
#[inline]
fn lookup_exp_table(i: u32) -> i16 {
    // From andete's expression.  The C code on x86 implicitly masks the
    // shift to bits [5:0] (the hardware's `shr` masking behavior). We
    // mirror that here: shifts >= 32 saturate to "fully attenuated"
    // which is what emu2413 produces on x86 / ARM (the only platforms
    // it runs on).  Without the mask, debug builds in Rust panic on
    // `shr-overflow`.
    let t = i32::from(EXP_TABLE[((i & 0xff) ^ 0xff) as usize]) + 1024;
    let shift = ((i & 0x7f00) >> 8) & 31;
    let res = t >> shift;
    let signed = if (i & 0x8000) != 0 { !res } else { res };
    (signed << 1) as i16
}

/// Convert a wave-table log-magnitude `h` to a linear sample, applying
/// the slot's envelope + total-level + AM offset. Direct port of
/// emu2413.cpp:918-925.
#[inline]
fn to_linear(h: u16, slot: &Slot, am: u8) -> i16 {
    if slot.eg_out > EG_MAX {
        return 0;
    }
    let att = (slot.eg_out + u32::from(slot.tll) + u32::from(am)).min(EG_MUTE) << 4;
    lookup_exp_table(u32::from(h) + att)
}

// ---------------------------------------------------------------------------
// Opll — the chip instance.
// ---------------------------------------------------------------------------

/// OPLL (YM2413 / VRC7) FM synthesizer instance.
///
/// One instance per VRC7-mapped cartridge. Caller drives the chip via
/// [`Opll::write_reg`] and pulls samples via [`Opll::calc`] at the
/// OPLL's native 49,716 Hz sample rate.
///
/// # Example
///
/// ```ignore
/// // VRC7-mode chip for Lagrange Point
/// let mut opll = Opll::new(ChipType::Vrc7);
/// opll.write_reg(0x30, 0x01); // channel 0 instrument = patch 1
/// opll.write_reg(0x10, 0x80); // channel 0 fnum low
/// opll.write_reg(0x20, 0x15); // channel 0 fnum high + block + key-on
/// let sample: i16 = opll.calc();
/// ```
#[derive(Clone)]
pub struct Opll {
    chip_type: ChipType,

    /// Current register address (set by writes to `$9010` on VRC7).
    adr: u8,

    /// All 64 OPLL registers (shadow).
    reg: [u8; 0x40],

    /// Test flag (register $0F bit 4).
    test_flag: u8,

    /// Bit mask of key-on slots (1 bit per slot).
    slot_key_status: u32,

    /// EG global counter (drives envelope timing).
    eg_counter: u32,

    /// PM (pitch modulation) LFO phase.
    pm_phase: u32,
    /// AM (amplitude modulation) LFO phase.
    am_phase: i32,
    /// Current AM LFO output value (0..13).
    lfo_am: u8,

    /// Per-channel patch number (0-15; 0=user patch).
    patch_number: [i32; 9],

    /// 18 slots (9 channels × 2 ops). VRC7 only uses indices [0..12).
    slot: [Slot; 18],

    /// Loaded patch set: 19 slots (16 melodic + 3 rhythm) × 2 ops each.
    /// Index 0 is the user patch (writeable via $00-$07).
    patch_set: Vec<Patch>,

    /// Per-channel output sample (after operator + envelope).
    ch_out: [i16; 14],

    /// Mixed mono output.
    mix_out: i16,

    /// Full 1024-entry sine + half-sine wave tables (built at
    /// construction).
    waves: WaveTables,

    /// TLL + RKS tables (built at construction).  TLL is heap-allocated
    /// (~128 KiB) since it indexes `[128][64][4]` of `u32`.
    tll_rks: TllRksTables,
}

impl Opll {
    /// Construct a new OPLL instance for the given chip type.
    ///
    /// VRC7 mode loads the Konami custom patch set (the Nuke.YKT
    /// analysis values) — this is the table Lagrange Point uses.
    pub fn new(chip_type: ChipType) -> Self {
        let mut opll = Self {
            chip_type,
            adr: 0,
            reg: [0; 0x40],
            test_flag: 0,
            slot_key_status: 0,
            eg_counter: 0,
            pm_phase: 0,
            am_phase: 0,
            lfo_am: 0,
            patch_number: [0; 9],
            slot: [Slot::default(); 18],
            patch_set: vec![Patch::default(); 19 * 2],
            ch_out: [0; 14],
            mix_out: 0,
            waves: WaveTables::new(),
            tll_rks: TllRksTables::new(),
        };
        opll.reset_patch(chip_type);
        opll.reset();
        opll
    }

    /// Reset all channel/operator state. Patches are preserved.
    pub fn reset(&mut self) {
        self.adr = 0;
        self.reg = [0; 0x40];
        self.test_flag = 0;
        self.slot_key_status = 0;
        self.eg_counter = 0;
        self.pm_phase = 0;
        self.am_phase = 0;
        self.lfo_am = 0;
        self.patch_number = [0; 9];
        // Per emu2413.cpp:561-582 each slot enters Release with eg_out
        // at EG_MUTE (max attenuation) — ready for the next key-on.
        for (i, s) in self.slot.iter_mut().enumerate() {
            s.reset_to_release(i as u8);
        }
        self.ch_out = [0; 14];
        self.mix_out = 0;
    }

    /// Load the patch ROM for a chip type.
    pub fn reset_patch(&mut self, chip_type: ChipType) {
        let dump = chip_type.patch_dump();
        for i in 0..19 {
            let chunk: &[u8; 8] = (&dump[i * 8..i * 8 + 8]).try_into().unwrap();
            self.patch_set[i * 2] = Patch::from_dump_modulator(chunk);
            self.patch_set[i * 2 + 1] = Patch::from_dump_carrier(chunk);
        }
    }

    /// Write `val` to OPLL register `reg` (0x00..=0x3F). Larger
    /// addresses are masked to 6 bits. Direct port of
    /// `OPLL_writeReg` in emu2413.cpp:1223-1394.
    ///
    /// This is the entry point VRC7 calls when the CPU writes to
    /// `$9030` (after latching the register address via `$9010`).
    /// The decoder routes the write to the appropriate channel /
    /// patch / control surface and schedules per-slot
    /// `commit_slot_update` for the next OPLL tick.
    ///
    /// VRC7-specific behaviour (`chip_type == Vrc7`):
    /// - `$0E` (rhythm mode) is ignored — VRC7 has no rhythm channels
    /// - Register addresses for channels 6, 7, 8 (`$16+`, `$26+`,
    ///   `$36+`) are ignored — VRC7 wires only 6 melodic channels
    #[allow(clippy::too_many_lines)]
    pub fn write_reg(&mut self, reg: u8, val: u8) {
        if reg >= 0x40 {
            return;
        }

        // Mirror registers (emu2413.cpp:1230-1232): `$19-$1F` → `$10-$16`,
        // `$29-$2F` → `$20-$26`, `$39-$3F` → `$30-$36`.
        let reg = if (0x19..=0x1F).contains(&reg)
            || (0x29..=0x2F).contains(&reg)
            || (0x39..=0x3F).contains(&reg)
        {
            reg - 9
        } else {
            reg
        };
        self.reg[reg as usize] = val;

        let is_vrc7 = self.chip_type == ChipType::Vrc7;

        match reg {
            // ---- $00-$07: user patch (patch[0] = modulator, patch[1] = carrier) ----
            0x00 => {
                self.patch_set[0].am = (val >> 7) & 1;
                self.patch_set[0].pm = (val >> 6) & 1;
                self.patch_set[0].eg = (val >> 5) & 1;
                self.patch_set[0].kr = (val >> 4) & 1;
                self.patch_set[0].ml = val & 0x0f;
                for ch in 0..9 {
                    if self.patch_number[ch] == 0 {
                        self.slot[ch * 2].update_requests |= UPDATE_RKS | UPDATE_EG;
                    }
                }
                self.refresh_user_patch_pointers();
            }
            0x01 => {
                self.patch_set[1].am = (val >> 7) & 1;
                self.patch_set[1].pm = (val >> 6) & 1;
                self.patch_set[1].eg = (val >> 5) & 1;
                self.patch_set[1].kr = (val >> 4) & 1;
                self.patch_set[1].ml = val & 0x0f;
                for ch in 0..9 {
                    if self.patch_number[ch] == 0 {
                        self.slot[ch * 2 + 1].update_requests |= UPDATE_RKS | UPDATE_EG;
                    }
                }
                self.refresh_user_patch_pointers();
            }
            0x02 => {
                self.patch_set[0].kl = (val >> 6) & 3;
                self.patch_set[0].tl = val & 0x3f;
                for ch in 0..9 {
                    if self.patch_number[ch] == 0 {
                        self.slot[ch * 2].update_requests |= UPDATE_TLL;
                    }
                }
                self.refresh_user_patch_pointers();
            }
            0x03 => {
                self.patch_set[1].kl = (val >> 6) & 3;
                self.patch_set[1].ws = (val >> 4) & 1;
                self.patch_set[0].ws = (val >> 3) & 1;
                self.patch_set[0].fb = val & 7;
                for ch in 0..9 {
                    if self.patch_number[ch] == 0 {
                        self.slot[ch * 2].update_requests |= UPDATE_WS;
                        self.slot[ch * 2 + 1].update_requests |= UPDATE_WS | UPDATE_TLL;
                    }
                }
                self.refresh_user_patch_pointers();
            }
            0x04 => {
                self.patch_set[0].ar = (val >> 4) & 0x0f;
                self.patch_set[0].dr = val & 0x0f;
                for ch in 0..9 {
                    if self.patch_number[ch] == 0 {
                        self.slot[ch * 2].update_requests |= UPDATE_EG;
                    }
                }
                self.refresh_user_patch_pointers();
            }
            0x05 => {
                self.patch_set[1].ar = (val >> 4) & 0x0f;
                self.patch_set[1].dr = val & 0x0f;
                for ch in 0..9 {
                    if self.patch_number[ch] == 0 {
                        self.slot[ch * 2 + 1].update_requests |= UPDATE_EG;
                    }
                }
                self.refresh_user_patch_pointers();
            }
            0x06 => {
                self.patch_set[0].sl = (val >> 4) & 0x0f;
                self.patch_set[0].rr = val & 0x0f;
                for ch in 0..9 {
                    if self.patch_number[ch] == 0 {
                        self.slot[ch * 2].update_requests |= UPDATE_EG;
                    }
                }
                self.refresh_user_patch_pointers();
            }
            0x07 => {
                self.patch_set[1].sl = (val >> 4) & 0x0f;
                self.patch_set[1].rr = val & 0x0f;
                for ch in 0..9 {
                    if self.patch_number[ch] == 0 {
                        self.slot[ch * 2 + 1].update_requests |= UPDATE_EG;
                    }
                }
                self.refresh_user_patch_pointers();
            }

            // ---- $0E: rhythm mode (VRC7 ignores; YM2413 not yet implemented) ----
            0x0E => {
                // VRC7 has no rhythm channels; ignore.
                // Full YM2413 rhythm-mode handling would go here.
            }

            // ---- $0F: test flag ----
            0x0F => {
                self.test_flag = val;
            }

            // ---- $10-$18: per-channel fnum low byte (VRC7 caps at $15) ----
            0x10..=0x18 => {
                let ch = (reg - 0x10) as usize;
                if is_vrc7 && reg >= 0x16 {
                    return;
                }
                let fnum_high_bit = u16::from(self.reg[0x20 + ch] & 1);
                let fnum = u16::from(val) | (fnum_high_bit << 8);
                self.set_fnumber_internal(ch, fnum);
            }

            // ---- $20-$28: per-channel fnum-high + block + key-on + sustain ----
            0x20..=0x28 => {
                let ch = (reg - 0x20) as usize;
                if is_vrc7 && reg >= 0x26 {
                    return;
                }
                let fnum = (u16::from(val & 1) << 8) | u16::from(self.reg[0x10 + ch]);
                self.set_fnumber_internal(ch, fnum);
                let blk = (val >> 1) & 7;
                self.set_block_internal(ch, blk);
                let sus = (val >> 5) & 1;
                self.set_sus_flag_internal(ch, sus);
                self.update_key_status();
            }

            // ---- $30-$38: per-channel volume + instrument select ----
            0x30..=0x38 => {
                let ch = (reg - 0x30) as usize;
                if is_vrc7 && reg >= 0x36 {
                    return;
                }
                let inst = (val >> 4) & 0x0f;
                self.set_patch_internal(ch, usize::from(inst));
                let vol = i32::from((val & 0x0f) << 2);
                self.set_volume_internal(ch, vol);
            }

            _ => {}
        }
    }

    /// Re-point the channels currently using the user patch (number 0)
    /// at the freshly-rewritten patch_set[0] / patch_set[1] entries.
    /// Each slot caches a copy of its `Patch` for hot-path access, so
    /// patch-modifying writes to `$00-$07` must propagate the new
    /// fields into the slots.
    fn refresh_user_patch_pointers(&mut self) {
        for ch in 0..9 {
            if self.patch_number[ch] == 0 {
                self.slot[ch * 2].patch = self.patch_set[0];
                self.slot[ch * 2 + 1].patch = self.patch_set[1];
            }
        }
    }

    /// Production set_patch: assigns instrument `num` to channel `ch`
    /// and requests slot recomputation. Mirrors `set_patch` in
    /// emu2413.cpp:645-651.
    fn set_patch_internal(&mut self, ch: usize, num: usize) {
        if ch >= 9 || num * 2 + 1 >= self.patch_set.len() {
            return;
        }
        self.patch_number[ch] = num as i32;
        self.slot[ch * 2].patch = self.patch_set[num * 2];
        self.slot[ch * 2 + 1].patch = self.patch_set[num * 2 + 1];
        self.slot[ch * 2].update_requests |= UPDATE_ALL;
        self.slot[ch * 2 + 1].update_requests |= UPDATE_ALL;
    }

    fn set_fnumber_internal(&mut self, ch: usize, fnum: u16) {
        if ch >= 9 {
            return;
        }
        let f = fnum & 0x1ff;
        for slot_idx in [ch * 2, ch * 2 + 1] {
            self.slot[slot_idx].fnum = f;
            self.slot[slot_idx].blk_fnum = (self.slot[slot_idx].blk_fnum & 0xe00) | f;
            self.slot[slot_idx].update_requests |= UPDATE_EG | UPDATE_RKS | UPDATE_TLL;
        }
    }

    fn set_block_internal(&mut self, ch: usize, blk: u8) {
        if ch >= 9 {
            return;
        }
        let b = blk & 7;
        for slot_idx in [ch * 2, ch * 2 + 1] {
            self.slot[slot_idx].blk = b;
            self.slot[slot_idx].blk_fnum =
                (u16::from(b) << 9) | (self.slot[slot_idx].blk_fnum & 0x1ff);
            self.slot[slot_idx].update_requests |= UPDATE_EG | UPDATE_RKS | UPDATE_TLL;
        }
    }

    fn set_sus_flag_internal(&mut self, ch: usize, sus: u8) {
        if ch >= 9 {
            return;
        }
        self.slot[ch * 2 + 1].sus_flag = sus;
        self.slot[ch * 2 + 1].update_requests |= UPDATE_EG;
        // For the rhythm "single slot mode" carriers we'd also set
        // the modulator's sus_flag; not relevant to VRC7's 6 melodic
        // channels (none have type & 1 == 1 for the modulator).
    }

    fn set_volume_internal(&mut self, ch: usize, volume: i32) {
        if ch >= 9 {
            return;
        }
        self.slot[ch * 2 + 1].volume = volume;
        self.slot[ch * 2 + 1].update_requests |= UPDATE_TLL;
    }

    /// Update the key-on/off state across all 9 channels based on
    /// the current `$20-$28` bit 4 values. Mirrors
    /// `update_key_status` in emu2413.cpp:600-643.
    ///
    /// VRC7-only path (no rhythm). For each channel: bit 4 of `$2x`
    /// is the key-on flag; this routine compares against the prior
    /// `slot_key_status` snapshot and issues `slot_on` / `slot_off`
    /// only on transitions.
    fn update_key_status(&mut self) {
        let mut new_status: u32 = 0;
        let ch_count = if self.chip_type == ChipType::Vrc7 {
            6
        } else {
            9
        };
        for ch in 0..ch_count {
            if self.reg[0x20 + ch] & 0x10 != 0 {
                new_status |= 3u32 << (ch * 2);
            }
        }
        let changed = self.slot_key_status ^ new_status;
        if changed != 0 {
            for i in 0..18 {
                if (changed >> i) & 1 != 0 {
                    if (new_status >> i) & 1 != 0 {
                        self.slot_on(i);
                    } else {
                        self.slot_off(i);
                    }
                }
            }
        }
        self.slot_key_status = new_status;
    }

    /// Slot key-on. Sets `key_flag`, enters Damp state, requests EG
    /// recompute. Per `slotOn` in emu2413.cpp:584-589.
    fn slot_on(&mut self, slot_idx: usize) {
        if slot_idx >= 18 {
            return;
        }
        self.slot[slot_idx].key_flag = 1;
        self.slot[slot_idx].eg_state = EgState::Damp;
        self.slot[slot_idx].update_requests |= UPDATE_EG;
    }

    /// Slot key-off. For carriers (type & 1 == 1), enters Release;
    /// for modulators, just clears key_flag. Per `slotOff` in
    /// emu2413.cpp:591-598.
    fn slot_off(&mut self, slot_idx: usize) {
        if slot_idx >= 18 {
            return;
        }
        self.slot[slot_idx].key_flag = 0;
        if self.slot[slot_idx].type_flags & 1 != 0 {
            self.slot[slot_idx].eg_state = EgState::Release;
            self.slot[slot_idx].update_requests |= UPDATE_EG;
        }
    }

    /// Read a register shadow byte (debugger / save-state helper).
    #[must_use]
    pub fn read_reg(&self, reg: u8) -> u8 {
        self.reg[(reg & 0x3F) as usize]
    }

    /// Generate one mono sample at the OPLL's native 49,716 Hz rate.
    ///
    /// Drives the full per-clock pipeline: AM/PM LFO update → per-slot
    /// commit_slot_update / calc_envelope / calc_phase → per-channel
    /// 2-op FM output (modulator with self-feedback, carrier modulated
    /// by modulator's output) → channel summation. For VRC7 (chip type
    /// 1), only the 6 melodic channels are summed; the rhythm channels
    /// in slots 12..18 are not used.
    ///
    /// Until the register-write decoder lands (Sprint 1.2), `calc`
    /// produces silence because no slot is keyed on — but the full
    /// pipeline runs every call (so AM/PM/EG advance and the cost
    /// model is realistic).
    pub fn calc(&mut self) -> i16 {
        self.update_output();
        self.mix_output();
        self.mix_out
    }

    /// Returns the chip type.
    #[must_use]
    pub const fn chip_type(&self) -> ChipType {
        self.chip_type
    }

    /// AM + PM LFO update (emu2413.cpp:730-739).
    fn update_ampm(&mut self) {
        if self.test_flag & 2 != 0 {
            self.pm_phase = 0;
            self.am_phase = 0;
        } else {
            let pm_inc: u32 = if self.test_flag & 8 != 0 { 1024 } else { 1 };
            let am_inc: i32 = if self.test_flag & 8 != 0 { 64 } else { 1 };
            self.pm_phase = self.pm_phase.wrapping_add(pm_inc);
            self.am_phase = self.am_phase.wrapping_add(am_inc);
        }
        let idx = ((self.am_phase >> 6) as usize) % AM_TABLE.len();
        self.lfo_am = AM_TABLE[idx];
    }

    /// Get the effective rate for `get_parameter_rate` (emu2413.cpp:476-502).
    fn get_parameter_rate(slot: &Slot) -> u8 {
        if (slot.type_flags & 1) == 0 && slot.key_flag == 0 {
            return 0;
        }
        match slot.eg_state {
            EgState::Attack => slot.patch.ar,
            EgState::Decay => slot.patch.dr,
            EgState::Sustain => {
                if slot.patch.eg != 0 {
                    0
                } else {
                    slot.patch.rr
                }
            }
            EgState::Release => {
                if slot.sus_flag != 0 {
                    5
                } else if slot.patch.eg != 0 {
                    slot.patch.rr
                } else {
                    7
                }
            }
            EgState::Damp => DAMPER_RATE,
            EgState::Unknown => 0,
        }
    }

    /// Commit pending update requests for slot `i` (emu2413.cpp:514-559).
    /// Translates patch/fnum/volume changes into the cached rate / level
    /// state the per-clock DSP reads.
    fn commit_slot_update(&mut self, i: usize) {
        let requests = self.slot[i].update_requests;
        if requests == 0 {
            return;
        }

        // Snapshot slot fields we need for table lookups (avoids overlapping borrows).
        let blk_fnum = self.slot[i].blk_fnum;
        let patch_ws = self.slot[i].patch.ws;
        let patch_tl = usize::from(self.slot[i].patch.tl);
        let patch_kl = usize::from(self.slot[i].patch.kl);
        let patch_kr = usize::from(self.slot[i].patch.kr);
        let volume = self.slot[i].volume as usize;
        let is_carrier = self.slot[i].type_flags & 1 != 0;

        if requests & UPDATE_WS != 0 {
            self.slot[i].wave_table_idx = patch_ws;
        }
        if requests & UPDATE_TLL != 0 {
            let row = (blk_fnum >> 5) as usize;
            self.slot[i].tll = if is_carrier {
                self.tll_rks.tll_at(row, volume.min(63), patch_kl) as u16
            } else {
                self.tll_rks.tll_at(row, patch_tl, patch_kl) as u16
            };
        }
        if requests & UPDATE_RKS != 0 {
            let row = (blk_fnum >> 8) as usize;
            self.slot[i].rks = self.tll_rks.rks[row][patch_kr];
        }

        if requests & (UPDATE_RKS | UPDATE_EG) != 0 {
            let p_rate = Self::get_parameter_rate(&self.slot[i]);
            if p_rate == 0 {
                self.slot[i].eg_shift = 0;
                self.slot[i].eg_rate_h = 0;
                self.slot[i].eg_rate_l = 0;
                self.slot[i].update_requests = 0;
                return;
            }
            let rks_h2 = self.slot[i].rks >> 2;
            self.slot[i].eg_rate_h = (p_rate + rks_h2).min(15);
            self.slot[i].eg_rate_l = self.slot[i].rks & 3;
            let eg_state = self.slot[i].eg_state;
            let eg_rate_h = self.slot[i].eg_rate_h;
            self.slot[i].eg_shift = if eg_state == EgState::Attack {
                if eg_rate_h > 0 && eg_rate_h < 12 {
                    u32::from(13 - eg_rate_h)
                } else {
                    0
                }
            } else if eg_rate_h < 13 {
                u32::from(13 - eg_rate_h)
            } else {
                0
            };
        }

        self.slot[i].update_requests = 0;
    }

    /// Run one OPLL clock: advance EG counter, then for each of 18
    /// slots: commit pending updates → run envelope → run phase
    /// (emu2413.cpp:889-908).
    fn update_slots(&mut self) {
        self.eg_counter = self.eg_counter.wrapping_add(1);
        for i in 0..18 {
            if self.slot[i].update_requests != 0 {
                self.commit_slot_update(i);
            }
            // Buddy lookup for the rare Damp→Attack carrier transition.
            // For VRC7 melodic channels neither slot has pg_keep set so
            // this defaults to false; the rhythm path is non-VRC7.
            let buddy_pg_keep = if i & 1 == 0 {
                // modulator (even) — buddy = carrier at i+1
                self.slot.get(i + 1).is_some_and(|s| s.pg_keep != 0)
            } else {
                // carrier (odd) — buddy = modulator at i-1
                self.slot[i - 1].pg_keep != 0
            };
            let test = self.test_flag & 1;
            let step = self.slot[i].calc_envelope(buddy_pg_keep, self.eg_counter, test);
            if step == EnvelopeStep::ResetBuddyPhase {
                // Carrier just transitioned Damp→Attack; reset modulator.
                let buddy_idx = i ^ 1;
                self.slot[buddy_idx].pg_phase = 0;
            }
            let pm_phase_i32 = self.pm_phase as i32;
            let pg_reset = self.test_flag & 4 != 0;
            self.slot[i].calc_phase(pm_phase_i32, pg_reset);
        }
    }

    /// Generate one modulator-slot sample. The modulator's own previous
    /// output is fed back into its phase index per emu2413.cpp:938-948.
    fn calc_slot_mod(&mut self, ch: usize) -> i16 {
        let mod_idx = ch * 2;
        let fb = self.slot[mod_idx].patch.fb;
        let am = if self.slot[mod_idx].patch.am != 0 {
            self.lfo_am
        } else {
            0
        };

        let fm = if fb > 0 {
            // (output[0] + output[1]) >> (9 - FB)
            let sum = self.slot[mod_idx].output[0] + self.slot[mod_idx].output[1];
            (sum >> (9 - fb)) as i16
        } else {
            0
        };

        let pg_out = self.slot[mod_idx].pg_out;
        let wave_idx = self.slot[mod_idx].wave_table_idx;
        let phase = pg_out.wrapping_add(fm as u32) & (PG_WIDTH as u32 - 1);
        let h = self.waves.sample(wave_idx, phase);

        let out = to_linear(h, &self.slot[mod_idx], am);
        self.slot[mod_idx].output[1] = self.slot[mod_idx].output[0];
        self.slot[mod_idx].output[0] = i32::from(out);
        out
    }

    /// Generate one carrier-slot sample. The carrier takes the
    /// modulator's output as FM input per emu2413.cpp:927-936.
    fn calc_slot_car(&mut self, ch: usize, fm: i16) -> i16 {
        let car_idx = ch * 2 + 1;
        let am = if self.slot[car_idx].patch.am != 0 {
            self.lfo_am
        } else {
            0
        };

        // Phase index = pg_out + 2 * (fm >> 1), mask to PG_WIDTH.
        let pg_out = self.slot[car_idx].pg_out;
        let wave_idx = self.slot[car_idx].wave_table_idx;
        let phase_offset = (2 * i32::from(fm >> 1)) as u32;
        let phase = pg_out.wrapping_add(phase_offset) & (PG_WIDTH as u32 - 1);
        let h = self.waves.sample(wave_idx, phase);

        let out = to_linear(h, &self.slot[car_idx], am);
        self.slot[car_idx].output[1] = self.slot[car_idx].output[0];
        self.slot[car_idx].output[0] = i32::from(out);
        out
    }

    /// Drive one chip-clock of output: LFO + slots + 6 channel outputs
    /// (VRC7 path of emu2413.cpp:996-1058; rhythm path elided).
    fn update_output(&mut self) {
        self.update_ampm();
        self.update_slots();
        // VRC7 melodic channels (6 channels, slots 0..12).
        for ch in 0..6 {
            let fm = self.calc_slot_mod(ch);
            let car_out = self.calc_slot_car(ch, fm);
            // _MO(x) = -(x) >> 1
            self.ch_out[ch] = ((-i32::from(car_out)) >> 1) as i16;
        }
    }

    /// Sum the 6 VRC7 channel outputs into `mix_out`
    /// (emu2413.cpp:1060-1077, chip_type != 0 path).
    fn mix_output(&mut self) {
        let mut sum: i32 = 0;
        for ch in 0..6 {
            sum += i32::from(self.ch_out[ch]);
        }
        self.mix_out = sum.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16;
    }

    // ---- Test / register-write helpers (used by Sprint 1.2 register
    // decoder + by unit tests in this sprint) ----

    /// Assign the patch at `num` to channel `ch` and request
    /// recomputation of all derived slot state. Mirrors `set_patch`
    /// in emu2413.cpp:645-651.
    #[cfg(test)]
    fn set_patch(&mut self, ch: usize, num: usize) {
        self.patch_number[ch] = num as i32;
        self.slot[ch * 2].patch = self.patch_set[num * 2];
        self.slot[ch * 2 + 1].patch = self.patch_set[num * 2 + 1];
        self.slot[ch * 2].update_requests |= UPDATE_ALL;
        self.slot[ch * 2 + 1].update_requests |= UPDATE_ALL;
    }

    /// Set channel `ch`'s 9-bit fnum on both modulator and carrier.
    /// Mirrors `set_fnumber` in emu2413.cpp:673-683.
    #[cfg(test)]
    fn set_fnumber(&mut self, ch: usize, fnum: u16) {
        let car = ch * 2 + 1;
        let mod_ = ch * 2;
        self.slot[car].fnum = fnum & 0x1ff;
        self.slot[car].blk_fnum = (self.slot[car].blk_fnum & 0xe00) | (fnum & 0x1ff);
        self.slot[mod_].fnum = fnum & 0x1ff;
        self.slot[mod_].blk_fnum = (self.slot[mod_].blk_fnum & 0xe00) | (fnum & 0x1ff);
        self.slot[car].update_requests |= UPDATE_EG | UPDATE_RKS | UPDATE_TLL;
        self.slot[mod_].update_requests |= UPDATE_EG | UPDATE_RKS | UPDATE_TLL;
    }

    /// Set channel `ch`'s 3-bit block on both modulator and carrier.
    /// Mirrors `set_block` in emu2413.cpp:685-695.
    #[cfg(test)]
    fn set_block(&mut self, ch: usize, blk: u8) {
        let car = ch * 2 + 1;
        let mod_ = ch * 2;
        let blk_low3 = blk & 7;
        self.slot[car].blk = blk_low3;
        self.slot[car].blk_fnum = (u16::from(blk_low3) << 9) | (self.slot[car].blk_fnum & 0x1ff);
        self.slot[mod_].blk = blk_low3;
        self.slot[mod_].blk_fnum = (u16::from(blk_low3) << 9) | (self.slot[mod_].blk_fnum & 0x1ff);
        self.slot[car].update_requests |= UPDATE_EG | UPDATE_RKS | UPDATE_TLL;
        self.slot[mod_].update_requests |= UPDATE_EG | UPDATE_RKS | UPDATE_TLL;
    }

    /// Set channel `ch`'s carrier volume (6-bit, post-`<< 2` from
    /// register data). Mirrors `set_volume` in emu2413.cpp:663-666.
    #[cfg(test)]
    fn set_volume(&mut self, ch: usize, volume: i32) {
        let car = ch * 2 + 1;
        self.slot[car].volume = volume;
        self.slot[car].update_requests |= UPDATE_TLL;
    }

    /// Key on channel `ch` (start envelope from Damp). Mirrors
    /// `slotOn` per `update_key_status` (emu2413.cpp:584-589).
    #[cfg(test)]
    fn key_on(&mut self, ch: usize) {
        for slot_idx in [ch * 2, ch * 2 + 1] {
            self.slot[slot_idx].key_flag = 1;
            self.slot[slot_idx].eg_state = EgState::Damp;
            self.slot[slot_idx].update_requests |= UPDATE_EG;
        }
    }
}

// ---------------------------------------------------------------------------
// Tests — verify the static tables match the C source byte-for-byte.
// These tests run unconditionally (no feature gate) since the OPLL
// constants are pure data and the tests are cheap.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exp_table_size_matches_c_source() {
        assert_eq!(EXP_TABLE.len(), 256);
        // Spot-check: emu2413.cpp line 138 value at index 0 is 0
        assert_eq!(EXP_TABLE[0], 0);
        // Index 255 = round((exp2(255/256) - 1) * 1024) = 1018
        assert_eq!(EXP_TABLE[255], 1018);
    }

    #[test]
    fn fullsin_table_quarter_matches_c_source() {
        assert_eq!(FULLSIN_TABLE_QUARTER.len(), 256);
        // emu2413.cpp line 157 first value is 2137 (largest log-sin)
        assert_eq!(FULLSIN_TABLE_QUARTER[0], 2137);
        // Last value tapers to 0 (sin → 1, -log2(1) → 0)
        assert_eq!(FULLSIN_TABLE_QUARTER[255], 0);
    }

    #[test]
    fn patch_dump_sizes_match_19_x_8() {
        assert_eq!(DEFAULT_INST_YM2413.len(), 19 * 8);
        assert_eq!(DEFAULT_INST_VRC7.len(), 19 * 8);
        assert_eq!(DEFAULT_INST_YMF281B.len(), 19 * 8);
    }

    #[test]
    fn vrc7_patch_0_is_user_patch_all_zeros() {
        // The user-defined patch slot is always zeroed in the dump.
        // Lagrange Point uses patches 1-15 plus user patch via $00-$07.
        for b in &DEFAULT_INST_VRC7[0..8] {
            assert_eq!(*b, 0);
        }
    }

    #[test]
    fn vrc7_patch_1_matches_nuke_ykt_reference() {
        // emu2413.cpp line 65 (VRC7 patch 1 from Nuke.YKT analysis).
        assert_eq!(
            DEFAULT_INST_VRC7[8..16],
            [0x03, 0x21, 0x05, 0x06, 0xe8, 0x81, 0x42, 0x27]
        );
    }

    #[test]
    fn pm_table_first_row_is_all_zero() {
        // emu2413.cpp line 182: fnum=000xxxxxx row is no pitch modulation.
        assert_eq!(PM_TABLE[0], [0, 0, 0, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn am_table_length_matches_c_source() {
        // emu2413.cpp declares uint8_t am_table[210].
        assert_eq!(AM_TABLE.len(), 210);
        assert_eq!(AM_TABLE[0], 0);
        // Peak is 13 (per the C source comment, "13, 13, 13").
        let peak = *AM_TABLE.iter().max().unwrap();
        assert_eq!(peak, 13);
    }

    #[test]
    fn ml_table_matches_c_source() {
        assert_eq!(ML_TABLE.len(), 16);
        // emu2413.cpp line 222: first entry is 1 (not doubled).
        assert_eq!(ML_TABLE[0], 1);
        assert_eq!(ML_TABLE[1], 2);
        assert_eq!(ML_TABLE[14], 30);
        assert_eq!(ML_TABLE[15], 30);
    }

    #[test]
    fn constants_match_emu2413_defines() {
        assert_eq!(PG_BITS, 10);
        assert_eq!(PG_WIDTH, 1024);
        assert_eq!(DP_BITS, 19);
        assert_eq!(DP_BASE_BITS, 9);
        assert_eq!(EG_BITS, 7);
        assert_eq!(EG_MUTE, 127);
        assert_eq!(EG_MAX, 123);
        assert_eq!(TL_BITS, 6);
        assert_eq!(tl_to_eg(5), 10);
    }

    #[test]
    fn new_vrc7_opll_has_vrc7_chip_type() {
        let opll = Opll::new(ChipType::Vrc7);
        assert_eq!(opll.chip_type(), ChipType::Vrc7);
    }

    #[test]
    fn calc_returns_zero_in_scaffold_stage() {
        // ADR-0004 deferred behavior is preserved in v1.1.0-rc:
        // calc() returns 0 until the DSP lands.
        let mut opll = Opll::new(ChipType::Vrc7);
        opll.write_reg(0x30, 0x01);
        opll.write_reg(0x10, 0x80);
        opll.write_reg(0x20, 0x15);
        for _ in 0..100 {
            assert_eq!(opll.calc(), 0);
        }
    }

    #[test]
    fn register_shadow_round_trips() {
        let mut opll = Opll::new(ChipType::Vrc7);
        opll.write_reg(0x10, 0xAB);
        opll.write_reg(0x30, 0x4F);
        assert_eq!(opll.read_reg(0x10), 0xAB);
        assert_eq!(opll.read_reg(0x30), 0x4F);
        // emu2413.cpp:1226 — write_reg silently ignores addresses
        // >= 0x40 (no wrap, no fault). Verify the high-byte write
        // does NOT bleed into register 0x00.
        let pre = opll.read_reg(0x00);
        opll.write_reg(0x80, 0x12);
        assert_eq!(
            opll.read_reg(0x00),
            pre,
            "writes to reg >= 0x40 must be ignored, not masked"
        );
        // Mirror registers ($19-$1F → $10-$16, etc.): writing 0x19
        // should land at 0x10 per emu2413.cpp:1230-1232.
        opll.write_reg(0x19, 0xCC);
        assert_eq!(
            opll.read_reg(0x10),
            0xCC,
            "register 0x19 should mirror to 0x10"
        );
    }

    #[test]
    fn reset_clears_register_shadow() {
        let mut opll = Opll::new(ChipType::Vrc7);
        opll.write_reg(0x10, 0xFF);
        opll.write_reg(0x20, 0xAA);
        opll.reset();
        assert_eq!(opll.read_reg(0x10), 0);
        assert_eq!(opll.read_reg(0x20), 0);
    }

    #[test]
    fn patch_set_loaded_from_vrc7_dump() {
        let opll = Opll::new(ChipType::Vrc7);
        // VRC7 patch 1 modulator decode: dump[8..16] = 0x03,0x21,0x05,0x06,...
        // ML = dump[0] & 0x0f = 0x03 & 0x0f = 3
        let p1_mod = opll.patch_set[2]; // patch 1 modulator = index 2
        assert_eq!(p1_mod.ml, 3);
    }

    // -----------------------------------------------------------------------
    // Phase generator tests — verify calc_phase against the emu2413.cpp:765
    // closed-form. The expected values are hand-derived from the C formula
    // so any deviation (sign, mask, shift order) trips the assertion.
    // -----------------------------------------------------------------------

    fn fresh_slot(number: u8) -> Slot {
        let mut s = Slot::default();
        s.reset_to_release(number);
        s
    }

    #[test]
    fn calc_phase_zero_fnum_zero_increment() {
        let mut s = fresh_slot(0);
        s.fnum = 0;
        s.blk = 0;
        s.patch.ml = 0; // ML_TABLE[0] = 1
        s.patch.pm = 0;
        s.calc_phase(0, false);
        // Increment = (0 * 2 + 0) * 1 = 0; phase stays at 0.
        assert_eq!(s.pg_phase, 0);
        assert_eq!(s.pg_out, 0);
    }

    #[test]
    fn calc_phase_no_pm_one_step() {
        let mut s = fresh_slot(0);
        s.fnum = 0x100; // 256
        s.blk = 2; // shift << 2
        s.patch.ml = 2; // ML_TABLE[2] = 4
        s.patch.pm = 0;
        // Increment per C: ((256 * 2 + 0) * 4) << 2 >> 2 = 2048.
        s.calc_phase(0, false);
        assert_eq!(s.pg_phase, 2048);
        // pg_out = pg_phase >> DP_BASE_BITS = 2048 >> 9 = 4.
        assert_eq!(s.pg_out, 4);
    }

    #[test]
    fn calc_phase_pm_offset_applied_when_patch_pm_set() {
        let mut s = fresh_slot(0);
        s.fnum = 0x080; // fnum_row = (128 >> 6) & 7 = 2
        s.blk = 0;
        s.patch.ml = 0; // ML_TABLE[0] = 1
        s.patch.pm = 1;
        // pm_phase >> 10 = 2 → col 2 → pm_table[2][2] = 2.
        // Increment = (128*2 + 2) * 1 = 258. Shifted << 0 >> 2 = 64.
        s.calc_phase(2 << 10, false);
        assert_eq!(s.pg_phase, 64);
    }

    #[test]
    fn calc_phase_pm_disabled_yields_pm_zero() {
        let mut s = fresh_slot(0);
        s.fnum = 0x080;
        s.patch.ml = 0;
        s.patch.pm = 0;
        // Same pm_phase as previous, but pm bit OFF: increment = (128*2)*1 = 256.
        s.calc_phase(2 << 10, false);
        // 256 >> 2 = 64.
        assert_eq!(s.pg_phase, 64);
    }

    #[test]
    fn calc_phase_dp_width_wraps_modulo() {
        let mut s = fresh_slot(0);
        s.pg_phase = DP_WIDTH - 4;
        s.fnum = 0x040;
        s.blk = 1; // (64*2 * 1) << 1 = 256; >> 2 = 64.
        s.patch.ml = 0;
        s.patch.pm = 0;
        s.calc_phase(0, false);
        // (DP_WIDTH - 4 + 64) mod DP_WIDTH = 60.
        assert_eq!(s.pg_phase, 60);
    }

    #[test]
    fn calc_phase_reset_zeros_phase_before_increment() {
        let mut s = fresh_slot(0);
        s.pg_phase = 12345;
        s.fnum = 0;
        s.patch.ml = 0;
        s.patch.pm = 0;
        s.calc_phase(0, true);
        // reset → pg_phase = 0; increment = 0 → stays 0.
        assert_eq!(s.pg_phase, 0);
    }

    // -----------------------------------------------------------------------
    // Envelope generator tests — exercise reset → key-on → Damp → Attack →
    // Decay → Sustain transitions via direct slot mutation. The reference
    // values come from manually tracing emu2413.cpp:817-887 with specific
    // patch parameters.
    // -----------------------------------------------------------------------

    #[test]
    fn reset_to_release_puts_slot_at_eg_mute() {
        let mut s = Slot::default();
        s.reset_to_release(3);
        assert_eq!(s.eg_state, EgState::Release);
        assert_eq!(s.eg_out, EG_MUTE);
        assert_eq!(s.number, 3);
        assert_eq!(s.type_flags, 1); // 3 % 2 = 1 (carrier)
    }

    #[test]
    fn start_envelope_saturated_ar_skips_to_decay_at_zero() {
        let mut s = fresh_slot(0);
        s.patch.ar = 15;
        s.rks = 0;
        s.eg_out = 50;
        s.start_envelope();
        assert_eq!(s.eg_state, EgState::Decay);
        assert_eq!(s.eg_out, 0);
    }

    #[test]
    fn start_envelope_non_saturated_ar_enters_attack() {
        let mut s = fresh_slot(0);
        s.patch.ar = 8;
        s.rks = 0;
        s.eg_out = 50;
        s.start_envelope();
        assert_eq!(s.eg_state, EgState::Attack);
        // eg_out preserved on Attack entry (only saturated AR zeros it).
        assert_eq!(s.eg_out, 50);
    }

    #[test]
    fn start_envelope_rks_contributes_to_effective_ar() {
        let mut s = fresh_slot(0);
        s.patch.ar = 12;
        s.rks = 12; // rks >> 2 = 3 → effective = 15 → saturate.
        s.start_envelope();
        assert_eq!(s.eg_state, EgState::Decay);
        assert_eq!(s.eg_out, 0);
    }

    #[test]
    fn calc_envelope_decay_to_sustain_at_sl_match() {
        // Decay → Sustain transition is checked unconditionally
        // (emu2413.cpp:870-875 — NOT synchronized with eg_counter mask).
        let mut s = fresh_slot(0);
        s.eg_state = EgState::Decay;
        s.eg_out = 32;
        s.patch.sl = 4; // 32 >> 3 = 4 → match.
        s.eg_rate_h = 0; // No decrement applied.
        let step = s.calc_envelope(false, 0, 0);
        assert_eq!(s.eg_state, EgState::Sustain);
        assert_eq!(step, EnvelopeStep::Continue);
    }

    #[test]
    fn calc_envelope_attack_to_decay_when_eg_out_hits_zero() {
        let mut s = fresh_slot(0);
        s.eg_state = EgState::Attack;
        s.eg_out = 0; // Already at min attenuation = max volume.
        s.eg_rate_h = 0;
        s.calc_envelope(false, 0, 0);
        assert_eq!(s.eg_state, EgState::Decay);
    }

    #[test]
    fn calc_envelope_damp_to_attack_via_carrier_buddy_reset() {
        let mut s = fresh_slot(1); // Carrier (odd index, type & 1 == 1)
        s.eg_state = EgState::Damp;
        s.eg_out = EG_MAX;
        s.eg_shift = 0; // mask = 0; (eg_counter & 0) == 0 always.
        s.patch.ar = 8;
        s.rks = 0;
        s.pg_keep = 0;
        s.pg_phase = 0x1234;
        let step = s.calc_envelope(false, 0, 0);
        // Damp → Attack via start_envelope; carrier resets pg_phase
        // and signals buddy reset to caller.
        assert_eq!(s.eg_state, EgState::Attack);
        assert_eq!(s.pg_phase, 0);
        assert_eq!(step, EnvelopeStep::ResetBuddyPhase);
    }

    #[test]
    fn calc_envelope_test_flag_zeros_eg_out_each_tick() {
        let mut s = fresh_slot(0);
        s.eg_state = EgState::Sustain;
        s.eg_out = 64;
        s.calc_envelope(false, 0, 1);
        assert_eq!(s.eg_out, 0);
    }

    #[test]
    fn lookup_decay_step_rate_15_returns_2() {
        let mut s = fresh_slot(0);
        s.eg_rate_h = 15;
        assert_eq!(s.lookup_decay_step(0), 2);
        assert_eq!(s.lookup_decay_step(0xff), 2);
    }

    #[test]
    fn lookup_attack_step_rate_0_returns_0() {
        let mut s = fresh_slot(0);
        s.eg_rate_h = 0;
        s.eg_rate_l = 0;
        assert_eq!(s.lookup_attack_step(0), 0);
        s.eg_rate_h = 15;
        assert_eq!(s.lookup_attack_step(0), 0);
    }

    #[test]
    fn lookup_attack_step_rate_12_uses_eg_step_table_complement() {
        // Direct port verification: at rate 12, value is `4 - EG_STEP_TABLES[L][index]`.
        let mut s = fresh_slot(0);
        s.eg_rate_h = 12;
        s.eg_rate_l = 0;
        // counter = 0 → index = (0 & 0xc) >> 1 = 0 → table[0][0] = 0 → 4 - 0 = 4.
        assert_eq!(s.lookup_attack_step(0), 4);
        // counter = 2 → index = (2 & 0xc) >> 1 = 0 → 4.
        assert_eq!(s.lookup_attack_step(2), 4);
        // counter = 4 → index = (4 & 0xc) >> 1 = 2 → table[0][2] = 0 → 4.
        assert_eq!(s.lookup_attack_step(4), 4);
        s.eg_rate_l = 3; // table[3] = [0,1,1,1,1,1,1,1]
        // counter = 4 → index = 2 → table[3][2] = 1 → 4 - 1 = 3.
        assert_eq!(s.lookup_attack_step(4), 3);
    }

    // -----------------------------------------------------------------------
    // Wave table + exp/to_linear tests — verify table construction and the
    // log-to-linear decode produces emu2413-compatible values.
    // -----------------------------------------------------------------------

    #[test]
    fn wave_tables_first_quarter_matches_quarter_lut() {
        let w = WaveTables::new();
        for (x, &expected) in FULLSIN_TABLE_QUARTER.iter().enumerate() {
            assert_eq!(w.fullsin[x], expected);
        }
    }

    #[test]
    fn wave_tables_second_quarter_mirrors_first() {
        let w = WaveTables::new();
        // fullsin[256 + x] == fullsin[256 - x - 1]
        for x in 0..256 {
            assert_eq!(w.fullsin[256 + x], w.fullsin[256 - x - 1]);
        }
        // Endpoints: fullsin[256] (start of mirror) == fullsin[255] (last of first quarter).
        assert_eq!(w.fullsin[256], FULLSIN_TABLE_QUARTER[255]);
        // fullsin[511] (last of mirror) == fullsin[0] (first of first quarter).
        assert_eq!(w.fullsin[511], FULLSIN_TABLE_QUARTER[0]);
    }

    #[test]
    fn wave_tables_second_half_has_sign_bit_set() {
        let w = WaveTables::new();
        for x in 0..512 {
            assert_eq!(w.fullsin[512 + x], 0x8000 | w.fullsin[x]);
        }
    }

    #[test]
    fn halfsin_first_half_matches_fullsin_second_half_is_mute() {
        let w = WaveTables::new();
        for x in 0..512 {
            assert_eq!(w.halfsin[x], w.fullsin[x]);
        }
        for x in 512..1024 {
            assert_eq!(w.halfsin[x], 0xfff);
        }
    }

    #[test]
    fn lookup_exp_table_positive_low_input_is_near_zero() {
        // i = 0x7fff (max positive log-magnitude before sign bit) → ~0.
        let v = lookup_exp_table(0x7f00);
        assert!(v.unsigned_abs() < 100, "got {v}");
    }

    #[test]
    fn lookup_exp_table_signed_negates_via_bitwise_not() {
        // Same magnitude with sign bit set produces a sign-flipped value.
        let pos = lookup_exp_table(0x0010);
        let neg = lookup_exp_table(0x8010);
        // ~x in C; ~res when res is positive → ~res = -res - 1; << 1 doubles.
        // The relationship is: signed = !res; ((!res) << 1) == -(res << 1) - 2.
        // So neg ≈ -pos - 2 (depending on rounding).
        let diff = i32::from(neg) + i32::from(pos) + 2;
        assert!(diff.abs() <= 2, "pos={pos}, neg={neg}");
    }

    #[test]
    fn to_linear_returns_zero_when_eg_out_above_eg_max() {
        let mut slot = fresh_slot(0);
        slot.eg_out = EG_MUTE; // > EG_MAX (123)
        slot.tll = 0;
        assert_eq!(to_linear(0, &slot, 0), 0);
    }

    #[test]
    fn to_linear_zero_attenuation_yields_max_magnitude() {
        let mut slot = fresh_slot(0);
        slot.eg_out = 0;
        slot.tll = 0;
        // h = 0 → att = 0 → lookup_exp_table(0) ≈ +/- large magnitude.
        let out = to_linear(0, &slot, 0);
        assert!(out.unsigned_abs() > 1000, "expected loud, got {out}");
    }

    // -----------------------------------------------------------------------
    // TLL + RKS table tests
    // -----------------------------------------------------------------------

    #[test]
    fn tll_kl_zero_is_just_tl_doubled() {
        let t = TllRksTables::new();
        // KL=0 → TLL = TL2EG(TL) = TL * 2 for every block/fnum row.
        for block in 0..8 {
            for fnum in 0..16 {
                let row = (block << 4) | fnum;
                for tl in 0..64 {
                    assert_eq!(t.tll_at(row, tl, 0), tl_to_eg(tl as u32));
                }
            }
        }
    }

    #[test]
    fn rks_kr_zero_is_block_shifted_right_two() {
        let t = TllRksTables::new();
        for block in 0..8 {
            for fnum8 in 0..2 {
                let idx = (block << 1) | fnum8;
                assert_eq!(t.rks[idx][0], (block >> 1) as u8);
                assert_eq!(t.rks[idx][1], ((block << 1) + fnum8) as u8);
            }
        }
    }

    // -----------------------------------------------------------------------
    // LFO + operator + per-channel update tests
    // -----------------------------------------------------------------------

    #[test]
    fn update_ampm_advances_am_phase_and_loads_lfo_am() {
        let mut opll = Opll::new(ChipType::Vrc7);
        let am0 = opll.lfo_am;
        // Drive 64 cycles — am_table index = (am_phase >> 6) — should advance by 1.
        for _ in 0..64 {
            opll.update_ampm();
        }
        let am1 = opll.lfo_am;
        // After 64 ticks the table index advances, so the value may
        // have changed (or stayed if next sample is the same).
        // Stronger assertion: drive 64 * 14 (one full peak-to-peak cycle)
        // and verify the LFO has visited multiple distinct values.
        let mut seen = alloc::vec::Vec::new();
        seen.push(am0);
        seen.push(am1);
        for _ in 0..(64 * 14) {
            opll.update_ampm();
            seen.push(opll.lfo_am);
        }
        let max = seen.iter().copied().max().unwrap();
        let min = seen.iter().copied().min().unwrap();
        assert!(max > min, "LFO did not sweep; seen min={min} max={max}");
        assert!(max <= 13, "LFO max should be 13 per AM_TABLE; got {max}");
    }

    #[test]
    fn update_ampm_test_bit_1_resets_phases() {
        let mut opll = Opll::new(ChipType::Vrc7);
        for _ in 0..1000 {
            opll.update_ampm();
        }
        opll.test_flag = 0b10;
        opll.update_ampm();
        assert_eq!(opll.pm_phase, 0);
        assert_eq!(opll.am_phase, 0);
    }

    #[test]
    fn opll_calc_silent_with_no_key_on() {
        let mut opll = Opll::new(ChipType::Vrc7);
        // Default state — all slots in Release, eg_out=EG_MUTE → mix_out = 0.
        for _ in 0..100 {
            assert_eq!(opll.calc(), 0);
        }
    }

    #[test]
    fn opll_calc_runs_full_pipeline_advances_eg_counter() {
        let mut opll = Opll::new(ChipType::Vrc7);
        assert_eq!(opll.eg_counter, 0);
        opll.calc();
        assert_eq!(opll.eg_counter, 1);
        for _ in 0..99 {
            opll.calc();
        }
        assert_eq!(opll.eg_counter, 100);
    }

    #[test]
    fn opll_keyed_on_channel_produces_nonzero_output_within_one_envelope() {
        let mut opll = Opll::new(ChipType::Vrc7);
        // Channel 0: assign VRC7 patch 1 (a non-zero instrument),
        // set frequency, key on. Then run enough cycles for the
        // envelope to traverse Damp → Attack → audible level.
        opll.set_patch(0, 1);
        opll.set_block(0, 4);
        opll.set_fnumber(0, 256);
        opll.set_volume(0, 0); // max volume (volume is attenuation; 0 = loud)
        opll.key_on(0);
        let mut peak_abs: i16 = 0;
        // Run ~16k cycles (≈ 330 ms at 49716 Hz) — should clear Damp
        // and reach Attack/Decay.
        for _ in 0..16_384 {
            let s = opll.calc();
            peak_abs = peak_abs.max(s.unsigned_abs() as i16);
        }
        assert!(
            peak_abs > 0,
            "expected non-silent output after key-on; peak_abs = {peak_abs}"
        );
    }

    #[test]
    fn opll_reset_initializes_all_18_slots_to_release() {
        let mut opll = Opll::new(ChipType::Vrc7);
        // Mutate a slot to verify reset() truly resets it.
        opll.slot[0].eg_out = 0;
        opll.slot[0].eg_state = EgState::Attack;
        opll.reset();
        for (i, s) in opll.slot.iter().enumerate() {
            assert_eq!(s.eg_state, EgState::Release, "slot {i} eg_state");
            assert_eq!(s.eg_out, EG_MUTE, "slot {i} eg_out");
            assert_eq!(s.number, i as u8, "slot {i} number");
            assert_eq!(s.type_flags & 1, (i & 1) as u8, "slot {i} M/C bit");
        }
    }
}
