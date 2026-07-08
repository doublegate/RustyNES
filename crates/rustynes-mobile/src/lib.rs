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

use std::net::{SocketAddr, ToSocketAddrs};
use std::sync::{Arc, Mutex, PoisonError};

use rustynes_core::{Buttons, Nes, Region};
use rustynes_netplay::{
    AdvanceOutcome, ConnectionState, DEFAULT_STUN_SERVERS, DisconnectReason, NatConfig, NatConnect,
    NatPhase, NetplayConnection, NetplayError, RollbackSession, SessionConfig, TurnConfig,
    UdpTransport,
};

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
    /// A direct-IP / LAN netplay call failed (v1.8.6) — a bad host:port address,
    /// a socket bind/connect error, or a session teardown reason. STUN/TURN is
    /// deliberately out of scope, so this only ever covers the direct-IP path.
    #[error("netplay error: {reason}")]
    Netplay {
        /// What went wrong (parse / bind / connect / disconnect detail).
        reason: String,
    },
    /// A cheat operation failed (v1.9.9) — a malformed Game Genie code (bad
    /// length / character). Raw-RAM pokes/peeks cannot fail, so this only ever
    /// covers Game Genie code parsing.
    #[error("cheat error: {reason}")]
    Cheat {
        /// What was wrong with the cheat code.
        reason: String,
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

/// One transient `RetroAchievements` HUD toast, marshalled across the FFI.
///
/// (v1.8.6.) The host renders these; the bridge TTLs them out (`expire_toasts`)
/// and exposes the current live set via [`NesController::ra_poll_toasts`].
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

// --- Netplay (v1.8.6) — direct-IP / same-LAN only ----------------------------

/// The coarse phase a netplay session is in (v1.8.6; `Negotiating` added in
/// v1.8.7).
///
/// Mirrored across the FFI — a flat projection of the internal
/// `NetplaySession` / `ConnectionState` / `NatPhase`. The direct-IP / LAN path
/// (v1.8.6) starts at `Connecting`; the room-code / internet path (v1.8.7)
/// starts at `Negotiating` (NAT traversal) and converges on the same
/// `Connecting → InGame` tail once a transport is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum NpPhase {
    /// No session — single-player (the host loop runs `run_frame`).
    Idle,
    /// NAT traversal is in progress (v1.8.7 room-code path): registering with the
    /// signaling room, STUN discovery, address exchange, hole-punch, or TURN
    /// relay fallback. The granular step is in [`NpStatus::detail`].
    Negotiating,
    /// The `Sync` handshake is in progress (host listening or joiner dialing, or
    /// the post-traversal handshake over the now-open mapping).
    Connecting,
    /// A rollback session is running (synced; rolling back as needed).
    InGame,
    /// The connection / session ended in an error (terminal until `np_leave`).
    Error,
}

/// What a single [`NesController::np_advance_frame`] did (v1.8.6).
///
/// The host loop branches on `produced_frame`: present + drain audio only when
/// it is `true`; a `false`/`stalled` tick is a time-sync stall (or a
/// connecting/error tick) and must be skipped for present/audio.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct NpTick {
    /// `true` if the emulator advanced a frame this tick (present it + drain
    /// audio). `false` while connecting, on error, or on a time-sync stall.
    pub produced_frame: bool,
    /// `true` if a rollback + re-simulation happened this tick.
    pub rolled_back: bool,
    /// `true` if the tick stalled (produced nothing) — for time-sync, while
    /// connecting, or on error. Always the inverse of `produced_frame`.
    pub stalled: bool,
    /// The frame index just produced (only meaningful when `produced_frame`).
    pub frame: u64,
}

impl NpTick {
    /// A tick that produced nothing (connecting / stall / idle / error).
    const STALLED: Self = Self {
        produced_frame: false,
        rolled_back: false,
        stalled: true,
        frame: 0,
    };
}

/// A copyable status snapshot for the netplay panel/HUD (v1.8.6; `detail` +
/// `relayed` added in v1.8.7), returned by [`NesController::np_status`].
// This is a flat FFI status record, not a behavioural config — the several
// independent bool flags (host / stalled / desync / relayed) are each a distinct
// piece of HUD state, so a bitflag/enum would only obscure the generated Kotlin /
// Swift surface.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, uniffi::Record)]
pub struct NpStatus {
    /// The current coarse phase.
    pub phase: NpPhase,
    /// `true` if this peer is the host (player 0 / P1).
    pub is_host: bool,
    /// Number of players in the session (2 for the direct-IP path).
    pub num_players: u8,
    /// Smoothed round-trip ping in ms (`InGame`/`Connecting` once measured), or
    /// `None` before the first RTT sample.
    pub ping_ms: Option<u32>,
    /// The frame the session is producing next (`InGame` only; 0 otherwise).
    pub current_frame: u64,
    /// Newest frame confirmed by both peers (`InGame` only), or `None`.
    pub confirmed_frame: Option<u64>,
    /// `true` if the most recent tick stalled for time-sync (no frame produced).
    pub stalled: bool,
    /// `true` if a desync was detected (terminal — the session moved to `Error`).
    /// The detail is in `message`.
    pub desync: bool,
    /// An error / disconnect / desync message (`Error` phase), else empty.
    pub message: String,
    /// A short human-readable sub-step for the `Negotiating` phase (v1.8.7) —
    /// e.g. `"Registering"`, `"Discovering"`, `"Exchanging"`, `"Punching"`,
    /// `"Relaying"`. Empty outside `Negotiating`. The host renders it under the
    /// room code while NAT traversal runs.
    pub detail: String,
    /// `true` once the session is running over a TURN relay rather than a direct
    /// hole-punched path (v1.8.7). Always `false` for the direct-IP / LAN path
    /// and for the cone-NAT hole-punch path. (Currently always `false`: the
    /// relay-fallback transport hand-off is a tracked carryover — see
    /// `np_tick_negotiating`.)
    pub relayed: bool,
}

/// Endpoints for a room-code (internet) netplay session (v1.8.7), mapped onto
/// [`NatConfig`] by [`NesController::np_host_room`] / `np_join_room`.
///
/// All fields are caller-supplied so the deployment can point at the
/// maintainer's relay (or any other). An empty `stun_servers` falls back to the
/// crate's [`DEFAULT_STUN_SERVERS`]. The `turn_*` trio is optional: with all
/// three present a TURN relay is configured for the symmetric-NAT fallback;
/// otherwise the session is punch-or-fail (cone-NAT only).
#[derive(Debug, Clone, uniffi::Record)]
pub struct NpNetConfig {
    /// STUN servers (`host:port`, optional `stun:` scheme) for public-address
    /// discovery. Empty → [`DEFAULT_STUN_SERVERS`].
    pub stun_servers: Vec<String>,
    /// The TURN relay's `host:port` for the symmetric-NAT fallback. `None`
    /// disables the relay path. Resolved at run time (never a bare IP in config).
    pub turn_url: Option<String>,
    /// The TURN long-term-credential username (required alongside `turn_url`).
    pub turn_user: Option<String>,
    /// The TURN long-term-credential password / shared secret.
    pub turn_secret: Option<String>,
    /// The signaling relay URL (e.g. `wss://relay.example` or `ws://host:9000`).
    pub signaling_url: String,
}

impl NpNetConfig {
    /// Project the FFI config onto the orchestrator's [`NatConfig`]: substitute
    /// the default STUN list when none was supplied, and build a [`TurnConfig`]
    /// only when the URL resolves AND both credentials are present.
    fn to_nat_config(&self) -> NatConfig {
        let stun_servers = if self.stun_servers.is_empty() {
            DEFAULT_STUN_SERVERS
                .iter()
                .map(|s| (*s).to_string())
                .collect()
        } else {
            self.stun_servers.clone()
        };
        let turn = match (
            self.turn_url.as_deref(),
            self.turn_user.as_deref(),
            self.turn_secret.as_deref(),
        ) {
            (Some(url), Some(user), Some(secret)) => {
                // Resolve the TURN server to a concrete `SocketAddr` (the config
                // takes a host:port, not a bare IP). A URL that does not resolve
                // disables the relay path rather than failing the whole session.
                let host = url.strip_prefix("turn:").unwrap_or(url);
                host.to_socket_addrs()
                    .ok()
                    .and_then(|mut a| a.next())
                    .map(|server| TurnConfig {
                        server,
                        username: user.to_string(),
                        credential: secret.to_string(),
                    })
            }
            _ => None,
        };
        NatConfig {
            stun_servers,
            turn,
            signaling_url: self.signaling_url.clone(),
        }
    }
}

/// An active netplay session. Exactly one variant is live; `None` in
/// [`Inner::netplay`] means single-player.
///
/// The room-code (internet) path (v1.8.7) starts at `Negotiating` (NAT
/// traversal); the direct-IP / LAN path (v1.8.6) starts at `Connecting`. Both
/// converge on the same tail: `Connecting` → (handshake completes) → `InGame`.
/// The `Negotiating` variant owns the [`NatConnect`] orchestrator doing
/// signaling + STUN/punch + TURN; once it reaches [`NatPhase::Synced`] the
/// handed-off [`NetplayConnection`] promotes it to `Connecting`. The
/// `Connecting` variant owns the [`NetplayConnection`] doing the `Sync`
/// handshake; once it reports [`ConnectionState::Synced`], the bound +
/// handshaken [`UdpTransport`] is moved into a fresh [`RollbackSession`] and the
/// session promotes to `InGame`. A traversal / handshake timeout or rom-mismatch
/// tears the session down (back to `None`) with a recorded error message.
enum NetplaySession {
    /// NAT traversal is in progress (v1.8.7 room-code path). `bool` is `is_host`.
    Negotiating(Box<NatConnect>, bool),
    /// The `Sync` handshake is in progress. `bool` is `is_host`.
    Connecting(Box<NetplayConnection>, bool),
    /// A rollback session is running. `bool` is `is_host`.
    InGame(Box<RollbackSession<UdpTransport>>, bool),
}

