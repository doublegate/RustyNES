// SPDX-License-Identifier: GPL-3.0-or-later
//
// The netplay signalling lobby: one Durable Object holding every signalling
// WebSocket, running the reference room protocol (`protocol.ts`, ported from
// `crates/rustynes-netplay/src/signaling.rs`).
//
// # Why one lobby object, not one object per room
//
// The plan sketched "a Durable Object per room". The protocol rules that out
// without breaking compatibility: a client opens ONE socket to a fixed URL and
// only then names a room (`join`), asks for the directory of every open room
// (`list-rooms`), or asks the server to pick any open room (`quick-match`).
// The last two span all rooms, and the first means the room is unknown when
// the connection is routed. Routing per room would need the room in the URL,
// i.e. a different protocol, and existing clients (the browser lobby, the
// Android and iOS room-code path, `nat_connect.rs`) would stop working.
//
// One object is also enough: signalling is a handful of small frames per
// session (join, one offer/answer per pair, a few candidates), so even
// thousands of concurrent sessions stay far below one object's throughput, and
// with the hibernation API an idle socket costs no duration. If that ever
// stops being true the lobby can be sharded by a URL parameter that new
// clients send, with the unsharded URL kept for old ones.
//
// # Hibernation
//
// Sockets are accepted with `ctx.acceptWebSocket`, so the object can be
// evicted from memory while sockets stay open. Everything the relay needs to
// resume is in each socket's attachment (its client id and room membership,
// see `Membership`), and the constructor rebuilds the relay from them. The
// limiters below are in memory and reset on eviction; the hard bounds that
// survive it are the per-IP and global socket caps, which are counted from the
// live sockets themselves (`getWebSockets(tag)`).
//
// # Boundary rules the reference server does not have
//
// These are additions for a public service, applied before the protocol sees a
// frame; a well-formed client never meets them:
//   - text frames over MAX_FRAME_BYTES (UTF-8) are refused;
//   - a room code must be 1-32 of [A-Za-z0-9_-] (the apps generate 6
//     alphanumerics; quick-match codes are `QM-000001`);
//   - a ROM hash must be empty or up to 64 hex digits (SHA-256 is 64);
//   - each socket has a message token bucket; each IP a socket cap, a
//     connection-attempt rate and a room-creation rate.
// A refusal is the reference's own rejection shape: an `error` frame, then the
// socket is closed.

import { DurableObject } from "cloudflare:workers";
import { CLIENT_IP_HEADER, type Env } from "./env";
import { TokenBucket, WindowLimiter, intVar, ipKey, loadSalt } from "./limits";
import { type Action, type ClientId, type Membership, Relay, type SignalMessage, encodeSignal, parseSignal } from "./protocol";

/** Largest accepted text frame, in UTF-8 bytes. A data-channel SDP is ~1 KB. */
export const MAX_FRAME_BYTES = 16 * 1024;
/** Per-socket message bucket: burst, then a steady rate (ICE trickle fits). */
const MSG_BURST = 60;
const MSG_PER_SECOND = 20;

const ROOM_CODE = /^[A-Za-z0-9_-]{1,32}$/;
const ROM_HASH = /^[0-9A-Fa-f]{0,64}$/;

interface Attachment {
  id: ClientId;
  /** The salted IP key (never the IP); also the socket's hibernation tag. */
  ip: string;
  m: Membership | null;
  /** Set once the lobby has decided to close this socket. */
  closed?: boolean;
}

export class Lobby extends DurableObject<Env> {
  private relay: Relay;
  private sockets = new Map<ClientId, WebSocket>();
  private buckets = new Map<ClientId, TokenBucket>();
  private nextId = 1;
  private salt = "";
  private readonly connects: WindowLimiter;
  private readonly roomCreates: WindowLimiter;

  constructor(ctx: DurableObjectState, env: Env) {
    super(ctx, env);
    this.connects = new WindowLimiter(intVar(env.LOBBY_CONNECTS_PER_IP_PER_MINUTE, 30, 0, 100000), 60_000);
    this.roomCreates = new WindowLimiter(intVar(env.LOBBY_ROOMS_PER_IP_PER_MINUTE, 10, 0, 100000), 60_000);
    // Rebuild the relay from the sockets that survived hibernation.
    const entries: Array<{ client: ClientId; m: Membership }> = [];
    for (const ws of ctx.getWebSockets()) {
      const att = ws.deserializeAttachment() as Attachment | null;
      if (!att) continue;
      this.nextId = Math.max(this.nextId, att.id + 1);
      if (att.closed) continue;
      this.sockets.set(att.id, ws);
      if (att.m) entries.push({ client: att.id, m: att.m });
    }
    this.relay = Relay.restore(entries);
    ctx.blockConcurrencyWhile(async () => {
      this.salt = await loadSalt(ctx.storage);
    });
  }

