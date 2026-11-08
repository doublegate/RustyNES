//! v3.7.0 "the mobile extras" (D24, `T-MOBILE-EXTRAS` row 3) and the per-game
//! picture-correction switch (`T-MOBILE-GAME-CORRECTIONS`): three desktop
//! features brought to both mobile apps through the bridge, so Android and
//! iOS share one implementation instead of each growing its own.
//!
//! ## The 20-band graphic equalizer
//!
//! The desktop's EQ (`rustynes-frontend`'s `eq` module, v1.7.0 H3) is a
//! cascade of RBJ-cookbook peaking biquads at the ISO third-octave centres
//! [`crate::eq::EQ20_FREQS`]. The bridge compiles **that same source file**
//! (a `#[path]` module in `lib.rs`, not a copy), so a fix to the filter
//! reaches all three hosts at once. It runs on the drained host-rate samples
//! ([`NesController::drain_audio`] / [`NesController::drain_audio_bytes`]),
//! after the core has produced them and before the host plays them: an
//! output stage, exactly where the desktop puts it, so the core's synthesis,
//! the save-state bytes, movies and netplay never see it.
//!
//! **A flat bank is not run at all.** Every gain at 0 dB (the default, and
//! what a host sets to switch the EQ off) leaves no filter installed, so the
//! drained samples are bit-identical to a bridge without this module; the
//! tests below compare the two bit for bit.
//!
//! The gains persist across [`NesController::load_rom`] (a player's sound
//! preference outlives one game), and the filter is re-voiced at the new
//! sample rate there. The filter's history (two samples per band) is reset
//! by a load, so one game's tail never rings into the next.
//!
//! ## The Game Genie code database
//!
//! The desktop's catalog (`rustynes-frontend`'s `genie_db` module: the curated
//! starter set plus the ~10.8k-code libretro bulk catalog and its
//! header-excluded re-key, v2.1.3) is likewise compiled from the same file.
//! A loaded ROM is matched on both of its CRC32 keys, taken from the bytes the
//! host passed **before** the game database rewrites a header
//! (`rustynes_gamedb::correct_rom`): the full-file key is the No-Intro one and
//! would no longer match a corrected header. The bridge only LISTS codes; a
//! host enables one through the existing [`NesController::cheat_add_genie`],
//! so validation, the `RetroAchievements` hardcore refusal and the core's
//! substitution path are the ones that already exist.
//!
//! ## The per-game picture corrections (ADR 0051)
//!
//! `rustynes_core::game_corrections` lists dumps whose authentic hardware
//! glitch the maintainer chose not to draw by default (v3.6.2: *Rad Racer*'s
//! coherent mid-line copy). The core turns the correction on at load for a
//! listed dump; the desktop's `[enhancements] disable_game_corrections` and
//! libretro's `rustynes_game_corrections` let the player see the hardware's
//! picture instead. [`NesController::set_game_corrections_enabled`] is the
//! bridge's equivalent.
//!
//! The setting is the player's and outlives a ROM load. It reaches the
//! console only while nothing else owns the console's options: during a
//! movie the recording's (or the played movie's) options hold, and during a
//! netplay session the options both peers handshook with hold, because the
//! correction changes the picture and so every frame hash and desync check.
//! That is the core's existing carry (`HardwareOptions`, recorded in `.rnm`
//! format 8 and the netplay digest); this module adds no new carry. A change
//! made during a session therefore applies on the first frame after it ends
//! ([`sync_idle_corrections`], run at the top of every single-player frame).

use crate::{Inner, NesController, for_each_console, movie_owns_timeline, netplay_owns_timeline};

/// One entry of the Game Genie code database, marshalled across the FFI
/// (v3.7.0). A flat projection of the desktop's `GenieDbCode` without its
/// CRC key, which a host has no use for.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct CheatDbEntry {
    /// The game title the catalog lists the code under (display only).
    pub game: String,
    /// The human-readable effect (for example "Infinite lives").
    pub name: String,
    /// The canonical Game Genie code (6 or 8 characters), already validated
    /// by the core's decoder; pass it to [`NesController::cheat_add_genie`].
    pub code: String,
    /// The effect's category for grouping ("Lives", "Items", ...; "Misc"
    /// when the catalog row has none).
    pub category: String,
}

