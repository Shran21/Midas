import assert from "node:assert/strict";
import test from "node:test";
import type { DeviceUnattendedPolicy, Session } from "../src/api.ts";
import {
  draftFor,
  freshPasskey,
  localTime,
  parseLocalTime,
  policyKey,
  windowFor,
} from "../src/unattended/model.ts";

test("local maintenance times reject normalization, clock-change gaps and repeated times", () => {
  const previous = process.env.TZ;
  process.env.TZ = "Europe/Budapest";
  try {
    const seconds = Date.parse("2026-11-10T12:34:56Z") / 1000;
    assert.equal(localTime(seconds), "2026-11-10T13:34:56");
    assert.equal(parseLocalTime(localTime(seconds)), seconds);
    assert.equal(parseLocalTime("2026-11-10T13:34"), seconds - 56);
    for (const value of [
      "",
      "2026-02-30T12:00",
      "2026-11-10T25:00",
      "2026-11-10T12:00Z",
      "2026-03-29T02:30",
      "2026-10-25T02:30",
      "1960-01-01T00:00",
    ]) {
      assert.equal(parseLocalTime(value), null, value);
    }
  } finally {
    if (previous === undefined) delete process.env.TZ;
    else process.env.TZ = previous;
  }
});

test("enabling needs a future ordered window; disabling can clear an expired or invalid window", () => {
  const now = Date.parse("2026-11-10T12:00:00Z") / 1000;
  const draft = {
    enabled: true,
    start: localTime(now - 60),
    end: localTime(now + 60),
    base: "",
  };
  assert.deepEqual(windowFor(draft, now), {
    starts_at: now - 60,
    ends_at: now + 60,
  });
  assert.equal(windowFor({ ...draft, end: draft.start }, now), "invalidOrder");
  assert.equal(windowFor({ ...draft, end: localTime(now) }, now), "expired");
  assert.equal(windowFor({ ...draft, start: "" }, now), "invalidTime");
  assert.equal(
    windowFor({ ...draft, enabled: false, start: "", end: "" }, now),
    null,
  );
});

test("draft preserves exact seconds and detects changes to the edited scope only", () => {
  const policy: DeviceUnattendedPolicy = {
    enabled: true,
    tenant_enabled: false,
    maintenance_window: { starts_at: 1794312007, ends_at: 1794315609 },
    within_maintenance_window: false,
    reboot_confirmation_required: true,
    reconnect_verification_required: true,
  };
  const draft = draftFor(policy);
  assert.deepEqual(windowFor(draft, 0), policy.maintenance_window);
  assert.equal(
    draft.base,
    policyKey({
      ...policy,
      tenant_enabled: true,
      within_maintenance_window: true,
    }),
  );
  assert.notEqual(draft.base, policyKey({ ...policy, enabled: false }));
  assert.notEqual(
    draft.base,
    policyKey({ ...policy, maintenance_window: null }),
  );
  assert.deepEqual(draftFor({ enabled: false }), {
    enabled: false,
    start: "",
    end: "",
    base: policyKey({ enabled: false }),
  });
});

test("only an unexpired passkey within five minutes can skip the confirmation prompt", () => {
  const session: Session = {
    id: "s",
    operator_id: "o",
    authentication_method: "passkey",
    authenticated_at: 1000,
    created_at: 1000,
    last_seen_at: 1000,
    expires_at: 2000,
  };
  assert.equal(freshPasskey(session, 1299), true);
  assert.equal(freshPasskey(session, 1300), false);
  assert.equal(freshPasskey(session, 999), false);
  assert.equal(freshPasskey({ ...session, expires_at: 1200 }, 1200), false);
  for (const authentication_method of [
    "password_totp",
    "recovery_code",
    "directory",
  ] as const) {
    assert.equal(
      freshPasskey({ ...session, authentication_method }, 1000),
      false,
    );
  }
});
