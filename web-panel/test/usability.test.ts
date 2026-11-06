import assert from "node:assert/strict";
import test from "node:test";
import {
  newRequest,
  reduce,
  type State,
  secondsLeft,
} from "../src/usability/model.ts";

const idle: State = { kind: "idle" };

test("declined request never connects and a new request can be allowed and ended", () => {
  const first = newRequest("normal", "console", 0, "a");
  let state = reduce(idle, { type: "request", request: first });
  assert.equal(state.kind, "waiting");
  state = reduce(state, { type: "decline", id: "a", at: 5_000 });
  assert.equal(state.kind, "declined");
  state = reduce(state, { type: "allow", id: "a", at: 6_000 });
  assert.equal(
    state.kind,
    "declined",
    "a late allow cannot reactivate a declined request",
  );
  const second = newRequest("admin", "console", 10_000, "b");
  state = reduce(state, { type: "request", request: second });
  state = reduce(state, { type: "allow", id: "a", at: 11_000 });
  assert.equal(
    state.kind,
    "waiting",
    "an answer for another request is ignored",
  );
  state = reduce(state, { type: "allow", id: "b", at: 12_000 });
  assert.equal(state.kind, "connected");
  state = reduce(state, { type: "end", id: "b", by: "user", at: 20_000 });
  assert.deepEqual(state.kind === "ended" ? [state.by, state.at] : null, [
    "user",
    20_000,
  ]);
});

test("expiry closes the request and blocks a late allow", () => {
  const request = newRequest("normal", "rdp", 0, "c");
  let state = reduce(idle, { type: "request", request });
  assert.equal(secondsLeft(state, 15_500), 45);
  assert.equal(
    reduce(state, { type: "expire", id: "c", at: 30_000 }).kind,
    "waiting",
  );
  assert.equal(
    reduce(state, { type: "allow", id: "c", at: 60_000 }).kind,
    "expired",
  );
  state = reduce(state, { type: "expire", id: "c", at: 60_000 });
  assert.equal(state.kind, "expired");
  assert.equal(
    reduce(state, { type: "allow", id: "c", at: 60_001 }).kind,
    "expired",
  );
});

test("only one request is pending and the operator can end an active session", () => {
  const request = newRequest("normal", "console", 0, "d");
  let state = reduce(idle, { type: "request", request });
  state = reduce(state, {
    type: "request",
    request: newRequest("admin", "console", 1_000, "e"),
  });
  assert.equal(state.kind === "waiting" && state.request.id, "d");
  state = reduce(state, { type: "allow", id: "d", at: 2_000 });
  assert.equal(
    reduce(state, {
      type: "request",
      request: newRequest("normal", "console", 3_000, "f"),
    }).kind,
    "connected",
  );
  state = reduce(state, { type: "end", id: "d", by: "operator", at: 4_000 });
  assert.equal(state.kind === "ended" && state.by, "operator");
});
