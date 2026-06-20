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

/// Mutable state behind the controller's lock.
struct Inner {
    nes: Nes,
    masks: [u8; 4],
    sample_rate: u32,
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
            }),
        }))
    }

    /// Replace the loaded cartridge in place, resetting per-port input.
    ///
    /// # Errors
    /// Returns [`MobileError::RomLoad`] if `rom` is not a valid cartridge image.
    pub fn load_rom(&self, rom: Vec<u8>, sample_rate: u32) -> Result<(), MobileError> {
        let nes = Nes::from_rom_with_sample_rate(&rom, sample_rate).map_err(|e| {
            MobileError::RomLoad {
                reason: e.to_string(),
            }
        })?;
        let mut g = self.lock();
        g.nes = nes;
        g.masks = [0; 4];
        g.sample_rate = sample_rate;
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
        g.nes.run_frame().to_vec()
    }

    /// Run one frame and discard the framebuffer copy — for callers that read
    /// the framebuffer through the native surface path and only need the tick.
    pub fn step_frame(&self) {
        let mut g = self.lock();
        let _ = g.nes.run_frame();
    }

    /// Drain the audio samples produced since the last call (interleaved mono
    /// `f32`, host sample rate). The resampler/DRC lives in the platform host.
    pub fn drain_audio(&self) -> Vec<f32> {
        self.lock().nes.drain_audio()
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
    /// produced by a different ROM.
    pub fn load_state(&self, data: Vec<u8>) -> Result<(), MobileError> {
        self.lock()
            .nes
            .restore(&data)
            .map_err(|e| MobileError::SaveState {
                reason: e.to_string(),
            })
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
    fn info_reports_nrom() {
        let ctrl = NesController::new(tiny_nrom(), DEFAULT_SAMPLE_RATE).expect("load");
        let info = ctrl.info();
        assert_eq!(info.mapper_id, 0);
        assert_eq!(info.region, NesRegion::Ntsc);
    }
}
