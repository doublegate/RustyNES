#![allow(
    clippy::missing_const_for_fn,
    clippy::items_after_statements,
    clippy::too_many_lines,
    clippy::needless_pass_by_ref_mut
)]
//! Netplay host/join panel + status HUD (v2.3.0 Stage 3).
//!
//! This panel is a **command + view surface** only: it never owns the
//! [`NetplayUi`](crate::netplay_ui::NetplayUi) session (that lives in `App`,
//! driven by the produce path). Edits here emit a [`NetplayRequest`] the app
//! drains each pacer iteration (mirroring the settings panel's
//! `take_apply`), and the app pushes back a status snapshot via
//! `set_netplay_status` (mirroring `set_movie_status`) which this panel
//! renders read-only.
//!
//! # Native vs wasm32
//!
//! Netplay drives a UDP socket (`std::net`), which does not exist on
//! `wasm32-unknown-unknown`. On wasm32 the panel renders a single
//! "netplay is native-only" note and emits no requests. The host/join
//! controls + status block are native-only.
//!
//! # HUD
//!
//! Beyond the panel, the app draws a compact status line in the debugger top
//! toolbar from the same [`NetplayStatusView`] (connecting / synced ping /
//! rollback + resim / desync), so the user sees liveness without opening the
//! panel.

/// A read-only snapshot of the netplay state the app pushes into the
/// debugger each pacer iteration (mirrors `MovieStatus`). On wasm32 the
/// netplay UI is absent, so this is a plain inert struct here.
#[derive(Clone, Debug, Default)]
pub struct NetplayStatusView {
    /// `"Idle"`, `"Connecting"`, `"InGame"`, or `"Error"`.
    pub phase: NetplayPhaseView,
    /// `true` if this peer hosts (player 0 / P1).
    pub is_host: bool,
    /// Smoothed round-trip ping in ms, once measured.
    pub ping_ms: Option<u32>,
    /// Frame being produced next (`InGame`).
    pub current_frame: u32,
    /// Newest both-confirmed frame (`InGame`).
    pub confirmed_frame: Option<u32>,
    /// The most recent tick rolled back + re-simulated.
    pub rolled_back: bool,
    /// Frames re-simulated on the most recent tick.
    pub resimulated_frames: u32,
    /// The most recent tick stalled for time-sync (no frame produced).
    pub stalled: bool,
    /// Error / disconnect text (Error phase), else empty.
    pub message: String,
}

/// The coarse netplay phase, decoupled from the native-only
/// `netplay_ui::NetplayPhase` so this panel module compiles on wasm32 too.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum NetplayPhaseView {
    /// Single-player (no netplay).
    #[default]
    Idle,
    /// Handshake in progress.
    Connecting,
    /// Rollback session running.
    InGame,
    /// Terminal error.
    Error,
}

/// A request the netplay panel emits for the app to act on. Drained by the
/// app via `NetplayPanelState::take_request` each pacer iteration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NetplayRequest {
    /// Host (listen) on the given local UDP port. The joiner's address is
    /// learned from its first `Sync`, so no remote is pre-entered.
    Host {
        /// Local UDP port to bind.
        port: u16,
        /// How many players (2..=4) the host runs the session with. 3-4
        /// players use the Four Score adapter.
        num_players: u8,
    },
    /// Join a host at the given `host:port` address.
    Join {
        /// The host's `IP:port`.
        remote: String,
    },
    /// Leave the current session (back to single-player).
    Leave,
}

/// Persistent state of the netplay panel (text fields + pending request +
/// pushed status snapshot).
// The host/join text fields + `seeded` flag drive the native host/join
// controls; on wasm32 the panel is an informational note, so they are unused
// there (the struct is shared so the app's `set_status` / `take_request`
// surface stays target-agnostic).
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
#[derive(Debug, Default)]
pub struct NetplayPanelState {
    /// Host port text buffer (default seeded from config on first sync).
    host_port: String,
    /// Number of players the host runs with (2..=4; v2.5.0).
    host_num_players: u8,
    /// Join "host:port" address buffer.
    join_remote: String,
    /// `true` once the fields have been seeded from config (so we don't
    /// clobber user edits on later syncs).
    seeded: bool,
    /// Pending request, drained by the app.
    request: Option<NetplayRequest>,
    /// Latest status pushed by the app.
    status: NetplayStatusView,
}

