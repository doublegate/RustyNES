// SPDX-License-Identifier: GPL-3.0-or-later
//
// `/ra/dorequest.php`: the browser RetroAchievements proxy (T-RA-PROXY,
// ADR 0015).
//
// # Why a proxy at all
//
// RetroAchievements identifies and allowlists a client by its HTTP
// `User-Agent` (natively `RustyNES/<ver> (<OS> <arch>) rcheevos/<ver>`, the RA_USER_AGENT
// const in crates/rustynes-cheevos/src/http.rs). Browsers forbid scripts from
// setting `User-Agent`, so the web build's rcheevos calls come here
// (`ra_glue.js` posts to `RA_PROXY_BASE + "/dorequest.php"`), and this handler
// forwards them to retroachievements.org with RustyNES's User-Agent. It is the
// contract of scripts/cheevos/auth-proxy.example.toml, hosted.
//
// # What it refuses
//
//   - any origin but the Pages site and (when enabled) localhost, and requests
//     with no Origin at all (only the browser build needs this proxy);
//   - anything but `POST /ra/dorequest.php` with a form body and no query
//     string (rcheevos sends every parameter in the body);
//   - an `r=` request type outside RA_ALLOWED_REQUESTS: exactly the calls the
//     vendored rc_client 12.5 makes on the casual runtime path. The editor
//     and upload calls (`submitcodenote`, `uploadachievement`, ...) and the
//     account-wide queries are not forwarded;
//   - any request with `h=1`, the hardcore flag (`startsession`,
//     `awardachievement`, `submitlbentry`, `ping`). Hardcore cannot be trusted
//     in a browser, and the proxy is the third of ADR 0015's three structural
//     casual-only layers. The parameter is parsed, not substring-matched: the
//     reference stub's `b"h=1" in body` would also refuse, e.g., `hash=1…`;
//   - more than the abuse guards allow (budget.ts, "The RetroAchievements
//     guards"): a per-IP hourly limit, a tighter per-IP limit on `login2`, and
//     a daily cap across all callers. The Origin check stops other web pages,
//     not other programs, which can send any Origin; these bound what such a
//     program can push through RustyNES's shared User-Agent. Checked last, so
//     only a request that would really be forwarded is counted.
//
// # Credentials
//
// `login2` carries the user's password (`p=`) or token (`t=`); every later
// call carries the token. They pass through to RetroAchievements and back and
// are never stored, logged or echoed by the Worker: nothing here writes to the
// console, and Workers Logs are off (wrangler.jsonc). PRIVACY.md says so.

import { corsHeaders, json } from "./cors";
import type { Env } from "./env";

/** The request types the casual browser runtime needs (rc_client 12.5.0). */
export const RA_ALLOWED_REQUESTS: ReadonlySet<string> = new Set([
  "login2", // rc_client_begin_login_with_password / _with_token
  "gameid", // resolve a ROM hash to a game id
  "achievementsets", // fetch the game's achievement sets (rc_client 12.x)
  "patch", // the older game-data call, kept for rcheevos fallbacks
  "startsession", // begin a play session (casual: h=0)
  "ping", // rich presence / keep-alive
  "awardachievement", // a casual unlock
  "submitlbentry", // a leaderboard entry (casual)
]);

export const MAX_RA_BODY = 64 * 1024;
/** A form field name rcheevos sends: letters, digits and `_` only. */
const FIELD_NAME = /^[A-Za-z0-9_]+$/;
const UPSTREAM_TIMEOUT_MS = 15000;

/** `s` without its trailing `/` characters (a linear scan, no regex). */
export function trimTrailingSlashes(s: string): string {
  let end = s.length;
  while (end > 0 && s.charCodeAt(end - 1) === 0x2f) end -= 1;
  return s.slice(0, end);
}

