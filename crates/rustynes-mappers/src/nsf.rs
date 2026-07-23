//! NSF music-file player (v1.1.0 beta.2, Workstream D, T-110-D1; non-60-Hz +
//! `NSFE` support added in the v2.1.x "Fathom" line).
//!
//! Both containers are parsed: the classic `NESM\x1a` header and the extended
//! chunked `NSFE` (see [`parse_nsfe`]), dispatched by [`parse_nsf`] on the magic.
//! Expansion-chip audio (VRC6/7, FDS, MMC5, N163,
//! Sunsoft 5B) declared in the `$07B` bitfield IS synthesized — the NSF
//! mapper routes the expansion register windows into the existing cartridge
//! synth cores via [`crate::nsf_expansion`] (v1.7.0 "Forge" G2/G3).
//!
//! An `.nsf` file is a ripped NES music engine plus a small header describing
//! how to drive it: a `load`/`init`/`play` address triplet, a song count, and
//! optional 4 KiB bank-switching. There is no PPU program — the file is *only*
//! sound code.
//!
//! Rather than invent a bespoke "run init / run play" execution mode in the
//! core (which would need its own determinism story), this module follows the
//! Mesen2 / FCEUX / rustico approach: a synthetic 6502 **driver** ("BIOS") is
//! mapped into otherwise-unused address space, and the standard reset / NMI
//! vectors are pointed at it. The driver calls `init` once (with the selected
//! song in A and the NTSC/PAL flag in X), then spins. **Play rate:** at the
//! standard 60 Hz the driver enables vblank NMI and the PPU's ordinary 60 Hz
//! vblank NMI calls `play` once per frame (the original, byte-identical path).
//! At a **non-standard rate** (a PAL 50 Hz tune, or any custom µs divider, on
//! the NTSC console — see [`Nsf::nonstandard_play_period_cycles`]) the driver
//! instead disables the APU frame IRQ and arms a mapper **cycle-timer** that
//! raises a (level-triggered, `$5FF1`-acked) IRQ every `period` CPU cycles; the
//! IRQ handler calls `play`. Either way it runs through the unchanged
//! `Nes::run_frame` lockstep loop, so the APU produces audio exactly as it does
//! for a cartridge and the determinism contract is untouched.
//!
//! Scope: the base 2A03 APU, standard `$5FF8-$5FFF` `$8000-$FFFF` 4 KiB
//! bank-switching, NTSC / PAL / custom play rates, and expansion-chip audio
//! (VRC6/7, MMC5, N163, Sunsoft 5B, FDS) routed into the existing synth cores.
//! The FDS-style `$5FF6/$5FF7` RAM banking is deferred (documented).

use crate::cartridge::Mirroring;
use crate::mapper::{Mapper, MapperCaps, MapperError, MapperFrameEvents};
use crate::nsf_expansion::NsfExpansion;
use alloc::{boxed::Box, format, vec, vec::Vec};

/// Header length of a classic NSF file (`NESM` form).
const NSF_HEADER_LEN: usize = 0x80;

/// `NESM\x1A` magic.
const NSF_MAGIC: &[u8; 5] = b"NESM\x1A";

/// Address the synthetic driver ("BIOS") is mapped at. `$5000` is in the
/// `$4020-$5FFF` expansion window that base NSFs never touch (the only thing
/// down here for a 2A03 tune is the `$5FF8-$5FFF` bank registers).
const DRIVER_BASE: u16 = 0x5000;

/// Offset of the song-number immediate operand inside the driver image (see the
/// driver listing in [`NsfMapper::build_driver`]). Re-patched on track change.
const DRIVER_SONG_OPERAND: usize = 0x06;

/// Entry points within the driver (absolute addresses).
const DRIVER_INIT_ENTRY: u16 = DRIVER_BASE; // reset vector target
const DRIVER_NMI_ENTRY: u16 = DRIVER_BASE + 0x15; // NMI vector target
const DRIVER_IRQ_ENTRY: u16 = DRIVER_BASE + 0x23; // IRQ vector target (RTI stub)

/// Parsed NSF header + program image.
#[derive(Debug, Clone)]
pub struct Nsf {
    /// Total number of songs in the file (1-based count).
    pub total_songs: u8,
    /// The 1-based song the file wants to start on.
    pub starting_song: u8,
    /// 16-bit load address of the program image.
    pub load_addr: u16,
    /// `init` routine address (called once per track with A = song index).
    pub init_addr: u16,
    /// `play` routine address (called once per frame).
    pub play_addr: u16,
    /// `true` when the file uses `$5FF8-$5FFF` 4 KiB bank switching.
    pub bankswitched: bool,
    /// Initial bank register values (`$070-$077` of the header).
    pub initial_banks: [u8; 8],
    /// Expansion-chip bitfield (`$07B`). Routed into the matching synth cores
    /// by the `nsf_expansion` module when any bit is set (v1.7.0 G2/G3).
    pub expansion: u8,
    /// `true` when the file declares a PAL or dual-region timing preference.
    pub pal: bool,
    /// NTSC play-speed divider (header `$6E-$6F`), in microseconds per `play`
    /// call. `0` means "use the hardware default" (≈16639 µs ≈ 60.0988 Hz).
    /// Classic `NESM` only; `NSFe` has no µs word and derives the rate from region.
    pub play_speed_ntsc: u16,
    /// PAL play-speed divider (header `$78-$79`), in microseconds per `play`
    /// call. `0` means the PAL hardware default (≈19997 µs ≈ 50.007 Hz).
    pub play_speed_pal: u16,
    /// The program image, padded + 4 KiB-aligned when bank-switched.
    pub prg: Vec<u8>,
    /// UTF-8-lossy song / artist / copyright strings (trimmed at the first NUL).
    pub song_name: Box<str>,
    /// Artist name.
    pub artist: Box<str>,
    /// Copyright holder.
    pub copyright: Box<str>,
}

fn read_u16(bytes: &[u8], off: usize) -> u16 {
    u16::from(bytes[off]) | (u16::from(bytes[off + 1]) << 8)
}

fn read_string(bytes: &[u8], off: usize) -> Box<str> {
    let raw = &bytes[off..off + 32];
    let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
    alloc::string::String::from_utf8_lossy(&raw[..end])
        .into_owned()
        .into_boxed_str()
}

/// `NSFE` magic (the extended chunked container).
const NSFE_MAGIC: &[u8; 4] = b"NSFE";

