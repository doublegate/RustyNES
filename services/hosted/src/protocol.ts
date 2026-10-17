// SPDX-License-Identifier: GPL-3.0-or-later
//
// The netplay signalling room protocol, ported from RustyNES's own reference
// implementation, `crates/rustynes-netplay/src/signaling.rs` (`SignalMessage`
// + `Relay`), which `examples/signaling_server.rs` drives. Both are RustyNES
// code under the same licence; nothing here comes from another project.
//
// # What "byte-compatible" means here
//
// The reference server is a thin loop: every inbound text frame goes through
// `SignalMessage::parse`; a frame that parses is handed to `Relay::handle`;
// every resulting `Action::Send` goes out as `SignalMessage::to_json`. So a
// client sees exactly (a) which frames are accepted, (b) the routing decision
// and (c) the re-encoded text. This module reproduces all three, including the
// reference parser's leniencies, because a client that happens to depend on
// one of them must keep working against the hosted lobby:
//
// - keys are found by the first `"key"` token followed by a colon, so a string
//   VALUE equal to a key name is skipped (`"type":"candidate"`);
// - string unescaping understands only `\n`, `\r` and `\t`; any other escaped
//   character is kept literally (`\"` -> `"`, `\/` -> `/`, and `A` -> the
//   five characters `u0041`, NOT `A`);
// - numbers are the leading ASCII digits, parsed as a u64 (overflow = absent),
//   then narrowed: a slot that does not fit a u8 reads as 0, a `max_players`
//   that does not fit reads as 2, an `sdp_m_line_index` that does not fit a u32
//   rejects the whole frame;
// - whitespace around the colon is Rust's `char::is_whitespace`, which unlike
//   JavaScript's `trimStart` does not include U+FEFF.
//
// The conformance vectors in `test/vectors/signaling-vectors.json` are
// generated FROM the Rust code (crates/rustynes-netplay/tests/
// hosted_signaling_vectors.rs) and replayed against this port, so every one of
// these statements is checked rather than asserted.
//
// The one place the two can legitimately differ is iteration order: Rust keeps
// rooms in a randomised `HashMap`, this port in an insertion-ordered `Map`, so
// a `room-list` with several open rooms, or a `quick-match` with several
// candidate rooms, may pick a different (equally valid) order. The reference
// has no defined order there.

/** Mirrors `rustynes_netplay::signaling::MAX_ROOM_LIST`. */
export const MAX_ROOM_LIST = 256;

export type ClientId = number;

export interface RoomInfo {
  code: string;
  players: number;
  maxPlayers: number;
  romHash: string;
}

export type SignalMessage =
  | { type: "join"; room: string; romHash: string; maxPlayers: number }
  | { type: "joined"; slot: number; maxPlayers: number }
  | { type: "peer-joined"; slot: number }
  | { type: "peer-left"; slot: number }
  | { type: "offer"; from: number; to: number; sdp: string }
  | { type: "answer"; from: number; to: number; sdp: string }
  | {
      type: "candidate";
      from: number;
      to: number;
      candidate: string;
      sdpMid: string;
      sdpMLineIndex: number;
    }
  | { type: "public-addr"; from: number; to: number; addr: string }
  | { type: "error"; reason: string }
  | { type: "list-rooms"; romHash: string }
  | { type: "room-list"; rooms: RoomInfo[] }
  | { type: "quick-match"; romHash: string; maxPlayers: number }
  | { type: "matched"; room: string; slot: number; maxPlayers: number };

export type Action =
  | { kind: "send"; to: ClientId; msg: SignalMessage }
  | { kind: "close"; who: ClientId };

// ── the reference's minimal JSON helpers ────────────────────────────────────

/** Rust's `char::is_whitespace` (the Unicode White_Space property). */
function isRustWhitespace(cp: number): boolean {
  return (
    (cp >= 0x09 && cp <= 0x0d) ||
    cp === 0x20 ||
    cp === 0x85 ||
    cp === 0xa0 ||
    cp === 0x1680 ||
    (cp >= 0x2000 && cp <= 0x200a) ||
    cp === 0x2028 ||
    cp === 0x2029 ||
    cp === 0x202f ||
    cp === 0x205f ||
    cp === 0x3000
  );
}

/** `str::trim_start` with Rust's whitespace definition. */
function trimStartRust(s: string): string {
  let i = 0;
  while (i < s.length) {
    const cp = s.codePointAt(i)!;
    if (!isRustWhitespace(cp)) break;
    i += cp > 0xffff ? 2 : 1;
  }
  return s.slice(i);
}

