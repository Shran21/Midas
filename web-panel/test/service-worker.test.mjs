import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import vm from "node:vm";

const origin = "https://midas.example";
const absolute = (input) =>
  new URL(typeof input === "string" ? input : input.url, origin).href;

function response(url, type) {
  return {
    ok: true,
    status: 200,
    type: "basic",
    url,
    headers: new Headers({ "content-type": type }),
    clone() {
      return this;
    },
  };
}

async function load({ offline = false } = {}) {
  const stores = new Map();
  const handlers = {};
  const open = async (name) => {
    if (!stores.has(name)) stores.set(name, new Map());
    const entries = stores.get(name);
    return {
      addAll: async (urls) => {
        for (const url of urls)
          entries.set(absolute(url), response(absolute(url), "text/html"));
      },
      put: async (request, value) => entries.set(absolute(request), value),
    };
  };
  const caches = {
    open,
    keys: async () => [...stores.keys()],
    delete: async (name) => stores.delete(name),
    match: async (request) => {
      for (const entries of stores.values()) {
        const hit = entries.get(absolute(request));
        if (hit) return hit;
      }
      return undefined;
    },
  };
  const fetch = async (request) => {
    if (offline) throw new TypeError("offline");
    const url = new URL(absolute(request));
    if (url.pathname.startsWith("/api/") || url.pathname.startsWith("/health"))
      return response(url.href, "application/json");
    if (url.pathname.startsWith("/assets/"))
      return response(url.href, "text/javascript");
    return response(url.href, "text/html");
  };
  class Request {
    constructor(input) {
      this.url = absolute(input);
      this.method = "GET";
      this.mode = "cors";
    }
  }
  const self = {
    location: new URL(`${origin}/sw.js`),
    addEventListener: (type, handler) => {
      handlers[type] = handler;
    },
    skipWaiting: () => {},
    clients: { claim: async () => {} },
  };
  const source = await readFile(
    new URL("../public/sw.js", import.meta.url),
    "utf8",
  );
  vm.runInNewContext(source, {
    self,
    caches,
    fetch,
    Request,
    Response,
    URL,
    Promise,
  });
  const dispatch = async (type, request) => {
    let responded;
    const pending = [];
    handlers[type]({
      request,
      respondWith: (value) => {
        responded = value;
      },
      waitUntil: (value) => pending.push(value),
    });
    await Promise.all(pending);
    return responded === undefined ? undefined : await responded;
  };
  await dispatch("install");
  return { stores, dispatch, caches };
}

const get = (path, mode = "cors", method = "GET") => ({
  url: new URL(path, origin).href,
  mode,
  method,
});

test("caches only static interface assets", async () => {
  const { stores, dispatch } = await load();
  for (const request of [
    get("/api/session"),
    get("/api/tenants"),
    get("/health/live"),
    get("/api/session", "navigate"),
    get("/assets/app.js", "cors", "POST"),
    { url: "https://cdn.example/assets/app.js", mode: "cors", method: "GET" },
  ]) {
    assert.equal(await dispatch("fetch", request), undefined, request.url);
  }
  assert.ok(await dispatch("fetch", get("/assets/index-abc.js")));
  assert.ok(await dispatch("fetch", get("/devices", "navigate")));
  const cached = [...stores.values()].flatMap((entries) => [...entries.keys()]);
  assert.ok(cached.includes(`${origin}/assets/index-abc.js`));
  for (const url of cached) {
    const path = new URL(url).pathname;
    assert.ok(
      path.startsWith("/assets/") ||
        ["/", "/manifest.webmanifest", "/icon.svg"].includes(path),
      url,
    );
  }
});

test("serves the cached shell offline and removes old caches", async () => {
  const online = await load();
  const offline = await load({ offline: true });
  offline.stores.set(
    "midas-static-v0",
    new Map([[`${origin}/old`, response("old", "text/html")]]),
  );
  await offline.dispatch("activate");
  assert.deepEqual([...offline.stores.keys()], ["midas-static-v1"]);
  const shell = await offline.dispatch("fetch", get("/devices", "navigate"));
  assert.equal(shell.headers.get("content-type"), "text/html");
  assert.ok(online.stores.get("midas-static-v1").has(`${origin}/`));
});