/// Detect the classic `NESM` magic at the start of `bytes`.
#[must_use]
pub fn is_nesm(bytes: &[u8]) -> bool {
    bytes.len() >= NSF_MAGIC.len() && &bytes[0..NSF_MAGIC.len()] == NSF_MAGIC
}

/// Detect the extended `NSFE` magic at the start of `bytes`.
#[must_use]
pub fn is_nsfe(bytes: &[u8]) -> bool {
    bytes.len() >= NSFE_MAGIC.len() && &bytes[0..NSFE_MAGIC.len()] == NSFE_MAGIC
}

/// Detect an NSF-family music file (classic `NESM` **or** extended `NSFE`).
#[must_use]
pub fn is_nsf(bytes: &[u8]) -> bool {
    is_nesm(bytes) || is_nsfe(bytes)
}

/// Parse an NSF-family music file — classic `NESM` **or** extended `NSFE`.
///
/// Dispatches on the container magic; both fill the same [`Nsf`]. `NSFe` carries
/// its play rate through the region flags (no µs divider), so its
/// [`Nsf::play_speed_ntsc`]/[`Nsf::play_speed_pal`] stay `0` and
/// [`Nsf::effective_speed_us`] resolves them to the region default.
///
/// # Errors
///
/// Returns a [`MapperError::Invalid`] when the magic is wrong, the file is
/// truncated/short, or the header/chunks are internally inconsistent.
pub fn parse_nsf(bytes: &[u8]) -> Result<Nsf, MapperError> {
    if is_nsfe(bytes) {
        return parse_nsfe(bytes);
    }
    if bytes.len() < NSF_HEADER_LEN {
        return Err(MapperError::Invalid(format!(
            "NSF file is shorter than the {NSF_HEADER_LEN}-byte header ({} bytes)",
            bytes.len()
        )));
    }
    if !is_nsf(bytes) {
        return Err(MapperError::Invalid(
            "NSF magic bytes do not match \"NESM\\x1A\"".into(),
        ));
    }

    let total_songs = bytes[0x06];
    let starting_song = bytes[0x07];
    let load_addr = read_u16(bytes, 0x08);
    let init_addr = read_u16(bytes, 0x0A);
    let play_addr = read_u16(bytes, 0x0C);
    let mut initial_banks = [0u8; 8];
    initial_banks.copy_from_slice(&bytes[0x70..0x78]);
    let bankswitched = initial_banks.iter().any(|&b| b != 0);
    let expansion = bytes[0x7B];
    // PAL/NTSC selection byte ($07A): bit0 = PAL, bit1 = dual. Treat either as
    // "not strictly NTSC" for the init-register flag AND for picking which
    // play-speed divider drives the (now non-60-Hz-capable) player.
    let pal = bytes[0x7A] & 0b11 != 0;
    // Play-speed dividers ($6E-$6F NTSC, $78-$79 PAL), microseconds per `play`.
    let play_speed_ntsc = read_u16(bytes, 0x6E);
    let play_speed_pal = read_u16(bytes, 0x78);

    if total_songs == 0 {
        return Err(MapperError::Invalid("NSF declares zero songs".into()));
    }
    if load_addr < 0x6000 {
        return Err(MapperError::Invalid(format!(
            "NSF load address ${load_addr:04X} is below $6000"
        )));
    }

    let mut prg: Vec<u8> = bytes[NSF_HEADER_LEN..].to_vec();

    if bankswitched {
        // Pad the front so that bank 0 begins at `load_addr & 0x0FFF`, then
        // round the whole image up to a 4 KiB bank boundary (rustico/Mesen2).
        let pad = (load_addr & 0x0FFF) as usize;
        let mut image = vec![0u8; pad];
        image.extend_from_slice(&prg);
        if !image.len().is_multiple_of(0x1000) {
            image.resize(image.len() + (0x1000 - image.len() % 0x1000), 0);
        }
        prg = image;
    }

    Ok(Nsf {
        total_songs,
        starting_song,
        load_addr,
        init_addr,
        play_addr,
        bankswitched,
        initial_banks,
        expansion,
        pal,
        play_speed_ntsc,
        play_speed_pal,
        prg,
        song_name: read_string(bytes, 0x0E),
        artist: read_string(bytes, 0x2E),
        copyright: read_string(bytes, 0x4E),
    })
}

/// NTSC CPU clock (Hz) — the console NSF playback runs on. Used to convert a
/// play-speed divider (µs) into a CPU-cycle period for the non-60-Hz timer.
const NTSC_CPU_HZ: u32 = 1_789_773;
/// CPU cycles in one NTSC PPU frame (89342 dots / 3). The vblank-NMI path calls
/// `play` once per frame; a divider that resolves to this period IS 60 Hz.
const NTSC_FRAME_CYCLES: u32 = 29781;
/// Default NTSC divider when the header word is 0 (`1_000_000 / 60.0988`).
const NTSC_STD_SPEED_US: u16 = 16639;
/// Default PAL divider when the header word is 0 (`1_000_000 / 50.007`).
const PAL_STD_SPEED_US: u16 = 19997;

impl Nsf {
    /// The effective play-speed divider (µs) this file wants, resolving a `0`
    /// header word to the region hardware default and picking NTSC vs PAL by the
    /// region flag.
    #[must_use]
    pub const fn effective_speed_us(&self) -> u16 {
        if self.pal {
            if self.play_speed_pal == 0 {
                PAL_STD_SPEED_US
            } else {
                self.play_speed_pal
            }
        } else if self.play_speed_ntsc == 0 {
            NTSC_STD_SPEED_US
        } else {
            self.play_speed_ntsc
        }
    }

    /// The `play` period in NTSC CPU cycles, or `None` when the file plays at
    /// the standard once-per-NTSC-frame 60 Hz rate (the fast, vblank-NMI-driven
    /// path). `Some(cycles)` selects the cycle-timer IRQ driver — a PAL tune
    /// (50 Hz) or any custom divider on the NTSC console.
    #[must_use]
    pub fn nonstandard_play_period_cycles(&self) -> Option<u32> {
        let us = u64::from(self.effective_speed_us());
        // Max divider (65535 µs) → ~117k cycles, always fits u32.
        let cycles = u32::try_from(us * u64::from(NTSC_CPU_HZ) / 1_000_000).unwrap_or(u32::MAX);
        // Within a cycle or two of a full NTSC frame ⇒ treat as plain 60 Hz.
        (cycles.abs_diff(NTSC_FRAME_CYCLES) > 2).then_some(cycles.max(1))
    }
}

