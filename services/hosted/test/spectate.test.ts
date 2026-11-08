// SPDX-License-Identifier: GPL-3.0-or-later
//
// The edge spectate relay (v3.7.0, T-NETPLAY-EDGE-SPECTATE): it relays the
// match identity and confirmed inputs only, gives a mid-session viewer the
// log from frame 0, survives hibernation, and keeps its limits and its kill
// switch. Frames are built here in the netplay wire format
// (crates/rustynes-netplay/src/message.rs, protocol 10); the byte layouts are
// pinned against the Rust encoder by test/vectors/spectate-vectors.json.

import { env, exports } from "cloudflare:workers";
import { evictDurableObject, runInDurableObject } from "cloudflare:test";
import { describe, expect, it } from "vitest";
import worker from "../src/index";
import type { Env } from "../src/env";
import { CLIENT_IP_HEADER } from "../src/env";
import { type Spectate, SYNC_MAGIC, classify } from "../src/spectate";
import { sleep, waitFor } from "./helpers";
import vectors from "./vectors/spectate-vectors.json";

/** A protocol 10 `Sync`: tag 2, magic, epoch, ROM hash, config hash, share. */
function sync(epoch = 10, rom = 7): Uint8Array {
  const b = new Uint8Array(89);
  const v = new DataView(b.buffer);
  b[0] = 2;
  v.setUint32(1, SYNC_MAGIC, true);
  v.setUint32(5, epoch, true);
  b.fill(rom, 9, 41);
  b.fill(9, 41, 73);
  return b;
}

/** `ConfirmedInputs` for two players: frames `start..start+count`. */
function inputs(start: number, count: number, n = 2): Uint8Array {
  const b = new Uint8Array(7 + count * n);
  const v = new DataView(b.buffer);
  b[0] = 10;
  b[1] = n;
  v.setUint32(2, start, true);
  b[6] = count;
  for (let i = 0; i < count * n; i++) b[7 + i] = (start * n + i) & 0xff;
  return b;
}

interface Client {
  ws: WebSocket;
  frames: Uint8Array[];
  closed: () => boolean;
}

function wrapBinary(ws: WebSocket): Client {
  ws.accept();
  const frames: Uint8Array[] = [];
  let closed = false;
  // workerd delivers a binary message to a test-side client as a Blob; the
  // chain keeps the frames in arrival order while each is read.
  let chain = Promise.resolve();
  ws.addEventListener("message", (e) => {
    const d = e.data as unknown;
    if (typeof d === "string") return;
    chain = chain.then(async () => {
      frames.push(d instanceof Blob ? new Uint8Array(await d.arrayBuffer()) : new Uint8Array(d as ArrayBuffer));
    });
  });
  ws.addEventListener("close", () => {
    closed = true;
  });
  return { ws, frames, closed: () => closed };
}

let counter = 0;
const freshMatch = () => `m${Date.now().toString(36)}${counter++}`;
const KEY = "0123456789abcdef0123456789abcdef";

/** A fresh client address per call by default, so the per-IP match limit is
 * reached only by the test about it. */
const freshIp = () => `203.0.113.${counter++ % 250}`;

async function open(match: string, query: string, ip = freshIp()): Promise<Response> {
  return exports.default.fetch(`https://hosted.test/spectate/${match}?${query}`, {
    headers: { Upgrade: "websocket", "CF-Connecting-IP": ip },
  });
}

async function connect(match: string, query: string, ip?: string): Promise<Client> {
  const r = await open(match, query, ip);
  if (r.status !== 101 || !r.webSocket) throw new Error(`upgrade refused: ${r.status} ${await r.text()}`);
  return wrapBinary(r.webSocket);
}

const same = (a: Uint8Array, b: Uint8Array) => a.length === b.length && a.every((x, i) => x === b[i]);

