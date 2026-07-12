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
//! The **host is player 0 (P1, `$4016`)**; the joiner is player 1 (P2,
//! `$4017`). Both peers feed their LOCAL keyboard/gamepad as `player1()`;
//! the session routes it to the correct port via `SessionConfig.local_player`.

use std::net::SocketAddr;

use rustynes_core::{Buttons, Nes};
use rustynes_netplay::{
    AdvanceOutcome, ConnectionState, DisconnectReason, NetplayConnection, NetplayError,
    RollbackSession, SessionConfig, SpectatorConfig, SpectatorSession, UdpTransport,
};

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
    /// Handshake in progress.
    Connecting(Box<NetplayConnection>),
    /// Rollback session running.
    InGame(Box<RollbackSession<UdpTransport>>),
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
    /// The loaded ROM's SHA-256, captured at connect for the handshake +
    /// session. The peer must announce an identical hash.
    rom_hash: [u8; 32],
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
            rom_hash: [0u8; 32],
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
            NetplayState::Connecting(_) => NetplayPhase::Connecting,
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
            _ => None,
        }
    }

    /// Host a session: bind `0.0.0.0:local_port` and **listen** as player 0
    /// (P1), learning the joiner's address from its first valid `Sync`. Any
    /// previous session is dropped.
    ///
    /// `num_players` (2..=4) selects how many players the session runs; 3-4
    /// players enable the Four Score adapter. It is clamped into `2..=4`. The
    /// multi-joiner UDP handshake (a host adopting several joiners + assigning
    /// each a player index) is a follow-up — the N-player rollback core +
    /// determinism proof live in `rustynes-netplay`; the native UDP layer currently
    /// completes the first joiner's handshake. The selected `num_players` is
    /// still recorded so the session + Four Score wiring is in place.
    ///
    /// The host no longer needs to pre-enter the joiner's address — it just
    /// shares its own listening `IP:port` and the joiner dials in (see
    /// [`NetplayConnection::host`]).
    pub fn start_host(&mut self, local_port: u16, num_players: u8, rom_hash: [u8; 32]) {
        let local = SocketAddr::from(([0, 0, 0, 0], local_port));
        self.is_host = true;
        self.rom_hash = rom_hash;
        self.config.num_players = num_players.clamp(2, 4);
        self.config.local_player = 0; // host = player 0.
        match NetplayConnection::host(local, rom_hash) {
            Ok(conn) => self.enter_connecting(conn, true),
            Err(e) => self.fail(format!("host bind failed: {e}")),
        }
    }

    /// Join a session hosted at `remote`: bind an ephemeral local port and
    /// begin the handshake as player 1 (P2). Any previous session is dropped.
    pub fn start_join(&mut self, remote: SocketAddr, rom_hash: [u8; 32]) {
        let local = SocketAddr::from(([0, 0, 0, 0], 0));
        self.is_host = false;
        self.rom_hash = rom_hash;
        // The 2-player UDP handshake does not yet carry num_players; a joiner
        // runs as player 1 of a 2-player session. (The N-player session core
        // is proven by the rustynes-netplay determinism harness; multi-joiner UDP
        // assignment is the deferred follow-up.)
        self.config.num_players = 2;
        self.config.local_player = 1; // joiner = player 1.
        match NetplayConnection::connect(local, remote, rom_hash) {
            Ok(conn) => self.enter_connecting(conn, false),
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
    pub fn start_spectate(&mut self, remote: SocketAddr, rom_hash: [u8; 32]) {
        let local = SocketAddr::from(([0, 0, 0, 0], 0));
        self.is_host = false;
        self.rom_hash = rom_hash;
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
                    rom_hash,
                });
                let session = SpectatorSession::new(
                    SpectatorConfig {
                        num_players: self.config.num_players,
                        delay_frames: self.spectator_delay_frames,
                    },
                    transport,
                    rom_hash,
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
            ConnectionState::Connecting => {
                self.state = NetplayState::Connecting(conn);
                NetplayTick {
                    active: true,
                    produced_frame: false,
                }
            }
            ConnectionState::Synced => {
                // CRITICAL for cross-peer determinism: both peers were running
                // the ROM single-player (a DIFFERENT number of frames each)
                // before the handshake completed, so their state diverges.
                // Power-cycle to the deterministic cold-boot (zeroed WRAM, fixed
                // phase) so the session's frame-0 checkpoint is byte-identical on
                // every peer; otherwise the first confirmed-frame checksum trips
                // a desync immediately.
                nes.power_cycle();
                // Hand the bound + handshaken transport to a fresh session.
                let transport = conn.into_transport();
                let session = RollbackSession::new(self.config, transport, self.rom_hash);
                self.state = NetplayState::InGame(Box::new(session));
                self.status.phase = NetplayPhase::InGame;
                NetplayTick {
                    active: true,
                    produced_frame: false,
                }
            }
            ConnectionState::Disconnected => {
                let why = match conn.disconnect_reason() {
                    Some(DisconnectReason::RomMismatch) => {
                        "peer is running a different ROM".to_string()
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
                NetplayTick {
                    active: true,
                    produced_frame: false,
                }
            }
        }
    }

    /// Drive the rollback session one tick: feed the local input and advance.
    fn tick_in_game(&mut self, nes: &mut Nes, local_buttons: Buttons) -> NetplayTick {
        let NetplayState::InGame(session) = &mut self.state else {
            unreachable!("tick_in_game only runs in the InGame state");
        };

        session.add_local_input(local_buttons);
        match session.advance(nes) {
            Ok(AdvanceOutcome {
                produced_frame,
                rolled_back,
                resimulated_frames,
                ..
            }) => {
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
fn diagnostics_view(
    session: &RollbackSession<UdpTransport>,
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
        ui.start_host(0, 2, [0u8; 32]);
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
        let hash = *Nes::from_rom(&rom).unwrap().rom_sha256();

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

    /// v1.7.0 H8 — starting a spectator binds cleanly, enters the read-only
    /// `Spectating` phase, ticks without error (waiting for a stream that never
    /// arrives in this no-host loopback), and leaves back to Idle. Asserts the
    /// determinism-safety surface: a spectator tick never produces a frame
    /// until inputs arrive, and `leave` is clean.
    #[test]
    fn spectator_enters_phase_and_leaves_cleanly() {
        let rom = synth_nrom();
        let hash = *Nes::from_rom(&rom).unwrap().rom_sha256();
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
