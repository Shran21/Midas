import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join } from "node:path";
import test from "node:test";

const require = createRequire(new URL("../package.json", import.meta.url));
const vite = createRequire(require.resolve("vite"));
const glob = createRequire(vite.resolve("tinyglobby"));
const finder = createRequire(glob.resolve("fdir"));
const packages = [vite, glob, finder];

test("all build-tool paths use the reviewed local matcher", () => {
  const upstream = JSON.parse(
    readFileSync(new URL("../vendor/picomatch/upstream.json", import.meta.url)),
  );
  for (const dependency of packages) {
    const installed = dirname(dependency.resolve("picomatch/package.json"));
    assert.equal(dependency("picomatch/package.json").version, "4.0.7");
    for (const [name, hash] of Object.entries(upstream.files)) {
      const local = readFileSync(
        new URL(`../vendor/picomatch/${name}`, import.meta.url),
      );
      assert.deepEqual(readFileSync(join(installed, name)), local);
      if (name !== "package.json" && name !== "lib/parse.js") {
        assert.equal(createHash("sha256").update(local).digest("hex"), hash);
      }
    }
  }
});

test("repeated extglobs are refused before compiling a regular expression", () => {
  for (const dependency of packages) {
    const matcher = dependency("picomatch");
    for (const pattern of ["+(*)", "+(*a)", "*(*a)", "x/+(*a)/y", "+(a|aa)"]) {
      for (const options of [
        {},
        { maxExtglobRecursion: false },
        { fastpaths: false, capture: true },
      ]) {
        assert.throws(() => matcher.makeRe(pattern, options), SyntaxError);
        assert.throws(() => matcher(pattern, options), SyntaxError);
      }
    }
  }
});

test("ordinary build patterns and bounded extglobs keep working", () => {
  const matcher = vite("picomatch");
  for (const [pattern, yes, no] of [
    ["src/**/*.{ts,tsx}", "src/pages/Devices.tsx", "src/assets/logo.svg"],
    [
      "**/node_modules/**",
      "web/node_modules/react/index.js",
      "web/src/main.tsx",
    ],
    ["src/@(main|index).ts", "src/main.ts", "src/other.ts"],
    ["*.json", "package.json", "package.json.bak"],
  ]) {
    assert.equal(matcher(pattern)(yes), true);
    assert.equal(matcher(pattern)(no), false);
  }
});