/// Parse an extended `NSFE` (chunked) music file into the shared [`Nsf`].
///
/// Layout: the 4-byte `NSFE` magic, then a sequence of chunks each prefixed by
/// a little-endian `u32` size + a 4-byte `FourCC` tag, terminated by the zero-length
/// `NEND` chunk (or EOF). We consume `INFO` (required, first), `DATA` (the
/// program image, required), and the optional `BANK` (initial 4 KiB banks) and
/// `auth` (game / artist / copyright / ripper NUL-separated strings). Unknown
/// chunks are skipped — including mandatory (uppercase-initial) ones we do not
/// model, which is the tolerant behaviour real players use for base-2A03 tunes.
///
/// # Errors
///
/// Returns [`MapperError::Invalid`] on a truncated file, a chunk that runs past
/// EOF, a missing/short `INFO`, or a missing `DATA`.
fn parse_nsfe(bytes: &[u8]) -> Result<Nsf, MapperError> {
    let inval = |m: &str| MapperError::Invalid(alloc::format!("NSFE: {m}"));
    if !is_nsfe(bytes) {
        return Err(inval("magic bytes do not match \"NSFE\""));
    }
    let mut pos = NSFE_MAGIC.len();
    let mut info: Option<&[u8]> = None;
    let mut data: Option<&[u8]> = None;
    let mut banks = [0u8; 8];
    let (mut song_name, mut artist, mut copyright) = (
        Box::<str>::default(),
        Box::<str>::default(),
        Box::<str>::default(),
    );

    while pos + 8 <= bytes.len() {
        let size = u32::from_le_bytes([bytes[pos], bytes[pos + 1], bytes[pos + 2], bytes[pos + 3]])
            as usize;
        let tag = &bytes[pos + 4..pos + 8];
        let body_start = pos + 8;
        let body_end = body_start
            .checked_add(size)
            .filter(|&e| e <= bytes.len())
            .ok_or_else(|| inval("chunk size runs past end of file"))?;
        let body = &bytes[body_start..body_end];
        match tag {
            b"NEND" => break,
            b"INFO" => info = Some(body),
            b"DATA" => data = Some(body),
            b"BANK" => {
                let n = body.len().min(8);
                banks[..n].copy_from_slice(&body[..n]);
            }
            b"auth" => {
                // Four NUL-terminated UTF-8 strings: game, artist, copyright, ripper.
                let mut parts = body.split(|&b| b == 0);
                let take = |p: Option<&[u8]>| -> Box<str> {
                    alloc::string::String::from_utf8_lossy(p.unwrap_or(&[]))
                        .into_owned()
                        .into_boxed_str()
                };
                song_name = take(parts.next());
                artist = take(parts.next());
                copyright = take(parts.next());
            }
            _ => { /* skip unknown / unmodelled chunk */ }
        }
        pos = body_end;
    }

    let info = info.ok_or_else(|| inval("missing required INFO chunk"))?;
    if info.len() < 8 {
        return Err(inval("INFO chunk shorter than 8 bytes"));
    }
    let prg = data
        .ok_or_else(|| inval("missing required DATA chunk"))?
        .to_vec();

    let load_addr = read_u16(info, 0);
    let init_addr = read_u16(info, 2);
    let play_addr = read_u16(info, 4);
    let pal = info[6] & 0b11 != 0;
    let expansion = info[7];
    let total_songs = info.get(8).copied().unwrap_or(1).max(1);
    // NSFe starting track is 0-based; the shared struct stores 1-based.
    let starting_song = info.get(9).copied().unwrap_or(0).saturating_add(1);

    if load_addr < 0x6000 {
        return Err(inval("INFO load address is below $6000"));
    }

    let bankswitched = banks.iter().any(|&b| b != 0);
    let mut prg = prg;
    if bankswitched {
        let pad = (load_addr & 0x0FFF) as usize;
        let mut image = vec![0u8; pad];
        image.extend_from_slice(&prg);
        if !image.len().is_multiple_of(0x1000) {
            image.resize(image.len() + (0x1000 - image.len() % 0x1000), 0);
        }
        prg = image;
    }

    Ok(Nsf {
        total_songs,
        starting_song,
        load_addr,
        init_addr,
        play_addr,
        bankswitched,
        initial_banks: banks,
        expansion,
        pal,
        // NSFe has no µs divider — the region flag drives the rate via
        // `effective_speed_us` (NTSC 60 Hz vs PAL 50 Hz).
        play_speed_ntsc: 0,
        play_speed_pal: 0,
        prg,
        song_name,
        artist,
        copyright,
    })
}

/// A synthetic "mapper" that plays an [`Nsf`] through the standard lockstep
/// engine. See the module docs for the driver mechanism.
#[allow(clippy::struct_excessive_bools)] // independent state flags, not an FSM
pub struct NsfMapper {
    prg: Box<[u8]>,
    /// 8 KiB of work RAM at `$6000-$7FFF`.
    wram: Box<[u8; 0x2000]>,
    /// 4 KiB bank registers for `$8000-$FFFF` (eight slots).
    banks: [u8; 8],
    bankswitched: bool,
    load_addr: u16,
    init_addr: u16,
    play_addr: u16,
    pal: bool,
    /// Number of songs (1-based count).
    total_songs: u8,
    /// Currently-selected song (0-based; what `init` receives in A).
    current_song: u8,
    /// The synthetic 6502 driver image served at [`DRIVER_BASE`].
    driver: [u8; 0x50],
    /// `Some(period)` selects the **non-60-Hz cycle-timer IRQ** driver: `play`
    /// is called every `period` CPU cycles (a PAL 50-Hz tune, or any custom
    /// divider, on the NTSC console). `None` is the standard once-per-vblank
    /// 60-Hz path (byte-identical to the pre-feature player).
    play_period_cycles: Option<u32>,
    /// Free-running CPU-cycle counter toward the next `play` (timer mode only).
    play_cycle_counter: u32,
    /// Whether the driver has finished `init` and armed the play-timer (set by
    /// the `$5FF0` write the timer-mode driver issues after `JSR init`). Gates
    /// the IRQ so no `play` fires mid-init.
    timer_enabled: bool,
    /// Level-triggered play-timer IRQ line (timer mode only), cleared by the
    /// driver's `$5FF1` acknowledge write.
    irq_pending: bool,
    /// Raw `$07B` expansion bitfield (kept for save-state reconstruction).
    expansion: u8,
    /// Expansion-audio synth cores, present only when the bitfield requests
    /// at least one chip (G2/G3). `None` for a base-2A03 NSF, so the common
    /// path carries no extra state and is byte-identical to before.
    exp_audio: Option<NsfExpansion>,
}

