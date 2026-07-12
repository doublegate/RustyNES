//! v2.6.0 / v2.7.1: the wasm-only browser **netplay** path (WebRTC over a
//! WebSocket signaling server), generalized in v2.7.1 from 2 players to an
//! **N-peer mesh** (up to 4).
//!
//! # Scope (read this first)
//!
//! This is a **bounded, compile-verified** transport + signaling-handshake
//! path — NOT a polished multi-screen lobby. The native frontend
//! (`crate::netplay_ui`) drives a [`RollbackSession`] over a native
//! `UdpTransport` / `UdpMeshTransport`; a browser cannot open a raw UDP socket,
//! so this path drives the SAME session core over a [`WebRtcMeshTransport`]
//! instead, with each peer-to-peer WebRTC connection brokered through a
//! WebSocket **signaling server** (the reference server is `rustynes-netplay`'s
//! `signaling_server` example — see `docs/netplay-webrtc.md`).
//!
//! **What is verified:** this module COMPILES on `wasm32-unknown-unknown` for
//! both the `wasm-winit` and `wasm-canvas` frontends, and the signaling +
//! offer/answer/ICE mesh wiring is structurally complete (the pure N-peer
//! signaling *protocol* is unit-tested in `rustynes_netplay::signaling`). A full
//! browser session is **documented-pending**: it needs the signaling server
//! deployed + N real browsers/tabs, which cannot be exercised headlessly. So
//! there is deliberately no end-to-end browser test here.
//!
//! # Flow (N players)
//!
//! 1. [`BrowserNetplay::connect`] opens a `WebSocket` to the signaling server
//!    and `join`s a room (by code) announcing the ROM hash + desired
//!    `max_players`.
//! 2. The signaling server assigns the next free **slot** (`0..max_players`) and
//!    reports each higher-slot newcomer to the already-present peers
//!    (`peer-joined { slot }`).
//! 3. For every pair of peers the **lower slot offers to the higher slot**: on a
//!    `peer-joined`, the existing peer creates an `RtcDataChannel` + offer to the
//!    newcomer; the newcomer answers each inbound offer. Offers / answers / ICE
//!    candidates are routed peer-to-peer by `{ from, to }` slot through the relay.
//! 4. When **all `max_players - 1`** data channels are open they are bundled into
//!    a [`WebRtcMeshTransport`] and handed to a [`RollbackSession`]; thereafter
//!    [`BrowserNetplay::tick`] drives the session each rAF frame, exactly like
//!    the native path drives it.
//!
//! All callbacks use the **safe** `Closure::wrap` / `JsCast` web-sys patterns;
//! `#![forbid(unsafe_code)]` holds across the crate.
//!
//! (Module-gated to `wasm32` at its `pub mod` declaration in `lib.rs`.)

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use rustynes_core::{Buttons, Nes};
use rustynes_netplay::{
    AdvanceOutcome, NetplayError, RollbackSession, SessionConfig, SignalMessage,
    WebRtcMeshTransport,
};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    MessageEvent, RtcConfiguration, RtcDataChannel, RtcDataChannelInit, RtcIceCandidate,
    RtcIceCandidateInit, RtcPeerConnection, RtcPeerConnectionIceEvent, RtcSdpType,
    RtcSessionDescriptionInit, WebSocket,
};

/// `console.log` shim (no extra web-sys feature needed).
fn log(msg: &str) {
    web_sys::console::log_1(&JsValue::from_str(msg));
}

/// The coarse phase the browser netplay path is in (mirrors the native
/// `netplay_ui::NetplayPhase` shape for a shared HUD).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum BrowserNetplayPhase {
    /// No session — single-player.
    #[default]
    Idle,
    /// Connecting: WebSocket + WebRTC handshake in progress.
    Connecting,
    /// A rollback session is running over the mesh.
    InGame,
    /// Terminal error until reset.
    Error,
}

/// One peer-to-peer leg of the mesh: the `RtcPeerConnection` to a specific other
/// slot, plus its data channel once `onopen` fires.
struct PeerLeg {
    pc: RtcPeerConnection,
    channel: Option<RtcDataChannel>,
}

