//! v2.6.0: the WebRTC **signaling** room/relay protocol — the pure, async-free
//! core of the reference signaling server.
//!
//! # Why a signaling server
//!
//! Two browsers cannot open a raw UDP socket, so the wasm netplay path uses
//! WebRTC (a `WebRtcTransport` / `WebRtcMeshTransport` over `RtcDataChannel`s,
//! on wasm). A WebRTC peer connection forms only after the two peers exchange an
//! SDP offer/answer and their ICE candidates through a third party — the
//! **signaling server**. It is a small relay that groups peers by **room id**
//! and forwards their handshake messages; it carries **no gameplay traffic**
//! (that flows peer-to-peer over the data channels once connected).
//!
//! A room holds up to `max_players` peers (2..=4). For more than two players the
//! peers form a **full mesh** — every peer connects to every other — and the
//! relay routes each offer/answer/candidate to a specific peer by its slot.
//!
//! # What is here
//!
//! This module is the **transport-agnostic protocol core**: message
//! parsing/encoding ([`SignalMessage`]) and the per-server room bookkeeping +
//! routing decision ([`Relay`]). It does **no** I/O and pulls in **no** async
//! runtime, so it is unit-tested headlessly in the default build. The async
//! WebSocket plumbing that drives it lives in `examples/signaling_server.rs`
//! (behind the `signaling-server` feature), which is a thin loop:
//! parse a frame → [`Relay::handle`] → send the resulting [`Action`]s out.
//!
//! # Wire format
//!
//! JSON over WebSocket (text frames). A client first `join`s a room (announcing
//! the player count it wants); the server assigns it the next free slot and
//! nudges each *existing* peer that a higher-slot newcomer arrived. The rule is
//! **the lower slot of any pair offers to the higher slot**, so each existing
//! peer offers to the newcomer. `offer` / `answer` / `candidate` carry
//! `{ from, to }` slots and are **routed to the `to` peer**.
//!
//! ```text
//! → { "type": "join",      "room": "<code>", "rom_hash": "<hex>", "max_players": 4 }
//! ← { "type": "joined",    "slot": 2, "max_players": 4 }        (assigned slot + room size)
//! ← { "type": "peer-joined","slot": 3 }                         (a higher-slot peer joined → offer to it)
//! ↔ { "type": "offer",     "from": 1, "to": 3, "sdp": "..." }   (routed to slot 3)
//! ↔ { "type": "answer",    "from": 3, "to": 1, "sdp": "..." }   (routed to slot 1)
//! ↔ { "type": "candidate", "from": 1, "to": 3, "candidate": "...", "sdp_mid": "...", "sdp_m_line_index": N }
//! ← { "type": "peer-left", "slot": 2 }                          (a peer disconnected)
//! ← { "type": "error",     "reason": "..." }                   (room full / rom mismatch)
//! ```
//!
//! The server verifies every peer in a room announced the **same** `rom_hash`
//! (a cheap guard against accidentally pairing different games); a mismatch
//! rejects the joiner. A 2-player session is just the `max_players = 2` case
//! (and a legacy client that omits `max_players` / `from` / `to` defaults to 2
//! players and the "other peer" routing).

use std::collections::HashMap;

/// An opaque per-connection identifier the async layer assigns to each WebSocket
/// client.
///
/// E.g. a monotonically increasing counter. The [`Relay`] uses it to route
/// relayed messages back to the correct socket without knowing anything about
/// the transport.
pub type ClientId = u64;

/// The maximum number of rooms a single [`SignalMessage::RoomList`] carries.
///
/// Both the encoder and [`SignalMessage::parse`] cap at this count, so a
/// malicious / oversized frame cannot drive an unbounded allocation (a `DoS`). A
/// public lobby with more than this many concurrent open rooms simply truncates
/// the browse list — matchmaking ([`SignalMessage::QuickMatch`]) still reaches
/// them.
pub const MAX_ROOM_LIST: usize = 256;

/// One open room's public metadata, for the lobby browser
/// ([`SignalMessage::RoomList`]).
///
/// Carries only what a joiner needs to decide whether to join: the room `code`,
/// how many players are present vs. the room's capacity, and the `rom_hash` the
/// room is playing (so the browser can label / filter by game). It deliberately
/// exposes **no** SDP, ICE, or per-client identity — the lobby is a directory,
/// not a transport.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoomInfo {
    /// The room code a joiner passes to [`SignalMessage::Join`].
    pub code: String,
    /// Players currently in the room (`1..=max_players`).
    pub players: u8,
    /// The room's total capacity (2..=4).
    pub max_players: u8,
    /// Hex SHA-256 of the ROM the room is playing (may be empty for a room that
    /// announced no hash).
    pub rom_hash: String,
}

impl RoomInfo {
    /// `true` if the room has a free slot a new peer could take.
    #[must_use]
    pub const fn is_joinable(&self) -> bool {
        self.players < self.max_players
    }

    /// Encode as a flat JSON object (an element of the `rooms` array).
    fn to_json_object(&self) -> String {
        format!(
            r#"{{"room":{},"players":{},"max_players":{},"rom_hash":{}}}"#,
            json_quote(&self.code),
            self.players,
            self.max_players,
            json_quote(&self.rom_hash)
        )
    }

    /// Parse one flat JSON object (a `rooms` array element). Returns `None` if a
    /// required field is missing / malformed.
    fn parse_object(obj: &str) -> Option<Self> {
        Some(Self {
            code: json_str_field(obj, "room")?,
            players: u8::try_from(json_num_field(obj, "players")?).ok()?,
            max_players: u8::try_from(json_num_field(obj, "max_players")?).ok()?,
            rom_hash: json_str_field(obj, "rom_hash").unwrap_or_default(),
        })
    }
}

