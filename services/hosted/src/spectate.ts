// SPDX-License-Identifier: GPL-3.0-or-later
//
// Edge-relayed spectating (v3.7.0, T-NETPLAY-EDGE-SPECTATE): one Durable
// Object per match, which fans out the match's CONFIRMED inputs to any number
// of viewers. Each viewer runs the core locally from that stream
// (crates/rustynes-netplay/src/edge.rs), so it can seek, rewind and pause on
// its own timeline.
//
// # Inputs only
//
// The relay carries exactly two kinds of binary frame, in the netplay wire
// format (crates/rustynes-netplay/src/message.rs, protocol 10):
//   - tag 2, `Sync`: the match's identity (ROM, configuration, emulation
//     epoch), 89 bytes under the magic "RN10". Kept, and sent to every viewer
//     first, so a viewer refuses a match it cannot reproduce.
//   - tag 10, `ConfirmedInputs`: every player's confirmed input for
//     consecutive frames. Appended to the match's log only if it continues
//     the log (no gap; an overlap is accepted as long as it extends it), and
//     forwarded.
// Everything else the publisher sends (a state checkpoint, a checksum, a raw
// per-player input, a text frame, an oversized frame) is dropped and counted,
// never forwarded: the relay never carries frames, audio or machine state.
// Viewers send nothing; anything a viewer sends is ignored, except the
// "ping" the runtime answers itself without waking the object.
//
// # Joining mid-session
//
// A viewer that connects receives the identity and the whole log from frame
// 0, then the live feed. It catches up by running the log (CPU on the viewer,
// two to four bytes a frame on the relay). The log is bounded by
// SPECTATE_MAX_FRAMES; a match that runs past it stops being relayed.
//
// # Who may publish
//
// The first publisher of a match id claims it with a key it chose (32-64 hex
// digits); the relay stores only the key's SHA-256. A later publisher
// connection must present the same key (a reconnecting host), so nobody else
// can write into a match. Claiming a NEW match passes the Budget object's
// per-IP hourly and global daily match limits.
//
// # Hibernation
//
// Sockets are accepted with `ctx.acceptWebSocket`; each carries its role in
// its attachment and tag. The log lives in the object's storage, one row per
// published frame (a publisher batches several video frames into one), so an
// eviction loses nothing and the next viewer still gets the log from frame 0.
// The limiters are in memory and reset on eviction; the socket caps are
// counted from the live sockets themselves.

import { DurableObject } from "cloudflare:workers";
import { CLIENT_IP_HEADER, type Env } from "./env";
import { TokenBucket, WindowLimiter, intVar, ipKey, loadSalt } from "./limits";

/** Tag bytes of the two frames the relay carries. */
export const TAG_SYNC = 2;
export const TAG_CONFIRMED_INPUTS = 10;
/** Protocol 10's `Sync` magic, "RN10", little-endian. */
export const SYNC_MAGIC = 0x524e3130;
/** A protocol 10 `Sync` frame: tag + magic + epoch + two hashes + key share. */
export const SYNC_LEN = 89;
/** The largest frame the relay accepts: a full four-player batch is 263. */
export const MAX_FRAME_BYTES = 512;
/** `ConfirmedInputs` caps (message.rs). */
const MAX_CONFIRMED_BATCH = 64;

/** A match id: what the clients put in the URL. */
export const MATCH_ID = /^[A-Za-z0-9_-]{1,32}$/;
/** A publisher key: 128 to 256 bits, hex. */
export const PUBLISH_KEY = /^[0-9A-Fa-f]{32,64}$/;

/** Publisher message bucket: an 8-frame batch every 133 ms is 7.5/s. */
const PUB_BURST = 60;
const PUB_PER_SECOND = 30;

type Role = "publish" | "view";

interface Attachment {
  role: Role;
  ip: string;
}

interface Meta {
  /** SHA-256 hex of the claiming publisher's key. */
  keyHash: string;
  /** The identity frame, hex. */
  sync?: string;
  /** Players in the match, from the first input batch. */
  numPlayers?: number;
  /** The frame after the log's last (the log covers 0..frames). */
  frames: number;
  /** Rows in the log. */
  rows: number;
}

/** What a relay decided about one publisher frame (exported for the tests). */
export type Verdict =
  | { kind: "sync" }
  | { kind: "inputs"; start: number; count: number; numPlayers: number }
  | { kind: "drop"; why: string };