impl From<crate::genie_db::GenieDbCode> for CheatDbEntry {
    fn from(c: crate::genie_db::GenieDbCode) -> Self {
        Self {
            game: c.game,
            name: c.name,
            code: c.code,
            category: c.category,
        }
    }
}

/// The equalizer's state behind the controller's lock: the player's gains,
/// and the voiced filter when they are not flat.
pub struct EqState {
    /// Per-band gains in dB, sanitized (finite, `-12..=12`).
    gains: [f32; crate::eq::EQ20_BAND_COUNT],
    /// The voiced cascade, or `None` when every gain is 0 dB. `None` is the
    /// bypass: the drain path does not touch the samples at all.
    filter: Option<crate::eq::Equalizer>,
}

impl EqState {
    /// The flat (bypassed) equalizer every controller starts with.
    pub const FLAT: Self = Self {
        gains: [0.0; crate::eq::EQ20_BAND_COUNT],
        filter: None,
    };

    /// Re-voice the filter for `sample_rate` from the stored gains, clearing
    /// its history. Called on a gain change and on a ROM load (which may
    /// bring a new host rate).
    pub fn revoice(&mut self, sample_rate: u32) {
        let flat = self.gains.iter().all(|&g| g == 0.0);
        self.filter = (!flat).then(|| crate::eq::Equalizer::new_20(self.gains, sample_rate));
    }

    /// Filter drained samples in place; nothing at all when flat.
    pub fn process(&mut self, samples: &mut [f32]) {
        if let Some(filter) = self.filter.as_mut() {
            filter.process(samples);
        }
    }
}

/// The largest boost or cut a band takes, in dB: the desktop slider's range.
const EQ_LIMIT_DB: f32 = 12.0;

/// Clamp one host-supplied gain to the slider range; a non-finite value (a
/// host bug, or a corrupt stored preference) becomes 0 dB rather than a NaN
/// that would silence the stream for good.
fn sanitize_gain(g: f32) -> f32 {
    if g.is_finite() {
        g.clamp(-EQ_LIMIT_DB, EQ_LIMIT_DB)
    } else {
        0.0
    }
}

/// The two CRC32 keys the code database is indexed by, from the bytes the
/// host supplied (decompressed, before any header correction). Either is
/// `None` for an image that is not iNES (an FDS disk, an NSF).
pub fn database_keys(rom: &[u8]) -> [Option<u32>; 2] {
    [
        rustynes_gamedb::rom_crc32(rom),
        rustynes_gamedb::rom_crc32_full(rom),
    ]
}

/// Put the player's correction preference on the console(s), unless a movie
/// or a netplay session owns the console's options (see the module docs).
/// Cheap (one flag per console), so it runs at the top of every frame.
pub fn sync_idle_corrections(g: &mut Inner) {
    if movie_owns_timeline(g) || netplay_owns_timeline(g) {
        return;
    }
    let on = g.correction_listed && !g.corrections_disabled;
    for_each_console(g, |nes| nes.set_coherent_midline_copy(on));
}

/// The centre frequencies (Hz) of the 20-band equalizer, in band order: what
/// a host labels its sliders with. [`NesController::set_equalizer_gains`]
/// takes one gain per entry.
#[uniffi::export]
#[must_use]
pub fn equalizer_band_frequencies() -> Vec<f32> {
    crate::eq::EQ20_FREQS.to_vec()
}

/// The database entry for a Game Genie code the player typed, if the catalog
/// knows it: the reverse lookup that names a pasted code's game and effect.
/// `None` for an unknown or malformed code.
#[uniffi::export]
#[must_use]
pub fn cheat_database_lookup(code: String) -> Option<CheatDbEntry> {
    crate::genie_db::entry_for_code(&code).map(CheatDbEntry::from)
}