/// Shared connection state mutated by the (async) signaling + WebRTC callbacks
/// and read by the per-frame [`BrowserNetplay::tick`]. `Rc<RefCell<…>>` because
/// every web-sys callback runs on the single browser main thread.
#[derive(Default)]
struct Shared {
    phase: BrowserNetplayPhase,
    /// Our assigned slot (`0..max_players`). The lower slot of any pair offers.
    slot: u8,
    /// The room's total player count (2..=4), learned from `Joined`.
    max_players: u8,
    /// One leg per *other* peer, keyed by that peer's slot. Built lazily as
    /// offers/peer-joined nudges arrive.
    legs: BTreeMap<u8, PeerLeg>,
    /// STUN/TURN server URLs for each new peer connection (ICE).
    ice_servers: Vec<String>,
    /// A short status / error message for the HUD.
    message: String,
}

impl Shared {
    /// How many peer data channels are open so far.
    fn open_channel_count(&self) -> usize {
        self.legs.values().filter(|l| l.channel.is_some()).count()
    }
}

/// The browser netplay driver.
///
/// Owned by the wasm `App`, driven once per rAF frame. Holds the signaling
/// socket alive for the session's lifetime (dropping it tears signaling down);
/// the per-peer connections live in [`Shared::legs`].
pub struct BrowserNetplay {
    shared: Rc<RefCell<Shared>>,
    rom_hash: [u8; 32],
    /// The rollback session, once every data channel is open.
    session: Option<RollbackSession<WebRtcMeshTransport>>,
    config: SessionConfig,
    // Kept alive for the connection's lifetime (never read directly): dropping
    // the socket / closures would tear down signaling.
    keepalive_socket: Option<WebSocket>,
    keepalive_closures: Vec<Closure<dyn FnMut(JsValue)>>,
}

impl BrowserNetplay {
    /// A fresh, idle driver.
    #[must_use]
    pub fn new(rom_hash: [u8; 32]) -> Self {
        Self {
            shared: Rc::new(RefCell::new(Shared::default())),
            rom_hash,
            session: None,
            config: SessionConfig::default(),
            keepalive_socket: None,
            keepalive_closures: Vec::new(),
        }
    }

    /// The current phase (for the HUD).
    #[must_use]
    pub fn phase(&self) -> BrowserNetplayPhase {
        self.shared.borrow().phase
    }

    /// `true` while connecting or in-game (so the produce path drives via
    /// [`tick`](Self::tick) instead of `run_frame`).
    #[must_use]
    pub fn is_active(&self) -> bool {
        !matches!(self.phase(), BrowserNetplayPhase::Idle)
    }

    /// A copy of the latest status message.
    #[must_use]
    pub fn message(&self) -> String {
        self.shared.borrow().message.clone()
    }

    /// Set the number of players (2..=4) for the session. 3-4 players
    /// auto-enable the Four Score adapter in the session core and form a full
    /// WebRTC mesh (every peer connected to every other). Clamped into `2..=4`.
    pub fn set_num_players(&mut self, num_players: u8) {
        self.config.num_players = num_players.clamp(2, 4);
    }

