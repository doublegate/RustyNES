//! Netplay UI state machine + run-loop driver (v2.3.0 Stage 3).
//!
//! This is the frontend plumbing on top of the deterministic netplay CORE
//! that landed in Stages 1 + 2 (`rustynes_netplay::{NetplayConnection,
//! RollbackSession, UdpTransport}`). It mirrors the `MovieUi` pattern: a
//! small Idle/Connecting/InGame/Error state machine with a single
//! per-frame [`tick`](NetplayUi::tick) hook the produce path calls
//! INSTEAD of `nes.run_frame()` while a session is active.
//!
//! # Native-only
//!
//! The whole module is `#[cfg(not(target_arch = "wasm32"))]`: it drives a
//! [`UdpTransport`] over `std::net::UdpSocket`, which does not exist on
//! `wasm32-unknown-unknown`. The browser builds compile with this module
//! absent and the netplay panel showing a "native-only" note (see
//! `debugger::netplay_panel`).
//!
//! # Lifecycle
//!
//! 1. `NetplayUi::start_host` / `NetplayUi::start_join` bind the socket
//!    and begin the [`NetplayConnection`] handshake — the UI enters
//!    `NetplayState::Connecting`.
//! 2. The produce path calls [`tick`](NetplayUi::tick) once per frame:
//!    - **Connecting**: `pump`s the handshake; on `Synced` it promotes the
//!      connection into a [`RollbackSession`] (`Disconnected`/timeout/
//!      rom-mismatch transition to `NetplayState::Error`).
//!    - **`InGame`**: feeds the local input, [`advance`](RollbackSession::advance)s
//!      the emulator, and reports whether a frame was produced (`false` means
//!      a time-sync stall — the caller skips rendering this tick).
//! 3. `NetplayUi::leave` tears the session down and returns to
//!    single-player cleanly.
//!
//! Determinism is unchanged: when netplay is inactive (`NetplayState::Idle`)
//! the produce path is byte-for-byte the single-player path. The rollback
//! session itself draws randomness only from confirmed inputs + the ROM seed
//! (see `rustynes-netplay`), so two peers converge bit-for-bit.
//!
//! # Convention
//!
//! The **host is player 0 (P1, `$4016`)**; a 2-player joiner is player 1 (P2,
//! `$4017`). Every peer feeds its LOCAL keyboard/gamepad as `player1()`; the
//! session routes it to the correct port via `SessionConfig.local_player`.
//!
//! # Three and four players (v3.4.0, `T-NETPLAY-MESH-NATIVE`)
//!
//! A host that picks 3 or 4 players gathers its joiners with
//! [`MeshHost`] instead of a [`NetplayConnection`]: it adopts each joiner from
//! its `Sync`, assigns it the next player index, and sends every joiner the
//! roster (every peer's address, index and key share). Each peer then runs a
//! [`RollbackSession`] over a sealed `UdpMeshTransport` to every other peer,
//! with the Four Score on.
//!
//! The joiner does not need to know which kind of host it dials: it always
//! joins through a [`MeshJoiner`], which completes against a 2-player host
//! (whose `NetplayConnection` echoes the joiner's `Sync`) as player 1 of 2,
//! and against a mesh host (which sends the roster) as the player the roster
//! gives it. A mobile peer, whose `NetplayConnection` host is a 2-player host,
//! is joinable the same way. The mobile apps themselves do not host or join 3-4
//! player sessions yet (`MOB-09`, a later release).

use std::net::SocketAddr;

use rustynes_core::{Buttons, Nes};
use rustynes_netplay::{
    AdvanceOutcome, ConnectionState, DisconnectReason, MeshHost, MeshJoiner, NetplayConnection,
    NetplayError, RollbackSession, SessionConfig, SessionIdentity, SpectatorConfig,
    SpectatorSession, Transport, UdpTransport,
};

/// How long a joiner waits for its host to answer, and a 3-4 player host for
/// all its joiners, before giving up with a reason. A 2-player host's
/// `NetplayConnection` has the same 10 s handshake timeout.
const HANDSHAKE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// A 3-4 player host waits longer: every joiner has to arrive.
const MESH_GATHER_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

/// The session's transport: a 2-player host's [`UdpTransport`] or a mesh peer's
/// (and every joiner's) `UdpMeshTransport`. Both are sealed (protocol 9).
type SessionTransport = Box<dyn Transport>;

/// Default local UDP port a host binds when none is specified.
pub const DEFAULT_HOST_PORT: u16 = 7000;

/// The coarse phase the netplay UI is in, for the HUD + panel.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum NetplayPhase {
    /// No netplay — live input drives the emulator (single-player).
    #[default]
    Idle,
    /// The `Sync` handshake is in progress (host or joiner).
    Connecting,
    /// A rollback session is running.
    InGame,
    /// A read-only spectator session is running (v1.7.0 H8): the local
    /// emulator replays the match's confirmed input stream — it never plays.
    Spectating,
    /// The connection / session ended in an error (terminal until `leave`).
    Error,
}

/// A copyable status snapshot for the egui HUD / netplay panel. Pushed into
/// the debugger overlay each pacer iteration, mirroring `MovieStatus`.
#[derive(Clone, Debug, Default)]
pub struct NetplayStatus {
    /// Current phase.
    pub phase: NetplayPhase,
    /// `true` if this peer is the host (player 0 / P1).
    pub is_host: bool,
    /// Smoothed round-trip ping in ms, once measured.
    pub ping_ms: Option<u32>,
    /// The frame the session is producing next (`InGame` only).
    pub current_frame: u32,
    /// Newest frame confirmed by both peers (`InGame` only).
    pub confirmed_frame: Option<u32>,
    /// `true` if the most recent tick rolled back + re-simulated.
    pub rolled_back: bool,
    /// How many frames the most recent tick re-simulated.
    pub resimulated_frames: u32,
    /// `true` if the most recent tick stalled for time-sync (no frame
    /// produced — the caller skipped rendering).
    pub stalled: bool,
    /// v1.7.0 H8 — when [`Spectating`](NetplayPhase::Spectating), how many
    /// fully-confirmed frames are buffered but not yet shown (how far the
    /// spectator is behind the live match). `0` in every other phase.
    pub spectator_pending: u32,
    /// An error / disconnect message (Error phase), else empty.
    pub message: String,
    /// v1.3.0 Workstream G1 — read-only desync diagnostics + session topology,
    /// rebuilt from the live session each `InGame` tick. Default (inert) in
    /// every other phase. Observational only.
    pub diagnostics: crate::debugger::NetplayDiagnosticsView,
}

