// Offline cache so BurnCraft can be installed as a PWA and opened without a connection.
const CACHE = "burncraft-v1";
self.addEventListener("install", (e) => {
  e.waitUntil(caches.open(CACHE).then((c) => c.addAll(["./", "./index.html", "./manifest.webmanifest", "./favicon.png", "./icons/icon-192.png", "./icons/icon-512.png"])));
});
self.addEventListener("fetch", (e) => {
  e.respondWith(
    caches.match(e.request).then((hit) =>
      hit || fetch(e.request).then((resp) => {
        const copy = resp.clone();
        caches.open(CACHE).then((c) => c.put(e.request, copy));
        return resp;
      })
    )
  );
});
