// v1.7.0 "Forge" beta.5 Workstream H6 — RustyNES PWA service worker.
//
// Makes the wasm demo installable + offline-capable. Trunk hashes the
// `.wasm` / `.js` glue filenames per build, so a fixed precache manifest
// would go stale on every rebuild; instead this uses a runtime
// cache-first-then-network strategy over same-origin GETs. The app shell
// (HTML, wasm, JS glue, icons, manifest) is cached the first time it loads,
// so a subsequent offline visit serves from the cache. ROMs are loaded by
// the user from local disk and never fetched over the network, so nothing
// proprietary is ever cached.
//
// Bump CACHE_NAME to evict an old shell after a deploy.

"use strict";

const CACHE_NAME = "rustynes-shell-v1";

// On install, take over as soon as possible (no waiting for old clients).
self.addEventListener("install", (event) => {
    self.skipWaiting();
});

// On activate, drop any caches from a previous CACHE_NAME so a new deploy's
// shell is not served the stale wasm.
self.addEventListener("activate", (event) => {
    event.waitUntil(
        (async () => {
            const keys = await caches.keys();
            await Promise.all(
                keys.filter((k) => k !== CACHE_NAME).map((k) => caches.delete(k))
            );
            await self.clients.claim();
        })()
    );
});

// Cache-first for same-origin GETs; fall back to (and populate the cache
// from) the network. Cross-origin and non-GET requests pass straight through.
self.addEventListener("fetch", (event) => {
    const req = event.request;
    if (req.method !== "GET") {
        return;
    }
    const url = new URL(req.url);
    if (url.origin !== self.location.origin) {
        return;
    }
    event.respondWith(
        (async () => {
            const cache = await caches.open(CACHE_NAME);
            const cached = await cache.match(req);
            if (cached) {
                // Refresh the cache entry in the background (best-effort).
                event.waitUntil(
                    fetch(req)
                        .then((resp) => {
                            if (resp && resp.ok) {
                                cache.put(req, resp.clone());
                            }
                        })
                        .catch(() => {})
                );
                return cached;
            }
            try {
                const resp = await fetch(req);
                if (resp && resp.ok) {
                    cache.put(req, resp.clone());
                }
                return resp;
            } catch (err) {
                // Offline + uncached: nothing we can do. Let it fail.
                return Response.error();
            }
        })()
    );
});
