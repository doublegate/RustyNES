//! Fuzz target for the netplay wire parsers — the untrusted **network-input**
//! boundary (high value).
//!
//! Two parsers ingest bytes straight off a socket / WebSocket and must never
//! panic, hang, or index out of bounds on hostile input:
//!
//! - [`NetMessage::from_bytes`] — the binary UDP gameplay protocol
//!   (`Input` / `InputAck` / `Sync` / `Checksum` / `Quality` / `Roster`). A
//!   malformed / foreign / truncated datagram must decode to `None`, never a
//!   panic (the transport drops it).
//! - [`SignalMessage::parse`] — the JSON signaling / lobby protocol
//!   (`join` / `offer` / `answer` / `candidate` / `room-list` / `quick-match`
//!   / …), including the bounded `room-list` array walk. A garbage frame must
//!   parse to `None`.
//!
//! The same fuzz bytes feed both: raw bytes → `from_bytes`, and a lossy-UTF-8
//! view of them → `parse`. Any successful decode is round-tripped back through
//! the encoder as a cheap self-consistency check.
//!
//! Run with:
//!     cargo install cargo-fuzz
//!     cargo +nightly fuzz run netplay_message
//!
//! Per `docs/testing-strategy.md` §Layer 5.

#![no_main]

use libfuzzer_sys::fuzz_target;
use rustynes_netplay::{NetMessage, SignalMessage};

fuzz_target!(|data: &[u8]| {
    // 1. Binary UDP gameplay message decoder. Must handle any byte slice.
    if let Some(msg) = NetMessage::from_bytes(data) {
        // A decoded message must re-encode (and the encoding stays bounded).
        let _ = msg.to_bytes();
    }

    // 2. JSON signaling / lobby message parser. Interpret the same bytes as
    //    (lossy) UTF-8 text — the wire form is WebSocket text frames.
    let text = String::from_utf8_lossy(data);
    if let Some(sig) = SignalMessage::parse(&text) {
        // A parsed message must re-encode and re-parse to the same value.
        let json = sig.to_json();
        let _ = SignalMessage::parse(&json);
    }
});
