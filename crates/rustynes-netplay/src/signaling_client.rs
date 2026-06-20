//! A **blocking** signaling client (v1.8.7).
//!
//! A worker thread owns the WebSocket connection to the reference signaling
//! [`Relay`](crate::signaling::Relay) and bridges it to the orchestrator via
//! `std::sync::mpsc`.
//!
//! # Why a blocking worker, not async
//!
//! The reference signaling **server** uses tokio + tokio-tungstenite (behind the
//! non-default `signaling-server` feature). The **client** must not: it has to
//! cross-compile to `aarch64-linux-android` for the mobile bridge, where pulling
//! a tokio runtime is unwanted weight. So this mirrors the proven pattern in
//! `rustynes-cheevos`'s `http.rs`: a single worker thread does the blocking I/O
//! (here a synchronous [`tungstenite`] WebSocket instead of `ureq` HTTP), and
//! the caller polls a completion channel from its own thread (here the
//! [`NatConnect`](crate::nat_connect::NatConnect) pump). No async runtime, no
//! `tokio`.
//!
//! # Transport choice (WebSocket, not HTTP-rendezvous)
//!
//! The deployed relay is genuinely WebSocket-on-the-wire (Caddy terminates TLS
//! to `wss://`, fronting the `signaling_server` example — see
//! `docs/netplay-webrtc.md` §3.2/§3.4). To interoperate with that *same*
//! deployed relay — one relay serving both the browser SDP handshake and this
//! native raw-UDP rendezvous — the client must speak WebSocket. The synchronous
//! [`tungstenite`] crate (the same one `tokio-tungstenite` wraps) provides a
//! blocking client with `rustls`, and it cross-compiles to Android; `ureq` would
//! not interoperate with the WS relay, so it is not used here.
//!
//! # Protocol
//!
//! The wire messages are [`SignalMessage`]s, in
//! the JSON text frames the [`Relay`](crate::signaling::Relay) already speaks.
//! The client sends [`Join`](crate::signaling::SignalMessage::Join) /
//! [`PublicAddr`](crate::signaling::SignalMessage::PublicAddr) and surfaces
//! inbound [`Joined`](crate::signaling::SignalMessage::Joined) /
//! [`PeerJoined`](crate::signaling::SignalMessage::PeerJoined) /
//! `PublicAddr` / [`Error`](crate::signaling::SignalMessage::Error) over the
//! completion channel the orchestrator drains.
//!
//! Native-only and gated behind the `netplay-client` feature (it pulls
//! `tungstenite` + `rustls`); with the feature off the crate stays lean (the
//! pure session core + UDP transport).

#![cfg(all(not(target_arch = "wasm32"), feature = "netplay-client"))]

use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::thread::JoinHandle;

use tungstenite::Message;

use crate::signaling::SignalMessage;

/// An event surfaced from the signaling worker to the orchestrator.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SignalEvent {
    /// The WebSocket connected and the worker is ready to relay frames.
    Connected,
    /// A decoded inbound [`SignalMessage`] from the relay.
    Message(SignalMessage),
    /// The connection failed or closed; the `String` is a short reason. Terminal.
    Closed(String),
}

/// A blocking signaling-client handle: owns the worker thread + the two channels
/// bridging it to the caller.
#[derive(Debug)]
pub struct SignalingClient {
    out_tx: Option<Sender<SignalMessage>>,
    event_rx: Receiver<SignalEvent>,
    worker: Option<JoinHandle<()>>,
}

impl SignalingClient {
    /// Connect to `url` (e.g. `ws://host:9000` or `wss://host`) on a worker
    /// thread. Returns immediately; the caller drains [`poll`](Self::poll) for a
    /// [`SignalEvent::Connected`] (or [`SignalEvent::Closed`] on failure).
    #[must_use]
    pub fn connect(url: &str) -> Self {
        let (out_tx, out_rx) = std::sync::mpsc::channel::<SignalMessage>();
        let (event_tx, event_rx) = std::sync::mpsc::channel::<SignalEvent>();
        let url = url.to_string();

        let worker = std::thread::Builder::new()
            .name("netplay-signal".into())
            .spawn(move || worker_loop(&url, &out_rx, &event_tx))
            .expect("spawn netplay-signal worker thread");

        Self {
            out_tx: Some(out_tx),
            event_rx,
            worker: Some(worker),
        }
    }