/// A signaling message, in both directions.
///
/// Parsed from / encoded to the JSON wire form (see the module docs). The
/// encode/parse here is hand-rolled and dependency-free so this module needs no
/// `serde` — the async server example may use `serde_json` for convenience, but
/// the protocol does not require it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SignalMessage {
    /// Client → server: join `room`, announcing the `rom_hash` (hex) it will
    /// play and the `max_players` the room should hold (2..=4). The first joiner
    /// sets the room size; subsequent joiners' `max_players` is ignored. Each
    /// joiner is assigned the next free slot (0..`max_players`).
    Join {
        /// The room code grouping the peers.
        room: String,
        /// Hex SHA-256 of the ROM, so the server can reject a mismatched peer.
        rom_hash: String,
        /// The total player count the first joiner wants (2..=4). Defaults to 2
        /// when absent (the legacy 2-player wire form).
        max_players: u8,
    },
    /// Server → client: you are in the room at `slot`, which holds `max_players`
    /// peers total.
    Joined {
        /// The assigned slot (`0..max_players`). The lower slot of any pair is
        /// the WebRTC offerer.
        slot: u8,
        /// The room's total player count (2..=4).
        max_players: u8,
    },
    /// Server → client: the peer at `slot` is now present. Sent only to the
    /// *existing* peers when a higher-slot peer joins, so each existing peer
    /// creates a WebRTC offer to the newcomer (lower slot offers to higher).
    PeerJoined {
        /// The newcomer's slot (always greater than the recipient's own slot).
        slot: u8,
    },
    /// Server → client: the peer at `slot` disconnected.
    PeerLeft {
        /// The departed peer's slot.
        slot: u8,
    },
    /// Relayed peer→peer: an SDP offer from peer `from` to peer `to`.
    Offer {
        /// The sender's slot (the offerer).
        from: u8,
        /// The destination slot (the answerer).
        to: u8,
        /// The offer SDP blob.
        sdp: String,
    },
    /// Relayed peer→peer: an SDP answer from peer `from` to peer `to`.
    Answer {
        /// The sender's slot (the answerer).
        from: u8,
        /// The destination slot (the offerer).
        to: u8,
        /// The answer SDP blob.
        sdp: String,
    },
    /// Relayed peer→peer: one ICE candidate from peer `from` to peer `to`.
    Candidate {
        /// The sender's slot.
        from: u8,
        /// The destination slot.
        to: u8,
        /// The candidate string.
        candidate: String,
        /// The media-stream id this candidate is for.
        sdp_mid: String,
        /// The media-line index.
        sdp_m_line_index: u32,
    },
    /// Relayed peer→peer: a STUN-discovered **public reflexive address** from
    /// peer `from` to peer `to`, for the native raw-UDP NAT-traversal path
    /// (v1.8.7). Unlike [`Offer`](Self::Offer) / [`Answer`](Self::Answer) /
    /// [`Candidate`](Self::Candidate) — which carry browser WebRTC SDP/ICE — this
    /// carries a single `IP:port` string (an `addr.to_string()` of a
    /// [`SocketAddr`](std::net::SocketAddr)) the mobile / native client exchanges
    /// to drive UDP hole punching ([`crate::stun::HolePunch`]). One relay serves
    /// both the browser SDP handshake and the native address rendezvous; this
    /// variant is routed by slot exactly like the SDP ones.
    PublicAddr {
        /// The sender's slot.
        from: u8,
        /// The destination slot.
        to: u8,
        /// The sender's public reflexive address as `IP:port` (a
        /// [`SocketAddr`](std::net::SocketAddr) string).
        addr: String,
    },
    /// Server → client: a fatal signaling error (room full, rom mismatch, …).
    Error {
        /// A short human-readable reason.
        reason: String,
    },
    /// Client → server: request the current lobby directory — the open,
    /// joinable rooms — for a **browse-and-join** UI (v2.2.0). Optionally
    /// filtered to rooms playing a specific `rom_hash` (empty = all games). The
    /// server replies with a [`RoomList`](Self::RoomList). Carrying no per-room
    /// identity, this is safe to answer for any connected client.
    ListRooms {
        /// Restrict the listing to rooms whose `rom_hash` matches (hex; empty =
        /// no filter).
        rom_hash: String,
    },
    /// Server → client: the open, joinable rooms (a [`ListRooms`](Self::ListRooms)
    /// reply, capped at [`MAX_ROOM_LIST`]). Each [`RoomInfo`] carries the code a
    /// joiner passes to [`Join`](Self::Join).
    RoomList {
        /// The open rooms (may be empty).
        rooms: Vec<RoomInfo>,
    },
    /// Client → server: **matchmaking** — put me into *any* open room playing
    /// `rom_hash` with a free slot, creating a fresh room if none exists
    /// (v2.2.0). The server resolves a target room and joins the client to it,
    /// replying with [`Matched`](Self::Matched) (which names the resolved room
    /// code) and nudging the room's existing peers exactly like a normal
    /// [`Join`](Self::Join). This is the "quick play" path — the user never sees
    /// or types a room code.
    QuickMatch {
        /// Hex SHA-256 of the ROM to match on (a room's game must match).
        rom_hash: String,
        /// The player count to request when *creating* a new room (2..=4).
        max_players: u8,
    },
    /// Server → client: a [`QuickMatch`](Self::QuickMatch) landed you in room
    /// `room` at `slot` (which holds `max_players`). Distinct from
    /// [`Joined`](Self::Joined) only in that it also reports the resolved room
    /// **code**, so the matchmade client can display / share it. WebRTC pairing
    /// then proceeds identically (lower slot offers to higher).
    Matched {
        /// The resolved room code (existing or freshly created).
        room: String,
        /// The assigned slot (`0..max_players`).
        slot: u8,
        /// The room's total capacity (2..=4).
        max_players: u8,
    },
}

