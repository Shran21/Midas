import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import {
  mkdtempSync,
  readFileSync,
  rmdirSync,
  unlinkSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const fixed = "https://github.com/advisories/GHSA-fc5q-39xc-vjpf";

export function validateAdvisories(report) {
  assert.ok(
    report && typeof report === "object" && !Array.isArray(report),
    "Invalid advisory response",
  );
  assert.ok(
    Object.keys(report).every((name) => name === "picomatch"),
    "Unexpected advisory package",
  );
  const entries = report.picomatch === undefined ? [] : report.picomatch;
  assert.ok(Array.isArray(entries), "Invalid advisory list");
  for (const item of entries) {
    assert.ok(
      item && typeof item === "object" && !Array.isArray(item),
      "Invalid advisory",
    );
    if (item.name !== undefined)
      assert.equal(item.name, "picomatch", "Invalid advisory package");
    assert.equal(
      item.url,
      fixed,
      `Unreviewed upstream advisory: ${JSON.stringify(item.url)}`,
    );
    assert.ok(
      typeof item.vulnerable_versions === "string" &&
        typeof item.severity === "string",
      "Incomplete advisory",
    );
  }
  return entries.length;
}

async function main() {
  const root = fileURLToPath(new URL("../../", import.meta.url));
  execFileSync(
    process.execPath,
    [
      "--test",
      "web-panel/test/dependencies.test.mjs",
      ".github/scripts/check-vendored-dependencies.test.mjs",
    ],
    { cwd: root, stdio: "inherit" },
  );
  const npm = JSON.parse(
    readFileSync(join(root, "web-panel/vendor/picomatch/upstream.json")),
  );
  assert.equal(npm.package, "picomatch");
  assert.equal(npm.version, "4.0.7");
  const response = await fetch(
    "https://registry.npmjs.org/-/npm/v1/security/advisories/bulk",
    {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ [npm.package]: [npm.version] }),
      signal: AbortSignal.timeout(15_000),
    },
  );
  assert.ok(response.ok, `Upstream npm audit failed: HTTP ${response.status}`);
  const chunks = [];
  let size = 0;
  for await (const chunk of response.body) {
    size += chunk.length;
    assert.ok(size <= 1_048_576, "Oversized advisory response");
    chunks.push(chunk);
  }
  const report = JSON.parse(Buffer.concat(chunks).toString("utf8"));
  const count = validateAdvisories(report);
  console.log(
    `Upstream picomatch audit: ${count} advisory covered by the verified local fix; no unreviewed advisories`,
  );

  const cargo = JSON.parse(
    readFileSync(join(root, "server/vendor/rustls-acme/upstream.json")),
  );
  assert.equal(cargo.package, "rustls-acme");
  assert.equal(cargo.version, "0.15.4");
  assert.match(cargo.checksum, /^[a-f0-9]{64}$/);
  const temporary = mkdtempSync(join(tmpdir(), "midas-vendor-audit-"));
  const lock = join(temporary, "Cargo.lock");
  try {
    writeFileSync(
      lock,
      `version = 4\n\n[[package]]\nname = "${cargo.package}"\nversion = "${cargo.version}"\nsource = "registry+https://github.com/rust-lang/crates.io-index"\nchecksum = "${cargo.checksum}"\n`,
    );
    execFileSync(
      "cargo",
      ["audit", "--file", lock, "--no-fetch", "--deny", "warnings"],
      { cwd: root, stdio: "inherit" },
    );
  } finally {
    unlinkSync(lock);
    rmdirSync(temporary);
  }
}

if (
  process.argv[1] &&
  resolve(process.argv[1]) === fileURLToPath(import.meta.url)
) {
  await main();
}
