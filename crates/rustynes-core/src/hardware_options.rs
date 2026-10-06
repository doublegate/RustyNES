//! v2.9.8 — the emulation options a movie or a netplay session must agree on.
//!
//! The determinism contract is "same ROM + same seed + same input ⇒
//! bit-identical output". Until v2.9.8 a `.rnm` movie recorded the ROM and the
//! input and nothing else, and netplay compared only the ROM, so every host
//! knob that changes what the console *does* was left to whatever the player
//! happened to have configured: record on the Famicom model, replay on the NES
//! model, and the replay silently ran a different machine. The maintainer's
//! decision of 2026-10-01 is that a replay or a peer must never silently
//! diverge, so this module gives that knob set one canonical, serialisable
//! form, [`HardwareOptions`], which the movie stores in its header and netplay
//! folds into its handshake.
//!
//! Two kinds of fact are kept apart here because they are enforced
//! differently:
//!
//! - **[`HardwareOptions`] are host settings** — a console model, a die
//!   revision, a power-on fill, an overclock, a Game Genie code. The host can
//!   change them, so a movie *applies* them before frame 0 whatever the
//!   player's own settings are.
//! - **[`BoardDescription`] is what the cartridge header says** — mapper,
//!   submapper, mirroring, RAM sizes. Since v2.9.8 `Nes::rom_sha256` excludes
//!   the 16-byte header, so two images with the same PRG/CHR but different
//!   (or differently corrected) headers share an identity while building
//!   different machines. A host cannot apply a header, so a movie *checks* the
//!   description and refuses on a mismatch, naming the field.
//!
//! # What is deliberately not here
//!
//! The survey behind this list is in `docs/frontend.md` § "What a movie
//! records". In short: presentation (custom palette, the APU channel mask,
//! gain and analog filter model, the sample rate), performance selectors that
//! are proven byte-identical (the fast dot path), debug observability
//! (tracing, logging, breakpoints, provenance), and **inputs** — the two
//! standard pads are in the movie's input stream, while Four Score players
//! 3/4, expansion devices, the Famicom microphone, Vs. coins / service and FDS
//! disk swaps are per-frame inputs a `.rnm` does not carry (a documented limit
//! of the input stream, not an option).
//!
//! This module is `no_std`-clean: `core` + `alloc` and the save-state
//! `BinWriter` / `BinReader` primitives only.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use rustynes_mappers::{ConsoleType, Mirroring, VsPpuType};
use sha2::{Digest, Sha256};

use crate::Cpu2A03Revision;
use crate::Region;
use crate::genie::GenieCode;
use crate::nes::{ConsoleModel, Nes, PowerOnRam};
use crate::save_state::{BinReader, BinWriter};
use rustynes_ppu::{PaletteInit, PpuRevision};

/// Why an options or board block could not be decoded.
///
/// Carried by `MovieError::BadOptions`; a `&'static str` because every case
/// is a fixed fact about the bytes ("an unknown console-model byte"), never a
/// value worth echoing back from an untrusted file.
pub type OptionsDecodeError = &'static str;

/// The most Game Genie codes one record may carry. A real cheat list is a
/// handful of codes; the bound exists so a hostile count byte cannot ask the
/// decoder to loop or allocate without limit. 255 is what the count byte can
/// express, so it costs no legitimate list.
const MAX_GENIE_CODES: usize = u8::MAX as usize;

/// v3.0.0 (ADR 0045) — which emulator *behaviour* a movie or a netplay peer
/// expects, beside the options and the board that [`HardwareOptions`] and
/// [`BoardDescription`] describe.
///
/// Two builds with identical options can still emulate a game differently
/// whenever an accuracy fix lands: v3.0.0's T-MMC3-BG-A12, for instance,
/// moves the MMC3 IRQ for games with the background at `$1000`. Without a
/// record of which behaviour a recording assumes, a movie replays under the
/// new timing and a mixed-version netplay session desyncs, and neither says
/// why. `.rnm` format 5 records this number, and netplay protocol 6 sends it
/// in the handshake; a mismatch is refused, naming both epochs.
///
/// **The bump rule.** Increment it, in the same change, whenever a change
/// makes the core produce a different framebuffer, audio sample or bus cycle
/// from the same inputs than the last release did. Every such change already
/// re-blesses a golden or moves a commercial snapshot, so that is the
/// trigger to look for. Refactors, byte-identical performance work, frontend
/// features and new mapper families (which have no earlier output to differ
/// from) do not bump it. A release that moved goldens without raising it
/// breaks the promise this constant exists to keep.
///
/// It was 1 at v3.0.0, the first release to carry it. Earlier builds have no
/// epoch, and are refused by the movie format (5) and the protocol (6)
/// instead.
///
/// | epoch | release | what moved |
/// | --- | --- | --- |
/// | 1 | v3.0.0 | the MMC3 background-at-`$1000` A12 rule (T-MMC3-BG-A12) |
/// | 2 | v3.0.1 | mapper 45 CHR-RAM unbanked (T-GA23C-CHRRAM; *Famicom Yarou Vol.1*) |
pub const EMULATION_EPOCH: u32 = 2;