    /// Begin a browser netplay session: open the WebSocket to `signaling_url`,
    /// join `room` (announcing the desired player count), and wire the N-peer
    /// WebRTC mesh handshake. The local player is the signaling slot.
    ///
    /// `ice_servers` is the STUN/TURN server URL list for each peer connection's
    /// `RtcConfiguration` (NAT traversal); pass the configured `[netplay]
    /// stun_servers`, or [`rustynes_netplay::DEFAULT_STUN_SERVERS`] for the public
    /// default. An empty list falls back to the public default.
    ///
    /// This kicks off the async handshake and returns immediately; progress is
    /// observed via [`phase`](Self::phase) and driven by [`tick`](Self::tick).
    ///
    /// # Errors
    ///
    /// Returns a `JsValue` if the WebSocket cannot be created.
    pub fn connect(
        &mut self,
        signaling_url: &str,
        room: &str,
        ice_servers: &[String],
    ) -> Result<(), JsValue> {
        {
            let mut s = self.shared.borrow_mut();
            s.phase = BrowserNetplayPhase::Connecting;
            s.message = format!("connecting to {signaling_url} (room {room})");
            s.ice_servers = ice_servers.to_vec();
            s.max_players = self.config.num_players;
        }

        // The signaling WebSocket. Each peer-to-peer RtcPeerConnection is created
        // lazily during the handshake (in `handle_signal`).
        let ws = WebSocket::new(signaling_url)?;

        // Inbound signaling: drive the N-peer offer/answer/ICE state machine.
        let shared_for_msg = Rc::clone(&self.shared);
        let ws_for_msg = ws.clone();
        let on_message = Closure::<dyn FnMut(JsValue)>::wrap(Box::new(move |ev: JsValue| {
            let ev: MessageEvent = ev.unchecked_into();
            let Some(text) = ev.data().as_string() else {
                return;
            };
            let Some(msg) = SignalMessage::parse(&text) else {
                return;
            };
            handle_signal(&ws_for_msg, &shared_for_msg, msg);
        }));
        ws.set_onmessage(Some(on_message.as_ref().unchecked_ref()));

        // On open, announce ourselves to the room with the desired player count.
        let ws_for_open = ws.clone();
        let room_owned = room.to_string();
        let rom_hex_open = hex32(&self.rom_hash);
        let max_players = self.config.num_players;
        let on_open = Closure::<dyn FnMut(JsValue)>::wrap(Box::new(move |_ev: JsValue| {
            let join = SignalMessage::Join {
                room: room_owned.clone(),
                rom_hash: rom_hex_open.clone(),
                max_players,
            };
            let _ = ws_for_open.send_with_str(&join.to_json());
        }));
        ws.set_onopen(Some(on_open.as_ref().unchecked_ref()));

        // Surface a socket error / close as a terminal netplay error.
        let shared_for_err = Rc::clone(&self.shared);
        let on_error = Closure::<dyn FnMut(JsValue)>::wrap(Box::new(move |_ev: JsValue| {
            let mut s = shared_for_err.borrow_mut();
            s.phase = BrowserNetplayPhase::Error;
            s.message = "signaling socket error".to_string();
        }));
        ws.set_onerror(Some(on_error.as_ref().unchecked_ref()));

        // Keep everything alive.
        self.keepalive_closures
            .extend([on_message, on_open, on_error]);
        self.keepalive_socket = Some(ws);
        Ok(())
    }

    /// Per-frame hook, called from the wasm produce path in place of
    /// `run_frame` while [`is_active`](Self::is_active).
    ///
    /// - **Idle**: returns `false` (the caller runs the single-player frame).
    /// - **Connecting**: promotes to a session the first frame *all*
    ///   `max_players - 1` data channels are open; otherwise produces nothing.
    /// - **`InGame`**: feeds `local_buttons`, advances the session, returns
    ///   whether a frame was produced.
    ///
    /// Returns `true` if netplay consumed this tick (so the caller must NOT also
    /// call `nes.run_frame()`).
    pub fn tick(&mut self, nes: &mut Nes, local_buttons: Buttons) -> bool {
        match self.phase() {
            BrowserNetplayPhase::Idle => false,
            BrowserNetplayPhase::Error => true,
            BrowserNetplayPhase::Connecting => {
                self.try_promote(nes);
                true
            }
            BrowserNetplayPhase::InGame => {
                if let Some(session) = self.session.as_mut() {
                    session.add_local_input(local_buttons);
                    match session.advance(nes) {
                        Ok(AdvanceOutcome { .. }) => {}
                        Err(e) => {
                            let mut s = self.shared.borrow_mut();
                            s.phase = BrowserNetplayPhase::Error;
                            s.message = describe_err(&e);
                        }
                    }
                }
                true
            }
        }
    }