/// The libretro-thumbnails NES `Named_Boxarts` raw base (v3.7.0, D24 row 1).
const BOX_ART_BASE: &str = "https://raw.githubusercontent.com/libretro-thumbnails/\
                            Nintendo_-_Nintendo_Entertainment_System/master/Named_Boxarts/";

/// libretro's invalid thumbnail-name characters, each replaced by `_`.
const BOX_ART_INVALID: &[char] = &['&', '*', '/', ':', '`', '<', '>', '?', '\\', '|'];

/// A display name as a libretro thumbnail base name: the file extension dropped,
/// each [`BOX_ART_INVALID`] character replaced by `_`, the ends trimmed.
fn box_art_sanitize(display_name: &str) -> String {
    let trimmed = display_name.trim();
    let no_ext = trimmed
        .rsplit_once('.')
        .map_or(trimmed, |(stem, _)| stem)
        .trim();
    no_ext
        .chars()
        .map(|c| if BOX_ART_INVALID.contains(&c) { '_' } else { c })
        .collect::<String>()
        .trim()
        .to_owned()
}

/// Percent-encode a file name for a raw GitHub path the way Android's `BoxArt.kt`
/// does (Java's form encoding with `+` as `%20`, and `(`, `)`, `,`, `'`, `!` kept
/// readable): ASCII letters, digits and `.-*_` stay, every other UTF-8 byte is
/// `%XX` in upper case.
fn box_art_encode(name: &str) -> String {
    use core::fmt::Write as _;
    let mut out = String::with_capacity(name.len() * 3);
    for &b in name.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'.' | b'-' | b'*' | b'_' => {
                out.push(char::from(b));
            }
            b'(' | b')' | b',' | b'\'' | b'!' => out.push(char::from(b)),
            _ => {
                let _ = write!(out, "%{b:02X}");
            }
        }
    }
    out
}

/// v3.7.0 (D24 row 1) — the box-art URLs to try for a game, best first.
///
/// They point into the community libretro-thumbnails library (the no-intro-named
/// PNG set `RetroArch` uses): the sanitized display name, then its "short name"
/// (the text before the first `(`, which drops region and revision tags). The iOS app fetches these on
/// the player's request; the rules are Android's `BoxArt.kt` (v1.8.8), so both
/// apps find the same cover for the same file name.
#[uniffi::export]
#[must_use]
pub fn box_art_candidate_urls(display_name: String) -> Vec<String> {
    let full = box_art_sanitize(&display_name);
    let short = full.split('(').next().unwrap_or("").trim().to_owned();
    let mut out = Vec::with_capacity(2);
    if !full.is_empty() {
        out.push(format!(
            "{BOX_ART_BASE}{}",
            box_art_encode(&format!("{full}.png"))
        ));
    }
    if !short.is_empty() && short != full {
        out.push(format!(
            "{BOX_ART_BASE}{}",
            box_art_encode(&format!("{short}.png"))
        ));
    }
    out
}

#[uniffi::export]
impl NesController {
    /// Set the 20-band equalizer's gains in dB, one per
    /// [`equalizer_band_frequencies`] entry. Each is clamped to `-12..=12`,
    /// and a non-finite one is taken as 0. All zeros switches the EQ off,
    /// leaving the audio bit-identical to no EQ.
    ///
    /// Returns `false`, changing nothing, when `gains_db` is not exactly one
    /// gain per band (the call does not throw, so the generated error types
    /// stay as they were).
    pub fn set_equalizer_gains(&self, gains_db: Vec<f32>) -> bool {
        let Ok(gains) = <[f32; crate::eq::EQ20_BAND_COUNT]>::try_from(gains_db.as_slice()) else {
            return false;
        };
        let mut g = self.lock();
        g.eq.gains = gains.map(sanitize_gain);
        let rate = g.sample_rate;
        g.eq.revoice(rate);
        drop(g);
        true
    }

    /// The equalizer's current gains in dB (after clamping), one per band.
    pub fn equalizer_gains(&self) -> Vec<f32> {
        self.lock().eq.gains.to_vec()
    }

