import assert from "node:assert/strict";
import test from "node:test";
import {
  CHUNK,
  dataMessage,
  readData,
  readReply,
  size,
  transferId,
  utf8Length,
} from "../src/remote/transferModel.ts";

const ID = "00112233445566778899aabbccddeeff";

test("file data is framed with its transfer and offset", () => {
  const bytes = new Uint8Array([7, 8, 9]);
  const message = dataMessage(ID, 70_000, bytes);
  assert.equal(message.length, 25 + 3);
  assert.equal(message[0], 1);
  // Data towards the device is never read back as data from it.
  assert.equal(readData(message.buffer), null);
  const back = message.slice();
  back[0] = 2;
  assert.deepEqual(readData(back.buffer), {
    id: ID,
    offset: 70_000,
    bytes,
  });
  const empty = back.slice(0, 25);
  assert.equal(readData(empty.buffer), null);
  const large = new Uint8Array(25 + CHUNK + 1);
  large[0] = 2;
  assert.equal(readData(large.buffer), null);
});

test("transfer ids are stable for the same file and differ otherwise", async () => {
  const first = await transferId("jelentés.pdf", 5, 1_700_000_000_000);
  assert.match(first, /^[0-9a-f]{32}$/);
  assert.equal(await transferId("jelentés.pdf", 5, 1_700_000_000_000), first);
  assert.notEqual(
    await transferId("jelentés.pdf", 6, 1_700_000_000_000),
    first,
  );
});

test("device answers are read strictly", () => {
  const digest = "ab".repeat(32);
  assert.deepEqual(
    readReply(
      JSON.stringify({
        type: "upload_done",
        id: ID,
        name: "jelentés (2).pdf",
        size: 5,
        sha256: digest,
      }),
    ),
    {
      type: "upload_done",
      id: ID,
      name: "jelentés (2).pdf",
      size: 5,
      sha256: digest,
    },
  );
  const listing = {
    type: "listing",
    path: "C:\\Users\\a",
    parent: "C:\\Users",
    entries: [{ name: "Dokumentumok", dir: true, size: 0, modified: 1 }],
    truncated: false,
  };
  assert.equal(readReply(JSON.stringify(listing))?.type, "listing");
  assert.equal(
    readReply(
      JSON.stringify({
        ...listing,
        entries: [{ name: "a\\b", dir: false, size: 1, modified: 1 }],
      }),
    ),
    null,
  );
  assert.deepEqual(
    readReply(
      JSON.stringify({
        type: "failed",
        id: null,
        operation: "clipboard_get",
        problem: "empty",
      }),
    ),
    { type: "failed", id: null, operation: "clipboard_get", problem: "empty" },
  );
  for (const refused of [
    { type: "failed", id: null, operation: null, problem: "boom" },
    { type: "upload_ack", id: ID.toUpperCase(), offset: 1 },
    { type: "upload_ack", id: ID, offset: -1 },
    { type: "download_done", id: ID, size: 1, sha256: "ab" },
    { type: "run" },
  ])
    assert.equal(readReply(JSON.stringify(refused)), null, refused.type);
  assert.equal(readReply("not json"), null);
});

test("sizes and text lengths are what the agent counts", () => {
  assert.equal(utf8Length("árvíz"), 7);
  assert.equal(size(512, "en-GB"), "512 B");
  assert.equal(size(1536, "en-GB"), "1.5 KB");
  assert.equal(size(5 * 1024 * 1024, "hu-HU"), "5 MB");
});
