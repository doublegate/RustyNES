//! v2.5.0 Phase C: a WebRTC [`Transport`] skeleton for the browser (wasm32).
//!
//! # Why
//!
//! On native, two peers reach each other over UDP (a
//! [`UdpTransport`](crate::UdpTransport)). A browser cannot open a raw UDP
//! socket, so the wasm netplay path uses **WebRTC**: an
//! [`RtcPeerConnection`](web_sys::RtcPeerConnection) carrying an
//! [`RtcDataChannel`](web_sys::RtcDataChannel) configured **unreliable +
//! unordered** (`maxRetransmits = 0`, `ordered = false`) so it has the same
//! lossy / out-of-order delivery semantics the rollback protocol already
//! tolerates — exactly matching the UDP transport.
//!
//! [`WebRtcTransport`] implements the same [`Transport`] trait the rest of the
//! crate speaks: [`send`](Transport::send) serializes a [`NetMessage`] with
//! [`NetMessage::to_bytes`] and pushes it down the data channel;
//! [`poll`](Transport::poll) drains a queue that the channel's `onmessage`
//! callback fills. So a [`RollbackSession`](crate::session::RollbackSession)
//! drives a browser peer with **no change** to the session core — the same way
//! it drives a native UDP peer.
//!
//! # What is NOT here (documented-pending)
//!
//! A WebRTC connection needs **signaling**: the two browsers must exchange an
//! SDP offer/answer and ICE candidates through a third party (a signaling
//! server) before the peer connection forms. That server, the offer/answer
//! dance, and the wasm-frontend wiring (the frontend currently gates netplay to
//! `cfg(not(wasm32))`) are **not implemented here** — this is a compile-verified
//! structural skeleton. Full browser netplay is pending a signaling server + a
//! browser + that frontend wiring. The signaling design and the remaining steps
//! are documented in `docs/netplay-webrtc.md`.
//!
//! This whole module is `#[cfg(target_arch = "wasm32")]` (gated at its
//! declaration in `lib.rs`).

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use web_sys::{MessageEvent, RtcDataChannel};

use crate::message::NetMessage;
use crate::transport::Transport;

/// The shared inbound queue an `onmessage` callback fills and
/// [`WebRtcTransport::poll`] drains. `Rc<RefCell<…>>` because the callback (a
/// `Closure` owned by the data channel) and the transport both reference it on
/// the single browser main thread.
type Inbox = Rc<RefCell<VecDeque<NetMessage>>>;

/// A [`Transport`] over a WebRTC [`RtcDataChannel`].
///
/// Construct it from an **already-open**, unreliable+unordered data channel
/// (the signaling + peer-connection setup that yields the channel is the
/// caller's job — see the module docs and `docs/netplay-webrtc.md`).
/// [`WebRtcTransport::new`] installs the `onmessage` handler that feeds the
/// inbound queue; thereafter the [`Transport`] surface is identical to the
/// native UDP one.
pub struct WebRtcTransport {
    channel: RtcDataChannel,
    inbox: Inbox,
    /// The `onmessage` closure is kept alive for the transport's lifetime: if it
    /// were dropped, the browser would stop invoking it and inbound messages
    /// would silently stop arriving.
    _on_message: Closure<dyn FnMut(MessageEvent)>,
}

impl WebRtcTransport {
    /// Wrap an open, unreliable+unordered [`RtcDataChannel`], installing the
    /// `onmessage` handler that decodes inbound datagrams into the poll queue.
    ///
    /// The channel's binary type is set to `arraybuffer` so `onmessage` receives
    /// the bytes as a [`js_sys::ArrayBuffer`] (rather than a `Blob`, which would
    /// need an async read). Each message is decoded with
    /// [`NetMessage::from_bytes`]; anything malformed is dropped (the browser
    /// peer is as untrusted as a UDP datagram).
    #[must_use]
    pub fn new(channel: RtcDataChannel) -> Self {
        channel.set_binary_type(web_sys::RtcDataChannelType::Arraybuffer);
        let inbox: Inbox = Rc::new(RefCell::new(VecDeque::new()));
        let inbox_cb = Rc::clone(&inbox);

        // `Closure::wrap` is the *safe* wasm-bindgen pattern (no `unsafe`): it
        // boxes the Rust closure so JS can call it. Kept in `_on_message`.
        let on_message = Closure::wrap(Box::new(move |evt: MessageEvent| {
            // The data arrives as an ArrayBuffer (we set binary_type above).
            if let Ok(buf) = evt.data().dyn_into::<js_sys::ArrayBuffer>() {
                let bytes = js_sys::Uint8Array::new(&buf).to_vec();
                if let Some(msg) = NetMessage::from_bytes(&bytes) {
                    inbox_cb.borrow_mut().push_back(msg);
                }
                // A malformed payload is silently dropped — same policy as the
                // UDP transport's `from_bytes` rejection.
            }
        }) as Box<dyn FnMut(MessageEvent)>);

        channel.set_onmessage(Some(on_message.as_ref().unchecked_ref()));

        Self {
            channel,
            inbox,
            _on_message: on_message,
        }
    }