    /// Whether the equalizer is filtering (some band is not 0 dB).
    pub fn equalizer_active(&self) -> bool {
        self.lock().eq.filter.is_some()
    }

    /// The Game Genie codes the database lists for the loaded ROM, grouped by
    /// category and sorted by effect within each (the desktop's pick-list
    /// order), de-duplicated by code across the ROM's two CRC keys. Empty for
    /// an unlisted game, an FDS disk or an NSF.
    pub fn cheat_database_codes(&self) -> Vec<CheatDbEntry> {
        let crcs = self.database_crcs();
        crate::genie_db::codes_for_crcs_by_category(&crcs)
            .into_iter()
            .flat_map(|(_, codes)| codes)
            .map(CheatDbEntry::from)
            .collect()
    }

    /// The game title the code database lists the loaded ROM under, or
    /// `None` when it has no codes for it.
    pub fn cheat_database_game(&self) -> Option<String> {
        crate::genie_db::game_for_crcs(&self.database_crcs())
    }

    /// Whether the per-game picture corrections may apply (the default,
    /// `true`). `false` shows every game as the hardware draws it, *Rad
    /// Racer*'s line-139 dash included. The player's setting: it persists
    /// across ROM loads, and it applies at once unless a movie or a netplay
    /// session holds the console's options, in which case it applies when
    /// that ends.
    pub fn set_game_corrections_enabled(&self, enabled: bool) {
        let mut g = self.lock();
        g.corrections_disabled = !enabled;
        sync_idle_corrections(&mut g);
        drop(g);
    }

    /// The player's correction setting (see
    /// [`Self::set_game_corrections_enabled`]).
    pub fn game_corrections_enabled(&self) -> bool {
        !self.lock().corrections_disabled
    }

    /// Whether the loaded ROM is one `rustynes_core::game_corrections` lists,
    /// so a host can say "this game has a correction" beside the switch.
    pub fn game_correction_listed(&self) -> bool {
        self.lock().correction_listed
    }

    /// Whether the loaded console is drawing with a correction right now: the
    /// listing and the setting together, or a movie's or a netplay session's
    /// recorded choice while one of them holds the options.
    pub fn game_correction_active(&self) -> bool {
        self.lock().nes.coherent_midline_copy()
    }
}