describe("classify", () => {
  it("accepts the identity and contiguous confirmed inputs only", () => {
    expect(classify(sync(), 0, undefined).kind).toBe("sync");
    const wrongMagic = sync();
    wrongMagic[1] = 0x39; // "RNE9", protocol 9
    expect(classify(wrongMagic, 0, undefined)).toEqual({ kind: "drop", why: "not a protocol 10 Sync" });
    expect(classify(inputs(0, 8), 0, undefined)).toEqual({ kind: "inputs", start: 0, count: 8, numPlayers: 2 });
    // An overlap that extends the log is accepted; a gap or a repeat is not.
    expect(classify(inputs(4, 8), 8, 2).kind).toBe("inputs");
    expect(classify(inputs(9, 8), 8, 2)).toEqual({ kind: "drop", why: "gap" });
    expect(classify(inputs(0, 8), 8, 2)).toEqual({ kind: "drop", why: "already relayed" });
    expect(classify(inputs(8, 8, 4), 8, 2)).toEqual({ kind: "drop", why: "player count changed" });
    const truncated = inputs(0, 8).slice(0, 20);
    expect(classify(truncated, 0, undefined).kind).toBe("drop");
  });

  it("never relays state, checksums, per-player inputs or oversized frames", () => {
    const checkpoint = new Uint8Array(1 + 14 + 100);
    checkpoint[0] = 11; // StateCheckpoint
    const checksum = new Uint8Array(21);
    checksum[0] = 3;
    const input = new Uint8Array([0, 1, 0, 0, 0, 0, 0x80]);
    const forensic = new Uint8Array(30);
    forensic[0] = 8;
    for (const f of [checkpoint, checksum, input, forensic]) {
      expect(classify(f, 0, undefined).kind, `tag ${f[0]}`).toBe("drop");
    }
    expect(classify(new Uint8Array(513).fill(10), 0, undefined)).toEqual({ kind: "drop", why: "size" });
  });

  it("decodes the Rust encoder's frames the same way", () => {
    for (const v of (vectors as { frames: Array<{ name: string; hex: string; verdict: unknown }> }).frames) {
      const bytes = new Uint8Array(v.hex.match(/../g)!.map((h) => Number.parseInt(h, 16)));
      expect(classify(bytes, 0, undefined), v.name).toEqual(v.verdict);
    }
  });
});

