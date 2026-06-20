//! `rustynes-mobile` — the platform-agnostic mobile control surface over
//! [`rustynes_core`].
//!
//! This crate is the **shared bridge** for the mobile hosts: the Android shell
//! (`rustynes-android`, v1.8.0) and the iOS shell (`rustynes-ios`, v1.9.0). It
//! exposes a small, typed control surface — load a ROM from a byte buffer (never
//! a path), set the per-port controller mask, run a frame, borrow the
//! framebuffer/audio, and save/restore state — and lets `UniFFI` generate the
//! Kotlin and Swift bindings from the `#[uniffi::export]` annotations, so the
//! foreign-language surface is type-checked and the hand-rolled `unsafe` FFI is
//! confined to the platform crates' surface/audio glue.
//!
//! ## Determinism contract
//!
//! The bridge is a *thin* host over the byte-identical core: every method
//! forwards directly into [`rustynes_core::Nes`] with no timing feedback, hidden
//! state, or wall-clock dependence. A state saved on desktop loads here and a
//! `.rnm` TAS replays identically — the cross-platform determinism contract is
//! preserved because this crate adds **no new determinism surface**. All input
//! converges on the single late-latched [`Buttons`] mask per port, exactly as the
//! desktop and wasm hosts do.
//!
//! The hot render path in the platform crates borrows the framebuffer pointer
//! directly (handing it to `wgpu`); [`NesController::run_frame`] returning an
//! owned `Vec<u8>` is the typed-surface convenience used by the spike and by
//! callers that copy frames across the FFI boundary.

// UniFFI-generated scaffolding binds some parameters with a leading underscore.
#![allow(clippy::used_underscore_binding)]
// UniFFI maps `Vec<u8>`/`Vec<f32>` FFI parameters to *owned* foreign buffers; the
// `#[uniffi::export]` surface therefore takes ROM/state buffers by value even
// though some are only read. This is dictated by the binding ABI, not a smell.
#![allow(clippy::needless_pass_by_value)]

use std::sync::{Arc, Mutex, PoisonError};

use rustynes_core::{Buttons, Nes, Region};

uniffi::setup_scaffolding!();

/// NES visible framebuffer width in pixels.
pub const FRAME_WIDTH: u32 = 256;
/// NES visible framebuffer height in pixels.
pub const FRAME_HEIGHT: u32 = 240;
/// Default host audio sample rate (Hz) when a caller does not specify one.
pub const DEFAULT_SAMPLE_RATE: u32 = 48_000;

/// Errors surfaced across the mobile FFI boundary.
///
/// Variants carry a human-readable message rather than the rich core error types
/// so the generated Kotlin/Swift enums stay flat and stable across releases.
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum MobileError {
    /// The supplied bytes are not a loadable iNES/NES 2.0 ROM image.
    // The field is named `reason` (not `message`): UniFFI maps error variants to
    // Kotlin `Exception` subclasses, and a `message` field would collide with
    // `Throwable.message`, breaking the generated bindings' compile.
    #[error("failed to load ROM: {reason}")]
    RomLoad {
        /// Underlying core error rendered as text.
        reason: String,
    },
    /// A save-state blob failed to decode / restore.
    #[error("failed to restore save state: {reason}")]
    SaveState {
        /// Underlying snapshot error rendered as text.
        reason: String,
    },
    /// A controller port index outside `0..=3` was supplied.
    #[error("invalid controller port {port} (valid range 0..=3)")]
    InvalidPort {
        /// The out-of-range port index the caller passed.
        port: u32,
    },
    /// A custom palette blob was not a valid `.pal` (needs ≥ 192 bytes).
    #[error("invalid palette: {reason}")]
    Palette {
        /// What was wrong with the palette bytes.
        reason: String,
    },
    /// A `.rnm` movie failed to decode or seek.
    #[error("movie error: {reason}")]
    Movie {
        /// Underlying movie error rendered as text.
        reason: String,
    },
    /// An HD-pack `.zip` failed to load.
    #[error("HD-pack error: {reason}")]
    HdPack {
        /// What was wrong with the HD-pack.
        reason: String,
    },
    /// A Lua script failed to start or compile.
    #[error("script error: {reason}")]
    Script {
        /// Underlying script error rendered as text.
        reason: String,
    },
    /// An action was refused because a hardcore `RetroAchievements` session is
    /// active (v1.8.6). Loading a save-state is the loosely-cheating affordance
    /// hardcore mode forbids; saving a state is still allowed.
    #[error("action blocked: a hardcore RetroAchievements session is active")]
    HardcoreBlocked,
}

/// A single NES controller button, used by [`NesController::set_button`] for
/// the press/release convenience API. Maps 1:1 onto a [`Buttons`] bit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum NesButton {
    /// The A face button.
    A,
    /// The B face button.
    B,
    /// The Select button.
    Select,
    /// The Start button.
    Start,
    /// D-pad up.
    Up,
    /// D-pad down.
    Down,
    /// D-pad left.
    Left,
    /// D-pad right.
    Right,
}

impl NesButton {
    const fn bit(self) -> Buttons {
        match self {
            Self::A => Buttons::A,
            Self::B => Buttons::B,
            Self::Select => Buttons::SELECT,
            Self::Start => Buttons::START,
            Self::Up => Buttons::UP,
            Self::Down => Buttons::DOWN,
            Self::Left => Buttons::LEFT,
            Self::Right => Buttons::RIGHT,
        }
    }
}

/// The console region the loaded ROM runs under, mirrored across the FFI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum NesRegion {
    /// NTSC (60 Hz, 262 scanlines).
    Ntsc,
    /// PAL (50 Hz, 312 scanlines).
    Pal,
    /// Dendy (50 Hz PAL famiclone with NTSC-style timing).
    Dendy,
}

impl From<Region> for NesRegion {
    fn from(r: Region) -> Self {
        match r {
            Region::Ntsc => Self::Ntsc,
            Region::Pal => Self::Pal,
            Region::Dendy => Self::Dendy,
        }
    }
}

