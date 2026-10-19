// SPDX-License-Identifier: GPL-3.0-or-later
//
// The browser glue's choice of proxy base (crates/rustynes-frontend/web/
// cheevos/ra_glue.js). The page sets it; anything but HTTPS (or plain HTTP to
// this machine) is ignored, because the login request carries the password.

import { afterEach, describe, expect, it } from "vitest";
// @ts-expect-error -- a plain ES module without type declarations
import { RA_PROXY_BASE, rcheevosBase, resolveProxyBase } from "../../../crates/rustynes-frontend/web/cheevos/ra_glue.js";

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

// v3.4.1: where the Emscripten module is loaded from. The glue is served from
// trunk's snippets/ directory, so the base is resolved against the PAGE (its
// `baseURI`), from `<meta name="rustynes-rcheevos-base">` or `cheevos/`. The
// Workers test runtime has no DOM, so `document` is stubbed per case.
describe("ra_glue.js rcheevos base", () => {
  const page = "https://doublegate.github.io/RustyNES/";
  const d = globalThis as { document?: unknown };
  const withMeta = (content: string | null) => {
    d.document = {
      baseURI: page,
      querySelector: () => (content === null ? null : { getAttribute: () => content }),
    };
  };
  afterEach(() => {
    delete d.document;
  });

  it("defaults to cheevos/ beside the page, not beside the glue", () => {
    withMeta(null);
    expect(rcheevosBase()).toBe("https://doublegate.github.io/RustyNES/cheevos/");
  });

  it("takes the meta tag, trimmed, with a trailing slash added", () => {
    withMeta("  /assets/rc  ");
    expect(rcheevosBase()).toBe("https://doublegate.github.io/assets/rc/");
    withMeta("https://cdn.example/rc/");
    expect(rcheevosBase()).toBe("https://cdn.example/rc/");
  });

  it("treats a blank meta value as absent", () => {
    withMeta("   ");
    expect(rcheevosBase()).toBe("https://doublegate.github.io/RustyNES/cheevos/");
  });
});
