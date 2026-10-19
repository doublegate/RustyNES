// SPDX-License-Identifier: GPL-3.0-or-later
//
// The TURN spend guards and the RetroAchievements proxy's abuse guards: one
// Durable Object class, two named instances. Every `/turn-credentials` request
// passes through the `global` instance before anything is minted, and every
// `/ra/dorequest.php` request that would be forwarded passes through the `ra`
// instance first (separate instances, so one service's traffic never queues
// behind the other's).
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
// # The RetroAchievements guards
//
// The proxy forwards to retroachievements.org under RustyNES's own
// User-Agent, which RetroAchievements identifies and allowlists the client
// by (ra.ts). Its Origin check stops other web pages, not other programs: a
// non-browser caller can send the Pages Origin. Unmetered, that would let
// anyone push `login2` attempts (credential stuffing) or unlocks through one
// shared identity, and a ban of that identity, or the Worker's own request
// limits, would take the browser RetroAchievements client down for everyone.
// So the same two counters bound it, plus a third:
//
//   1. per IP: at most RA_PER_IP_PER_HOUR forwarded calls per rolling hour
//      (the casual runtime makes about one call a minute while a game runs:
//      a ping every two minutes, plus unlocks and leaderboard entries);
//   2. per IP: at most RA_LOGINS_PER_IP_PER_HOUR of them `login2`, the call
//      a credential-stuffing run would repeat;
//   3. global: at most RA_DAILY_CAP forwarded calls per UTC day.
//
// Only a request the proxy would actually forward is counted; one it refuses
// (a disallowed call type, hardcore, a malformed body) costs nothing. The
// kill switch is RA_ENABLED (ra.ts).
//
// # Revoking credentials when their TTL ends (v3.4.1, T-HOSTED-TURN-EXPIRY)
//
// The short TTL was meant to bound how long a credential that leaks keeps
// working. Cloudflare does not enforce it: on 2026-10-19 a 3-hour credential
// still obtained a relay 2 h 38 min after its expiry, and a 600 s one 6.6
// minutes after, although Cloudflare's own page says an expired credential is
// disconnected. So the Worker enforces it. Each minted TURN username is stored
// here as `revoke:<due ms, zero-padded>:<username>` (keys sort by due time),
// and the alarm revokes every due one through Cloudflare's revoke endpoint
// (`POST .../credentials/<username>/revoke`, 204 on success). Revocation
// happens at the TTL, never earlier, so a session inside its three hours is
// untouched. A failure is never silent and never cut short:
//
//   - 2xx: done. 404: done too (Cloudflare does not know the credential), and
//     logged, because a 404 from a wrong key id would look the same.
//   - A missing key, 401 or 403: a configuration fault, logged as an error and
//     retried hourly WITHOUT counting as an attempt, so a broken token cannot
//     use up the retries before anyone fixes it.
//   - Anything else (5xx, a timeout): retried with backoff, 5 minutes doubling
//     to an hour.
//   - Only an entry REVOKE_GIVE_UP_MS (48 h, the longest TTL Cloudflare
//     allows) past its due time is dropped, with an error log.
//
// A retry is re-keyed under its next attempt time, so the key order stays the
// order of work; the value keeps the original due time and the attempt count.
// Logs carry the status and attempt count only, never the username, and
// Workers Logs are off in production anyway (wrangler.jsonc).
//
// The object has one alarm. It is set to the earlier of the hourly sweep and
// the next due revocation; whichever fires, both kinds of work are checked.
// The re-arm at the end of an alarm reads the revocation list AGAIN and keeps
// an earlier alarm set meanwhile: a credential minted while the alarm awaited
// Cloudflare would otherwise be overwritten by a later re-arm (the automated
// security review on v3.4.1).
//
// # Why the counts are persisted
//
// A Durable Object is evicted from memory after a short idle period. An
// in-memory window would reset then, and a caller who pauses between bursts
// would multiply the per-IP limit. So both counters live in the object's
// storage; an hourly alarm sweeps expired per-IP windows and past days, so
// storage stays bounded by the number of distinct callers in the last two
// hours (a window that starts just after one sweep is deleted by the sweep
// after next) and no day's count outlives that day by more than an hour.

import { DurableObject } from "cloudflare:workers";
import type { Env } from "./env";
import { intVar, ipKey, loadSalt } from "./limits";
import { TURN_API } from "./turn";

const HOUR_MS = 3_600_000;
/** Storage-key prefix of the pending credential revocations. */
const REVOKE_PREFIX = "revoke:";
/** How long past its due time a revocation keeps being retried (48 h). */
export const REVOKE_GIVE_UP_MS = 48 * HOUR_MS;
/** First retry delay; doubles per attempt up to an hour. */
const REVOKE_RETRY_MS = 5 * 60_000;
const REVOKE_TIMEOUT_MS = 5000;
/** Revocations handled per alarm, so one alarm stays short. */
const REVOKE_BATCH = 50;

