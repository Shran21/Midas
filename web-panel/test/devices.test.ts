import assert from "node:assert/strict";
import test from "node:test";
import type { DeviceSummary } from "../src/api.ts";
import {
  condition,
  deviceActions,
  filtered,
  noFilters,
  palette,
  percent,
} from "../src/devices/model.ts";

const device = (changes: Partial<DeviceSummary> = {}): DeviceSummary => ({
  id: "d1",
  name: "WS-BUD-014",
  kind: "workstation",
  platform: "windows",
  enrollment: "active",
  site: { id: "s1", name: "Budapest" },
  groups: [{ id: "g1", name: "Pénzügy" }],
  online: true,
  last_seen_at: 0,
  decided_at: 0,
  os: "Windows 11 Pro",
  capabilities: [],
  health: "ok",
  health_reported_at: 0,
  inventory_reconciled_at: 0,
  restart_pending: false,
  open_alerts: 0,
  critical_alerts: 0,
  metrics: null,
  permissions: ["device.read", "monitor.read", "remote.control"],
  ...changes,
});

const access = {
  tenant_wide: [] as string[],
  fresh_passkey: ["device.enroll", "remote.admin"],
};

test("only permitted actions are offered and the server's scope decides tenant-wide ones", () => {
  const ids = deviceActions(device(), access).map((action) => action.id);
  assert.deepEqual(ids, [
    "open",
    "refresh",
    "monitoring",
    "inventory",
    "policy",
    "remote",
  ]);
  const scopedEnroll = device({
    permissions: ["device.read", "device.enroll"],
  });
  assert.ok(
    !deviceActions(scopedEnroll, access).some((a) => a.id === "revoke"),
    "revocation is checked tenant-wide, so a device-scoped grant does not offer it",
  );
  const revoke = deviceActions(device(), {
    ...access,
    tenant_wide: ["device.enroll"],
  }).find((action) => action.id === "revoke");
  assert.ok(revoke?.destructive && revoke.fresh && revoke.blocked === null);
});

test("an offline device keeps stored data and blocks what needs the agent", () => {
  const actions = deviceActions(device({ online: false }), access);
  const blocked = Object.fromEntries(
    actions.map((action) => [action.id, action.blocked]),
  );
  assert.equal(blocked.open, null);
  assert.equal(blocked.inventory, null);
  assert.equal(blocked.monitoring, null);
  assert.equal(blocked.refresh, "offline");
  assert.equal(blocked.remote, "planned");
  const revoked = deviceActions(
    device({ enrollment: "revoked", online: false }),
    { ...access, tenant_wide: ["device.enroll"] },
  );
  assert.equal(revoked.find((a) => a.id === "refresh")?.blocked, "revoked");
  assert.ok(!revoked.some((a) => a.id === "revoke"));
});

test("conditions and filters", () => {
  assert.equal(condition(device()), "online");
  assert.equal(condition(device({ open_alerts: 1 })), "attention");
  assert.equal(condition(device({ health: "warning" })), "attention");
  assert.equal(condition(device({ online: false, open_alerts: 3 })), "offline");
  assert.equal(condition(device({ enrollment: "revoked" })), "revoked");
  const list = [
    device(),
    device({
      id: "d2",
      name: "SRV-LNX-01",
      kind: "server",
      platform: "linux",
      site: null,
      groups: [],
      restart_pending: true,
      open_alerts: 1,
    }),
  ];
  const names = (changes: Partial<typeof noFilters>) =>
    filtered(list, { ...noFilters, ...changes }, "hu").map((d) => d.name);
  assert.deepEqual(names({ query: "pénz" }), ["WS-BUD-014"]);
  assert.deepEqual(names({ site: "none" }), ["SRV-LNX-01"]);
  assert.deepEqual(names({ group: "g1" }), ["WS-BUD-014"]);
  assert.deepEqual(names({ restart: true }), ["SRV-LNX-01"]);
  assert.deepEqual(names({ alerts: true, platform: "linux" }), ["SRV-LNX-01"]);
  assert.deepEqual(names({ condition: "attention" }), ["SRV-LNX-01"]);
  assert.equal(percent(1, 0), null);
  assert.equal(percent(1, 3), 33);
});

test("the palette finds only accessible devices and runnable actions", () => {
  const labels = {
    open: "Open",
    refresh: "Refresh status now",
    monitoring: "Monitoring",
    inventory: "Inventory",
    policy: "Policy",
    remote: "Remote",
    remoteAdmin: "Admin remote",
    terminal: "Terminal",
    files: "Files",
    scripts: "Scripts",
    updates: "Updates",
    revoke: "Revoke device",
  } as const;
  const online = device();
  const offline = device({ id: "d2", name: "WS-OFF-002", online: false });
  const run = (query: string, tenantWide: string[] = []) =>
    palette(
      query,
      [{ id: "devices", label: "Devices" }],
      [online, offline],
      { ...access, tenant_wide: tenantWide },
      (id) => labels[id],
      "en",
    ).map((entry) => entry.label);
  assert.deepEqual(run(""), ["Devices", "WS-BUD-014", "WS-OFF-002"]);
  assert.deepEqual(run("bud refresh"), ["WS-BUD-014: Refresh status now"]);
  assert.deepEqual(run("off refresh"), [], "a blocked action is not offered");
  assert.deepEqual(run("remote"), [], "a planned action is not offered");
  assert.deepEqual(
    run("revoke"),
    [],
    "revocation needs tenant-wide device.enroll",
  );
  assert.deepEqual(run("bud revoke", ["device.enroll"]), [
    "WS-BUD-014: Revoke device",
  ]);
});