/// Every host-settable option that changes what the emulated console does.
///
/// [`Default`] is the stock NES: the configuration every release before the
/// option existed emulated, and what a foreign movie import (`.fm2`, `.bk2`,
/// `.fcm`, `.fmv`, `.vmv`, `.mc2`) records, because those formats cannot say
/// anything else.
///
/// One exception to "all defaults are the default build": the Vs. PPU type is
/// a property of the cartridge header that a host may override, so its
/// default is `None`, meaning "whatever the header declares" — applying a
/// stock `VsPpuType::None` to a Vs. cartridge would strip its RGB PPU.
///
/// `#[non_exhaustive]` since v3.0.0 (T-API-EXTENSIBLE): outside this crate,
/// start from [`HardwareOptions::default`] (the stock NES) or
/// [`HardwareOptions::capture`] and set the fields that differ. A later
/// option is then not a break.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
#[non_exhaustive]
pub struct HardwareOptions {
    /// Which console's reset wiring is modelled ([`Nes::set_console_model`]).
    pub console_model: ConsoleModel,
    /// The 2C02 die revision ([`Nes::set_ppu_revision`]).
    pub ppu_revision: PpuRevision,
    /// The 2A03 die revision ([`Nes::set_cpu_2a03_revision`]).
    pub cpu_2a03_revision: Cpu2A03Revision,
    /// The optional OAM-decay model ([`Nes::set_oam_decay`]).
    pub oam_decay: bool,
    /// The power-on work-RAM fill ([`Nes::set_power_on_ram`]).
    pub power_on_ram: PowerOnRam,
    /// The power-up palette-RAM contents ([`Nes::set_power_up_palette`]).
    pub power_up_palette: PaletteInit,
    /// The extra-vblank-scanline overclock ([`Nes::set_extra_scanlines`]).
    pub extra_scanlines: u16,
    /// Whether the Four Score adapter is plugged in ([`Nes::set_four_score`]).
    /// It changes `$4016` / `$4017` reads 9-24 even with players 3/4 idle.
    pub four_score: bool,
    /// The beam-relative Zapper light model
    /// ([`Nes::set_zapper_temporal_light`]).
    pub zapper_temporal_light: bool,
    /// The Vs. System DIP-switch bank ([`Nes::set_vs_dip`]); read through
    /// `$4016` / `$4017` on Vs. carts, inert elsewhere.
    pub vs_dip: u8,
    /// The Vs. System PPU type ([`Nes::set_vs_ppu_type`]); `None` = the
    /// header's. See the type-level note.
    pub vs_ppu_type: Option<VsPpuType>,
    /// The per-game nametable mirroring override
    /// ([`Nes::set_mirroring_override`]); `None` = the mapper decides.
    pub mirroring_override: Option<Mirroring>,
    /// The active Game Genie codes, canonical upper-case strings in address
    /// order ([`Nes::add_genie_code`]).
    pub genie_codes: Vec<String>,
}

impl Default for HardwareOptions {
    fn default() -> Self {
        Self {
            console_model: ConsoleModel::default(),
            ppu_revision: PpuRevision::default(),
            cpu_2a03_revision: Cpu2A03Revision::default(),
            oam_decay: false,
            power_on_ram: PowerOnRam::default(),
            power_up_palette: PaletteInit::default(),
            extra_scanlines: 0,
            four_score: false,
            // The core's own default since v2.2.x (`zapper_temporal_light`
            // is on in a freshly-built `Nes`); the stock machine, not `false`.
            zapper_temporal_light: true,
            vs_dip: 0,
            vs_ppu_type: None,
            mirroring_override: None,
            genie_codes: Vec::new(),
        }
    }
}

impl HardwareOptions {
    /// The options `nes` is running with right now.
    #[must_use]
    pub fn capture(nes: &Nes) -> Self {
        Self {
            console_model: nes.console_model(),
            ppu_revision: nes.ppu_revision(),
            cpu_2a03_revision: nes.cpu_2a03_revision(),
            oam_decay: nes.oam_decay_enabled(),
            power_on_ram: nes.power_on_ram(),
            power_up_palette: nes.power_up_palette(),
            extra_scanlines: nes.extra_scanlines(),
            four_score: nes.four_score(),
            zapper_temporal_light: nes.zapper_temporal_light(),
            vs_dip: nes.vs_dip(),
            vs_ppu_type: Some(nes.vs_ppu_type()),
            mirroring_override: nes.mirroring_override(),
            genie_codes: nes.genie_codes().map(|g| g.code().to_string()).collect(),
        }
    }

    /// Apply every option to `nes`, including the two power-on fills.
    ///
    /// The fills **write live state**: [`Nes::set_power_on_ram`] refills work
    /// RAM and the open-bus latch, and [`Nes::set_power_up_palette`] rewrites
    /// palette RAM. That is right before a power cycle or a save-state restore
    /// (which replace that state anyway) and wrong in the middle of a game,
    /// where [`Self::apply_live`] is the call to make.
    ///
    /// # Errors
    ///
    /// Returns the offending code if a Game Genie string does not decode.
    /// Codes read through [`Self::read_from`] are validated there, so this
    /// can only fail for a hand-built value.
    pub fn apply(&self, nes: &mut Nes) -> Result<(), String> {
        nes.set_power_on_ram(self.power_on_ram);
        nes.set_power_up_palette(self.power_up_palette);
        self.apply_live(nes)
    }