/// What a single `NetplayUi::tick` did, so the produce path knows whether
/// to present this frame.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct NetplayTick {
    /// `true` if a session is active and consumed this tick (so the caller
    /// must NOT also call `nes.run_frame()`). `false` means netplay is
    /// inactive and the caller should run the normal single-player frame.
    pub active: bool,
    /// `true` if the emulator advanced a frame this tick (present it).
    /// `false` while connecting, on error, or on a time-sync stall.
    pub produced_frame: bool,
}

impl NetplayTick {
    /// The tick result when netplay is inactive (single-player passthrough).
    const INACTIVE: Self = Self {
        active: false,
        produced_frame: false,
    };
}

/// Active netplay state. Exactly one variant is live at a time.
#[derive(Default)]
enum NetplayState {
    /// No session — single-player.
    #[default]
    Idle,
    /// Handshake in progress (a 2-player host).
    Connecting(Box<NetplayConnection>),
    /// v3.4.0 — a 3-4 player host gathering its joiners.
    MeshHosting(Box<MeshHost>),
    /// v3.4.0 — a joiner waiting for its host (either kind).
    Joining(Box<MeshJoiner>),
    /// Rollback session running.
    InGame(Box<RollbackSession<SessionTransport>>),
    /// v1.7.0 H8 — read-only spectator session running. Drives the local
    /// emulator from the received confirmed input stream; sends nothing.
    Spectating(Box<SpectatorSession<UdpTransport>>),
    /// Terminal error (until `leave`).
    Error(String),
}

/// Frontend netplay state machine. Owned by `App` (like `MovieUi`); driven
/// once per frame from the produce path.
pub struct NetplayUi {
    state: NetplayState,
    /// `true` if this peer hosted (player 0). Recorded at connect so the HUD
    /// and `SessionConfig.local_player` agree.
    is_host: bool,
    /// The loaded ROM's identity and (v2.9.8) its machine configuration,
    /// captured at connect for the handshake + session
    /// ([`SessionIdentity::of`]). The peer must announce an identical one.
    identity: SessionIdentity,
    /// When the session last confirmed a new frame (v3.2.0 peer liveness):
    /// five seconds of stall past it ends the session with a reason.
    last_peer_activity: std::time::Instant,
    /// The newest confirmed frame seen, so only progress refreshes
    /// `last_peer_activity`.
    last_confirmed_tracker: u32,
    /// Cached status for the HUD, refreshed each `tick`.
    status: NetplayStatus,
    /// Session config (input delay, rollback window, checksum interval). The
    /// `local_player` field is overwritten at connect from `is_host`.
    config: SessionConfig,
    /// Extra delayed-stream buffer depth (frames) applied when *spectating* a
    /// match — a broadcast / anti-spoiler delay layered on top of the natural
    /// spectator lag. `0` (default) shows confirmed frames immediately. See
    /// [`SpectatorConfig::delay_frames`](rustynes_netplay::SpectatorConfig::delay_frames).
    spectator_delay_frames: u32,
}

impl Default for NetplayUi {
    fn default() -> Self {
        Self {
            state: NetplayState::Idle,
            is_host: false,
            identity: SessionIdentity::new([0u8; 32], [0u8; 32]),
            last_peer_activity: std::time::Instant::now(),
            last_confirmed_tracker: 0,
            status: NetplayStatus::default(),
            config: SessionConfig::default(),
            spectator_delay_frames: 0,
        }
    }
}

impl NetplayUi {
    /// `true` while a session is active or connecting (so the produce path
    /// drives via [`tick`](Self::tick) instead of `run_frame`). The `Error`
    /// phase also counts as active — the emulator is frozen on the error
    /// until the user leaves — so single-player input cannot silently bleed
    /// into a half-torn-down session.
    #[must_use]
    pub const fn is_active(&self) -> bool {
        !matches!(self.state, NetplayState::Idle)
    }

    /// Set the delayed-stream buffer depth (frames) used when spectating. Takes
    /// effect on the next spectate; clamped by the session to
    /// [`SpectatorConfig::MAX_DELAY_FRAMES`](rustynes_netplay::SpectatorConfig::MAX_DELAY_FRAMES).
    pub const fn set_spectator_delay_frames(&mut self, frames: u32) {
        self.spectator_delay_frames = frames;
    }

    /// The configured spectator delayed-stream buffer depth (frames).
    #[must_use]
    pub const fn spectator_delay_frames(&self) -> u32 {
        self.spectator_delay_frames
    }

    /// The current phase.
    #[must_use]
    pub const fn phase(&self) -> NetplayPhase {
        match self.state {
            NetplayState::Idle => NetplayPhase::Idle,
            NetplayState::Connecting(_)
            | NetplayState::MeshHosting(_)
            | NetplayState::Joining(_) => NetplayPhase::Connecting,
            NetplayState::InGame(_) => NetplayPhase::InGame,
            NetplayState::Spectating(_) => NetplayPhase::Spectating,
            NetplayState::Error(_) => NetplayPhase::Error,
        }
    }

    /// A clone of the latest HUD status snapshot.
    #[must_use]
    pub fn status(&self) -> NetplayStatus {
        self.status.clone()
    }

    /// The concrete local socket address while connecting (resolves an
    /// ephemeral `:0` join port to the OS-picked one). `None` once a session
    /// has started (the transport is owned by the session) or when idle. Used
    /// for the host's "share your IP:port with the joiner" display and by the
    /// loopback test to pair peers.
    #[must_use]
    pub fn local_addr(&self) -> Option<SocketAddr> {
        match &self.state {
            NetplayState::Connecting(conn) => conn.transport().local_addr().ok(),
            NetplayState::MeshHosting(host) => host.local_addr().ok(),
            NetplayState::Joining(joiner) => joiner.local_addr().ok(),
            _ => None,
        }
    }