interface Revocation {
  attempts: number;
  /** The credential's expiry, when the first attempt was due (ms). */
  dueMs?: number;
}

type RevokeOutcome = "done" | "retry" | "config";

/** `revoke:<due ms, 15 digits>:<username>`, so keys sort by due time. */
function revokeKey(dueMs: number, username: string): string {
  return `${REVOKE_PREFIX}${String(dueMs).padStart(15, "0")}:${username}`;
}

/** The due time and username of a `revoke:` key. */
function parseRevokeKey(key: string): { dueMs: number; username: string } {
  const rest = key.slice(REVOKE_PREFIX.length);
  return { dueMs: Number(rest.slice(0, 15)), username: rest.slice(16) };
}

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

/** Storage-key prefixes of the per-IP hourly windows (swept by the alarm). */
const WINDOW_PREFIXES = ["ip:", "ra-ip:", "ra-login:"] as const;
/** Storage-key prefixes of the per-day counters (past days swept). */
const DAY_PREFIXES = ["day:", "ra-day:"] as const;

export class Budget extends DurableObject<Env> {
  /** Count one credential for `ip` if both guards allow it. */
  async admitTurn(ip: string, now: number = Date.now()): Promise<Admission> {
    const perIp = intVar(this.env.TURN_PER_IP_PER_HOUR, 6, 0, 10000);
    const cap = intVar(this.env.TURN_DAILY_CAP, 500, 0, 10_000_000);
    return this.admit(ip, now, [{ prefix: "ip:", limit: perIp }], "day:", cap);
  }

  /**
   * Count one forwarded RetroAchievements call for `ip` (`login` when it is a
   * `login2`) if all three guards allow it. Nothing is refunded: an upstream
   * failure still sent a request under the shared User-Agent.
   */
  async admitRa(ip: string, login: boolean, now: number = Date.now()): Promise<Admission> {
    const windows = [{ prefix: "ra-ip:", limit: intVar(this.env.RA_PER_IP_PER_HOUR, 240, 0, 100_000) }];
    if (login) windows.push({ prefix: "ra-login:", limit: intVar(this.env.RA_LOGINS_PER_IP_PER_HOUR, 10, 0, 10_000) });
    const cap = intVar(this.env.RA_DAILY_CAP, 50_000, 0, 10_000_000);
    return this.admit(ip, now, windows, "ra-day:", cap);
  }

