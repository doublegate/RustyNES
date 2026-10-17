// SPDX-License-Identifier: GPL-3.0-or-later
//
// Origin checks and CORS headers.
//
// The services are meant for the RustyNES web build on GitHub Pages
// (https://doublegate.github.io) and for local development (`trunk serve` on
// http://localhost:<port> or http://127.0.0.1:<port>). A browser always sends
// `Origin` on a cross-origin fetch and on a WebSocket handshake, so a request
// from any OTHER page is refused outright rather than merely denied a CORS
// header: that keeps other sites from building on the lobby, the TURN budget
// or the RetroAchievements proxy from their visitors' browsers.
//
// A request with no `Origin` is a native client (the Android/iOS apps, the
// desktop build), which the lobby and TURN endpoints serve. This is NOT
// authentication: a non-browser caller can send any `Origin` it likes, which is
// why the TURN endpoint also has its own spend guards (budget.ts).

import type { Env } from "./env";

const LOCALHOST = /^http:\/\/(localhost|127\.0\.0\.1)(:\d{1,5})?$/;

/** The configured allowlist, or `undefined` when `origin` is not on it. */
export function allowedOrigin(origin: string, env: Env): string | undefined {
  const list = (env.ALLOWED_ORIGINS ?? "")
    .split(",")
    .map((o) => o.trim())
    .filter((o) => o !== "");
  // Return the CONFIGURED string, not the request's, so nothing the client
  // sent is reflected into a response header.
  const hit = list.find((o) => o === origin);
  if (hit) return hit;
  if (env.ALLOW_LOCALHOST === "true" && LOCALHOST.test(origin)) return origin;
  return undefined;
}

/** CORS response headers for an allowed origin (none for a native client). */
export function corsHeaders(origin: string | undefined, methods: string): Record<string, string> {
  const h: Record<string, string> = { Vary: "Origin" };
  if (origin) {
    h["Access-Control-Allow-Origin"] = origin;
    h["Access-Control-Allow-Methods"] = methods;
    h["Access-Control-Allow-Headers"] = "Content-Type";
    h["Access-Control-Max-Age"] = "600";
  }
  return h;
}

/** A JSON response with no-store caching and the given extra headers. */
export function json(status: number, body: unknown, headers: Record<string, string> = {}): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "Content-Type": "application/json", "Cache-Control": "no-store", ...headers },
  });
}