/**
 * `field_value_start`: the value after the first `"key"` token that is
 * followed (after whitespace) by a colon; a token that is a value is skipped.
 */
function fieldValueStart(json: string, key: string): string | undefined {
  const needle = `"${key}"`;
  let from = 0;
  for (;;) {
    const at = json.indexOf(needle, from);
    if (at < 0) return undefined;
    const after = at + needle.length;
    const rest = trimStartRust(json.slice(after));
    if (rest.startsWith(":")) return trimStartRust(rest.slice(1));
    from = after;
  }
}

/** `json_str_field`, with the reference's (deliberately small) unescaping. */
export function jsonStrField(json: string, key: string): string | undefined {
  const rest = fieldValueStart(json, key);
  if (rest === undefined || !rest.startsWith('"')) return undefined;
  let out = "";
  let escaped = false;
  // Iterate by code point, as Rust iterates `char`s.
  for (const c of rest.slice(1)) {
    if (escaped) {
      if (c === "n") out += "\n";
      else if (c === "r") out += "\r";
      else if (c === "t") out += "\t";
      else out += c;
      escaped = false;
    } else if (c === "\\") {
      escaped = true;
    } else if (c === '"') {
      return out;
    } else {
      out += c;
    }
  }
  return undefined;
}

const U64_MAX = (1n << 64n) - 1n;

/** `json_num_field`: the leading ASCII digits as a u64 (overflow = absent). */
export function jsonNumField(json: string, key: string): bigint | undefined {
  const rest = fieldValueStart(json, key);
  if (rest === undefined) return undefined;
  let digits = "";
  for (const c of rest) {
    if (c >= "0" && c <= "9") digits += c;
    else break;
  }
  if (digits === "") return undefined;
  const n = BigInt(digits);
  return n > U64_MAX ? undefined : n;
}

const toU8 = (n: bigint | undefined): number | undefined =>
  n !== undefined && n <= 255n ? Number(n) : undefined;
const toU32 = (n: bigint | undefined): number | undefined =>
  n !== undefined && n <= 0xffffffffn ? Number(n) : undefined;

/** `json_quote`: escape `"`, `\`, `\n`, `\r`, `\t`, other controls as `\u00xx`. */
export function jsonQuote(s: string): string {
  let out = '"';
  for (const c of s) {
    const cp = c.codePointAt(0)!;
    if (c === '"') out += '\\"';
    else if (c === "\\") out += "\\\\";
    else if (c === "\n") out += "\\n";
    else if (c === "\r") out += "\\r";
    else if (c === "\t") out += "\\t";
    else if (cp < 0x20) out += "\\u" + cp.toString(16).padStart(4, "0");
    else out += c;
  }
  return out + '"';
}

/** `parse_room_array`: the `rooms` array of a `room-list` frame. */
function parseRoomArray(json: string): RoomInfo[] {
  const start = fieldValueStart(json, "rooms");
  if (start === undefined || !start.startsWith("[")) return [];
  const body = start.slice(1);
  const rooms: RoomInfo[] = [];
  let depth = 0;
  let objStart: number | undefined;
  let inString = false;
  let escaped = false;
  for (let i = 0; i < body.length; i++) {
    const c = body[i];
    if (inString) {
      if (escaped) escaped = false;
      else if (c === "\\") escaped = true;
      else if (c === '"') inString = false;
      continue;
    }
    if (c === '"') inString = true;
    else if (c === "{") {
      if (depth === 0) objStart = i;
      depth += 1;
    } else if (c === "}") {
      depth -= 1;
      if (depth === 0 && objStart !== undefined) {
        const obj = body.slice(objStart, i + 1);
        objStart = undefined;
        const code = jsonStrField(obj, "room");
        const players = toU8(jsonNumField(obj, "players"));
        const maxPlayers = toU8(jsonNumField(obj, "max_players"));
        if (code !== undefined && players !== undefined && maxPlayers !== undefined) {
          rooms.push({ code, players, maxPlayers, romHash: jsonStrField(obj, "rom_hash") ?? "" });
          if (rooms.length >= MAX_ROOM_LIST) break;
        }
      }
    } else if (c === "]" && depth === 0) break;
  }
  return rooms;
}