impl NsfMapper {
    /// Build the player from a parsed [`Nsf`], starting on its declared song.
    #[must_use]
    pub fn new(nsf: &Nsf) -> Self {
        // `starting_song` is 1-based; clamp to the valid 0-based range in case a
        // malformed header declares a start past `total_songs` (which is already
        // guaranteed non-zero by `parse_nsf`).
        let start = nsf
            .starting_song
            .saturating_sub(1)
            .min(nsf.total_songs.saturating_sub(1));
        let mut m = Self {
            prg: nsf.prg.clone().into_boxed_slice(),
            wram: Box::new([0u8; 0x2000]),
            banks: nsf.initial_banks,
            bankswitched: nsf.bankswitched,
            load_addr: nsf.load_addr,
            init_addr: nsf.init_addr,
            play_addr: nsf.play_addr,
            pal: nsf.pal,
            total_songs: nsf.total_songs,
            current_song: start,
            driver: [0u8; 0x50],
            play_period_cycles: nsf.nonstandard_play_period_cycles(),
            play_cycle_counter: 0,
            timer_enabled: false,
            irq_pending: false,
            expansion: nsf.expansion,
            exp_audio: NsfExpansion::from_bits(nsf.expansion),
        };
        m.build_driver();
        m
    }

    /// Number of selectable songs.
    #[must_use]
    pub const fn song_count(&self) -> u8 {
        self.total_songs
    }

    /// The currently-selected 0-based song.
    #[must_use]
    pub const fn current_song(&self) -> u8 {
        self.current_song
    }

    /// Select a 0-based song. Clamped to the valid range; re-patches the driver
    /// so the next reset runs `init` for the new track.
    pub fn set_song(&mut self, song: u8) {
        self.current_song = song.min(self.total_songs.saturating_sub(1));
        self.driver[DRIVER_SONG_OPERAND] = self.current_song;
    }

    /// Assemble the synthetic 6502 driver. Layout (addresses relative to
    /// [`DRIVER_BASE`] = `$5000`):
    ///
    /// ```text
    /// INIT ($5000):  SEI; CLD; LDX #$FF; TXS
    ///                LDA #song; LDX #region; JSR init_addr   ; song operand @ +6
    ///                LDA #$80; STA $2000                      ; enable vblank NMI
    ///                CLI; loop: JMP loop                      ; spin; NMI drives play
    /// NMI  ($5015):  PHA; TXA; PHA; TYA; PHA; JSR play_addr
    ///                PLA; TAY; PLA; TAX; PLA; RTI
    /// IRQ  ($5023):  RTI                                      ; base-NSF stub
    /// ```
    fn build_driver(&mut self) {
        let il = (self.init_addr & 0xFF) as u8;
        let ih = (self.init_addr >> 8) as u8;
        let pl = (self.play_addr & 0xFF) as u8;
        let ph = (self.play_addr >> 8) as u8;
        let region = u8::from(self.pal);
        let spin = DRIVER_BASE + 0x12;
        let code: [u8; 0x24] = [
            0x78, // 5000 SEI
            0xD8, // 5001 CLD
            0xA2,
            0xFF, // 5002 LDX #$FF
            0x9A, // 5004 TXS
            0xA9,
            self.current_song, // 5005 LDA #song   (operand @ 0x06)
            0xA2,
            region, // 5007 LDX #region
            0x20,
            il,
            ih, // 5009 JSR init_addr
            0xA9,
            0x80, // 500C LDA #$80
            0x8D,
            0x00,
            0x20, // 500E STA $2000
            0x58, // 5011 CLI
            0x4C,
            (spin & 0xFF) as u8,
            (spin >> 8) as u8, // 5012 JMP spin
            0x48,              // 5015 PHA
            0x8A,              // 5016 TXA
            0x48,              // 5017 PHA
            0x98,              // 5018 TYA
            0x48,              // 5019 PHA
            0x20,
            pl,
            ph,   // 501A JSR play_addr
            0x68, // 501D PLA
            0xA8, // 501E TAY
            0x68, // 501F PLA
            0xAA, // 5020 TAX
            0x68, // 5021 PLA
            0x40, // 5022 RTI
            0x40, // 5023 RTI  (IRQ stub)
        ];
        self.driver[..code.len()].copy_from_slice(&code);
        // Non-60-Hz files replace the standard vblank-NMI image with the
        // cycle-timer IRQ driver (below).
        if self.play_period_cycles.is_some() {
            self.build_timer_driver(il, ih, pl, ph, region);
        }
    }