impl NesController {
    /// The loaded ROM's code-database keys, present ones only.
    fn database_crcs(&self) -> Vec<u32> {
        self.lock().rom_crcs.into_iter().flatten().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DEFAULT_SAMPLE_RATE;

    /// An NROM image whose program starts pulse 1 on a steady ~440 Hz tone at
    /// full constant volume and spins, so the drained audio carries a tone an
    /// equalizer can change. (Silence, or the mixer's DC level, passes a
    /// peaking EQ unchanged, which would make the comparisons below vacuous.)
    fn tone_nrom() -> Vec<u8> {
        let mut rom = vec![0u8; 16 + 16 * 1024 + 8 * 1024];
        rom[0..4].copy_from_slice(b"NES\x1a");
        rom[4] = 1;
        rom[5] = 1;
        let prg = 16;
        let program: [u8; 23] = [
            0xA9, 0x01, 0x8D, 0x15, 0x40, // LDA #$01 / STA $4015: pulse 1 on
            0xA9, 0xBF, 0x8D, 0x00, 0x40, // LDA #$BF / STA $4000: duty 2, halt, constant 15
            0xA9, 0xFD, 0x8D, 0x02, 0x40, // LDA #$FD / STA $4002: timer low
            0xA9, 0x08, 0x8D, 0x03, 0x40, // LDA #$08 / STA $4003: timer high 0, length load
            0x4C, 0x14, 0x80, // JMP $8014 (spin here)
        ];
        rom[prg..prg + program.len()].copy_from_slice(&program);
        let reset = prg + 0x3FFC;
        rom[reset] = 0x00;
        rom[reset + 1] = 0x80;
        rom
    }

    /// Drain `frames` frames of audio from `ctrl`.
    fn drain(ctrl: &NesController, frames: usize) -> Vec<f32> {
        let mut out = Vec::new();
        for _ in 0..frames {
            ctrl.step_frame();
            out.extend(ctrl.drain_audio());
        }
        out
    }

    fn bits(s: &[f32]) -> Vec<u32> {
        s.iter().map(|v| v.to_bits()).collect()
    }

    /// The default EQ is flat and changes nothing, bit for bit, on both drain
    /// paths; a boosted band changes the tone; flattening it again restores
    /// the bypass exactly.
    #[test]
    // Sample counts here are a few thousand, far below f32's 2^24 exact range.
    #[allow(clippy::cast_precision_loss)]
    fn a_flat_equalizer_is_bit_identical_and_a_boost_is_not() {
        let plain = NesController::new(tone_nrom(), DEFAULT_SAMPLE_RATE).unwrap();
        let eq = NesController::new(tone_nrom(), DEFAULT_SAMPLE_RATE).unwrap();
        assert!(!eq.equalizer_active());
        assert_eq!(eq.equalizer_gains(), vec![0.0; 20]);
        assert!(eq.set_equalizer_gains(vec![0.0; 20]), "flat is accepted");
        assert!(!eq.equalizer_active(), "flat installs no filter");

        let a = drain(&plain, 30);
        let b = drain(&eq, 30);
        assert!(
            a.iter().any(|&s| (s - a[a.len() / 2]).abs() > 1e-4),
            "the test ROM must produce a tone, not a constant"
        );
        assert_eq!(bits(&a), bits(&b), "a flat EQ leaves the drain untouched");

        // The byte drain applies the same stage.
        plain.step_frame();
        eq.step_frame();
        assert_eq!(plain.drain_audio_bytes(), eq.drain_audio_bytes());

        let mut gains = vec![0.0; 20];
        gains[6] = 12.0; // 400 Hz, right under the ~440 Hz tone
        assert!(eq.set_equalizer_gains(gains));
        assert!(eq.equalizer_active());
        let a = drain(&plain, 30);
        let b = drain(&eq, 30);
        assert_eq!(a.len(), b.len());
        assert_ne!(bits(&a), bits(&b), "a +12 dB band changes the tone");
        let rms = |s: &[f32]| (s.iter().map(|v| v * v).sum::<f32>() / s.len() as f32).sqrt();
        // Compare the AC energy (the mixer has a DC level a peaking band
        // passes at unity), so the boost is measured where it applies.
        let ac = |s: &[f32]| {
            let mean = s.iter().sum::<f32>() / s.len() as f32;
            rms(&s.iter().map(|v| v - mean).collect::<Vec<_>>())
        };
        assert!(
            ac(&b) > ac(&a) * 1.3,
            "boosted {} vs plain {}",
            ac(&b),
            ac(&a)
        );

        plain.step_frame();
        eq.step_frame();
        assert_ne!(plain.drain_audio_bytes(), eq.drain_audio_bytes());

        assert!(eq.set_equalizer_gains(vec![0.0; 20]));
        assert!(!eq.equalizer_active());
        let a = drain(&plain, 5);
        let b = drain(&eq, 5);
        assert_eq!(bits(&a), bits(&b), "flat again is the bypass again");
    }

    /// A wrong band count is refused and changes nothing; out-of-range and
    /// non-finite gains are clamped and zeroed; the gains survive a ROM load.
    #[test]
    fn equalizer_gains_are_validated_and_persist_across_a_load() {
        let ctrl = NesController::new(tone_nrom(), DEFAULT_SAMPLE_RATE).unwrap();
        assert!(!ctrl.set_equalizer_gains(vec![3.0; 19]));
        assert!(!ctrl.set_equalizer_gains(vec![3.0; 21]));
        assert!(!ctrl.set_equalizer_gains(Vec::new()));
        assert!(!ctrl.equalizer_active(), "a refused call changes nothing");

        let mut gains = vec![0.0; 20];
        gains[0] = 40.0;
        gains[1] = -40.0;
        gains[2] = f32::NAN;
        gains[3] = f32::INFINITY;
        gains[4] = 5.5;
        assert!(ctrl.set_equalizer_gains(gains));
        let got = ctrl.equalizer_gains();
        assert_eq!(&got[..5], &[12.0, -12.0, 0.0, 0.0, 5.5]);
        assert!(ctrl.equalizer_active());

        ctrl.load_rom(tone_nrom(), 44_100).unwrap();
        assert_eq!(
            ctrl.equalizer_gains(),
            got,
            "the preference outlives a game"
        );
        assert!(ctrl.equalizer_active());
        let out = drain(&ctrl, 10);
        assert!(
            out.iter().all(|s| s.is_finite()),
            "re-voiced at 44.1 kHz, stable"
        );
    }

    #[test]
    fn the_band_frequencies_are_the_desktops() {
        assert_eq!(equalizer_band_frequencies(), crate::eq::EQ20_FREQS.to_vec());
        assert_eq!(equalizer_band_frequencies().len(), 20);
    }

    /// Super Mario Bros.' header-excluded CRC (`0xD445_F698`, the key the
    /// re-keyed catalog lists it under) on a forged image: the database
    /// offers its codes, names the game, and every offered code is accepted
    /// by the existing add path. An unlisted ROM gets nothing.
    #[test]
    fn the_code_database_lists_a_known_games_codes() {
        let mut header = [0u8; 16];
        header[..4].copy_from_slice(b"NES\x1A");
        header[4] = 2;
        header[5] = 1;
        let smb = rustynes_gamedb::test_support::image_with_crc(header, 0xD445_F698);
        let ctrl = NesController::new(smb, DEFAULT_SAMPLE_RATE).unwrap();
        let codes = ctrl.cheat_database_codes();
        assert!(!codes.is_empty(), "SMB has catalogued codes");
        assert_eq!(
            ctrl.cheat_database_game().as_deref(),
            Some("Super Mario Bros.")
        );
        // Grouped by category, as the desktop's pick-list is.
        let cats: Vec<&str> = codes.iter().map(|c| c.category.as_str()).collect();
        let mut sorted = cats.clone();
        sorted.sort_unstable();
        assert_eq!(cats, sorted, "grouped by category");
        let mut seen = std::collections::HashSet::new();
        assert!(
            codes.iter().all(|c| seen.insert(c.code.clone())),
            "no duplicate code"
        );
        for c in &codes {
            ctrl.cheat_add_genie(c.code.clone())
                .expect("a listed code is valid");
        }
        // The core keys active codes by address (a later code for the same
        // address replaces the earlier one), and the catalog has alternatives
        // for one address, so the active count is the distinct addresses.
        let addrs: std::collections::BTreeSet<u16> = codes
            .iter()
            .map(|c| rustynes_core::GenieCode::new(&c.code).unwrap().addr())
            .collect();
        assert_eq!(ctrl.cheat_genie_codes().len(), addrs.len());

        let other = NesController::new(tone_nrom(), DEFAULT_SAMPLE_RATE).unwrap();
        assert_eq!(other.cheat_database_codes(), Vec::new());
        assert_eq!(other.cheat_database_game(), None);

        // The keys follow a load.
        other
            .load_rom(
                rustynes_gamedb::test_support::image_with_crc(header, 0xD445_F698),
                DEFAULT_SAMPLE_RATE,
            )
            .unwrap();
        assert_eq!(other.cheat_database_codes(), codes);
    }

    /// The candidate URLs follow Android's `BoxArt.kt` rules: extension dropped,
    /// invalid characters to `_`, full name then short name, spaces as `%20` and
    /// parentheses kept readable.
    #[test]
    fn box_art_candidates_follow_the_android_rules() {
        let base = BOX_ART_BASE;
        assert_eq!(
            box_art_candidate_urls("Mega Man 2 (USA).nes".into()),
            vec![
                format!("{base}Mega%20Man%202%20(USA).png"),
                format!("{base}Mega%20Man%202.png"),
            ]
        );
        // No tag: one candidate (the short name is the full name).
        assert_eq!(
            box_art_candidate_urls("Tetris.nes".into()),
            vec![format!("{base}Tetris.png")]
        );
        // libretro's invalid characters become `_`; `&` included.
        let urls = box_art_candidate_urls("Mario & Yoshi: A/B (Japan).zip".into());
        assert_eq!(
            urls[0],
            format!("{base}Mario%20_%20Yoshi_%20A_B%20(Japan).png")
        );
        // Apostrophes and commas stay readable; non-ASCII is UTF-8 percent-encoded.
        let urls = box_art_candidate_urls("Kirby's Adventure, Pokémon (USA).nes".into());
        assert_eq!(
            urls[0],
            format!("{base}Kirby's%20Adventure,%20Pok%C3%A9mon%20(USA).png")
        );
        // A name with no stem gives nothing to fetch.
        assert_eq!(box_art_candidate_urls("   ".into()), Vec::<String>::new());
        assert_eq!(box_art_candidate_urls("(USA)".into()).len(), 1);
    }

    #[test]
    fn a_pasted_code_is_identified() {
        let e = cheat_database_lookup("sxiopo".into()).expect("SXIOPO is catalogued");
        assert_eq!(e.code, "SXIOPO");
        assert_ne!(e.game, "");
        assert_eq!(cheat_database_lookup("ZZZZZZ".into()), None);
        assert_eq!(cheat_database_lookup("not a code".into()), None);
    }

    /// The switch is the player's: it defaults on, persists across a load,
    /// and on an unlisted ROM leaves the correction off either way.
    #[test]
    fn the_correction_switch_defaults_on_and_persists() {
        let ctrl = NesController::new(tone_nrom(), DEFAULT_SAMPLE_RATE).unwrap();
        assert!(ctrl.game_corrections_enabled());
        assert!(!ctrl.game_correction_listed());
        assert!(!ctrl.game_correction_active(), "an unlisted ROM gets none");
        ctrl.set_game_corrections_enabled(false);
        assert!(!ctrl.game_corrections_enabled());
        ctrl.load_rom(tone_nrom(), DEFAULT_SAMPLE_RATE).unwrap();
        assert!(!ctrl.game_corrections_enabled(), "it outlives a load");
        ctrl.set_game_corrections_enabled(true);
        ctrl.step_frame();
        assert!(
            !ctrl.game_correction_active(),
            "still none on an unlisted ROM"
        );
    }
}

/// `T-MOBILE-GAME-CORRECTIONS` — the gate on a real *Rad Racer* dump.
///
/// Needs your own dump at `tests/roms/external/mapper-001-MMC1/Rad Racer.nes`
/// (gitignored, never committed), the file the oracle's
/// `rad_racer_coherent_copy` test reads; without it the test says so and
/// passes, like every `commercial-roms` suite. Run with
/// `cargo test -p rustynes-mobile --features commercial-roms`.
///
/// It drives the oracle test's scripted race through the BRIDGE (START four
/// times, then the accelerator from frame 600) on two controllers, the
/// switch on and off, and checks each against a bare core built from the
/// same bytes with the correction set by hand:
///
/// - every frame of the bridge's OFF run equals the core's uncorrected frame,
///   and every frame of the ON run the corrected one, byte for byte;
/// - the two runs differ (so the comparison is not vacuous), and the
///   oracle's line-139 dash detector finds dashes only in the OFF run.
#[cfg(all(test, feature = "commercial-roms"))]
mod rad_racer {
    use rustynes_core::{Buttons, FrameInput, Nes};