/** `SignalMessage::parse`. `undefined` = the frame is dropped. */
export function parseSignal(json: string): SignalMessage | undefined {
  const slotOr = (key: string): number => toU8(jsonNumField(json, key)) ?? 0;
  const maxOr2 = (): number => toU8(jsonNumField(json, "max_players")) ?? 2;
  const ty = jsonStrField(json, "type");
  if (ty === undefined) return undefined;
  switch (ty) {
    case "join": {
      const room = jsonStrField(json, "room");
      if (room === undefined) return undefined;
      return { type: "join", room, romHash: jsonStrField(json, "rom_hash") ?? "", maxPlayers: maxOr2() };
    }
    case "joined": {
      const slot = toU8(jsonNumField(json, "slot"));
      if (slot === undefined) return undefined;
      return { type: "joined", slot, maxPlayers: maxOr2() };
    }
    case "peer-joined":
      return { type: "peer-joined", slot: slotOr("slot") };
    case "peer-left":
      return { type: "peer-left", slot: slotOr("slot") };
    case "offer":
    case "answer": {
      const sdp = jsonStrField(json, "sdp");
      if (sdp === undefined) return undefined;
      return { type: ty, from: slotOr("from"), to: slotOr("to"), sdp };
    }
    case "candidate": {
      const candidate = jsonStrField(json, "candidate");
      if (candidate === undefined) return undefined;
      const sdpMid = jsonStrField(json, "sdp_mid") ?? "";
      const sdpMLineIndex = toU32(jsonNumField(json, "sdp_m_line_index"));
      if (sdpMLineIndex === undefined) return undefined;
      return { type: "candidate", from: slotOr("from"), to: slotOr("to"), candidate, sdpMid, sdpMLineIndex };
    }
    case "public-addr": {
      const addr = jsonStrField(json, "addr");
      if (addr === undefined) return undefined;
      return { type: "public-addr", from: slotOr("from"), to: slotOr("to"), addr };
    }
    case "error":
      return { type: "error", reason: jsonStrField(json, "reason") ?? "" };
    case "list-rooms":
      return { type: "list-rooms", romHash: jsonStrField(json, "rom_hash") ?? "" };
    case "room-list":
      return { type: "room-list", rooms: parseRoomArray(json) };
    case "quick-match":
      return { type: "quick-match", romHash: jsonStrField(json, "rom_hash") ?? "", maxPlayers: maxOr2() };
    case "matched": {
      const room = jsonStrField(json, "room");
      const slot = toU8(jsonNumField(json, "slot"));
      if (room === undefined || slot === undefined) return undefined;
      return { type: "matched", room, slot, maxPlayers: maxOr2() };
    }
    default:
      return undefined;
  }
}

/** `SignalMessage::to_json`. */
export function encodeSignal(m: SignalMessage): string {
  switch (m.type) {
    case "join":
      return `{"type":"join","room":${jsonQuote(m.room)},"rom_hash":${jsonQuote(m.romHash)},"max_players":${m.maxPlayers}}`;
    case "joined":
      return `{"type":"joined","slot":${m.slot},"max_players":${m.maxPlayers}}`;
    case "peer-joined":
      return `{"type":"peer-joined","slot":${m.slot}}`;
    case "peer-left":
      return `{"type":"peer-left","slot":${m.slot}}`;
    case "offer":
    case "answer":
      return `{"type":"${m.type}","from":${m.from},"to":${m.to},"sdp":${jsonQuote(m.sdp)}}`;
    case "candidate":
      return `{"type":"candidate","from":${m.from},"to":${m.to},"candidate":${jsonQuote(m.candidate)},"sdp_mid":${jsonQuote(m.sdpMid)},"sdp_m_line_index":${m.sdpMLineIndex}}`;
    case "public-addr":
      return `{"type":"public-addr","from":${m.from},"to":${m.to},"addr":${jsonQuote(m.addr)}}`;
    case "error":
      return `{"type":"error","reason":${jsonQuote(m.reason)}}`;
    case "list-rooms":
      return `{"type":"list-rooms","rom_hash":${jsonQuote(m.romHash)}}`;
    case "room-list":
      return (
        `{"type":"room-list","rooms":[` +
        m.rooms
          .slice(0, MAX_ROOM_LIST)
          .map(
            (r) =>
              `{"room":${jsonQuote(r.code)},"players":${r.players},"max_players":${r.maxPlayers},"rom_hash":${jsonQuote(r.romHash)}}`,
          )
          .join(",") +
        "]}"
      );
    case "quick-match":
      return `{"type":"quick-match","rom_hash":${jsonQuote(m.romHash)},"max_players":${m.maxPlayers}}`;
    case "matched":
      return `{"type":"matched","room":${jsonQuote(m.room)},"slot":${m.slot},"max_players":${m.maxPlayers}}`;
  }
}

// ── the relay ───────────────────────────────────────────────────────────────

interface Room {
  /** `slots[i]` is the client at slot `i`, in entry order. */
  slots: ClientId[];
  romHash: string;
  maxPlayers: number;
}