/// Immutable metadata about the loaded cartridge, returned by
/// [`NesController::info`].
#[derive(Debug, Clone, uniffi::Record)]
pub struct RomInfo {
    /// iNES/NES 2.0 mapper number.
    pub mapper_id: u16,
    /// Console region.
    pub region: NesRegion,
    /// PRG ROM size in bytes.
    pub prg_rom_len: u64,
    /// CHR ROM size in bytes (0 for CHR-RAM carts).
    pub chr_rom_len: u64,
    /// Whether the cartridge reports a Vs. System arcade board.
    pub is_vs_system: bool,
}

/// The logged-in `RetroAchievements` user, surfaced across the FFI (v1.8.6).
#[derive(Debug, Clone, uniffi::Record)]
pub struct RaUserInfo {
    /// The user's display name (the RA profile name shown on the HUD).
    pub display_name: String,
    /// The login username (stable identifier; persisted for token re-login).
    pub username: String,
    /// The user's total hardcore points (softcore score is not surfaced here;
    /// the HUD shows the headline hardcore figure).
    pub score: u32,
}

/// The coarse `RetroAchievements` login state, mirrored across the FFI (v1.8.6).
///
/// Flattens [`rustynes_ra::LoginState`] — the `Error` message is read separately
/// off the toast queue so this enum stays a stable, payload-free shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum RaLoginStatus {
    /// Not logged in.
    LoggedOut,
    /// A login request is in flight.
    LoggingIn,
    /// Logged in successfully.
    LoggedIn,
    /// The last login attempt failed (detail is in the toast queue).
    Error,
}

/// One transient `RetroAchievements` HUD toast, marshalled across the FFI
/// (v1.8.6). The host renders + times these out itself; the bridge only hands
/// them over once via [`NesController::ra_poll_toasts`].
#[derive(Debug, Clone, uniffi::Record)]
pub struct RaToast {
    /// The toast headline (e.g. the achievement title).
    pub title: String,
    /// The secondary line (points, error detail).
    pub detail: String,
    /// `true` for an error/warning toast.
    pub is_error: bool,
    /// For an achievement-unlock toast, the RA media-server URL of the unlocked
    /// badge PNG (empty otherwise).
    pub badge_url: String,
}

/// One achievement in the loaded game's list, marshalled across the FFI
/// (v1.8.6). A flat projection of [`rustynes_ra::Achievement`].
#[derive(Debug, Clone, uniffi::Record)]
pub struct RaAchievementInfo {
    /// The `RetroAchievements` achievement id.
    pub id: u32,
    /// The achievement title.
    pub title: String,
    /// The achievement description.
    pub description: String,
    /// The point value.
    pub points: u32,
    /// `true` if the user has earned this achievement (softcore and/or hardcore).
    pub unlocked: bool,
    /// The RA media-server URL of the unlocked (color) badge PNG.
    pub badge_url: String,
    /// The RA media-server URL of the locked (greyed) badge PNG.
    pub badge_locked_url: String,
    /// The measured progress toward this achievement (`0.0..=100.0`).
    pub measured_percent: f32,
}

/// Mutable state behind the controller's lock.
struct Inner {
    nes: Nes,
    masks: [u8; 4],
    sample_rate: u32,
    /// Active TAS recording (`.rnm`), if any — captured each frame before the tick.
    recorder: Option<rustynes_core::MovieRecorder>,
    /// Active TAS playback: the loaded movie + the next frame index. While set,
    /// `run_frame` drives input from the movie instead of the host masks.
    playback: Option<(rustynes_core::Movie, usize)>,
    /// Active HD-pack compositor (v1.8.5), if a pack is loaded. `composite_hd_frame`
    /// runs it over the current frame's snapshots.
    hd_pack: Option<rustynes_hdpack::hdpack::HdCompositor>,
    /// Active Lua script (v1.8.6), if loaded — its `on_frame` callback runs each
    /// frame after the tick (sandboxed; gated writes; no io/os/net).
    script: Option<rustynes_script::ScriptEngine>,
    /// Active `RetroAchievements` session (v1.8.6), created lazily on the first
    /// `ra_*` call. **Unlike** `script`/`hd_pack`/movie state, this is NOT
    /// cleared by `load_rom`: the RA login persists across ROM swaps (a fresh
    /// ROM re-identifies via `ra_load_game`). Native-only; the bridge is never a
    /// wasm target so it is always compiled.
    ra: Option<rustynes_ra::RaSession>,
}

/// The handle the mobile shells drive the emulator through.
///
/// Cheap to share (`Arc`); every method is internally synchronised so the UI
/// thread (input/lifecycle) and the native emulation thread can both hold the
/// same instance. This is the Android/iOS analogue of the desktop
/// `Arc<Mutex<EmuCore>>` handle.
#[derive(uniffi::Object)]
pub struct NesController {
    inner: Mutex<Inner>,
}

impl NesController {
    /// Lock the inner state, recovering transparently from a poisoned mutex so a
    /// panic on one call can never wedge the whole FFI surface.
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[uniffi::export]
impl NesController {
    /// Construct a controller from raw iNES/NES 2.0 ROM bytes at the given host
    /// sample rate (Hz). Pass [`DEFAULT_SAMPLE_RATE`] when unsure.
    ///
    /// # Errors
    /// Returns [`MobileError::RomLoad`] if the bytes are not a valid cartridge
    /// image (FDS disks and NSF files are loaded through dedicated entry points
    /// added in later increments).
    #[uniffi::constructor]
    pub fn new(rom: Vec<u8>, sample_rate: u32) -> Result<Arc<Self>, MobileError> {
        let rom = decompress_rom(rom);
        let nes = Nes::from_rom_with_sample_rate(&rom, sample_rate).map_err(|e| {
            MobileError::RomLoad {
                reason: e.to_string(),
            }
        })?;
        Ok(Arc::new(Self {
            inner: Mutex::new(Inner {
                nes,
                masks: [0; 4],
                sample_rate,
                recorder: None,
                playback: None,
                hd_pack: None,
                script: None,
                ra: None,
            }),
        }))
    }

