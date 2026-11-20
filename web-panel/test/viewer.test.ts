import assert from "node:assert/strict";
import test from "node:test";
import {
  bitrate,
  keyCode,
  readStats,
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
  });
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
