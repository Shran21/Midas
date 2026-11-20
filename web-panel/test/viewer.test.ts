import assert from "node:assert/strict";
import test from "node:test";
import {
  bitrate,
  combination,
  keyCode,
  modifiers,
  readIceServers,
  readSasResult,
  readStats,
  SYSTEM_KEYS,
  textMessages,
  videoPoint,
} from "../src/remote/viewerModel.ts";

test("pointer positions map onto the letterboxed picture only", () => {
  const box = { left: 100, top: 50, width: 800, height: 600 };
  const video = { width: 1920, height: 1080 };
  // 800 x 450 picture centred vertically: top 125, bottom 575.
  assert.deepEqual(videoPoint(100, 125, box, video), { x: 0, y: 0 });
  assert.deepEqual(videoPoint(900, 575, box, video), { x: 1, y: 1 });
  const middle = videoPoint(500, 350, box, video);
  assert.ok(
    middle &&
      Math.abs(middle.x - 0.5) < 1e-9 &&
      Math.abs(middle.y - 0.5) < 1e-9,
  );
  assert.equal(videoPoint(500, 100, box, video), null, "letterbox band");
  assert.equal(videoPoint(50, 350, box, video), null);
  assert.equal(videoPoint(500, 350, box, { width: 0, height: 1080 }), null);
  assert.equal(videoPoint(Number.NaN, 350, box, video), null);
});

test("only plain physical key names are sent", () => {
  assert.equal(keyCode("KeyA"), "KeyA");
  assert.equal(keyCode("IntlBackslash"), "IntlBackslash");
  assert.equal(keyCode(""), null);
  assert.equal(keyCode("Key A"), null);
  assert.equal(keyCode("x".repeat(33)), null);
});

test("agent statistics are read strictly", () => {
  const report = {
    type: "stats",
    auto: true,
    profile: "text",
    width: 1280,
    height: 720,
    target_bitrate: 1_500_000,
    target_fps: 30,
    sent_bitrate: 1_234_567,
    sent_fps: 29.5,
  };
  assert.deepEqual(readStats(JSON.stringify(report)), {
    width: 1280,
    height: 720,
    targetBitrate: 1_500_000,
    targetFps: 30,
    sentBitrate: 1_234_567,
    sentFps: 29.5,
    profile: "text",
    auto: true,
    desktop: "user",
    rttMs: null,
    loss: null,
    degraded: false,
    path: null,
  });
  const measured = readStats(
    JSON.stringify({
      ...report,
      rtt_ms: 42,
      loss: 0.031,
      network: "degraded",
      path: "relay",
    }),
  );
  assert.equal(measured?.rttMs, 42);
  assert.equal(measured?.loss, 0.031);
  assert.equal(measured?.degraded, true);
  assert.equal(measured?.path, "relay");
  for (const field of [
    { rtt_ms: -1 },
    { loss: 1.5 },
    { network: "fine" },
    { path: "tunnel" },
  ])
    assert.equal(readStats(JSON.stringify({ ...report, ...field })), null);
  assert.equal(
    readStats(JSON.stringify({ ...report, desktop: "secure" }))?.desktop,
    "secure",
  );
  assert.equal(
    readStats(JSON.stringify({ ...report, desktop: "elevated" }))?.desktop,
    "elevated",
  );
  assert.equal(
    readStats(JSON.stringify({ ...report, desktop: "admin" })),
    null,
  );
  assert.equal(readStats(JSON.stringify({ ...report, desktop: 2 })), null);
  for (const invalid of [
    "not json",
    "null",
    JSON.stringify({ ...report, type: "other" }),
    JSON.stringify({ ...report, width: -1 }),
    JSON.stringify({ ...report, sent_fps: "fast" }),
    JSON.stringify({ ...report, profile: "video" }),
    JSON.stringify({ ...report, auto: "yes" }),
  ])
    assert.equal(readStats(invalid), null, invalid);
  assert.equal(bitrate(1_234_567, "en"), "1.2 Mbit/s");
  assert.equal(bitrate(850_000, "en"), "850 kbit/s");
});

test("relays from the server are taken only when well formed", () => {
  const relay = {
    urls: [
      "turn:relay.example.com:3478?transport=udp",
      "turns:relay.example.com:443?transport=tcp",
    ],
    username: "1700000300:request",
    credential: "c2VjcmV0",
  };
  assert.deepEqual(readIceServers({ ice_servers: [relay] }), [relay]);
  assert.deepEqual(readIceServers({ ice_servers: [] }), []);
  for (const invalid of [
    null,
    { ice_servers: "turn:relay" },
    { ice_servers: [{ ...relay, urls: ["stun:relay:3478"] }] },
    { ice_servers: [{ ...relay, urls: ["turn:relay 3478"] }] },
    { ice_servers: [{ ...relay, username: "" }] },
    { ice_servers: [{ ...relay, credential: 7 }] },
    { ice_servers: [relay, relay, relay, relay, relay] },
  ])
    assert.deepEqual(readIceServers(invalid), [], JSON.stringify(invalid));
});

test("system keys go down in order and up in reverse", () => {
  assert.deepEqual(combination(SYSTEM_KEYS.taskManager), [
    { type: "key", code: "ControlLeft", down: true },
    { type: "key", code: "ShiftLeft", down: true },
    { type: "key", code: "Escape", down: true },
    { type: "key", code: "Escape", down: false },
    { type: "key", code: "ShiftLeft", down: false },
    { type: "key", code: "ControlLeft", down: false },
  ]);
  for (const keys of Object.values(SYSTEM_KEYS))
    for (const code of keys) assert.match(code, /^[A-Za-z0-9]{1,32}$/);
});

test("pointer modifiers, text pieces and the Ctrl+Alt+Del answer", () => {
  assert.equal(
    modifiers({ ctrlKey: true, shiftKey: false, altKey: true, metaKey: false }),
    5,
  );
  assert.equal(
    modifiers({
      ctrlKey: false,
      shiftKey: false,
      altKey: false,
      metaKey: false,
    }),
    0,
  );
  const long = "á".repeat(70);
  const pieces = textMessages(`${long}\r\nő\u0007\tű`);
  assert.equal(pieces.length, 2);
  assert.equal([...pieces[0].text].length, 64);
  assert.equal(pieces[1].text, `${"á".repeat(6)}\nő\tű`);
  assert.deepEqual(textMessages(""), []);
  // A character outside the basic plane stays whole across a piece boundary.
  const emoji = textMessages(`${"x".repeat(63)}😀y`);
  assert.equal(emoji[0].text.endsWith("😀"), true);
  assert.equal(readSasResult('{"type":"sas","result":"sent"}'), "sent");
  assert.equal(readSasResult('{"type":"sas","result":"maybe"}'), null);
  assert.equal(readSasResult('{"type":"stats"}'), null);
  assert.equal(readSasResult("not json"), null);
});