    /// Once every peer leg's data channel is open, bundle them into a
    /// [`WebRtcMeshTransport`] and start the session.
    fn try_promote(&mut self, nes: &mut Nes) {
        let need = usize::from(self.config.num_players.saturating_sub(1));
        let (ready, slot) = {
            let s = self.shared.borrow();
            (s.open_channel_count() == need && need > 0, s.slot)
        };
        if !ready {
            return;
        }
        // Take the open channels in slot order (deterministic ordering is
        // irrelevant — each NetMessage carries its own player field).
        let channels: Vec<RtcDataChannel> = {
            let mut s = self.shared.borrow_mut();
            s.legs
                .values_mut()
                .filter_map(|l| l.channel.clone())
                .collect()
        };
        // CRITICAL for cross-peer determinism: both peers were running the ROM
        // single-player (for a DIFFERENT number of frames each) before the
        // handshake completed, so their current state diverges. Power-cycle to
        // the deterministic cold-boot state (zeroed WRAM, fixed phase — the same
        // byte-identical power-on the determinism contract relies on) so the
        // rollback session's frame-0 checkpoint is identical on every peer.
        // Without this, the first checksum (frame == checksum_interval) trips a
        // desync immediately.
        nes.power_cycle();
        let transport = WebRtcMeshTransport::new(channels);
        let mut cfg = self.config;
        cfg.local_player = slot;
        self.session = Some(RollbackSession::new(cfg, transport, self.rom_hash));
        self.shared.borrow_mut().phase = BrowserNetplayPhase::InGame;
        log("browser netplay: mesh complete, session started");
    }

    /// Tear the session down and return to single-player.
    pub fn leave(&mut self) {
        self.session = None;
        if let Some(ws) = self.keepalive_socket.take() {
            let _ = ws.close();
        }
        // Close every peer connection, then drop the shared state.
        {
            let s = self.shared.borrow();
            for leg in s.legs.values() {
                leg.pc.close();
            }
        }
        self.keepalive_closures.clear();
        *self.shared.borrow_mut() = Shared::default();
    }
}

/// Build an `RtcPeerConnection` with the configured STUN/TURN servers for ICE.
///
/// Each entry is a server URL string (`stun:host:port` or `turn:host:port`); an
/// empty list falls back to [`rustynes_netplay::DEFAULT_STUN_SERVERS`]. A production
/// deployment passes its own `coturn` URLs (STUN + a TURN relay for symmetric
/// NATs) via the `[netplay] stun_servers` config.
fn new_peer_connection(servers: &[String]) -> Result<RtcPeerConnection, JsValue> {
    let cfg = RtcConfiguration::new();
    let ice_servers = js_sys::Array::new();
    let default_servers: Vec<String> = rustynes_netplay::DEFAULT_STUN_SERVERS
        .iter()
        .map(|s| (*s).to_string())
        .collect();
    let urls: &[String] = if servers.is_empty() {
        &default_servers
    } else {
        servers
    };
    for url in urls {
        let server = js_sys::Object::new();
        js_sys::Reflect::set(&server, &JsValue::from_str("urls"), &JsValue::from_str(url))?;
        ice_servers.push(&server);
    }
    cfg.set_ice_servers(&ice_servers);
    RtcPeerConnection::new_with_configuration(&cfg)
}

