#![allow(
    clippy::missing_const_for_fn,
    clippy::too_many_lines,
    clippy::assigning_clones
)]
//! v2.7.0: the **wasm-only** browser netplay lobby — a minimal egui overlay
//! that drives [`crate::wasm_netplay::BrowserNetplay`].
//!
//! A browser cannot open a UDP socket, so the native netplay panel
//! (`debugger/netplay_panel.rs`) is a "native-only" note on wasm. This module is
//! its browser counterpart: a small command + view surface for the WebRTC path.
//! The user fills in the signaling-server URL, a room/lobby code, picks Host or
//! Join + the player count, and clicks Connect; the lobby emits a
//! [`LobbyRequest`] the `App` drains each rAF frame and acts on by driving the
//! `BrowserNetplay` handshake.
//!
//! It is intentionally **bounded** — a functional lobby, not a polished
//! multi-screen UI. A full end-to-end browser session needs the signaling server
//! deployed (`deploy/`) plus two real browsers/tabs, which cannot be verified
//! headlessly (see `docs/netplay-webrtc.md`).
//!
//! (Module-gated to `wasm32` at its `pub mod` declaration in `lib.rs`.)

use crate::wasm_netplay::BrowserNetplayPhase;

/// A request the lobby emits for the `App` to act on (drained each frame via
/// [`WasmLobbyState::take_request`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LobbyRequest {
    /// Connect to the signaling server + join the room, then run the WebRTC
    /// handshake. `host` selects the offerer role hint (the server still assigns
    /// the authoritative slot); `num_players` is 2..=4.
    Connect {
        /// The `wss://...` (or `ws://...`) signaling-server URL.
        signaling_url: String,
        /// The room / lobby code both peers share.
        room: String,
        /// `true` to host (P1), `false` to join (P2).
        host: bool,
        /// Player count (2..=4); 3-4 use the Four Score adapter.
        num_players: u8,
    },
    /// Leave the current session / abort the handshake (back to single-player).
    Leave,
}

/// Persistent state of the wasm lobby: the editable fields, a pending request,
/// and the latest phase/status snapshot the `App` pushes in.
#[derive(Debug)]
pub struct WasmLobbyState {
    /// Whether the lobby window is open.
    pub open: bool,
    /// Signaling-server URL buffer (seeded from config once).
    signaling_url: String,
    /// Room / lobby code buffer.
    room: String,
    /// `true` = host (P1), `false` = join (P2).
    host: bool,
    /// Player count (2..=4).
    num_players: u8,
    /// `true` once the fields have been seeded from config (so later seeds don't
    /// clobber the user's edits).
    seeded: bool,
    /// Pending request, drained by the `App`.
    request: Option<LobbyRequest>,
    /// Latest phase pushed by the `App` (drives the status line + button state).
    phase: BrowserNetplayPhase,
    /// Latest status message pushed by the `App`.
    message: String,
}

impl Default for WasmLobbyState {
    fn default() -> Self {
        Self {
            open: false,
            signaling_url: String::new(),
            room: String::new(),
            host: true,
            num_players: 2,
            seeded: false,
            request: None,
            phase: BrowserNetplayPhase::Idle,
            message: String::new(),
        }
    }
}

impl WasmLobbyState {
    /// Drain the pending connect/leave request, if any.
    pub fn take_request(&mut self) -> Option<LobbyRequest> {
        self.request.take()
    }

    /// Push the latest browser-netplay phase + status message for the lobby to
    /// render (called by the `App` each frame, mirroring the native
    /// `set_netplay_status`).
    pub fn set_status(&mut self, phase: BrowserNetplayPhase, message: String) {
        self.phase = phase;
        self.message = message;
    }

    /// Seed the editable fields from config once (signaling URL + player count).
    fn seed(&mut self, signaling_url: &str, num_players: u8) {
        if self.seeded {
            return;
        }
        self.signaling_url = signaling_url.to_string();
        self.num_players = num_players.clamp(2, 4);
        self.seeded = true;
    }
}

/// Render the lobby window.
///
/// On a Connect/Leave click a [`LobbyRequest`] is queued for the `App` to act
/// on. The host/join controls are disabled while a session is connecting /
/// in-game; Leave is enabled then.
pub fn show(ctx: &egui::Context, state: &mut WasmLobbyState, config: &mut crate::config::Config) {
    state.seed(&config.netplay.signaling_url, config.netplay.num_players);

    let mut open = state.open;
    egui::Window::new("Netplay (browser)")
        .open(&mut open)
        .default_pos([600.0, 96.0])
        .default_size([400.0, 320.0])
        .resizable(true)
        .show(ctx, |ui| {
            body(ui, state, config);
        });
    state.open = open;
}