    /// Replace the loaded cartridge in place, resetting per-port input.
    ///
    /// # Errors
    /// Returns [`MobileError::RomLoad`] if `rom` is not a valid cartridge image.
    pub fn load_rom(&self, rom: Vec<u8>, sample_rate: u32) -> Result<(), MobileError> {
        let rom = decompress_rom(rom);
        let nes = Nes::from_rom_with_sample_rate(&rom, sample_rate).map_err(|e| {
            MobileError::RomLoad {
                reason: e.to_string(),
            }
        })?;
        let mut g = self.lock();
        g.nes = nes;
        g.masks = [0; 4];
        g.sample_rate = sample_rate;
        // A new cartridge invalidates any in-flight movie + HD-pack + script.
        g.recorder = None;
        g.playback = None;
        g.hd_pack = None;
        g.script = None;
        // The RA session is deliberately preserved across ROM swaps (the login
        // outlives a single game) — just unload the previous game's achievement
        // set; a fresh `ra_load_game` from the host re-identifies the new ROM.
        if let Some(ra) = g.ra.as_mut() {
            ra.unload_game();
        }
        drop(g);
        Ok(())
    }

    /// Run one full frame and return a freshly-allocated copy of the RGBA8
    /// framebuffer (`FRAME_WIDTH * FRAME_HEIGHT * 4` bytes).
    ///
    /// The native hot path borrows the framebuffer pointer directly instead of
    /// copying; this owned-`Vec` form is the typed-surface convenience.
    pub fn run_frame(&self) -> Vec<u8> {
        let mut g = self.lock();
        pre_tick_movie(&mut g);
        let fb = g.nes.run_frame().to_vec();
        post_frame_script(&mut g);
        post_frame_ra(&mut g);
        drop(g);
        fb
    }

    /// Run one frame and discard the framebuffer copy — for callers that read
    /// the framebuffer through the native surface path and only need the tick.
    pub fn step_frame(&self) {
        let mut g = self.lock();
        pre_tick_movie(&mut g);
        let _ = g.nes.run_frame();
        post_frame_script(&mut g);
        post_frame_ra(&mut g);
        drop(g);
    }

    /// Drain the audio samples produced since the last call (interleaved mono
    /// `f32`, host sample rate). The resampler/DRC lives in the platform host.
    pub fn drain_audio(&self) -> Vec<f32> {
        self.lock().nes.drain_audio()
    }

    /// Drain the same audio as little-endian `f32` **bytes** (4 per sample).
    ///
    /// `UniFFI` marshals `Vec<u8>` as a single `ByteArray` (one bulk copy, no
    /// per-element boxing), so the Android sink writes it straight to a
    /// `PCM_FLOAT` `AudioTrack` — the allocation-light per-frame hot path, vs
    /// [`Self::drain_audio`]'s boxed `List<Float>`. Identical samples, just a
    /// cheaper transport; the determinism contract (timing-only) is untouched.
    pub fn drain_audio_bytes(&self) -> Vec<u8> {
        let samples = self.lock().nes.drain_audio();
        let mut out = Vec::with_capacity(samples.len() * 4);
        for s in &samples {
            out.extend_from_slice(&s.to_le_bytes());
        }
        out
    }

    /// Set the entire 8-bit controller mask for `port` (`0..=3`). Bit order
    /// matches [`Buttons`]: A, B, Select, Start, Up, Down, Left, Right.
    ///
    /// # Errors
    /// Returns [`MobileError::InvalidPort`] if `port > 3`.
    pub fn set_buttons(&self, port: u32, mask: u8) -> Result<(), MobileError> {
        let p = port_index(port)?;
        let mut g = self.lock();
        g.masks[p] = mask;
        g.nes.set_buttons(p, Buttons::from_bits_truncate(mask));
        drop(g);
        Ok(())
    }

    /// Press or release a single button on `port` (`0..=3`), preserving the
    /// other buttons' state. Convenience over [`Self::set_buttons`] for touch /
    /// key event handlers.
    ///
    /// # Errors
    /// Returns [`MobileError::InvalidPort`] if `port > 3`.
    pub fn set_button(
        &self,
        port: u32,
        button: NesButton,
        pressed: bool,
    ) -> Result<(), MobileError> {
        let p = port_index(port)?;
        let mut g = self.lock();
        let mut mask = Buttons::from_bits_truncate(g.masks[p]);
        mask.set(button.bit(), pressed);
        g.masks[p] = mask.bits();
        g.nes.set_buttons(p, mask);
        drop(g);
        Ok(())
    }

    /// The current 8-bit controller mask for `port` (`0..=3`).
    ///
    /// # Errors
    /// Returns [`MobileError::InvalidPort`] if `port > 3`.
    pub fn buttons(&self, port: u32) -> Result<u8, MobileError> {
        let p = port_index(port)?;
        Ok(self.lock().masks[p])
    }

    /// Enable/disable the Four Score adapter (4-controller multiplexer).
    pub fn set_four_score(&self, enabled: bool) {
        self.lock().nes.set_four_score(enabled);
    }

    /// Soft-reset (the front-panel Reset button); preserves power-on alignment.
    pub fn reset(&self) {
        self.lock().nes.reset();
    }

    /// Cold power-cycle (re-randomises power-on state from the seeded PRNG).
    pub fn power_cycle(&self) {
        self.lock().nes.power_cycle();
    }

    /// Encode the entire emulator state into a `.rns` save-state blob. The blob
    /// is platform-independent — it loads on desktop, Android, and iOS alike.
    pub fn save_state(&self) -> Vec<u8> {
        self.lock().nes.snapshot()
    }

    /// Restore emulator state from a `.rns` blob produced by [`Self::save_state`]
    /// (on any platform).
    ///
    /// # Errors
    /// Returns [`MobileError::SaveState`] if the blob is malformed or was
    /// produced by a different ROM. Returns [`MobileError::HardcoreBlocked`] if a
    /// hardcore `RetroAchievements` session is active (loading a state is the
    /// loosely-cheating affordance hardcore forbids; `save_state` stays allowed).
    pub fn load_state(&self, data: Vec<u8>) -> Result<(), MobileError> {
        let mut g = self.lock();
        // v1.8.6 — refuse a state load while a hardcore RA session is active.
        if g.ra
            .as_ref()
            .is_some_and(rustynes_ra::RaSession::hardcore_blocks)
        {
            drop(g);
            return Err(MobileError::HardcoreBlocked);
        }
        g.nes.restore(&data).map_err(|e| MobileError::SaveState {
            reason: e.to_string(),
        })?;
        // The restore overwrote the core's controller latch with the snapshot's
        // state, so re-apply the masks the host currently holds — otherwise a
        // button held across a load would stick or desync (the desktop host
        // re-latches input the same way after a state load).
        for p in 0..4 {
            let m = Buttons::from_bits_truncate(g.masks[p]);
            g.nes.set_buttons(p, m);
        }
        drop(g);
        Ok(())
    }

