/**
 * Sonar service worker — offline support for the GitHub Pages app.
 *
 * Strategy:
 *  - the app shell (html/js/css/wasm): cache-first with background refresh
 *  - navigation: network-first, offline fallback to the cached shell
 *
 * The WASM engine (~230 KB) is cached on first load so the whole app —
 * engine, docs, tests, benchmarks — works fully offline.
 */

const CACHE = "sonar-v0.5.0-beta.1-md3";
const ASSETS = [
  "./",
  "./index.html",
  "./style.css",
  "./app.js",
  "./worker.js",
  "./engine.js",
  "./tests.js",
  "./docs-data.js",
  "./docs-verify.js",
  "./bench-live.js",
  "./search.js",
  "./engine.wasm",
  "./manifest.json",
  "./favicon.svg",
];

self.addEventListener("install", (ev) => {
  ev.waitUntil(
    (async () => {
      const cache = await caches.open(CACHE);
      // Add assets individually so one failure does not abort the install.
      await Promise.allSettled(ASSETS.map((a) => cache.add(a)));
      await self.skipWaiting();
    })()
  );
});

self.addEventListener("activate", (ev) => {
  ev.waitUntil(
    (async () => {
      const keys = await caches.keys();
      await Promise.all(keys.filter((k) => k !== CACHE).map((k) => caches.delete(k)));
      await self.clients.claim();
    })()
  );
});

self.addEventListener("fetch", (ev) => {
  const url = new URL(ev.request.url);
  if (url.origin !== self.location.origin) return;

  // Navigation: network-first, fall back to the cached shell.
  if (ev.request.mode === "navigate") {
    ev.respondWith(
      (async () => {
        try {
          const fresh = await fetch(ev.request);
          const cache = await caches.open(CACHE);
          cache.put("./index.html", fresh.clone());
          return fresh;
        } catch {
          const cache = await caches.open(CACHE);
          return (await cache.match("./index.html")) || Response.error();
        }
      })()
    );
    return;
  }

  // Static assets: cache-first.
  ev.respondWith(
    (async () => {
      const cache = await caches.open(CACHE);
      const hit = await cache.match(ev.request, { ignoreSearch: true });
      if (hit) {
        // Refresh in the background.
        ev.waitUntil(
          (async () => {
            try {
              const fresh = await fetch(ev.request);
              if (fresh.ok) cache.put(ev.request, fresh);
            } catch {
              /* offline: keep the cached copy */
            }
          })()
        );
        return hit;
      }
      const fresh = await fetch(ev.request);
      if (fresh.ok) cache.put(ev.request, fresh.clone());
      return fresh;
    })()
  );
});