/** Classify one publisher frame against the log, without side effects. */
export function classify(bytes: Uint8Array, logFrames: number, numPlayers: number | undefined): Verdict {
  if (bytes.byteLength === 0 || bytes.byteLength > MAX_FRAME_BYTES) return { kind: "drop", why: "size" };
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const tag = bytes[0];
  if (tag === TAG_SYNC) {
    if (bytes.byteLength !== SYNC_LEN || view.getUint32(1, true) !== SYNC_MAGIC) {
      return { kind: "drop", why: "not a protocol 10 Sync" };
    }
    return { kind: "sync" };
  }
  if (tag !== TAG_CONFIRMED_INPUTS) return { kind: "drop", why: `tag ${tag} is not relayed` };
  if (bytes.byteLength < 7) return { kind: "drop", why: "truncated" };
  const n = bytes[1];
  const start = view.getUint32(2, true);
  const count = bytes[6];
  if (n < 2 || n > 4 || count < 1 || count > MAX_CONFIRMED_BATCH || bytes.byteLength !== 7 + count * n) {
    return { kind: "drop", why: "malformed inputs" };
  }
  if (numPlayers !== undefined && n !== numPlayers) return { kind: "drop", why: "player count changed" };
  if (start > logFrames) return { kind: "drop", why: "gap" };
  if (start + count <= logFrames) return { kind: "drop", why: "already relayed" };
  return { kind: "inputs", start, count, numPlayers: n };
}

const hex = (b: Uint8Array) => [...b].map((x) => x.toString(16).padStart(2, "0")).join("");
const unhex = (s: string) => new Uint8Array(s.match(/../g)!.map((h) => Number.parseInt(h, 16)));
const rowKey = (i: number) => `log:${String(i).padStart(9, "0")}`;

async function sha256Hex(text: string): Promise<string> {
  return hex(new Uint8Array(await crypto.subtle.digest("SHA-256", new TextEncoder().encode(text))));
}

export class Spectate extends DurableObject<Env> {
  private salt = "";
  private meta: Meta | undefined;
  private readonly connects: WindowLimiter;
  private bucket: TokenBucket | undefined;
  /** Frames dropped from the publisher since the object woke (for tests). */
  dropped = 0;

  constructor(ctx: DurableObjectState, env: Env) {
    super(ctx, env);
    this.connects = new WindowLimiter(intVar(env.SPECTATE_CONNECTS_PER_IP_PER_MINUTE, 30, 0, 100000), 60_000);
    // Answered by the runtime without waking the object (not billed).
    ctx.setWebSocketAutoResponse(new WebSocketRequestResponsePair("ping", "pong"));
    ctx.blockConcurrencyWhile(async () => {
      this.salt = await loadSalt(ctx.storage);
      this.meta = await ctx.storage.get<Meta>("meta");
    });
  }

  async fetch(request: Request): Promise<Response> {
    if (this.env.SPECTATE_ENABLED !== "true") {
      return new Response("spectating is switched off", { status: 503, headers: { "Retry-After": "3600" } });
    }
    if (request.headers.get("Upgrade")?.toLowerCase() !== "websocket") {
      return new Response("expected a WebSocket upgrade", { status: 426 });
    }
    const url = new URL(request.url);
    const role = url.searchParams.get("role");
    const ipRaw = request.headers.get(CLIENT_IP_HEADER) ?? "unknown";
    const key = await ipKey(ipRaw, this.salt);
    const now = Date.now();
    if (!this.connects.hit(key, now)) {
      return new Response("too many connection attempts", { status: 429, headers: { "Retry-After": "60" } });
    }

    if (role === "publish") {
      const pubKey = url.searchParams.get("key") ?? "";
      if (!PUBLISH_KEY.test(pubKey)) return new Response("a publisher needs a key", { status: 400 });
      const keyHash = await sha256Hex(pubKey.toLowerCase());
      if (this.meta === undefined) {
        const admission = await this.env.BUDGET.getByName("budget").admitSpectate(ipRaw);
        if (!admission.ok) {
          return new Response(admission.error, {
            status: admission.status,
            headers: { "Retry-After": String(admission.retryAfter) },
          });
        }
        this.meta = { keyHash, frames: 0, rows: 0 };
        await this.ctx.storage.put("meta", this.meta);
      } else if (this.meta.keyHash !== keyHash) {
        return new Response("this match belongs to another publisher", { status: 403 });
      }
      if (this.ctx.getWebSockets("publish").length > 0) {
        return new Response("this match already has a publisher", { status: 409 });
      }
      await this.touch(now);
      return this.accept("publish", key).response;
    }

    if (role === "view") {
      if (this.meta === undefined) return new Response("no such match", { status: 404 });
      if (this.ctx.getWebSockets("view").length >= intVar(this.env.SPECTATE_MAX_VIEWERS, 200, 1, 32000)) {
        return new Response("this match is full", { status: 503, headers: { "Retry-After": "60" } });
      }
      if (this.ctx.getWebSockets(key).length >= intVar(this.env.SPECTATE_MAX_VIEWERS_PER_IP, 8, 1, 1000)) {
        return new Response("too many viewers from this address", { status: 429, headers: { "Retry-After": "60" } });
      }
      const { response, server } = this.accept("view", key);
      await this.sendBacklog(server);
      return response;
    }
    return new Response("role must be publish or view", { status: 400 });
  }