  /**
   * The shared admission: every hourly window under `windows` must have room
   * for `ip`, and the day counter under `dayPrefix` must be under `cap`; then
   * all of them are counted in one storage write (so a refusal counts nothing).
   */
  private async admit(
    ip: string,
    now: number,
    windows: Array<{ prefix: string; limit: number }>,
    dayPrefix: string,
    cap: number,
  ): Promise<Admission> {
    const salt = await loadSalt(this.ctx.storage);
    const key = await ipKey(ip, salt);
    const day = utcDay(now);

    const writes: Record<string, Window | number> = {};
    for (const { prefix, limit } of windows) {
      const winKey = `${prefix}${key}`;
      const win = await this.ctx.storage.get<Window>(winKey);
      const active = win && now - win.start < HOUR_MS ? win : undefined;
      if ((active?.count ?? 0) >= limit) {
        const retry = active ? Math.ceil((active.start + HOUR_MS - now) / 1000) : 3600;
        return { ok: false, status: 429, error: "rate-limited", retryAfter: Math.max(1, retry) };
      }
      writes[winKey] = active ? { start: active.start, count: active.count + 1 } : { start: now, count: 1 };
    }
    const dayKey = `${dayPrefix}${day}`;
    const used = (await this.ctx.storage.get<number>(dayKey)) ?? 0;
    if (used >= cap) {
      return { ok: false, status: 503, error: "daily-cap", retryAfter: secondsToMidnight(now) };
    }
    writes[dayKey] = used + 1;

    await this.ctx.storage.put(writes);
    // Armed for the sweep unless an alarm already fires within the hour: a
    // pending revocation can hold the single alarm hours out, and the sweep
    // must still delete this window within PRIVACY.md's two hours (CodeRabbit
    // on #618).
    const alarm = await this.ctx.storage.getAlarm();
    if (alarm === null || alarm > now + HOUR_MS) await this.ctx.storage.setAlarm(now + HOUR_MS);
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

  /**
   * Record credentials to revoke at `dueMs` (their expiry), and make sure the
   * alarm fires by then.
   */
  async scheduleRevoke(usernames: string[], dueMs: number): Promise<void> {
    if (usernames.length === 0) return;
    const writes: Record<string, Revocation> = {};
    for (const u of usernames) writes[revokeKey(dueMs, u)] = { attempts: 0 };
    await this.ctx.storage.put(writes);
    const alarm = await this.ctx.storage.getAlarm();
    if (alarm === null || alarm > dueMs) await this.ctx.storage.setAlarm(dueMs);
  }

  /** One revocation request, classified (see "Revoking credentials"). */
  private async revokeOne(username: string): Promise<RevokeOutcome> {
    if (!this.env.TURN_KEY_ID || !this.env.TURN_KEY_API_TOKEN) {
      console.error("turn revoke: TURN key not configured; revocation deferred");
      return "config";
    }
    try {
      const resp = await fetch(
        `${TURN_API}/${encodeURIComponent(this.env.TURN_KEY_ID)}/credentials/${encodeURIComponent(username)}/revoke`,
        {
          method: "POST",
          headers: { Authorization: `Bearer ${this.env.TURN_KEY_API_TOKEN}` },
          signal: AbortSignal.timeout(REVOKE_TIMEOUT_MS),
        },
      );
      if (resp.ok) return "done";
      if (resp.status === 404) {
        console.warn("turn revoke: 404, credential unknown to Cloudflare; treated as revoked");
        return "done";
      }
      if (resp.status === 401 || resp.status === 403) {
        console.error(`turn revoke: ${resp.status}, the TURN key token was refused; revocation deferred`);
        return "config";
      }
      console.warn(`turn revoke: ${resp.status}; will retry`);
      return "retry";
    } catch {
      console.warn("turn revoke: request failed or timed out; will retry");
      return "retry";
    }
  }

  /**
   * Revoke every due credential (at most REVOKE_BATCH per call); a failure
   * is re-keyed under its next attempt time.
   */
  private async revokeDue(now: number): Promise<void> {
    let handled = 0;
    let revoked = 0;
    for (const [key, value] of await this.ctx.storage.list<Revocation>({ prefix: REVOKE_PREFIX })) {
      const { dueMs: attemptAt, username } = parseRevokeKey(key);
      if (attemptAt > now || handled >= REVOKE_BATCH) break; // keys sort by attempt time
      handled += 1;
      const firstDue = value?.dueMs ?? attemptAt;
      const outcome = await this.revokeOne(username);
      if (outcome === "done") {
        await this.ctx.storage.delete(key);
        revoked += 1;
        continue;
      }
      if (now - firstDue >= REVOKE_GIVE_UP_MS) {
        console.error(`turn revoke: given up ${Math.round((now - firstDue) / HOUR_MS)} h after the credential's expiry`);
        await this.ctx.storage.delete(key);
        continue;
      }
      const attempts = (value?.attempts ?? 0) + (outcome === "retry" ? 1 : 0);
      const delay = outcome === "config" ? HOUR_MS : Math.min(REVOKE_RETRY_MS * 2 ** Math.max(0, attempts - 1), HOUR_MS);
      await this.ctx.storage.delete(key);
      await this.ctx.storage.put(revokeKey(now + delay, username), { attempts, dueMs: firstDue });
    }
    // A count only: an operator tailing the Worker can see revocations happen.
    if (revoked > 0) console.log(`turn revoke: ${revoked} revoked`);
  }

  /** When the earliest pending revocation is due (read fresh), if any. */
  private async nextRevocation(now: number): Promise<number | undefined> {
    const first = await this.ctx.storage.list({ prefix: REVOKE_PREFIX, limit: 1 });
    for (const k of first.keys()) return Math.max(parseRevokeKey(k).dueMs, now + 1000);
    return undefined;
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

  /**
   * Revoke the credentials that are due, sweep expired per-IP windows and
   * past days, and re-arm for the earlier of the next sweep and the next
   * revocation.
   */
  async alarm(): Promise<void> {
    const now = Date.now();
    await this.revokeDue(now);
    let remaining = 0;
    const stale: string[] = [];
    for (const prefix of WINDOW_PREFIXES) {
      for (const [k, v] of await this.ctx.storage.list({ prefix })) {
        if (now - (v as Window).start >= HOUR_MS) stale.push(k);
        else remaining += 1;
      }
    }
    for (const prefix of DAY_PREFIXES) {
      const today = `${prefix}${utcDay(now)}`;
      for (const k of (await this.ctx.storage.list({ prefix })).keys()) {
        if (k !== today) stale.push(k);
        else remaining += 1;
      }
    }
    // `delete` takes at most 128 keys per call.
    for (let i = 0; i < stale.length; i += 128) await this.ctx.storage.delete(stale.slice(i, i + 128));
    // Today's counter counts as remaining, so the sweep keeps running until
    // the first run after midnight deletes it: PRIVACY.md's "deleted within
    // an hour after the day ends". With nothing left the alarm stops, and the
    // next admission arms it again.
    const nextSweep = remaining > 0 ? now + HOUR_MS : undefined;
    // Read fresh, after the awaits above: a credential scheduled meanwhile
    // set its own alarm, which must not be pushed later.
    const nextRevoke = await this.nextRevocation(now);
    const pending = await this.ctx.storage.getAlarm();
    const next = Math.min(
      nextSweep ?? Infinity,
      nextRevoke ?? Infinity,
      pending !== null && pending > now ? pending : Infinity,
    );
    if (Number.isFinite(next)) await this.ctx.storage.setAlarm(next);
  }
}