describe("relay", () => {
  it("gives a mid-session viewer the log from frame 0, then the live feed", async () => {
    const match = freshMatch();
    const pub = await connect(match, `role=publish&key=${KEY}`);
    pub.ws.send(sync());
    pub.ws.send(inputs(0, 64));
    pub.ws.send(inputs(64, 64));
    const stub = env.SPECTATE.getByName(`match:${match}`);
    await waitForStats(stub, (s) => s.frames === 128);
    const viewer = await connect(match, "role=view");
    await waitFor(() => viewer.frames.length === 3, "backlog");
    expect(same(viewer.frames[0], sync())).toBe(true);
    expect(same(viewer.frames[1], inputs(0, 64))).toBe(true);
    expect(same(viewer.frames[2], inputs(64, 64))).toBe(true);
    pub.ws.send(inputs(128, 8));
    await waitFor(() => viewer.frames.length === 4, "live frame");
    expect(same(viewer.frames[3], inputs(128, 8))).toBe(true);
    pub.ws.close(1000, "");
    viewer.ws.close(1000, "");
  });

  it("forwards inputs only: state, checksums and stray input are dropped", async () => {
    const match = freshMatch();
    const pub = await connect(match, `role=publish&key=${KEY}`);
    pub.ws.send(sync());
    pub.ws.send(inputs(0, 8));
    const viewer = await connect(match, "role=view");
    await waitFor(() => viewer.frames.length === 2, "backlog");
    const checkpoint = new Uint8Array(115);
    checkpoint[0] = 11;
    const checksum = new Uint8Array(21);
    checksum[0] = 3;
    pub.ws.send(checkpoint);
    pub.ws.send(checksum);
    pub.ws.send(new Uint8Array([0, 1, 9, 0, 0, 0, 1]));
    pub.ws.send("a text frame");
    pub.ws.send(sync(10, 8)); // another game mid-stream
    pub.ws.send(inputs(8, 8));
    await waitFor(() => viewer.frames.length === 3, "the valid frame");
    await sleep(30);
    expect(viewer.frames.length).toBe(3);
    expect(same(viewer.frames[2], inputs(8, 8))).toBe(true);
    const stats = await env.SPECTATE.getByName(`match:${match}`).stats();
    expect(stats.dropped).toBe(5);
    expect(stats.frames).toBe(16);
    pub.ws.close(1000, "");
    viewer.ws.close(1000, "");
  });

  it("keeps the log across hibernation", async () => {
    const match = freshMatch();
    const pub = await connect(match, `role=publish&key=${KEY}`);
    pub.ws.send(sync());
    pub.ws.send(inputs(0, 32));
    const stub = env.SPECTATE.getByName(`match:${match}`);
    await waitForStats(stub, (s) => s.frames === 32);
    await evictDurableObject(stub);
    pub.ws.send(inputs(32, 32));
    await waitForStats(stub, (s) => s.frames === 64);
    const viewer = await connect(match, "role=view");
    await waitFor(() => viewer.frames.length === 3, "backlog after eviction");
    expect(same(viewer.frames[2], inputs(32, 32))).toBe(true);
    pub.ws.close(1000, "");
    viewer.ws.close(1000, "");
  });

  it("only the claiming publisher may write, one at a time", async () => {
    const match = freshMatch();
    const pub = await connect(match, `role=publish&key=${KEY}`);
    expect((await open(match, "role=publish&key=ffffffffffffffffffffffffffffffff")).status).toBe(403);
    expect((await open(match, `role=publish&key=${KEY}`)).status).toBe(409);
    expect((await open(match, "role=publish&key=short")).status).toBe(400);
    expect((await open(match, "role=lurk")).status).toBe(400);
    pub.ws.close(1000, "");
    // Viewers send nothing that reaches the log.
    const viewer = await connect(match, "role=view");
    viewer.ws.send(inputs(0, 8));
    await sleep(30);
    expect((await env.SPECTATE.getByName(`match:${match}`).stats()).frames).toBe(0);
    viewer.ws.close(1000, "");
  });

  it("refuses a viewer of a match nobody published", async () => {
    expect((await open(freshMatch(), "role=view")).status).toBe(404);
    expect((await open("bad id!", "role=view")).status).toBe(400);
  });

  it("caps viewers per address", async () => {
    const match = freshMatch();
    const pub = await connect(match, `role=publish&key=${KEY}`);
    const cap = Number(env.SPECTATE_MAX_VIEWERS_PER_IP);
    const viewers: Client[] = [];
    for (let i = 0; i < cap; i++) viewers.push(await connect(match, "role=view", "198.51.100.50"));
    expect((await open(match, "role=view", "198.51.100.50")).status).toBe(429);
    expect((await open(match, "role=view", "198.51.100.51")).status).toBe(101);
    for (const v of viewers) v.ws.close(1000, "");
    pub.ws.close(1000, "");
  });

  it("limits how many new matches one address may claim", async () => {
    const perHour = Number(env.SPECTATE_MATCHES_PER_IP_PER_HOUR);
    const ip = "198.51.100.200";
    const opened: Client[] = [];
    for (let i = 0; i < perHour; i++) opened.push(await connect(freshMatch(), `role=publish&key=${KEY}`, ip));
    expect((await open(freshMatch(), `role=publish&key=${KEY}`, ip)).status).toBe(429);
    for (const c of opened) c.ws.close(1000, "");
  });

  it("the kill switch refuses before any Durable Object is reached", async () => {
    for (const v of ["false", "", undefined]) {
      const e = { ...env, SPECTATE_ENABLED: v } as Env;
      const r = await worker.fetch(
        new Request("https://hosted.test/spectate/abc?role=view", {
          headers: { Upgrade: "websocket", [CLIENT_IP_HEADER]: "x" },
        }),
        e,
      );
      expect(r.status, String(v)).toBe(503);
    }
    // And the object refuses on its own, should a request reach it.
    const stub = env.SPECTATE.getByName("match:off");
    await runInDurableObject(stub, async (instance: Spectate) => {
      const self = instance as unknown as { env: Env };
      const saved = self.env.SPECTATE_ENABLED;
      (self.env as { SPECTATE_ENABLED?: string }).SPECTATE_ENABLED = "false";
      const r = await instance.fetch(
        new Request("https://x/spectate/off?role=view", { headers: { Upgrade: "websocket" } }),
      );
      (self.env as { SPECTATE_ENABLED?: string }).SPECTATE_ENABLED = saved;
      expect(r.status).toBe(503);
    });
  });
});

async function waitForStats(
  stub: DurableObjectStub<Spectate>,
  cond: (s: { frames: number; rows: number; dropped: number; viewers: number }) => boolean,
): Promise<void> {
  const start = Date.now();
  for (;;) {
    const s = await stub.stats();
    if (cond(s)) return;
    if (Date.now() - start > 2000) throw new Error(`timed out: ${JSON.stringify(s)}`);
    await sleep(5);
  }
}