    use crate::{DEFAULT_SAMPLE_RATE, NesController};

    const ROM: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/roms/external/mapper-001-MMC1/Rad Racer.nes"
    );
    const FIRST: u64 = 700;
    const LAST: u64 = 1100;

    const fn buttons_at(f: u64) -> Buttons {
        match f {
            120..=125 | 240..=245 | 360..=365 | 480..=485 => Buttons::START,
            600.. => Buttons::A,
            _ => Buttons::empty(),
        }
    }

    /// The oracle test's detector (`rad_racer_coherent_copy.rs`), over a
    /// little-endian `u16` index framebuffer: one whole wrong tile (8 pixels
    /// on an 8-pixel boundary) in the sand right of the road on line 139.
    fn line_139_has_a_dash(idx: &[u16]) -> bool {
        let mut counts = std::collections::BTreeMap::<u16, usize>::new();
        for y in 125..150 {
            for x in 236..256 {
                *counts.entry(idx[y * 256 + x]).or_default() += 1;
            }
        }
        let sand = counts
            .into_iter()
            .max_by_key(|&(_, n)| n)
            .map(|(c, _)| c)
            .unwrap();
        let row = &idx[139 * 256..140 * 256];
        let mut x = 255usize;
        while x > 0 && row[x] == sand {
            x -= 1;
        }
        let end = x;
        while x > 0 && row[x] != sand {
            x -= 1;
        }
        let width = end - x;
        let start = x + 1;
        width == 8 && start.is_multiple_of(8) && x >= 4 && row[x - 3..=x].iter().all(|&p| p == sand)
    }