/**
 * One client's room membership, as the Durable Object persists it in the
 * socket's attachment so the relay can be rebuilt after hibernation. `seq` is
 * the global entry order: slot = rank among the room's members by `seq`,
 * because the reference compacts slots with an order-preserving `retain`.
 */
export interface Membership {
  room: string;
  seq: number;
  romHash: string;
  maxPlayers: number;
}

/** A port of `rustynes_netplay::signaling::Relay`. */
export class Relay {
  private rooms = new Map<string, Room>();
  private clientRoom = new Map<ClientId, string>();
  private quickMatchSeq = 0;
  /** Entry sequence numbers (persistence only; not part of the protocol). */
  private entrySeq = 0;
  private clientSeq = new Map<ClientId, number>();
  /** Clients whose membership changed since the last `takeDirty()`. */
  private dirty = new Set<ClientId>();

  /** Rebuild from persisted memberships (after a Durable Object wakes). */
  static restore(entries: Array<{ client: ClientId; m: Membership }>): Relay {
    const r = new Relay();
    const sorted = [...entries].sort((a, b) => a.m.seq - b.m.seq);
    for (const { client, m } of sorted) {
      let room = r.rooms.get(m.room);
      if (!room) {
        room = { slots: [], romHash: m.romHash, maxPlayers: m.maxPlayers };
        r.rooms.set(m.room, room);
      }
      room.slots.push(client);
      r.clientRoom.set(client, m.room);
      r.clientSeq.set(client, m.seq);
      r.entrySeq = Math.max(r.entrySeq, m.seq + 1);
      const qm = /^QM-(\d+)$/.exec(m.room);
      if (qm) r.quickMatchSeq = Math.max(r.quickMatchSeq, Number(qm[1]));
    }
    return r;
  }

  roomCount(): number {
    return this.rooms.size;
  }

  /** The membership to persist for `client`, or `null` when in no room. */
  membership(client: ClientId): Membership | null {
    const code = this.clientRoom.get(client);
    if (code === undefined) return null;
    const room = this.rooms.get(code)!;
    return { room: code, seq: this.clientSeq.get(client)!, romHash: room.romHash, maxPlayers: room.maxPlayers };
  }

  takeDirty(): ClientId[] {
    const out = [...this.dirty];
    this.dirty.clear();
    return out;
  }

  /**
   * Would handling `msg` create a new room? Used only by the hosted lobby's
   * room-creation rate limit, which runs BEFORE `handle` so a refused
   * creation changes no state.
   */
  wouldCreateRoom(client: ClientId, msg: SignalMessage): boolean {
    if (msg.type === "join") {
      const room = this.rooms.get(msg.room);
      // Joining the only member's own room after leaving it re-creates it.
      return !room || (room.slots.length === 1 && room.slots[0] === client);
    }
    if (msg.type === "quick-match") return this.findQuickMatchTarget(client, msg.romHash) === undefined;
    return false;
  }

  handle(client: ClientId, msg: SignalMessage): Action[] {
    switch (msg.type) {
      case "join":
        return this.handleJoin(client, msg.room, msg.romHash, msg.maxPlayers);
      case "offer":
      case "answer":
      case "candidate":
      case "public-addr":
        return this.relay(client, msg);
      case "list-rooms":
        return [{ kind: "send", to: client, msg: { type: "room-list", rooms: this.openRooms(msg.romHash) } }];
      case "quick-match":
        return this.handleQuickMatch(client, msg.romHash, msg.maxPlayers);
      default:
        // Server-to-client types arriving from a client are ignored.
        return [];
    }
  }

  disconnect(client: ClientId): Action[] {
    return this.leaveRoom(client);
  }

  private leaveRoom(client: ClientId): Action[] {
    const code = this.clientRoom.get(client);
    if (code === undefined) return [];
    this.clientRoom.delete(client);
    this.clientSeq.delete(client);
    this.dirty.add(client);
    const actions: Action[] = [];
    const room = this.rooms.get(code);
    if (room) {
      const idx = room.slots.indexOf(client);
      room.slots = room.slots.filter((c) => c !== client);
      if (idx >= 0 && idx <= 255) {
        for (const peer of room.slots) actions.push({ kind: "send", to: peer, msg: { type: "peer-left", slot: idx } });
      }
      // Remaining members' slots shift down; their persisted `seq` already
      // encodes that (rank by seq), so nothing else changes for them.
      if (room.slots.length === 0) this.rooms.delete(code);
    }
    return actions;
  }