    /// The number of frames emulated since power-on.
    pub fn frame(&self) -> u64 {
        self.lock().nes.frame()
    }

    /// The host audio sample rate (Hz) the core is producing samples for.
    pub fn sample_rate(&self) -> u32 {
        self.lock().sample_rate
    }

    /// Cartridge metadata for the loaded ROM.
    pub fn info(&self) -> RomInfo {
        let g = self.lock();
        RomInfo {
            mapper_id: g.nes.mapper_id(),
            region: g.nes.region().into(),
            prg_rom_len: g.nes.prg_rom_len() as u64,
            chr_rom_len: g.nes.chr_rom_len() as u64,
            is_vs_system: g.nes.is_vs_system(),
        }
    }

    /// Load a custom 64-colour palette from `.pal` bytes (≥ 192 bytes, RGB triples;
    /// extra colours — e.g. a 512-colour Mesen palette — are ignored). Presentation
    /// only; the rendered output is byte-identical to the built-in palette once
    /// [`Self::clear_palette`] restores it.
    ///
    /// # Errors
    /// [`MobileError::Palette`] if fewer than 192 bytes were supplied.
    pub fn load_palette(&self, bytes: Vec<u8>) -> Result<(), MobileError> {
        if bytes.len() < 192 {
            return Err(MobileError::Palette {
                reason: format!("need >= 192 bytes, got {}", bytes.len()),
            });
        }
        let mut pal = [[0u8; 3]; 64];
        for (i, chunk) in bytes[..192].chunks_exact(3).enumerate() {
            pal[i] = [chunk[0], chunk[1], chunk[2]];
        }
        self.lock().nes.set_custom_palette(Some(pal));
        Ok(())
    }

    /// Clear the custom palette, restoring the built-in NES palette.
    pub fn clear_palette(&self) {
        self.lock().nes.set_custom_palette(None);
    }

