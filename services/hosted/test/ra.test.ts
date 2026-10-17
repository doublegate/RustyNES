// SPDX-License-Identifier: GPL-3.0-or-later
//
// The browser RetroAchievements proxy. retroachievements.org is replaced by a
// mock of `fetch`; nothing here reaches the network, and no real account or
// credential is used (the "password" below is a placeholder).

import { env, exports } from "cloudflare:workers";
import { runInDurableObject } from "cloudflare:test";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import "../src/index";
import type { Env } from "../src/env";
import { MAX_RA_BODY, RA_ALLOWED_REQUESTS, handleRa, trimTrailingSlashes } from "../src/ra";

const PAGES = "https://doublegate.github.io";
const FORM = "application/x-www-form-urlencoded";

function mockRa(status = 200, body = '{"Success":true}') {
  return vi.spyOn(globalThis, "fetch").mockImplementation(async () =>
    new Response(body, { status, headers: { "Content-Type": "application/json", "Set-Cookie": "upstream=1" } }),
  );
}

const post = (body: string, headers: Record<string, string> = {}, path = "/ra/dorequest.php") =>
  exports.default.fetch(`https://hosted.test${path}`, {
    method: "POST",
    headers: { Origin: PAGES, "Content-Type": FORM, ...headers },
    body,
  });

// Each test starts with empty RA guard counters (the limits are the test
// values in vitest.config.ts: 10 per IP per hour, 3 logins per IP per hour,
// 15 a day in all), so one test's requests do not spend the next one's.
beforeEach(async () => {
  await runInDurableObject(env.BUDGET.getByName("ra"), async (_obj, state) => {
    await state.storage.deleteAll();
  });
});

afterEach(() => {
  vi.restoreAllMocks();
});

let ipSeq = 0;
const freshIp = () => `198.51.100.${++ipSeq}`;

describe("forwarding", () => {
  it("forwards an allowed call with RustyNES's User-Agent and passes the answer back", async () => {
    const spy = mockRa(200, '{"Success":true,"Token":"tok"}');
    const body = "r=login2&u=player&p=placeholder-password";
    const resp = await post(body);
    expect(resp.status).toBe(200);
    expect(await resp.text()).toBe('{"Success":true,"Token":"tok"}');
    expect(resp.headers.get("Access-Control-Allow-Origin")).toBe(PAGES);
    expect(resp.headers.get("Cache-Control")).toBe("no-store");
    // Upstream headers such as cookies are not passed to the browser.
    expect(resp.headers.get("Set-Cookie")).toBeNull();

    const [url, init] = spy.mock.calls[0];
    expect(String(url)).toBe("https://retroachievements.org/dorequest.php");
    const h = init!.headers as Record<string, string>;
    expect(h["User-Agent"]).toBe(env.RA_USER_AGENT);
    expect(h["User-Agent"].startsWith("RustyNES/")).toBe(true);
    expect(h["Content-Type"]).toBe(FORM);
    expect(init!.body).toBe(body);
  });

  it("passes RetroAchievements' own error status through", async () => {
    mockRa(401, '{"Success":false,"Error":"Invalid user/password combination."}');
    const resp = await post("r=login2&u=player&p=wrong");
    expect(resp.status).toBe(401);
    expect(await resp.text()).toContain("Invalid user/password");
  });

  it("accepts every allowlisted request type, casual", async () => {
    mockRa();
    for (const r of RA_ALLOWED_REQUESTS) {
      const resp = await post(`r=${r}&u=player&t=tok&h=0`);
      expect(resp.status, r).toBe(200);
    }
  });

  it("forwards exactly the fields it checked, re-encoded, never the raw bytes", async () => {
    const spy = mockRa();
    // `%2D` decodes to `-`; the upstream body is the parsed set, re-serialised.
    const resp = await post("r=gameid&m=ab%2Dcd&u=player");
    expect(resp.status).toBe(200);
    const sent = String(spy.mock.calls[0][1]!.body);
    expect(sent).toBe(new URLSearchParams([["r", "gameid"], ["m", "ab-cd"], ["u", "player"]]).toString());
    expect([...new URLSearchParams(sent).getAll("r")]).toEqual(["gameid"]);
  });

  it("a network failure is a 502", async () => {
    vi.spyOn(globalThis, "fetch").mockRejectedValue(new TypeError("network"));
    expect((await post("r=ping&u=player&t=tok")).status).toBe(502);
  });

  it("answers the CORS preflight for the Pages origin", async () => {
    const resp = await exports.default.fetch("https://hosted.test/ra/dorequest.php", {
      method: "OPTIONS",
      headers: { Origin: PAGES, "Access-Control-Request-Method": "POST" },
    });
    expect(resp.status).toBe(204);
    expect(resp.headers.get("Access-Control-Allow-Origin")).toBe(PAGES);
    expect(resp.headers.get("Access-Control-Allow-Methods")).toContain("POST");
  });
});