    /// Assemble the **non-60-Hz** driver: instead of enabling vblank NMI, `INIT`
    /// arms the mapper play-timer (`STA $5FF0`) after `JSR init`, and the `IRQ`
    /// handler acknowledges the timer (`STA $5FF1`) then calls `play`. The NMI
    /// entry becomes an `RTI` stub. Vector layout (`DRIVER_NMI_ENTRY` /
    /// `DRIVER_IRQ_ENTRY`) is unchanged, so `cpu_read` of `$FFFA-$FFFF` and the
    /// `DRIVER_SONG_OPERAND` (@ +6) stay valid.
    fn build_timer_driver(&mut self, il: u8, ih: u8, pl: u8, ph: u8, region: u8) {
        self.driver = [0u8; 0x50];
        // INIT @ $5000: run the tune's `init`, then jump PAST the fixed NMI
        // ($5015) / IRQ ($5023) entries to the continuation @ $5034 — which
        // MUST disable the APU frame-counter IRQ before arming, or that IRQ
        // shares this handler and drives `play` continuously.
        let init_block: [u8; 0x0F] = [
            0x78, // SEI
            0xD8, // CLD
            0xA2,
            0xFF, // LDX #$FF
            0x9A, // TXS
            0xA9,
            self.current_song, // LDA #song   (operand @ 0x06)
            0xA2,
            region, // LDX #region
            0x20,
            il,
            ih, // JSR init
            0x4C,
            0x34,
            0x50, // JMP $5034 (continuation)
        ];
        self.driver[..init_block.len()].copy_from_slice(&init_block);
        // NMI stub @ $5015 (never enabled in timer mode; the vector still points
        // here, so keep a safe RTI).
        self.driver[0x15] = 0x40; // RTI
        // IRQ play handler @ $5023: ack the timer ($5FF1), then `play`.
        let irq_block: [u8; 0x11] = [
            0x48, // PHA
            0x8A, // TXA
            0x48, // PHA
            0x98, // TYA
            0x48, // PHA
            0x8D, 0xF1, 0x5F, // STA $5FF1  (ack timer IRQ)
            0x20, pl, ph,   // JSR play
            0x68, // PLA
            0xA8, // TAY
            0x68, // PLA
            0xAA, // TAX
            0x68, // PLA
            0x40, // RTI
        ];
        self.driver[0x23..0x23 + irq_block.len()].copy_from_slice(&irq_block);
        // Continuation @ $5034: disable the APU frame-counter IRQ ($4017 = $40,
        // bit6 inhibit) ONCE (not per-`play`, so the APU frame sequencer isn't
        // disturbed), arm the play-timer ($5FF0), enable IRQ, spin.
        let cont_block: [u8; 0x0E] = [
            0xA9, 0x40, // LDA #$40
            0x8D, 0x17, 0x40, // STA $4017  (frame IRQ inhibit, 4-step)
            0xA9, 0x01, // LDA #$01
            0x8D, 0xF0, 0x5F, // STA $5FF0  (arm play-timer)
            0x58, // CLI
            0x4C, 0x3F, 0x50, // JMP $503F (spin: self)
        ];
        self.driver[0x34..0x34 + cont_block.len()].copy_from_slice(&cont_block);
    }

    /// Resolve a `$8000-$FFFF` CPU address to a PRG offset.
    fn prg_offset(&self, addr: u16) -> Option<usize> {
        if self.bankswitched {
            let slot = ((addr - 0x8000) >> 12) as usize; // 0..=7
            let bank = self.banks[slot] as usize;
            let off = bank * 0x1000 + (addr as usize & 0x0FFF);
            (off < self.prg.len()).then_some(off)
        } else {
            // Linear image loaded at `load_addr`.
            let base = self.load_addr as usize;
            let a = addr as usize;
            (a >= base)
                .then(|| a - base)
                .filter(|&o| o < self.prg.len())
        }
    }
}

