// SPDX-License-Identifier: GPL-3.0-or-later
//
// The in-memory WindowLimiter's retention, which PRIVACY.md states ("within
// two minutes while the lobby is in use"): an expired entry must be gone by
// the first prune that comes a full window after the previous one, i.e. less
// than two windows after the entry was created.

import { describe, expect, it } from "vitest";
import { WindowLimiter } from "../src/limits";

const MIN = 60_000;

describe("WindowLimiter retention", () => {
  it("an entry created just after a prune is gone within two windows", () => {
    const l = new WindowLimiter(5, MIN);
    const t0 = 1_000_000_000;
    l.prune(t0); // the lobby's first activity prunes (lastPrune was 0)
    expect(l.hit("a", t0 + 1)).toBe(true); // just after that prune
    expect(l.size).toBe(1);
    // A prune a full window after the last one, while "a" is still 1 ms short
    // of expiry, keeps it.
    l.prune(t0 + MIN);
    expect(l.size).toBe(1);
    // The next one, a window later, deletes it: age 2 windows - 1 ms.
    l.prune(t0 + 2 * MIN);
    expect(l.size).toBe(0);
  });

  it("prunes no more than once per window, however busy", () => {
    const l = new WindowLimiter(1000, MIN);
    const t0 = 1_000_000_000;
    l.prune(t0);
    l.hit("a", t0);
    for (let t = t0 + MIN; t < t0 + 2 * MIN; t += 1000) {
      l.hit("b", t);
      l.prune(t);
    }
    // "a" expired at t0 + MIN and was pruned there; "b" is live.
    expect(l.size).toBe(1);
  });
});