impl SignalMessage {
    /// Parse a JSON text frame into a [`SignalMessage`]. Returns `None` on a
    /// malformed / unknown-type frame (the server drops it, never panics). The
    /// parser is minimal + dependency-free: it extracts the `"type"` discriminant
    /// and the string/number fields each variant needs.
    #[must_use]
    pub fn parse(json: &str) -> Option<Self> {
        // Optional slot field, defaulting to 0 (the legacy 2-player wire form
        // omitted from/to/slot — slot 0 was the implicit offerer).
        let slot_or = |key: &str| -> u8 {
            json_num_field(json, key)
                .and_then(|n| u8::try_from(n).ok())
                .unwrap_or(0)
        };
        let ty = json_str_field(json, "type")?;
        match ty.as_str() {
            "join" => Some(Self::Join {
                room: json_str_field(json, "room")?,
                rom_hash: json_str_field(json, "rom_hash").unwrap_or_default(),
                // Default 2 keeps the legacy 2-player Join (no max_players) valid.
                max_players: json_num_field(json, "max_players")
                    .and_then(|n| u8::try_from(n).ok())
                    .unwrap_or(2),
            }),
            "joined" => Some(Self::Joined {
                slot: u8::try_from(json_num_field(json, "slot")?).ok()?,
                max_players: json_num_field(json, "max_players")
                    .and_then(|n| u8::try_from(n).ok())
                    .unwrap_or(2),
            }),
            "peer-joined" => Some(Self::PeerJoined {
                slot: slot_or("slot"),
            }),
            "peer-left" => Some(Self::PeerLeft {
                slot: slot_or("slot"),
            }),
            "offer" => Some(Self::Offer {
                from: slot_or("from"),
                to: slot_or("to"),
                sdp: json_str_field(json, "sdp")?,
            }),
            "answer" => Some(Self::Answer {
                from: slot_or("from"),
                to: slot_or("to"),
                sdp: json_str_field(json, "sdp")?,
            }),
            "candidate" => Some(Self::Candidate {
                from: slot_or("from"),
                to: slot_or("to"),
                candidate: json_str_field(json, "candidate")?,
                sdp_mid: json_str_field(json, "sdp_mid").unwrap_or_default(),
                sdp_m_line_index: u32::try_from(json_num_field(json, "sdp_m_line_index")?).ok()?,
            }),
            "public-addr" => Some(Self::PublicAddr {
                from: slot_or("from"),
                to: slot_or("to"),
                addr: json_str_field(json, "addr")?,
            }),
            "error" => Some(Self::Error {
                reason: json_str_field(json, "reason").unwrap_or_default(),
            }),
            "list-rooms" => Some(Self::ListRooms {
                rom_hash: json_str_field(json, "rom_hash").unwrap_or_default(),
            }),
            "room-list" => Some(Self::RoomList {
                rooms: parse_room_array(json),
            }),
            "quick-match" => Some(Self::QuickMatch {
                rom_hash: json_str_field(json, "rom_hash").unwrap_or_default(),
                max_players: json_num_field(json, "max_players")
                    .and_then(|n| u8::try_from(n).ok())
                    .unwrap_or(2),
            }),
            "matched" => Some(Self::Matched {
                room: json_str_field(json, "room")?,
                slot: u8::try_from(json_num_field(json, "slot")?).ok()?,
                max_players: json_num_field(json, "max_players")
                    .and_then(|n| u8::try_from(n).ok())
                    .unwrap_or(2),
            }),
            _ => None,
        }
    }