    /// The underlying data channel (e.g. to inspect `ready_state`).
    #[must_use]
    pub const fn channel(&self) -> &RtcDataChannel {
        &self.channel
    }
}

impl Transport for WebRtcTransport {
    fn send(&mut self, msg: &NetMessage) {
        // Serialize and push down the channel. A send can fail if the channel is
        // not open yet (or has closed); like the UDP transport we swallow the
        // error — the rollback protocol tolerates the loss and resends.
        let bytes = msg.to_bytes();
        let _ = self.channel.send_with_u8_array(&bytes);
    }

    fn poll(&mut self) -> Vec<NetMessage> {
        // Drain everything the `onmessage` callback has queued since last poll.
        self.inbox.borrow_mut().drain(..).collect()
    }
}

/// A [`Transport`] over a **mesh** of WebRTC data channels — the browser
/// analogue of the native [`UdpMeshTransport`](crate::UdpMeshTransport).
///
/// In a >2-player session each peer holds one open, unreliable+unordered
/// [`RtcDataChannel`] to **every other peer**. [`send`](Transport::send) fans the
/// message out to all of them (a broadcast, exactly like the rollback session's
/// own-input broadcast over UDP); [`poll`](Transport::poll) drains a single
/// shared queue that *every* channel's `onmessage` callback fills, so the session
/// sees one merged inbound stream. Each [`NetMessage::Input`] carries its
/// `player` field, so the session demultiplexes by player regardless of which
/// channel a datagram arrived on — the mesh transport needs no per-channel player
/// mapping.
///
/// Construct it from the set of already-open channels (the signaling +
/// per-peer offer/answer dance that yields them is the caller's job — see
/// `wasm_netplay`). For the 2-player case a single-element mesh behaves exactly
/// like a [`WebRtcTransport`].
pub struct WebRtcMeshTransport {
    channels: Vec<RtcDataChannel>,
    inbox: Inbox,
    /// One `onmessage` closure per channel, kept alive for the transport's
    /// lifetime (dropping any would silence that peer's inbound messages).
    _on_message: Vec<Closure<dyn FnMut(MessageEvent)>>,
}

impl WebRtcMeshTransport {
    /// Wrap a set of open, unreliable+unordered data channels — one per other
    /// peer — installing an `onmessage` handler on each that decodes inbound
    /// datagrams into one shared poll queue. Malformed payloads are dropped (each
    /// peer is as untrusted as a UDP datagram).
    #[must_use]
    pub fn new(channels: Vec<RtcDataChannel>) -> Self {
        let inbox: Inbox = Rc::new(RefCell::new(VecDeque::new()));
        let mut closures = Vec::with_capacity(channels.len());
        for channel in &channels {
            channel.set_binary_type(web_sys::RtcDataChannelType::Arraybuffer);
            let inbox_cb = Rc::clone(&inbox);
            let on_message = Closure::wrap(Box::new(move |evt: MessageEvent| {
                if let Ok(buf) = evt.data().dyn_into::<js_sys::ArrayBuffer>() {
                    let bytes = js_sys::Uint8Array::new(&buf).to_vec();
                    if let Some(msg) = NetMessage::from_bytes(&bytes) {
                        inbox_cb.borrow_mut().push_back(msg);
                    }
                }
            }) as Box<dyn FnMut(MessageEvent)>);
            channel.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
            closures.push(on_message);
        }
        Self {
            channels,
            inbox,
            _on_message: closures,
        }
    }

    /// The number of peer channels in the mesh.
    #[must_use]
    pub fn peer_count(&self) -> usize {
        self.channels.len()
    }
}

impl Transport for WebRtcMeshTransport {
    fn send(&mut self, msg: &NetMessage) {
        // Broadcast to every peer channel. A send to a not-yet-open / closed
        // channel is swallowed (the rollback protocol tolerates loss + resends).
        let bytes = msg.to_bytes();
        for channel in &self.channels {
            let _ = channel.send_with_u8_array(&bytes);
        }
    }

    fn poll(&mut self) -> Vec<NetMessage> {
        // Drain the merged queue every channel's `onmessage` fills.
        self.inbox.borrow_mut().drain(..).collect()
    }
}
