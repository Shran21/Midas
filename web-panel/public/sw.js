const CACHE = "midas-static-v1";
const SHELL = ["/", "/manifest.webmanifest", "/icon.svg"];

// Only the static interface is cached; API, health and session data must never be served from cache.
function isStatic(url) {
  return (
    url.origin === self.location.origin &&
    (url.pathname.startsWith("/assets/") || SHELL.includes(url.pathname))
  );
}

self.addEventListener("install", (event) => {
  event.waitUntil(caches.open(CACHE).then((cache) => cache.addAll(SHELL)));
  self.skipWaiting();
});

self.addEventListener("activate", (event) => {
  event.waitUntil(
    caches
      .keys()
      .then((keys) =>
        Promise.all(
          keys.filter((key) => key !== CACHE).map((key) => caches.delete(key)),
        ),
      )
      .then(() => self.clients.claim()),
  );
});

async function store(request, response) {
  if (response.ok && response.type === "basic") {
    const cache = await caches.open(CACHE);
    await cache.put(request, response.clone());
  }
  return response;
}

self.addEventListener("fetch", (event) => {
  const request = event.request;
  if (request.method !== "GET") return;
  const url = new URL(request.url);
  if (
    url.origin !== self.location.origin ||
    /^\/(api|health)(\/|$)/.test(url.pathname)
  )
    return;
  if (request.mode === "navigate") {
    event.respondWith(
      fetch(request)
        .then((response) =>
          (response.headers.get("content-type") ?? "").startsWith("text/html")
            ? store(new Request("/"), response)
            : response,
        )
        .catch(() =>
          caches.match("/").then((cached) => cached ?? Response.error()),
        ),
    );
    return;
  }
  if (!isStatic(url)) return;
  event.respondWith(
    caches
      .match(request)
      .then(
        (cached) =>
          cached ?? fetch(request).then((response) => store(request, response)),
      ),
  );
});