    /// Encode this message to a JSON text frame.
    #[must_use]
    pub fn to_json(&self) -> String {
        match self {
            Self::Join {
                room,
                rom_hash,
                max_players,
            } => format!(
                r#"{{"type":"join","room":{},"rom_hash":{},"max_players":{max_players}}}"#,
                json_quote(room),
                json_quote(rom_hash)
            ),
            Self::Joined { slot, max_players } => {
                format!(r#"{{"type":"joined","slot":{slot},"max_players":{max_players}}}"#)
            }
            Self::PeerJoined { slot } => {
                format!(r#"{{"type":"peer-joined","slot":{slot}}}"#)
            }
            Self::PeerLeft { slot } => format!(r#"{{"type":"peer-left","slot":{slot}}}"#),
            Self::Offer { from, to, sdp } => {
                format!(
                    r#"{{"type":"offer","from":{from},"to":{to},"sdp":{}}}"#,
                    json_quote(sdp)
                )
            }
            Self::Answer { from, to, sdp } => {
                format!(
                    r#"{{"type":"answer","from":{from},"to":{to},"sdp":{}}}"#,
                    json_quote(sdp)
                )
            }
            Self::Candidate {
                from,
                to,
                candidate,
                sdp_mid,
                sdp_m_line_index,
            } => format!(
                r#"{{"type":"candidate","from":{from},"to":{to},"candidate":{},"sdp_mid":{},"sdp_m_line_index":{}}}"#,
                json_quote(candidate),
                json_quote(sdp_mid),
                sdp_m_line_index
            ),
            Self::PublicAddr { from, to, addr } => {
                format!(
                    r#"{{"type":"public-addr","from":{from},"to":{to},"addr":{}}}"#,
                    json_quote(addr)
                )
            }
            Self::Error { reason } => {
                format!(r#"{{"type":"error","reason":{}}}"#, json_quote(reason))
            }
            Self::ListRooms { rom_hash } => {
                format!(
                    r#"{{"type":"list-rooms","rom_hash":{}}}"#,
                    json_quote(rom_hash)
                )
            }
            Self::RoomList { rooms } => {
                let mut out = String::from(r#"{"type":"room-list","rooms":["#);
                for (i, r) in rooms.iter().take(MAX_ROOM_LIST).enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    out.push_str(&r.to_json_object());
                }
                out.push_str("]}");
                out
            }
            Self::QuickMatch {
                rom_hash,
                max_players,
            } => format!(
                r#"{{"type":"quick-match","rom_hash":{},"max_players":{max_players}}}"#,
                json_quote(rom_hash)
            ),
            Self::Matched {
                room,
                slot,
                max_players,
            } => format!(
                r#"{{"type":"matched","room":{},"slot":{slot},"max_players":{max_players}}}"#,
                json_quote(room)
            ),
        }
    }
}

/// Split the `rooms` array of a `room-list` frame into [`RoomInfo`]s.
///
/// Extracts the `"rooms":[ ... ]` array substring, walks it at brace depth 1 to
/// slice out each top-level `{ ... }` element, and parses each with
/// [`RoomInfo::parse_object`]. Malformed elements are skipped (never panic); the
/// count is capped at [`MAX_ROOM_LIST`] so an oversized frame cannot force an
/// unbounded allocation. Depth tracking ignores braces inside quoted strings so
/// a `rom_hash`/code value containing a brace cannot desync the walk.
fn parse_room_array(json: &str) -> Vec<RoomInfo> {
    let Some(arr_start) = field_value_start(json, "rooms") else {
        return Vec::new();
    };
    // The value must open with '['.
    let Some(after_bracket) = arr_start.strip_prefix('[') else {
        return Vec::new();
    };

    let mut rooms = Vec::new();
    let mut depth = 0i32;
    let mut obj_start: Option<usize> = None;
    let mut in_string = false;
    let mut escaped = false;
    for (i, c) in after_bracket.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        match c {
            '"' => in_string = true,
            '{' => {
                if depth == 0 {
                    obj_start = Some(i);
                }
                depth += 1;
            }
            '}' => {
                depth -= 1;
                if depth == 0
                    && let Some(start) = obj_start.take()
                    && let Some(room) = RoomInfo::parse_object(&after_bracket[start..=i])
                {
                    rooms.push(room);
                    if rooms.len() >= MAX_ROOM_LIST {
                        break;
                    }
                }
            }
            // The closing ']' at depth 0 ends the array.
            ']' if depth == 0 => break,
            _ => {}
        }
    }
    rooms
}

/// One action the async layer must perform after [`Relay::handle`]: send a
/// message to a specific client, or close a client.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// Send `msg` to client `to`.
    Send {
        /// The destination client.
        to: ClientId,
        /// The message to send.
        msg: SignalMessage,
    },
    /// Close client `who` (after delivering any preceding [`Action::Send`]s to
    /// it). Used to reject a room-full / rom-mismatch joiner.
    Close {
        /// The client to close.
        who: ClientId,
    },
}

/// One room: up to `max_players` peers (each assigned the next free slot; the
/// lower slot of any pair is the WebRTC offerer) and the `rom_hash` the first
/// joiner announced.
#[derive(Debug, Default)]
struct Room {
    /// The clients in the room, in slot order. `slots[i]` is the peer at slot
    /// `i`; `slots[0]` joined first.
    slots: Vec<ClientId>,
    /// The rom hash the first joiner announced (peers must match it).
    rom_hash: String,
    /// The total player count the first joiner requested (2..=4). Once the room
    /// holds this many peers it is full and further joiners are rejected.
    max_players: u8,
}

/// The pure signaling **relay**: room bookkeeping + the routing decision, with
/// no I/O. The async server feeds it `(client, message)` events and a
/// disconnect event; it returns the [`Action`]s to perform.
#[derive(Debug, Default)]
pub struct Relay {
    rooms: HashMap<String, Room>,
    /// Reverse index: which room each connected client is in (for O(1)
    /// disconnect handling + relay).
    client_room: HashMap<ClientId, String>,
    /// Monotonic counter feeding the generated room codes for matchmaking
    /// ([`SignalMessage::QuickMatch`] rooms). Deterministic (no RNG in this
    /// I/O-free core), so the server behaves reproducibly in tests.
    quick_match_seq: u64,
}

impl Relay {
    /// A fresh relay with no rooms.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The number of active rooms (diagnostic / tests).
    #[must_use]
    pub fn room_count(&self) -> usize {
        self.rooms.len()
    }

    /// Handle one inbound `msg` from `client`. Returns the actions to perform.
    ///
    /// - A `Join` adds the client to the room at the next free slot; a joiner
    ///   past `max_players` or a rom-hash mismatch is rejected (`Error` +
    ///   `Close`). The newcomer is told its slot (`Joined`); every *existing*
    ///   peer is nudged with `PeerJoined { slot: newcomer }` so it offers to the
    ///   newcomer (lower slot offers to higher).
    /// - An `Offer` / `Answer` / `Candidate` carries a `to` slot and is
    ///   **relayed to that specific peer** in the same room (a 2-peer room with
    ///   a legacy `to = 0` falls back to "the other peer").
    /// - Anything else from a client (server→client message types) is ignored.
    #[must_use]
    pub fn handle(&mut self, client: ClientId, msg: SignalMessage) -> Vec<Action> {
        match msg {
            SignalMessage::Join {
                room,
                rom_hash,
                max_players,
            } => self.handle_join(client, &room, &rom_hash, max_players),
            relayable @ (SignalMessage::Offer { .. }
            | SignalMessage::Answer { .. }
            | SignalMessage::Candidate { .. }
            | SignalMessage::PublicAddr { .. }) => self.relay(client, &relayable),
            SignalMessage::ListRooms { rom_hash } => self.handle_list_rooms(client, &rom_hash),
            SignalMessage::QuickMatch {
                rom_hash,
                max_players,
            } => self.handle_quick_match(client, &rom_hash, max_players),
            // Server→client message types arriving FROM a client are not
            // expected; ignore them rather than trust them.
            _ => Vec::new(),
        }
    }

    /// Handle a client disconnect: remove it from its room, notify every
    /// remaining peer with `PeerLeft { slot }` (the departed peer's slot), and
    /// drop the room if now empty. A mid-session disconnect is terminal for the
    /// mesh (the rollback session cannot continue a player short), so the
    /// remaining peers' slots are left compacted.
    #[must_use]
    pub fn disconnect(&mut self, client: ClientId) -> Vec<Action> {
        let Some(room_code) = self.client_room.remove(&client) else {
            return Vec::new();
        };
        let mut actions = Vec::new();
        if let Some(room) = self.rooms.get_mut(&room_code) {
            let departed_slot = room
                .slots
                .iter()
                .position(|&c| c == client)
                .and_then(|i| u8::try_from(i).ok())
                .unwrap_or(0);
            room.slots.retain(|&c| c != client);
            for &peer in &room.slots {
                actions.push(Action::Send {
                    to: peer,
                    msg: SignalMessage::PeerLeft {
                        slot: departed_slot,
                    },
                });
            }
            if room.slots.is_empty() {
                self.rooms.remove(&room_code);
            }
        }
        actions
    }

    fn handle_join(
        &mut self,
        client: ClientId,
        room_code: &str,
        rom_hash: &str,
        req_max_players: u8,
    ) -> Vec<Action> {
        match self.add_to_room(client, room_code, rom_hash, req_max_players) {
            Ok((slot, max_players, existing_peers)) => {
                Self::join_actions(client, slot, max_players, &existing_peers, None)
            }
            Err(reason) => Self::reject(client, reason),
        }
    }

    /// Reply to a [`SignalMessage::ListRooms`] with the open, joinable rooms —
    /// the lobby directory (v2.2.0). Only sent to the requester. An optional
    /// non-empty `rom_hash` filter restricts the listing to rooms playing that
    /// game. Full rooms are omitted (a browse-and-join UI only shows enterable
    /// rooms). Capped at [`MAX_ROOM_LIST`].
    fn handle_list_rooms(&self, client: ClientId, rom_hash: &str) -> Vec<Action> {
        vec![Action::Send {
            to: client,
            msg: SignalMessage::RoomList {
                rooms: self.open_rooms(rom_hash),
            },
        }]
    }

    /// Handle a [`SignalMessage::QuickMatch`]: join the client to any open room
    /// playing `rom_hash`, or create a fresh room if none exists (v2.2.0). The
    /// client learns the resolved room via [`SignalMessage::Matched`]; existing
    /// peers get the usual `PeerJoined` nudge.
    fn handle_quick_match(
        &mut self,
        client: ClientId,
        rom_hash: &str,
        max_players: u8,
    ) -> Vec<Action> {
        // Prefer an existing open room for this exact game with a free slot.
        let target = self
            .open_rooms(rom_hash)
            .into_iter()
            .find(|r| r.is_joinable() && (rom_hash.is_empty() || r.rom_hash == rom_hash))
            .map(|r| r.code);

        let room_code = target.unwrap_or_else(|| self.next_room_code());
        match self.add_to_room(client, &room_code, rom_hash, max_players) {
            Ok((slot, max, existing_peers)) => {
                Self::join_actions(client, slot, max, &existing_peers, Some(room_code))
            }
            // A race (the chosen room filled between selection and add) falls
            // back to a rejection the client can retry — never panics.
            Err(reason) => Self::reject(client, reason),
        }
    }

    /// The shared room-entry primitive: create/lookup the room, enforce capacity
    /// + rom-hash matching, add the client, and return `(slot, max_players,
    /// existing_peers)` — or an error reason for a full / mismatched room.
    fn add_to_room(
        &mut self,
        client: ClientId,
        room_code: &str,
        rom_hash: &str,
        req_max_players: u8,
    ) -> Result<(u8, u8, Vec<ClientId>), &'static str> {
        let room = self.rooms.entry(room_code.to_string()).or_default();

        // The first joiner sets the room size (clamped 2..=4); later joiners
        // inherit it.
        if room.slots.is_empty() {
            room.max_players = req_max_players.clamp(2, 4);
        }
        let max_players = room.max_players;

        // Reject a joiner past the room's player count. Drop a freshly-created
        // but now-unused room so a rejected QuickMatch race leaves no ghost.
        if room.slots.len() >= usize::from(max_players) {
            if room.slots.is_empty() {
                self.rooms.remove(room_code);
            }
            return Err("room full");
        }

        // The first joiner sets the room's rom hash; the rest must match it
        // (a non-empty mismatch is rejected; an empty hash skips the check).
        if room.slots.is_empty() {
            room.rom_hash = rom_hash.to_string();
        } else if !room.rom_hash.is_empty() && !rom_hash.is_empty() && room.rom_hash != rom_hash {
            return Err("rom mismatch");
        }

        let slot = u8::try_from(room.slots.len()).unwrap_or(u8::MAX);
        let existing_peers: Vec<ClientId> = room.slots.clone();
        room.slots.push(client);
        self.client_room.insert(client, room_code.to_string());
        Ok((slot, max_players, existing_peers))
    }

    /// Build the actions for a successful room entry: tell the newcomer its slot
    /// (via [`Matched`](SignalMessage::Matched) when `matched_room` is set — the
    /// `QuickMatch` path — else [`Joined`](SignalMessage::Joined)), then nudge each
    /// existing peer with `PeerJoined` (lower slot offers to higher).
    fn join_actions(
        client: ClientId,
        slot: u8,
        max_players: u8,
        existing_peers: &[ClientId],
        matched_room: Option<String>,
    ) -> Vec<Action> {
        let self_msg = matched_room.map_or(SignalMessage::Joined { slot, max_players }, |room| {
            SignalMessage::Matched {
                room,
                slot,
                max_players,
            }
        });
        let mut actions = vec![Action::Send {
            to: client,
            msg: self_msg,
        }];
        for &peer in existing_peers {
            actions.push(Action::Send {
                to: peer,
                msg: SignalMessage::PeerJoined { slot },
            });
        }
        actions
    }

    /// A room-full / rom-mismatch rejection: an `Error` then `Close`.
    fn reject(client: ClientId, reason: &str) -> Vec<Action> {
        vec![
            Action::Send {
                to: client,
                msg: SignalMessage::Error {
                    reason: reason.to_string(),
                },
            },
            Action::Close { who: client },
        ]
    }

    /// The open (has a free slot), optionally `rom_hash`-filtered rooms as
    /// [`RoomInfo`]s, capped at [`MAX_ROOM_LIST`]. The order is unspecified
    /// (`HashMap` iteration); a client sorts for display.
    #[must_use]
    pub fn open_rooms(&self, rom_hash_filter: &str) -> Vec<RoomInfo> {
        self.rooms
            .iter()
            .filter(|(_, r)| !r.slots.is_empty() && r.slots.len() < usize::from(r.max_players))
            .filter(|(_, r)| rom_hash_filter.is_empty() || r.rom_hash == rom_hash_filter)
            .take(MAX_ROOM_LIST)
            .map(|(code, r)| RoomInfo {
                code: code.clone(),
                players: u8::try_from(r.slots.len()).unwrap_or(u8::MAX),
                max_players: r.max_players,
                rom_hash: r.rom_hash.clone(),
            })
            .collect()
    }

    /// Generate the next unused matchmaking room code from the monotonic
    /// counter, e.g. `QM-000001`. Bumps until an unused code is found so a
    /// generated code never collides with a live room.
    fn next_room_code(&mut self) -> String {
        loop {
            self.quick_match_seq = self.quick_match_seq.wrapping_add(1);
            let code = format!("QM-{:06}", self.quick_match_seq);
            if !self.rooms.contains_key(&code) {
                return code;
            }
        }
    }

    /// Relay an `Offer` / `Answer` / `Candidate` to the peer named by its `to`
    /// slot. If `to` is the sender's own slot (the legacy 2-peer form encoded
    /// `to = 0`), fall back to forwarding to every *other* peer — which in a
    /// 2-peer room is exactly the one other peer.
    fn relay(&self, client: ClientId, msg: &SignalMessage) -> Vec<Action> {
        let Some(room_code) = self.client_room.get(&client) else {
            return Vec::new();
        };
        let Some(room) = self.rooms.get(room_code) else {
            return Vec::new();
        };
        let sender_slot = room.slots.iter().position(|&c| c == client);
        let to_slot = signal_to_slot(msg);

        // Route to the explicit `to` slot, unless it names the sender itself
        // (legacy 2-peer fallback) — then broadcast to the other peer(s).
        let explicit = match (to_slot, sender_slot) {
            (Some(to), Some(s)) if usize::from(to) != s => room.slots.get(usize::from(to)).copied(),
            _ => None,
        };

        explicit.map_or_else(
            || {
                room.slots
                    .iter()
                    .filter(|&&c| c != client)
                    .map(|&peer| Action::Send {
                        to: peer,
                        msg: msg.clone(),
                    })
                    .collect()
            },
            |peer| {
                vec![Action::Send {
                    to: peer,
                    msg: msg.clone(),
                }]
            },
        )
    }
}

/// The `to` slot a relayable signaling message targets, if any.
const fn signal_to_slot(msg: &SignalMessage) -> Option<u8> {
    match msg {
        SignalMessage::Offer { to, .. }
        | SignalMessage::Answer { to, .. }
        | SignalMessage::Candidate { to, .. }
        | SignalMessage::PublicAddr { to, .. } => Some(*to),
        _ => None,
    }
}

// ── minimal dependency-free JSON helpers ────────────────────────────────────
// Just enough to read a flat object's string/number fields and quote a string.
// The signaling messages are flat JSON objects, so a full parser is overkill.

/// Quote + escape a string into a JSON string literal (handles `"`, `\`, and
/// control chars — enough for SDP / candidate blobs, which can contain newlines).
fn json_quote(s: &str) -> String {
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
                use std::fmt::Write as _;
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Find the start of the value for `"key":` in a flat JSON object. Crucially,
/// the key token must be the one **followed by a colon** — so a string *value*
/// that happens to equal the key name (e.g. `"type":"candidate"` when looking up
/// the `candidate` key) is skipped, not mistaken for the key. Returns the slice
/// starting at the first non-space char after the colon.
fn field_value_start<'a>(json: &'a str, key: &str) -> Option<&'a str> {
    let needle = format!("\"{key}\"");
    let mut from = 0;
    while let Some(rel) = json[from..].find(&needle) {
        let after = from + rel + needle.len();
        let rest = json[after..].trim_start();
        if let Some(value) = rest.strip_prefix(':') {
            return Some(value.trim_start());
        }
        // This occurrence was a value, not a key — keep searching past it.
        from = after;
    }
    None
}