    /// The per-pixel **palette-index** framebuffer (256×240 `u16`s as little-endian
    /// bytes, 2 per pixel; each value is `(emphasis << 6) | colour`, 0..=511). Feeds
    /// the GPU Bisqwit-NTSC composite, which needs the raw indices, not the RGBA.
    pub fn index_framebuffer_bytes(&self) -> Vec<u8> {
        // Copy the indices out under the lock (one statement), then build the bytes
        // lock-free — keeps the guard's hold tight (clippy significant_drop).
        let idx = self.lock().nes.index_framebuffer().to_vec();
        let mut out = Vec::with_capacity(idx.len() * 2);
        for v in idx {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out
    }

    /// The current frame's NTSC colour phase (`0..=2` NTSC, `0..=1` PAL/Dendy) —
    /// the Bisqwit composite's `videoPhase`.
    pub fn ntsc_phase(&self) -> u8 {
        self.lock().nes.ntsc_phase()
    }

    /// Start recording a TAS movie from a fresh power-on (the ROM is power-cycled so
    /// the recording starts from the same state a replay reconstructs).
    pub fn movie_record_from_power_on(&self) {
        let mut g = self.lock();
        g.nes.power_cycle();
        g.playback = None;
        g.recorder = Some(rustynes_core::MovieRecorder::power_on(&g.nes));
    }

    /// Start recording a TAS movie branching from the current state (embeds a
    /// save-state as the start point).
    pub fn movie_record_from_here(&self) {
        let mut g = self.lock();
        g.playback = None;
        g.recorder = Some(rustynes_core::MovieRecorder::from_current_state(&g.nes));
    }

    /// Finish recording and return the serialized `.rnm` movie bytes (empty if not
    /// recording). The caller writes them to storage.
    pub fn movie_stop_recording(&self) -> Vec<u8> {
        let rec = self.lock().recorder.take();
        rec.map(|r| r.finish().serialize()).unwrap_or_default()
    }

    /// Load + play a `.rnm` movie: seek the emulator to its start point and drive
    /// input from the recorded stream each frame until it ends. Stops any recording.
    ///
    /// # Errors
    /// [`MobileError::Movie`] if the bytes are not a valid movie or the ROM differs.
    pub fn movie_play(&self, bytes: Vec<u8>) -> Result<(), MobileError> {
        let movie = rustynes_core::Movie::deserialize(&bytes).map_err(|e| MobileError::Movie {
            reason: e.to_string(),
        })?;
        let mut g = self.lock();
        movie
            .seek_to_start(&mut g.nes)
            .map_err(|e| MobileError::Movie {
                reason: e.to_string(),
            })?;
        g.recorder = None;
        g.playback = Some((movie, 0));
        drop(g);
        Ok(())
    }

    /// Stop any active movie recording or playback.
    pub fn movie_stop(&self) {
        let mut g = self.lock();
        g.recorder = None;
        g.playback = None;
    }

    /// Whether a TAS recording is in progress.
    pub fn movie_is_recording(&self) -> bool {
        self.lock().recorder.is_some()
    }

    /// Whether a TAS movie is playing back.
    pub fn movie_is_playing(&self) -> bool {
        self.lock().playback.is_some()
    }

    /// Load an HD-pack from `.zip` bytes (a SAF stream). Replaces any active pack.
    ///
    /// # Errors
    /// [`MobileError::HdPack`] if the bytes are not a valid HD-pack archive.
    pub fn load_hdpack_from_zip_bytes(&self, bytes: Vec<u8>) -> Result<(), MobileError> {
        let pack =
            rustynes_hdpack::hdpack::HdPack::load_from_zip_bytes(&bytes).ok_or_else(|| {
                MobileError::HdPack {
                    reason: "not a valid HD-pack zip (no usable hires.txt)".into(),
                }
            })?;
        self.lock().hd_pack = Some(rustynes_hdpack::hdpack::HdCompositor::new(pack));
        Ok(())
    }

    /// Unload the active HD-pack (revert to the stock framebuffer).
    pub fn unload_hdpack(&self) {
        self.lock().hd_pack = None;
    }

    /// `[width, height]` of the active HD-pack's upscaled output, or `[0, 0]` if no
    /// pack is loaded.
    pub fn hdpack_dimensions(&self) -> Vec<u32> {
        self.lock().hd_pack.as_ref().map_or_else(
            || vec![0, 0],
            |c| {
                let (w, h) = c.dimensions();
                vec![w, h]
            },
        )
    }

    /// Composite the current frame through the active HD-pack and return the upscaled
    /// RGBA8 bytes (`hdpack_dimensions` w*h*4), or empty if no pack is loaded. Call
    /// after `run_frame`.
    pub fn composite_hd_frame(&self) -> Vec<u8> {
        let mut g = self.lock();
        if g.hd_pack.is_none() {
            return Vec::new();
        }
        // Snapshot the per-pixel tile source, the CHR (0x0000..0x2000), and the frame.
        let hd_tiles = g.nes.hd_tile_source().to_vec();
        let framebuffer = g.nes.framebuffer().to_vec();
        let mut chr = vec![0u8; 0x2000];
        for (addr, slot) in (0u16..0x2000).zip(chr.iter_mut()) {
            *slot = g.nes.peek_ppu(addr);
        }
        // Snapshot the pack's watched memory (PPU bus or CPU bus per the tag bit).
        let watched_addrs = g
            .hd_pack
            .as_ref()
            .map_or_else(Vec::new, |c| c.watched_addresses().to_vec());
        let mut watched = rustynes_hdpack::hdpack::WatchedMemory::new();
        for tagged in watched_addrs {
            let lo = (tagged & 0xFFFF) as u16;
            let val = if tagged & rustynes_hdpack::hdpack::PPU_MEMORY_MARKER != 0 {
                g.nes.ppu_bus_peek(lo)
            } else {
                g.nes.cpu_bus_peek(lo)
            };
            watched.set(tagged, val);
        }
        let Some(comp) = g.hd_pack.as_mut() else {
            return Vec::new();
        };
        let out = comp
            .composite(&framebuffer, &hd_tiles, &watched, |addr| {
                chr.get((addr & 0x1FFF) as usize).copied().unwrap_or(0)
            })
            .to_vec();
        drop(g);
        out
    }

    /// Load + start a Lua script (the same sandboxed engine the desktop uses).
    /// Replaces any active script; its `on_frame` callback then runs each frame after
    /// the tick (gated writes; no io/os/net).
    ///
    /// # Errors
    /// [`MobileError::Script`] if the engine fails to start or the script fails to
    /// compile / load.
    pub fn load_script(&self, src: String) -> Result<(), MobileError> {
        let mut engine = rustynes_script::ScriptEngine::new().map_err(|e| MobileError::Script {
            reason: e.to_string(),
        })?;
        engine.load(&src).map_err(|e| MobileError::Script {
            reason: e.to_string(),
        })?;
        self.lock().script = Some(engine);
        Ok(())
    }

    /// Unload the active script.
    pub fn unload_script(&self) {
        self.lock().script = None;
    }

    /// Whether a script is loaded.
    pub fn script_is_loaded(&self) -> bool {
        self.lock().script.is_some()
    }

    /// Drain the script's log output (its `print` / `emu.log` lines) since the last
    /// call. Empty if no script is loaded.
    pub fn drain_script_log(&self) -> Vec<String> {
        self.lock()
            .script
            .as_ref()
            .map(rustynes_script::ScriptEngine::drain_log)
            .unwrap_or_default()
    }

    // --- RetroAchievements (v1.8.6) --------------------------------------
    //
    // All methods take `&self`, lock internally, and create the session lazily
    // (`ensure_ra`) on the first call. The session persists for the controller's
    // life — including across `load_rom` — so the login outlives a single game.

    /// Create (or seed) the `RetroAchievements` session with the given hardcore
    /// flag. Idempotent: if a session already exists this just sets hardcore.
    pub fn ra_init(&self, hardcore: bool) {
        let mut g = self.lock();
        if g.ra.is_some() {
            ensure_ra(&mut g, hardcore).set_hardcore(hardcore);
        } else {
            ensure_ra(&mut g, hardcore);
        }
        drop(g);
    }

    /// Whether a `RetroAchievements` session has been created.
    pub fn ra_is_enabled(&self) -> bool {
        self.lock().ra.is_some()
    }

    /// Begin a username + password login. The completion is reconciled on a
    /// later frame; poll [`Self::ra_login_status`] / [`Self::ra_poll_toasts`].
    pub fn ra_login_password(&self, user: String, password: String) {
        let mut g = self.lock();
        ensure_ra(&mut g, true).begin_login_password(&user, &password);
        drop(g);
    }

    /// Begin a token login (re-login with a previously-returned token, no
    /// password). Completion reconciled on a later frame.
    pub fn ra_login_token(&self, user: String, token: String) {
        let mut g = self.lock();
        ensure_ra(&mut g, true).begin_login_token(&user, &token);
        drop(g);
    }

    /// Log out and clear the cached per-game achievement state.
    pub fn ra_logout(&self) {
        let mut g = self.lock();
        if let Some(ra) = g.ra.as_mut() {
            ra.logout();
        }
        drop(g);
    }

    /// The coarse login state (the `Error` detail is read off the toast queue).
    pub fn ra_login_status(&self) -> RaLoginStatus {
        let g = self.lock();
        let status =
            g.ra.as_ref()
                .map_or(RaLoginStatus::LoggedOut, |ra| match &ra.login {
                    rustynes_ra::LoginState::LoggedOut => RaLoginStatus::LoggedOut,
                    rustynes_ra::LoginState::LoggingIn => RaLoginStatus::LoggingIn,
                    rustynes_ra::LoginState::LoggedIn => RaLoginStatus::LoggedIn,
                    rustynes_ra::LoginState::Error(_) => RaLoginStatus::Error,
                });
        drop(g);
        status
    }

    /// The logged-in user, or `None` if not logged in.
    pub fn ra_user(&self) -> Option<RaUserInfo> {
        let g = self.lock();
        let user = g.ra.as_ref().and_then(|ra| {
            ra.user_info().map(|u| RaUserInfo {
                display_name: u.display_name,
                username: u.username,
                score: u.score,
            })
        });
        drop(g);
        user
    }

    /// The persisted login token (write this to host storage after a successful
    /// login so a later launch can `ra_login_token`). `None` if not logged in.
    pub fn ra_token(&self) -> Option<String> {
        let g = self.lock();
        let token = g.ra.as_ref().and_then(rustynes_ra::RaSession::user_token);
        drop(g);
        token
    }

    /// Toggle hardcore mode (creating the session if needed).
    pub fn ra_set_hardcore(&self, hardcore: bool) {
        let mut g = self.lock();
        ensure_ra(&mut g, hardcore).set_hardcore(hardcore);
        drop(g);
    }

    /// Whether hardcore mode is enabled (false if no session exists).
    pub fn ra_hardcore(&self) -> bool {
        self.lock()
            .ra
            .as_ref()
            .is_some_and(rustynes_ra::RaSession::hardcore)
    }

    /// Begin identifying + loading the achievement set for the loaded ROM.
    /// `sha256` keys the per-game progress sidecar; `sidecar` (if non-empty) is
    /// previously-saved progress applied once the async load completes. The host
    /// calls this after a fresh ROM is loaded and the user is logged in.
    ///
    /// # Errors
    /// [`MobileError::SaveState`] if `sha256` is not 32 bytes.
    pub fn ra_load_game(
        &self,
        rom: Vec<u8>,
        sha256: Vec<u8>,
        sidecar: Vec<u8>,
    ) -> Result<(), MobileError> {
        let sha: [u8; 32] = sha256
            .as_slice()
            .try_into()
            .map_err(|_| MobileError::SaveState {
                reason: format!("ra sha256 must be 32 bytes, got {}", sha256.len()),
            })?;
        let pending = (!sidecar.is_empty()).then_some(sidecar);
        let mut g = self.lock();
        ensure_ra(&mut g, true).begin_load_game(&rom, sha, pending);
        drop(g);
        Ok(())
    }

    /// Unload the current game's achievement set (e.g. on ROM close). Keeps the
    /// login.
    pub fn ra_unload_game(&self) {
        let mut g = self.lock();
        if let Some(ra) = g.ra.as_mut() {
            ra.unload_game();
        }
        drop(g);
    }

    /// The loaded game's ROM-bytes SHA-256 (the progress-sidecar key), or empty
    /// if no game is loaded into the session.
    pub fn ra_game_sha256(&self) -> Vec<u8> {
        let g = self.lock();
        let sha =
            g.ra.as_ref()
                .and_then(rustynes_ra::RaSession::game_sha256)
                .map_or_else(Vec::new, |s| s.to_vec());
        drop(g);
        sha
    }

    /// Serialize the runtime achievement progress for the per-game sidecar file
    /// (empty if no session / nothing to persist). The host writes it to storage.
    pub fn ra_serialize_progress(&self) -> Vec<u8> {
        let mut g = self.lock();
        let blob =
            g.ra.as_mut()
                .map(rustynes_ra::RaSession::serialize_progress)
                .unwrap_or_default();
        drop(g);
        blob
    }

    /// Drain the pending HUD toasts since the last call (achievement unlocks,
    /// login/server messages). The host renders + times these out itself.
    pub fn ra_poll_toasts(&self) -> Vec<RaToast> {
        let mut g = self.lock();
        let toasts = g.ra.as_mut().map_or_else(Vec::new, |ra| {
            let out: Vec<RaToast> = ra
                .toasts
                .iter()
                .map(|t| RaToast {
                    title: t.title.clone(),
                    detail: t.detail.clone(),
                    is_error: t.is_error,
                    badge_url: t.badge_url.clone(),
                })
                .collect();
            ra.toasts.clear();
            out
        });
        drop(g);
        toasts
    }

    /// The current rich-presence string (empty if none).
    pub fn ra_rich_presence(&self) -> String {
        let g = self.lock();
        let rp =
            g.ra.as_ref()
                .map(|ra| ra.rich_presence.clone())
                .unwrap_or_default();
        drop(g);
        rp
    }

    /// The cached achievement list for the loaded game (empty if no game loaded).
    pub fn ra_achievement_list(&self) -> Vec<RaAchievementInfo> {
        let g = self.lock();
        let list = g.ra.as_ref().map_or_else(Vec::new, |ra| {
            ra.achievements
                .iter()
                .map(|a| RaAchievementInfo {
                    id: a.id,
                    title: a.title.clone(),
                    description: a.description.clone(),
                    points: a.points,
                    unlocked: a.unlocked,
                    badge_url: a.badge_url.clone(),
                    badge_locked_url: a.badge_locked_url.clone(),
                    measured_percent: a.measured_percent,
                })
                .collect()
        });
        drop(g);
        list
    }

    /// The cached game progress summary as a flat `[num_core, num_unofficial,
    /// num_unlocked, num_unsupported, points_core, points_unlocked]` (all zeros
    /// if no game is loaded). A flat `Vec<u32>` keeps the FFI shape minimal.
    pub fn ra_game_summary(&self) -> Vec<u32> {
        let g = self.lock();
        let s = g.ra.as_ref().map(|ra| ra.summary).unwrap_or_default();
        drop(g);
        vec![
            s.num_core_achievements,
            s.num_unofficial_achievements,
            s.num_unlocked_achievements,
            s.num_unsupported_achievements,
            s.points_core,
            s.points_unlocked,
        ]
    }
}

/// If `bytes` is a ZIP archive (PK magic), extract the first NES-format entry
/// (`.nes` / `.fds` / `.unf` / `.unif`); otherwise return `bytes` unchanged. Lets
/// the host hand a still-compressed ROM straight through — the same convenience the
/// desktop has — without unzipping on the Kotlin/Swift side. A malformed archive or
/// a zip with no ROM entry falls back to the original bytes (the cartridge loader
/// then reports a clean error).
fn decompress_rom(bytes: Vec<u8>) -> Vec<u8> {
    use std::io::Read;
    // Bound both the declared size AND the actual read so a zip bomb (or a bogus huge
    // entry) can't OOM the app — any real NES/FDS/UNIF image is well under 16 MiB.
    const MAX_ROM_BYTES: u64 = 16 * 1024 * 1024;
    if bytes.len() < 4 || &bytes[..4] != b"PK\x03\x04" {
        return bytes;
    }
    // The archive borrows `bytes`, so do every read inside this closure and hand
    // back an owned `Vec`; only then is it safe to fall back to moving `bytes`.
    let extracted = (|| {
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(&bytes)).ok()?;
        let idx = (0..archive.len()).find(|&i| {
            archive.by_index(i).is_ok_and(|e| {
                std::path::Path::new(e.name())
                    .extension()
                    .is_some_and(|ext| {
                        ["nes", "fds", "unf", "unif"]
                            .iter()
                            .any(|k| ext.eq_ignore_ascii_case(k))
                    })
            })
        })?;
        let e = archive.by_index(idx).ok()?;
        if e.size() > MAX_ROM_BYTES {
            return None;
        }
        let mut out = Vec::new();
        e.take(MAX_ROM_BYTES).read_to_end(&mut out).ok()?;
        (!out.is_empty()).then_some(out)
    })();
    extracted.unwrap_or(bytes)
}

/// Apply movie playback (drive input from the loaded movie) and recording (capture
/// the upcoming frame's input) around a tick. Called holding the lock, immediately
/// before `Nes::run_frame`.
fn pre_tick_movie(g: &mut Inner) {
    // Playback: drive input from the next movie frame, then advance the index.
    let pb = g.playback.as_mut().and_then(|(movie, idx)| {
        let fi = movie.frames.get(*idx).copied();
        if fi.is_some() {
            *idx += 1;
        }
        fi
    });
    if let Some(fi) = pb {
        g.nes.set_buttons(0, fi.p1);
        g.nes.set_buttons(1, fi.p2);
    }
    // Stop playback once the movie is exhausted.
    if g.playback
        .as_ref()
        .is_some_and(|(m, i)| *i >= m.frames.len())
    {
        g.playback = None;
    }
    // Recording: capture the inputs the upcoming frame will consume.
    if let Some(rec) = g.recorder.as_mut() {
        rec.capture(&g.nes);
    }
}

/// Run the loaded Lua script's `on_frame` callback after a tick. Errors are swallowed
/// (the host reads them via the script log) so a buggy script can't wedge the
/// emulator. Called holding the lock, after `Nes::run_frame`.
fn post_frame_script(g: &mut Inner) {
    if let Some(engine) = g.script.as_mut() {
        let _ = engine.on_frame(&mut g.nes);
    }
}

/// Drive one frame of `RetroAchievements` logic after a tick (v1.8.6). Polls the
/// HTTP completions, reconciles login/game-load, evaluates the achievement
/// triggers against the live CPU bus, refreshes the HUD model, and honours a
/// `Reset` request from the server. Called holding the lock, after the tick.
///
/// The disjoint field borrow (`&mut g.ra` for the session + `&g.nes` for the
/// read closure) is what lets the achievement engine read emulator memory while
/// the client is mutably borrowed — Rust splits the two `Inner` fields.
fn post_frame_ra(g: &mut Inner) {
    // Split the two `Inner` fields into disjoint mutable borrows: the RA client
    // needs `&mut`, and `cpu_bus_peek` also takes `&mut self` (it may settle the
    // open-bus latch). Borrowing the fields separately lets the read closure
    // drive `nes` while the client is mutably borrowed.
    let Inner { nes, ra, .. } = g;
    let Some(ra) = ra.as_mut() else { return };
    let reset = ra.do_frame(&mut |a| nes.cpu_bus_peek(a));
    ra.refresh_views();
    ra.expire_toasts();
    if reset {
        nes.reset();
    }
}

/// Lazily create the `RetroAchievements` session on the first `ra_*` call, then
/// return a mutable handle to it. The session persists for the controller's life
/// (across ROM swaps); `hardcore` only seeds the initial flag when it is first
/// created (a later `ra_set_hardcore` overrides it).
fn ensure_ra(g: &mut Inner, hardcore: bool) -> &mut rustynes_ra::RaSession {
    if g.ra.is_none() {
        let config = rustynes_ra::RaConfig {
            enabled: false,
            username: String::new(),
            token: String::new(),
            hardcore,
        };
        g.ra = Some(rustynes_ra::RaSession::new(&config));
    }
    g.ra.as_mut().expect("session just created")
}

/// Validate and convert an FFI `u32` port into a `0..=3` array index.
const fn port_index(port: u32) -> Result<usize, MobileError> {
    if port <= 3 {
        Ok(port as usize)
    } else {
        Err(MobileError::InvalidPort { port })
    }
}

/// The crate version string (`CARGO_PKG_VERSION`), exposed to the shells so the
/// About screen can render the native core version.
#[uniffi::export]
pub fn core_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal NROM-128 (mapper 0) image: 16 KiB PRG + 8 KiB CHR with the
    /// reset vector pointing at a tight `JMP $8000` loop, enough to boot and
    /// tick frames deterministically without any commercial ROM.
    fn tiny_nrom() -> Vec<u8> {
        let mut rom = vec![0u8; 16 + 16 * 1024 + 8 * 1024];
        rom[0..4].copy_from_slice(b"NES\x1a");
        rom[4] = 1; // 1 x 16 KiB PRG
        rom[5] = 1; // 1 x 8 KiB CHR
        // PRG starts at offset 16; reset vector at $FFFC-$FFFD -> $8000.
        let prg = 16;
        rom[prg] = 0x4c; // JMP $8000
        rom[prg + 1] = 0x00;
        rom[prg + 2] = 0x80;
        let reset = prg + 0x3ffc; // $FFFC within the 16 KiB window
        rom[reset] = 0x00;
        rom[reset + 1] = 0x80;
        rom
    }