    /// Apply every option except the two power-on fills, never writing work
    /// RAM or palette RAM. Each knob is compared first and set only if it
    /// differs, so calling this once per frame to hold a movie's options in
    /// place costs a handful of comparisons.
    ///
    /// The fills are excluded because they act only at the next power cycle
    /// and applying them writes the running game's RAM; see [`Self::apply`].
    ///
    /// # Errors
    ///
    /// Returns the offending code if a Game Genie string does not decode.
    pub fn apply_live(&self, nes: &mut Nes) -> Result<(), String> {
        if nes.console_model() != self.console_model {
            nes.set_console_model(self.console_model);
        }
        if nes.ppu_revision() != self.ppu_revision {
            nes.set_ppu_revision(self.ppu_revision);
        }
        if nes.cpu_2a03_revision() != self.cpu_2a03_revision {
            nes.set_cpu_2a03_revision(self.cpu_2a03_revision);
        }
        if nes.oam_decay_enabled() != self.oam_decay {
            nes.set_oam_decay(self.oam_decay);
        }
        if nes.extra_scanlines() != self.extra_scanlines {
            nes.set_extra_scanlines(self.extra_scanlines);
        }
        if nes.four_score() != self.four_score {
            nes.set_four_score(self.four_score);
        }
        if nes.zapper_temporal_light() != self.zapper_temporal_light {
            nes.set_zapper_temporal_light(self.zapper_temporal_light);
        }
        if nes.vs_dip() != self.vs_dip {
            nes.set_vs_dip(self.vs_dip);
        }
        if let Some(t) = self.vs_ppu_type
            && nes.vs_ppu_type() != t
        {
            nes.set_vs_ppu_type(t);
        }
        if nes.mirroring_override() != self.mirroring_override {
            nes.set_mirroring_override(self.mirroring_override);
        }
        // The live codes iterate in address order, as `capture` records them,
        // so an unchanged list compares equal without sorting.
        if !nes
            .genie_codes()
            .map(GenieCode::code)
            .eq(self.genie_codes.iter().map(String::as_str))
        {
            nes.clear_genie_codes();
            for code in &self.genie_codes {
                nes.add_genie_code(code).map_err(|_| code.clone())?;
            }
        }
        Ok(())
    }

    /// Put `nes` back on these options after a movie ran on others, without
    /// disturbing the game in progress.
    ///
    /// [`Self::apply_live`] alone would leave the movie's power-on fills
    /// stored, so the player's next power cycle would boot with the movie's
    /// RAM pattern. Storing the player's fills means calling their setters,
    /// which write work RAM and palette RAM, so this takes a snapshot first
    /// and restores it afterwards: the snapshot carries RAM, the open-bus
    /// latch and palette RAM but not configuration, so the restore puts the
    /// running game back exactly while the stored fills stay the player's.
    /// Skipped when the fills already match, which is the common case.
    ///
    /// # Errors
    ///
    /// Returns the offending code if a Game Genie string does not decode.
    pub fn restore_after_playback(&self, nes: &mut Nes) -> Result<(), String> {
        if nes.power_on_ram() != self.power_on_ram
            || nes.power_up_palette() != self.power_up_palette
        {
            let snap = nes.snapshot();
            nes.set_power_on_ram(self.power_on_ram);
            nes.set_power_up_palette(self.power_up_palette);
            // A snapshot this build just took of this machine always
            // restores; `restore_quiet` keeps the rewind history, which still
            // describes the timeline the player is on.
            let restored = nes.restore_quiet(&snap);
            debug_assert!(restored.is_ok(), "own snapshot must restore");
        }
        self.apply_live(nes)
    }

    /// Append the canonical encoding to `w`.
    ///
    /// Layout (all little-endian): console model, PPU revision, 2A03
    /// revision, OAM decay, power-on RAM kind + `u64` payload, power-up
    /// palette, extra scanlines (`u16`), Four Score, Zapper light model, Vs.
    /// DIP, Vs. PPU type (`0xFF` = the header's), mirroring override (`0` =
    /// none, else variant + 1), then a code count and each Game Genie code as
    /// a length byte plus ASCII. Every enum is an explicit byte, never a
    /// discriminant cast, so reordering a Rust enum cannot silently change
    /// what an old file means.
    pub fn write_to(&self, w: &mut BinWriter) {
        w.u8(match self.console_model {
            ConsoleModel::Nes => 0,
            ConsoleModel::Famicom => 1,
        });
        w.u8(match self.ppu_revision {
            PpuRevision::Rp2c02H => 0,
            PpuRevision::Rp2c02G => 1,
        });
        w.u8(match self.cpu_2a03_revision {
            Cpu2A03Revision::Rp2A03G => 0,
            Cpu2A03Revision::Rp2A03H => 1,
        });
        w.u8(u8::from(self.oam_decay));
        let (kind, value) = match self.power_on_ram {
            PowerOnRam::Zeroed => (0u8, 0u64),
            PowerOnRam::Seeded(seed) => (1, seed),
            PowerOnRam::Filled(byte) => (2, u64::from(byte)),
        };
        w.u8(kind);
        w.u64(value);
        w.u8(match self.power_up_palette {
            PaletteInit::Zeroed => 0,
            PaletteInit::Blargg => 1,
        });
        w.u16(self.extra_scanlines);
        w.u8(u8::from(self.four_score));
        w.u8(u8::from(self.zapper_temporal_light));
        w.u8(self.vs_dip);
        w.u8(self.vs_ppu_type.map_or(0xFF, vs_ppu_to_byte));
        w.u8(self
            .mirroring_override
            .map_or(0, |m| mirroring_to_byte(m) + 1));
        let count = self.genie_codes.len().min(MAX_GENIE_CODES);
        w.u8(u8::try_from(count).unwrap_or(u8::MAX));
        for code in self.genie_codes.iter().take(count) {
            let bytes = code.as_bytes();
            w.u8(u8::try_from(bytes.len()).unwrap_or(u8::MAX));
            w.bytes(&bytes[..bytes.len().min(usize::from(u8::MAX))]);
        }
    }

