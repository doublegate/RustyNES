// SPDX-License-Identifier: GPL-3.0-or-later
//
// `/turn-credentials` and its spend guards. Cloudflare's TURN API is replaced
// by a mock of `fetch` in every test, so nothing here reaches the network.
// The spend limits are the test values from vitest.config.ts (daily cap 5,
// 3 per IP per hour); wrangler.jsonc holds the deployed ones.

import { env, exports } from "cloudflare:workers";
import { runInDurableObject } from "cloudflare:test";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import "../src/index";
import { utcDay } from "../src/budget";
import type { Env } from "../src/env";
import { DEFAULT_TTL, TURN_API, handleTurn, sanitizeIceServers } from "../src/turn";

const UPSTREAM_OK = {
  iceServers: [
    { urls: ["stun:stun.cloudflare.com:3478", "stun:stun.cloudflare.com:53"] },
    {
      urls: [
        "turn:turn.cloudflare.com:3478?transport=udp",
        "turn:turn.cloudflare.com:53?transport=udp",
        "turn:turn.cloudflare.com:3478?transport=tcp",
        "turns:turn.cloudflare.com:5349?transport=tcp",
      ],
      username: "u-short-lived",
      credential: "c-short-lived",
    },
  ],
};

let ipSeq = 0;
const freshIp = () => `203.0.113.${++ipSeq}`;

function mockUpstream(impl: (url: string, init: RequestInit) => Promise<Response> | Response) {
  return vi.spyOn(globalThis, "fetch").mockImplementation(async (input, init) => impl(String(input instanceof Request ? input.url : input), init ?? {}));
}

const call = (ip: string, headers: Record<string, string> = {}) =>
  exports.default.fetch("https://hosted.test/turn-credentials", { headers: { "CF-Connecting-IP": ip, ...headers } });

