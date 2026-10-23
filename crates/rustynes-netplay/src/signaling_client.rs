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

use std::net::ToSocketAddrs;
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
            join_or_detach(worker, DROP_GRACE);
        }
    }
}

/// How long `Drop` waits for the worker before detaching it (v2.7.4, audit
/// MOB-04). An idle or already-closed worker notices the closed channel within
/// one `READ_TIMEOUT` (20 ms), so this covers the normal case; only a worker
/// blocked on the network outlives it.
const DROP_GRACE: std::time::Duration = std::time::Duration::from_millis(100);

/// Join `worker` if it finishes within `grace`; otherwise let it go.
///
/// Before v2.7.4 `Drop` joined unconditionally, and a worker stuck in the
/// WebSocket handshake with a silent relay held the dropping thread for up to
/// `CONNECT_TIMEOUT` (10 s) -- on mobile, the UI thread leaving a netplay
/// screen. Detaching is safe here: the worker owns everything it touches (its
/// socket and its half of the two channels), both channels are already closed
/// from this side, so it exits by itself when the network call returns, and it
/// never calls back into anything the caller frees. Returns whether it joined.
fn join_or_detach(worker: JoinHandle<()>, grace: std::time::Duration) -> bool {
    let deadline = std::time::Instant::now() + grace;
    while !worker.is_finished() {
        if std::time::Instant::now() >= deadline {
            return false; // dropping the handle detaches the thread
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    let _ = worker.join();
    true
}

/// The worker body: connect, then pump outbound queued messages + inbound WS
/// frames until either side closes. Polls non-blockingly with a short read
/// timeout so it can interleave the outbound queue without an async runtime.
fn worker_loop(url: &str, out_rx: &Receiver<SignalMessage>, event_tx: &Sender<SignalEvent>) {
    // A *bounded* connect: `tungstenite::connect` blocks indefinitely on a TCP
    // connect to an unreachable/offline relay, so resolve the host + dial with a
    // `connect_timeout`, set read/write timeouts on the raw stream, then hand it
    // to `client_tls` (which upgrades to TLS for `wss://` and stays plain for
    // `ws://`, per the URI scheme). The read timeout is set on the underlying
    // `TcpStream` BEFORE the TLS wrap, so it survives into the `wss://` path too.
    let mut socket = match connect_bounded(url, CONNECT_TIMEOUT) {
        Ok(socket) => socket,
        Err(e) => {
            let _ = event_tx.send(SignalEvent::Closed(format!("connect failed: {e}")));
            return;
        }
    };
    if event_tx.send(SignalEvent::Connected).is_err() {
        return; // caller dropped already.
    }

    // Now the (blocking) handshake is done, set the short read timeout on the
    // stream so the loop can interleave the outbound drain. The handshake itself
    // ran with the generous connect timeout (set in connect_bounded) — a slow
    // loopback handshake (observed on macOS CI) otherwise hits the 20 ms loop
    // timeout and tungstenite aborts with `Interrupted handshake (WouldBlock)`.
    // Reaches through the TLS stream for wss.
    if let Some(stream) = stream_of(&socket) {
        let _ = stream.set_read_timeout(Some(READ_TIMEOUT));
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

/// How long to wait for the TCP connect to the relay before giving up. Bounds
/// the otherwise-indefinite block when the relay is offline/unreachable.
const CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// The read timeout that lets the worker loop interleave the outbound drain and
/// notice a disconnect (set on the raw `TcpStream`, so it holds for `wss://`).
const READ_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(20);

/// Where a signaling URL points, as [`parse_signaling_url`] reads it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignalingTarget {
    /// The host, with the brackets around an IPv6 literal removed.
    pub host: String,
    /// The port: the URL's own, else 80 for `ws://` and 443 for `wss://`.
    pub port: u16,
    /// `true` for `wss://` (TLS), `false` for `ws://`.
    pub tls: bool,
}

/// Parse a signaling relay URL exactly as [`SignalingClient::connect`] will
/// dial it (v3.5.0, `T-MOBILE-RELAY-VALIDATE`).
///
/// This is the ONE parser: the connect path calls it, and so does
/// [`validate_signaling_url`], which the Android and iOS apps reach through
/// the `rustynes-mobile` bridge (`np_validate_relay_url`). Before v3.5.0 each
/// app had its own check (a regex on Android, `URL(string:)` plus a host test
/// on iOS) and both accepted URLs this parser then refused at connect time,
/// such as a non-numeric port or an unclosed IPv6 literal, so a typo enabled
/// the room-code buttons and failed only after the player pressed one.
///
/// The URL is parsed by `tungstenite`'s client request builder (the `http`
/// crate's URI grammar), which is what the handshake sends. On top of that it
/// refuses three things that grammar lets through but no relay can serve:
///
/// - a scheme other than `ws` / `wss` (`uri_mode`);
/// - an empty host (`ws://:9000`, `wss://[]`);
/// - a port that is not ASCII digits naming 1..=65535, or anything
///   after an IPv6 literal's `]` but such a port. `http::Uri::port` returns
///   `None` for `:notaport`, `:9000x`, `:99999`, a bare `:` and `[::1]x`
///   alike, and the connect path before v3.5.0 then dialled the scheme's
///   default port (80 or 443), a server the user never named, so the URL was
///   neither refused nor honoured. The port text is therefore read from the
///   authority itself. Port 0 is refused: it cannot be connected to.
///
/// Deliberately NOT judged here: whether the host is a valid DNS name
/// (`wss://h..` parses and fails at resolution, as on every other client),
/// and the scheme's case. The `http` grammar refuses `WSS://`, so the apps now
/// refuse it too, where Android's case-insensitive regex used to accept a URL
/// the connect path would then refuse.
///
/// # Errors
/// A short reason, suitable for a log line or a settings-screen hint.
pub fn parse_signaling_url(url: &str) -> Result<SignalingTarget, String> {
    parse_request(url).map(|(_, target)| target)
}

/// Whether `url` is a signaling relay URL the connect path can dial: the
/// verdict of [`parse_signaling_url`] without the parsed parts.
///
/// The URL is taken as given: callers trim user input first (both apps do),
/// and surrounding whitespace is refused rather than silently stripped.
///
/// # Errors
/// The same reasons as [`parse_signaling_url`].
pub fn validate_signaling_url(url: &str) -> Result<(), String> {
    parse_signaling_url(url).map(|_| ())
}

/// [`parse_signaling_url`], keeping the handshake request it built.
fn parse_request(
    url: &str,
) -> Result<(tungstenite::handshake::client::Request, SignalingTarget), String> {
    use tungstenite::client::{IntoClientRequest, uri_mode};
    use tungstenite::stream::Mode;

    let request = url.into_client_request().map_err(|e| e.to_string())?;
    let uri = request.uri();
    let mode = uri_mode(uri).map_err(|e| e.to_string())?;
    let host = uri
        .host()
        .ok_or_else(|| "no host in signaling URL".to_string())?;
    // Strip the brackets around an IPv6 literal before DNS resolution.
    let host = host
        .strip_prefix('[')
        .map_or(host, |h| h.strip_suffix(']').unwrap_or(h));
    if host.is_empty() {
        return Err("no host in signaling URL".to_string());
    }
    let port = match authority_port_text(uri.authority().map_or("", |a| a.as_str())) {
        Ok(None) => match mode {
            Mode::Plain => 80,
            Mode::Tls => 443,
        },
        Ok(Some(text)) => match text.parse::<u16>() {
            Ok(n) if n != 0 && text.bytes().all(|b| b.is_ascii_digit()) => n,
            _ => return Err(format!("invalid port {text:?} in signaling URL")),
        },
        Err(e) => return Err(e),
    };
    let target = SignalingTarget {
        host: host.to_string(),
        port,
        tls: matches!(mode, Mode::Tls),
    };
    Ok((request, target))
}

/// The port text of a URI authority (`[userinfo@]host[:port]`), or `None` when
/// it names no port. Errors on text after an IPv6 literal's `]` that is not a
/// `:port`. The digits are checked by the caller.
fn authority_port_text(authority: &str) -> Result<Option<&str>, String> {
    // Userinfo may contain ':'; the host starts after the LAST '@'.
    let host_port = authority.rsplit_once('@').map_or(authority, |(_, hp)| hp);
    let after_host = if host_port.starts_with('[') {
        let close = host_port
            .find(']')
            .ok_or_else(|| "unclosed IPv6 literal in signaling URL".to_string())?;
        &host_port[close + 1..]
    } else {
        host_port.find(':').map_or("", |i| &host_port[i..])
    };
    match after_host.strip_prefix(':') {
        Some(port) => Ok(Some(port)),
        None if after_host.is_empty() => Ok(None),
        None => Err(format!(
            "unexpected {after_host:?} after the host in signaling URL"
        )),
    }
}

/// A bounded WebSocket connect: resolve the host, dial with `connect_timeout`,
/// set a *generous* (`connect_timeout`) read/write timeout on the raw `TcpStream`
/// for the WS (and, for `wss://`, TLS) handshake via `client_tls`, then let the
/// caller re-assert the short per-loop read timeout. Mirrors what
/// `tungstenite::connect` does internally, minus the unbounded TCP connect.
///
/// The handshake MUST run with the generous timeout, not the 20 ms loop timeout:
/// a slow loopback handshake (observed on macOS CI) otherwise hits the short read
/// timeout and tungstenite aborts with `Interrupted handshake (WouldBlock)`.
fn connect_bounded(
    url: &str,
    connect_timeout: std::time::Duration,
) -> Result<tungstenite::WebSocket<tungstenite::stream::MaybeTlsStream<std::net::TcpStream>>, String>
{
    let (request, target) = parse_request(url)?;
    let host = target.host.as_str();
    let port = target.port;

    // Resolve + dial with a bounded connect; try each resolved address in turn.
    let addrs: Vec<std::net::SocketAddr> = (host, port)
        .to_socket_addrs()
        .map_err(|e| format!("resolve {host}:{port}: {e}"))?
        .collect();
    let mut last_err = format!("no addresses resolved for {host}:{port}");
    let mut stream = None;
    for addr in &addrs {
        match std::net::TcpStream::connect_timeout(addr, connect_timeout) {
            Ok(s) => {
                stream = Some(s);
                break;
            }
            Err(e) => last_err = format!("connect {addr}: {e}"),
        }
    }
    let stream = stream.ok_or(last_err)?;
    stream.set_nodelay(true).ok();
    // Generous read/write timeout for the handshake; the worker loop re-asserts
    // the short READ_TIMEOUT once the handshake completes.
    stream
        .set_read_timeout(Some(connect_timeout))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(connect_timeout))
        .map_err(|e| e.to_string())?;

    // `client_tls` upgrades to TLS for `wss://` and stays plain for `ws://`.
    let (socket, _resp) = tungstenite::client_tls(request, stream).map_err(|e| e.to_string())?;
    Ok(socket)
}

/// Best-effort access to the underlying TCP stream to set a read timeout. Both
/// the plain (`ws://`) and the rustls (`wss://`) variants are reached — the
/// rustls `StreamOwned` exposes the raw socket via its public `sock` field — so
/// the worker can set a short read timeout for either transport (without it,
/// `socket.read()` blocks and the loop can't interleave the outbound drain or
/// notice a disconnect). Returns `None` only for the unused native-tls variant.
fn stream_of(
    socket: &tungstenite::WebSocket<tungstenite::stream::MaybeTlsStream<std::net::TcpStream>>,
) -> Option<&std::net::TcpStream> {
    match socket.get_ref() {
        tungstenite::stream::MaybeTlsStream::Plain(s) => Some(s),
        tungstenite::stream::MaybeTlsStream::Rustls(s) => Some(&s.sock),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// v2.7.4 (frontend audit MOB-04): dropping the client must not wait for
    /// the network. A relay that accepts the TCP connection and then says
    /// nothing keeps the worker in the WebSocket handshake for up to
    /// `CONNECT_TIMEOUT` (10 s); before v2.7.4 `Drop` joined it, so leaving a
    /// netplay screen froze the caller -- on mobile, the UI thread -- that long.
    #[test]
    fn dropping_the_client_does_not_wait_for_a_silent_relay() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            // Accept, then hold the socket open without answering.
            if let Ok((stream, _)) = listener.accept() {
                std::thread::sleep(std::time::Duration::from_secs(15));
                drop(stream);
            }
        });
        let client = SignalingClient::connect(&format!("ws://127.0.0.1:{port}"));
        // Give the worker time to connect and enter the handshake.
        std::thread::sleep(std::time::Duration::from_millis(200));
        let start = std::time::Instant::now();
        drop(client);
        let took = start.elapsed();
        assert!(
            took < std::time::Duration::from_secs(1),
            "drop waited {took:?} for the network"
        );
    }

    /// The shared relay-URL table (`testdata/relay_urls.tsv`), parsed:
    /// `(accept, url, why)` rows.
    fn relay_url_table() -> Vec<(bool, String, String)> {
        let rows: Vec<_> = include_str!("../testdata/relay_urls.tsv")
            .lines()
            .filter(|l| !l.starts_with('#') && !l.is_empty())
            .map(|l| {
                let mut f = l.splitn(3, '\t');
                let verdict = f.next().expect("verdict");
                let url = f.next().expect("url");
                let why = f.next().expect("why").to_string();
                let accept = match verdict {
                    "accept" => true,
                    "refuse" => false,
                    v => panic!("bad verdict {v:?} in {l:?}"),
                };
                let url = if url == "<empty>" {
                    String::new()
                } else {
                    url.replace("\\n", "\n")
                };
                (accept, url, why)
            })
            .collect();
        rows
    }

    /// v3.5.0 (`T-MOBILE-RELAY-VALIDATE`): the one parser gives every row of
    /// the shared table its verdict. The mobile bridge runs the same table
    /// through `np_validate_relay_url`.
    #[test]
    fn the_relay_url_table_matches_the_parser() {
        let rows = relay_url_table();
        let accepted = rows.iter().filter(|r| r.0).count();
        // Non-zero on both sides, so an empty or one-sided table cannot pass.
        assert!(
            accepted >= 10 && rows.len() - accepted >= 20,
            "{accepted} of {}",
            rows.len()
        );
        let mut wrong = Vec::new();
        for (accept, url, why) in &rows {
            let got = validate_signaling_url(url);
            if got.is_ok() != *accept {
                wrong.push(format!(
                    "{url:?} ({why}): want accept={accept}, got {got:?}"
                ));
            }
        }
        assert!(
            wrong.is_empty(),
            "{} of {} rows wrong:\n{}",
            wrong.len(),
            rows.len(),
            wrong.join("\n")
        );
    }

    #[test]
    fn the_parser_reports_the_port_and_host_the_connect_path_dials() {
        let t = parse_signaling_url("ws://[::1]:9000").unwrap();
        assert_eq!((t.host.as_str(), t.port, t.tls), ("::1", 9000, false));
        let t = parse_signaling_url("wss://user:pw@h.example").unwrap();
        assert_eq!((t.host.as_str(), t.port, t.tls), ("h.example", 443, true));
        let t = parse_signaling_url("ws://h").unwrap();
        assert_eq!(t.port, 80);
    }

    /// The connect path refuses a bad port BEFORE dialling. Until v3.5.0
    /// `ws://127.0.0.1:notaport` dialled 127.0.0.1:80 (`http::Uri::port`
    /// is `None` for non-numeric text) and failed, if at all, with an
    /// unrelated connect error; on a host with a server on port 80 it would
    /// have connected to it.
    #[test]
    fn connect_refuses_a_bad_port_without_dialling() {
        let client = SignalingClient::connect("ws://127.0.0.1:notaport");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            if let Some(ev) = client.poll().into_iter().next() {
                match ev {
                    SignalEvent::Closed(reason) => {
                        assert!(reason.contains("invalid port"), "{reason}");
                        break;
                    }
                    other => panic!("unexpected {other:?}"),
                }
            }
            assert!(std::time::Instant::now() < deadline, "no Closed event");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
}