impl Mapper for NsfMapper {
    fn caps(&self) -> MapperCaps {
        // Base 60-Hz NSF (no expansion audio): no per-cycle hooks, no IRQ, no
        // synthesis — same as before (`MapperCaps::NONE`). Expansion audio adds
        // the CPU-cycle clock (oscillators), the frame-event hook (MMC5
        // envelope/length cadence), and audio mixing. A non-60-Hz file adds the
        // CPU-cycle clock (to advance the play-timer) + the IRQ source.
        let timer = self.play_period_cycles.is_some();
        let exp = self.exp_audio.is_some();
        MapperCaps {
            cpu_cycle_hook: exp || timer,
            audio: exp,
            frame_event_hook: exp,
            irq_source: timer,
        }
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        // Expansion-audio read ports (N163 data port `$4800-$4FFF`, MMC5
        // status `$5015`) take precedence over the default mapper read.
        if let Some(exp) = self.exp_audio.as_mut()
            && let Some(byte) = exp.cpu_read(addr)
        {
            return byte;
        }
        match addr {
            // Synthetic driver image.
            a if (DRIVER_BASE..DRIVER_BASE + 0x50).contains(&a) => {
                self.driver[(a - DRIVER_BASE) as usize]
            }
            // A non-bankswitched NSF may load its program into `$6000-$7FFF`
            // (allowed: `load_addr >= 0x6000`). Serve the program there first,
            // falling back to WRAM only where it doesn't reach (gemini #44).
            0x6000..=0x7FFF => {
                if !self.bankswitched
                    && let Some(off) = self.prg_offset(addr)
                {
                    return self.prg[off];
                }
                self.wram[(addr - 0x6000) as usize]
            }
            // Interrupt vectors point at the driver, overriding any PRG bytes.
            0xFFFA => (DRIVER_NMI_ENTRY & 0xFF) as u8,
            0xFFFB => (DRIVER_NMI_ENTRY >> 8) as u8,
            0xFFFC => (DRIVER_INIT_ENTRY & 0xFF) as u8,
            0xFFFD => (DRIVER_INIT_ENTRY >> 8) as u8,
            0xFFFE => (DRIVER_IRQ_ENTRY & 0xFF) as u8,
            0xFFFF => (DRIVER_IRQ_ENTRY >> 8) as u8,
            0x8000..=0xFFFF => self.prg_offset(addr).map_or(0, |o| self.prg[o]),
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        match addr {
            // 4 KiB bank registers for $8000-$FFFF (take priority over any
            // expansion-audio window — base NSFs bank only through here).
            0x5FF8..=0x5FFF if self.bankswitched => {
                self.banks[(addr - 0x5FF8) as usize] = value;
            }
            // Non-60-Hz play-timer control (the timer-mode driver writes these;
            // the standard driver never does, and they collide with no
            // expansion-audio register). `$5FF0` = arm (init finished, start
            // counting); `$5FF1` = acknowledge the level-triggered timer IRQ.
            0x5FF0 => {
                self.timer_enabled = true;
                self.play_cycle_counter = 0;
                self.irq_pending = false;
            }
            0x5FF1 => self.irq_pending = false,
            0x6000..=0x7FFF => self.wram[(addr - 0x6000) as usize] = value,
            _ => {
                // Route everything else to the expansion-audio chips. For a
                // base-2A03 NSF (`exp_audio == None`) this is a no-op, so the
                // behaviour is unchanged.
                if let Some(exp) = self.exp_audio.as_mut() {
                    exp.cpu_write(addr, value);
                }
            }
        }
    }

    fn cpu_read_unmapped(&self, addr: u16) -> bool {
        // The driver image and the bank registers ARE mapped in $4020-$5FFF, so
        // the bus must use our real bytes there (not open bus). Everything else
        // in that window is unmapped (open bus).
        if !(0x4020..=0x5FFF).contains(&addr) {
            return false;
        }
        // Driver image and bank registers are always mapped.
        if (DRIVER_BASE..DRIVER_BASE + 0x50).contains(&addr) || (0x5FF8..=0x5FFF).contains(&addr) {
            return false;
        }
        // Expansion-audio read ports (N163 `$4800-$4FFF`, MMC5 `$5015`) are
        // mapped when those chips are present (real bytes, not open bus).
        if self.exp_audio.is_some() && ((0x4800..=0x4FFF).contains(&addr) || addr == 0x5015) {
            return false;
        }
        true
    }

    fn notify_cpu_cycle(&mut self) {
        if let Some(exp) = self.exp_audio.as_mut() {
            exp.clock();
        }
        // Non-60-Hz play-timer: once armed by the driver's `$5FF0` write, count
        // CPU cycles and raise the (level-triggered) IRQ line every `period`
        // cycles. It stays asserted until the driver's `$5FF1` ack, mirroring
        // the MMC3/FME-7 mapper-IRQ discipline the bus already polls.
        if let Some(period) = self.play_period_cycles
            && self.timer_enabled
        {
            self.play_cycle_counter += 1;
            if self.play_cycle_counter >= period {
                self.play_cycle_counter = 0;
                self.irq_pending = true;
            }
        }
    }

    fn irq_pending(&self) -> bool {
        self.irq_pending
    }

    fn notify_frame_event(&mut self, events: MapperFrameEvents) {
        if let Some(exp) = self.exp_audio.as_mut() {
            exp.frame_event(events.quarter, events.half);
        }
    }

    fn mix_audio(&mut self) -> i32 {
        self.exp_audio.as_ref().map_or(0, NsfExpansion::mix)
    }

    fn ppu_read(&mut self, _addr: u16) -> u8 {
        // No CHR: NSF files carry no graphics. Reads return open-bus-ish 0.
        0
    }

    fn ppu_write(&mut self, _addr: u16, _value: u8) {}

    fn current_mirroring(&self) -> Mirroring {
        Mirroring::Horizontal
    }

    fn nsf_song_count(&self) -> u8 {
        self.total_songs
    }

    fn nsf_current_song(&self) -> u8 {
        self.current_song
    }

    fn nsf_set_song(&mut self, song: u8) -> bool {
        self.set_song(song);
        true
    }

    fn save_state(&self) -> Vec<u8> {
        // v1: version + song + 8 bank regs + WRAM.
        // v2 (G2/G3): appends a 1-byte expansion-audio presence tail when
        // expansion audio is present (ADR-0003: additive; v1 readers ignore
        // the tail). A base-2A03 NSF still writes a v1 blob, so existing
        // save-states stay byte-identical.
        let has_exp = self.exp_audio.is_some();
        let version = if has_exp { 2u8 } else { 1u8 };
        let mut out = Vec::with_capacity(2 + 8 + self.wram.len() + usize::from(has_exp));
        out.push(version);
        out.push(self.current_song);
        out.extend_from_slice(&self.banks);
        out.extend_from_slice(self.wram.as_ref());
        if let Some(exp) = self.exp_audio.as_ref() {
            exp.save_state(&mut out);
        }
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let version = data.first().copied().unwrap_or(0);
        if !(1..=2).contains(&version) {
            return Err(MapperError::UnsupportedVersion(version));
        }
        let core_len = 2 + 8 + self.wram.len();
        // v1 must match exactly; v2 carries a 1-byte expansion tail.
        let expected = core_len + usize::from(version == 2);
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        // Clamp on restore: a corrupt save-state must not feed an out-of-range
        // song index into `build_driver` (valid round-trips are unaffected, since
        // `set_song` already keeps `current_song < total_songs`).
        self.current_song = data[1].min(self.total_songs.saturating_sub(1));
        self.banks.copy_from_slice(&data[2..10]);
        self.wram.copy_from_slice(&data[10..core_len]);
        // The expansion-audio chips are reconstructed from the (immutable)
        // `$07B` bitfield; the v2 presence byte is self-describing but does
        // not carry oscillator phase, so we rebuild the chips fresh. Live
        // synthesis re-converges from the next register write (the correct
        // behaviour for a paused/restored NSF — see `NsfExpansion::save_state`).
        self.exp_audio = NsfExpansion::from_bits(self.expansion);
        // Consume the v2 expansion tail (round-trips with `save_state`). The
        // byte only carries which chips were present, so we validate it against
        // the chips rebuilt from `$07B` rather than discarding it; a mismatch
        // means the tail describes a different chip set than this ROM's header.
        if version == 2 {
            let tail = data[core_len];
            let rebuilt = self
                .exp_audio
                .as_ref()
                .map_or(0, NsfExpansion::presence_bits);
            if tail != rebuilt {
                return Err(MapperError::Invalid(format!(
                    "NSF v2 expansion presence tail {tail:#04x} disagrees with the $07B bitfield {rebuilt:#04x}"
                )));
            }
        }
        // Non-60-Hz play-timer runtime is not serialized (the sub-period counter
        // phase is inaudible across a restore, so the save-state format is
        // unchanged for both standard and timer files). A restored timer file
        // was already past `init`, so re-arm it; a standard file leaves these
        // inert (`play_period_cycles` is `None`).
        self.timer_enabled = self.play_period_cycles.is_some();
        self.play_cycle_counter = 0;
        self.irq_pending = false;
        self.build_driver();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal valid NSF: 1 song, load=$8000, init=$8000, play=$8003,
    /// non-bankswitched, a few bytes of "program".
    fn synth_nsf() -> Vec<u8> {
        let mut f = vec![0u8; NSF_HEADER_LEN];
        f[0..5].copy_from_slice(NSF_MAGIC);
        f[0x05] = 1; // version
        f[0x06] = 3; // total songs
        f[0x07] = 1; // starting song (1-based)
        f[0x08] = 0x00;
        f[0x09] = 0x80; // load $8000
        f[0x0A] = 0x00;
        f[0x0B] = 0x80; // init $8000
        f[0x0C] = 0x03;
        f[0x0D] = 0x80; // play $8003
        // program: RTS at init, RTS at play
        f.extend_from_slice(&[0x60, 0xEA, 0xEA, 0x60]);
        f
    }

    #[test]
    fn parses_header_fields() {
        let nsf = parse_nsf(&synth_nsf()).expect("valid nsf");
        assert_eq!(nsf.total_songs, 3);
        assert_eq!(nsf.starting_song, 1);
        assert_eq!(nsf.load_addr, 0x8000);
        assert_eq!(nsf.init_addr, 0x8000);
        assert_eq!(nsf.play_addr, 0x8003);
        assert!(!nsf.bankswitched);
    }

    #[test]
    fn rejects_bad_magic() {
        let mut f = synth_nsf();
        f[1] = b'X';
        assert!(parse_nsf(&f).is_err());
    }

    #[test]
    fn driver_vectors_point_into_driver() {
        let nsf = parse_nsf(&synth_nsf()).expect("valid nsf");
        let mut m = NsfMapper::new(&nsf);
        // Reset vector -> driver init entry.
        let lo = m.cpu_read(0xFFFC);
        let hi = m.cpu_read(0xFFFD);
        assert_eq!(u16::from(lo) | (u16::from(hi) << 8), DRIVER_INIT_ENTRY);
        // NMI vector -> driver NMI entry.
        let lo = m.cpu_read(0xFFFA);
        let hi = m.cpu_read(0xFFFB);
        assert_eq!(u16::from(lo) | (u16::from(hi) << 8), DRIVER_NMI_ENTRY);
        // The driver's JSR init operand encodes init_addr ($8000).
        assert_eq!(m.cpu_read(DRIVER_BASE + 0x0A), 0x00);
        assert_eq!(m.cpu_read(DRIVER_BASE + 0x0B), 0x80);
    }

    #[test]
    fn track_select_patches_driver_and_clamps() {
        let nsf = parse_nsf(&synth_nsf()).expect("valid nsf");
        let mut m = NsfMapper::new(&nsf);
        m.set_song(2);
        assert_eq!(m.current_song(), 2);
        assert_eq!(m.cpu_read(DRIVER_BASE + 0x06), 2);
        // Clamp: only 3 songs (0..=2), so 9 -> 2.
        m.set_song(9);
        assert_eq!(m.current_song(), 2);
    }

    #[test]
    fn save_state_round_trips() {
        let nsf = parse_nsf(&synth_nsf()).expect("valid nsf");
        let mut m = NsfMapper::new(&nsf);
        m.set_song(1);
        m.cpu_write(0x6000, 0xAB);
        let blob = m.save_state();
        let mut m2 = NsfMapper::new(&nsf);
        m2.load_state(&blob).expect("round trip");
        assert_eq!(m2.current_song(), 1);
        assert_eq!(m2.cpu_read(0x6000), 0xAB);
    }

    #[test]
    fn expansion_save_state_round_trips_v2_tail() {
        // An NSF declaring VRC6 expansion audio ($07B bit0) emits a v2 blob with
        // the 1-byte presence tail; load_state must consume + validate it (round
        // trip), not error or leave bytes unread.
        let mut f = synth_nsf();
        f[0x7B] = 0x01; // EXP_VRC6
        let nsf = parse_nsf(&f).expect("valid expansion nsf");
        let mut m = NsfMapper::new(&nsf);
        assert!(m.exp_audio.is_some(), "VRC6 expansion must be present");
        m.set_song(2);
        m.cpu_write(0x6000, 0xCD);
        let blob = m.save_state();
        assert_eq!(blob.first().copied(), Some(2u8), "expansion NSF is v2");
        let mut m2 = NsfMapper::new(&nsf);
        m2.load_state(&blob).expect("v2 round trip");
        assert_eq!(m2.current_song(), 2);
        assert_eq!(m2.cpu_read(0x6000), 0xCD);
        assert!(
            m2.exp_audio.is_some(),
            "expansion rebuilt from $07B on load"
        );
    }

    #[test]
    fn expansion_load_state_rejects_corrupt_presence_tail() {
        // A v2 blob whose presence tail disagrees with the $07B bitfield is a
        // corrupt save-state and must be rejected rather than silently accepted.
        let mut f = synth_nsf();
        f[0x7B] = 0x01; // EXP_VRC6
        let nsf = parse_nsf(&f).expect("valid expansion nsf");
        let mut m = NsfMapper::new(&nsf);
        let mut blob = m.save_state();
        *blob.last_mut().expect("tail byte") ^= 0x80; // flip a presence bit
        assert!(matches!(m.load_state(&blob), Err(MapperError::Invalid(_))));
    }

    // ---- non-60-Hz playback (speed-word / cycle-timer IRQ) ----

    fn synth_nsf_speed(ntsc_us: u16) -> Vec<u8> {
        let mut f = synth_nsf();
        f[0x6E] = (ntsc_us & 0xFF) as u8;
        f[0x6F] = (ntsc_us >> 8) as u8;
        f
    }

    #[test]
    fn parses_and_resolves_speed_words() {
        // Explicit NTSC divider is parsed and used.
        let nsf = parse_nsf(&synth_nsf_speed(20000)).expect("valid");
        assert_eq!(nsf.play_speed_ntsc, 20000);
        assert_eq!(nsf.effective_speed_us(), 20000);
        // A 0 divider resolves to the NTSC hardware default (60 Hz standard).
        let nsf0 = parse_nsf(&synth_nsf_speed(0)).expect("valid");
        assert_eq!(nsf0.effective_speed_us(), NTSC_STD_SPEED_US);
    }

    #[test]
    fn standard_rate_is_vblank_driven_timer_none() {
        // 0 (default) and the exact NTSC divider both classify as plain 60 Hz.
        for us in [0u16, NTSC_STD_SPEED_US] {
            let nsf = parse_nsf(&synth_nsf_speed(us)).expect("valid");
            assert_eq!(nsf.nonstandard_play_period_cycles(), None, "us={us}");
            let m = NsfMapper::new(&nsf);
            assert!(m.play_period_cycles.is_none());
            // Base 60-Hz NSF keeps the no-hooks capability set (byte-identical).
            assert_eq!(m.caps(), MapperCaps::NONE);
        }
    }

    #[test]
    fn nonstandard_rate_selects_cycle_timer_driver() {
        // A PAL-ish 50-Hz divider (20000 µs) resolves to a non-frame period and
        // selects the timer driver (CPU-cycle hook + IRQ source).
        let nsf = parse_nsf(&synth_nsf_speed(20000)).expect("valid");
        let period = nsf.nonstandard_play_period_cycles().expect("timer mode");
        assert!(
            period > NTSC_FRAME_CYCLES,
            "slower than 60 Hz -> longer period"
        );
        let mut m = NsfMapper::new(&nsf);
        let caps = m.caps();
        assert!(caps.irq_source && caps.cpu_cycle_hook);
        // NMI vector -> RTI stub; IRQ vector -> play handler (starts with PHA).
        assert_eq!(m.cpu_read(DRIVER_NMI_ENTRY), 0x40); // RTI
        assert_eq!(m.cpu_read(DRIVER_IRQ_ENTRY), 0x48); // PHA
        // INIT jumps past the fixed NMI/IRQ entries to the continuation @ $5034.
        assert_eq!(m.cpu_read(DRIVER_BASE + 0x0C), 0x4C); // JMP
        assert_eq!(m.cpu_read(DRIVER_BASE + 0x0D), 0x34);
        assert_eq!(m.cpu_read(DRIVER_BASE + 0x0E), 0x50);
        // The continuation disables the APU frame IRQ (`STA $4017`) BEFORE it
        // arms the play-timer (`STA $5FF0`) — the ordering that stops the frame
        // IRQ from co-driving `play`.
        assert_eq!(m.cpu_read(DRIVER_BASE + 0x36), 0x8D); // STA $4017
        assert_eq!(m.cpu_read(DRIVER_BASE + 0x37), 0x17);
        assert_eq!(m.cpu_read(DRIVER_BASE + 0x38), 0x40);
        assert_eq!(m.cpu_read(DRIVER_BASE + 0x3B), 0x8D); // STA $5FF0 (arm)
        assert_eq!(m.cpu_read(DRIVER_BASE + 0x3C), 0xF0);
        assert_eq!(m.cpu_read(DRIVER_BASE + 0x3D), 0x5F);
    }

    #[test]
    fn timer_raises_and_acks_irq_only_after_arming() {
        let nsf = parse_nsf(&synth_nsf_speed(20000)).expect("valid");
        let mut m = NsfMapper::new(&nsf);
        let period = m.play_period_cycles.expect("timer mode");
        // Before arming: cycles must NOT raise the IRQ (init still running).
        for _ in 0..(period + 10) {
            m.notify_cpu_cycle();
        }
        assert!(
            !m.irq_pending(),
            "timer must stay quiet until $5FF0 arms it"
        );
        // Arm (the driver's post-init `STA $5FF0`).
        m.cpu_write(0x5FF0, 1);
        for _ in 0..(period - 1) {
            m.notify_cpu_cycle();
        }
        assert!(!m.irq_pending(), "no IRQ one cycle early");
        m.notify_cpu_cycle(); // period-th cycle
        assert!(m.irq_pending(), "IRQ fires exactly at the period");
        // Level-triggered: the driver's `$5FF1` ack clears it.
        m.cpu_write(0x5FF1, 0);
        assert!(!m.irq_pending(), "ack clears the level-triggered line");
    }

    #[test]
    fn timer_mode_save_state_round_trips_and_rearms() {
        let nsf = parse_nsf(&synth_nsf_speed(20000)).expect("valid");
        let mut m = NsfMapper::new(&nsf);
        m.cpu_write(0x5FF0, 1); // arm
        m.set_song(2);
        let blob = m.save_state();
        let mut m2 = NsfMapper::new(&nsf);
        m2.load_state(&blob).expect("round trip");
        assert_eq!(m2.current_song(), 2);
        // A restored timer file is re-armed (was past init) and fires again.
        let period = m2.play_period_cycles.expect("timer mode");
        for _ in 0..period {
            m2.notify_cpu_cycle();
        }
        assert!(m2.irq_pending(), "restored timer re-arms and fires");
    }

    // ---- NSFe (extended chunked container) ----

    /// Build a minimal `NSFe`: INFO (load/init/play $8000/$8000/$8003, 2 songs,
    /// optional PAL flag) + a short DATA + an `auth` metadata chunk + NEND.
    fn synth_nsfe(pal: bool) -> Vec<u8> {
        let mut f = Vec::new();
        f.extend_from_slice(b"NSFE");
        let chunk = |tag: &[u8; 4], body: &[u8], out: &mut Vec<u8>| {
            out.extend_from_slice(&u32::try_from(body.len()).unwrap().to_le_bytes());
            out.extend_from_slice(tag);
            out.extend_from_slice(body);
        };
        // INFO: load, init, play, region, expansion, tracks, start(0-based)
        let region = u8::from(pal);
        chunk(
            b"INFO",
            &[0x00, 0x80, 0x00, 0x80, 0x03, 0x80, region, 0x00, 2, 1],
            &mut f,
        );
        chunk(b"DATA", &[0x60, 0xEA, 0xEA, 0x60], &mut f);
        chunk(b"auth", b"Game Title\0Artist\0(c) 2026\0Ripper", &mut f);
        chunk(b"NEND", &[], &mut f);
        f
    }

    #[test]
    fn parses_nsfe_info_data_and_auth() {
        assert!(is_nsfe(&synth_nsfe(false)));
        assert!(is_nsf(&synth_nsfe(false))); // combined detector accepts NSFE
        let nsf = parse_nsf(&synth_nsfe(false)).expect("valid nsfe");
        assert_eq!(nsf.load_addr, 0x8000);
        assert_eq!(nsf.init_addr, 0x8000);
        assert_eq!(nsf.play_addr, 0x8003);
        assert_eq!(nsf.total_songs, 2);
        assert_eq!(nsf.starting_song, 2); // 0-based start 1 -> 1-based 2
        assert_eq!(&*nsf.song_name, "Game Title");
        assert_eq!(&*nsf.artist, "Artist");
        assert_eq!(&*nsf.copyright, "(c) 2026");
        assert!(!nsf.pal);
        // NTSC NSFe plays at the standard 60 Hz (vblank path).
        assert_eq!(nsf.nonstandard_play_period_cycles(), None);
    }

    #[test]
    fn nsfe_pal_region_selects_nonstandard_rate() {
        let nsf = parse_nsf(&synth_nsfe(true)).expect("valid pal nsfe");
        assert!(nsf.pal);
        // PAL on the NTSC console -> 50 Hz -> the cycle-timer driver.
        assert_eq!(nsf.effective_speed_us(), PAL_STD_SPEED_US);
        assert!(nsf.nonstandard_play_period_cycles().is_some());
        assert!(NsfMapper::new(&nsf).caps().irq_source);
    }

    #[test]
    fn nsfe_rejects_missing_info_or_data() {
        // NSFE magic + immediate NEND: no INFO, no DATA.
        let mut f = Vec::from(*b"NSFE");
        f.extend_from_slice(&0u32.to_le_bytes());
        f.extend_from_slice(b"NEND");
        assert!(matches!(parse_nsf(&f), Err(MapperError::Invalid(_))));
    }
}
