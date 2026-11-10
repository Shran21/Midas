import type { DeviceSummary, TenantContext } from "../api";

export type ActionId =
  | "open"
  | "refresh"
  | "monitoring"
  | "inventory"
  | "policy"
  | "support"
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
  /** Needs a graphical desktop on the host; Server Core and other headless hosts lack one. */
  graphical?: boolean;
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
    id: "support",
    permission: "remote.view",
    scope: "device",
    needsAgent: false,
  },
  {
    id: "remote",
    permission: "remote.control",
    scope: "device",
    needsAgent: true,
    planned: true,
    graphical: true,
  },
  {
    id: "remoteAdmin",
    permission: "remote.admin",
    scope: "device",
    needsAgent: true,
    planned: true,
    graphical: true,
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

export type Blocked = "offline" | "revoked" | "planned" | "headless";

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
        !(definition.id === "revoke" && device.enrollment === "revoked") &&
        (definition.id !== "support" ||
          device.permissions.includes("device.read")),
    )
    .map((definition) => ({
      id: definition.id,
      permission: definition.permission,
      blocked:
        definition.graphical && desktop(device) === false
          ? "headless"
          : definition.planned
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

/**
 * Whether the host has a graphical desktop, as its agent reports it; `null` for agents that do
 * not report it yet.
 */
export function desktop(device: DeviceSummary): boolean | null {
  if (device.capabilities.includes("host.desktop")) return true;
  if (device.capabilities.includes("host.headless")) return false;
  return null;
}

/** The quick actions of the device page: graphical support, or terminal and files without it. */
export function commonActionIds(device: DeviceSummary): ActionId[] {
  return desktop(device) === false
    ? ["refresh", "terminal", "files", "revoke"]
    : ["refresh", "support", "remote", "remoteAdmin", "revoke"];
}

export type CapabilityState =
  | "available"
  | "planned"
  | "headless"
  | "linuxLater"
  | "unknown"
  | "notReported";

export type Capability = {
  id:
    | "remote"
    | "remoteAdmin"
    | "signIn"
    | "metrics"
    | "inventory"
    | "health"
    | "terminal"
    | "files"
    | "scripts"
    | "updates";
  state: CapabilityState;
};

/**
 * What the host can offer, split into graphical and background capabilities. It describes the
 * host; whether the operator may use a capability is still decided by `deviceActions`.
 */
export function hostCapabilities(device: DeviceSummary): {
  graphical: Capability[];
  background: Capability[];
} {
  const hasDesktop = desktop(device);
  const graphicalState: CapabilityState =
    hasDesktop === false
      ? "headless"
      : device.platform === "linux"
        ? "linuxLater"
        : hasDesktop === null
          ? "unknown"
          : "planned";
  const reported = (present: boolean): CapabilityState =>
    present ? "available" : "notReported";
  return {
    graphical: (["remote", "remoteAdmin", "signIn"] as const).map((id) => ({
      id,
      state: graphicalState,
    })),
    background: [
      {
        id: "metrics",
        state: reported(device.capabilities.includes("metrics")),
      },
      {
        id: "inventory",
        state: reported(device.capabilities.includes("inventory")),
      },
      {
        id: "health",
        state: reported(
          device.capabilities.some((name) => name.startsWith("health.")),
        ),
      },
      ...(["terminal", "files", "scripts", "updates"] as const).map((id) => ({
        id,
        state: "planned" as const,
      })),
    ],
  };
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

export type PaletteEntry =
  | { kind: "section"; id: string; label: string }
  | { kind: "device"; device: DeviceSummary; label: string }
  | { kind: "action"; device: DeviceSummary; action: Action; label: string };

/**
 * Palette results: every word of the query must occur in the entry's label. Devices come from the
 * server's permission-filtered list and actions from `deviceActions`, so only accessible devices
 * and runnable actions can be found.
 */
export function palette(
  query: string,
  sections: { id: string; label: string }[],
  devices: DeviceSummary[],
  access: Access,
  actionLabel: (id: ActionId) => string,
  locale: string,
  limit = 30,
): PaletteEntry[] {
  const words = query
    .trim()
    .toLocaleLowerCase(locale)
    .split(/\s+/)
    .filter(Boolean);
  const matches = (label: string) => {
    const text = label.toLocaleLowerCase(locale);
    return words.every((word) => text.includes(word));
  };
  const entries: PaletteEntry[] = sections.map((section) => ({
    kind: "section",
    ...section,
  }));
  for (const device of devices) {
    entries.push({ kind: "device", device, label: device.name });
    if (words.length === 0) continue;
    for (const action of deviceActions(device, access)) {
      if (action.blocked !== null || action.id === "open") continue;
      entries.push({
        kind: "action",
        device,
        action,
        label: `${device.name}: ${actionLabel(action.id)}`,
      });
    }
  }
  return entries.filter((entry) => matches(entry.label)).slice(0, limit);
}
