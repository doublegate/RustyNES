//! v3.4.0 (`T-HOSTED-NETPLAY`): the **signalling conformance vectors** that pin
//! the hosted Cloudflare lobby (`services/hosted/`) to this crate's reference
//! [`Relay`] byte for byte.
//!
//! # Why vectors, not a live cross-process test
//!
//! The hosted lobby is a TypeScript port of [`rustynes_netplay::signaling`]
//! running in a Durable Object. The question that matters is "does a client
//! that works against `examples/signaling_server.rs` work, unchanged, against
//! the hosted lobby?" — and the reference server is a thin loop around two pure
//! functions: [`SignalMessage::parse`] on every inbound text frame, then
//! [`Relay::handle`] / [`Relay::disconnect`], whose [`Action`]s are sent as
//! [`SignalMessage::to_json`] text. So the observable behaviour of the server is
//! fully captured by "this sequence of inbound frames and disconnects produces
//! exactly these outbound frames and closes", and that is what this file
//! records.
//!
//! Running `wrangler dev` from `cargo test` would need Node, the npm tree and a
//! free port inside every Rust CI job; recording the reference's behaviour once
//! as data and replaying it on both sides needs neither, and catches drift in
//! either direction:
//!
//! - **this test** regenerates the vectors from the live Rust code and fails if
//!   they differ from the committed file, so a change to the Rust protocol that
//!   is not carried to the Worker cannot land silently;
//! - **`services/hosted/test/conformance.test.ts`** replays the same file
//!   against the TypeScript port twice: once against the pure relay, and once
//!   end to end over real `WebSocket`s into the Durable Object (vitest in the
//!   Workers runtime), comparing every outbound frame as an exact string.
//!
//! # Re-recording
//!
//! After an intentional protocol change:
//!
//! ```text
//! RUSTYNES_SIGNALING_VECTORS_BLESS=1 cargo test -p rustynes-netplay --test hosted_signaling_vectors
//! ```
//!
//! then port the change to `services/hosted/src/protocol.ts` until
//! `npm test` in `services/hosted/` is green again.
//!
//! # What the vectors deliberately avoid
//!
//! The Rust relay keeps its rooms in a `HashMap`, whose iteration order is
//! randomised per process. Two operations depend on that order: the
//! `room-list` reply (several open rooms are listed in an unspecified order)
//! and `quick-match` when more than one room could take the player. The
//! reference itself is nondeterministic there, so no scenario has two
//! candidate rooms at either point; anything else would record one arbitrary
//! run. The Worker's order is insertion order (a JavaScript `Map`), which is one
//! of the orders the reference can produce.

use std::fmt::Write as _;
use std::path::PathBuf;

use rustynes_netplay::signaling::{Action, ClientId, Relay, SignalMessage};

/// One step of a scenario: a client sends a raw text frame, or disconnects.
enum Step {
    Send(ClientId, String),
    Disconnect(ClientId),
}

fn send(client: ClientId, text: &str) -> Step {
    Step::Send(client, text.to_string())
}

const H1: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const H2: &str = "2222222222222222222222222222222222222222222222222222222222222222";