    fn indices(bytes: &[u8]) -> Vec<u16> {
        bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&b| u16::from_le_bytes(b))
            .collect()
    }

    /// Run the race through the bridge, the switch set to `enabled`, against
    /// a core reference with the correction set to `enabled`; return the
    /// frames' dash count after asserting every frame matched the reference.
    fn race(rom: &[u8], enabled: bool) -> (u64, Vec<u64>) {
        let ctrl = NesController::new(rom.to_vec(), DEFAULT_SAMPLE_RATE).unwrap();
        assert!(ctrl.game_correction_listed(), "the dump is listed");
        ctrl.set_game_corrections_enabled(enabled);
        assert_eq!(ctrl.game_correction_active(), enabled);

        // The reference: the bytes the bridge builds from (decompression is
        // a no-op for a bare .nes; the database correction is applied the
        // same way), with the correction set by hand.
        let mut corrected = rom.to_vec();
        let crc = rustynes_gamedb::correct_rom(&mut corrected);
        let mut nes = Nes::from_rom_with_sample_rate(&corrected, DEFAULT_SAMPLE_RATE).unwrap();
        if let Some(crc) = crc {
            rustynes_gamedb::correct_console(&mut nes, crc);
        }
        nes.set_coherent_midline_copy(enabled);

        let mut dashes = 0;
        let mut hashes = Vec::new();
        for f in 0..=LAST {
            let b = buttons_at(f);
            ctrl.set_buttons(0, b.bits()).unwrap();
            let frame = ctrl.run_frame();
            FrameInput::four_players(b, Buttons::empty(), Buttons::empty(), Buttons::empty())
                .apply_to(&mut nes);
            nes.run_frame();
            assert!(
                frame == nes.framebuffer(),
                "frame {f}: the bridge's {} picture is not the core's",
                if enabled { "corrected" } else { "uncorrected" }
            );
            if f >= FIRST {
                let idx = indices(&ctrl.index_framebuffer_bytes());
                if line_139_has_a_dash(&idx) {
                    dashes += 1;
                }
                hashes.push(rustynes_gamedb::crc32(&frame).into());
            }
        }
        (dashes, hashes)
    }

    #[test]
    fn the_switch_selects_the_corrected_or_the_hardware_picture() {
        let Ok(rom) = std::fs::read(ROM) else {
            eprintln!("rad_racer (mobile): no dump at {ROM}; nothing to check");
            return;
        };
        let (on_dashes, on) = race(&rom, true);
        let (off_dashes, off) = race(&rom, false);
        eprintln!("line-139 dash frames {FIRST}..={LAST}: switch on {on_dashes}, off {off_dashes}");
        assert_ne!(on, off, "the two pictures differ, so the frame checks bite");
        assert_eq!(on_dashes, 0, "the correction draws no dash");
        assert!(
            off_dashes > 0,
            "the hardware's dash appears with the switch off"
        );
    }
}