    /// Queue a [`SignalMessage`] to send to the relay. A silent no-op if the
    /// worker has exited (the orchestrator surfaces the closure via
    /// [`poll`](Self::poll)).
    pub fn send(&self, msg: SignalMessage) {
        if let Some(tx) = &self.out_tx {
            let _ = tx.send(msg);
        }
    }

    /// Drain all pending [`SignalEvent`]s without blocking.
    pub fn poll(&self) -> Vec<SignalEvent> {
        let mut out = Vec::new();
        while let Ok(ev) = self.event_rx.try_recv() {
            out.push(ev);
        }
        out
    }
}

impl Drop for SignalingClient {
    fn drop(&mut self) {
        // Dropping the send channel signals the worker to close + exit.
        self.out_tx = None;
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

/// The worker body: connect, then pump outbound queued messages + inbound WS
/// frames until either side closes. Polls non-blockingly with a short read
/// timeout so it can interleave the outbound queue without an async runtime.
fn worker_loop(url: &str, out_rx: &Receiver<SignalMessage>, event_tx: &Sender<SignalEvent>) {
    let (mut socket, _resp) = match tungstenite::connect(url) {
        Ok(pair) => pair,
        Err(e) => {
            let _ = event_tx.send(SignalEvent::Closed(format!("connect failed: {e}")));
            return;
        }
    };
    if event_tx.send(SignalEvent::Connected).is_err() {
        return; // caller dropped already.
    }

    // A short non-blocking read so the loop can also drain the outbound queue.
    if let Some(stream) = stream_of(&socket) {
        let _ = stream.set_read_timeout(Some(std::time::Duration::from_millis(20)));
    }

    loop {
        // 1. Flush any queued outbound messages. A disconnected sender (the
        //    handle was dropped) means we should close.
        loop {
            match out_rx.try_recv() {
                Ok(msg) => {
                    if socket.send(Message::Text(msg.to_json().into())).is_err() {
                        let _ = event_tx.send(SignalEvent::Closed("send failed".into()));
                        return;
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    let _ = socket.close(None);
                    return;
                }
            }
        }

        // 2. Read one inbound frame (bounded by the read timeout above).
        match socket.read() {
            Ok(Message::Text(txt)) => {
                // Unparseable frames are dropped (never panic), as the relay does.
                if let Some(msg) = SignalMessage::parse(&txt)
                    && event_tx.send(SignalEvent::Message(msg)).is_err()
                {
                    return;
                }
            }
            Ok(Message::Close(_)) => {
                let _ = event_tx.send(SignalEvent::Closed("relay closed".into()));
                return;
            }
            Ok(Message::Ping(p)) => {
                let _ = socket.send(Message::Pong(p));
            }
            Ok(_) => {} // Binary / Pong / Frame — ignore.
            Err(tungstenite::Error::Io(e))
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                // No frame within the read window: loop back to flush outbound.
            }
            Err(e) => {
                let _ = event_tx.send(SignalEvent::Closed(format!("read error: {e}")));
                return;
            }
        }
    }
}

/// Best-effort access to the underlying TCP stream to set a read timeout. Only
/// the plain (`ws://`) maybe-TLS stream variants are handled; if the timeout
/// can't be set the loop still functions (it just blocks on `read` and relies on
/// the outbound channel disconnect to exit on drop). Returns `None` for the TLS
/// case, where tungstenite does not expose the raw stream uniformly.
fn stream_of(
    socket: &tungstenite::WebSocket<tungstenite::stream::MaybeTlsStream<std::net::TcpStream>>,
) -> Option<&std::net::TcpStream> {
    match socket.get_ref() {
        tungstenite::stream::MaybeTlsStream::Plain(s) => Some(s),
        _ => None,
    }
}
