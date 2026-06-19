//! NSF (classic `NESM`) music-file player (v1.1.0 beta.2, Workstream D, T-110-D1).
//!
//! Only the classic `NESM\x1a` container is parsed here; `NSFe` is a
//! documented deferral. Expansion-chip audio (VRC6/7, FDS, MMC5, N163,
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
//! song in A and the NTSC/PAL flag in X), enables vblank NMI, then spins; the
//! PPU's ordinary 60 Hz vblank NMI calls `play` once per frame. The whole thing
//! then runs through the unchanged `Nes::run_frame` lockstep loop, so the APU
//! produces audio exactly as it does for a cartridge and the determinism
//! contract is untouched.
//!
//! Scope: the base 2A03 APU, standard `$5FF8-$5FFF` `$8000-$FFFF` 4 KiB
//! bank-switching, NTSC 60 Hz playback, and expansion-chip audio (VRC6/7,
//! MMC5, N163, Sunsoft 5B, FDS) routed into the existing synth cores. The
//! FDS-style `$5FF6/$5FF7` RAM banking and exact non-60 Hz play rates are
//! deferred (documented).

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

/// Detect the NSF magic at the start of `bytes`.
#[must_use]
pub fn is_nsf(bytes: &[u8]) -> bool {
    bytes.len() >= NSF_MAGIC.len() && &bytes[0..NSF_MAGIC.len()] == NSF_MAGIC
}

/// Parse an `.nsf` (classic `NESM`) file.
///
/// # Errors
///
/// Returns a [`MapperError::Invalid`] when the magic is wrong, the file is
/// shorter than the 128-byte header, or the header is internally inconsistent.
pub fn parse_nsf(bytes: &[u8]) -> Result<Nsf, MapperError> {
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
    // "not strictly NTSC" for the init-register flag; playback is 60 Hz NTSC.
    let pal = bytes[0x7A] & 0b11 != 0;

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
        prg,
        song_name: read_string(bytes, 0x0E),
        artist: read_string(bytes, 0x2E),
        copyright: read_string(bytes, 0x4E),
    })
}

/// A synthetic "mapper" that plays an [`Nsf`] through the standard lockstep
/// engine. See the module docs for the driver mechanism.
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
    driver: [u8; 0x40],
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
            driver: [0u8; 0x40],
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
        // Base NSF (no expansion audio): no per-cycle hooks, no IRQ, no
        // synthesis — same as before. When the `$07B` bitfield requests
        // expansion audio, enable the CPU-cycle clock (oscillators), the
        // frame-event hook (MMC5 envelope/length cadence), and audio mixing.
        if self.exp_audio.is_some() {
            MapperCaps {
                cpu_cycle_hook: true,
                audio: true,
                frame_event_hook: true,
                irq_source: false,
            }
        } else {
            MapperCaps::NONE
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
            a if (DRIVER_BASE..DRIVER_BASE + 0x40).contains(&a) => {
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
        if (DRIVER_BASE..DRIVER_BASE + 0x40).contains(&addr) || (0x5FF8..=0x5FFF).contains(&addr) {
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
    }

    fn notify_frame_event(&mut self, events: MapperFrameEvents) {
        if let Some(exp) = self.exp_audio.as_mut() {
            exp.frame_event(events.quarter, events.half);
        }
    }

    fn mix_audio(&mut self) -> i16 {
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
}