  async fetch(request: Request): Promise<Response> {
    if (request.headers.get("Upgrade")?.toLowerCase() !== "websocket") {
      return new Response("expected a WebSocket upgrade", { status: 426 });
    }
    const key = await ipKey(request.headers.get(CLIENT_IP_HEADER) ?? "unknown", this.salt);
    if (!this.connects.hit(key, Date.now())) {
      return new Response("too many connection attempts", { status: 429, headers: { "Retry-After": "60" } });
    }
    if (this.ctx.getWebSockets().length >= intVar(this.env.LOBBY_MAX_CONNECTIONS, 2000, 1, 32000)) {
      return new Response("lobby full", { status: 503, headers: { "Retry-After": "60" } });
    }
    if (this.ctx.getWebSockets(key).length >= intVar(this.env.LOBBY_MAX_CONNECTIONS_PER_IP, 8, 1, 1000)) {
      return new Response("too many connections from this address", { status: 429, headers: { "Retry-After": "60" } });
    }
    const pair = new WebSocketPair();
    const server = pair[1];
    this.ctx.acceptWebSocket(server, [key]);
    const id = this.nextId++;
    server.serializeAttachment({ id, ip: key, m: null } satisfies Attachment);
    this.sockets.set(id, server);
    return new Response(null, { status: 101, webSocket: pair[0] });
  }

  async webSocketMessage(ws: WebSocket, message: string | ArrayBuffer): Promise<void> {
    const att = ws.deserializeAttachment() as Attachment | null;
    if (!att || att.closed) return;
    // The reference ignores binary frames; so does the lobby.
    if (typeof message !== "string") return;
    const id = att.id;
    const now = Date.now();
    // Any activity prunes the per-IP counters, not only a new connection or
    // room, so PRIVACY.md's retention bound holds on a lobby whose traffic is
    // all in-room messages (limits.ts, WindowLimiter).
    this.connects.prune(now);
    this.roomCreates.prune(now);
    if (message.length > MAX_FRAME_BYTES || new TextEncoder().encode(message).byteLength > MAX_FRAME_BYTES) {
      return this.refuse(id, "frame too large");
    }
    let bucket = this.buckets.get(id);
    if (!bucket) {
      bucket = new TokenBucket(MSG_BURST, MSG_PER_SECOND, now);
      this.buckets.set(id, bucket);
    }
    if (!bucket.take(now)) return this.refuse(id, "rate limited");

    const msg = parseSignal(message);
    if (!msg) return; // unparseable frames are dropped, as the reference does
    const invalid = validate(msg);
    if (invalid) return this.refuse(id, invalid);
    if (this.relay.wouldCreateRoom(id, msg) && !this.roomCreates.hit(att.ip, now)) {
      return this.refuse(id, "rate limited");
    }
    this.dispatch(this.relay.handle(id, msg));
  }

  async webSocketClose(ws: WebSocket, code: number, _reason: string, _wasClean: boolean): Promise<void> {
    this.onGone(ws);
    try {
      // Complete the closing handshake (1005/1006 cannot be sent back).
      ws.close(code === 1005 || code === 1006 ? 1000 : code, "");
    } catch {
      // already closed
    }
  }

  async webSocketError(ws: WebSocket, _error: unknown): Promise<void> {
    this.onGone(ws);
  }

  /** A socket went away: the reference's `Relay::disconnect`. */
  private onGone(ws: WebSocket): void {
    const att = ws.deserializeAttachment() as Attachment | null;
    if (!att) return;
    this.buckets.delete(att.id);
    this.sockets.delete(att.id);
    this.dispatch(this.relay.disconnect(att.id));
  }

  /** A lobby-level refusal: leave any room (peers are told), then error + close. */
  private refuse(id: ClientId, reason: string): void {
    this.dispatch([...this.relay.disconnect(id), ...Relay.reject(id, reason)]);
  }

  private dispatch(actions: Action[]): void {
    for (const a of actions) {
      if (a.kind === "send") {
        const ws = this.sockets.get(a.to);
        if (!ws) continue;
        try {
          ws.send(encodeSignal(a.msg));
        } catch {
          // The peer's socket is closing; its close event cleans it up.
        }
      } else {
        const ws = this.sockets.get(a.who);
        if (!ws) continue;
        // The reference drops the outbound channel here, so nothing more is
        // sent to this client; any room membership it still has (a lobby
        // refusal of an in-room client) is released when it is refused.
        this.sockets.delete(a.who);
        const att = ws.deserializeAttachment() as Attachment;
        ws.serializeAttachment({ ...att, closed: true, m: null } satisfies Attachment);
        try {
          ws.close(1000, "");
        } catch {
          // already closing
        }
      }
    }
    for (const id of this.relay.takeDirty()) {
      const ws = this.sockets.get(id);
      if (!ws) continue;
      const att = ws.deserializeAttachment() as Attachment;
      ws.serializeAttachment({ ...att, m: this.relay.membership(id) } satisfies Attachment);
    }
  }
}

/** Boundary validation of a parsed frame; a reason string when refused. */
export function validate(msg: SignalMessage): string | undefined {
  if (msg.type === "join") {
    if (!ROOM_CODE.test(msg.room)) return "invalid room code";
    if (!ROM_HASH.test(msg.romHash)) return "invalid rom hash";
  } else if (msg.type === "quick-match" || msg.type === "list-rooms") {
    if (!ROM_HASH.test(msg.romHash)) return "invalid rom hash";
  }
  return undefined;
}