impl NetplayPanelState {
    /// Drain the pending request (host / join / leave), if any.
    pub fn take_request(&mut self) -> Option<NetplayRequest> {
        self.request.take()
    }

    /// Push the latest netplay status for the panel + HUD to render.
    pub fn set_status(&mut self, status: NetplayStatusView) {
        self.status = status;
    }

    /// The current status snapshot (used by the app for the toolbar HUD).
    #[must_use]
    pub fn status(&self) -> &NetplayStatusView {
        &self.status
    }

    /// Seed the editable fields from config once (host port + last join addr).
    /// Native-only: the wasm32 panel has no host/join controls.
    #[cfg(not(target_arch = "wasm32"))]
    fn seed(&mut self, host_port: u16, last_join: &str, num_players: u8) {
        if self.seeded {
            return;
        }
        self.host_port = host_port.to_string();
        self.join_remote = last_join.to_string();
        self.host_num_players = num_players.clamp(2, 4);
        self.seeded = true;
    }
}

/// Render the netplay panel. Native: full host/join controls + status. On any
/// host/join/leave click a [`NetplayRequest`] is queued for the app. wasm32:
/// a "native-only" note.
#[cfg(not(target_arch = "wasm32"))]
pub fn show(
    ctx: &egui::Context,
    open: &mut bool,
    state: &mut NetplayPanelState,
    config: &mut crate::config::Config,
) {
    state.seed(
        config.netplay.host_port,
        &config.netplay.last_join_address,
        config.netplay.num_players,
    );
    egui::Window::new("Netplay")
        .open(open)
        .default_pos([600.0, 96.0])
        .default_size([400.0, 320.0])
        .resizable(true)
        .show(ctx, |ui| {
            body(ui, state, config);
        });
}

/// wasm32 variant: netplay needs `std::net`, which is absent in the browser,
/// so the panel is informational only.
#[cfg(target_arch = "wasm32")]
pub fn show(
    ctx: &egui::Context,
    open: &mut bool,
    _state: &mut NetplayPanelState,
    _config: &mut crate::config::Config,
) {
    egui::Window::new("Netplay")
        .open(open)
        .default_pos([600.0, 96.0])
        .default_size([320.0, 120.0])
        .resizable(false)
        .show(ctx, |ui| {
            ui.label(egui::RichText::new("Use the \"Netplay (browser)\" panel").strong());
            ui.label(
                "This UDP netplay panel is native-only (a browser cannot open a \
                 raw UDP socket). In the browser, use the separate \"Netplay \
                 (browser)\" panel, which runs the same rollback netcode over \
                 WebRTC via a signaling server (2-4 players).",
            );
            ui.label(
                egui::RichText::new(
                    "Tip: keep BOTH browser windows visible side-by-side — a \
                     backgrounded tab is rAF-throttled by the browser and will \
                     desync the session.",
                )
                .weak(),
            );
        });
}

