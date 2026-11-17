import assert from "node:assert/strict";
import test from "node:test";
import { validateAdvisories } from "./check-vendored-dependencies.mjs";

const repaired = {
  name: "picomatch",
  url: "https://github.com/advisories/GHSA-fc5q-39xc-vjpf",
  vulnerable_versions: "<=4.0.7",
  severity: "high",
};

test("the known patched advisory is identified without suppressing a new one", () => {
  assert.equal(validateAdvisories({ picomatch: [repaired] }), 1);
  assert.equal(
    validateAdvisories({ picomatch: [{ ...repaired, name: undefined }] }),
    1,
  );
  assert.equal(validateAdvisories({}), 0);
  assert.throws(() =>
    validateAdvisories({
      picomatch: [
        repaired,
        { ...repaired, url: "https://github.com/advisories/GHSA-new-finding" },
      ],
    }),
  );
});

test("invalid advisory responses fail closed", () => {
  for (const value of [
    null,
    [],
    "error",
    { picomatch: null },
    { picomatch: {} },
    { picomatch: [null] },
    { picomatch: [{ url: repaired.url }] },
    { different: [] },
  ]) {
    assert.throws(() => validateAdvisories(value));
  }
});
