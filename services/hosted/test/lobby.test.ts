// SPDX-License-Identifier: GPL-3.0-or-later
//
// The lobby's boundary rules (the parts a public service adds to the
// reference protocol) and the Worker's routing to it.

import { env, exports } from "cloudflare:workers";
import { runInDurableObject } from "cloudflare:test";
import { describe, expect, it, vi } from "vitest";
import "../src/index";
import { type Lobby, MAX_FRAME_BYTES } from "../src/lobby";
import { connectStub, sleep, waitFor, wrap } from "./helpers";

const err = (reason: string) => `{"type":"error","reason":"${reason}"}`;

describe("routing", () => {
  it("upgrades /signal through the Worker into the lobby", async () => {
    const resp = await exports.default.fetch("https://hosted.test/signal", {
      headers: { Upgrade: "websocket", "CF-Connecting-IP": "192.0.2.10" },
    });
    expect(resp.status).toBe(101);
    const c = wrap(resp.webSocket!);
    c.send('{"type":"join","room":"routed","max_players":2}');
    await waitFor(() => c.inbox.length === 1, "joined");
    expect(c.inbox[0]).toBe('{"type":"joined","slot":0,"max_players":2}');
    c.ws.close(1000, "");
  });

  it("refuses a page from another origin, including look-alikes", async () => {
    for (const origin of [
      "https://evil.example",
      // Prefix and suffix look-alikes of the allowed origins: the match is exact.
      "https://doublegate.github.io.evil.example",
      "https://doublegate.github.io:8443",
      "http://doublegate.github.io",
      "http://localhost.evil.example",
      "http://127.0.0.1.nip.io:8080",
    ]) {
      const resp = await exports.default.fetch("https://hosted.test/signal", {
        headers: { Upgrade: "websocket", Origin: origin },
      });
      expect(resp.status, origin).toBe(403);
    }
  });

  it("admits the Pages origin and localhost", async () => {
    for (const origin of ["https://doublegate.github.io", "http://127.0.0.1:8080", "http://localhost:8081"]) {
      const resp = await exports.default.fetch("https://hosted.test/signal", {
        headers: { Upgrade: "websocket", Origin: origin, "CF-Connecting-IP": "192.0.2.11" },
      });
      expect(resp.status, origin).toBe(101);
      resp.webSocket!.accept();
      resp.webSocket!.close(1000, "");
    }
  });

  it("does not let a client choose its own rate-limit key", async () => {
    // A client-sent copy of the internal IP header is overwritten by the
    // Worker with CF-Connecting-IP, so spoofing it does not escape the cap.
    const stubCap = Number(env.LOBBY_MAX_CONNECTIONS_PER_IP);
    const open: WebSocket[] = [];
    for (let i = 0; i < stubCap; i++) {
      const r = await exports.default.fetch("https://hosted.test/signal", {
        headers: { Upgrade: "websocket", "CF-Connecting-IP": "192.0.2.99", "x-rustynes-client-ip": `10.9.9.${i}` },
      });
      expect(r.status).toBe(101);
      r.webSocket!.accept();
      open.push(r.webSocket!);
    }
    const over = await exports.default.fetch("https://hosted.test/signal", {
      headers: { Upgrade: "websocket", "CF-Connecting-IP": "192.0.2.99", "x-rustynes-client-ip": "10.9.9.200" },
    });
    expect(over.status).toBe(429);
    for (const ws of open) ws.close(1000, "");
  });

  it("answers a plain GET on / and /healthz", async () => {
    expect((await exports.default.fetch("https://hosted.test/")).status).toBe(200);
    expect((await exports.default.fetch("https://hosted.test/healthz")).status).toBe(200);
    expect((await exports.default.fetch("https://hosted.test/signal")).status).toBe(426);
    expect((await exports.default.fetch("https://hosted.test/nope")).status).toBe(404);
  });
});