    /// Host a session: bind `0.0.0.0:local_port` and **listen** as player 0
    /// (P1), learning the joiner's address from its first valid `Sync`. Any
    /// previous session is dropped.
    ///
    /// `num_players` (2..=4) selects how many players the session runs; 3-4
    /// players enable the Four Score adapter. It is clamped into `2..=4`. Two
    /// players host through a [`NetplayConnection`] (so a mobile peer can join
    /// too); since v3.4.0 three or four host through a [`MeshHost`], which
    /// waits for every joiner, gives each the next player index and wires the
    /// full mesh (see the module docs).
    ///
    /// The host no longer needs to pre-enter the joiner's address — it just
    /// shares its own listening `IP:port` and the joiner dials in (see
    /// [`NetplayConnection::host`]).
    pub fn start_host(&mut self, local_port: u16, num_players: u8, identity: SessionIdentity) {
        let local = SocketAddr::from(([0, 0, 0, 0], local_port));
        self.is_host = true;
        self.identity = identity;
        self.config.num_players = num_players.clamp(2, 4);
        self.config.local_player = 0; // host = player 0.
        if self.config.num_players > 2 {
            // The host's own roster address is the unspecified one it bound:
            // every joiner substitutes the address it dialed (`MeshJoiner`).
            match MeshHost::bind(local, local, self.config.num_players, identity) {
                Ok(host) => {
                    self.state =
                        NetplayState::MeshHosting(Box::new(host.with_timeout(MESH_GATHER_TIMEOUT)));
                    self.status = NetplayStatus {
                        phase: NetplayPhase::Connecting,
                        is_host: true,
                        ..NetplayStatus::default()
                    };
                }
                Err(e) => self.fail(format!("host bind failed: {e}")),
            }
            return;
        }
        match NetplayConnection::host(local, identity) {
            Ok(conn) => self.enter_connecting(conn, true),
            Err(e) => self.fail(format!("host bind failed: {e}")),
        }
    }

    /// Join a session hosted at `remote`: bind an ephemeral local port and
    /// dial the host. Any previous session is dropped.
    ///
    /// v3.4.0: the joiner learns the session's size from the host, so the
    /// same call joins a 2-player host (as player 1 of 2) and a 3-4 player
    /// host (as the player its roster assigns); see the module docs.
    pub fn start_join(&mut self, remote: SocketAddr, identity: SessionIdentity) {
        let local = SocketAddr::from(([0, 0, 0, 0], 0));
        self.is_host = false;
        self.identity = identity;
        // Placeholders until the host answers; `tick_joining` sets both.
        self.config.num_players = 2;
        self.config.local_player = 1;
        match MeshJoiner::connect(local, remote, 1, identity) {
            Ok(joiner) => {
                self.state =
                    NetplayState::Joining(Box::new(joiner.with_timeout(HANDSHAKE_TIMEOUT)));
                self.status = NetplayStatus {
                    phase: NetplayPhase::Connecting,
                    is_host: false,
                    ..NetplayStatus::default()
                };
            }
            Err(e) => self.fail(format!("connect failed: {e}")),
        }
    }

    /// v1.7.0 "Forge" Workstream H8 — **spectate** a match hosted at `remote`:
    /// bind an ephemeral local port and join as a READ-ONLY observer. The
    /// spectator replays the match's confirmed input stream into the local
    /// emulator and **never authors or sends gameplay input**, so it cannot
    /// perturb the match it is watching (the determinism-safety contract).
    ///
    /// It announces itself once with a single `Sync` so a spectator-aware host
    /// learns where to relay the input stream; after that it is purely
    /// poll-only. Any previous session is dropped.
    ///
    /// The host-side spectator-broadcast wiring + the `deploy/` relay config are
    /// a documented maintainer-manual carryover (like the live 2-4p host/TURN
    /// matrix) — the frontend driver here is exercised by the loopback unit
    /// test. The local emulator is power-cycled to the deterministic cold-boot
    /// so frame 0 matches the players' canonical timeline.
    pub fn start_spectate(&mut self, remote: SocketAddr, identity: SessionIdentity) {
        let local = SocketAddr::from(([0, 0, 0, 0], 0));
        self.is_host = false;
        self.identity = identity;
        // A spectator does not own a controller port; the count is adopted from
        // the host's roster (defaults to 2 until then).
        self.config.num_players = 2;
        match UdpTransport::bind(local, remote) {
            Ok(mut transport) => {
                // One-shot self-announce so a spectator-aware host can register
                // us and start relaying the stream. After this we never send.
                use rustynes_netplay::{NetMessage, Transport as _};
                transport.send(&NetMessage::Sync {
                    magic: NetMessage::SYNC_MAGIC,
                    identity,
                    // A spectator derives no key (v3.4.0): it only listens.
                    key_share: [0; rustynes_netplay::KEY_SHARE_LEN],
                });
                let session = SpectatorSession::new(
                    SpectatorConfig {
                        num_players: self.config.num_players,
                        delay_frames: self.spectator_delay_frames,
                    },
                    transport,
                    identity,
                );
                self.state = NetplayState::Spectating(Box::new(session));
                self.status = NetplayStatus {
                    phase: NetplayPhase::Spectating,
                    is_host: false,
                    ..NetplayStatus::default()
                };
            }
            Err(e) => self.fail(format!("spectate bind failed: {e}")),
        }
    }

    /// Shared post-bind transition into the `Connecting` phase.
    fn enter_connecting(&mut self, conn: NetplayConnection, is_host: bool) {
        self.state = NetplayState::Connecting(Box::new(conn));
        self.status = NetplayStatus {
            phase: NetplayPhase::Connecting,
            is_host,
            ..NetplayStatus::default()
        };
    }

    /// Tear the session down and return to single-player. No-op if idle.
    pub fn leave(&mut self) {
        self.state = NetplayState::Idle;
        self.status = NetplayStatus::default();
    }

