// SPDX-License-Identifier: GPL-3.0-or-later
//
// The browser RetroAchievements proxy. retroachievements.org is replaced by a
// mock of `fetch`; nothing here reaches the network, and no real account or
// credential is used (the "password" below is a placeholder).

import { env, exports } from "cloudflare:workers";
import { afterEach, describe, expect, it, vi } from "vitest";
import "../src/index";
import type { Env } from "../src/env";
import { MAX_RA_BODY, RA_ALLOWED_REQUESTS, handleRa } from "../src/ra";

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

afterEach(() => {
  vi.restoreAllMocks();
});

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
    expect((await handleRa(req, e, PAGES)).status).toBe(503);
    expect(spy).not.toHaveBeenCalled();
  });

  it("refuses to forward under a User-Agent RetroAchievements would not recognise", async () => {
    const spy = mockRa();
    const e = { ...env, RA_USER_AGENT: "SomethingElse/1.0" } as Env;
    const req = new Request("https://h/ra/dorequest.php", { method: "POST", headers: { "Content-Type": FORM }, body: "r=ping" });
    expect((await handleRa(req, e, PAGES)).status).toBe(503);
    expect(spy).not.toHaveBeenCalled();
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