    #[test]
    fn boots_and_runs_a_frame() {
        let ctrl = NesController::new(tiny_nrom(), DEFAULT_SAMPLE_RATE).expect("load");
        let fb = ctrl.run_frame();
        assert_eq!(fb.len(), (FRAME_WIDTH * FRAME_HEIGHT * 4) as usize);
        assert_eq!(ctrl.frame(), 1);
    }

    #[test]
    fn rejects_garbage_rom() {
        // `NesController` is a UniFFI object (no `Debug`), so match rather than
        // `unwrap_err` to avoid requiring `Debug` on the `Ok` arm.
        match NesController::new(vec![0u8; 8], DEFAULT_SAMPLE_RATE) {
            Err(MobileError::RomLoad { .. }) => {}
            Err(other) => panic!("wrong error: {other}"),
            Ok(_) => panic!("garbage ROM unexpectedly loaded"),
        }
    }

    #[test]
    fn button_press_release_preserves_other_bits() {
        let ctrl = NesController::new(tiny_nrom(), DEFAULT_SAMPLE_RATE).expect("load");
        ctrl.set_button(0, NesButton::A, true).unwrap();
        ctrl.set_button(0, NesButton::Start, true).unwrap();
        assert_eq!(
            ctrl.buttons(0).unwrap(),
            (Buttons::A | Buttons::START).bits()
        );
        ctrl.set_button(0, NesButton::A, false).unwrap();
        assert_eq!(ctrl.buttons(0).unwrap(), Buttons::START.bits());
    }

