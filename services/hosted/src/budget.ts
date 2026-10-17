// SPDX-License-Identifier: GPL-3.0-or-later
//
// The TURN spend guards: one global Durable Object that every
// `/turn-credentials` request passes through before anything is minted.
//
// # Why these guards exist
//
// The endpoint is public (a `*.workers.dev` URL any client, or anyone at all,
// can call), and Cloudflare bills the maintainer's account for TURN egress past
// the free 1,000 GB a month that TURN shares with the Realtime SFU (README.md,
// "What is billed"). Two counters bound how many credentials exist:
//
//   1. per IP: at most TURN_PER_IP_PER_HOUR credentials per rolling hour, so
//      one caller cannot stockpile them;
//   2. global: at most TURN_DAILY_CAP credentials per UTC day, all callers
//      together, so a distributed caller cannot either.
//
// Both refuse with a reason the clients turn into "connect directly (STUN
// only)". Neither bounds BYTES: a credential, once issued, can relay as much
// as the caller pushes until it expires. That is what the short TTL and the
// account's budget alert (README.md, before the first deploy) are for.
//
// # Why the counts are persisted
//
// A Durable Object is evicted from memory after a short idle period. An
// in-memory window would reset then, and a caller who pauses between bursts
// would multiply the per-IP limit. So both counters live in the object's
// storage; an alarm sweeps expired per-IP windows so storage stays bounded by
// the number of distinct callers in the last hour.

import { DurableObject } from "cloudflare:workers";
import type { Env } from "./env";
import { intVar, ipKey, loadSalt } from "./limits";

const HOUR_MS = 3_600_000;

export type Admission =
  | { ok: true; day: string; key: string }
  | { ok: false; status: 429 | 503; error: "rate-limited" | "daily-cap"; retryAfter: number };

interface Window {
  start: number;
  count: number;
}

/** The UTC day a timestamp falls in, `YYYY-MM-DD`. */
export function utcDay(now: number): string {
  return new Date(now).toISOString().slice(0, 10);
}

/** Seconds from `now` to the next UTC midnight. */
function secondsToMidnight(now: number): number {
  const d = new Date(now);
  const next = Date.UTC(d.getUTCFullYear(), d.getUTCMonth(), d.getUTCDate() + 1);
  return Math.max(1, Math.ceil((next - now) / 1000));
}

export class Budget extends DurableObject<Env> {
  /** Count one credential for `ip` if both guards allow it. */
  async admitTurn(ip: string, now: number = Date.now()): Promise<Admission> {
    const salt = await loadSalt(this.ctx.storage);
    const key = await ipKey(ip, salt);
    const perIp = intVar(this.env.TURN_PER_IP_PER_HOUR, 6, 0, 10000);
    const cap = intVar(this.env.TURN_DAILY_CAP, 500, 0, 10_000_000);
    const day = utcDay(now);

    const winKey = `ip:${key}`;
    const win = await this.ctx.storage.get<Window>(winKey);
    const active = win && now - win.start < HOUR_MS ? win : undefined;
    if ((active?.count ?? 0) >= perIp) {
      const retry = active ? Math.ceil((active.start + HOUR_MS - now) / 1000) : 3600;
      return { ok: false, status: 429, error: "rate-limited", retryAfter: Math.max(1, retry) };
    }
    const dayKey = `day:${day}`;
    const used = (await this.ctx.storage.get<number>(dayKey)) ?? 0;
    if (used >= cap) {
      return { ok: false, status: 503, error: "daily-cap", retryAfter: secondsToMidnight(now) };
    }

    await this.ctx.storage.put({
      [winKey]: active ? { start: active.start, count: active.count + 1 } : { start: now, count: 1 },
      [dayKey]: used + 1,
    });
    if ((await this.ctx.storage.getAlarm()) === null) await this.ctx.storage.setAlarm(now + HOUR_MS);
    return { ok: true, day, key };
  }

  /** Give back an admission whose upstream mint failed (nothing was issued). */
  async refundTurn(key: string, day: string): Promise<void> {
    const winKey = `ip:${key}`;
    const dayKey = `day:${day}`;
    const win = await this.ctx.storage.get<Window>(winKey);
    const used = await this.ctx.storage.get<number>(dayKey);
    if (win && win.count > 0) await this.ctx.storage.put(winKey, { ...win, count: win.count - 1 });
    if (used && used > 0) await this.ctx.storage.put(dayKey, used - 1);
  }

  /** Credentials minted today (for the runbook's checks and the tests). */
  async usage(now: number = Date.now()): Promise<{ day: string; minted: number; cap: number }> {
    const day = utcDay(now);
    return {
      day,
      minted: (await this.ctx.storage.get<number>(`day:${day}`)) ?? 0,
      cap: intVar(this.env.TURN_DAILY_CAP, 500, 0, 10_000_000),
    };
  }

  /** Sweep expired per-IP windows and past days; re-arm while any remain. */
  async alarm(): Promise<void> {
    const now = Date.now();
    const today = `day:${utcDay(now)}`;
    let remaining = 0;
    const stale: string[] = [];
    for (const [k, v] of await this.ctx.storage.list({ prefix: "ip:" })) {
      if (now - (v as Window).start >= HOUR_MS) stale.push(k);
      else remaining += 1;
    }
    for (const k of (await this.ctx.storage.list({ prefix: "day:" })).keys()) {
      if (k !== today) stale.push(k);
    }
    // `delete` takes at most 128 keys per call.
    for (let i = 0; i < stale.length; i += 128) await this.ctx.storage.delete(stale.slice(i, i + 128));
    if (remaining > 0) await this.ctx.storage.setAlarm(now + HOUR_MS);
  }
}
