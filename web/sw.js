// Offline cache so BurnCraft can be installed as a PWA and opened without a connection.
// The page itself comes from the network first, so new versions arrive (each build gives its .wasm/.js new
// names); the cached copy is only used offline. Everything else comes from the cache when it's there.
const CACHE = "burncraft-v2";
self.addEventListener("install", (e) => {
  self.skipWaiting();
  e.waitUntil(caches.open(CACHE).then((c) => c.addAll(["./", "./index.html", "./manifest.webmanifest", "./favicon.png", "./icons/icon-192.png", "./icons/icon-512.png"])));
});
self.addEventListener("activate", (e) => {
  // drop caches from older versions of this worker
  e.waitUntil(
    caches.keys()
      .then((keys) => Promise.all(keys.filter((k) => k !== CACHE).map((k) => caches.delete(k))))
      .then(() => self.clients.claim())
  );
});
self.addEventListener("fetch", (e) => {
  if (e.request.method !== "GET") return;
  const keep = (resp) => {
    if (resp.ok) {
      const copy = resp.clone();
      caches.open(CACHE).then((c) => c.put(e.request, copy));
    }
    return resp;
  };
  if (e.request.mode === "navigate") {
    e.respondWith(
      fetch(e.request).then(keep).catch(() => caches.match(e.request).then((hit) => hit || caches.match("./index.html")))
    );
    return;
  }
  e.respondWith(caches.match(e.request).then((hit) => hit || fetch(e.request).then(keep)));
});