  private handleJoin(client: ClientId, code: string, romHash: string, reqMax: number): Action[] {
    const actions = this.leaveRoom(client);
    const r = this.addToRoom(client, code, romHash, reqMax);
    actions.push(...(typeof r === "string" ? Relay.reject(client, r) : Relay.joinActions(client, r, undefined)));
    return actions;
  }

  private findQuickMatchTarget(client: ClientId, romHash: string): string | undefined {
    for (const [code, r] of this.rooms) {
      const others = r.slots.filter((c) => c !== client);
      // The reference leaves the current room BEFORE choosing, so the
      // client's own room counts without the client in it.
      if (others.length > 0 && others.length < r.maxPlayers && (romHash === "" || r.romHash === romHash)) return code;
    }
    return undefined;
  }

  private handleQuickMatch(client: ClientId, romHash: string, maxPlayers: number): Action[] {
    const actions = this.leaveRoom(client);
    let target: string | undefined;
    for (const [code, r] of this.rooms) {
      if (r.slots.length > 0 && r.slots.length < r.maxPlayers && (romHash === "" || r.romHash === romHash)) {
        target = code;
        break;
      }
    }
    const code = target ?? this.nextRoomCode();
    const r = this.addToRoom(client, code, romHash, maxPlayers);
    actions.push(...(typeof r === "string" ? Relay.reject(client, r) : Relay.joinActions(client, r, code)));
    return actions;
  }

  private addToRoom(
    client: ClientId,
    code: string,
    romHash: string,
    reqMax: number,
  ): { slot: number; maxPlayers: number; existing: ClientId[] } | string {
    let room = this.rooms.get(code);
    if (!room) {
      room = { slots: [], romHash: "", maxPlayers: 0 };
      this.rooms.set(code, room);
    }
    if (room.slots.length === 0) room.maxPlayers = Math.min(4, Math.max(2, reqMax));
    const maxPlayers = room.maxPlayers;
    if (room.slots.length >= maxPlayers) return "room full";
    if (room.slots.length === 0) room.romHash = romHash;
    else if (room.romHash !== "" && romHash !== "" && room.romHash !== romHash) return "rom mismatch";
    const slot = Math.min(room.slots.length, 255);
    const existing = [...room.slots];
    room.slots.push(client);
    this.clientRoom.set(client, code);
    this.clientSeq.set(client, this.entrySeq++);
    this.dirty.add(client);
    return { slot, maxPlayers, existing };
  }

  private static joinActions(
    client: ClientId,
    r: { slot: number; maxPlayers: number; existing: ClientId[] },
    matchedRoom: string | undefined,
  ): Action[] {
    const self: SignalMessage =
      matchedRoom === undefined
        ? { type: "joined", slot: r.slot, maxPlayers: r.maxPlayers }
        : { type: "matched", room: matchedRoom, slot: r.slot, maxPlayers: r.maxPlayers };
    const actions: Action[] = [{ kind: "send", to: client, msg: self }];
    for (const peer of r.existing) actions.push({ kind: "send", to: peer, msg: { type: "peer-joined", slot: r.slot } });
    return actions;
  }

  static reject(client: ClientId, reason: string): Action[] {
    return [
      { kind: "send", to: client, msg: { type: "error", reason } },
      { kind: "close", who: client },
    ];
  }

  openRooms(filter: string): RoomInfo[] {
    const out: RoomInfo[] = [];
    for (const [code, r] of this.rooms) {
      if (r.slots.length === 0 || r.slots.length >= r.maxPlayers) continue;
      if (filter !== "" && r.romHash !== filter) continue;
      out.push({ code, players: Math.min(r.slots.length, 255), maxPlayers: r.maxPlayers, romHash: r.romHash });
      if (out.length >= MAX_ROOM_LIST) break;
    }
    return out;
  }

  private nextRoomCode(): string {
    for (;;) {
      this.quickMatchSeq += 1;
      const code = `QM-${String(this.quickMatchSeq).padStart(6, "0")}`;
      if (!this.rooms.has(code)) return code;
    }
  }

  private relay(client: ClientId, msg: SignalMessage & { to: number }): Action[] {
    const code = this.clientRoom.get(client);
    if (code === undefined) return [];
    const room = this.rooms.get(code);
    if (!room) return [];
    const sender = room.slots.indexOf(client);
    let explicit: ClientId | undefined;
    if (sender >= 0 && msg.to !== sender) explicit = room.slots[msg.to];
    if (explicit !== undefined) return [{ kind: "send", to: explicit, msg }];
    return room.slots.filter((c) => c !== client).map((peer) => ({ kind: "send" as const, to: peer, msg }));
  }
}