  private accept(role: Role, ip: string): { response: Response; server: WebSocket } {
    const pair = new WebSocketPair();
    const server = pair[1];
    this.ctx.acceptWebSocket(server, [role, ip]);
    server.serializeAttachment({ role, ip } satisfies Attachment);
    return { response: new Response(null, { status: 101, webSocket: pair[0] }), server };
  }

  /** The identity and the whole log, in order, to one viewer. */
  private async sendBacklog(ws: WebSocket): Promise<void> {
    const meta = this.meta;
    if (!meta) return;
    try {
      if (meta.sync) ws.send(unhex(meta.sync));
      const rows = await this.ctx.storage.list<Uint8Array>({ prefix: "log:" });
      for (const frame of rows.values()) ws.send(frame);
    } catch {
      // The viewer went away during the backlog; its close cleans it up.
    }
  }

  async webSocketMessage(ws: WebSocket, message: string | ArrayBuffer): Promise<void> {
    const att = ws.deserializeAttachment() as Attachment | null;
    if (!att || att.role !== "publish") return; // viewers send nothing
    if (typeof message === "string") {
      this.dropped++;
      return;
    }
    const now = Date.now();
    this.connects.prune(now);
    this.bucket ??= new TokenBucket(PUB_BURST, PUB_PER_SECOND, now);
    if (!this.bucket.take(now)) {
      this.dropped++;
      return;
    }
    const meta = this.meta;
    if (!meta) return;
    const bytes = new Uint8Array(message);
    const verdict = classify(bytes, meta.frames, meta.numPlayers);
    if (verdict.kind === "drop") {
      this.dropped++;
      return;
    }
    if (verdict.kind === "sync") {
      const sync = hex(bytes);
      if (meta.sync !== undefined && meta.sync !== sync) {
        // A match does not change game mid-stream.
        this.dropped++;
        return;
      }
      if (meta.sync === undefined) {
        meta.sync = sync;
        await this.ctx.storage.put("meta", meta);
        this.broadcast(bytes);
      }
      return;
    }
    if (meta.sync === undefined) {
      // Inputs before the identity: a viewer could not check them.
      this.dropped++;
      return;
    }
    const maxFrames = intVar(this.env.SPECTATE_MAX_FRAMES, 216_000, 60, 10_000_000);
    if (verdict.start + verdict.count > maxFrames) {
      this.dropped++;
      try {
        ws.close(1008, "match too long to relay");
      } catch {
        // already closing
      }
      return;
    }
    meta.numPlayers = verdict.numPlayers;
    meta.frames = verdict.start + verdict.count;
    const row = meta.rows;
    meta.rows += 1;
    await this.ctx.storage.put({ meta, [rowKey(row)]: bytes });
    await this.touch(now);
    this.broadcast(bytes);
  }

  private broadcast(bytes: Uint8Array): void {
    for (const v of this.ctx.getWebSockets("view")) {
      try {
        v.send(bytes);
      } catch {
        // closing; its close event cleans up
      }
    }
  }

  /** Keep the match for SPECTATE_IDLE_SECONDS after the publisher's last word. */
  private async touch(now: number): Promise<void> {
    const idle = intVar(this.env.SPECTATE_IDLE_SECONDS, 3600, 60, 86_400) * 1000;
    const alarm = await this.ctx.storage.getAlarm();
    // Re-armed at most once a minute, not per frame.
    if (alarm === null || alarm < now + idle - 60_000) await this.ctx.storage.setAlarm(now + idle);
  }

  /** The idle timer: forget the match (its log, key and identity). */
  async alarm(): Promise<void> {
    if (this.ctx.getWebSockets("publish").length > 0) {
      await this.touch(Date.now());
      return;
    }
    for (const ws of this.ctx.getWebSockets()) {
      try {
        ws.close(1001, "match ended");
      } catch {
        // already closing
      }
    }
    await this.ctx.storage.deleteAll();
    this.meta = undefined;
    this.salt = await loadSalt(this.ctx.storage);
  }

  async webSocketClose(ws: WebSocket, code: number): Promise<void> {
    try {
      ws.close(code === 1005 || code === 1006 ? 1000 : code, "");
    } catch {
      // already closed
    }
  }

  /** Test hook: the log's extent and how many frames were dropped. */
  async stats(): Promise<{ frames: number; rows: number; dropped: number; viewers: number }> {
    return {
      frames: this.meta?.frames ?? 0,
      rows: this.meta?.rows ?? 0,
      dropped: this.dropped,
      viewers: this.ctx.getWebSockets("view").length,
    };
  }
}