fn join(room: &str, hash: &str, max: u8) -> String {
    format!(r#"{{"type":"join","room":"{room}","rom_hash":"{hash}","max_players":{max}}}"#)
}

/// The scenarios. Each runs against a fresh [`Relay`]; client ids are the
/// harness's socket numbers.
#[allow(clippy::too_many_lines)]
fn scenarios() -> Vec<(&'static str, Vec<Step>)> {
    vec![
        (
            "two-player join, offer, answer, candidate, leave",
            vec![
                send(1, &join("ABC123", H1, 2)),
                send(2, &join("ABC123", H1, 2)),
                send(
                    1,
                    r#"{"type":"offer","from":0,"to":1,"sdp":"v=0\r\no=- 1 2 IN IP4 0.0.0.0\r\n"}"#,
                ),
                send(2, r#"{"type":"answer","from":1,"to":0,"sdp":"answer-sdp"}"#),
                send(
                    1,
                    r#"{"type":"candidate","from":0,"to":1,"candidate":"candidate:1 1 udp 2122260223 192.168.1.5 50000 typ host","sdp_mid":"0","sdp_m_line_index":0}"#,
                ),
                send(
                    2,
                    r#"{"type":"public-addr","from":1,"to":0,"addr":"198.51.100.9:40000"}"#,
                ),
                Step::Disconnect(1),
                Step::Disconnect(2),
            ],
        ),
        (
            "legacy two-player wire form (no max_players, no from/to)",
            vec![
                send(1, r#"{"type":"join","room":"legacy","rom_hash":""}"#),
                send(2, r#"{"type":"join","room":"legacy"}"#),
                send(1, r#"{"type":"offer","sdp":"o"}"#),
                send(2, r#"{"type":"answer","sdp":"a"}"#),
            ],
        ),
        (
            "four-player mesh, slot-routed, compaction after a leave",
            vec![
                send(1, &join("mesh4", H1, 4)),
                send(2, &join("mesh4", H1, 2)),
                send(3, &join("mesh4", H1, 4)),
                send(1, r#"{"type":"offer","from":0,"to":2,"sdp":"o02"}"#),
                send(3, r#"{"type":"answer","from":2,"to":0,"sdp":"a20"}"#),
                send(
                    2,
                    r#"{"type":"candidate","from":1,"to":2,"candidate":"c","sdp_mid":"","sdp_m_line_index":3}"#,
                ),
                send(4, &join("mesh4", H1, 4)),
                send(5, &join("mesh4", H1, 4)),
                Step::Disconnect(2),
                send(6, &join("mesh4", H1, 4)),
                send(6, r#"{"type":"offer","from":3,"to":9,"sdp":"bad-to"}"#),
            ],
        ),
        (
            "relayed from is the sender's real slot, not the claimed one",
            vec![
                send(1, &join("spoof", H1, 3)),
                send(2, &join("spoof", H1, 3)),
                send(3, &join("spoof", H1, 3)),
                // Slot 1 (client 2) claims to be slot 0 in every relayable type.
                send(2, r#"{"type":"offer","from":0,"to":2,"sdp":"forged"}"#),
                send(2, r#"{"type":"answer","from":0,"to":2,"sdp":"forged"}"#),
                send(
                    2,
                    r#"{"type":"candidate","from":0,"to":2,"candidate":"c","sdp_mid":"0","sdp_m_line_index":0}"#,
                ),
                send(
                    2,
                    r#"{"type":"public-addr","from":0,"to":2,"addr":"203.0.113.7:9"}"#,
                ),
                // An out-of-range claim is stamped too.
                send(3, r#"{"type":"offer","from":200,"to":0,"sdp":"o"}"#),
            ],
        ),
        (
            "relay to own slot falls back to every other peer",
            vec![
                send(1, &join("three", "", 3)),
                send(2, &join("three", "", 3)),
                send(3, &join("three", "", 3)),
                send(2, r#"{"type":"offer","from":1,"to":1,"sdp":"self"}"#),
            ],
        ),
        (
            "rom mismatch rejected, empty hash bypasses the check",
            vec![
                send(1, &join("roms", H1, 3)),
                send(2, &join("roms", H2, 3)),
                send(3, &join("roms", "", 3)),
            ],
        ),
        (
            "max_players clamps to 2..=4",
            vec![
                send(1, &join("big", H1, 9)),
                send(2, &join("small", H1, 1)),
                send(3, r#"{"type":"join","room":"huge","max_players":300}"#),
            ],
        ),
        (
            "lobby directory: one open room, filters, full rooms hidden",
            vec![
                send(9, r#"{"type":"list-rooms","rom_hash":""}"#),
                send(1, &join("open1", H1, 2)),
                send(9, r#"{"type":"list-rooms"}"#),
                send(9, &format!(r#"{{"type":"list-rooms","rom_hash":"{H1}"}}"#)),
                send(9, &format!(r#"{{"type":"list-rooms","rom_hash":"{H2}"}}"#)),
                send(2, &join("open1", H1, 2)),
                send(9, r#"{"type":"list-rooms","rom_hash":""}"#),
            ],
        ),
        (
            "quick-match creates, fills and creates again",
            vec![
                send(
                    1,
                    &format!(r#"{{"type":"quick-match","rom_hash":"{H1}","max_players":2}}"#),
                ),
                send(
                    2,
                    &format!(r#"{{"type":"quick-match","rom_hash":"{H1}","max_players":4}}"#),
                ),
                send(3, &format!(r#"{{"type":"quick-match","rom_hash":"{H1}"}}"#)),
                send(
                    4,
                    &format!(r#"{{"type":"quick-match","rom_hash":"{H2}","max_players":3}}"#),
                ),
                // Two rooms are open now (QM-000002 for H1, QM-000003 for H2),
                // so only a filtered listing has a defined order.
                send(9, &format!(r#"{{"type":"list-rooms","rom_hash":"{H2}"}}"#)),
            ],
        ),
        (
            "re-join leaves the old room first (AUD-13)",
            vec![
                send(1, &join("roomA", H1, 2)),
                send(2, &join("roomA", H1, 2)),
                send(2, &join("roomB", H1, 2)),
                send(3, &join("roomA", H1, 2)),
            ],
        ),
        (
            "server-to-client types from a client are ignored; junk is dropped",
            vec![
                send(1, &join("ign", H1, 2)),
                send(1, r#"{"type":"joined","slot":1,"max_players":2}"#),
                send(1, r#"{"type":"error","reason":"spoof"}"#),
                send(1, r#"{"type":"peer-left","slot":0}"#),
                send(1, "not json at all"),
                send(1, r#"{"type":"unknown-kind"}"#),
                send(1, r#"{"type":"offer","from":0,"to":1}"#),
                send(7, r#"{"type":"offer","from":0,"to":1,"sdp":"nobody-home"}"#),
            ],
        ),
        (
            "parser leniency: whitespace, key-named values, escapes, unicode",
            vec![
                send(
                    1,
                    "{ \"type\" : \"join\" , \"room\" : \"ws\" , \"max_players\" : 2 }",
                ),
                send(2, r#"{"room":"ws","type":"join"}"#),
                send(
                    1,
                    r#"{"type":"candidate","candidate":"cand","from":0,"to":1,"sdp_mid":"candidate","sdp_m_line_index":1}"#,
                ),
                send(
                    1,
                    r#"{"type":"offer","from":0,"to":1,"sdp":"q\"b\\s\/t\tn\nu\u0041 \u00e9 é 𝄞"}"#,
                ),
                send(
                    1,
                    r#"{"type":"offer","from":0,"to":256,"sdp":"slot-overflow"}"#,
                ),
                send(
                    1,
                    r#"{"type":"candidate","from":0,"to":1,"candidate":"x","sdp_m_line_index":4294967296}"#,
                ),
                send(
                    1,
                    r#"{"type":"candidate","from":0,"to":1,"candidate":"ctl\u0001","sdp_m_line_index":4294967295}"#,
                ),
            ],
        ),
    ]
}

/// Standard JSON string quoting for the vector FILE itself (not the protocol's
/// encoder, which is under test): escape `"`, `\` and every control character.
fn q(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn action_json(a: &Action) -> String {
    match a {
        Action::Send { to, msg } => {
            format!(r#"{{"send":{{"to":{to},"text":{}}}}}"#, q(&msg.to_json()))
        }
        Action::Close { who } => format!(r#"{{"close":{who}}}"#),
    }
}

/// Run every scenario against the reference relay and render the vector file.
fn render() -> String {
    let mut out = String::new();
    out.push_str("{\n  \"comment\": ");
    out.push_str(&q(
        "Generated by crates/rustynes-netplay/tests/hosted_signaling_vectors.rs from the reference \
         signalling Relay. Do not edit by hand: re-record with RUSTYNES_SIGNALING_VECTORS_BLESS=1.",
    ));
    out.push_str(",\n  \"scenarios\": [\n");
    let all = scenarios();
    for (si, (name, steps)) in all.iter().enumerate() {
        let mut relay = Relay::new();
        let _ = writeln!(
            out,
            "    {{\n      \"name\": {},\n      \"steps\": [",
            q(name)
        );
        for (i, step) in steps.iter().enumerate() {
            // Exactly what examples/signaling_server.rs does per frame: parse,
            // drop on failure, else Relay::handle; and Relay::disconnect on close.
            let (head, actions) = match step {
                Step::Send(c, text) => (
                    format!(r#""op":"send","client":{c},"text":{}"#, q(text)),
                    SignalMessage::parse(text).map_or_else(Vec::new, |m| relay.handle(*c, m)),
                ),
                Step::Disconnect(c) => (
                    format!(r#""op":"disconnect","client":{c}"#),
                    relay.disconnect(*c),
                ),
            };
            let acts: Vec<String> = actions.iter().map(action_json).collect();
            let sep = if i + 1 == steps.len() { "" } else { "," };
            let _ = writeln!(
                out,
                "        {{{head},\"expect\":[{}]}}{sep}",
                acts.join(",")
            );
        }
        let sep = if si + 1 == all.len() { "" } else { "," };
        let _ = writeln!(out, "      ]\n    }}{sep}");
    }
    out.push_str("  ]\n}\n");
    out
}

fn vector_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../services/hosted/test/vectors/signaling-vectors.json")
}

#[test]
fn committed_vectors_match_the_reference_relay() {
    let rendered = render();
    let path = vector_path();
    if std::env::var_os("RUSTYNES_SIGNALING_VECTORS_BLESS").is_some() {
        std::fs::write(&path, &rendered).expect("write signaling vectors");
        return;
    }
    let committed = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "read {}: {e} (record it with RUSTYNES_SIGNALING_VECTORS_BLESS=1)",
            path.display()
        )
    });
    assert!(
        committed == rendered,
        "the reference signalling Relay no longer produces the committed vectors in {}.\n\
         If the protocol change is intentional, re-record with \
         RUSTYNES_SIGNALING_VECTORS_BLESS=1 and port it to services/hosted/src/protocol.ts.",
        path.display()
    );
}

/// Every scenario's outbound actions, in order, exactly as [`render`] records
/// them (what the reference relay SENDS and closes, nothing a client sent).
fn emitted_actions() -> Vec<Action> {
    let mut out = Vec::new();
    for (_, steps) in scenarios() {
        let mut relay = Relay::new();
        for step in &steps {
            out.extend(match step {
                Step::Send(c, text) => {
                    SignalMessage::parse(text).map_or_else(Vec::new, |m| relay.handle(*c, m))
                }
                Step::Disconnect(c) => relay.disconnect(*c),
            });
        }
    }
    out
}

/// The vectors must exercise every message kind the relay routes or answers, or
/// a gap in the port could hide behind a scenario list that never reaches it.
///
/// v3.4.0 (review of #613): checked against the relay's own actions. The
/// first form searched the rendered file, where a client's inbound frame of
/// the same type (a spoofed `joined`, a routed `offer` as the client sent it)
/// satisfied the search by itself, so only `room-list` and `matched`, which
/// no client sends, were really checked.
#[test]
fn vectors_cover_every_routed_message_kind() {
    let actions = emitted_actions();
    let sent: Vec<String> = actions
        .iter()
        .filter_map(|a| match a {
            Action::Send { msg, .. } => Some(msg.to_json()),
            Action::Close { .. } => None,
        })
        .collect();
    for kind in [
        "joined",
        "peer-joined",
        "peer-left",
        "offer",
        "answer",
        "candidate",
        "public-addr",
        "error",
        "room-list",
        "matched",
    ] {
        let needle = format!(r#""type":"{kind}""#);
        assert!(
            sent.iter().any(|json| json.contains(&needle)),
            "the relay never sends a frame of type {kind} in the vectors"
        );
    }
    assert!(
        actions.iter().any(|a| matches!(a, Action::Close { .. })),
        "no close action in the vectors"
    );
}