    /// Decode what [`Self::write_to`] wrote. Strict: an unknown enum byte, a
    /// boolean other than 0 or 1, or a Game Genie code that does not decode is
    /// an error, never a silent default, because a default here is exactly
    /// the silent divergence this record exists to prevent.
    ///
    /// # Errors
    ///
    /// A fixed description of the first malformed field, or `"truncated"`.
    pub fn read_from(r: &mut BinReader<'_>) -> Result<Self, OptionsDecodeError> {
        let console_model = match byte(r)? {
            0 => ConsoleModel::Nes,
            1 => ConsoleModel::Famicom,
            _ => return Err("unknown console-model byte"),
        };
        let ppu_revision = match byte(r)? {
            0 => PpuRevision::Rp2c02H,
            1 => PpuRevision::Rp2c02G,
            _ => return Err("unknown PPU-revision byte"),
        };
        let cpu_2a03_revision = match byte(r)? {
            0 => Cpu2A03Revision::Rp2A03G,
            1 => Cpu2A03Revision::Rp2A03H,
            _ => return Err("unknown 2A03-revision byte"),
        };
        let oam_decay = flag(r, "OAM-decay flag is not 0 or 1")?;
        let kind = byte(r)?;
        let value = r.u64().map_err(|_| "truncated")?;
        let power_on_ram = match kind {
            0 if value == 0 => PowerOnRam::Zeroed,
            1 => PowerOnRam::Seeded(value),
            2 => {
                PowerOnRam::Filled(u8::try_from(value).map_err(|_| "power-on fill is not a byte")?)
            }
            _ => return Err("unknown power-on RAM kind"),
        };
        let power_up_palette = match byte(r)? {
            0 => PaletteInit::Zeroed,
            1 => PaletteInit::Blargg,
            _ => return Err("unknown power-up palette byte"),
        };
        let extra_scanlines = r.u16().map_err(|_| "truncated")?;
        // NC-11 (v2.9.9): the core clamps the overclock, so a larger value
        // could only come from an edited file, and would not replay as
        // written.
        if extra_scanlines > crate::nes::MAX_EXTRA_SCANLINES {
            return Err("extra-scanline overclock is above the core's maximum");
        }
        let four_score = flag(r, "Four Score flag is not 0 or 1")?;
        let zapper_temporal_light = flag(r, "Zapper light flag is not 0 or 1")?;
        let vs_dip = byte(r)?;
        let vs_ppu_type = match byte(r)? {
            0xFF => None,
            b => Some(vs_ppu_from_byte(b).ok_or("unknown Vs. PPU type byte")?),
        };
        let mirroring_override = match byte(r)? {
            0 => None,
            b => Some(mirroring_from_byte(b - 1).ok_or("unknown mirroring byte")?),
        };
        let count = usize::from(byte(r)?);
        // Canonical form, as `capture` records it and the console holds it:
        // one code per address, the later one winning, in address order.
        // A list in any other shape never compared equal to the live one,
        // so `apply_live` cleared and re-added every code on every frame
        // (NC-13, v2.9.9).
        let mut by_addr = alloc::collections::BTreeMap::new();
        for _ in 0..count {
            let len = usize::from(byte(r)?);
            let raw = r.take(len).map_err(|_| "truncated")?;
            let text = core::str::from_utf8(raw).map_err(|_| "Game Genie code is not text")?;
            let code = GenieCode::new(text).map_err(|_| "Game Genie code does not decode")?;
            by_addr.insert(code.addr(), code.code().to_string());
        }
        let genie_codes: Vec<String> = by_addr.into_values().collect();
        Ok(Self {
            console_model,
            ppu_revision,
            cpu_2a03_revision,
            oam_decay,
            power_on_ram,
            power_up_palette,
            extra_scanlines,
            four_score,
            zapper_temporal_light,
            vs_dip,
            vs_ppu_type,
            mirroring_override,
            genie_codes,
        })
    }