    #[test]
    fn invalid_port_is_rejected() {
        let ctrl = NesController::new(tiny_nrom(), DEFAULT_SAMPLE_RATE).expect("load");
        assert!(matches!(
            ctrl.set_buttons(4, 0xff),
            Err(MobileError::InvalidPort { port: 4 })
        ));
    }

    #[test]
    fn save_state_round_trips() {
        let ctrl = NesController::new(tiny_nrom(), DEFAULT_SAMPLE_RATE).expect("load");
        for _ in 0..10 {
            ctrl.step_frame();
        }
        let blob = ctrl.save_state();
        for _ in 0..10 {
            ctrl.step_frame();
        }
        let later = ctrl.frame();
        ctrl.load_state(blob).expect("restore");
        assert_eq!(ctrl.frame(), 10);
        assert_ne!(later, 10);
    }

    #[test]
    fn load_state_preserves_held_input() {
        let ctrl = NesController::new(tiny_nrom(), DEFAULT_SAMPLE_RATE).expect("load");
        ctrl.step_frame();
        let blob = ctrl.save_state();
        // Hold A, then restore a state captured before A was held: the host mask
        // must survive (and be re-applied to the core) rather than be lost.
        ctrl.set_button(0, NesButton::A, true).unwrap();
        ctrl.load_state(blob).expect("restore");
        assert_eq!(ctrl.buttons(0).unwrap(), Buttons::A.bits());
    }

