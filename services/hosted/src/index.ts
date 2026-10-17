// SPDX-License-Identifier: GPL-3.0-or-later
//
// RustyNES hosted services (v3.4.0): the request router.
//
//   GET  /signal (or /)        WebSocket upgrade -> the netplay lobby (lobby.ts)
//   GET|POST /turn-credentials short-lived TURN ICE servers (turn.ts)
//   POST /ra/dorequest.php     the browser RetroAchievements proxy (ra.ts)
//   GET  /healthz              liveness, for the runbook's checks
//
// Every route applies the same Origin rule first (cors.ts): a browser page
// that is not the RustyNES site (or localhost, in development) is refused.
// The client IP Cloudflare reports (`CF-Connecting-IP`) is passed to the
// Durable Objects for their per-IP limits, which hash it immediately; the
// Worker itself neither logs nor stores it.

import { allowedOrigin, corsHeaders, json } from "./cors";
import { CLIENT_IP_HEADER, type Env } from "./env";
import { handleRa } from "./ra";
import { handleTurn } from "./turn";

export { Budget } from "./budget";
export { Lobby } from "./lobby";

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    const url = new URL(request.url);
    const path = url.pathname;
    const origin = request.headers.get("Origin");
    const cors = origin === null ? undefined : allowedOrigin(origin, env);
    if (origin !== null && cors === undefined) {
      return json(403, { error: "origin", message: "This origin may not use the RustyNES services." });
    }
    const ip = request.headers.get("CF-Connecting-IP") ?? "unknown";

    if (path === "/signal" || path === "/") {
      if (request.headers.get("Upgrade")?.toLowerCase() !== "websocket") {
        return new Response("RustyNES netplay lobby: connect with a WebSocket.\n", {
          status: path === "/" ? 200 : 426,
          headers: { "Content-Type": "text/plain" },
        });
      }
      // Re-issue the request with the IP header set by US (a client-sent
      // copy is overwritten, so it cannot pick its own rate-limit key).
      const headers = new Headers(request.headers);
      headers.set(CLIENT_IP_HEADER, ip);
      return env.LOBBY.getByName("lobby").fetch(new Request(request.url, { headers }));
    }

    if (path === "/turn-credentials") {
      if (request.method === "OPTIONS") return new Response(null, { status: 204, headers: corsHeaders(cors, "GET, POST, OPTIONS") });
      return handleTurn(request, env, ip, cors);
    }

    if (path === "/ra/dorequest.php") {
      if (request.method === "OPTIONS") return new Response(null, { status: 204, headers: corsHeaders(cors, "POST, OPTIONS") });
      return handleRa(request, env, cors);
    }

    if (path === "/healthz") return new Response("ok\n", { headers: { "Content-Type": "text/plain" } });
    return json(404, { error: "not-found" });
  },
} satisfies ExportedHandler<Env>;