    /// The canonical encoding as a byte vector.
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut w = BinWriter::with_capacity(32);
        self.write_to(&mut w);
        w.into_vec()
    }

    /// Every option that differs between `self` and `other`, by name, in
    /// field order. Empty when they agree. For an error message a person can
    /// act on: "OAM decay, console model" rather than "the options differ".
    #[must_use]
    pub fn differences(&self, other: &Self) -> Vec<&'static str> {
        let mut out = Vec::new();
        let mut check = |differs: bool, name: &'static str| {
            if differs {
                out.push(name);
            }
        };
        check(self.console_model != other.console_model, "console model");
        check(self.ppu_revision != other.ppu_revision, "PPU revision");
        check(
            self.cpu_2a03_revision != other.cpu_2a03_revision,
            "2A03 revision",
        );
        check(self.oam_decay != other.oam_decay, "OAM decay");
        check(self.power_on_ram != other.power_on_ram, "power-on RAM");
        check(
            self.power_up_palette != other.power_up_palette,
            "power-up palette",
        );
        check(
            self.extra_scanlines != other.extra_scanlines,
            "overclock scanlines",
        );
        check(self.four_score != other.four_score, "Four Score");
        check(
            self.zapper_temporal_light != other.zapper_temporal_light,
            "Zapper light model",
        );
        check(self.vs_dip != other.vs_dip, "Vs. DIP switches");
        check(self.vs_ppu_type != other.vs_ppu_type, "Vs. PPU type");
        check(
            self.mirroring_override != other.mirroring_override,
            "mirroring override",
        );
        check(self.genie_codes != other.genie_codes, "Game Genie codes");
        out
    }
}

/// What the cartridge header told the core to build, after any load-time
/// correction: the facts that change emulation and that a host cannot apply.
///
/// Region is not here: a movie has always recorded it in its fixed header,
/// and netplay folds it into [`config_digest`] beside this.
///
/// `#[non_exhaustive]` since v3.0.0 (T-API-EXTENSIBLE): build one with
/// [`BoardDescription::capture`]. v2.9.9 added three fields to it, each a
/// break; a later field no longer is.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[non_exhaustive]
pub struct BoardDescription {
    /// iNES / NES 2.0 mapper number.
    pub mapper_id: u16,
    /// NES 2.0 submapper (0 for iNES 1.0).
    pub submapper: u8,
    /// Header mirroring.
    pub mirroring: Mirroring,
    /// Console type (NES / Vs. System / PlayChoice-10 / extended).
    pub console_type: ConsoleType,
    /// Whether the header marks a Vs. `DualSystem` board.
    pub vs_dual_system: bool,
    /// PRG-RAM bytes (volatile + battery).
    pub prg_ram_size: u32,
    /// CHR-RAM bytes.
    pub chr_ram_size: u32,
    /// Battery-backed save RAM present.
    pub has_battery: bool,
    /// 512-byte trainer present.
    pub has_trainer: bool,
    /// PRG-ROM bytes. The ROM identity hashes the body after the header, so
    /// the same body split differently between PRG and CHR (2 x 16 KiB +
    /// 4 x 8 KiB against 1 x 16 KiB + 6 x 8 KiB) shared an identity and a
    /// description until v2.9.9 (core re-audit NC-10).
    pub prg_rom_len: u32,
    /// CHR-ROM bytes (0 for CHR-RAM boards); see `prg_rom_len`.
    pub chr_rom_len: u32,
    /// The raw header nametable bits mappers 30 and 218 wire from
    /// ([`rustynes_mappers::Cartridge::nametable_wiring_bits`]); v2.9.9.
    pub nametable_wiring_bits: u8,
}

impl BoardDescription {
    /// The board `nes` was built from.
    #[must_use]
    #[allow(clippy::cast_possible_truncation)] // ROM sizes are far below 4 GiB
    pub fn capture(nes: &Nes) -> Self {
        let c = nes.cartridge();
        Self {
            mapper_id: c.mapper_id,
            submapper: c.submapper,
            mirroring: c.mirroring,
            console_type: c.console_type,
            vs_dual_system: c.vs_dual_system,
            prg_ram_size: c.prg_ram_size,
            chr_ram_size: c.chr_ram_size,
            has_battery: c.has_battery,
            has_trainer: c.has_trainer,
            prg_rom_len: c.prg_rom.len() as u32,
            chr_rom_len: c.chr_rom.len() as u32,
            nametable_wiring_bits: c.nametable_wiring_bits,
        }
    }