/// Extract a string field `"key":"value"` from a flat JSON object, unescaping
/// the basic escapes. Returns `None` if the key is absent or not a string.
fn json_str_field(json: &str, key: &str) -> Option<String> {
    let rest = field_value_start(json, key)?;
    let mut chars = rest.char_indices();
    // Must open with a quote to be a string value.
    if chars.next()?.1 != '"' {
        return None;
    }
    let mut out = String::new();
    let mut escaped = false;
    for (_, c) in chars {
        if escaped {
            match c {
                'n' => out.push('\n'),
                'r' => out.push('\r'),
                't' => out.push('\t'),
                other => out.push(other),
            }
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c == '"' {
            return Some(out);
        } else {
            out.push(c);
        }
    }
    None
}

/// Extract a non-negative integer field `"key":N` from a flat JSON object.
fn json_num_field(json: &str, key: &str) -> Option<u64> {
    let rest = field_value_start(json, key)?;
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() {
        None
    } else {
        digits.parse().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_roundtrips() {
        let msgs = [
            SignalMessage::Join {
                room: "abc123".into(),
                rom_hash: "deadbeef".into(),
                max_players: 4,
            },
            SignalMessage::Joined {
                slot: 1,
                max_players: 3,
            },
            SignalMessage::PeerJoined { slot: 2 },
            SignalMessage::PeerLeft { slot: 1 },
            SignalMessage::Offer {
                from: 0,
                to: 2,
                sdp: "v=0\r\no=- 1 2 IN IP4 0.0.0.0\r\n".into(),
            },
            SignalMessage::Answer {
                from: 2,
                to: 0,
                sdp: "answer-sdp".into(),
            },
            SignalMessage::Candidate {
                from: 1,
                to: 3,
                candidate: "candidate:1 1 udp 2122260223 192.168.1.5 50000 typ host".into(),
                sdp_mid: "0".into(),
                sdp_m_line_index: 0,
            },
            SignalMessage::PublicAddr {
                from: 0,
                to: 1,
                addr: "203.0.113.7:51234".into(),
            },
            SignalMessage::Error {
                reason: "room full".into(),
            },
        ];
        for m in &msgs {
            let json = m.to_json();
            let back =
                SignalMessage::parse(&json).unwrap_or_else(|| panic!("parse failed for {json}"));
            assert_eq!(*m, back, "roundtrip mismatch for {json}");
        }
    }

    #[test]
    fn public_addr_relays_to_the_named_slot() {
        // The native raw-UDP rendezvous rides the same slot-routed relay as the
        // browser SDP messages: peer at slot 0 sends its STUN-discovered public
        // address specifically to the peer at slot 1.
        let mut relay = Relay::new();
        let _ = relay.handle(1, join("r", "h"));
        let _ = relay.handle(2, join("r", "h"));
        let pub_addr = SignalMessage::PublicAddr {
            from: 0,
            to: 1,
            addr: "198.51.100.9:40000".into(),
        };
        let acts = relay.handle(1, pub_addr.clone());
        assert_eq!(
            acts,
            vec![Action::Send {
                to: 2,
                msg: pub_addr
            }]
        );

        // And the reverse direction routes back to slot 0 (client 1).
        let reply = SignalMessage::PublicAddr {
            from: 1,
            to: 0,
            addr: "192.0.2.5:55555".into(),
        };
        let acts = relay.handle(2, reply.clone());
        assert_eq!(acts, vec![Action::Send { to: 1, msg: reply }]);
    }

    #[test]
    fn parse_rejects_garbage() {
        assert!(SignalMessage::parse("not json").is_none());
        assert!(SignalMessage::parse(r#"{"type":"bogus"}"#).is_none());
        assert!(SignalMessage::parse(r#"{"type":"offer"}"#).is_none()); // missing sdp
        assert!(SignalMessage::parse("{}").is_none());
    }

    #[test]
    fn quotes_with_embedded_newlines_and_quotes() {
        let sdp = "line1\r\nline2 \"quoted\" \\backslash";
        let m = SignalMessage::Offer {
            from: 1,
            to: 2,
            sdp: sdp.into(),
        };
        let back = SignalMessage::parse(&m.to_json()).unwrap();
        assert_eq!(back, m);
    }

    #[test]
    fn two_peers_pair_in_a_room() {
        let mut relay = Relay::new();
        // Peer 1 joins.
        let a1 = relay.handle(1, join("r", "hash"));
        assert_eq!(
            a1,
            vec![Action::Send {
                to: 1,
                msg: SignalMessage::Joined {
                    slot: 0,
                    max_players: 2
                }
            }]
        );

        // Peer 2 joins → it learns its slot; the existing peer 1 is nudged to
        // offer to the newcomer. The newcomer does NOT get a PeerJoined.
        let a2 = relay.handle(2, join("r", "hash"));
        assert!(a2.contains(&Action::Send {
            to: 2,
            msg: SignalMessage::Joined {
                slot: 1,
                max_players: 2
            }
        }));
        assert!(a2.contains(&Action::Send {
            to: 1,
            msg: SignalMessage::PeerJoined { slot: 1 }
        }));
        assert!(!a2.iter().any(|a| matches!(
            a,
            Action::Send {
                to: 2,
                msg: SignalMessage::PeerJoined { .. }
            }
        )));
        assert_eq!(relay.room_count(), 1);
    }

    #[test]
    fn offer_answer_candidate_relay_to_the_other_peer() {
        let mut relay = Relay::new();
        let _ = relay.handle(1, join("r", "h"));
        let _ = relay.handle(2, join("r", "h"));

        // Legacy 2-peer form: to = 0 names the offerer's own slot, so it falls
        // back to "the other peer".
        let offer = SignalMessage::Offer {
            from: 0,
            to: 1,
            sdp: "o".into(),
        };
        let acts = relay.handle(1, offer.clone());
        assert_eq!(acts, vec![Action::Send { to: 2, msg: offer }]);

        let answer = SignalMessage::Answer {
            from: 1,
            to: 0,
            sdp: "a".into(),
        };
        let acts = relay.handle(2, answer.clone());
        assert_eq!(acts, vec![Action::Send { to: 1, msg: answer }]);

        let cand = SignalMessage::Candidate {
            from: 0,
            to: 1,
            candidate: "c".into(),
            sdp_mid: "0".into(),
            sdp_m_line_index: 0,
        };
        let acts = relay.handle(1, cand.clone());
        assert_eq!(acts, vec![Action::Send { to: 2, msg: cand }]);
    }

    #[test]
    fn joiner_past_room_size_is_rejected() {
        let mut relay = Relay::new();
        let _ = relay.handle(1, join("r", "h"));
        let _ = relay.handle(2, join("r", "h"));
        let acts = relay.handle(3, join("r", "h"));
        assert!(acts.contains(&Action::Close { who: 3 }));
        assert!(acts.iter().any(|a| matches!(
            a,
            Action::Send {
                to: 3,
                msg: SignalMessage::Error { .. }
            }
        )));
    }

    #[test]
    fn four_peer_mesh_assigns_slots_and_nudges_existing_peers() {
        let mut relay = Relay::new();
        // Peer 1 opens a 4-player room.
        let a1 = relay.handle(1, join_n("r", "h", 4));
        assert!(a1.contains(&Action::Send {
            to: 1,
            msg: SignalMessage::Joined {
                slot: 0,
                max_players: 4
            }
        }));

        // Peer 2 (slot 1): existing peer 1 is nudged with PeerJoined{slot:1}.
        let a2 = relay.handle(2, join_n("r", "h", 4));
        assert!(a2.contains(&Action::Send {
            to: 1,
            msg: SignalMessage::PeerJoined { slot: 1 }
        }));

        // Peer 3 (slot 2): both existing peers 1 and 2 are nudged.
        let a3 = relay.handle(3, join_n("r", "h", 4));
        assert!(a3.contains(&Action::Send {
            to: 1,
            msg: SignalMessage::PeerJoined { slot: 2 }
        }));
        assert!(a3.contains(&Action::Send {
            to: 2,
            msg: SignalMessage::PeerJoined { slot: 2 }
        }));

        // Peer 4 (slot 3): all three existing peers nudged; room now full.
        let a4 = relay.handle(4, join_n("r", "h", 4));
        for existing in [1, 2, 3] {
            assert!(a4.contains(&Action::Send {
                to: existing,
                msg: SignalMessage::PeerJoined { slot: 3 }
            }));
        }
        assert!(a4.contains(&Action::Send {
            to: 4,
            msg: SignalMessage::Joined {
                slot: 3,
                max_players: 4
            }
        }));

        // A 5th joiner is rejected (room full).
        let a5 = relay.handle(5, join_n("r", "h", 4));
        assert!(a5.contains(&Action::Close { who: 5 }));
    }

    #[test]
    fn mesh_relays_offer_to_the_named_slot_only() {
        let mut relay = Relay::new();
        for id in [1, 2, 3] {
            let _ = relay.handle(id, join_n("r", "h", 3));
        }
        // Peer at slot 0 (client 1) offers specifically to slot 2 (client 3).
        let offer = SignalMessage::Offer {
            from: 0,
            to: 2,
            sdp: "o".into(),
        };
        let acts = relay.handle(1, offer.clone());
        assert_eq!(acts, vec![Action::Send { to: 3, msg: offer }]);

        // Slot 2 (client 3) answers slot 0 (client 1) — not client 2.
        let answer = SignalMessage::Answer {
            from: 2,
            to: 0,
            sdp: "a".into(),
        };
        let acts = relay.handle(3, answer.clone());
        assert_eq!(acts, vec![Action::Send { to: 1, msg: answer }]);
    }

    #[test]
    fn rom_mismatch_is_rejected() {
        let mut relay = Relay::new();
        let _ = relay.handle(1, join("r", "hashA"));
        let acts = relay.handle(2, join("r", "hashB"));
        assert!(acts.contains(&Action::Close { who: 2 }));
    }

    #[test]
    fn disconnect_notifies_peer_and_cleans_room() {
        let mut relay = Relay::new();
        let _ = relay.handle(1, join("r", "h"));
        let _ = relay.handle(2, join("r", "h"));
        let acts = relay.disconnect(1);
        assert_eq!(
            acts,
            vec![Action::Send {
                to: 2,
                msg: SignalMessage::PeerLeft { slot: 0 }
            }]
        );
        // Room still alive with peer 2.
        assert_eq!(relay.room_count(), 1);
        // Peer 2 leaves → room is dropped.
        let acts = relay.disconnect(2);
        assert!(acts.is_empty());
        assert_eq!(relay.room_count(), 0);
    }

    #[test]
    fn rejoin_after_room_emptied_works() {
        let mut relay = Relay::new();
        let _ = relay.handle(1, join("r", "h"));
        let _ = relay.disconnect(1);
        // A fresh peer can take slot 0 again.
        let acts = relay.handle(9, join("r", "h2"));
        assert_eq!(
            acts,
            vec![Action::Send {
                to: 9,
                msg: SignalMessage::Joined {
                    slot: 0,
                    max_players: 2
                }
            }]
        );
    }

    #[test]
    fn legacy_join_without_max_players_defaults_to_two() {
        // A legacy client's Join omitted max_players; parse defaults it to 2.
        let parsed = SignalMessage::parse(r#"{"type":"join","room":"r","rom_hash":"h"}"#).unwrap();
        assert_eq!(
            parsed,
            SignalMessage::Join {
                room: "r".into(),
                rom_hash: "h".into(),
                max_players: 2
            }
        );
    }

    #[test]
    fn lobby_messages_roundtrip() {
        let msgs = [
            SignalMessage::ListRooms {
                rom_hash: "deadbeef".into(),
            },
            SignalMessage::ListRooms {
                rom_hash: String::new(),
            },
            SignalMessage::RoomList { rooms: Vec::new() },
            SignalMessage::RoomList {
                rooms: vec![
                    RoomInfo {
                        code: "AB12CD".into(),
                        players: 1,
                        max_players: 2,
                        rom_hash: "deadbeef".into(),
                    },
                    RoomInfo {
                        code: "QM-000001".into(),
                        players: 3,
                        max_players: 4,
                        rom_hash: String::new(),
                    },
                ],
            },
            SignalMessage::QuickMatch {
                rom_hash: "cafe".into(),
                max_players: 3,
            },
            SignalMessage::Matched {
                room: "QM-000007".into(),
                slot: 2,
                max_players: 4,
            },
        ];
        for m in &msgs {
            let json = m.to_json();
            let back =
                SignalMessage::parse(&json).unwrap_or_else(|| panic!("parse failed for {json}"));
            assert_eq!(*m, back, "roundtrip mismatch for {json}");
        }
    }

    #[test]
    fn list_rooms_returns_only_open_matching_rooms() {
        let mut relay = Relay::new();
        // A 2-player room for game "aa" with one peer (open).
        let _ = relay.handle(1, join("open", "aa"));
        // A full 2-player room for game "bb".
        let _ = relay.handle(2, join("full", "bb"));
        let _ = relay.handle(3, join("full", "bb"));

        // Unfiltered: only the open room is listed (the full one is omitted).
        let acts = relay.handle(
            1,
            SignalMessage::ListRooms {
                rom_hash: String::new(),
            },
        );
        let Action::Send {
            to: 1,
            msg: SignalMessage::RoomList { rooms },
        } = &acts[0]
        else {
            panic!("expected a room-list reply, got {acts:?}");
        };
        assert_eq!(rooms.len(), 1);
        assert_eq!(rooms[0].code, "open");
        assert_eq!(rooms[0].players, 1);
        assert!(rooms[0].is_joinable());

        // Filtered to game "bb": the only "bb" room is full, so the list is empty.
        let acts = relay.handle(
            1,
            SignalMessage::ListRooms {
                rom_hash: "bb".into(),
            },
        );
        let Action::Send {
            msg: SignalMessage::RoomList { rooms },
            ..
        } = &acts[0]
        else {
            panic!("expected a room-list reply");
        };
        assert!(rooms.is_empty(), "the matching room is full → omitted");
    }

    #[test]
    fn quick_match_joins_an_open_room_then_creates_one() {
        let mut relay = Relay::new();
        // Host opens a room for game "aa".
        let _ = relay.handle(1, join_n("host", "aa", 2));

        // A quick-match for "aa" lands in the existing open room at slot 1 and
        // the existing peer is nudged.
        let acts = relay.handle(
            2,
            SignalMessage::QuickMatch {
                rom_hash: "aa".into(),
                max_players: 2,
            },
        );
        assert!(acts.contains(&Action::Send {
            to: 2,
            msg: SignalMessage::Matched {
                room: "host".into(),
                slot: 1,
                max_players: 2,
            },
        }));
        assert!(acts.contains(&Action::Send {
            to: 1,
            msg: SignalMessage::PeerJoined { slot: 1 },
        }));

        // A quick-match for a DIFFERENT game finds no open room → a new one is
        // created (a generated QM- code) with the client at slot 0.
        let acts = relay.handle(
            3,
            SignalMessage::QuickMatch {
                rom_hash: "zz".into(),
                max_players: 2,
            },
        );
        let Action::Send {
            to: 3,
            msg:
                SignalMessage::Matched {
                    room,
                    slot,
                    max_players,
                },
        } = &acts[0]
        else {
            panic!("expected a matched reply, got {acts:?}");
        };
        assert!(
            room.starts_with("QM-"),
            "created room has a generated code: {room}"
        );
        assert_eq!(*slot, 0);
        assert_eq!(*max_players, 2);
    }

    fn join(room: &str, hash: &str) -> SignalMessage {
        join_n(room, hash, 2)
    }

    fn join_n(room: &str, hash: &str, max_players: u8) -> SignalMessage {
        SignalMessage::Join {
            room: room.into(),
            rom_hash: hash.into(),
            max_players,
        }
    }
}