/// Drive the N-peer WebRTC mesh handshake in response to one inbound signaling
/// message.
///
/// - `Joined { slot, max_players }`: record our slot + room size.
/// - `PeerJoined { slot }`: a higher-slot peer joined — we offer to it (lower
///   slot offers to higher).
/// - `Offer { from, .. }`: a lower-slot peer offered to us — we answer.
/// - `Answer { from, .. }` / `Candidate { from, .. }`: feed the leg to peer
///   `from`.
fn handle_signal(ws: &WebSocket, shared: &Rc<RefCell<Shared>>, msg: SignalMessage) {
    match msg {
        SignalMessage::Joined { slot, max_players } => {
            let mut s = shared.borrow_mut();
            s.slot = slot;
            s.max_players = max_players;
        }
        SignalMessage::PeerJoined { slot: peer } => {
            // We are the existing (lower) peer: create the leg + offer to `peer`.
            offer_to_peer(ws, shared, peer);
        }
        SignalMessage::Offer {
            from: peer, sdp, ..
        } => {
            // A lower-slot peer offered to us: create the leg + answer.
            answer_to_peer(ws, shared, peer, sdp);
        }
        SignalMessage::Answer {
            from: peer, sdp, ..
        } => {
            if let Some(pc) = leg_pc(shared, peer) {
                spawn_set_remote_answer(pc, sdp);
            }
        }
        SignalMessage::Candidate {
            from: peer,
            candidate,
            sdp_mid,
            sdp_m_line_index,
            ..
        } => {
            if let Some(pc) = leg_pc(shared, peer) {
                add_ice_candidate(&pc, &candidate, &sdp_mid, sdp_m_line_index);
            }
        }
        SignalMessage::PeerLeft { slot } => {
            let mut s = shared.borrow_mut();
            s.phase = BrowserNetplayPhase::Error;
            s.message = format!("peer {slot} left");
        }
        SignalMessage::Error { reason } => {
            let mut s = shared.borrow_mut();
            s.phase = BrowserNetplayPhase::Error;
            s.message = reason;
        }
        // v2.2.0 "Capstone": a `QuickMatch` reply. Identical to `Joined` for the
        // WebRTC pairing that follows (record our slot + room size); the extra
        // `room` code is surfaced so the matchmade user can see / share it.
        SignalMessage::Matched {
            room,
            slot,
            max_players,
        } => {
            let mut s = shared.borrow_mut();
            s.slot = slot;
            s.max_players = max_players;
            s.message = format!("matched into room {room}");
        }
        // v2.2.0 "Capstone": the lobby directory reply. The browser lobby-browse
        // UI is not wired yet, so record the open-room count for display; the
        // codes are reachable via a subsequent `Join`.
        SignalMessage::RoomList { rooms } => {
            let mut s = shared.borrow_mut();
            s.message = format!("{} open room(s)", rooms.len());
        }
        // Client->server message types are never inbound to a client:
        // `Join` / `ListRooms` / `QuickMatch` (requests we send), and
        // `PublicAddr` (the native-UDP mobile rendezvous; the browser SDP/ICE
        // path does not use it).
        SignalMessage::Join { .. }
        | SignalMessage::ListRooms { .. }
        | SignalMessage::QuickMatch { .. }
        | SignalMessage::PublicAddr { .. } => {}
    }
}

/// The `RtcPeerConnection` for the leg to `peer`, if it exists.
fn leg_pc(shared: &Rc<RefCell<Shared>>, peer: u8) -> Option<RtcPeerConnection> {
    shared.borrow().legs.get(&peer).map(|l| l.pc.clone())
}

/// Create (or reuse) the peer connection to `peer`, wiring its ICE trickle to
/// the signaling relay with the `{ from: our_slot, to: peer }` routing.
fn ensure_leg(ws: &WebSocket, shared: &Rc<RefCell<Shared>>, peer: u8) -> Option<RtcPeerConnection> {
    if let Some(pc) = leg_pc(shared, peer) {
        return Some(pc);
    }
    let (ice_servers, our_slot) = {
        let s = shared.borrow();
        (s.ice_servers.clone(), s.slot)
    };
    let Ok(pc) = new_peer_connection(&ice_servers) else {
        let mut s = shared.borrow_mut();
        s.phase = BrowserNetplayPhase::Error;
        s.message = "failed to create peer connection".to_string();
        return None;
    };

    // Trickle our ICE candidates to `peer` through the relay.
    let ws_ice = ws.clone();
    let on_ice = Closure::<dyn FnMut(JsValue)>::wrap(Box::new(move |ev: JsValue| {
        let ev: RtcPeerConnectionIceEvent = ev.unchecked_into();
        if let Some(cand) = ev.candidate() {
            let msg = SignalMessage::Candidate {
                from: our_slot,
                to: peer,
                candidate: cand.candidate(),
                sdp_mid: cand.sdp_mid().unwrap_or_default(),
                sdp_m_line_index: u32::from(cand.sdp_m_line_index().unwrap_or(0)),
            };
            let _ = ws_ice.send_with_str(&msg.to_json());
        }
    }));
    pc.set_onicecandidate(Some(on_ice.as_ref().unchecked_ref()));
    // Session-lifetime closure: leak it (consistent with the existing skeleton).
    on_ice.forget();

    shared.borrow_mut().legs.insert(
        peer,
        PeerLeg {
            pc: pc.clone(),
            channel: None,
        },
    );
    Some(pc)
}

