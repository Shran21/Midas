import type { DeviceSummary, TenantContext } from "../api";

export type ActionId =
  | "open"
  | "refresh"
  | "monitoring"
  | "inventory"
  | "policy"
  | "remote"
  | "remoteAdmin"
  | "terminal"
  | "files"
  | "scripts"
  | "updates"
  | "revoke";

type Definition = {
  id: ActionId;
  permission: string;
  /** Tenant-wide operations are checked without a device target by the server. */
  scope: "device" | "tenant";
  needsAgent: boolean;
  /** Not built yet; listed so operators see what is coming and what it will need. */
  planned?: boolean;
  destructive?: boolean;
};

const catalogue: Definition[] = [
  { id: "open", permission: "device.read", scope: "device", needsAgent: false },
  {
    id: "refresh",
    permission: "monitor.read",
    scope: "device",
    needsAgent: true,
  },
  {
    id: "monitoring",
    permission: "monitor.read",
    scope: "device",
    needsAgent: false,
  },
  {
    id: "inventory",
    permission: "device.read",
    scope: "device",
    needsAgent: false,
  },
  {
    id: "policy",
    permission: "device.read",
    scope: "device",
    needsAgent: false,
  },
  {
    id: "remote",
    permission: "remote.control",
    scope: "device",
    needsAgent: true,
    planned: true,
  },
  {
    id: "remoteAdmin",
    permission: "remote.admin",
    scope: "device",
    needsAgent: true,
    planned: true,
  },
  {
    id: "terminal",
    permission: "terminal.execute",
    scope: "device",
    needsAgent: true,
    planned: true,
  },
  {
    id: "files",
    permission: "files.read",
    scope: "device",
    needsAgent: true,
    planned: true,
  },
  {
    id: "scripts",
    permission: "script.execute",
    scope: "device",
    needsAgent: false,
    planned: true,
  },
  {
    id: "updates",
    permission: "updates.manage",
    scope: "device",
    needsAgent: false,
    planned: true,
  },
  {
    id: "revoke",
    permission: "device.enroll",
    scope: "tenant",
    needsAgent: false,
    destructive: true,
  },
];

export type Blocked = "offline" | "revoked" | "planned";

export type Action = {
  id: ActionId;
  permission: string;
  blocked: Blocked | null;
  /** The server will ask for a passkey sign-in from the last five minutes. */
  fresh: boolean;
  destructive: boolean;
};

export type Access = Pick<TenantContext, "tenant_wide" | "fresh_passkey">;

/**
 * The actions an operator is offered for a device, in menu order. The context menu and the
 * device page both use this list; the server still authorizes every request on its own.
 */
export function deviceActions(device: DeviceSummary, access: Access): Action[] {
  return catalogue
    .filter((definition) =>
      definition.scope === "tenant"
        ? access.tenant_wide.includes(definition.permission)
        : device.permissions.includes(definition.permission),
    )
    .filter(
      (definition) =>
        !(definition.id === "revoke" && device.enrollment === "revoked"),
    )
    .map((definition) => ({
      id: definition.id,
      permission: definition.permission,
      blocked: definition.planned
        ? "planned"
        : definition.needsAgent && device.enrollment === "revoked"
          ? "revoked"
          : definition.needsAgent && !device.online
            ? "offline"
            : null,
      fresh: access.fresh_passkey.includes(definition.permission),
      destructive: definition.destructive ?? false,
    }));
}

export type Condition = "online" | "attention" | "offline" | "revoked";

export function condition(device: DeviceSummary): Condition {
  if (device.enrollment === "revoked") return "revoked";
  if (!device.online) return "offline";
  if (
    device.open_alerts > 0 ||
    device.health === "warning" ||
    device.health === "critical"
  )
    return "attention";
  return "online";
}

export function percent(used: number, total: number): number | null {
  if (total <= 0) return null;
  return Math.min(100, Math.round((used / total) * 100));
}

export type Filters = {
  query: string;
  kind: "all" | "workstation" | "server";
  condition: "all" | Condition;
  platform: "all" | "windows" | "linux";
  site: string;
  group: string;
  alerts: boolean;
  restart: boolean;
};

export const noFilters: Filters = {
  query: "",
  kind: "all",
  condition: "all",
  platform: "all",
  site: "all",
  group: "all",
  alerts: false,
  restart: false,
};

export function filtered(
  devices: DeviceSummary[],
  filters: Filters,
  locale: string,
): DeviceSummary[] {
  const query = filters.query.trim().toLocaleLowerCase(locale);
  return devices.filter(
    (device) =>
      (filters.kind === "all" || device.kind === filters.kind) &&
      (filters.condition === "all" ||
        condition(device) === filters.condition) &&
      (filters.platform === "all" || device.platform === filters.platform) &&
      (filters.site === "all" ||
        (filters.site === "none"
          ? device.site === null
          : device.site?.id === filters.site)) &&
      (filters.group === "all" ||
        device.groups.some((group) => group.id === filters.group)) &&
      (!filters.alerts || device.open_alerts > 0) &&
      (!filters.restart || device.restart_pending) &&
      (query === "" ||
        [
          device.name,
          device.os ?? "",
          device.site?.name ?? "",
          ...device.groups.map((group) => group.name),
        ].some((value) => value.toLocaleLowerCase(locale).includes(query))),
  );
}

/** Sites and groups that occur in the list, for the filter menus. */
export function scopes(devices: DeviceSummary[]) {
  const sites = new Map<string, string>();
  const groups = new Map<string, string>();
  for (const device of devices) {
    if (device.site) sites.set(device.site.id, device.site.name);
    for (const group of device.groups) groups.set(group.id, group.name);
  }
  const sorted = (entries: Map<string, string>) =>
    [...entries]
      .map(([id, name]) => ({ id, name }))
      .sort((a, b) => a.name.localeCompare(b.name));
  return { sites: sorted(sites), groups: sorted(groups) };
}
