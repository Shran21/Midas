import assert from "node:assert/strict";
import test from "node:test";
import {
  clampView,
  Gestures,
  HOLD,
  IDENTITY,
  MAX_SCALE,
  type Output,
  TWO_TAP,
  zoomBy,
} from "../src/remote/gestures.ts";

const size = { width: 800, height: 450 };
const presses = (out: Output[]) =>
  out.flatMap((o) => ("press" in o ? [o.press] : []));

test("a tap clicks and a held finger gives a secondary click", () => {
  const g = new Gestures(() => size);
  assert.deepEqual(g.down(1, { x: 100, y: 50 }, 0), []);
  assert.deepEqual(g.move(1, { x: 104, y: 52 }), [], "within the slop");
  assert.deepEqual(presses(g.up(1, { x: 104, y: 52 }, 120)), [
    { x: 100, y: 50, buttons: 0 },
    { x: 100, y: 50, buttons: 1 },
    { x: 100, y: 50, buttons: 0 },
  ]);
  g.down(2, { x: 300, y: 200 }, 1000);
  assert.deepEqual(g.tick(1000 + HOLD - 1), []);
  assert.deepEqual(
    presses(g.tick(1000 + HOLD)).map((p) => p.buttons),
    [0, 2, 0],
  );
  // Lifting the finger afterwards adds nothing.
  assert.deepEqual(g.up(2, { x: 300, y: 200 }, 1000 + HOLD + 50), []);
});

test("one moving finger drags with the primary button", () => {
  const g = new Gestures(() => size);
  g.down(1, { x: 10, y: 10 }, 0);
  assert.deepEqual(presses(g.move(1, { x: 40, y: 10 })), [
    { x: 10, y: 10, buttons: 0 },
    { x: 10, y: 10, buttons: 1 },
    { x: 40, y: 10, buttons: 1 },
  ]);
  assert.deepEqual(presses(g.move(1, { x: 60, y: 20 })), [
    { x: 60, y: 20, buttons: 1 },
  ]);
  // A dragging finger is never taken for a hold.
  assert.deepEqual(g.tick(HOLD * 2), []);
  assert.deepEqual(presses(g.up(1, { x: 70, y: 20 }, 900)), [
    { x: 70, y: 20, buttons: 0 },
  ]);
});

test("a cancelled drag lets the button go", () => {
  const g = new Gestures(() => size);
  g.down(1, { x: 10, y: 10 }, 0);
  g.move(1, { x: 50, y: 10 });
  assert.deepEqual(presses(g.cancel()), [{ x: 50, y: 10, buttons: 0 }]);
  assert.deepEqual(g.cancel(), []);
});

test("a quick two-finger tap is a secondary click between the fingers", () => {
  const g = new Gestures(() => size);
  g.down(1, { x: 100, y: 100 }, 0);
  g.down(2, { x: 200, y: 100 }, 20);
  assert.deepEqual(presses(g.up(1, { x: 100, y: 100 }, 20 + TWO_TAP - 10)), [
    { x: 150, y: 100, buttons: 0 },
    { x: 150, y: 100, buttons: 2 },
    { x: 150, y: 100, buttons: 0 },
  ]);
  assert.deepEqual(g.up(2, { x: 200, y: 100 }, 400), []);
  // Too slow: nothing.
  g.down(1, { x: 100, y: 100 }, 1000);
  g.down(2, { x: 200, y: 100 }, 1000);
  assert.deepEqual(g.up(1, { x: 100, y: 100 }, 1000 + TWO_TAP + 1), []);
});

test("pinching zooms around the fingers and panning stays inside the picture", () => {
  const g = new Gestures(() => size);
  // Fingers around the centre spread to twice their distance.
  g.down(1, { x: 350, y: 225 }, 0);
  g.down(2, { x: 450, y: 225 }, 0);
  g.move(1, { x: 300, y: 225 });
  const out = g.move(2, { x: 500, y: 225 });
  const last = out.at(-1);
  assert.ok(last && "view" in last);
  assert.ok(Math.abs(g.view.scale - 2) < 1e-9);
  assert.ok(Math.abs(g.view.x) < 1e-9 && Math.abs(g.view.y) < 1e-9);
  // No click comes out of a zoom, however fast.
  assert.deepEqual(presses(g.up(1, { x: 300, y: 225 }, 100)), []);
  g.up(2, { x: 500, y: 225 }, 100);
  assert.deepEqual(clampView({ scale: 2, x: 1000, y: -1000 }, size), {
    scale: 2,
    x: 400,
    y: -225,
  });
  assert.equal(clampView({ scale: 10, x: 0, y: 0 }, size).scale, MAX_SCALE);
  assert.deepEqual(clampView({ scale: 0.5, x: 30, y: 0 }, size), IDENTITY);
});

test("zoom buttons scale around the centre", () => {
  const zoomed = zoomBy(IDENTITY, 2, size);
  assert.deepEqual(zoomed, { scale: 2, x: 0, y: 0 });
  assert.deepEqual(zoomBy({ scale: 2, x: 100, y: 0 }, 0.5, size), IDENTITY);
  assert.equal(zoomBy(zoomed, 10, size).scale, MAX_SCALE);
});
