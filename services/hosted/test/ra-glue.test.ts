// SPDX-License-Identifier: GPL-3.0-or-later
//
// The browser glue's choice of proxy base (crates/rustynes-frontend/web/
// cheevos/ra_glue.js). The page sets it; anything but HTTPS (or plain HTTP to
// this machine) is ignored, because the login request carries the password.

import { afterEach, describe, expect, it } from "vitest";
// @ts-expect-error -- a plain ES module without type declarations
import { RA_PROXY_BASE, resolveProxyBase } from "../../../crates/rustynes-frontend/web/cheevos/ra_glue.js";

const g = globalThis as { RUSTYNES_RA_PROXY_BASE?: string };

afterEach(() => {
  delete g.RUSTYNES_RA_PROXY_BASE;
});

describe("ra_glue.js proxy base", () => {
  it("is unset in the repository, so RA calls fail closed", () => {
    expect(RA_PROXY_BASE).toBe("");
    expect(resolveProxyBase()).toBe("");
  });

  it("takes the hosted base the page sets", () => {
    g.RUSTYNES_RA_PROXY_BASE = "https://rustynes-hosted.example.workers.dev/ra/";
    expect(resolveProxyBase()).toBe("https://rustynes-hosted.example.workers.dev/ra");
    g.RUSTYNES_RA_PROXY_BASE = "http://127.0.0.1:8787/ra";
    expect(resolveProxyBase()).toBe("http://127.0.0.1:8787/ra");
  });

  it("ignores anything that would expose the password", () => {
    for (const bad of ["http://example.com/ra", "javascript:alert(1)", "https://a b/ra", "https://x/ra?y=1", "//x/ra"]) {
      g.RUSTYNES_RA_PROXY_BASE = bad;
      expect(resolveProxyBase(), bad).toBe("");
    }
  });
});