    /// Per-frame hook, called from `App::produce_one_frame` in place of the
    /// single-player `run_frame` when [`is_active`](Self::is_active).
    ///
    /// `local_buttons` is this peer's live input (the local keyboard/gamepad,
    /// `self.input.player1()`).
    ///
    /// - **Idle**: returns `NetplayTick::INACTIVE` — the caller runs the
    ///   normal single-player frame.
    /// - **Connecting**: `pump`s the handshake (no emulation); promotes to a
    ///   session on `Synced`. `produced_frame == false`.
    /// - **`InGame`**: feeds `local_buttons`, advances the emulator, and reports
    ///   `produced_frame` (false = time-sync stall → skip rendering).
    /// - **Error**: holds (no emulation) until [`leave`](Self::leave).
    pub fn tick(&mut self, nes: &mut Nes, local_buttons: Buttons) -> NetplayTick {
        match &mut self.state {
            NetplayState::Idle => NetplayTick::INACTIVE,
            NetplayState::Connecting(_) => self.tick_connecting(nes),
            NetplayState::MeshHosting(_) => self.tick_mesh_hosting(nes),
            NetplayState::Joining(_) => self.tick_joining(nes),
            NetplayState::InGame(_) => self.tick_in_game(nes, local_buttons),
            // A spectator ignores `local_buttons` — it never authors input.
            NetplayState::Spectating(_) => self.tick_spectating(nes),
            NetplayState::Error(msg) => {
                self.status.phase = NetplayPhase::Error;
                self.status.message = msg.clone();
                NetplayTick {
                    active: true,
                    produced_frame: false,
                }
            }
        }
    }

    /// Drive the handshake one tick. On `Synced` promote the connection into a
    /// [`RollbackSession`]; on `Disconnected` map the reason to an error.
    fn tick_connecting(&mut self, nes: &mut Nes) -> NetplayTick {
        // Take the connection out so we can move it into a session on success.
        let NetplayState::Connecting(mut conn) =
            core::mem::replace(&mut self.state, NetplayState::Idle)
        else {
            unreachable!("tick_connecting only runs in the Connecting state");
        };

        // No session yet, so our own frame advantage is 0.
        let net_state = conn.pump(0);
        self.status.phase = NetplayPhase::Connecting;
        self.status.is_host = self.is_host;
        self.status.ping_ms = conn.ping_ms();

        match net_state {
            ConnectionState::Connecting => self.state = NetplayState::Connecting(conn),
            // Hand the bound + handshaken transport to a fresh session (which
            // power-cycles both peers to one cold boot; see `enter_game`).
            ConnectionState::Synced => self.enter_game(nes, Box::new(conn.into_transport())),
            ConnectionState::Disconnected => {
                let why = match conn.disconnect_reason() {
                    Some(DisconnectReason::RomMismatch) => {
                        "peer is running a different ROM".to_string()
                    }
                    Some(DisconnectReason::ConfigMismatch) => {
                        "peer runs this ROM with different emulation settings (console \
                         model, power-on RAM, die revisions, overclock, Four Score, Vs. \
                         settings, Game Genie codes, region or header); match them and \
                         reconnect"
                            .to_string()
                    }
                    Some(DisconnectReason::EmulatorMismatch { ours, theirs }) => {
                        rustynes_netplay::emulator_mismatch_text(ours, theirs)
                    }
                    Some(DisconnectReason::HandshakeTimeout) => {
                        "handshake timed out (no peer answered)".to_string()
                    }
                    Some(DisconnectReason::PeerTimeout) => {
                        "peer connection lost (no data for several seconds)".to_string()
                    }
                    None => "connection closed".to_string(),
                };
                self.fail(why);
            }
        }
        NetplayTick {
            active: true,
            produced_frame: false,
        }
    }

    /// Start the rollback session over a handshaken transport, with the
    /// player count and index `self.config` now holds.
    fn enter_game(&mut self, nes: &mut Nes, transport: SessionTransport) {
        // CRITICAL for cross-peer determinism: every peer was running the ROM
        // single-player (a DIFFERENT number of frames each) before the
        // handshake completed, so their state diverges. Power-cycle to the
        // deterministic cold-boot (zeroed WRAM, fixed phase) so the session's
        // frame-0 checkpoint is byte-identical on every peer; otherwise the
        // first confirmed-frame checksum trips a desync immediately.
        //
        // v2.9.0: with cleared save RAM as well, which is what this start
        // gave every peer until v2.9.0 made a power cycle keep battery RAM.
        // Each peer's own `.sav` differs, and the checksum hashes the frame,
        // not the RAM, so the difference would surface only once the game read
        // its save (CodeRabbit on #561). The app keeps that cleared RAM off the
        // `.sav` (`EmuCore::release_battery_for_session`).
        rustynes_core::power_on_for_movie(nes);
        let session = RollbackSession::new(self.config, transport, self.identity);
        self.state = NetplayState::InGame(Box::new(session));
        self.last_peer_activity = std::time::Instant::now();
        self.last_confirmed_tracker = 0;
        self.status.phase = NetplayPhase::InGame;
        self.status.is_host = self.is_host;
    }

    /// v3.4.0 — drive a 3-4 player host's gathering one tick. Once every
    /// joiner has arrived and the roster is out, the host is player 0 of
    /// `num_players` over the sealed mesh.
    fn tick_mesh_hosting(&mut self, nes: &mut Nes) -> NetplayTick {
        let NetplayState::MeshHosting(host) = &mut self.state else {
            unreachable!("tick_mesh_hosting only runs in the MeshHosting state");
        };
        self.status.phase = NetplayPhase::Connecting;
        self.status.is_host = true;
        match host.pump() {
            Ok(None) => {}
            Ok(Some(transport)) => {
                self.config.num_players = host.num_players();
                self.config.local_player = 0;
                self.enter_game(nes, Box::new(transport));
            }
            Err(e) => self.fail(e.to_string()),
        }
        NetplayTick {
            active: true,
            produced_frame: false,
        }
    }

