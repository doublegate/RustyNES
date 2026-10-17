// SPDX-License-Identifier: GPL-3.0-or-later
//
// Small, allocation-light limiters shared by the two Durable Objects, plus the
// helpers that turn a client IP into a pseudonymous key and read numeric vars.
//
// Every limiter here lives in ONE Durable Object instance (the lobby, or the
// budget), so its counts are global for the deployment rather than per edge
// isolate: a plain Worker's memory is per isolate and per location, which
// would let a caller multiply any limit by the number of places it can reach.

/**
 * A fixed-window counter per key: at most `limit` events per `windowMs`.
 *
 * Fixed windows allow a burst of up to 2x `limit` across a window boundary;
 * that is accepted here because every limit is a spend or abuse bound with
 * headroom, not a precise quota. Stale keys are swept lazily so the map stays
 * bounded by the number of keys active in the current window.
 */
export class WindowLimiter {
  private windows = new Map<string, { start: number; count: number }>();
  private lastSweep = 0;

  constructor(
    private readonly limit: number,
    private readonly windowMs: number,
  ) {}

  /** Count one event for `key`; `false` (and no count) when over the limit. */
  hit(key: string, now: number): boolean {
    this.sweep(now);
    const w = this.windows.get(key);
    if (!w || now - w.start >= this.windowMs) {
      if (this.limit < 1) return false;
      this.windows.set(key, { start: now, count: 1 });
      return true;
    }
    if (w.count >= this.limit) return false;
    w.count += 1;
    return true;
  }

  /** Undo one counted event (for an operation that failed afterwards). */
  refund(key: string): void {
    const w = this.windows.get(key);
    if (w && w.count > 0) w.count -= 1;
  }

  private sweep(now: number): void {
    if (now - this.lastSweep < this.windowMs) return;
    this.lastSweep = now;
    for (const [k, w] of this.windows) if (now - w.start >= this.windowMs) this.windows.delete(k);
  }
}

/**
 * A token bucket: `burst` tokens, refilled at `perSecond`. Used per signalling
 * socket, where a steady trickle (ICE candidates) is normal and a flood is not.
 */
export class TokenBucket {
  private tokens: number;
  private last: number;

  constructor(
    private readonly burst: number,
    private readonly perSecond: number,
    now: number,
  ) {
    this.tokens = burst;
    this.last = now;
  }

  take(now: number): boolean {
    this.tokens = Math.min(this.burst, this.tokens + ((now - this.last) / 1000) * this.perSecond);
    this.last = now;
    if (this.tokens < 1) return false;
    this.tokens -= 1;
    return true;
  }
}

/** Parse an integer var, falling back to `def` and clamping into `[min, max]`. */
export function intVar(raw: string | undefined, def: number, min: number, max: number): number {
  const n = raw === undefined || raw.trim() === "" ? def : Number.parseInt(raw, 10);
  if (!Number.isFinite(n)) return def;
  return Math.min(max, Math.max(min, n));
}

/**
 * A salted SHA-256 of the client IP, hex, truncated to 128 bits.
 *
 * The services never need the IP itself, only "is this the same caller", so
 * the Durable Objects keep this key and not the address. The salt is random
 * per Durable Object and kept in its storage; without it, an IPv4 hash could be
 * reversed by trying all 2^32 addresses. Not anonymisation in the legal sense
 * (the key still singles one caller out while the salt exists), which is why
 * PRIVACY.md calls it pseudonymous and states how long it lives.
 */
export async function ipKey(ip: string, salt: string): Promise<string> {
  const data = new TextEncoder().encode(`${salt}|${ip}`);
  const digest = new Uint8Array(await crypto.subtle.digest("SHA-256", data));
  let hex = "";
  for (let i = 0; i < 16; i++) hex += digest[i].toString(16).padStart(2, "0");
  return hex;
}

/** Load (or create once) a Durable Object's random salt. */
export async function loadSalt(storage: DurableObjectStorage): Promise<string> {
  const existing = await storage.get<string>("ip-salt");
  if (existing) return existing;
  const bytes = crypto.getRandomValues(new Uint8Array(32));
  const salt = [...bytes].map((b) => b.toString(16).padStart(2, "0")).join("");
  await storage.put("ip-salt", salt);
  return salt;
}
