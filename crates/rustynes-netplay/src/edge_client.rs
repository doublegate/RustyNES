//! The native client of the hosted spectate relay (v3.7.0,
//! `T-NETPLAY-EDGE-SPECTATE`).
//!
//! A blocking WebSocket on a worker thread, as the
//! signalling client is ([`crate::signaling_client`], whose bounded connect it
//! reuses), carrying netplay wire frames ([`NetMessage::to_bytes`]) as binary
//! WebSocket messages.
//!
//! A **publisher** (a player) connects to
//! `wss://<hosted base>/spectate/<match>?role=publish&key=<hex>` and sends the
//! [`crate::EdgePublisher`]'s messages; a **viewer** connects with
//! `?role=view` and feeds what it receives to an [`crate::EdgeSpectator`].
//! [`spectate_url`] builds both. The relay itself is
//! `services/hosted/src/spectate.ts`.

use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::thread::JoinHandle;

use tungstenite::Message;

use crate::message::NetMessage;
use crate::signaling_client::{
    CONNECT_TIMEOUT, DROP_GRACE, READ_TIMEOUT, connect_bounded, join_or_detach, stream_of,
};

/// What the relay connection reports.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EdgeEvent {
    /// The WebSocket is open.
    Connected,
    /// A decoded wire frame.
    Message(NetMessage),
    /// The connection failed or closed, with a short reason. Terminal.
    Closed(String),
}

/// The spectate relay URL for `match_id` under the hosted base `base`.
///
/// The base is `https://...` or `http://...`, as the hosted settings hold it;
/// the URL is a publisher's (with its `key`) or a viewer (`None`). `None` when the base is
/// not an `http(s)` URL or the match id is not 1-32 of `[A-Za-z0-9_-]`.
#[must_use]
pub fn spectate_url(base: &str, match_id: &str, publish_key: Option<&str>) -> Option<String> {
    let base = base.trim().trim_end_matches('/');
    let ws = base.strip_prefix("https://").map_or_else(
        || {
            base.strip_prefix("http://")
                .map(|rest| format!("ws://{rest}"))
        },
        |rest| Some(format!("wss://{rest}")),
    )?;
    let ok_id = !match_id.is_empty()
        && match_id.len() <= 32
        && match_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
    if !ok_id {
        return None;
    }
    Some(publish_key.map_or_else(
        || format!("{ws}/spectate/{match_id}?role=view"),
        |key| format!("{ws}/spectate/{match_id}?role=publish&key={key}"),
    ))
}

/// A blocking relay connection: a worker thread and the two channels to it.
#[derive(Debug)]
pub struct EdgeRelayClient {
    out_tx: Option<Sender<Vec<u8>>>,
    event_rx: Receiver<EdgeEvent>,
    worker: Option<JoinHandle<()>>,
}

impl EdgeRelayClient {
    /// Connect to a spectate URL on a worker thread. Returns at once; drain
    /// [`Self::poll`] for [`EdgeEvent::Connected`] or [`EdgeEvent::Closed`].
    #[must_use]
    pub fn connect(url: &str) -> Self {
        let (out_tx, out_rx) = std::sync::mpsc::channel::<Vec<u8>>();
        let (event_tx, event_rx) = std::sync::mpsc::channel::<EdgeEvent>();
        let url = url.to_string();
        let worker = std::thread::Builder::new()
            .name("netplay-edge".into())
            .spawn(move || worker_loop(&url, &out_rx, &event_tx))
            .expect("spawn netplay-edge worker thread");
        Self {
            out_tx: Some(out_tx),
            event_rx,
            worker: Some(worker),
        }
    }

    /// Queue `msg` for the relay (a silent no-op once the worker has exited;
    /// [`Self::poll`] reports why).
    pub fn send(&self, msg: &NetMessage) {
        if let Some(tx) = &self.out_tx {
            let _ = tx.send(msg.to_bytes());
        }
    }

    /// Drain every pending event without blocking.
    pub fn poll(&self) -> Vec<EdgeEvent> {
        let mut out = Vec::new();
        while let Ok(ev) = self.event_rx.try_recv() {
            out.push(ev);
        }
        out
    }
}

impl Drop for EdgeRelayClient {
    fn drop(&mut self) {
        self.out_tx = None;
        if let Some(worker) = self.worker.take() {
            join_or_detach(worker, DROP_GRACE);
        }
    }
}

fn worker_loop(url: &str, out_rx: &Receiver<Vec<u8>>, event_tx: &Sender<EdgeEvent>) {
    let mut socket = match connect_bounded(url, CONNECT_TIMEOUT) {
        Ok(socket) => socket,
        Err(e) => {
            let _ = event_tx.send(EdgeEvent::Closed(format!("connect failed: {e}")));
            return;
        }
    };
    if event_tx.send(EdgeEvent::Connected).is_err() {
        return;
    }
    if let Some(stream) = stream_of(&socket) {
        let _ = stream.set_read_timeout(Some(READ_TIMEOUT));
    }
    loop {
        loop {
            match out_rx.try_recv() {
                Ok(bytes) => {
                    if socket.send(Message::Binary(bytes.into())).is_err() {
                        let _ = event_tx.send(EdgeEvent::Closed("send failed".into()));
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
        match socket.read() {
            Ok(Message::Binary(bytes)) => {
                // A frame that does not decode is dropped, never a panic.
                if let Some(msg) = NetMessage::from_bytes(&bytes)
                    && event_tx.send(EdgeEvent::Message(msg)).is_err()
                {
                    return;
                }
            }
            Ok(Message::Close(frame)) => {
                let why =
                    frame.map_or_else(|| "relay closed".to_string(), |f| f.reason.to_string());
                let _ = event_tx.send(EdgeEvent::Closed(why));
                return;
            }
            Ok(Message::Ping(p)) => {
                let _ = socket.send(Message::Pong(p));
            }
            Ok(_) => {}
            Err(tungstenite::Error::Io(e))
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(e) => {
                let _ = event_tx.send(EdgeEvent::Closed(format!("read error: {e}")));
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spectate_urls_follow_the_hosted_base() {
        assert_eq!(
            spectate_url("https://h.example.workers.dev/", "abc_1", None).as_deref(),
            Some("wss://h.example.workers.dev/spectate/abc_1?role=view")
        );
        assert_eq!(
            spectate_url("http://127.0.0.1:8799", "m", Some("00ff")).as_deref(),
            Some("ws://127.0.0.1:8799/spectate/m?role=publish&key=00ff")
        );
        assert_eq!(spectate_url("ftp://x", "m", None), None);
        assert_eq!(spectate_url("https://x", "bad id", None), None);
        assert_eq!(spectate_url("https://x", &"a".repeat(33), None), None);
    }
}