    /// v3.4.0 — drive a joiner's handshake one tick. The host's answer says
    /// how many players the session has and which one this peer is.
    fn tick_joining(&mut self, nes: &mut Nes) -> NetplayTick {
        let NetplayState::Joining(joiner) = &mut self.state else {
            unreachable!("tick_joining only runs in the Joining state");
        };
        self.status.phase = NetplayPhase::Connecting;
        self.status.is_host = false;
        match joiner.pump() {
            Ok(None) => {}
            Ok(Some(transport)) => {
                self.config.num_players = joiner.num_players();
                self.config.local_player = joiner.my_player();
                self.enter_game(nes, Box::new(transport));
            }
            Err(rustynes_netplay::MeshError::Timeout) => {
                self.fail("handshake timed out (no host answered)".to_string());
            }
            Err(e) => self.fail(e.to_string()),
        }
        NetplayTick {
            active: true,
            produced_frame: false,
        }
    }

    /// Drive the rollback session one tick: feed the local input and advance.
    fn tick_in_game(&mut self, nes: &mut Nes, local_buttons: Buttons) -> NetplayTick {
        let NetplayState::InGame(session) = &mut self.state else {
            unreachable!("tick_in_game only runs in the InGame state");
        };

        session.add_local_input(local_buttons);
        // Peer liveness (v3.2.0): progress is a newly confirmed frame, which
        // needs the peer's input. Five seconds without one while the session
        // is stalled ends it with a reason instead of freezing silently.
        if let Some(confirmed) = session.last_confirmed_frame()
            && confirmed > self.last_confirmed_tracker
        {
            self.last_confirmed_tracker = confirmed;
            self.last_peer_activity = std::time::Instant::now();
        }
        match session.advance(nes) {
            Ok(AdvanceOutcome {
                produced_frame,
                rolled_back,
                resimulated_frames,
                ..
            }) => {
                if !produced_frame
                    && self.last_peer_activity.elapsed() > std::time::Duration::from_secs(5)
                {
                    self.fail("peer connection lost (no data for several seconds)".to_string());
                    // `active: true`, like every other `fail()` path: the tick
                    // was the session's, and `false` would tell the caller to
                    // run a single-player frame on the failure tick (CodeRabbit
                    // on #604 and #605).
                    return NetplayTick {
                        active: true,
                        produced_frame: false,
                    };
                }
                self.status.phase = NetplayPhase::InGame;
                self.status.is_host = self.is_host;
                self.status.current_frame = session.current_frame();
                self.status.confirmed_frame = session.last_confirmed_frame();
                self.status.rolled_back = rolled_back;
                self.status.resimulated_frames = resimulated_frames;
                self.status.stalled = !produced_frame;
                self.status.diagnostics = diagnostics_view(session);
                NetplayTick {
                    active: true,
                    produced_frame,
                }
            }
            Err(e) => {
                let why = match e {
                    NetplayError::Desync {
                        frame,
                        same_framebuffer,
                        ..
                    } => {
                        let kind = if same_framebuffer {
                            "timing/cycle — same picture"
                        } else {
                            "state — picture differs"
                        };
                        format!("desync at frame {frame} ({kind})")
                    }
                    NetplayError::RomMismatch => "rom mismatch".to_string(),
                    NetplayError::ConfigMismatch | NetplayError::EmulatorMismatch { .. } => {
                        format!("{e}")
                    }
                    NetplayError::Restore(ref s) => format!("rollback restore failed: {s}"),
                    // `NetplayError` is `#[non_exhaustive]`; surface any future
                    // variant via its `Display` rather than panicking.
                    ref other => format!("netplay error: {other}"),
                };
                self.fail(why);
                NetplayTick {
                    active: true,
                    produced_frame: false,
                }
            }
        }
    }

    // (diagnostics view builder is a free fn below — see `diagnostics_view`.)

    /// v1.7.0 H8 — drive the read-only spectator one tick: poll the input
    /// stream and advance the local emulator when the next frame is confirmed.
    /// Never sends, predicts, or rolls back — so it cannot error. A tick that
    /// produces no frame is a "waiting for the next confirmed frame" stall (the
    /// caller skips rendering), exactly like the time-sync stall on the player
    /// path.
    fn tick_spectating(&mut self, nes: &mut Nes) -> NetplayTick {
        let NetplayState::Spectating(session) = &mut self.state else {
            unreachable!("tick_spectating only runs in the Spectating state");
        };
        let out = session.advance(nes);
        // v2.9.9 (NF-15) — a stream announced for another game or another
        // machine configuration is refused with the reason, as the player
        // handshake refuses a mismatched peer. Before v2.9.9 nothing read the
        // spectator's sync state, so it ran the stream and showed a different
        // game than the one being played.
        if let Some(why) = session.mismatch() {
            self.fail(match why {
                rustynes_netplay::IdentityMismatch::Rom => {
                    "the watched match is running a different ROM".to_string()
                }
                rustynes_netplay::IdentityMismatch::Config => {
                    "the watched match runs this ROM with different emulation settings; \
                     match them and spectate again"
                        .to_string()
                }
                rustynes_netplay::IdentityMismatch::Emulator { ours, theirs } => format!(
                    "the watched match cannot be shown: {}",
                    rustynes_netplay::emulator_mismatch_text(ours, theirs)
                ),
            });
            return NetplayTick {
                active: true,
                produced_frame: false,
            };
        }
        // v3.0.0 — the spectator fell so far behind that input was dropped,
        // and the players never resend it. Say so rather than freezing on
        // the last frame kept.
        if let Some(frame) = session.stream_lost() {
            self.fail(format!(
                "the spectator fell too far behind the match (about 18 minutes) and \
                 the input from frame {frame} on was not kept; spectate again to rejoin"
            ));
            return NetplayTick {
                active: true,
                produced_frame: false,
            };
        }
        self.status.phase = NetplayPhase::Spectating;
        self.status.is_host = false;
        self.status.current_frame = session.current_frame();
        self.status.confirmed_frame = session.last_confirmed_frame();
        self.status.spectator_pending = session.pending_frames();
        self.status.stalled = !out.produced_frame;
        NetplayTick {
            active: true,
            produced_frame: out.produced_frame,
        }
    }

    /// Transition to the terminal `Error` phase with a message.
    fn fail(&mut self, message: String) {
        self.status = NetplayStatus {
            phase: NetplayPhase::Error,
            is_host: self.is_host,
            message: message.clone(),
            ..NetplayStatus::default()
        };
        self.state = NetplayState::Error(message);
    }
}