// Each test starts with an empty global budget, so the daily cap one test
// spends does not leak into the next.
beforeEach(async () => {
  await runInDurableObject(env.BUDGET.getByName("global"), async (_obj, state) => {
    await state.storage.deleteAll();
  });
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe("minting", () => {
  it("returns short-lived ICE servers and passes the TTL upstream", async () => {
    const spy = mockUpstream(() => Response.json(UPSTREAM_OK, { status: 201 }));
    const before = Math.floor(Date.now() / 1000);
    const resp = await call(freshIp());
    expect(resp.status).toBe(200);
    expect(resp.headers.get("Cache-Control")).toBe("no-store");
    const body = (await resp.json()) as { iceServers: unknown[]; ttl: number; expiresAt: number };

    // The upstream call: the key id in the path, the token as a bearer, the TTL.
    expect(spy).toHaveBeenCalledTimes(1);
    const [url, init] = spy.mock.calls[0];
    expect(String(url)).toBe(`${TURN_API}/test-key-id/credentials/generate-ice-servers`);
    expect((init!.headers as Record<string, string>).Authorization).toBe("Bearer test-api-token");
    expect(JSON.parse(String(init!.body))).toEqual({ ttl: DEFAULT_TTL });

    // The answer: port-53 URLs dropped (browsers block them), TTL and expiry.
    expect(body.ttl).toBe(10800);
    expect(body.iceServers).toEqual([
      { urls: ["stun:stun.cloudflare.com:3478"] },
      {
        urls: [
          "turn:turn.cloudflare.com:3478?transport=udp",
          "turn:turn.cloudflare.com:3478?transport=tcp",
          "turns:turn.cloudflare.com:5349?transport=tcp",
        ],
        username: "u-short-lived",
        credential: "c-short-lived",
      },
    ]);
    const after = Math.floor(Date.now() / 1000);
    expect(body.expiresAt).toBeGreaterThanOrEqual(before + 10800);
    expect(body.expiresAt).toBeLessThanOrEqual(after + 10800);
  });

  it("never returns the TURN key or its token", async () => {
    mockUpstream(() => Response.json(UPSTREAM_OK, { status: 201 }));
    const text = await (await call(freshIp())).text();
    expect(text).not.toContain("test-api-token");
    expect(text).not.toContain("test-key-id");
  });

  it("clamps the configured TTL into 600..86400", async () => {
    const spy = mockUpstream(() => Response.json(UPSTREAM_OK, { status: 201 }));
    for (const [raw, want] of [
      ["30", 600],
      ["999999", 86400],
      ["not-a-number", DEFAULT_TTL],
    ] as const) {
      const e = { ...env, TURN_TTL_SECONDS: raw } as Env;
      const resp = await handleTurn(new Request("https://h/turn-credentials"), e, freshIp(), undefined);
      expect(((await resp.json()) as { ttl: number }).ttl, raw).toBe(want);
      expect(JSON.parse(String(spy.mock.calls.at(-1)![1]!.body)).ttl, raw).toBe(want);
    }
  });

  it("serves CORS to the Pages origin and refuses other origins", async () => {
    mockUpstream(() => Response.json(UPSTREAM_OK, { status: 201 }));
    const ok = await call(freshIp(), { Origin: "https://doublegate.github.io" });
    expect(ok.headers.get("Access-Control-Allow-Origin")).toBe("https://doublegate.github.io");
    const bad = await call(freshIp(), { Origin: "https://evil.example" });
    expect(bad.status).toBe(403);
    const pre = await exports.default.fetch("https://hosted.test/turn-credentials", {
      method: "OPTIONS",
      headers: { Origin: "http://localhost:8080" },
    });
    expect(pre.status).toBe(204);
    expect(pre.headers.get("Access-Control-Allow-Origin")).toBe("http://localhost:8080");
  });
});

describe("failure paths", () => {
  it("the kill switch refuses without calling upstream", async () => {
    const spy = mockUpstream(() => Response.json(UPSTREAM_OK, { status: 201 }));
    for (const v of ["false", "", undefined]) {
      const e = { ...env, TURN_ENABLED: v } as Env;
      const resp = await handleTurn(new Request("https://h/turn-credentials"), e, freshIp(), undefined);
      expect(resp.status).toBe(503);
      expect(((await resp.json()) as { error: string }).error).toBe("turn-disabled");
    }
    expect(spy).not.toHaveBeenCalled();
  });

  it("an unconfigured key refuses without calling upstream", async () => {
    const spy = mockUpstream(() => Response.json(UPSTREAM_OK, { status: 201 }));
    const e = { ...env, TURN_KEY_API_TOKEN: undefined } as Env;
    const resp = await handleTurn(new Request("https://h/turn-credentials"), e, freshIp(), undefined);
    expect(resp.status).toBe(503);
    expect(((await resp.json()) as { error: string }).error).toBe("turn-not-configured");
    expect(spy).not.toHaveBeenCalled();
  });

  for (const [name, impl] of [
    ["an upstream 500", () => new Response("boom", { status: 500 })],
    ["an upstream 401", () => Response.json({ error: "bad token" }, { status: 401 })],
    ["a network error", () => Promise.reject(new TypeError("network"))],
    ["a non-JSON body", () => new Response("<html>", { status: 201 })],
    ["a body without iceServers", () => Response.json({ servers: [] }, { status: 201 })],
    ["TURN URLs without credentials", () => Response.json({ iceServers: [{ urls: ["turn:x:3478"] }] }, { status: 201 })],
  ] as Array<[string, () => Response | Promise<Response>]>) {
    it(`${name} is a 502 and does not spend the budget`, async () => {
      mockUpstream(impl);
      const before = await env.BUDGET.getByName("global").usage();
      const resp = await call(freshIp());
      expect(resp.status).toBe(502);
      const body = (await resp.json()) as { error: string; message: string };
      expect(body.error).toBe("turn-upstream");
      expect(body.message).toContain("STUN only");
      expect((await env.BUDGET.getByName("global").usage()).minted).toBe(before.minted);
    });
  }

  it("rejects methods other than GET and POST", async () => {
    const resp = await exports.default.fetch("https://hosted.test/turn-credentials", { method: "DELETE" });
    expect(resp.status).toBe(405);
  });
});

describe("spend guards", () => {
  it("limits one IP with 429 and Retry-After", async () => {
    mockUpstream(() => Response.json(UPSTREAM_OK, { status: 201 }));
    const ip = freshIp();
    for (let i = 0; i < 3; i++) expect((await call(ip)).status).toBe(200);
    const limited = await call(ip);
    expect(limited.status).toBe(429);
    expect(Number(limited.headers.get("Retry-After"))).toBeGreaterThan(0);
    expect(((await limited.json()) as { error: string }).error).toBe("rate-limited");
    // A different caller is not affected by this one's limit.
    expect((await call(freshIp())).status).toBe(200);
  });

  it("caps credentials per UTC day across all callers, and resets the next day", async () => {
    const budget = env.BUDGET.getByName("cap-test");
    // 23:00 UTC on a future day (so the real-clock sweep alarm, armed an hour
    // after the first admission, cannot fire during the test), and a moment
    // after the following midnight.
    const d = new Date();
    const day1 = Date.UTC(d.getUTCFullYear(), d.getUTCMonth(), d.getUTCDate() + 2, 23, 0, 0);
    const day2 = day1 + 3_600_001;
    for (let i = 0; i < 5; i++) expect((await budget.admitTurn(`198.18.0.${i}`, day1)).ok).toBe(true);
    const over = await budget.admitTurn("198.18.0.99", day1);
    expect(over).toMatchObject({ ok: false, status: 503, error: "daily-cap", retryAfter: 3600 });
    expect(await budget.usage(day1)).toEqual({ day: utcDay(day1), minted: 5, cap: 5 });
    expect((await budget.admitTurn("198.18.0.99", day2)).ok).toBe(true);
  });

  it("the daily cap reaches the client as a 503 with a STUN-only message", async () => {
    mockUpstream(() => Response.json(UPSTREAM_OK, { status: 201 }));
    let last: Response | undefined;
    for (let i = 0; i < 8; i++) last = await call(freshIp());
    expect(last!.status).toBe(503);
    const body = (await last!.json()) as { error: string; message: string };
    expect(body.error).toBe("daily-cap");
    expect(body.message).toContain("STUN only");
  });

  it("keeps per-IP windows across an eviction of the budget object", async () => {
    const { evictDurableObject } = await import("cloudflare:test");
    const budget = env.BUDGET.getByName("evict-test");
    // Real time: the sweep alarm runs on the real clock, so an injected time
    // in the past would have the alarm fire at once and sweep the window.
    const t = Date.now();
    for (let i = 0; i < 3; i++) expect((await budget.admitTurn("198.18.1.1", t + i)).ok).toBe(true);
    await evictDurableObject(budget);
    expect((await budget.admitTurn("198.18.1.1", t + 10)).ok).toBe(false);
    // The window is an hour: one later it is open again.
    expect((await budget.admitTurn("198.18.1.1", t + 3_600_001)).ok).toBe(true);
  });
});

describe("sanitizeIceServers", () => {
  it("accepts a single-string urls field and rejects junk", () => {
    expect(sanitizeIceServers({ iceServers: [{ urls: "stun:a:3478" }] })).toEqual([{ urls: ["stun:a:3478"] }]);
    expect(sanitizeIceServers({ iceServers: [{ urls: ["http://x"] }] })).toBeUndefined();
    expect(sanitizeIceServers(null)).toBeUndefined();
    expect(sanitizeIceServers({ iceServers: "x" })).toBeUndefined();
  });
});