    /// The first field that differs between `self` (recorded) and `other`
    /// (the host's), or `None` when the boards agree.
    #[must_use]
    pub fn first_difference(&self, other: &Self) -> Option<&'static str> {
        [
            (self.mapper_id != other.mapper_id, "mapper"),
            (self.submapper != other.submapper, "submapper"),
            (self.mirroring != other.mirroring, "mirroring"),
            (self.console_type != other.console_type, "console type"),
            (
                self.vs_dual_system != other.vs_dual_system,
                "Vs. DualSystem",
            ),
            (self.prg_ram_size != other.prg_ram_size, "PRG-RAM size"),
            (self.chr_ram_size != other.chr_ram_size, "CHR-RAM size"),
            (self.has_battery != other.has_battery, "battery"),
            (self.has_trainer != other.has_trainer, "trainer"),
            (self.prg_rom_len != other.prg_rom_len, "PRG-ROM size"),
            (self.chr_rom_len != other.chr_rom_len, "CHR-ROM size"),
            (
                self.nametable_wiring_bits != other.nametable_wiring_bits,
                "header nametable wiring",
            ),
        ]
        .into_iter()
        .find_map(|(differs, name)| differs.then_some(name))
    }

    /// Append the canonical encoding to `w` (`u16` mapper, submapper,
    /// mirroring, console type, `DualSystem`, `u32` PRG-RAM, `u32` CHR-RAM,
    /// battery, trainer, and from v2.9.9 `u32` PRG-ROM, `u32` CHR-ROM and the
    /// wiring byte).
    pub fn write_to(&self, w: &mut BinWriter) {
        w.u16(self.mapper_id);
        w.u8(self.submapper);
        w.u8(mirroring_to_byte(self.mirroring));
        w.u8(match self.console_type {
            ConsoleType::Nes => 0,
            ConsoleType::VsSystem => 1,
            ConsoleType::Playchoice10 => 2,
            ConsoleType::Extended => 3,
        });
        w.u8(u8::from(self.vs_dual_system));
        w.u32(self.prg_ram_size);
        w.u32(self.chr_ram_size);
        w.u8(u8::from(self.has_battery));
        w.u8(u8::from(self.has_trainer));
        w.u32(self.prg_rom_len);
        w.u32(self.chr_rom_len);
        w.u8(self.nametable_wiring_bits);
    }

    /// Decode what [`Self::write_to`] wrote. Strict, as
    /// [`HardwareOptions::read_from`].
    ///
    /// # Errors
    ///
    /// A fixed description of the first malformed field, or `"truncated"`.
    pub fn read_from(r: &mut BinReader<'_>) -> Result<Self, OptionsDecodeError> {
        let mapper_id = r.u16().map_err(|_| "truncated")?;
        let submapper = byte(r)?;
        let mirroring = mirroring_from_byte(byte(r)?).ok_or("unknown board mirroring byte")?;
        let console_type = match byte(r)? {
            0 => ConsoleType::Nes,
            1 => ConsoleType::VsSystem,
            2 => ConsoleType::Playchoice10,
            3 => ConsoleType::Extended,
            _ => return Err("unknown console-type byte"),
        };
        let vs_dual_system = flag(r, "DualSystem flag is not 0 or 1")?;
        let prg_ram_size = r.u32().map_err(|_| "truncated")?;
        let chr_ram_size = r.u32().map_err(|_| "truncated")?;
        let has_battery = flag(r, "battery flag is not 0 or 1")?;
        let has_trainer = flag(r, "trainer flag is not 0 or 1")?;
        let prg_rom_len = r.u32().map_err(|_| "truncated")?;
        let chr_rom_len = r.u32().map_err(|_| "truncated")?;
        let nametable_wiring_bits = byte(r)?;
        if nametable_wiring_bits & !0x09 != 0 {
            return Err("header nametable wiring byte has bits other than 0 and 3");
        }
        Ok(Self {
            mapper_id,
            submapper,
            mirroring,
            console_type,
            vs_dual_system,
            prg_ram_size,
            chr_ram_size,
            has_battery,
            has_trainer,
            prg_rom_len,
            chr_rom_len,
            nametable_wiring_bits,
        })
    }
}

/// SHA-256 over everything two machines must share to run one timeline from
/// the same ROM: the region, the [`BoardDescription`] and the
/// [`HardwareOptions`], each in its canonical encoding.
///
/// Netplay sends it in the handshake beside the ROM hash, so peers on the
/// same game with different settings (or differently corrected headers)
/// refuse with a reason instead of connecting and desyncing at the first
/// frame the difference reaches. SHA-256 rather than a 64-bit hash because the
/// value is compared, never searched, and 32 bytes on a once-per-session
/// message cost nothing.
#[must_use]
pub fn config_digest(nes: &Nes) -> [u8; 32] {
    let mut w = BinWriter::with_capacity(64);
    w.u8(match nes.region() {
        Region::Ntsc => 0,
        Region::Pal => 1,
        Region::Dendy => 2,
    });
    BoardDescription::capture(nes).write_to(&mut w);
    HardwareOptions::capture(nes).write_to(&mut w);
    let mut out = [0u8; 32];
    out.copy_from_slice(&Sha256::digest(w.into_vec()));
    out
}

fn byte(r: &mut BinReader<'_>) -> Result<u8, OptionsDecodeError> {
    r.u8().map_err(|_| "truncated")
}

fn flag(r: &mut BinReader<'_>, what: OptionsDecodeError) -> Result<bool, OptionsDecodeError> {
    match byte(r)? {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(what),
    }
}

const fn mirroring_to_byte(m: Mirroring) -> u8 {
    match m {
        Mirroring::Horizontal => 0,
        Mirroring::Vertical => 1,
        Mirroring::SingleScreenA => 2,
        Mirroring::SingleScreenB => 3,
        Mirroring::FourScreen => 4,
        Mirroring::MapperControlled => 5,
    }
}

const fn mirroring_from_byte(b: u8) -> Option<Mirroring> {
    Some(match b {
        0 => Mirroring::Horizontal,
        1 => Mirroring::Vertical,
        2 => Mirroring::SingleScreenA,
        3 => Mirroring::SingleScreenB,
        4 => Mirroring::FourScreen,
        5 => Mirroring::MapperControlled,
        _ => return None,
    })
}