export async function handleRa(request: Request, env: Env, ip: string, cors: string | undefined): Promise<Response> {
  const h = corsHeaders(cors, "POST, OPTIONS");
  if (env.RA_ENABLED !== "true") {
    return json(503, { Success: false, Error: "The RetroAchievements proxy is switched off." }, h);
  }
  if (!cors) {
    // No (allowed) Origin: not the browser build. The Worker's router has
    // already refused a disallowed Origin; this catches a missing one.
    return json(403, { Success: false, Error: "This proxy serves the RustyNES web build only." }, h);
  }
  if (request.method !== "POST") {
    return json(405, { Success: false, Error: "Use POST." }, { ...h, Allow: "POST, OPTIONS" });
  }
  if (new URL(request.url).search !== "") {
    return json(400, { Success: false, Error: "Parameters belong in the body." }, h);
  }
  const ctype = request.headers.get("Content-Type") ?? "";
  if (!ctype.toLowerCase().startsWith("application/x-www-form-urlencoded")) {
    return json(415, { Success: false, Error: "Expected a form body." }, h);
  }
  const declared = Number(request.headers.get("Content-Length") ?? "0");
  if (declared > MAX_RA_BODY) return json(413, { Success: false, Error: "Body too large." }, h);
  const buf = await request.arrayBuffer();
  if (buf.byteLength > MAX_RA_BODY) return json(413, { Success: false, Error: "Body too large." }, h);
  const params = new URLSearchParams(new TextDecoder().decode(buf));

  // The checks below must see exactly what RetroAchievements will run, and
  // its PHP parses a form differently from URLSearchParams: a repeated field
  // keeps its LAST value in PHP and its FIRST here (`r=login2&r=submitcodenote`
  // would pass the allowlist and run the editor call), `name[]` becomes an
  // array, and `.` or a space in a name is folded to `_`. So every field name
  // must be a plain identifier, `r` and `h` may each appear at most once, and the body
  // sent upstream is this parsed set re-serialised, never the raw bytes (a
  // percent-encoded name such as `%72` then arrives as the `r` it decodes to,
  // and was counted as one here).
  for (const key of params.keys()) {
    if (!FIELD_NAME.test(key)) {
      return json(400, { Success: false, Error: "Unexpected field name." }, h);
    }
  }
  if (params.getAll("r").length > 1 || params.getAll("h").length > 1) {
    return json(400, { Success: false, Error: "A field is repeated." }, h);
  }
  const body = params.toString();

  const r = params.get("r") ?? "";
  if (!RA_ALLOWED_REQUESTS.has(r)) {
    return json(403, { Success: false, Error: "This request type is not available through the browser proxy." }, h);
  }
  if (params.getAll("h").some((v) => v.trim() !== "0" && v.trim() !== "")) {
    return json(403, { Success: false, Error: "Hardcore is not available in the browser." }, h);
  }

  // Linear, not `.replace(/\/+$/, "")`: that regex backtracks quadratically
  // on a long run of `/` not at the end (CodeQL js/polynomial-redos). The
  // value is the operator's var, not request input, but the scan costs
  // nothing and leaves no pattern to audit.
  const upstream = trimTrailingSlashes(env.RA_UPSTREAM ?? "https://retroachievements.org");
  const ua = env.RA_USER_AGENT ?? "";
  if (!ua.startsWith("RustyNES/")) {
    return json(503, { Success: false, Error: "The proxy's User-Agent is misconfigured." }, h);
  }

  const admission = await env.BUDGET.getByName("ra").admitRa(ip, r === "login2");
  if (!admission.ok) {
    const message =
      admission.error === "rate-limited"
        ? "Too many RetroAchievements requests from this address; try again later."
        : "The RetroAchievements proxy's daily limit is used up; try again after 00:00 UTC.";
    return json(admission.status, { Success: false, Error: message }, { ...h, "Retry-After": String(admission.retryAfter) });
  }
  let resp: Response;
  try {
    resp = await fetch(`${upstream}/dorequest.php`, {
      method: "POST",
      headers: { "User-Agent": ua, "Content-Type": "application/x-www-form-urlencoded" },
      body,
      redirect: "manual",
      signal: AbortSignal.timeout(UPSTREAM_TIMEOUT_MS),
    });
  } catch {
    return json(502, { Success: false, Error: "RetroAchievements could not be reached." }, h);
  }
  // Forward the real status and body (a 401 for a bad password is meaningful
  // to rcheevos), never upstream headers such as Set-Cookie.
  const text = await resp.text();
  return new Response(text, {
    status: resp.status,
    headers: {
      "Content-Type": resp.headers.get("Content-Type") ?? "application/json",
      "Cache-Control": "no-store",
      ...h,
    },
  });
}