/// Lower-slot side of a pair: create a data channel to `peer` + send an offer.
fn offer_to_peer(ws: &WebSocket, shared: &Rc<RefCell<Shared>>, peer: u8) {
    let Some(pc) = ensure_leg(ws, shared, peer) else {
        return;
    };
    let channel = create_unreliable_channel(&pc);
    wire_channel_open(&channel, shared, peer);
    let our_slot = shared.borrow().slot;
    spawn_offer(pc, ws.clone(), our_slot, peer);
}

/// Higher-slot side of a pair: adopt the remote data channel + send an answer.
fn answer_to_peer(ws: &WebSocket, shared: &Rc<RefCell<Shared>>, peer: u8, offer_sdp: String) {
    let Some(pc) = ensure_leg(ws, shared, peer) else {
        return;
    };
    wire_ondatachannel(&pc, shared, peer);
    let our_slot = shared.borrow().slot;
    spawn_answer(pc, ws.clone(), our_slot, peer, offer_sdp);
}

/// Create an unreliable + unordered data channel (matching UDP semantics the
/// rollback protocol tolerates).
fn create_unreliable_channel(pc: &RtcPeerConnection) -> RtcDataChannel {
    let init = RtcDataChannelInit::new();
    init.set_ordered(false);
    init.set_max_retransmits(0);
    pc.create_data_channel_with_data_channel_dict("rustynes-netplay", &init)
}

/// Install the `onopen` handler that stashes the open channel into the leg for
/// `peer`, so the next [`BrowserNetplay::try_promote`] can bundle it.
fn wire_channel_open(channel: &RtcDataChannel, shared: &Rc<RefCell<Shared>>, peer: u8) {
    channel.set_binary_type(web_sys::RtcDataChannelType::Arraybuffer);
    let shared = Rc::clone(shared);
    let chan = channel.clone();
    let on_open = Closure::<dyn FnMut(JsValue)>::wrap(Box::new(move |_ev: JsValue| {
        if let Some(leg) = shared.borrow_mut().legs.get_mut(&peer) {
            leg.channel = Some(chan.clone());
        }
    }));
    channel.set_onopen(Some(on_open.as_ref().unchecked_ref()));
    on_open.forget();
}

/// The answerer's `ondatachannel`: the remote peer created the channel, so we
/// adopt it for the leg to `peer` when it arrives.
fn wire_ondatachannel(pc: &RtcPeerConnection, shared: &Rc<RefCell<Shared>>, peer: u8) {
    let shared = Rc::clone(shared);
    let on_dc = Closure::<dyn FnMut(JsValue)>::wrap(Box::new(move |ev: JsValue| {
        let ev: web_sys::RtcDataChannelEvent = ev.unchecked_into();
        let channel = ev.channel();
        wire_channel_open(&channel, &shared, peer);
    }));
    pc.set_ondatachannel(Some(on_dc.as_ref().unchecked_ref()));
    on_dc.forget();
}

