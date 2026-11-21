import assert from "node:assert/strict";
import test from "node:test";
import {
  CHAT_LIMIT,
  chatValid,
  merge,
  readChat,
  readSent,
} from "../src/remote/chatModel.ts";

const message = (seq: number, from: "operator" | "user", text = "Szia!") => ({
  seq,
  from,
  text,
  at: 1_700_000_000 + seq,
});

test("chat messages are read strictly", () => {
  assert.deepEqual(readChat({ messages: [message(1, "user")] }), [
    message(1, "user"),
  ]);
  assert.equal(
    readChat({ messages: [{ ...message(1, "user"), from: "bot" }] }),
    null,
  );
  assert.equal(readChat({ messages: [message(0, "user")] }), null);
  assert.equal(readChat({ messages: [message(1, "user", "a\u0007")] }), null);
  assert.equal(readChat({}), null);
  assert.deepEqual(readSent(message(3, "operator")), message(3, "operator"));
});

test("only text both sides accept is sent", () => {
  assert.ok(chatValid("Két\nsor"));
  assert.ok(!chatValid("  \n "));
  assert.ok(!chatValid("x".repeat(CHAT_LIMIT + 1)));
  assert.ok(chatValid("ő".repeat(CHAT_LIMIT)));
});

test("new messages are added once and in order", () => {
  const known = [message(1, "operator")];
  const merged = merge(known, [
    message(3, "user"),
    message(1, "operator"),
    message(2, "user"),
  ]);
  assert.deepEqual(
    merged.map((m) => m.seq),
    [1, 2, 3],
  );
  assert.equal(merge(merged, [message(2, "user")]), merged);
});