describe("refusals (none reach RetroAchievements)", () => {
  const cases: Array<[string, () => Promise<Response>, number]> = [
    ["an editor call", () => post("r=uploadachievement&u=player&t=tok"), 403],
    ["a code-note write", () => post("r=submitcodenote&u=player&t=tok"), 403],
    ["no request type", () => post("u=player&t=tok"), 403],
    ["a hardcore unlock", () => post("r=awardachievement&u=player&t=tok&a=1&h=1"), 403],
    ["a hardcore session", () => post("r=startsession&u=player&t=tok&g=1&h=1"), 403],
    // Refused as a repeated field (400) before the hardcore check is reached.
    ["a second h=1 hidden behind h=0", () => post("r=awardachievement&u=player&t=tok&h=0&h=1"), 400],
    // PHP keeps the LAST value of a repeated field and URLSearchParams.get the
    // FIRST, so a second `r` would pass the allowlist here and be the request
    // RetroAchievements runs.
    ["a second r hidden behind an allowed one", () => post("r=login2&u=player&r=submitcodenote"), 400],
    ["the same r spelled with a percent-encoded name", () => post("r=ping&%72=uploadachievement&u=player"), 400],
    // PHP reads `name[]` / `name[k]` as an array field and folds `.` and space
    // in a name to `_`; no rcheevos call uses either, so they are refused.
    ["an array-syntax field", () => post("r=ping&h[]=1&u=player"), 400],
    ["a field name PHP would rewrite", () => post("r=ping&h.x=1&u=player"), 400],
    ["an empty field name", () => post("r=ping&=1&u=player"), 400],
    ["another origin", () => post("r=ping", { Origin: "https://evil.example" }), 403],
    [
      "no Origin (not the browser build)",
      () =>
        exports.default.fetch("https://hosted.test/ra/dorequest.php", {
          method: "POST",
          headers: { "Content-Type": FORM },
          body: "r=ping",
        }),
      403,
    ],
    ["a GET", () => exports.default.fetch("https://hosted.test/ra/dorequest.php", { headers: { Origin: PAGES } }), 405],
    ["a query string", () => post("r=ping", {}, "/ra/dorequest.php?r=uploadachievement"), 400],
    ["a JSON body", () => post('{"r":"ping"}', { "Content-Type": "application/json" }), 415],
    ["an oversized body", () => post(`r=ping&x=${"a".repeat(MAX_RA_BODY)}`), 413],
    ["another path", () => post("r=ping", {}, "/ra/other.php"), 404],
  ];
  for (const [name, run, status] of cases) {
    it(name, async () => {
      const spy = mockRa();
      const resp = await run();
      expect(resp.status).toBe(status);
      expect(spy).not.toHaveBeenCalled();
    });
  }

  it("does not mistake a hash value for the hardcore flag", async () => {
    // The reference stub's substring test (`b"h=1" in body`) refuses this.
    mockRa();
    expect((await post("r=gameid&m=0123456789abcdefh=1x&u=player")).status).toBe(200);
  });

  it("the kill switch refuses", async () => {
    const spy = mockRa();
    const e = { ...env, RA_ENABLED: "false" } as Env;
    const req = new Request("https://h/ra/dorequest.php", { method: "POST", headers: { "Content-Type": FORM }, body: "r=ping" });
    expect((await handleRa(req, e, freshIp(), PAGES)).status).toBe(503);
    expect(spy).not.toHaveBeenCalled();
  });

  it("refuses to forward under a User-Agent RetroAchievements would not recognise", async () => {
    const spy = mockRa();
    const e = { ...env, RA_USER_AGENT: "SomethingElse/1.0" } as Env;
    const req = new Request("https://h/ra/dorequest.php", { method: "POST", headers: { "Content-Type": FORM }, body: "r=ping" });
    expect((await handleRa(req, e, freshIp(), PAGES)).status).toBe(503);
    expect(spy).not.toHaveBeenCalled();
  });
});