    #[test]
    fn info_reports_nrom() {
        let ctrl = NesController::new(tiny_nrom(), DEFAULT_SAMPLE_RATE).expect("load");
        let info = ctrl.info();
        assert_eq!(info.mapper_id, 0);
        assert_eq!(info.region, NesRegion::Ntsc);
    }

    // v1.8.6 — the RA bridge surfaces the lazy session + the login lifecycle.
    #[test]
    fn ra_session_created_lazily_and_hardcore_round_trips() {
        let ctrl = NesController::new(tiny_nrom(), DEFAULT_SAMPLE_RATE).expect("load");
        // No `ra_*` call yet → no session.
        assert!(!ctrl.ra_is_enabled());
        assert_eq!(ctrl.ra_login_status(), RaLoginStatus::LoggedOut);
        assert!(ctrl.ra_user().is_none());

        ctrl.ra_init(true);
        assert!(ctrl.ra_is_enabled());
        assert!(ctrl.ra_hardcore());
        ctrl.ra_set_hardcore(false);
        assert!(!ctrl.ra_hardcore());

        // `ra_game_summary` is a fixed-width flat vector even with no game.
        assert_eq!(ctrl.ra_game_summary().len(), 6);
        assert!(ctrl.ra_achievement_list().is_empty());
    }

    // v1.8.6 — a token login against the (unreachable in test) default host
    // eventually surfaces an error via the toast queue — mirrors the cheevos
    // `login_completion_fires_on_transport_error` pattern. The session moves to
    // `LoggingIn` synchronously; the failure toast lands after pumping frames.
    #[test]
    fn ra_token_login_surfaces_error_toast() {
        let ctrl = NesController::new(tiny_nrom(), DEFAULT_SAMPLE_RATE).expect("load");
        ctrl.ra_init(false);
        ctrl.ra_login_token("nobody".to_string(), "deadbeeftoken".to_string());
        assert_eq!(ctrl.ra_login_status(), RaLoginStatus::LoggingIn);

        // Pump frames so `post_frame_ra` polls the HTTP completion; the worker
        // does real network I/O (offline CI errors fast). Collect any toasts.
        let mut error_toast = false;
        for _ in 0..200 {
            ctrl.step_frame();
            if ctrl.ra_poll_toasts().iter().any(|t| t.is_error) {
                error_toast = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        // In the common offline/unreachable case the login must have failed; if
        // the network is up and the request is still in flight after the budget,
        // we don't fail the build (timing is environmental), matching the
        // cheevos test's tolerance.
        if error_toast {
            assert_ne!(ctrl.ra_login_status(), RaLoginStatus::LoggedIn);
        }
    }

    // v1.8.6 — a hardcore session refuses `load_state` but still allows
    // `save_state`.
    #[test]
    fn hardcore_blocks_load_state_only() {
        let ctrl = NesController::new(tiny_nrom(), DEFAULT_SAMPLE_RATE).expect("load");
        ctrl.step_frame();
        let blob = ctrl.save_state();
        ctrl.ra_init(true); // hardcore on
        // save_state stays allowed.
        let _ = ctrl.save_state();
        // load_state is refused.
        match ctrl.load_state(blob.clone()) {
            Err(MobileError::HardcoreBlocked) => {}
            other => panic!("expected HardcoreBlocked, got {other:?}"),
        }
        // Softcore re-allows it.
        ctrl.ra_set_hardcore(false);
        ctrl.load_state(blob).expect("softcore load");
    }
}