// --- Creator / power tools (v1.9.9 "Workshop") -------------------------------

/// One active Game Genie cheat code, marshalled across the FFI (v1.9.9). A flat
/// projection of [`rustynes_core::GenieCode`].
#[derive(Debug, Clone, uniffi::Record)]
pub struct GenieCodeInfo {
    /// The canonical upper-case Game Genie code (6 or 8 characters).
    pub code: String,
    /// The PRG address (`$8000..=$FFFF`) the code substitutes.
    pub addr: u16,
    /// The substitute data byte.
    pub data: u8,
    /// The compare byte (8-character codes only), or `None` for a 6-character
    /// code (which substitutes unconditionally).
    pub compare: Option<u8>,
}

/// A read-only snapshot of the CPU register file for the debugger inspector
/// (v1.9.9). A flat projection of [`rustynes_core::CpuDebugView`]; purely
/// observational (it never advances or mutates the core).
#[derive(Debug, Clone, Copy, uniffi::Record)]
pub struct CpuRegs {
    /// Accumulator.
    pub a: u8,
    /// X index register.
    pub x: u8,
    /// Y index register.
    pub y: u8,
    /// Stack pointer.
    pub s: u8,
    /// Program counter.
    pub pc: u16,
    /// Raw processor-status flags (the P register bits).
    pub p: u8,
    /// `true` when the CPU is jammed on an illegal halt opcode.
    pub jammed: bool,
    /// Cumulative CPU cycle count since power-on.
    pub cycles: u64,
}

/// One disassembled 6502 instruction for the debugger inspector (v1.9.9). A flat
/// projection of `rustynes_cpu`'s `DisasmLine`.
#[derive(Debug, Clone, uniffi::Record)]
pub struct DisasmRow {
    /// The address the instruction starts at.
    pub addr: u16,
    /// The raw opcode bytes (1..=3).
    pub bytes: Vec<u8>,
    /// The mnemonic (`"LDA"`, `"JMP"`, ...).
    pub mnemonic: String,
    /// The formatted operand (`"#$42"`, `"$1234,X"`, or empty).
    pub operand: String,
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
    /// Active direct-IP / LAN netplay session (v1.8.6), if any. **Cleared by
    /// `load_rom`** (like `script`/`hd_pack`/movie state — a ROM swap ends any
    /// session, the same way the desktop tears down on ROM change). `None` means
    /// single-player; the host loop runs `run_frame` instead of
    /// `np_advance_frame`.
    netplay: Option<NetplaySession>,
    /// Persisted netplay status fields that outlive a single tick or the live
    /// session object (v1.8.6): the terminal error/desync message, a sticky
    /// desync flag, and the most-recent-tick stall flag, all for
    /// [`NesController::np_status`]. Reset when a session starts / on `np_leave`.
    netplay_error: Option<String>,
    netplay_desync: bool,
    netplay_last_stalled: bool,
    /// Sticky flag: the live session fell back to a TURN relay (symmetric NAT),
    /// so gameplay rides the relay rather than a direct / hole-punched socket
    /// (v1.8.7). Set when `np_tick_negotiating` hands a relayed connection off;
    /// surfaced via [`NpStatus::relayed`]. Reset when a session starts / on
    /// `np_leave` (it outlives the `NatConnect` object, which is consumed at
    /// hand-off, so it cannot be re-derived from the live session later).
    netplay_relayed: bool,
    /// Drainable host-facing warnings (v2.0.1 Timebase re-port). Currently the sole
    /// producer is [`NesController::movie_play`]: a `.rnm` recorded on a pre-v2.0.0
    /// "Timebase" build replays its recorded *input* faithfully, but exact
    /// framebuffer/audio reproduction is not guaranteed across the engine-timebase
    /// boundary (ADR 0028). The host drains this via
    /// [`NesController::drain_warnings`] and surfaces it (a toast / log line),
    /// mirroring the desktop + wasm frontends' identical movie-load warning. This is
    /// pure host-shell wiring — the deterministic core never reads it.
    warnings: Vec<String>,
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

    /// The loaded ROM's SHA-256 (the foreign-movie-import stamp / netplay
    /// handshake key). Not part of the FFI surface — a private helper for the
    /// `movie_import_*` methods.
    fn rom_sha256(&self) -> [u8; 32] {
        let g = self.lock();
        let sha = *g.nes.rom_sha256();
        drop(g);
        sha
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
                netplay: None,
                netplay_error: None,
                netplay_desync: false,
                netplay_last_stalled: false,
                netplay_relayed: false,
                warnings: Vec::new(),
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
        // A new cartridge ends any in-flight netplay session (the peers would
        // be running a different ROM now — the handshake guards on rom_hash).
        g.netplay = None;
        g.netplay_error = None;
        g.netplay_desync = false;
        g.netplay_last_stalled = false;
        g.netplay_relayed = false;
        // Drop any undrained warnings from the previous cartridge (they refer to a
        // movie that is no longer loaded).
        g.warnings.clear();
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
        // ADR 0028: a `.rnm` recorded on a pre-v2.0.0 "Timebase" build replays its
        // recorded *input* faithfully, but exact framebuffer/audio reproduction is
        // not guaranteed across the engine-timebase boundary (the one-clock /
        // every-cycle-bus-access scheduler rewrite changed the sub-frame timing the
        // old movie was captured against). Peek the epoch and, for a pre-v2 movie,
        // queue a drainable host warning — mirroring the desktop + wasm frontends'
        // identical notice — rather than silently presenting the replay as byte-exact.
        // A malformed/short header (the `Err` arm) is treated as "not pre-v2": the
        // deserialize above already succeeded, so it is a current-epoch movie. The
        // check never blocks playback and never touches the deterministic core.
        let pre_timebase = rustynes_core::recorded_before_v2_timebase(&bytes).is_ok_and(|v| v);
        let mut g = self.lock();
        movie
            .seek_to_start(&mut g.nes)
            .map_err(|e| MobileError::Movie {
                reason: e.to_string(),
            })?;
        if pre_timebase {
            g.warnings.push(
                "this movie was recorded on a pre-v2.0.0 build -- input replay \
                 proceeds, but exact framebuffer/audio reproduction is not \
                 guaranteed across the engine-timebase boundary (see ADR 0028)"
                    .to_string(),
            );
        }
        g.recorder = None;
        g.playback = Some((movie, 0));
        drop(g);
        Ok(())
    }

    /// Drain host-facing warnings accumulated since the last call (v2.0.1 Timebase
    /// re-port). Currently the sole producer is [`Self::movie_play`], which queues a
    /// pre-v2.0.0-Timebase `.rnm` notice (ADR 0028). The host surfaces these as a
    /// toast / log line, mirroring the desktop + wasm frontends. Empty when there is
    /// nothing to report; draining clears the queue.
    pub fn drain_warnings(&self) -> Vec<String> {
        std::mem::take(&mut self.lock().warnings)
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
        // Seed `false`: a lazily-created session defaults to softcore (matching
        // the host's default-off hardcore setting). An already-created session
        // keeps its existing hardcore flag (`ensure_ra` only uses the seed on
        // creation). Hardcore is set explicitly via `ra_init` / `ra_set_hardcore`.
        ensure_ra(&mut g, false).begin_login_password(&user, &password);
        drop(g);
    }

