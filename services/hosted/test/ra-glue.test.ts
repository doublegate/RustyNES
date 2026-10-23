// SPDX-License-Identifier: GPL-3.0-or-later
//
// The browser glue's choice of proxy base (crates/rustynes-frontend/web/
// cheevos/ra_glue.js). The page sets it; anything but HTTPS (or plain HTTP to
// this machine) is ignored, because the login request carries the password.

import { afterEach, describe, expect, it } from "vitest";
import {
  ra_init,
  ra_proxy_configured,
  ra_shutdown,
  raProxyBase,
  rcheevosBase,
  resolveProxyBase,
  // @ts-expect-error -- a plain ES module without type declarations
} from "../../../crates/rustynes-frontend/web/cheevos/ra_glue.js";

const g = globalThis as { RUSTYNES_RA_PROXY_BASE?: string };

afterEach(() => {
  delete g.RUSTYNES_RA_PROXY_BASE;
  ra_shutdown();
});

describe("ra_glue.js proxy base", () => {
  it("is unset in the repository, so RA calls fail closed", async () => {
    expect(raProxyBase()).toBe("");
    expect(resolveProxyBase()).toBe("");
    await ra_init();
    expect(raProxyBase()).toBe("");
    expect(ra_proxy_configured()).toBe(false);
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

// v3.5.0 (T-WEB-RA-PROXY-LAZY, Antigravity on #618): the proxy base is
// resolved at ra_init(), as rcheevosBase() is resolved when the module loads,
// not once at import. A page that sets it after the glue was imported (the
// module graph is evaluated before any later <script> or wasm start code runs)
// used to get a working rcheevos module and a proxy that failed closed. The
// test runtime has no rcheevos.wasm, so ra_init() itself returns false here;
// the base is resolved before the module load is attempted, which is what the
// Rust bridge reads next (`ra_proxy_configured()` right after `ra_init()`).
describe("ra_glue.js proxy base is resolved at ra_init, not at import", () => {
  it("honours a base set after the module was imported", async () => {
    // The module is already imported (top of this file), with no base set.
    expect(ra_proxy_configured()).toBe(false);
    g.RUSTYNES_RA_PROXY_BASE = "https://rustynes-hosted.example.workers.dev/ra";
    // Not yet: nothing is read until ra_init().
    expect(ra_proxy_configured()).toBe(false);
    expect(await ra_init()).toBe(false); // no rcheevos.wasm in this runtime
    expect(raProxyBase()).toBe("https://rustynes-hosted.example.workers.dev/ra");
    expect(ra_proxy_configured()).toBe(true);
  });

  it("re-resolves after a shutdown, and refuses a bad base set late", async () => {
    g.RUSTYNES_RA_PROXY_BASE = "http://127.0.0.1:8787/ra";
    await ra_init();
    expect(raProxyBase()).toBe("http://127.0.0.1:8787/ra");
    ra_shutdown();
    g.RUSTYNES_RA_PROXY_BASE = "http://example.com/ra"; // plain HTTP off-host
    await ra_init();
    expect(raProxyBase()).toBe("");
    expect(ra_proxy_configured()).toBe(false);
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

  it("falls back to cheevos/ when the meta value is not a URL", () => {
    withMeta("http://[bad");
    expect(rcheevosBase()).toBe("https://doublegate.github.io/RustyNES/cheevos/");
  });

  it("treats a blank meta value as absent", () => {
    withMeta("   ");
    expect(rcheevosBase()).toBe("https://doublegate.github.io/RustyNES/cheevos/");
  });
});