const fn vs_ppu_to_byte(t: VsPpuType) -> u8 {
    match t {
        VsPpuType::None => 0,
        VsPpuType::Rp2C03 => 1,
        VsPpuType::Rp2C04_0001 => 2,
        VsPpuType::Rp2C04_0002 => 3,
        VsPpuType::Rp2C04_0003 => 4,
        VsPpuType::Rp2C04_0004 => 5,
        VsPpuType::Rc2C05_01 => 6,
        VsPpuType::Rc2C05_02 => 7,
        VsPpuType::Rc2C05_03 => 8,
        VsPpuType::Rc2C05_04 => 9,
    }
}

const fn vs_ppu_from_byte(b: u8) -> Option<VsPpuType> {
    Some(match b {
        0 => VsPpuType::None,
        1 => VsPpuType::Rp2C03,
        2 => VsPpuType::Rp2C04_0001,
        3 => VsPpuType::Rp2C04_0002,
        4 => VsPpuType::Rp2C04_0003,
        5 => VsPpuType::Rp2C04_0004,
        6 => VsPpuType::Rc2C05_01,
        7 => VsPpuType::Rc2C05_02,
        8 => VsPpuType::Rc2C05_03,
        9 => VsPpuType::Rc2C05_04,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal NROM image (an infinite `JMP`), as `movie.rs` uses.
    fn synth_nrom() -> Vec<u8> {
        let mut bytes = alloc::vec![b'N', b'E', b'S', 0x1A, 1, 1, 0, 0];
        bytes.extend_from_slice(&[0u8; 8]);
        let mut prg = alloc::vec![0u8; 16 * 1024];
        prg[..3].copy_from_slice(&[0x4C, 0x00, 0xC0]);
        let len = prg.len();
        prg[len - 6..].copy_from_slice(&[0x00, 0xC0, 0x00, 0xC0, 0x00, 0xC0]);
        bytes.extend_from_slice(&prg);
        bytes.extend_from_slice(&[0u8; 8 * 1024]);
        bytes
    }

    fn non_default() -> HardwareOptions {
        HardwareOptions {
            console_model: ConsoleModel::Famicom,
            ppu_revision: PpuRevision::Rp2c02G,
            cpu_2a03_revision: Cpu2A03Revision::Rp2A03H,
            oam_decay: true,
            power_on_ram: PowerOnRam::Seeded(0x0123_4567_89AB_CDEF),
            power_up_palette: PaletteInit::Blargg,
            extra_scanlines: 40,
            four_score: true,
            zapper_temporal_light: false,
            vs_dip: 0xA5,
            vs_ppu_type: Some(VsPpuType::Rc2C05_03),
            mirroring_override: Some(Mirroring::SingleScreenB),
            // Address order, the canonical form `read_from` produces.
            genie_codes: alloc::vec!["AAEAULPA".to_string(), "SXIOPO".to_string()],
        }
    }

    /// NC-13 (v2.9.9 re-audit): a list in file order, or with a duplicate,
    /// decodes to the console's own form, so `capture` of the machine it
    /// configures compares equal and `apply_live` stops re-adding the codes.
    #[test]
    fn genie_codes_decode_to_the_canonical_order_without_duplicates() {
        let canonical = non_default();
        for list in [
            alloc::vec!["SXIOPO", "AAEAULPA"],
            alloc::vec!["AAEAULPA", "SXIOPO", "SXIOPO"],
            alloc::vec!["sxiopo", "AAEAULPA"],
        ] {
            let opts = HardwareOptions {
                genie_codes: list.iter().map(|c| (*c).to_string()).collect(),
                ..non_default()
            };
            let decoded = HardwareOptions::read_from(&mut BinReader::new(&opts.to_bytes()));
            assert_eq!(decoded, Ok(canonical.clone()), "list {list:?}");
        }
    }

    /// NC-11 (v2.9.9 re-audit): an overclock above the core's maximum is
    /// refused rather than replayed, and the maximum itself decodes.
    #[test]
    fn an_overclock_above_the_maximum_is_refused() {
        for (lines, ok) in [
            (crate::nes::MAX_EXTRA_SCANLINES, true),
            (crate::nes::MAX_EXTRA_SCANLINES + 1, false),
            (u16::MAX, false),
        ] {
            let opts = HardwareOptions {
                extra_scanlines: lines,
                ..HardwareOptions::default()
            };
            let decoded = HardwareOptions::read_from(&mut BinReader::new(&opts.to_bytes()));
            assert_eq!(decoded.is_ok(), ok, "extra_scanlines {lines}");
        }
    }

    #[test]
    fn options_round_trip_through_their_encoding() {
        for opts in [HardwareOptions::default(), non_default()] {
            let bytes = opts.to_bytes();
            let mut r = BinReader::new(&bytes);
            assert_eq!(HardwareOptions::read_from(&mut r), Ok(opts));
            assert_eq!(r.remaining(), 0, "the decoder consumes exactly the record");
        }
        let filled = HardwareOptions {
            power_on_ram: PowerOnRam::Filled(0xFF),
            ..HardwareOptions::default()
        };
        let bytes = filled.to_bytes();
        assert_eq!(
            HardwareOptions::read_from(&mut BinReader::new(&bytes)),
            Ok(filled)
        );
    }

    #[test]
    fn every_truncation_is_an_error_not_a_default() {
        let bytes = non_default().to_bytes();
        for len in 0..bytes.len() {
            assert!(
                HardwareOptions::read_from(&mut BinReader::new(&bytes[..len])).is_err(),
                "a record cut at {len} bytes must not decode"
            );
        }
    }

    #[test]
    fn an_unknown_byte_is_refused() {
        let mut bytes = HardwareOptions::default().to_bytes();
        bytes[0] = 7; // console model
        assert_eq!(
            HardwareOptions::read_from(&mut BinReader::new(&bytes)),
            Err("unknown console-model byte")
        );
        let mut bytes = HardwareOptions::default().to_bytes();
        bytes[3] = 2; // OAM decay flag
        assert!(HardwareOptions::read_from(&mut BinReader::new(&bytes)).is_err());
    }

    #[test]
    fn differences_name_each_differing_option() {
        let a = HardwareOptions::default();
        assert!(a.differences(&a).is_empty());
        let b = HardwareOptions {
            oam_decay: true,
            console_model: ConsoleModel::Famicom,
            ..HardwareOptions::default()
        };
        assert_eq!(a.differences(&b), ["console model", "OAM decay"]);
    }

    #[test]
    fn a_fresh_machine_captures_the_stock_options_and_its_own_vs_type() {
        let rom = synth_nrom();
        let nes = Nes::from_rom(&rom).unwrap();
        let captured = HardwareOptions::capture(&nes);
        assert_eq!(
            captured,
            HardwareOptions {
                vs_ppu_type: Some(VsPpuType::None),
                ..HardwareOptions::default()
            }
        );
    }

    #[test]
    fn board_round_trips_and_names_the_differing_field() {
        let rom = synth_nrom();
        let nes = Nes::from_rom(&rom).unwrap();
        let board = BoardDescription::capture(&nes);
        let mut w = BinWriter::new();
        board.write_to(&mut w);
        let bytes = w.into_vec();
        assert_eq!(
            BoardDescription::read_from(&mut BinReader::new(&bytes)),
            Ok(board)
        );
        let other = BoardDescription {
            submapper: board.submapper + 1,
            ..board
        };
        assert_eq!(board.first_difference(&other), Some("submapper"));
        assert_eq!(board.first_difference(&board), None);
    }

    #[test]
    fn the_config_digest_follows_every_option() {
        let rom = synth_nrom();
        let mut nes = Nes::from_rom(&rom).unwrap();
        let base = config_digest(&nes);
        assert_eq!(base, config_digest(&Nes::from_rom(&rom).unwrap()));
        nes.set_oam_decay(true);
        assert_ne!(config_digest(&nes), base);
    }

    /// NC-10 (v2.9.9 re-audit): the identity hashes the body after the
    /// header, so headers that build different machines from one body must
    /// differ in the board description, which movies and netplay check.
    #[test]
    fn headers_that_build_different_machines_differ_in_the_description() {
        // One 64 KiB body: CNROM read as 2 x 16 KiB PRG + 4 x 8 KiB CHR, and
        // as 1 x 16 KiB PRG + 6 x 8 KiB CHR.
        let body: Vec<u8> = (0u32..64 * 1024).map(|i| (i % 251) as u8).collect();
        let image = |prg16: u8, chr8: u8, flags6: u8, mapper: u8| {
            let mut v = alloc::vec![
                b'N',
                b'E',
                b'S',
                0x1A,
                prg16,
                chr8,
                flags6 | (mapper << 4),
                mapper & 0xF0
            ];
            v.extend_from_slice(&[0u8; 8]);
            v.extend_from_slice(&body);
            v
        };
        let a = Nes::from_rom(&image(2, 4, 0, 3)).unwrap();
        let b = Nes::from_rom(&image(1, 6, 0, 3)).unwrap();
        assert_eq!(a.rom_sha256(), b.rom_sha256(), "one identity");
        assert_eq!(
            BoardDescription::capture(&a).first_difference(&BoardDescription::capture(&b)),
            Some("PRG-ROM size")
        );
        assert_ne!(config_digest(&a), config_digest(&b));

        // Mapper 218: four-screen with bit 0 set or clear wires CIRAM A10 to
        // a different PPU line; both read as `FourScreen`.
        let c = Nes::from_rom(&image(2, 0, 0x08, 218)).unwrap();
        let d = Nes::from_rom(&image(2, 0, 0x09, 218)).unwrap();
        assert_eq!(
            BoardDescription::capture(&c).first_difference(&BoardDescription::capture(&d)),
            Some("header nametable wiring")
        );
    }

    #[test]
    fn a_wiring_byte_with_other_bits_is_refused() {
        let board = BoardDescription::capture(&Nes::from_rom(&synth_nrom()).unwrap());
        let mut w = BinWriter::with_capacity(32);
        board.write_to(&mut w);
        let mut bytes = w.into_vec();
        let last = bytes.len() - 1;
        bytes[last] = 0x02;
        assert!(BoardDescription::read_from(&mut BinReader::new(&bytes)).is_err());
    }
}