#[cfg(not(target_arch = "wasm32"))]
fn body(ui: &mut egui::Ui, state: &mut NetplayPanelState, config: &mut crate::config::Config) {
    use NetplayPhaseView::{Connecting, Error, Idle, InGame};

    // --- Status block ---
    ui.label(egui::RichText::new("Status").strong());
    let st = &state.status;
    match st.phase {
        Idle => {
            ui.label("Single-player (not connected).");
        }
        Connecting => {
            ui.colored_label(
                egui::Color32::from_rgb(0xF0, 0xC0, 0x40),
                format!(
                    "Connecting as {}...",
                    if st.is_host {
                        "host (P1)"
                    } else {
                        "joiner (P2)"
                    }
                ),
            );
            if let Some(ms) = st.ping_ms {
                ui.label(format!("ping: {ms} ms"));
            }
        }
        InGame => {
            ui.colored_label(
                egui::Color32::from_rgb(0x40, 0xC0, 0x40),
                format!(
                    "In game as {}",
                    if st.is_host {
                        "host (P1)"
                    } else {
                        "joiner (P2)"
                    }
                ),
            );
            ui.label(format!(
                "ping: {}   frame: {}   confirmed: {}",
                st.ping_ms
                    .map_or_else(|| "-".to_string(), |ms| format!("{ms} ms")),
                st.current_frame,
                st.confirmed_frame
                    .map_or_else(|| "-".to_string(), |f| f.to_string()),
            ));
            let mut sync = Vec::new();
            if st.rolled_back {
                sync.push(format!("rollback x{}", st.resimulated_frames));
            }
            if st.stalled {
                sync.push("stalled (time-sync)".to_string());
            }
            if !sync.is_empty() {
                ui.label(sync.join("   "));
            }
        }
        Error => {
            ui.colored_label(
                egui::Color32::from_rgb(0xE0, 0x40, 0x40),
                format!("Error: {}", st.message),
            );
        }
    }

    ui.separator();

    let active = !matches!(st.phase, Idle);

    // --- Host ---
    ui.add_enabled_ui(!active, |ui| {
        ui.label(egui::RichText::new("Host (player 1)").strong());
        ui.horizontal(|ui| {
            ui.label("local port:");
            ui.add(egui::TextEdit::singleline(&mut state.host_port).desired_width(70.0));
        });
        if state.host_num_players < 2 {
            state.host_num_players = 2;
        }
        ui.horizontal(|ui| {
            ui.label("players:");
            for n in 2u8..=4 {
                ui.selectable_value(&mut state.host_num_players, n, n.to_string());
            }
        });
        if state.host_num_players > 2 {
            ui.label(egui::RichText::new("3-4 players use the Four Score adapter.").weak());
        }
        ui.label(
            egui::RichText::new(
                "Share your IP:port with the joiner. The host waits and \
                 learns the joiner's address from its first connect.",
            )
            .weak(),
        );
        if ui.button("Host").clicked() {
            if let Ok(port) = state.host_port.trim().parse::<u16>() {
                let num_players = state.host_num_players.clamp(2, 4);
                config.netplay.host_port = port;
                config.netplay.num_players = num_players;
                state.request = Some(NetplayRequest::Host { port, num_players });
            }
        }
    });

    ui.separator();

    // --- Join ---
    ui.add_enabled_ui(!active, |ui| {
        ui.label(egui::RichText::new("Join (player 2)").strong());
        ui.horizontal(|ui| {
            ui.label("host:port:");
            ui.add(
                egui::TextEdit::singleline(&mut state.join_remote)
                    .hint_text("ip:port")
                    .desired_width(180.0),
            );
        });
        if ui.button("Join").clicked() {
            config.netplay.last_join_address = state.join_remote.trim().to_string();
            state.request = Some(NetplayRequest::Join {
                remote: state.join_remote.trim().to_string(),
            });
        }
    });

    ui.separator();

    // --- Leave ---
    ui.add_enabled_ui(active, |ui| {
        if ui.button("Leave").clicked() {
            state.request = Some(NetplayRequest::Leave);
        }
    });

    ui.separator();
    ui.label(
        egui::RichText::new(
            "Both peers must run the SAME ROM (the handshake checks the \
             SHA-256). The host is P1, the joiner is P2; both use their own \
             player-1 controls.",
        )
        .weak(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_is_idempotent() {
        let mut s = NetplayPanelState::default();
        s.seed(7000, "1.2.3.4:7000", 3);
        s.seed(9999, "other", 2); // second seed must NOT clobber.
        assert_eq!(s.host_port, "7000");
        assert_eq!(s.join_remote, "1.2.3.4:7000");
        assert_eq!(s.host_num_players, 3);
    }

    #[test]
    fn take_request_drains() {
        let mut s = NetplayPanelState::default();
        assert!(s.take_request().is_none());
        s.request = Some(NetplayRequest::Leave);
        assert_eq!(s.take_request(), Some(NetplayRequest::Leave));
        assert!(s.take_request().is_none());
    }

    #[test]
    fn status_round_trips() {
        let mut s = NetplayPanelState::default();
        s.set_status(NetplayStatusView {
            phase: NetplayPhaseView::InGame,
            current_frame: 42,
            ..NetplayStatusView::default()
        });
        assert_eq!(s.status().current_frame, 42);
        assert_eq!(s.status().phase, NetplayPhaseView::InGame);
    }
}