describe("boundary rules", () => {
  it("caps connections per IP with 429", async () => {
    const stub = env.LOBBY.getByName("cap-per-ip");
    const cap = Number(env.LOBBY_MAX_CONNECTIONS_PER_IP);
    const open = [];
    for (let i = 0; i < cap; i++) open.push(await connectStub(stub, "198.51.100.1"));
    await expect(connectStub(stub, "198.51.100.1")).rejects.toThrow("429");
    // Another address is unaffected.
    open.push(await connectStub(stub, "198.51.100.2"));
    for (const c of open) c.ws.close(1000, "");
  });

  it("refuses an oversized frame with the reference's error-then-close", async () => {
    const stub = env.LOBBY.getByName("big-frame");
    const c = await connectStub(stub, "198.51.100.3");
    c.send(`{"type":"offer","from":0,"to":1,"sdp":"${"x".repeat(MAX_FRAME_BYTES)}"}`);
    await waitFor(() => c.closed(), "close");
    expect(c.inbox).toEqual([err("frame too large")]);
  });

  it("counts the size in UTF-8 bytes, not UTF-16 units", async () => {
    const stub = env.LOBBY.getByName("utf8-frame");
    const c = await connectStub(stub, "198.51.100.4");
    // 6,000 three-byte characters: 6,000 UTF-16 units but 18,000 bytes.
    c.send(`{"type":"offer","from":0,"to":1,"sdp":"${"€".repeat(6000)}"}`);
    await waitFor(() => c.closed(), "close");
    expect(c.inbox).toEqual([err("frame too large")]);
  });

  it("validates room codes and ROM hashes", async () => {
    const stub = env.LOBBY.getByName("validate");
    const cases: Array<[string, string]> = [
      ['{"type":"join","room":"has space"}', "invalid room code"],
      ['{"type":"join","room":""}', "invalid room code"],
      [`{"type":"join","room":"${"a".repeat(33)}"}`, "invalid room code"],
      ['{"type":"join","room":"ok","rom_hash":"not-hex"}', "invalid rom hash"],
      [`{"type":"quick-match","rom_hash":"${"a".repeat(65)}"}`, "invalid rom hash"],
    ];
    for (const [i, [frame, reason]] of cases.entries()) {
      const c = await connectStub(stub, `198.51.100.${10 + i}`);
      c.send(frame);
      await waitFor(() => c.closed(), `close for ${frame}`);
      expect(c.inbox, frame).toEqual([err(reason)]);
    }
    // The boundary admits what the apps generate.
    const ok = await connectStub(stub, "198.51.100.30");
    ok.send(`{"type":"join","room":"AB12cd","rom_hash":"${"0f".repeat(32)}","max_players":2}`);
    await waitFor(() => ok.inbox.length === 1, "joined");
    expect(ok.inbox[0]).toBe('{"type":"joined","slot":0,"max_players":2}');
    ok.ws.close(1000, "");
  });

  it("limits room creation per IP, and tells the room when a member is refused", async () => {
    const stub = env.LOBBY.getByName("room-rate");
    const limit = Number(env.LOBBY_ROOMS_PER_IP_PER_MINUTE);
    const c = await connectStub(stub, "198.51.100.40");
    for (let i = 0; i < limit; i++) {
      c.send(`{"type":"join","room":"r${i}","max_players":2}`);
    }
    await waitFor(() => c.inbox.length === limit, "joins");
    const peer = await connectStub(stub, "198.51.100.41");
    peer.send(`{"type":"join","room":"r${limit - 1}","max_players":2}`);
    await waitFor(() => c.inbox.length === limit + 1, "peer-joined");
    // One room too many: refused, and the peer in its current room is told.
    c.send(`{"type":"join","room":"one-too-many","max_players":2}`);
    await waitFor(() => c.closed(), "close");
    expect(c.inbox.at(-1)).toBe(err("rate limited"));
    await waitFor(() => peer.inbox.length === 2, "peer-left");
    expect(peer.inbox[1]).toBe('{"type":"peer-left","slot":0}');
    // Joining an EXISTING room is not a creation and is not limited.
    const c2 = await connectStub(stub, "198.51.100.40");
    c2.send(`{"type":"join","room":"r${limit - 1}","max_players":2}`);
    await waitFor(() => c2.inbox.length === 1, "join existing");
    expect(c2.inbox[0]).toBe('{"type":"joined","slot":1,"max_players":2}');
    for (const s of [peer, c2]) s.ws.close(1000, "");
  });

  it("closes a socket that floods messages", async () => {
    const stub = env.LOBBY.getByName("flood");
    const c = await connectStub(stub, "198.51.100.50");
    for (let i = 0; i < 200; i++) c.send('{"type":"list-rooms","rom_hash":""}');
    await waitFor(() => c.closed(), "close");
    expect(c.inbox.at(-1)).toBe(err("rate limited"));
    expect(c.inbox.length).toBeLessThan(200);
  });

  // PRIVACY.md: the per-IP counters go within two minutes while the lobby is
  // in use, even when that use is only in-room messages (no new connection or
  // room, the only other places the limiters run).
  it("an in-room message prunes the per-IP counters once they expire", async () => {
    const stub = env.LOBBY.getByName("prune");
    const c = await connectStub(stub, "198.51.100.70");
    await runInDurableObject(stub, async (obj: Lobby, state) => {
      const counters = obj as unknown as { connects: { size: number } };
      expect(counters.connects.size).toBe(1);
      const later = Date.now() + 3 * 60_000;
      const spy = vi.spyOn(Date, "now").mockReturnValue(later);
      try {
        const [ws] = state.getWebSockets();
        await obj.webSocketMessage(ws, '{"type":"list-rooms"}');
      } finally {
        spy.mockRestore();
      }
      expect(counters.connects.size).toBe(0);
    });
    c.ws.close(1000, "");
  });

  it("ignores binary frames, as the reference does", async () => {
    const stub = env.LOBBY.getByName("binary");
    const c = await connectStub(stub, "198.51.100.60");
    c.ws.send(new Uint8Array([1, 2, 3]));
    c.send('{"type":"list-rooms"}');
    await waitFor(() => c.inbox.length === 1, "reply");
    await sleep(20);
    expect(c.inbox).toEqual(['{"type":"room-list","rooms":[]}']);
    c.ws.close(1000, "");
  });
});