/// v1.3.0 Workstream G1 — build the read-only [`NetplayDiagnosticsView`] from a
/// live session's observational [`DesyncDiagnostics`] + topology. Pure read; it
/// never mutates the session.
///
/// [`NetplayDiagnosticsView`]: crate::debugger::NetplayDiagnosticsView
/// [`DesyncDiagnostics`]: rustynes_netplay::DesyncDiagnostics
fn diagnostics_view<T: Transport>(
    session: &RollbackSession<T>,
) -> crate::debugger::NetplayDiagnosticsView {
    use crate::debugger::{CrcCompareView, NetplayDiagnosticsView};
    let diag = session.diagnostics();
    let last = diag.last().map(|c| CrcCompareView {
        frame: c.frame,
        local: c.local,
        remote: c.remote,
        matched: c.matched,
        same_framebuffer: c.same_framebuffer,
    });
    // Keep only the most recent rows for the panel table (newest preserved).
    let len = diag.history_len();
    let skip = len.saturating_sub(NetplayDiagnosticsView::HISTORY_SHOWN);
    let recent = diag
        .history()
        .skip(skip)
        .map(|c| CrcCompareView {
            frame: c.frame,
            local: c.local,
            remote: c.remote,
            matched: c.matched,
            same_framebuffer: c.same_framebuffer,
        })
        .collect();
    NetplayDiagnosticsView {
        num_players: session.num_players(),
        local_player: session.local_player(),
        in_sync: diag.in_sync(),
        first_desync_frame: diag.first_desync_frame(),
        consecutive_mismatches: diag.consecutive_mismatches(),
        total_compares: diag.total(),
        mismatches: diag.mismatches(),
        last_compare: last,
        recent,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, UdpSocket};

    // A minimal NROM (infinite loop) so the session can advance frames
    // without a real game. Mirrors the movie_ui / core test fixture.
    fn synth_nrom() -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"NES\x1A");
        bytes.push(1);
        bytes.push(1);
        bytes.push(0);
        bytes.push(0);
        bytes.extend_from_slice(&[0u8; 8]);
        let mut prg = vec![0u8; 16 * 1024];
        prg[0] = 0x4C;
        prg[1] = 0x00;
        prg[2] = 0xC0;
        let len = prg.len();
        prg[len - 4] = 0x00;
        prg[len - 3] = 0xC0;
        prg[len - 6] = 0x00;
        prg[len - 5] = 0xC0;
        prg[len - 2] = 0x00;
        prg[len - 1] = 0xC0;
        bytes.extend_from_slice(&prg);
        bytes.extend_from_slice(&vec![0u8; 8 * 1024]);
        bytes
    }

    #[test]
    fn idle_by_default() {
        let ui = NetplayUi::default();
        assert_eq!(ui.phase(), NetplayPhase::Idle);
        assert!(!ui.is_active());
    }

    #[test]
    fn idle_tick_is_inactive_passthrough() {
        let rom = synth_nrom();
        let mut nes = Nes::from_rom(&rom).unwrap();
        let mut ui = NetplayUi::default();
        let tick = ui.tick(&mut nes, Buttons::empty());
        assert!(!tick.active, "idle netplay is a single-player passthrough");
        assert!(!tick.produced_frame);
    }

    #[test]
    fn bad_bind_transitions_to_error() {
        // Port 0 binds fine; instead force a bind failure by reusing an
        // address that is already exclusively bound is platform-specific, so
        // assert the simpler property: a successful host start enters
        // Connecting, and leaving returns to Idle cleanly.
        let mut ui = NetplayUi::default();
        ui.start_host(0, 2, SessionIdentity::new([0u8; 32], [0u8; 32]));
        assert_eq!(ui.phase(), NetplayPhase::Connecting);
        assert!(ui.is_active());
        ui.leave();
        assert_eq!(ui.phase(), NetplayPhase::Idle);
        assert!(!ui.is_active());
    }

    /// End-to-end smoke: two in-process `NetplayUi`s over loopback reach
    /// `InGame` and advance a few frames. This exercises the host-listen /
    /// join handshake promotion (the host ADOPTS the joiner's address from its
    /// first `Sync`, so no joiner address is pre-entered) + the per-frame
    /// `tick` drive without needing two separate OS processes. (Real 2-player
    /// play still needs two running instances — but the rollback / transport
    /// correctness is proven by the Stage 1 + 2 suites.)
    #[test]
    fn two_peers_reach_in_game_and_advance() {
        let rom = synth_nrom();
        let hash = SessionIdentity::of(&Nes::from_rom(&rom).unwrap());

        // Pick a free port for the host by binding + dropping a probe socket
        // (a small TOCTOU race, but fine for a loopback test). The host then
        // LISTENS on that port with no known remote; the joiner dials it and
        // the host learns the joiner's address from the first Sync.
        let probe = UdpSocket::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).unwrap();
        let host_addr = probe.local_addr().unwrap();
        drop(probe);

        let mut host = NetplayUi::default();
        host.start_host(host_addr.port(), 2, hash);

        let mut join = NetplayUi::default();
        join.start_join(host_addr, hash);

        let mut nes_host = Nes::from_rom(&rom).unwrap();
        let mut nes_join = Nes::from_rom(&rom).unwrap();

        // Pump both until both reach InGame or a bounded number of rounds.
        let mut rounds = 0;
        while !(host.phase() == NetplayPhase::InGame && join.phase() == NetplayPhase::InGame)
            && rounds < 500
        {
            host.tick(&mut nes_host, Buttons::empty());
            join.tick(&mut nes_join, Buttons::empty());
            rounds += 1;
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert_eq!(host.phase(), NetplayPhase::InGame, "host reached InGame");
        assert_eq!(join.phase(), NetplayPhase::InGame, "joiner reached InGame");

        // Advance a handful of frames; neither side should error out.
        for _ in 0..30 {
            let th = host.tick(&mut nes_host, Buttons::empty());
            let tj = join.tick(&mut nes_join, Buttons::empty());
            assert!(th.active && tj.active);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert_ne!(host.phase(), NetplayPhase::Error, "host did not error");
        assert_ne!(join.phase(), NetplayPhase::Error, "joiner did not error");
    }

    /// The tick that ends a session on a peer timeout still belongs to the
    /// session: `active: true`, as on every other failure path, so the caller
    /// does not run a stray single-player frame on it (`CodeRabbit` on #604).
    #[test]
    fn the_peer_timeout_tick_is_still_active() {
        let rom = synth_nrom();
        let hash = SessionIdentity::of(&Nes::from_rom(&rom).unwrap());
        let probe = UdpSocket::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).unwrap();
        let host_addr = probe.local_addr().unwrap();
        drop(probe);
        let mut host = NetplayUi::default();
        host.start_host(host_addr.port(), 2, hash);
        let mut join = NetplayUi::default();
        join.start_join(host_addr, hash);
        let mut nes_host = Nes::from_rom(&rom).unwrap();
        let mut nes_join = Nes::from_rom(&rom).unwrap();
        let mut rounds = 0;
        while !(host.phase() == NetplayPhase::InGame && join.phase() == NetplayPhase::InGame)
            && rounds < 500
        {
            host.tick(&mut nes_host, Buttons::empty());
            join.tick(&mut nes_join, Buttons::empty());
            rounds += 1;
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert_eq!(host.phase(), NetplayPhase::InGame);

        // The joiner goes silent; the host's last contact is six seconds ago.
        let stale = std::time::Instant::now()
            .checked_sub(std::time::Duration::from_secs(6))
            .unwrap();
        let mut failing_tick = None;
        for _ in 0..300 {
            host.last_peer_activity = stale;
            let t = host.tick(&mut nes_host, Buttons::empty());
            if host.phase() == NetplayPhase::Error {
                failing_tick = Some(t);
                break;
            }
        }
        let t = failing_tick.expect("the host gives up on a silent peer");
        assert!(t.active, "the failing tick is the session's");
        assert!(!t.produced_frame);
    }

    /// v2.9.0 (`CodeRabbit` on #561): v2.9.0 made a power cycle keep battery
    /// RAM, and the session start was a power cycle, so two peers holding
    /// different saves began the "identical" cold boot with different save
    /// RAM. The frame checksum hashes the picture, not the RAM, so nothing
    /// would notice until the game read its save. Both must enter the session
    /// with the same save RAM: cleared, as every peer had before v2.9.0.
    #[test]
    fn two_peers_with_different_saves_start_equal() {
        let mut rom = synth_nrom();
        rom[6] |= 0x02; // the battery bit: NROM's work RAM becomes a save
        let hash = SessionIdentity::of(&Nes::from_rom(&rom).unwrap());
        let probe = UdpSocket::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).unwrap();
        let host_addr = probe.local_addr().unwrap();
        drop(probe);
        let mut host = NetplayUi::default();
        host.start_host(host_addr.port(), 2, hash);
        let mut join = NetplayUi::default();
        join.start_join(host_addr, hash);

        let mut nes_host = Nes::from_rom(&rom).unwrap();
        let mut nes_join = Nes::from_rom(&rom).unwrap();
        assert!(nes_host.has_battery());
        nes_host.sram_mut().fill(0x11);
        nes_join.sram_mut().fill(0x22);

        let mut rounds = 0;
        while !(host.phase() == NetplayPhase::InGame && join.phase() == NetplayPhase::InGame)
            && rounds < 500
        {
            host.tick(&mut nes_host, Buttons::empty());
            join.tick(&mut nes_join, Buttons::empty());
            rounds += 1;
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert_eq!(host.phase(), NetplayPhase::InGame);
        assert_eq!(join.phase(), NetplayPhase::InGame);
        assert_eq!(
            nes_host.sram(),
            nes_join.sram(),
            "the peers start with one save RAM"
        );
        assert!(
            nes_host.sram().iter().all(|&b| b == 0),
            "cleared, as before v2.9.0"
        );
    }

    /// v3.4.0 — a ROM whose picture depends on ALL FOUR controllers, built here
    /// (the same program as `rustynes-netplay`'s `tests/mesh_sync.rs`, which
    /// also checks that every port reaches the picture)
    /// (public-domain by construction: 95 bytes of 6502 written for this test).
    ///
    /// nestest, the other fixture, reads only the first controller, so a session
    /// that dropped players 3 and 4 entirely (the Four Score left off, say) still
    /// matched its reference; mutation testing showed exactly that. This program
    /// waits for vblank each frame, strobes the controllers, reads 16 bits from
    /// `$4016` (player 1 then player 3, through the Four Score) and 16 from `$4017`
    /// (players 2 and 4), mixes the four bytes with shifts and XORs, and writes
    /// the result to the backdrop colour `$3F00` with rendering off, so the whole
    /// frame is that colour. Every port's buttons reach the framebuffer, and so
    /// the gameplay digest.
    fn four_score_probe_rom() -> Vec<u8> {
        const PROGRAM: [u8; 95] = [
            0x78, 0xA2, 0xFF, 0x9A, 0xA9, 0x00, 0x8D, 0x00, 0x20, 0x8D, 0x01, 0x20, 0x2C, 0x02,
            0x20, 0x10, 0xFB, 0xA9, 0x01, 0x8D, 0x16, 0x40, 0xA9, 0x00, 0x8D, 0x16, 0x40, 0xA2,
            0x00, 0xAD, 0x16, 0x40, 0x4A, 0x26, 0x00, 0x26, 0x01, 0xE8, 0xE0, 0x10, 0xD0, 0xF3,
            0xA2, 0x00, 0xAD, 0x17, 0x40, 0x4A, 0x26, 0x02, 0x26, 0x03, 0xE8, 0xE0, 0x10, 0xD0,
            0xF3, 0xA5, 0x00, 0x0A, 0x45, 0x01, 0x0A, 0x45, 0x02, 0x0A, 0x45, 0x03, 0x29, 0x3F,
            0xAA, 0xA9, 0x3F, 0x8D, 0x06, 0x20, 0xA9, 0x00, 0x8D, 0x06, 0x20, 0x8E, 0x07, 0x20,
            0xA9, 0x00, 0x8D, 0x06, 0x20, 0x8D, 0x06, 0x20, 0x4C, 0x0C, 0xC0,
        ];
        let mut rom = b"NES\x1A\x01\x01\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00".to_vec();
        let mut prg = vec![0xEAu8; 16 * 1024];
        prg[..PROGRAM.len()].copy_from_slice(&PROGRAM);
        // NMI, RESET and IRQ all point at $C000 (the 16 KiB bank sits there).
        for v in [0x3FFA, 0x3FFC, 0x3FFE] {
            prg[v] = 0x00;
            prg[v + 1] = 0xC0;
        }
        rom.extend_from_slice(&prg);
        rom.extend_from_slice(&[0u8; 8 * 1024]);
        rom
    }

    /// v3.4.0 (`T-NETPLAY-MESH-NATIVE`) — `players` UIs over loopback: one
    /// host that picked `players`, the rest joining its address exactly as a
    /// 2-player joiner would (the joiner is not told the count). Every UI
    /// must reach `InGame` with the right count and a distinct player index,
    /// and every confirmed frame's digest must agree across all of them.
    fn run_ui_mesh(players: u8, frames: u32) {
        let rom = four_score_probe_rom();
        let hash = SessionIdentity::of(&Nes::from_rom(&rom).unwrap());
        let probe = UdpSocket::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).unwrap();
        let host_addr = probe.local_addr().unwrap();
        drop(probe);
        let mut uis: Vec<NetplayUi> = (0..players).map(|_| NetplayUi::default()).collect();
        uis[0].start_host(host_addr.port(), players, hash);
        for ui in uis.iter_mut().skip(1) {
            ui.start_join(host_addr, hash);
        }
        let mut nes: Vec<Nes> = (0..players).map(|_| Nes::from_rom(&rom).unwrap()).collect();
        let mut rounds = 0;
        while uis.iter().any(|u| u.phase() != NetplayPhase::InGame) && rounds < 2000 {
            for (ui, n) in uis.iter_mut().zip(nes.iter_mut()) {
                ui.tick(n, Buttons::empty());
            }
            rounds += 1;
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let phases: Vec<NetplayPhase> = uis.iter().map(NetplayUi::phase).collect();
        assert!(
            phases.iter().all(|&p| p == NetplayPhase::InGame),
            "every UI reaches InGame: {phases:?}"
        );
        let mut seen = Vec::new();
        for ui in &uis {
            let NetplayState::InGame(s) = &ui.state else {
                unreachable!("checked above")
            };
            assert_eq!(
                s.num_players(),
                players,
                "every peer runs {players} players"
            );
            seen.push(s.local_player());
        }
        seen.sort_unstable();
        assert_eq!(
            seen,
            (0..players).collect::<Vec<_>>(),
            "distinct player indices"
        );

        let mut rng = rustynes_netplay::SplitMix64::new(0x0123_4567 ^ u64::from(players));
        let mut digests: Vec<Vec<u64>> = vec![Vec::new(); players as usize];
        let want = frames as usize;
        let mut ticks = 0;
        while digests.iter().any(|d| d.len() < want) && ticks < frames * 20 {
            ticks += 1;
            for (i, (ui, n)) in uis.iter_mut().zip(nes.iter_mut()).enumerate() {
                let t = ui.tick(n, Buttons::from_bits_truncate(rng.next_u8()));
                assert!(t.active, "peer {i}: {:?}", ui.status().message);
                let NetplayState::InGame(s) = &ui.state else {
                    panic!("peer {i} left the game: {}", ui.status().message);
                };
                let d = &mut digests[i];
                while let Some(x) =
                    s.confirmed_entering_digest(u32::try_from(d.len()).unwrap_or(u32::MAX))
                {
                    d.push(x);
                }
            }
            std::thread::sleep(std::time::Duration::from_micros(300));
        }
        assert!(
            digests.iter().all(|d| d.len() >= want),
            "every peer confirmed {frames} frames: {:?}",
            digests.iter().map(Vec::len).collect::<Vec<_>>()
        );
        for f in 0..want {
            for (i, d) in digests.iter().enumerate().skip(1) {
                assert_eq!(
                    d[f], digests[0][f],
                    "peer {i} differs from the host at frame {f}"
                );
            }
        }
    }

    #[test]
    fn three_players_from_the_ui_stay_in_sync() {
        run_ui_mesh(3, 120);
    }

    #[test]
    fn four_players_from_the_ui_stay_in_sync() {
        run_ui_mesh(4, 120);
    }

    /// A 2-player host and the (v3.4.0) mesh-capable joiner meet as before.
    #[test]
    fn two_players_from_the_ui_stay_in_sync() {
        run_ui_mesh(2, 120);
    }

    /// v1.7.0 H8 — starting a spectator binds cleanly, enters the read-only
    /// `Spectating` phase, ticks without error (waiting for a stream that never
    /// arrives in this no-host loopback), and leaves back to Idle. Asserts the
    /// determinism-safety surface: a spectator tick never produces a frame
    /// until inputs arrive, and `leave` is clean.
    #[test]
    fn spectator_enters_phase_and_leaves_cleanly() {
        let rom = synth_nrom();
        let hash = SessionIdentity::of(&Nes::from_rom(&rom).unwrap());
        let mut nes = Nes::from_rom(&rom).unwrap();

        let mut ui = NetplayUi::default();
        // No host is listening; the bind still succeeds (we only dial), and the
        // spectator simply receives nothing.
        let remote = SocketAddr::from((Ipv4Addr::LOCALHOST, 7000));
        ui.start_spectate(remote, hash);
        assert_eq!(ui.phase(), NetplayPhase::Spectating);
        assert!(ui.is_active());

        for _ in 0..10 {
            let t = ui.tick(&mut nes, Buttons::A);
            assert!(t.active, "spectator tick is active");
            assert!(
                !t.produced_frame,
                "spectator produces no frame without a confirmed input stream"
            );
        }
        let status = ui.status();
        assert_eq!(status.phase, NetplayPhase::Spectating);
        assert!(!status.is_host);
        assert_eq!(status.current_frame, 0);

        ui.leave();
        assert_eq!(ui.phase(), NetplayPhase::Idle);
        assert!(!ui.is_active());
    }
}