describe("abuse guards", () => {
  it("limits one address per hour with 429 and Retry-After", async () => {
    const spy = mockRa();
    const ip = freshIp();
    for (let i = 0; i < 10; i++) expect((await post("r=ping&u=player&t=tok", { "CF-Connecting-IP": ip })).status, `ping ${i}`).toBe(200);
    const limited = await post("r=ping&u=player&t=tok", { "CF-Connecting-IP": ip });
    expect(limited.status).toBe(429);
    expect(Number(limited.headers.get("Retry-After"))).toBeGreaterThan(0);
    expect(limited.headers.get("Access-Control-Allow-Origin")).toBe(PAGES);
    expect(((await limited.json()) as { Success: boolean }).Success).toBe(false);
    expect(spy).toHaveBeenCalledTimes(10);
    // Another address is not affected.
    expect((await post("r=ping&u=player&t=tok", { "CF-Connecting-IP": freshIp() })).status).toBe(200);
  });

  it("limits logins per address more tightly than other calls", async () => {
    const spy = mockRa();
    const ip = freshIp();
    for (let i = 0; i < 3; i++) expect((await post("r=login2&u=player&p=placeholder", { "CF-Connecting-IP": ip })).status).toBe(200);
    expect((await post("r=login2&u=player&p=placeholder", { "CF-Connecting-IP": ip })).status).toBe(429);
    expect(spy).toHaveBeenCalledTimes(3);
    // The same address's other calls still pass.
    expect((await post("r=ping&u=player&t=tok", { "CF-Connecting-IP": ip })).status).toBe(200);
  });

  it("caps forwarded calls per UTC day across all addresses", async () => {
    const spy = mockRa();
    for (let i = 0; i < 15; i++) expect((await post("r=ping&u=player&t=tok", { "CF-Connecting-IP": freshIp() })).status, `call ${i}`).toBe(200);
    const over = await post("r=ping&u=player&t=tok", { "CF-Connecting-IP": freshIp() });
    expect(over.status).toBe(503);
    expect(Number(over.headers.get("Retry-After"))).toBeGreaterThan(0);
    expect(spy).toHaveBeenCalledTimes(15);
  });

  it("the hourly sweep deletes expired RA windows and past RA days", async () => {
    await runInDurableObject(env.BUDGET.getByName("ra"), async (obj, state) => {
      const now = Date.now();
      await state.storage.put({
        "ra-ip:stale": { start: now - 3_600_001, count: 4 },
        "ra-login:stale": { start: now - 3_600_001, count: 1 },
        "ra-ip:live": { start: now, count: 1 },
        "ra-day:2000-01-01": 9,
      });
      await obj.alarm();
      const left = [...(await state.storage.list()).keys()].sort();
      expect(left).toEqual(["ra-ip:live"]);
    });
  });

  // PRIVACY.md says the day's count is deleted within an hour after the UTC
  // day ends: so the sweep must keep running while today's counter exists,
  // even when every per-IP window has already gone.
  it("the sweep stays armed while only today's counter is left, and stops when nothing is", async () => {
    await runInDurableObject(env.BUDGET.getByName("ra"), async (obj, state) => {
      const d = new Date().toISOString().slice(0, 10);
      await state.storage.put(`ra-day:${d}`, 3);
      await state.storage.deleteAlarm();
      await obj.alarm();
      expect(await state.storage.getAlarm()).not.toBeNull();
      await state.storage.deleteAll();
      await state.storage.deleteAlarm();
      await obj.alarm();
      expect(await state.storage.getAlarm()).toBeNull();
    });
  });

  it("refused requests spend nothing", async () => {
    const spy = mockRa();
    const ip = freshIp();
    for (let i = 0; i < 20; i++) expect((await post("r=uploadachievement&u=player&t=tok", { "CF-Connecting-IP": ip })).status).toBe(403);
    expect((await post("r=ping&u=player&t=tok", { "CF-Connecting-IP": ip })).status).toBe(200);
    expect(spy).toHaveBeenCalledTimes(1);
  });
});

describe("trimTrailingSlashes", () => {
  it("strips only trailing slashes", () => {
    expect(trimTrailingSlashes("https://retroachievements.org")).toBe("https://retroachievements.org");
    expect(trimTrailingSlashes("https://retroachievements.org///")).toBe("https://retroachievements.org");
    expect(trimTrailingSlashes("https://a/b/")).toBe("https://a/b");
    expect(trimTrailingSlashes("///")).toBe("");
    expect(trimTrailingSlashes("")).toBe("");
  });

  it("is linear on a long run of slashes not at the end (the CodeQL input)", () => {
    // `/\/+$/` backtracks quadratically on this (measured on Node 26: 10k
    // slashes 66 ms, 20k 265 ms, 40k 1.07 s, so 200k is about half a
    // minute); the scan takes well under a millisecond.
    const s = `${"/".repeat(200_000)}x`;
    const t0 = performance.now();
    expect(trimTrailingSlashes(s)).toBe(s);
    expect(trimTrailingSlashes(`${s}${"/".repeat(200_000)}`)).toBe(s);
    expect(performance.now() - t0).toBeLessThan(500);
  });
});

describe("credentials", () => {
  it("never appear in anything the Worker logs", async () => {
    mockRa();
    const logs: unknown[] = [];
    for (const m of ["log", "info", "warn", "error", "debug"] as const) {
      vi.spyOn(console, m).mockImplementation((...a: unknown[]) => {
        logs.push(a);
      });
    }
    await post("r=login2&u=player&p=placeholder-password");
    await post("r=awardachievement&u=player&t=secret-token&h=1");
    expect(JSON.stringify(logs)).not.toContain("placeholder-password");
    expect(JSON.stringify(logs)).not.toContain("secret-token");
  });
});