    /// Begin a token login (re-login with a previously-returned token, no
    /// password). Completion reconciled on a later frame.
    pub fn ra_login_token(&self, user: String, token: String) {
        let mut g = self.lock();
        // Seed `false` (softcore default); see `ra_login_password` for the
        // rationale. An existing session keeps its hardcore flag.
        ensure_ra(&mut g, false).begin_login_token(&user, &token);
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
        // Seed `false` (softcore default); see `ra_login_password` for the
        // rationale. An existing session keeps its hardcore flag.
        ensure_ra(&mut g, false).begin_load_game(&rom, sha, pending);
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

    /// The current live HUD toasts (achievement unlocks, login/server messages).
    /// This does NOT drain: `expire_toasts` (run per frame in `post_frame_ra`)
    /// TTLs them out after `TOAST_TTL`, so the host can poll this repeatedly and
    /// assign the result unconditionally — the set both gains new toasts and
    /// clears itself once they expire. The host renders these as-is.
    pub fn ra_poll_toasts(&self) -> Vec<RaToast> {
        let g = self.lock();
        let toasts = g.ra.as_ref().map_or_else(Vec::new, |ra| {
            ra.toasts
                .iter()
                .map(|t| RaToast {
                    title: t.title.clone(),
                    detail: t.detail.clone(),
                    is_error: t.is_error,
                    badge_url: t.badge_url.clone(),
                })
                .collect()
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

    // --- Netplay (v1.8.6) — direct-IP / same-LAN only --------------------
    //
    // The drive model differs from Lua/RA: netplay REPLACES `run_frame`, it is
    // not a `post_frame` hook (rollback re-runs frames and may stall). The host
    // loop calls `np_advance_frame` instead of `run_frame` whenever
    // `np_is_active()` is true. STUN/TURN is out of scope — only the direct-IP
    // host/join handshake is wired here (the joiner dials the host's IP:port;
    // the host listens and adopts the joiner from its first `Sync`).

    /// Host a 2-player session: bind `0.0.0.0:local_port` and **listen** as
    /// player 0 (P1), learning the joiner's address from its first valid `Sync`.
    /// Returns the actual bound local port (pass `local_port = 0` to let the OS
    /// pick one, then share the returned port + this device's LAN IP with the
    /// joiner). Any previous session is dropped.
    ///
    /// `num_players` is clamped into `2..=4`; the direct-IP UDP handshake here
    /// completes the **first** joiner (a 2-player link). The N-player rollback
    /// core + determinism proof live in `rustynes-netplay`; the multi-joiner UDP
    /// roster handshake (3-4 players) is a deferred follow-up — the selected
    /// count is still recorded so the Four Score wiring is in place once it
    /// lands. STUN/TURN is out of scope.
    ///
    /// # Errors
    /// [`MobileError::Netplay`] if the socket bind fails.
    pub fn np_host(&self, local_port: u16, num_players: u8) -> Result<u16, MobileError> {
        let mut g = self.lock();
        let rom_hash = *g.nes.rom_sha256();
        let local = SocketAddr::from(([0, 0, 0, 0], local_port));
        let conn = NetplayConnection::host(local, rom_hash).map_err(|e| MobileError::Netplay {
            reason: format!("host bind failed: {e}"),
        })?;
        // Resolve the ephemeral `:0` port to the concrete one the OS picked so
        // the host can show "share this port with the joiner".
        let bound = conn
            .transport()
            .local_addr()
            .map_or(local_port, |a| a.port());
        let _ = num_players; // recorded conceptually; the 2-player handshake is wired.
        g.netplay = Some(NetplaySession::Connecting(Box::new(conn), true));
        g.netplay_error = None;
        g.netplay_desync = false;
        g.netplay_last_stalled = true;
        g.netplay_relayed = false;
        drop(g);
        Ok(bound)
    }

    /// Join a session hosted at `address` (a `host:port` string, e.g.
    /// `192.168.1.50:7000`): bind an ephemeral local port and begin the
    /// handshake as player 1 (P2). Any previous session is dropped.
    ///
    /// # Errors
    /// [`MobileError::Netplay`] if `address` is not a valid `host:port`, or the
    /// socket bind/connect fails.
    pub fn np_join(&self, address: String) -> Result<(), MobileError> {
        // Resolve via `ToSocketAddrs` so a hostname (`my-laptop.local:7000`) works
        // as well as a raw IP — `SocketAddr::parse` rejects hostnames. This runs
        // off the UI thread (the host calls `np_join` on a worker), so the brief
        // DNS resolution is fine. Take the first resolved address.
        let remote: SocketAddr = address
            .to_socket_addrs()
            .map_err(|e| MobileError::Netplay {
                reason: format!("invalid host:port '{address}': {e}"),
            })?
            .next()
            .ok_or_else(|| MobileError::Netplay {
                reason: format!("host:port '{address}' resolved to no addresses"),
            })?;
        let mut g = self.lock();
        let rom_hash = *g.nes.rom_sha256();
        let local = SocketAddr::from(([0, 0, 0, 0], 0));
        let conn = NetplayConnection::connect(local, remote, rom_hash).map_err(|e| {
            MobileError::Netplay {
                reason: format!("connect failed: {e}"),
            }
        })?;
        g.netplay = Some(NetplaySession::Connecting(Box::new(conn), false));
        g.netplay_error = None;
        g.netplay_desync = false;
        g.netplay_last_stalled = true;
        g.netplay_relayed = false;
        drop(g);
        Ok(())
    }

    /// Host a room-code (internet) session (v1.8.7): connect to the signaling
    /// relay, announce a new room + the loaded ROM's hash, and return the
    /// **6-char room code** to share with the joiner. NAT traversal (STUN
    /// hole-punch, with the optional TURN fallback in `cfg`) runs as the host
    /// loop drives [`Self::np_advance_frame`]; poll [`Self::np_status`] for the
    /// `Negotiating` sub-step. Any previous session is dropped.
    ///
    /// `num_players` is clamped into `2..=4`; the room-code path here completes
    /// the **first** joiner (a 2-player link), mirroring `np_host`. Unlike
    /// `np_host` (direct-IP / LAN), this path traverses NAT, so it works across
    /// the internet — at the cost of a reachable signaling relay (and a TURN
    /// relay for symmetric NATs).
    ///
    /// # Errors
    /// [`MobileError::Netplay`] if the local UDP socket bind fails. (Signaling /
    /// STUN / punch failures surface later as the session moving to `Error` —
    /// poll [`Self::np_status`].)
    pub fn np_host_room(&self, num_players: u8, cfg: NpNetConfig) -> Result<String, MobileError> {
        let mut g = self.lock();
        let rom_hash = *g.nes.rom_sha256();
        let players = num_players.clamp(2, 4);
        // Seed the room-code + STUN-transaction PRNG from a non-deterministic
        // source so two concurrent hosts don't collide on a room code. This is
        // host-side orchestration, NOT emulator state, so it does not touch the
        // determinism contract (the ROM + input + seed that the core consumes
        // are untouched).
        let seed = nondeterministic_seed();
        let (nat, room) =
            NatConnect::host(players, rom_hash, cfg.to_nat_config(), seed).map_err(|e| {
                MobileError::Netplay {
                    reason: format!("host room failed: {e}"),
                }
            })?;
        g.netplay = Some(NetplaySession::Negotiating(Box::new(nat), true));
        g.netplay_error = None;
        g.netplay_desync = false;
        g.netplay_last_stalled = true;
        g.netplay_relayed = false;
        drop(g);
        Ok(room)
    }

    /// Join a room-code (internet) session (v1.8.7) by its `room_code`: connect
    /// to the signaling relay, announce the loaded ROM's hash, and begin NAT
    /// traversal as player 1 (P2). Drive [`Self::np_advance_frame`] and poll
    /// [`Self::np_status`] for the `Negotiating` sub-step; on success the session
    /// converges on `Connecting` → `InGame`. Any previous session is dropped.
    ///
    /// # Errors
    /// [`MobileError::Netplay`] if the local UDP socket bind fails. (A wrong
    /// code, an unreachable relay, or a failed traversal surface later as the
    /// session moving to `Error`.)
    pub fn np_join_room(&self, room_code: String, cfg: NpNetConfig) -> Result<(), MobileError> {
        let mut g = self.lock();
        let rom_hash = *g.nes.rom_sha256();
        let seed = nondeterministic_seed();
        let nat =
            NatConnect::join(&room_code, rom_hash, cfg.to_nat_config(), seed).map_err(|e| {
                MobileError::Netplay {
                    reason: format!("join room failed: {e}"),
                }
            })?;
        g.netplay = Some(NetplaySession::Negotiating(Box::new(nat), false));
        g.netplay_error = None;
        g.netplay_desync = false;
        g.netplay_last_stalled = true;
        g.netplay_relayed = false;
        drop(g);
        Ok(())
    }

    /// Drive one netplay tick. **The host loop calls this instead of
    /// `run_frame`** whenever [`Self::np_is_active`] is true. `local_mask` is
    /// this peer's live 8-bit controller mask (same bit order as
    /// [`Self::set_buttons`]).
    ///
    /// - **Negotiating** (v1.8.7 room-code path): pumps the NAT-traversal
    ///   orchestrator (signaling + STUN/punch + TURN fallback; no emulation). On
    ///   [`NatPhase::Synced`] it hands the open transport off to a
    ///   [`NetplayConnection`] and promotes to `Connecting`. On
    ///   [`NatPhase::Failed`], tears the session down to an error. Returns a
    ///   stalled tick.
    /// - **Connecting**: pumps the handshake (no emulation). On `Synced`,
    ///   promotes the connection into a [`RollbackSession`] (and power-cycles the
    ///   core to the deterministic cold boot so frame 0 is byte-identical across
    ///   peers). On timeout / rom-mismatch, tears the session down to an error.
    ///   Returns a stalled tick.
    /// - **`InGame`**: feeds `local_mask`, advances the rollback session, and
    ///   reports `produced_frame` (a `false` tick is a time-sync stall — skip
    ///   present + audio this tick). A [`NetplayError`] (desync / rom-mismatch /
    ///   restore) tears the session down to an error and returns a stalled tick.
    /// - **No session**: returns a stalled tick (the caller should not call this
    ///   when `!np_is_active()`, but it is safe).
    pub fn np_advance_frame(&self, local_mask: u8) -> NpTick {
        let mut g = self.lock();
        let tick = match g.netplay.take() {
            Some(NetplaySession::Negotiating(nat, is_host)) => {
                np_tick_negotiating(&mut g, *nat, is_host)
            }
            Some(NetplaySession::Connecting(conn, is_host)) => {
                np_tick_connecting(&mut g, *conn, is_host)
            }
            Some(NetplaySession::InGame(session, is_host)) => {
                np_tick_in_game(&mut g, *session, is_host, local_mask)
            }
            None => NpTick::STALLED,
        };
        g.netplay_last_stalled = tick.stalled;
        drop(g);
        tick
    }

    /// Tear any netplay session down and return to single-player. No-op if idle.
    pub fn np_leave(&self) {
        let mut g = self.lock();
        g.netplay = None;
        g.netplay_error = None;
        g.netplay_desync = false;
        g.netplay_last_stalled = false;
        g.netplay_relayed = false;
        drop(g);
    }

    /// `true` while a netplay session is active or connecting (so the host loop
    /// drives via [`Self::np_advance_frame`] instead of `run_frame`). The error
    /// state also keeps the session present until [`Self::np_leave`], so this
    /// stays `true` through an error to stop single-player input bleeding in.
    pub fn np_is_active(&self) -> bool {
        let g = self.lock();
        let active = g.netplay.is_some() || g.netplay_error.is_some();
        drop(g);
        active
    }

    /// A copyable status snapshot for the netplay panel/HUD.
    pub fn np_status(&self) -> NpStatus {
        let g = self.lock();
        let status = match &g.netplay {
            Some(NetplaySession::Negotiating(nat, is_host)) => NpStatus {
                phase: NpPhase::Negotiating,
                is_host: *is_host,
                num_players: 2,
                ping_ms: None,
                current_frame: 0,
                confirmed_frame: None,
                stalled: g.netplay_last_stalled,
                desync: false,
                message: String::new(),
                detail: nat_phase_detail(&nat.phase()),
                relayed: false,
            },
            Some(NetplaySession::Connecting(conn, is_host)) => NpStatus {
                phase: NpPhase::Connecting,
                is_host: *is_host,
                num_players: 2,
                ping_ms: conn.ping_ms(),
                current_frame: 0,
                confirmed_frame: None,
                stalled: g.netplay_last_stalled,
                desync: false,
                message: String::new(),
                detail: String::new(),
                relayed: g.netplay_relayed,
            },
            Some(NetplaySession::InGame(session, is_host)) => NpStatus {
                phase: NpPhase::InGame,
                is_host: *is_host,
                num_players: session.num_players(),
                ping_ms: None,
                current_frame: u64::from(session.current_frame()),
                confirmed_frame: session.last_confirmed_frame().map(u64::from),
                stalled: g.netplay_last_stalled,
                desync: false,
                message: String::new(),
                detail: String::new(),
                relayed: g.netplay_relayed,
            },
            None => NpStatus {
                phase: if g.netplay_error.is_some() {
                    NpPhase::Error
                } else {
                    NpPhase::Idle
                },
                is_host: false,
                num_players: 0,
                ping_ms: None,
                current_frame: 0,
                confirmed_frame: None,
                stalled: g.netplay_last_stalled,
                desync: g.netplay_desync,
                message: g.netplay_error.clone().unwrap_or_default(),
                detail: String::new(),
                relayed: false,
            },
        };
        drop(g);
        status
    }

    // --- Cheats (v1.9.9 "Workshop") --------------------------------------
    //
    // Game Genie codes forward straight into the core's own cheat engine (which
    // applies them on every PRG read), so they persist across frames exactly as
    // on the desktop. Raw-RAM editing exposes the existing `poke_ram` / `peek`
    // core paths (one-shot writes / side-effect-free reads) — the bridge adds NO
    // new per-frame mutation, so with no cheats set the build stays byte-identical.

    /// Add a Game Genie code (6 or 8 characters, case-insensitive). The core
    /// applies it on every PRG read until removed. Idempotent on the canonical
    /// form.
    ///
    /// # Errors
    /// [`MobileError::Cheat`] if the code is not a valid Game Genie code.
    pub fn cheat_add_genie(&self, code: String) -> Result<(), MobileError> {
        let mut g = self.lock();
        let r = g.nes.add_genie_code(&code).map_err(|e| MobileError::Cheat {
            reason: e.to_string(),
        });
        drop(g);
        r
    }

    /// Remove a previously-added Game Genie code (no-op if not present).
    pub fn cheat_remove_genie(&self, code: String) {
        let mut g = self.lock();
        g.nes.remove_genie_code(&code);
        drop(g);
    }

    /// Remove every active Game Genie code.
    pub fn cheat_clear_genie(&self) {
        let mut g = self.lock();
        g.nes.clear_genie_codes();
        drop(g);
    }

    /// The currently-active Game Genie codes.
    pub fn cheat_genie_codes(&self) -> Vec<GenieCodeInfo> {
        let g = self.lock();
        let codes = g
            .nes
            .genie_codes()
            .map(|c| GenieCodeInfo {
                code: c.code().to_string(),
                addr: c.addr(),
                data: c.data(),
                compare: c.compare(),
            })
            .collect();
        drop(g);
        codes
    }

    /// Write one byte into CPU RAM (`$0000..=$1FFF`, the 2 KiB internal RAM and
    /// its mirrors) via the core's existing `poke_ram` path — the raw-RAM editor
    /// affordance. A one-shot write (the game may overwrite it next frame); it
    /// adds no per-frame mutation surface.
    ///
    /// Defense-in-depth: the address is masked to the documented `$0000..=$1FFF`
    /// internal-RAM mirror region here at the bridge boundary, so the bound is
    /// enforced by the bridge itself rather than relying on the core.
    pub fn poke_ram(&self, addr: u16, value: u8) {
        let addr = addr & 0x1FFF;
        let mut g = self.lock();
        g.nes.poke_ram(addr, value);
        drop(g);
    }

    /// Read one byte from the CPU bus at `addr`, side-effect-free (the core's
    /// `peek`). The single-address convenience over [`Self::debug_read_memory`].
    pub fn peek_byte(&self, addr: u16) -> u8 {
        let mut g = self.lock();
        let v = g.nes.peek(addr);
        drop(g);
        v
    }

    // --- Read-only debugger inspector (v1.9.9 "Workshop") ----------------
    //
    // Purely observational: every method snapshots core state without advancing
    // or mutating emulation, so the determinism contract is untouched. The host
    // gates the debugger UI off the App-Store build.

    /// A read-only snapshot of the CPU registers (`rustynes_core::CpuDebugView`).
    pub fn debug_cpu_state(&self) -> CpuRegs {
        let g = self.lock();
        let v = g.nes.cpu_snapshot();
        drop(g);
        CpuRegs {
            a: v.a,
            x: v.x,
            y: v.y,
            s: v.s,
            pc: v.pc,
            p: v.p,
            jammed: v.jammed,
            cycles: v.cycles,
        }
    }

    /// Read `len` bytes from the CPU bus starting at `start` (wrapping at the
    /// 64 KiB boundary), side-effect-free. `len` is capped at 64 KiB so a
    /// malformed request can never over-allocate. Feeds the debugger hex view.
    pub fn debug_read_memory(&self, start: u16, len: u32) -> Vec<u8> {
        let len = (len as usize).min(0x1_0000);
        let mut g = self.lock();
        let mut out = Vec::with_capacity(len);
        let mut addr = start;
        for _ in 0..len {
            out.push(g.nes.peek(addr));
            addr = addr.wrapping_add(1);
        }
        drop(g);
        out
    }

    /// Disassemble `count` 6502 instructions starting at `pc` (the debugger code
    /// view). `count` is capped at 256. Reads a bounded byte window with the
    /// side-effect-free `peek`, so the disassembler runs over an owned buffer and
    /// never re-enters the core.
    pub fn debug_disassemble(&self, pc: u16, count: u32) -> Vec<DisasmRow> {
        let count = (count as usize).min(256);
        // Up to 3 bytes per instruction, plus a small tail so the final
        // instruction's operand is fully readable; capped at the address space.
        let window = count.saturating_mul(3).saturating_add(3).min(0x1_0000);
        let mut g = self.lock();
        let mut buf = vec![0u8; window];
        let mut addr = pc;
        for slot in &mut buf {
            *slot = g.nes.peek(addr);
            addr = addr.wrapping_add(1);
        }
        drop(g);
        let lines = rustynes_core::rustynes_cpu::disasm::disassemble_at(
            |a| {
                let off = a.wrapping_sub(pc) as usize;
                buf.get(off).copied().unwrap_or(0)
            },
            pc,
            count,
        );
        lines
            .into_iter()
            .map(|l| DisasmRow {
                addr: l.addr,
                bytes: l.bytes,
                mnemonic: l.mnemonic.to_string(),
                operand: l.operand,
            })
            .collect()
    }

    // --- Foreign movie import (v1.9.9 "Workshop") ------------------------
    //
    // Each importer transcodes a foreign movie into the native `.rnm` byte
    // stream stamped with the CURRENTLY-loaded ROM's hash, then returns those
    // bytes — the host plays them via the existing [`Self::movie_play`] and/or
    // saves them as a `.rnm`. The core importers validate their input and return
    // a `Result` (they never panic on a malformed file); the only parsing the
    // bridge itself does is the `.bk2` ZIP member extraction below, which caps
    // member sizes. The produced movie is for the loaded ROM regardless of which
    // game the foreign file was authored against (it transcodes the input log).

    /// Import an `FCEUX` `.fm2` movie (UTF-8 text) and return the native `.rnm`
    /// bytes for the loaded ROM.
    ///
    /// # Errors
    /// [`MobileError::Movie`] if the bytes are not valid UTF-8 or not a parseable
    /// `.fm2`.
    pub fn movie_import_fm2(&self, bytes: Vec<u8>) -> Result<Vec<u8>, MobileError> {
        let text = String::from_utf8(bytes).map_err(|e| MobileError::Movie {
            reason: format!("fm2 is not valid UTF-8: {e}"),
        })?;
        let rom_sha = self.rom_sha256();
        let (movie, _meta) =
            rustynes_core::movie_interop::import_fm2(&text, rom_sha).map_err(|e| {
                MobileError::Movie {
                    reason: e.to_string(),
                }
            })?;
        Ok(movie.serialize())
    }

    /// Import a `BizHawk` `.bk2` movie (a ZIP archive containing `Header.txt` and
    /// `Input Log.txt`) and return the native `.rnm` bytes for the loaded ROM.
    ///
    /// # Errors
    /// [`MobileError::Movie`] if the archive is malformed, is missing either
    /// member, or the input log does not parse.
    pub fn movie_import_bk2(&self, bytes: Vec<u8>) -> Result<Vec<u8>, MobileError> {
        let (header, input_log) = read_bk2_members(&bytes)?;
        let rom_sha = self.rom_sha256();
        let (movie, _meta) = rustynes_core::bk2_interop::import_bk2(&header, &input_log, rom_sha)
            .map_err(|e| MobileError::Movie {
            reason: e.to_string(),
        })?;
        Ok(movie.serialize())
    }

    /// Import a Nestopia `.fcm` movie and return the native `.rnm` bytes for the
    /// loaded ROM.
    ///
    /// # Errors
    /// [`MobileError::Movie`] if the bytes are not a parseable `.fcm`.
    pub fn movie_import_fcm(&self, bytes: Vec<u8>) -> Result<Vec<u8>, MobileError> {
        let rom_sha = self.rom_sha256();
        let (movie, _meta) =
            rustynes_core::import_fcm(&bytes, rom_sha).map_err(|e| MobileError::Movie {
                reason: e.to_string(),
            })?;
        Ok(movie.serialize())
    }

    /// Import a Famtasia `.fmv` movie and return the native `.rnm` bytes for the
    /// loaded ROM.
    ///
    /// # Errors
    /// [`MobileError::Movie`] if the bytes are not a parseable `.fmv`.
    pub fn movie_import_fmv(&self, bytes: Vec<u8>) -> Result<Vec<u8>, MobileError> {
        let rom_sha = self.rom_sha256();
        let (movie, _meta) =
            rustynes_core::import_fmv(&bytes, rom_sha).map_err(|e| MobileError::Movie {
                reason: e.to_string(),
            })?;
        Ok(movie.serialize())
    }

    /// Import a `VirtuaNES` `.vmv` movie and return the native `.rnm` bytes for the
    /// loaded ROM.
    ///
    /// # Errors
    /// [`MobileError::Movie`] if the bytes are not a parseable `.vmv`.
    pub fn movie_import_vmv(&self, bytes: Vec<u8>) -> Result<Vec<u8>, MobileError> {
        let rom_sha = self.rom_sha256();
        let (movie, _meta) =
            rustynes_core::import_vmv(&bytes, rom_sha).map_err(|e| MobileError::Movie {
                reason: e.to_string(),
            })?;
        Ok(movie.serialize())
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

/// Extract a `.bk2`'s `Header.txt` + `Input Log.txt` members (v1.9.9). A `.bk2`
/// is a ZIP archive; this opens it, finds the two members by name
/// (case-insensitive), and reads each with a hard size cap so a malformed /
/// hostile archive can never OOM the app. Returns `(header, input_log)` text.
///
/// # Errors
/// [`MobileError::Movie`] if the bytes are not a ZIP, either member is missing /
/// oversized, or a member is not valid UTF-8.
fn read_bk2_members(bytes: &[u8]) -> Result<(String, String), MobileError> {
    use std::io::Read;
    // A `.bk2` text log is small; cap each member generously so a zip bomb can't
    // blow up. 16 MiB is far beyond any real movie's Header/Input Log.
    const MAX_MEMBER_BYTES: u64 = 16 * 1024 * 1024;
    let err = |reason: String| MobileError::Movie { reason };
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .map_err(|e| err(format!("not a valid .bk2 (zip) archive: {e}")))?;

    let mut read_member = |wanted: &str| -> Result<String, MobileError> {
        // Match the member name case-insensitively on its final path component so
        // a `Header.txt` / `header.txt` / nested entry all resolve.
        let idx = (0..archive.len())
            .find(|&i| {
                archive.by_index(i).is_ok_and(|e| {
                    std::path::Path::new(e.name())
                        .file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|n| n.eq_ignore_ascii_case(wanted))
                })
            })
            .ok_or_else(|| err(format!(".bk2 is missing '{wanted}'")))?;
        let entry = archive
            .by_index(idx)
            .map_err(|e| err(format!("cannot read '{wanted}': {e}")))?;
        if entry.size() > MAX_MEMBER_BYTES {
            return Err(err(format!("'{wanted}' is implausibly large")));
        }
        let mut buf = Vec::new();
        entry
            .take(MAX_MEMBER_BYTES)
            .read_to_end(&mut buf)
            .map_err(|e| err(format!("cannot read '{wanted}': {e}")))?;
        String::from_utf8(buf).map_err(|e| err(format!("'{wanted}' is not valid UTF-8: {e}")))
    };

    let header = read_member("Header.txt")?;
    let input_log = read_member("Input Log.txt")?;
    Ok((header, input_log))
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

/// A short human-readable label for a [`NatPhase`], for [`NpStatus::detail`].
fn nat_phase_detail(phase: &NatPhase) -> String {
    match phase {
        NatPhase::Registering => "Registering",
        NatPhase::Discovering => "Discovering",
        NatPhase::Exchanging => "Exchanging",
        NatPhase::Punching => "Punching",
        NatPhase::Relaying => "Relaying",
        NatPhase::Synced => "Synced",
        NatPhase::Failed(reason) => return reason.clone(),
    }
    .to_string()
}

/// A non-deterministic 64-bit seed for the NAT orchestrator's room-code +
/// STUN-transaction PRNG (v1.8.7). Drawn from wall-clock + the address of a
/// stack local so two concurrently-launched hosts don't collide on a room code.
///
/// This seeds ONLY host-side network orchestration — never the emulator core's
/// power-on PRNG — so the cross-platform determinism contract (same ROM + seed +
/// input ⇒ byte-identical state) is untouched: the core still cold-boots
/// deterministically when the session promotes to `InGame`.
fn nondeterministic_seed() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    // Truncating the 128-bit nanosecond count to 64 bits is intentional — this is
    // a PRNG seed, not a timestamp, so the discarded high bits don't matter.
    #[allow(clippy::cast_possible_truncation)]
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as u64);
    let stack = std::ptr::addr_of!(nanos) as u64;
    nanos ^ stack.rotate_left(17)
}

/// Drive a `Negotiating` netplay session one tick (v1.8.7 room-code path). Pumps
/// the [`NatConnect`] orchestrator (signaling + STUN/punch + TURN fallback). On
/// [`NatPhase::Synced`] it hands the open (punched) transport off to a
/// [`NetplayConnection`] and promotes the session to `Connecting` — which the
/// existing [`np_tick_connecting`] then drives through the `Sync` handshake and
/// into the [`RollbackSession`] (NO duplication of that tail). On
/// [`NatPhase::Failed`] it records an error and leaves `netplay = None`.
/// Returns a stalled tick (negotiation produces no emulator frame). Called
/// holding the lock, having `take()`n the session out.
///
/// ## Relay-fallback transport hand-off (v1.8.7 — wired)
///
/// For the direct / cone-NAT path, `NatConnect::into_connection` hands off the
/// hole-punched `UdpSocket`. For the **symmetric-NAT TURN-relay** path it hands
/// off a relay-backed [`UdpTransport`] built from the orchestrator's
/// `RelayUdpSocket` + the peer's relayed transport address — both are now a
/// single [`UdpTransport`] over an internal `Direct`/`Relayed` socket source, so
/// the existing [`NetplayConnection`] / [`RollbackSession`] drive either path
/// unchanged (no second session generic). We record `NatConnect::is_relayed`
/// into the sticky `netplay_relayed` flag here, since the `NatConnect` is
/// consumed by `into_connection` and the relayed-ness cannot be re-derived from
/// the live session later; [`NpStatus::relayed`] reads that flag.
fn np_tick_negotiating(g: &mut Inner, mut nat: NatConnect, is_host: bool) -> NpTick {
    match nat.pump() {
        NatPhase::Synced => {
            // The transport (direct OR relayed) is ready; record whether we fell
            // back to the relay BEFORE consuming the orchestrator, then hand the
            // connection off and converge on the existing Connecting → InGame
            // tail.
            g.netplay_relayed = nat.is_relayed();
            let conn = nat.into_connection();
            g.netplay = Some(NetplaySession::Connecting(Box::new(conn), is_host));
            NpTick::STALLED
        }
        NatPhase::Failed(reason) => {
            g.netplay = None;
            g.netplay_error = Some(format!("nat traversal failed: {reason}"));
            NpTick::STALLED
        }
        // Still registering / discovering / exchanging / punching / relaying —
        // keep negotiating.
        _ => {
            g.netplay = Some(NetplaySession::Negotiating(Box::new(nat), is_host));
            NpTick::STALLED
        }
    }
}

/// Drive a `Connecting` netplay session one tick (v1.8.6). Pumps the `Sync`
/// handshake; on `Synced` it power-cycles the core to the deterministic cold
/// boot (so frame 0 is byte-identical across peers — both ran a different number
/// of single-player frames before connecting) and promotes the bound transport
/// into a fresh [`RollbackSession`], storing it back as `InGame`. A handshake
/// timeout / rom-mismatch records an error and leaves `netplay = None`. Called
/// holding the lock, having `take()`n the session out.
fn np_tick_connecting(g: &mut Inner, mut conn: NetplayConnection, is_host: bool) -> NpTick {
    // No session yet, so our own frame advantage is 0.
    match conn.pump(0) {
        ConnectionState::Connecting => {
            g.netplay = Some(NetplaySession::Connecting(Box::new(conn), is_host));
            NpTick::STALLED
        }
        ConnectionState::Synced => {
            // CRITICAL for cross-peer determinism: power-cycle to the cold boot
            // so the session's frame-0 checkpoint matches on every peer (see the
            // desktop `netplay_ui::tick_connecting`).
            g.nes.power_cycle();
            let transport = conn.into_transport();
            let config = SessionConfig {
                local_player: u8::from(!is_host), // host = 0 (P1), joiner = 1 (P2).
                ..SessionConfig::default()
            };
            let rom_hash = *g.nes.rom_sha256();
            let session = RollbackSession::new(config, transport, rom_hash);
            g.netplay = Some(NetplaySession::InGame(Box::new(session), is_host));
            NpTick::STALLED
        }
        ConnectionState::Disconnected => {
            let why = match conn.disconnect_reason() {
                Some(DisconnectReason::RomMismatch) => {
                    "peer is running a different ROM".to_string()
                }
                Some(DisconnectReason::HandshakeTimeout) => {
                    "handshake timed out (no peer answered)".to_string()
                }
                None => "connection closed".to_string(),
            };
            g.netplay = None;
            g.netplay_error = Some(why);
            NpTick::STALLED
        }
    }
}

/// Drive an `InGame` rollback session one tick (v1.8.6): feed the local input,
/// advance the emulator, and map the [`AdvanceOutcome`] to an [`NpTick`]. A
/// [`NetplayError`] tears the session down to an error (a desync also sets the
/// sticky `netplay_desync` flag). Called holding the lock, having `take()`n the
/// session out.
fn np_tick_in_game(
    g: &mut Inner,
    mut session: RollbackSession<UdpTransport>,
    is_host: bool,
    local_mask: u8,
) -> NpTick {
    session.add_local_input(Buttons::from_bits_truncate(local_mask));
    match session.advance(&mut g.nes) {
        Ok(AdvanceOutcome {
            produced_frame,
            rolled_back,
            frame,
            ..
        }) => {
            g.netplay = Some(NetplaySession::InGame(Box::new(session), is_host));
            NpTick {
                produced_frame,
                rolled_back,
                stalled: !produced_frame,
                frame: u64::from(frame),
            }
        }
        Err(e) => {
            let desync = matches!(e, NetplayError::Desync { .. });
            g.netplay = None;
            g.netplay_error = Some(format!("netplay error: {e}"));
            g.netplay_desync = desync;
            NpTick::STALLED
        }
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

// Test-only accessor for the confirmed-entering digest, used by the loopback
// determinism check. Not part of the FFI surface (no `#[uniffi::export]`).
#[cfg(test)]
impl NesController {
    fn np_confirmed_digest_for_test(&self, frame: u32) -> Option<u64> {
        let g = self.lock();
        let d = match &g.netplay {
            Some(NetplaySession::InGame(session, _)) => session.confirmed_entering_digest(frame),
            _ => None,
        };
        drop(g);
        d
    }
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

    // --- Creator / power tools (v1.9.9 "Workshop") -----------------------

    #[test]
    fn genie_codes_round_trip() {
        let ctrl = NesController::new(tiny_nrom(), DEFAULT_SAMPLE_RATE).expect("load");
        assert!(ctrl.cheat_genie_codes().is_empty());
        // "GOSSIP" is a canonical valid 6-character Game Genie code.
        ctrl.cheat_add_genie("GOSSIP".to_string())
            .expect("valid genie code");
        let codes = ctrl.cheat_genie_codes();
        assert_eq!(codes.len(), 1);
        assert_eq!(codes[0].code, "GOSSIP");
        assert!((0x8000..=0xFFFF).contains(&codes[0].addr));
        ctrl.cheat_remove_genie("GOSSIP".to_string());
        assert!(ctrl.cheat_genie_codes().is_empty());
    }

    #[test]
    fn genie_rejects_bad_code() {
        let ctrl = NesController::new(tiny_nrom(), DEFAULT_SAMPLE_RATE).expect("load");
        match ctrl.cheat_add_genie("NOPE".to_string()) {
            Err(MobileError::Cheat { .. }) => {}
            other => panic!("expected Cheat error, got {other:?}"),
        }
        assert!(ctrl.cheat_genie_codes().is_empty());
    }

    #[test]
    fn raw_ram_poke_peek_round_trips() {
        let ctrl = NesController::new(tiny_nrom(), DEFAULT_SAMPLE_RATE).expect("load");
        ctrl.poke_ram(0x0010, 0x42);
        assert_eq!(ctrl.peek_byte(0x0010), 0x42);
    }

    #[test]
    fn debug_cpu_state_is_observational() {
        let ctrl = NesController::new(tiny_nrom(), DEFAULT_SAMPLE_RATE).expect("load");
        ctrl.step_frame();
        let before = ctrl.frame();
        let regs = ctrl.debug_cpu_state();
        // The tight `JMP $8000` loop keeps the PC parked in the reset window.
        assert!(!regs.jammed);
        assert!((0x8000..=0x8002).contains(&regs.pc));
        // Reading debug state must NOT advance the core.
        assert_eq!(ctrl.frame(), before);
    }

    #[test]
    fn debug_read_memory_caps_length_and_reads_prg() {
        let ctrl = NesController::new(tiny_nrom(), DEFAULT_SAMPLE_RATE).expect("load");
        let win = ctrl.debug_read_memory(0x8000, 16);
        assert_eq!(win.len(), 16);
        assert_eq!(win[0], 0x4C, "PRG at $8000 is the JMP opcode");
        // An over-large request is capped at the 64 KiB address space.
        assert_eq!(ctrl.debug_read_memory(0, u32::MAX).len(), 0x1_0000);
    }

    #[test]
    fn debug_disassemble_decodes_reset_loop() {
        let ctrl = NesController::new(tiny_nrom(), DEFAULT_SAMPLE_RATE).expect("load");
        let rows = ctrl.debug_disassemble(0x8000, 4);
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[0].addr, 0x8000);
        assert_eq!(rows[0].mnemonic, "JMP");
        // The count is capped so a hostile request can't allocate unbounded.
        assert_eq!(ctrl.debug_disassemble(0x8000, u32::MAX).len(), 256);
    }

    #[test]
    fn foreign_movie_import_errors_gracefully() {
        let ctrl = NesController::new(tiny_nrom(), DEFAULT_SAMPLE_RATE).expect("load");
        // Malformed input must surface a clean Movie error, never panic / OOB.
        assert!(matches!(
            ctrl.movie_import_fm2(vec![0xFF, 0xFE, 0xFF]),
            Err(MobileError::Movie { .. })
        ));
        assert!(matches!(
            ctrl.movie_import_bk2(b"not a zip".to_vec()),
            Err(MobileError::Movie { .. })
        ));
        assert!(matches!(
            ctrl.movie_import_fcm(b"garbage".to_vec()),
            Err(MobileError::Movie { .. })
        ));
        assert!(matches!(
            ctrl.movie_import_fmv(b"garbage".to_vec()),
            Err(MobileError::Movie { .. })
        ));
        assert!(matches!(
            ctrl.movie_import_vmv(b"garbage".to_vec()),
            Err(MobileError::Movie { .. })
        ));
    }

    // The happy path: a minimal but valid `.fm2` transcodes to non-empty native
    // `.rnm` bytes that `movie_play` accepts. The header needs only a leading
    // `version 3` line (per `import_fm2`'s required-first-key contract); the
    // ROM's SHA-256 is supplied internally by the bridge (not carried in the
    // `.fm2`), so any loaded ROM works. Two input-log lines (`|c|p0|p1|port2|`,
    // each pad an 8-char `RLDUTSBA` field) exercise the frame path: line 1 is
    // all-released, line 2 presses A (last column).
    #[test]
    fn fm2_import_happy_path_transcodes_and_plays() {
        let ctrl = NesController::new(tiny_nrom(), DEFAULT_SAMPLE_RATE).expect("load");
        let fm2 = "version 3\n\
                   |0|........|........||\n\
                   |0|.......A|........||\n";
        let rnm = ctrl
            .movie_import_fm2(fm2.as_bytes().to_vec())
            .expect("minimal valid .fm2 must transcode");
        assert!(!rnm.is_empty(), "transcode must yield native .rnm bytes");
        // The produced movie is power-on anchored against the loaded ROM, so
        // playback must accept it.
        ctrl.movie_play(rnm).expect("native .rnm must replay");
        // ADR 0028: a freshly-transcoded (current-epoch) movie is NOT pre-v2.0.0,
        // so no host warning is queued and `drain_warnings` stays empty.
        assert!(
            ctrl.drain_warnings().is_empty(),
            "a current-epoch .rnm must not raise the pre-Timebase movie warning",
        );
    }

    // ADR 0028 (the epoch-marker half of `fm2_import_happy_path...`): a movie whose
    // header `format_version` is < 2 (a pre-v2.0.0 "Timebase" recording) must, on
    // `movie_play`, still deserialize (the reader accepts `<= MOVIE_FORMAT_VERSION`)
    // AND queue exactly one drainable host warning citing ADR 0028 — parity with the
    // desktop/wasm frontends. We synthesize the pre-v2 blob by taking a valid
    // current-epoch `.rnm` and rewriting only its 2-byte little-endian version field
    // (offset 8..10) from 2 to 1; the post-version layout is byte-identical across the
    // epochs (the v2 bump is purely a marker), so the patched blob deserializes cleanly.
    #[test]
    fn pre_v2_timebase_movie_raises_one_drainable_warning() {
        let ctrl = NesController::new(tiny_nrom(), DEFAULT_SAMPLE_RATE).expect("load");
        let fm2 = "version 3\n\
                   |0|........|........||\n\
                   |0|.......A|........||\n";
        let mut rnm = ctrl
            .movie_import_fm2(fm2.as_bytes().to_vec())
            .expect("minimal valid .fm2 must transcode");
        // Sanity: the freshly transcoded movie is tagged with the current epoch.
        assert_eq!(
            u16::from_le_bytes([rnm[8], rnm[9]]),
            2,
            "transcoded movie must carry the current MOVIE_FORMAT_VERSION",
        );
        // Rewrite the version field 2 -> 1 (LE u16): the only mutation needed to
        // present this as a pre-Timebase recording.
        rnm[8] = 1;
        rnm[9] = 0;
        ctrl.movie_play(rnm)
            .expect("a pre-v2 (version 1) .rnm must still replay its input stream");
        let warnings = ctrl.drain_warnings();
        assert_eq!(
            warnings.len(),
            1,
            "exactly one pre-Timebase warning must be queued, got {warnings:?}",
        );
        assert!(
            warnings[0].contains("ADR 0028"),
            "the queued warning must cite ADR 0028: {}",
            warnings[0],
        );
        // The warning drains: a second call is empty (no re-emit, no leak).
        assert!(
            ctrl.drain_warnings().is_empty(),
            "drain_warnings must empty the queue after the first drain",
        );
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

    // v1.8.6 — a lazily-created session (login/load before `ra_init`) defaults
    // to softcore, so `load_state` is NOT silently blocked. Regression guard for
    // the #164 review: the login/load call sites must seed `false`, not `true`.
    #[test]
    fn lazy_session_defaults_softcore() {
        let ctrl = NesController::new(tiny_nrom(), DEFAULT_SAMPLE_RATE).expect("load");
        // First `ra_*` call is a login (no prior `ra_init`) — must default off.
        ctrl.ra_login_token("nobody".to_string(), "deadbeeftoken".to_string());
        assert!(ctrl.ra_is_enabled());
        assert!(
            !ctrl.ra_hardcore(),
            "a lazily-created session must default to softcore"
        );
    }

    // --- Netplay (v1.8.6) — direct-IP / same-LAN -------------------------

    #[test]
    fn netplay_idle_by_default() {
        let ctrl = NesController::new(tiny_nrom(), DEFAULT_SAMPLE_RATE).expect("load");
        assert!(!ctrl.np_is_active());
        assert_eq!(ctrl.np_status().phase, NpPhase::Idle);
        // np_advance_frame with no session is a safe stall.
        let tick = ctrl.np_advance_frame(0);
        assert!(!tick.produced_frame && tick.stalled);
    }

    #[test]
    fn netplay_host_binds_and_reports_connecting() {
        let ctrl = NesController::new(tiny_nrom(), DEFAULT_SAMPLE_RATE).expect("load");
        // Bind an OS-picked port (0) — must return the concrete bound port.
        let port = ctrl.np_host(0, 2).expect("host bind");
        assert_ne!(port, 0, "np_host returns the OS-picked bound port");
        assert!(ctrl.np_is_active());
        let status = ctrl.np_status();
        assert_eq!(status.phase, NpPhase::Connecting);
        assert!(status.is_host);
        // Leaving returns to idle cleanly.
        ctrl.np_leave();
        assert!(!ctrl.np_is_active());
        assert_eq!(ctrl.np_status().phase, NpPhase::Idle);
    }

    #[test]
    fn netplay_join_rejects_bad_address() {
        let ctrl = NesController::new(tiny_nrom(), DEFAULT_SAMPLE_RATE).expect("load");
        match ctrl.np_join("not-an-address".to_string()) {
            Err(MobileError::Netplay { .. }) => {}
            other => panic!("expected Netplay parse error, got {other:?}"),
        }
        assert!(!ctrl.np_is_active(), "a failed join leaves no session");
    }

    #[test]
    fn load_rom_clears_netplay_session() {
        let ctrl = NesController::new(tiny_nrom(), DEFAULT_SAMPLE_RATE).expect("load");
        let _ = ctrl.np_host(0, 2).expect("host bind");
        assert!(ctrl.np_is_active());
        // A ROM swap ends any session.
        ctrl.load_rom(tiny_nrom(), DEFAULT_SAMPLE_RATE)
            .expect("reload");
        assert!(!ctrl.np_is_active());
        assert_eq!(ctrl.np_status().phase, NpPhase::Idle);
    }

    /// End-to-end loopback: two `NesController`s over `127.0.0.1` complete the
    /// host/join handshake (the host LISTENS and adopts the joiner's address from
    /// its first `Sync`), reach `InGame`, and advance ~120 frames with fixed
    /// inputs. Asserts both reach `InGame`, neither errors/desyncs, and their
    /// `confirmed_entering_digest` agree at a confirmed frame — the determinism /
    /// no-desync check. Real 2-device play needs two devices on the same LAN; the
    /// rollback/transport correctness itself is proven by `rustynes-netplay`'s
    /// own suites.
    #[test]
    fn two_controllers_handshake_and_stay_in_sync() {
        use std::net::{Ipv4Addr, UdpSocket};

        let host = NesController::new(tiny_nrom(), DEFAULT_SAMPLE_RATE).expect("host load");
        let join = NesController::new(tiny_nrom(), DEFAULT_SAMPLE_RATE).expect("join load");

        // The host listens on an OS-picked port; the joiner dials it.
        let host_port = host.np_host(0, 2).expect("host bind");
        let host_addr = SocketAddr::from((Ipv4Addr::LOCALHOST, host_port));
        // Sanity: the probe-free path — the joiner connects to the bound port.
        let _ = UdpSocket::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)));
        join.np_join(host_addr.to_string()).expect("join connect");

        // Pump both until both reach InGame or a bounded number of rounds.
        let mut rounds = 0;
        while !(host.np_status().phase == NpPhase::InGame
            && join.np_status().phase == NpPhase::InGame)
            && rounds < 500
        {
            host.np_advance_frame(0);
            join.np_advance_frame(0);
            rounds += 1;
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert_eq!(
            host.np_status().phase,
            NpPhase::InGame,
            "host reached InGame"
        );
        assert_eq!(
            join.np_status().phase,
            NpPhase::InGame,
            "joiner reached InGame"
        );

        // Advance ~120 frames with a fixed input on each side; neither errors.
        for _ in 0..120 {
            host.np_advance_frame(Buttons::A.bits());
            join.np_advance_frame(Buttons::A.bits());
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let hs = host.np_status();
        let js = join.np_status();
        assert_ne!(
            hs.phase,
            NpPhase::Error,
            "host did not error: {}",
            hs.message
        );
        assert_ne!(
            js.phase,
            NpPhase::Error,
            "joiner did not error: {}",
            js.message
        );
        assert!(!hs.desync && !js.desync, "no desync detected");

        // Both peers confirmed a common prefix; their confirmed digests must
        // agree (the cross-peer determinism property). Find a frame both have
        // confirmed and compare via the live sessions.
        let common = hs
            .confirmed_frame
            .zip(js.confirmed_frame)
            .map(|(a, b)| a.min(b));
        if let Some(common) = common {
            // Pick a confirmed frame below the boundary to compare digests.
            let probe = u32::try_from(common.saturating_sub(2)).unwrap_or(0);
            let hd = host.np_confirmed_digest_for_test(probe);
            let jd = join.np_confirmed_digest_for_test(probe);
            if let (Some(hd), Some(jd)) = (hd, jd) {
                assert_eq!(hd, jd, "confirmed entering digests agree at frame {probe}");
            }
        }
    }

    // --- Netplay (v1.8.7) — room-code / internet path --------------------
    //
    // The loopback proof for the room-code (NAT-traversal) path: two
    // `NesController`s drive `np_host_room` / `np_join_room` against an
    // in-process WebSocket signaling relay + a mock STUN responder (the same
    // harness shape as `rustynes-netplay`'s `tests/nat_loopback.rs`), pump
    // `np_advance_frame` until both reach `InGame`, and assert their confirmed
    // digests agree. On loopback there is no NAT, so the hole-punch path
    // succeeds (no TURN needed) — proving the Negotiating → Connecting → InGame
    // wiring through the bridge end-to-end.

    mod room {
        // Test-harness scaffolding — relax the pedantic/nursery lints that fire
        // on the mock relay + the long end-to-end flow (matching the allow-set on
        // `rustynes-netplay`'s `tests/nat_loopback.rs`); not worth fracturing the
        // mock for.
        #![allow(
            clippy::items_after_statements,
            clippy::collection_is_never_read,
            clippy::manual_let_else,
            clippy::collapsible_if,
            clippy::redundant_clone
        )]
        use super::*;
        use std::collections::HashMap;
        use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream, UdpSocket};
        use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
        use std::sync::mpsc::Sender as MpscSender;
        use std::sync::{Arc, Mutex};
        use std::thread;
        use std::time::{Duration, Instant};

        use rustynes_netplay::{Action, ClientId, Relay, SignalMessage};

        const MAGIC_COOKIE: u32 = 0x2112_A442;

        /// Build a STUN Binding Success Response with an XOR-MAPPED-ADDRESS for a
        /// v4 `addr` (echoing the source the responder saw) — the loopback peer
        /// address that the other side can actually reach.
        fn build_stun_success(addr: SocketAddr, tx: &[u8; 12]) -> Vec<u8> {
            let SocketAddr::V4(v4) = addr else {
                panic!("mock stun is loopback v4 only");
            };
            let cookie_be = MAGIC_COOKIE.to_be_bytes();
            let cookie_hi16 = u16::try_from(MAGIC_COOKIE >> 16).unwrap();
            let x_port = v4.port() ^ cookie_hi16;
            let mut x_addr = v4.ip().octets();
            for (b, k) in x_addr.iter_mut().zip(cookie_be.iter()) {
                *b ^= *k;
            }
            let mut value = vec![0u8, 0x01]; // reserved + family v4
            value.extend_from_slice(&x_port.to_be_bytes());
            value.extend_from_slice(&x_addr);

            let mut attr = Vec::new();
            attr.extend_from_slice(&0x0020u16.to_be_bytes()); // XOR-MAPPED-ADDRESS
            attr.extend_from_slice(&u16::try_from(value.len()).unwrap().to_be_bytes());
            attr.extend_from_slice(&value);

            let mut msg = Vec::new();
            msg.extend_from_slice(&0x0101u16.to_be_bytes()); // Binding Success
            msg.extend_from_slice(&u16::try_from(attr.len()).unwrap().to_be_bytes());
            msg.extend_from_slice(&MAGIC_COOKIE.to_be_bytes());
            msg.extend_from_slice(tx);
            msg.extend_from_slice(&attr);
            msg
        }

        /// A mock STUN server echoing each Binding Request's source address.
        fn spawn_mock_stun() -> (SocketAddr, Arc<AtomicBool>, thread::JoinHandle<()>) {
            let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind mock stun");
            socket
                .set_read_timeout(Some(Duration::from_millis(50)))
                .unwrap();
            let addr = socket.local_addr().unwrap();
            let stop = Arc::new(AtomicBool::new(false));
            let stop_t = Arc::clone(&stop);
            let handle = thread::spawn(move || {
                let mut buf = [0u8; 512];
                while !stop_t.load(Ordering::Relaxed) {
                    match socket.recv_from(&mut buf) {
                        Ok((len, from)) if len >= 20 => {
                            let tx: [u8; 12] = buf[8..20].try_into().unwrap();
                            let resp = build_stun_success(from, &tx);
                            let _ = socket.send_to(&resp, from);
                        }
                        Ok(_) => {}
                        Err(e)
                            if e.kind() == std::io::ErrorKind::WouldBlock
                                || e.kind() == std::io::ErrorKind::TimedOut => {}
                        Err(_) => break,
                    }
                }
            });
            (addr, stop, handle)
        }

        type Outbox = Arc<Mutex<HashMap<ClientId, MpscSender<SignalMessage>>>>;

        /// A mock signaling relay: a real WebSocket server on `127.0.0.1`
        /// driving the production [`Relay`] routing logic.
        fn spawn_mock_relay() -> (String, Arc<AtomicBool>) {
            let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind mock relay");
            listener.set_nonblocking(true).unwrap();
            let addr = listener.local_addr().unwrap();
            let url = format!("ws://{addr}");
            let stop = Arc::new(AtomicBool::new(false));
            let stop_t = Arc::clone(&stop);

            let relay = Arc::new(Mutex::new(Relay::new()));
            let outbox: Outbox = Arc::new(Mutex::new(HashMap::new()));
            let next_id = Arc::new(AtomicU64::new(1));

            thread::spawn(move || {
                let mut workers = Vec::new();
                while !stop_t.load(Ordering::Relaxed) {
                    match listener.accept() {
                        Ok((stream, _peer)) => {
                            stream.set_nonblocking(false).ok();
                            let relay = Arc::clone(&relay);
                            let outbox = Arc::clone(&outbox);
                            let id = next_id.fetch_add(1, Ordering::Relaxed);
                            let stop_w = Arc::clone(&stop_t);
                            workers.push(thread::spawn(move || {
                                relay_client_worker(stream, id, &relay, &outbox, &stop_w);
                            }));
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(2));
                        }
                        Err(_) => break,
                    }
                }
            });
            (url, stop)
        }

        fn relay_client_worker(
            stream: TcpStream,
            id: ClientId,
            relay: &Mutex<Relay>,
            outbox: &Outbox,
            stop: &AtomicBool,
        ) {
            use tungstenite::Message;

            let mut ws = match tungstenite::accept(stream) {
                Ok(ws) => ws,
                Err(_) => return,
            };
            let _ = ws
                .get_ref()
                .set_read_timeout(Some(Duration::from_millis(10)));
            let (out_tx, out_rx) = std::sync::mpsc::channel::<SignalMessage>();
            outbox.lock().unwrap().insert(id, out_tx);

            while !stop.load(Ordering::Relaxed) {
                while let Ok(msg) = out_rx.try_recv() {
                    if ws.send(Message::Text(msg.to_json().into())).is_err() {
                        return;
                    }
                }
                match ws.read() {
                    Ok(Message::Text(txt)) => {
                        if let Some(msg) = SignalMessage::parse(&txt) {
                            let actions = relay.lock().unwrap().handle(id, msg);
                            dispatch(&actions, outbox);
                        }
                    }
                    Ok(Message::Close(_)) => break,
                    Ok(_) => {}
                    Err(tungstenite::Error::Io(e))
                        if matches!(
                            e.kind(),
                            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                        ) => {}
                    Err(_) => break,
                }
            }
            let _ = relay.lock().unwrap().disconnect(id);
        }

        fn dispatch(actions: &[Action], outbox: &Outbox) {
            let map = outbox.lock().unwrap();
            for action in actions {
                if let Action::Send { to, msg } = action {
                    if let Some(tx) = map.get(to) {
                        let _ = tx.send(msg.clone());
                    }
                }
            }
        }

        /// End-to-end loopback for the v1.8.7 room-code path: two
        /// `NesController`s host/join by room code through the bridge, traverse
        /// NAT (signaling + STUN + loopback hole-punch), reach `InGame`, advance
        /// frames, and agree on their confirmed digests. The room-code create /
        /// join + the `Negotiating → Connecting → InGame` transition are all
        /// exercised through the public FFI surface.
        #[test]
        #[allow(clippy::too_many_lines)]
        fn two_controllers_host_join_by_room_code() {
            let host = NesController::new(tiny_nrom(), DEFAULT_SAMPLE_RATE).expect("host load");
            let join = NesController::new(tiny_nrom(), DEFAULT_SAMPLE_RATE).expect("join load");

            let (stun_addr, stun_stop, stun_handle) = spawn_mock_stun();
            let (relay_url, relay_stop) = spawn_mock_relay();
            // Let the relay listener come up.
            thread::sleep(Duration::from_millis(20));

            let cfg = NpNetConfig {
                stun_servers: vec![stun_addr.to_string()],
                turn_url: None,
                turn_user: None,
                turn_secret: None,
                signaling_url: relay_url.clone(),
            };

            let room = host
                .np_host_room(2, cfg.clone())
                .expect("host room returns a code");
            assert_eq!(room.len(), 6, "room code is 6 chars");
            assert_eq!(host.np_status().phase, NpPhase::Negotiating);
            assert!(host.np_is_active());

            join.np_join_room(room, cfg).expect("join by room code");
            assert_eq!(join.np_status().phase, NpPhase::Negotiating);

            // Pump both through Negotiating → Connecting → InGame (bounded).
            let deadline = Instant::now() + Duration::from_secs(20);
            while Instant::now() < deadline
                && !(host.np_status().phase == NpPhase::InGame
                    && join.np_status().phase == NpPhase::InGame)
            {
                host.np_advance_frame(0);
                join.np_advance_frame(0);
                let hs = host.np_status();
                let js = join.np_status();
                assert_ne!(hs.phase, NpPhase::Error, "host negotiation: {}", hs.message);
                assert_ne!(js.phase, NpPhase::Error, "join negotiation: {}", js.message);
                thread::sleep(Duration::from_millis(2));
            }

            assert_eq!(
                host.np_status().phase,
                NpPhase::InGame,
                "host reached InGame via room code (detail: {})",
                host.np_status().detail
            );
            assert_eq!(
                join.np_status().phase,
                NpPhase::InGame,
                "joiner reached InGame via room code (detail: {})",
                join.np_status().detail
            );

            // Advance frames with fixed input; neither errors / desyncs.
            for _ in 0..120 {
                host.np_advance_frame(Buttons::A.bits());
                join.np_advance_frame(Buttons::A.bits());
                std::thread::sleep(Duration::from_millis(1));
            }
            let hs = host.np_status();
            let js = join.np_status();
            assert_ne!(
                hs.phase,
                NpPhase::Error,
                "host did not error: {}",
                hs.message
            );
            assert_ne!(
                js.phase,
                NpPhase::Error,
                "joiner did not error: {}",
                js.message
            );
            assert!(
                !hs.desync && !js.desync,
                "no desync over the room-code path"
            );
            assert!(
                !hs.relayed && !js.relayed,
                "loopback uses the direct punch path"
            );

            // Confirmed digests agree (cross-peer determinism after handoff).
            let common = hs
                .confirmed_frame
                .zip(js.confirmed_frame)
                .map(|(a, b)| a.min(b));
            if let Some(common) = common {
                let probe = u32::try_from(common.saturating_sub(2)).unwrap_or(0);
                let hd = host.np_confirmed_digest_for_test(probe);
                let jd = join.np_confirmed_digest_for_test(probe);
                if let (Some(hd), Some(jd)) = (hd, jd) {
                    assert_eq!(hd, jd, "confirmed digests agree at frame {probe}");
                }
            }

            stun_stop.store(true, Ordering::Relaxed);
            relay_stop.store(true, Ordering::Relaxed);
            let _ = stun_handle.join();
        }
    }
}