fn body(ui: &mut egui::Ui, state: &mut WasmLobbyState, config: &mut crate::config::Config) {
    use BrowserNetplayPhase::{Connecting, Error, Idle, InGame};

    let active = !matches!(state.phase, Idle);

    // --- Status block ---
    ui.label(egui::RichText::new("Status").strong());
    match state.phase {
        Idle => {
            ui.label("Single-player (not connected).");
        }
        Connecting => {
            ui.colored_label(
                egui::Color32::from_rgb(0xF0, 0xC0, 0x40),
                "Connecting (signaling + WebRTC handshake)...",
            );
            if !state.message.is_empty() {
                ui.label(egui::RichText::new(&state.message).weak());
            }
        }
        InGame => {
            let role = if state.host { "host" } else { "joiner" };
            ui.colored_label(
                egui::Color32::from_rgb(0x40, 0xC0, 0x40),
                format!("In game ({} players, joined as {role})", state.num_players),
            );
        }
        Error => {
            ui.colored_label(
                egui::Color32::from_rgb(0xE0, 0x40, 0x40),
                format!("Error: {}", state.message),
            );
        }
    }

    ui.separator();

    // --- Connection setup (disabled while active) ---
    ui.add_enabled_ui(!active, |ui| {
        ui.label(egui::RichText::new("Signaling server").strong());
        ui.horizontal(|ui| {
            ui.label("URL:");
            ui.add(
                egui::TextEdit::singleline(&mut state.signaling_url)
                    .hint_text("wss://signal.example.com")
                    .desired_width(240.0),
            );
        });

        ui.horizontal(|ui| {
            ui.label("room:");
            ui.add(
                egui::TextEdit::singleline(&mut state.room)
                    .hint_text("lobby code")
                    .desired_width(160.0),
            );
        });

        ui.horizontal(|ui| {
            ui.label("role:");
            ui.selectable_value(&mut state.host, true, "Host (P1)");
            ui.selectable_value(&mut state.host, false, "Join (P2)");
        });

        if state.num_players < 2 {
            state.num_players = 2;
        }
        ui.horizontal(|ui| {
            ui.label("players:");
            for n in 2u8..=4 {
                ui.selectable_value(&mut state.num_players, n, n.to_string());
            }
        });
        if state.num_players > 2 {
            ui.label(
                egui::RichText::new(
                    "3-4 players use the Four Score adapter and form a full WebRTC \
                     mesh (every peer connected to every other). All players must \
                     share the room code; each gets the next free slot.",
                )
                .weak(),
            );
        }

        if ui.button("Connect").clicked() {
            let signaling_url = state.signaling_url.trim().to_string();
            let room = state.room.trim().to_string();
            if signaling_url.is_empty() {
                state.message = "enter a signaling-server URL first".to_string();
            } else if room.is_empty() {
                state.message = "enter a room code first".to_string();
            } else {
                let num_players = state.num_players.clamp(2, 4);
                // Persist the conveniences for the next launch.
                config.netplay.signaling_url = signaling_url.clone();
                config.netplay.num_players = num_players;
                state.request = Some(LobbyRequest::Connect {
                    signaling_url,
                    room,
                    host: state.host,
                    num_players,
                });
            }
        }
    });

    ui.separator();

    // --- Leave (enabled while active) ---
    ui.add_enabled_ui(active, |ui| {
        if ui.button("Leave").clicked() {
            state.request = Some(LobbyRequest::Leave);
        }
    });

    ui.separator();
    ui.label(
        egui::RichText::new(
            "Both peers must run the SAME ROM (the signaling handshake checks \
             the SHA-256) and point at the same signaling server + room code. \
             A live session needs the server deployed (see deploy/) and two \
             browsers.",
        )
        .weak(),
    );
    ui.label(
        egui::RichText::new(
            "IMPORTANT: keep every player's window VISIBLE (side-by-side, not a \
             background tab). Browsers throttle requestAnimationFrame in hidden \
             tabs, which stalls and desyncs the rollback session.",
        )
        .weak(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_is_idempotent() {
        let mut s = WasmLobbyState::default();
        s.seed("wss://a.example", 3);
        s.seed("wss://b.example", 2); // second seed must NOT clobber.
        assert_eq!(s.signaling_url, "wss://a.example");
        assert_eq!(s.num_players, 3);
    }

    #[test]
    fn take_request_drains() {
        let mut s = WasmLobbyState::default();
        assert!(s.take_request().is_none());
        s.request = Some(LobbyRequest::Leave);
        assert_eq!(s.take_request(), Some(LobbyRequest::Leave));
        assert!(s.take_request().is_none());
    }

    #[test]
    fn set_status_round_trips() {
        let mut s = WasmLobbyState::default();
        s.set_status(BrowserNetplayPhase::Error, "boom".to_string());
        assert_eq!(s.phase, BrowserNetplayPhase::Error);
        assert_eq!(s.message, "boom");
    }
}
