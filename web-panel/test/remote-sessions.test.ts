import assert from "node:assert/strict";
import test from "node:test";
import type { DeviceSummary, RemoteSessions } from "../src/api.ts";
import { deviceActions } from "../src/devices/model.ts";
import {
  fresh,
  OBSERVATION_LIFETIME,
  readDecision,
  readSessions,
  selectionKey,
  unavailableReason,
  validReason,
} from "../src/remote/model.ts";

const connection = "7f23ce0e-1881-49fd-b426-f937bbfcfa03";
const generation = "d844c66a-be14-498f-b02c-62fb07f13661";
test("unattended decisions bind the whole target and expire within the observation window", () => {
  const selection = {
    connection_id: connection,
    os_session_id: "3",
    generation,
  };
  const value = { decision: "eligible", session: selection, expires_at: 120 };
  assert.deepEqual(readDecision(value, selection, 100000), {
    kind: "eligible",
    expires: 120000,
  });
  assert.deepEqual(
    readDecision({ decision: "local_consent_required" }, selection, 100000),
    { kind: "consent" },
  );
  for (const invalid of [
    null,
    {},
    { ...value, expires_at: 100 },
    { ...value, expires_at: 131 },
    { ...value, expires_at: NaN },
    { ...value, expires_at: 120.5 },
    { ...value, decision: "granted" },
  ])
    assert.equal(readDecision(invalid, selection, 100000), null);
  for (const changed of [
    { ...selection, connection_id: generation },
    { ...selection, generation: connection },
    { ...selection, os_session_id: "4" },
  ])
    assert.equal(readDecision(value, changed, 100000), null);
});
test("operation reasons use the server's UTF-8 and control character limits", () => {
  assert.equal(validReason("Planned maintenance"), true);
  assert.equal(validReason("ő".repeat(256)), true);
  for (const invalid of [
    "",
    " ",
    "ő".repeat(257),
    "a\nb",
    "a\u0085b",
    "a\u007fb",
    "a\tb",
  ])
    assert.equal(validReason(invalid), false);
});
const list = (): RemoteSessions => ({
  connection_id: connection,
  sessions: [
    {
      selection: { connection_id: connection, os_session_id: "3", generation },
      presence: "occupied",
      account: "EXAMPLE\\operator",
      station: "RDP-Tcp#2",
      console: false,
      locked: true,
    },
  ],
});

test("session lists preserve occupied locks and reject ambiguous or substituted targets", () => {
  assert.deepEqual(readSessions(list()), list());
  assert.deepEqual(
    readSessions({ connection_id: connection, sessions: [] })?.sessions,
    [],
  );
  for (const mutate of [
    (value: RemoteSessions) => {
      value.sessions[0].selection.connection_id = generation;
    },
    (value: RemoteSessions) => {
      value.sessions.push(value.sessions[0]);
    },
    (value: RemoteSessions) => {
      value.sessions[0].selection.generation =
        "00000000-0000-0000-0000-000000000000";
    },
    (value: RemoteSessions) => {
      value.sessions[0].presence = "vacant";
    },
    (value: RemoteSessions) => {
      value.sessions[0].selection.os_session_id = "0";
    },
    (value: RemoteSessions) => {
      value.sessions[0].selection.os_session_id = "4294967295";
    },
  ]) {
    const value = list();
    mutate(value);
    assert.equal(readSessions(value), null);
  }
  assert.equal(
    readSessions({
      connection_id: connection,
      sessions: [{ ...list().sessions[0], presence: "new-state" }],
    }),
    null,
  );
  assert.equal(readSessions(null), null);
  assert.equal(readSessions({ connection_id: connection }), null);
});

test("selection identifies a connection and generation, and observation age cannot be extended by a clock rollback", () => {
  const selection = list().sessions[0].selection;
  assert.notEqual(
    selectionKey(selection),
    selectionKey({ ...selection, connection_id: generation }),
  );
  assert.notEqual(
    selectionKey(selection),
    selectionKey({ ...selection, generation: connection }),
  );
  assert.equal(fresh(100, 100 + OBSERVATION_LIFETIME - 1), true);
  assert.equal(fresh(100, 100 + OBSERVATION_LIFETIME), false);
  assert.equal(fresh(100, 99), false);
  assert.equal(fresh(100, Number.NaN), false);
});

test("query availability requires both scoped permissions, active enrollment, connectivity and the reported capability", () => {
  const device = {
    permissions: ["device.read", "remote.view"],
    enrollment: "active",
    online: true,
    capabilities: ["remote.sessions"],
  } as DeviceSummary;
  assert.equal(unavailableReason(device, false), null);
  assert.equal(unavailableReason(device, true), "unverified");
  assert.equal(
    unavailableReason({ ...device, online: false }, false),
    "offline",
  );
  assert.equal(
    unavailableReason({ ...device, enrollment: "revoked" }, false),
    "revoked",
  );
  assert.equal(
    unavailableReason({ ...device, capabilities: [] }, false),
    "unsupported",
  );
  for (const permissions of [
    [],
    ["device.read"],
    ["remote.view"],
    ["remote.control"],
  ]) {
    const restricted = { ...device, permissions };
    assert.equal(unavailableReason(restricted, false), "denied");
    assert.ok(
      !deviceActions(restricted, { tenant_wide: [], fresh_passkey: [] }).some(
        (action) => action.id === "support",
      ),
    );
  }
  assert.equal(
    deviceActions(device, { tenant_wide: [], fresh_passkey: [] }).find(
      (action) => action.id === "support",
    )?.blocked,
    null,
  );
});