/// Lower slot: `createOffer` -> `setLocalDescription` -> send the offer SDP to
/// `peer` (with `{ from: our_slot, to: peer }` routing).
fn spawn_offer(pc: RtcPeerConnection, ws: WebSocket, our_slot: u8, peer: u8) {
    wasm_bindgen_futures::spawn_local(async move {
        let Ok(offer) = JsFuture::from(pc.create_offer()).await else {
            return;
        };
        let sdp = sdp_of(&offer);
        let desc = RtcSessionDescriptionInit::new(RtcSdpType::Offer);
        desc.set_sdp(&sdp);
        if JsFuture::from(pc.set_local_description(&desc))
            .await
            .is_err()
        {
            return;
        }
        let _ = ws.send_with_str(
            &SignalMessage::Offer {
                from: our_slot,
                to: peer,
                sdp,
            }
            .to_json(),
        );
    });
}

/// Higher slot: on an inbound offer from `peer`, `setRemoteDescription` ->
/// `createAnswer` -> `setLocalDescription` -> send the answer SDP back.
fn spawn_answer(pc: RtcPeerConnection, ws: WebSocket, our_slot: u8, peer: u8, offer_sdp: String) {
    wasm_bindgen_futures::spawn_local(async move {
        let remote = RtcSessionDescriptionInit::new(RtcSdpType::Offer);
        remote.set_sdp(&offer_sdp);
        if JsFuture::from(pc.set_remote_description(&remote))
            .await
            .is_err()
        {
            return;
        }
        let Ok(answer) = JsFuture::from(pc.create_answer()).await else {
            return;
        };
        let sdp = sdp_of(&answer);
        let desc = RtcSessionDescriptionInit::new(RtcSdpType::Answer);
        desc.set_sdp(&sdp);
        if JsFuture::from(pc.set_local_description(&desc))
            .await
            .is_err()
        {
            return;
        }
        let _ = ws.send_with_str(
            &SignalMessage::Answer {
                from: our_slot,
                to: peer,
                sdp,
            }
            .to_json(),
        );
    });
}

/// Lower slot: on the inbound answer, `setRemoteDescription`.
fn spawn_set_remote_answer(pc: RtcPeerConnection, answer_sdp: String) {
    wasm_bindgen_futures::spawn_local(async move {
        let remote = RtcSessionDescriptionInit::new(RtcSdpType::Answer);
        remote.set_sdp(&answer_sdp);
        let _ = JsFuture::from(pc.set_remote_description(&remote)).await;
    });
}

/// Feed one trickled ICE candidate to the peer connection.
fn add_ice_candidate(pc: &RtcPeerConnection, candidate: &str, sdp_mid: &str, m_line: u32) {
    let init = RtcIceCandidateInit::new(candidate);
    if !sdp_mid.is_empty() {
        init.set_sdp_mid(Some(sdp_mid));
    }
    init.set_sdp_m_line_index(Some(u16::try_from(m_line).unwrap_or(0)));
    if let Ok(cand) = RtcIceCandidate::new(&init) {
        let promise = pc.add_ice_candidate_with_opt_rtc_ice_candidate(Some(&cand));
        wasm_bindgen_futures::spawn_local(async move {
            let _ = JsFuture::from(promise).await;
        });
    }
}

/// Pull the `sdp` string out of an `RtcSessionDescription`-shaped `JsValue`.
fn sdp_of(desc: &JsValue) -> String {
    js_sys::Reflect::get(desc, &JsValue::from_str("sdp"))
        .ok()
        .and_then(|v| v.as_string())
        .unwrap_or_default()
}

/// Lowercase-hex a 32-byte ROM hash for the signaling `rom_hash` field.
fn hex32(bytes: &[u8; 32]) -> String {
    use std::fmt::Write as _;
    let mut s = String::with_capacity(64);
    for b in bytes {
        let _ = write!(s, "{b:02x}");
    }
    s
}

/// Map a [`NetplayError`] to a HUD message.
fn describe_err(e: &NetplayError) -> String {
    match e {
        NetplayError::Desync {
            frame,
            same_framebuffer,
            ..
        } => format!(
            "desync at frame {frame} ({})",
            if *same_framebuffer {
                "timing/cycle — same picture"
            } else {
                "state — picture differs"
            }
        ),
        NetplayError::RomMismatch => "rom mismatch".to_string(),
        other => format!("netplay error: {other}"),
    }
}
