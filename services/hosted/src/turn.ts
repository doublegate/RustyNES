// SPDX-License-Identifier: GPL-3.0-or-later
//
// `/turn-credentials`: short-lived Cloudflare Realtime TURN credentials.
//
// The Worker holds the TURN key (TURN_KEY_ID + TURN_KEY_API_TOKEN, Worker
// secrets) and exchanges it, per request, for an ICE server list whose
// username/credential expire after TURN_TTL_SECONDS. The key itself never
// leaves the Worker. Upstream call (Cloudflare docs, "Generate Credentials",
// developers.cloudflare.com/realtime/turn/generate-credentials/, read
// 2026-10-16):
//
//   POST https://rtc.live.cloudflare.com/v1/turn/keys/$TURN_KEY_ID/credentials/generate-ice-servers
//   Authorization: Bearer $TURN_KEY_API_TOKEN
//   {"ttl": <seconds>}
//   -> 201 {"iceServers":[{"urls":[...]},{"urls":[...],"username":"…","credential":"…"}]}
//
// Response to the client:
//
//   200 {"iceServers":[…], "ttl": <seconds>, "expiresAt": <unix seconds>}
//
// or an error object `{"error": <code>, "message": <text>}` whose codes the
// Rust clients map to "fall back to STUN only" (crates/rustynes-netplay/src/
// hosted.rs): `turn-disabled` (the kill switch), `turn-not-configured`,
// `rate-limited` (429, per IP), `daily-cap` (503, global), `turn-upstream`.
//
// # Why the TTL is 3 hours (TURN_TTL_SECONDS = 10800)
//
// A TURN allocation is kept alive by Refresh requests, which authenticate with
// the same long-term credential. Once the credential expires the server
// refuses the next Refresh and the allocation lapses at the end of its current
// lifetime (RFC 8656 section 7: 10 minutes by default). Neither RustyNES client
// re-fetches credentials mid-session (the browser would need
// `RTCPeerConnection.setConfiguration`, the native TURN client a new
// Allocate), so the credential must outlive the session it relays, which is
// also what Cloudflare's page says ("larger than the time you'd expect the
// users to use the TURN service"). A relayed netplay session that outlives the
// TTL drops its relay and the players reconnect through the lobby, which mints
// fresh credentials. Three hours covers a long sitting with one NES game; it
// is the shortest value that does, and shorter is better because the TTL is
// the window in which a leaked credential can be used. Valid range 600-86400.

import { corsHeaders, json } from "./cors";
import type { Env } from "./env";
import { intVar } from "./limits";

export const TURN_API = "https://rtc.live.cloudflare.com/v1/turn/keys";
export const DEFAULT_TTL = 10800;
const UPSTREAM_TIMEOUT_MS = 5000;

export interface IceServer {
  urls: string[];
  username?: string;
  credential?: string;
}

/**
 * Validate the upstream ICE server list. Drops port-53 URLs, which Cloudflare
 * notes browsers block (a client without trickle ICE would wait for them to
 * time out). Returns `undefined` when nothing usable is left.
 */
export function sanitizeIceServers(body: unknown): IceServer[] | undefined {
  if (!body || typeof body !== "object") return undefined;
  const list = (body as { iceServers?: unknown }).iceServers;
  if (!Array.isArray(list)) return undefined;
  const out: IceServer[] = [];
  for (const s of list) {
    if (!s || typeof s !== "object") continue;
    const raw = (s as { urls?: unknown }).urls;
    const urls = (Array.isArray(raw) ? raw : [raw]).filter(
      (u): u is string => typeof u === "string" && /^(stun|turns?):/.test(u) && !/:53(\?|$)/.test(u),
    );
    if (urls.length === 0) continue;
    const { username, credential } = s as { username?: unknown; credential?: unknown };
    const entry: IceServer = { urls };
    if (typeof username === "string" && typeof credential === "string") {
      entry.username = username;
      entry.credential = credential;
    } else if (urls.some((u) => u.startsWith("turn"))) {
      continue; // a TURN URL without credentials is useless
    }
    out.push(entry);
  }
  return out.length > 0 ? out : undefined;
}

export async function handleTurn(
  request: Request,
  env: Env,
  ip: string,
  cors: string | undefined,
): Promise<Response> {
  const h = corsHeaders(cors, "GET, POST, OPTIONS");
  if (request.method !== "GET" && request.method !== "POST") {
    return json(405, { error: "method", message: "use GET or POST" }, { ...h, Allow: "GET, POST, OPTIONS" });
  }
  if (env.TURN_ENABLED !== "true") {
    return json(503, { error: "turn-disabled", message: "TURN relaying is switched off; connect directly (STUN only)." }, h);
  }
  if (!env.TURN_KEY_ID || !env.TURN_KEY_API_TOKEN) {
    return json(503, { error: "turn-not-configured", message: "TURN is not configured on this deployment." }, h);
  }

  const budget = env.BUDGET.getByName("global");
  const admission = await budget.admitTurn(ip);
  if (!admission.ok) {
    const message =
      admission.error === "rate-limited"
        ? "Too many TURN credential requests from this address; try again later or connect directly (STUN only)."
        : "Today's TURN relay budget is used up; connect directly (STUN only) or try again after 00:00 UTC.";
    return json(admission.status, { error: admission.error, message }, { ...h, "Retry-After": String(admission.retryAfter) });
  }

  const ttl = intVar(env.TURN_TTL_SECONDS, DEFAULT_TTL, 600, 86400);
  let servers: IceServer[] | undefined;
  try {
    const resp = await fetch(
      `${TURN_API}/${encodeURIComponent(env.TURN_KEY_ID)}/credentials/generate-ice-servers`,
      {
        method: "POST",
        headers: { Authorization: `Bearer ${env.TURN_KEY_API_TOKEN}`, "Content-Type": "application/json" },
        body: JSON.stringify({ ttl }),
        signal: AbortSignal.timeout(UPSTREAM_TIMEOUT_MS),
      },
    );
    if (resp.ok) servers = sanitizeIceServers(await resp.json());
  } catch {
    servers = undefined; // network error, timeout or a non-JSON body
  }
  if (!servers) {
    // Nothing was issued, so the admission is given back.
    await budget.refundTurn(admission.key, admission.day);
    return json(502, { error: "turn-upstream", message: "Could not obtain TURN credentials; connect directly (STUN only)." }, h);
  }
  const expiresAt = Math.floor(Date.now() / 1000) + ttl;
  return json(200, { iceServers: servers, ttl, expiresAt }, h);
}
