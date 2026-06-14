//! NSF / `NSFe` music-file player (v1.1.0 beta.2, Workstream D, T-110-D1).
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
//! Scope for this first cut: the base 2A03 APU, standard `$5FF8-$5FFF`
//! `$8000-$FFFF` 4 KiB bank-switching, and NTSC 60 Hz playback. Expansion-chip
//! audio (VRC6/7, MMC5, N163, Sunsoft 5B, FDS), the FDS-style `$5FF6/$5FF7`
//! RAM banking, and exact non-60 Hz play rates are deferred (documented).

use crate::cartridge::Mirroring;
use crate::mapper::{Mapper, MapperCaps, MapperError};
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
    /// Expansion-chip bitfield (`$07B`). Audio for these is not yet synthesized.
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
        if image.len() % 0x1000 != 0 {
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
}

impl NsfMapper {
    /// Build the player from a parsed [`Nsf`], starting on its declared song.
    #[must_use]
    pub fn new(nsf: &Nsf) -> Self {
        let start = nsf.starting_song.saturating_sub(1);
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
        // No per-cycle hooks (no IRQ, no on-cart audio synthesis yet).
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            // Synthetic driver image.
            a if (DRIVER_BASE..DRIVER_BASE + 0x40).contains(&a) => {
                self.driver[(a - DRIVER_BASE) as usize]
            }
            0x6000..=0x7FFF => self.wram[(addr - 0x6000) as usize],
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
            // 4 KiB bank registers for $8000-$FFFF.
            0x5FF8..=0x5FFF if self.bankswitched => {
                self.banks[(addr - 0x5FF8) as usize] = value;
            }
            0x6000..=0x7FFF => self.wram[(addr - 0x6000) as usize] = value,
            _ => {}
        }
    }

    fn cpu_read_unmapped(&self, addr: u16) -> bool {
        // The driver image and the bank registers ARE mapped in $4020-$5FFF, so
        // the bus must use our real bytes there (not open bus). Everything else
        // in that window is unmapped (open bus).
        !((DRIVER_BASE..DRIVER_BASE + 0x40).contains(&addr) || (0x5FF8..=0x5FFF).contains(&addr))
            && (0x4020..=0x5FFF).contains(&addr)
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
        let mut out = Vec::with_capacity(2 + 8 + self.wram.len());
        out.push(1); // version tag
        out.push(self.current_song);
        out.extend_from_slice(&self.banks);
        out.extend_from_slice(self.wram.as_ref());
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        if data.first() != Some(&1) {
            return Err(MapperError::UnsupportedVersion(
                data.first().copied().unwrap_or(0),
            ));
        }
        let expected = 2 + 8 + self.wram.len();
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        self.current_song = data[1];
        self.banks.copy_from_slice(&data[2..10]);
        self.wram.copy_from_slice(&data[10..]);
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
}
