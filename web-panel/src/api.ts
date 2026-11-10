export type Method =
  | "passkey"
  | "password_totp"
  | "recovery_code"
  | "directory";
export type Session = {
  id: string;
  operator_id: string;
  authentication_method: Method;
  authenticated_at: number;
  created_at: number;
  last_seen_at: number;
  expires_at: number;
};
export type SignedIn = {
  session: Session;
  csrf_token: string;
  operator: { login: string; display_name: string; platform_admin: boolean };
};
export type Scope =
  | { kind: "tenant" }
  | { kind: "site" | "group" | "device"; id: string };
export type Site = { id: string; name: string };
export type Role = {
  id: string;
  name: string;
  builtin: boolean;
  allows: string[];
  denies: string[];
};
export type Member = {
  operator_id: string;
  login: string;
  display_name: string;
  enabled: boolean;
  assignments: {
    id: string;
    role_id: string;
    role_name: string;
    scope: Scope;
  }[];
  overrides: {
    id: string;
    permission: string;
    effect: "allow" | "deny";
    scope: Scope;
  }[];
};
export type CreatedOperator = {
  id: string;
  login: string;
  display_name: string;
  recovery_codes: string[];
};
export type Tenant = { id: string; name: string };
export type TenantContext = Tenant & {
  capabilities: string[];
  tenant_wide: string[];
  fresh_passkey: string[];
};
export type Named = { id: string; name: string };
export type HealthState = "ok" | "warning" | "critical" | "unavailable";
export type DeviceSummary = {
  id: string;
  name: string;
  kind: "workstation" | "server";
  platform: "windows" | "linux";
  enrollment: "active" | "revoked";
  site: Named | null;
  groups: Named[];
  online: boolean;
  last_seen_at: number | null;
  decided_at: number | null;
  os: string | null;
  capabilities: string[];
  health: HealthState | null;
  health_reported_at: number | null;
  inventory_reconciled_at: number | null;
  restart_pending: boolean;
  open_alerts: number;
  critical_alerts: number;
  metrics: {
    at: number;
    cpu_permille: number;
    memory_used: number;
    memory_total: number;
    disk_used: number;
    disk_total: number;
  } | null;
  permissions: string[];
};
export type HealthCheck = {
  id: string;
  state: HealthState;
  code: string;
  summary: string;
  subjects: string[];
  changed_at: number;
};
export type HealthView = {
  capabilities: string[];
  reported_at: number | null;
  checks: HealthCheck[];
};
export type InventoryView = {
  reconciled_at: number | null;
  items: { kind: string; key: string; value: string; updated_at: number }[];
};
export type MaintenanceWindow = { starts_at: number; ends_at: number };
export type DeviceUnattendedSpec = {
  enabled: boolean;
  maintenance_window: MaintenanceWindow | null;
};
export type DeviceUnattendedPolicy = DeviceUnattendedSpec & {
  tenant_enabled: boolean;
  within_maintenance_window: boolean;
  reboot_confirmation_required: boolean;
  reconnect_verification_required: boolean;
};
export type EffectiveSetting = {
  key: string;
  value: number;
  default: number;
  source: {
    level: "default" | "tenant" | "site" | "group";
    id: string | null;
    name: string | null;
  };
};
export type Sample = {
  at: number;
  cpu_permille: number;
  memory_used: number;
  memory_total: number;
  disk_used: number;
  disk_total: number;
  net_rx: number;
  net_tx: number;
};
export type MetricsView = {
  recent: Sample[];
  hourly: {
    hour: number;
    cpu_avg_permille: number;
    cpu_max_permille: number;
    memory_used_avg: number;
    memory_total: number;
    disk_used_max: number;
    disk_total: number;
  }[];
};
export type AlertView = {
  id: string;
  device_id: string;
  device_name: string;
  kind: string;
  severity: "warning" | "critical";
  summary: string;
  opened_at: number;
  last_seen_at: number;
  occurrences: number;
  resolved_at: number | null;
  acknowledged_at: number | null;
  acknowledged_by: string | null;
  notifications_sent: number;
  notifications_pending: number;
  notifications_failed: number;
};
export type Group = { id: string; name: string; priority: number };
export type PendingDevice = {
  id: string;
  name: string;
  platform: "windows" | "linux";
  kind: "workstation" | "server";
  site_id: string | null;
  requested_at: number;
};
export type Decision = {
  id: string;
  name: string;
  platform: "windows" | "linux";
  kind: "workstation" | "server";
  site_id: string | null;
  enrollment: "active" | "rejected" | "revoked";
  requested_at: number | null;
  decided_at: number | null;
  decided_by: string | null;
};
export type TokenSpec = {
  site_id: string | null;
  platform: "windows" | "linux";
  kind: "workstation" | "server";
  preapproved: boolean;
  lifetime_hours: number;
  uses: number;
};
export type CreatedToken = { id: string; token: string; expires_at: number };
export type ConnectorView = {
  device_id: string;
  domain: string;
  url: string;
  base_dns: string[];
  revision: number;
  last_check: {
    at: number;
    revision: number;
    ok: boolean;
    objects: number;
    error: string | null;
  } | null;
};
export type DiscoveredComputer = {
  domain: string;
  object_guid: string;
  name: string;
  dns_host_name: string;
  distinguished_name: string;
  operating_system: string;
  operating_system_version: string;
  last_logon_at: number | null;
  disabled: boolean;
  first_seen_at: number;
  last_seen_at: number;
  removed_at: number | null;
  status: "candidate" | "managed" | "conflict";
  devices: Named[];
  name_matches: Named[];
};

export class ApiError extends Error {
  constructor(
    readonly status: number,
    readonly code: string,
  ) {
    super(code);
  }
}

async function request<T>(
  path: string,
  init: { method?: string; body?: unknown; csrf?: string } = {},
): Promise<T> {
  const headers: Record<string, string> = {};
  if (init.body !== undefined) headers["content-type"] = "application/json";
  if (init.csrf) headers["x-csrf-token"] = init.csrf;
  let response: Response;
  try {
    response = await fetch(path, {
      method: init.method ?? "GET",
      headers,
      body: init.body === undefined ? undefined : JSON.stringify(init.body),
      credentials: "same-origin",
      cache: "no-store",
    });
  } catch {
    throw new ApiError(0, "network");
  }
  if (response.status === 204) return undefined as T;
  const body: unknown = await response.json().catch(() => null);
  if (response.ok && (typeof body !== "object" || body === null))
    throw new ApiError(502, "unexpected");
  if (!response.ok) {
    const code =
      typeof body === "object" &&
      body !== null &&
      "error" in body &&
      typeof body.error === "string"
        ? body.error
        : "unexpected";
    throw new ApiError(response.status, code);
  }
  return body as T;
}

export const api = {
  session: () => request<SignedIn>("/api/session"),
  password: (login: string, password: string, code: string) =>
    request<SignedIn>("/api/auth/password", {
      method: "POST",
      body: { login, password, code },
    }),
  directory: (login: string, password: string) =>
    request<SignedIn>("/api/auth/directory", {
      method: "POST",
      body: { login, password },
    }),
  recovery: (login: string, code: string) =>
    request<SignedIn>("/api/auth/recovery", {
      method: "POST",
      body: { login, code },
    }),
  passkey: async (login: string) => {
    const issued = await request<{
      id: string;
      options: { publicKey: Record<string, unknown> };
    }>("/api/auth/passkey/start", { method: "POST", body: { login } });
    const credential = await getCredential(issued.options.publicKey);
    return request<SignedIn>("/api/auth/passkey/finish", {
      method: "POST",
      body: { id: issued.id, response: credential },
    });
  },
  tenants: () => request<Tenant[]>("/api/tenants"),
  tenant: (id: string) =>
    request<TenantContext>(`/api/tenants/${encodeURIComponent(id)}`),
  sessions: () => request<Session[]>("/api/sessions"),
  revoke: (id: string, csrf: string) =>
    request<void>(`/api/sessions/${encodeURIComponent(id)}`, {
      method: "DELETE",
      csrf,
    }),
  roles: () => request<Role[]>("/api/roles"),
  members: (tenant: string) =>
    request<Member[]>(`/api/tenants/${encodeURIComponent(tenant)}/members`),
  sites: (tenant: string) =>
    request<Site[]>(`/api/tenants/${encodeURIComponent(tenant)}/sites`),
  createSite: (tenant: string, name: string, csrf: string) =>
    request<Site>(`/api/tenants/${encodeURIComponent(tenant)}/sites`, {
      method: "POST",
      body: { name },
      csrf,
    }),
  addMember: (tenant: string, login: string, csrf: string) =>
    request<{ id: string }>(
      `/api/tenants/${encodeURIComponent(tenant)}/members`,
      { method: "POST", body: { login }, csrf },
    ),
  setMember: (
    tenant: string,
    operator: string,
    enabled: boolean,
    csrf: string,
  ) =>
    request<void>(
      `/api/tenants/${encodeURIComponent(tenant)}/members/${encodeURIComponent(operator)}`,
      { method: "PUT", body: { enabled }, csrf },
    ),
  assign: (
    tenant: string,
    body: { operator: string; role: string; scope: Scope },
    csrf: string,
  ) =>
    request<{ id: string }>(
      `/api/tenants/${encodeURIComponent(tenant)}/assignments`,
      { method: "POST", body, csrf },
    ),
  unassign: (tenant: string, id: string, csrf: string) =>
    request<void>(
      `/api/tenants/${encodeURIComponent(tenant)}/assignments/${encodeURIComponent(id)}`,
      { method: "DELETE", csrf },
    ),
  override: (
    tenant: string,
    body: {
      operator: string;
      permission: string;
      effect: "allow" | "deny";
      scope: Scope;
    },
    csrf: string,
  ) =>
    request<{ id: string }>(
      `/api/tenants/${encodeURIComponent(tenant)}/overrides`,
      { method: "POST", body, csrf },
    ),
  removeOverride: (tenant: string, id: string, csrf: string) =>
    request<void>(
      `/api/tenants/${encodeURIComponent(tenant)}/overrides/${encodeURIComponent(id)}`,
      { method: "DELETE", csrf },
    ),
  createTenant: (name: string, csrf: string) =>
    request<Tenant>("/api/admin/tenants", {
      method: "POST",
      body: { name },
      csrf,
    }),
  createOperator: (login: string, display_name: string, csrf: string) =>
    request<CreatedOperator>("/api/admin/operators", {
      method: "POST",
      body: { login, display_name },
      csrf,
    }),
  setup: async (token: string, login: string, display_name: string) => {
    const issued = await request<{
      id: string;
      options: { publicKey: Record<string, unknown> };
    }>("/api/setup/start", {
      method: "POST",
      body: { token, login, display_name },
    });
    const response = await createCredential(issued.options.publicKey);
    return request<SignedIn>("/api/setup/finish", {
      method: "POST",
      body: { token, id: issued.id, response },
    });
  },
  addPasskey: async (csrf: string) => {
    const issued = await request<{
      id: string;
      options: { publicKey: Record<string, unknown> };
    }>("/api/account/passkeys/start", { method: "POST", body: {}, csrf });
    const response = await createCredential(issued.options.publicKey);
    return request<SignedIn>("/api/account/passkeys/finish", {
      method: "POST",
      body: { id: issued.id, response },
      csrf,
    });
  },
  logout: (csrf: string) =>
    request<void>("/api/session/logout", { method: "POST", csrf }),
  devices: (tenant: string) =>
    request<DeviceSummary[]>(`${tenantPath(tenant)}/devices`),
  device: (tenant: string, device: string) =>
    request<DeviceSummary>(devicePath(tenant, device)),
  health: (tenant: string, device: string) =>
    request<HealthView>(`${devicePath(tenant, device)}/health`),
  inventory: (tenant: string, device: string) =>
    request<InventoryView>(`${devicePath(tenant, device)}/inventory`),
  policy: (tenant: string, device: string) =>
    request<EffectiveSetting[]>(`${devicePath(tenant, device)}/policy`),
  tenantUnattended: (tenant: string) =>
    request<{ enabled: boolean }>(`${tenantPath(tenant)}/unattended-policy`),
  setTenantUnattended: (tenant: string, enabled: boolean, csrf: string) =>
    request<void>(`${tenantPath(tenant)}/unattended-policy`, {
      method: "PUT",
      body: { enabled },
      csrf,
    }),
  deviceUnattended: (tenant: string, device: string) =>
    request<DeviceUnattendedPolicy>(
      `${devicePath(tenant, device)}/unattended-policy`,
    ),
  setDeviceUnattended: (
    tenant: string,
    device: string,
    body: DeviceUnattendedSpec,
    csrf: string,
  ) =>
    request<void>(`${devicePath(tenant, device)}/unattended-policy`, {
      method: "PUT",
      body,
      csrf,
    }),
  metrics: (tenant: string, device: string) =>
    request<MetricsView>(`${devicePath(tenant, device)}/metrics`),
  deviceAlerts: (tenant: string, device: string) =>
    request<AlertView[]>(`${devicePath(tenant, device)}/alerts`),
  refresh: (tenant: string, device: string, csrf: string) =>
    request<void>(`${devicePath(tenant, device)}/refresh`, {
      method: "POST",
      csrf,
    }),
  revokeDevice: (tenant: string, device: string, csrf: string) =>
    request<void>(`${devicePath(tenant, device)}/revoke`, {
      method: "POST",
      csrf,
    }),
  alerts: (tenant: string, openOnly: boolean) =>
    request<AlertView[]>(
      `${tenantPath(tenant)}/alerts${openOnly ? "/open" : ""}`,
    ),
  acknowledge: (tenant: string, alert: string, csrf: string) =>
    request<void>(
      `${tenantPath(tenant)}/alerts/${encodeURIComponent(alert)}/acknowledge`,
      { method: "POST", csrf },
    ),
  groups: (tenant: string) => request<Group[]>(`${tenantPath(tenant)}/groups`),
  pendingDevices: (tenant: string) =>
    request<PendingDevice[]>(`${tenantPath(tenant)}/devices/pending`),
  decisions: (tenant: string) =>
    request<Decision[]>(`${tenantPath(tenant)}/devices/decisions`),
  decide: (
    tenant: string,
    device: string,
    decision: "approve" | "reject",
    csrf: string,
  ) =>
    request<void>(`${devicePath(tenant, device)}/${decision}`, {
      method: "POST",
      csrf,
    }),
  createToken: (tenant: string, spec: TokenSpec, csrf: string) =>
    request<CreatedToken>(`${tenantPath(tenant)}/enrollment-tokens`, {
      method: "POST",
      body: spec,
      csrf,
    }),
  connector: (tenant: string) =>
    request<ConnectorView>(`${tenantPath(tenant)}/directory-connector`),
  discovered: (tenant: string) =>
    request<DiscoveredComputer[]>(`${tenantPath(tenant)}/directory/computers`),
  setGroupMember: (
    tenant: string,
    group: string,
    device: string,
    member: boolean,
    csrf: string,
  ) =>
    request<void>(
      `${tenantPath(tenant)}/groups/${encodeURIComponent(group)}/devices/${encodeURIComponent(device)}`,
      { method: member ? "PUT" : "DELETE", csrf },
    ),
};

function tenantPath(tenant: string) {
  return `/api/tenants/${encodeURIComponent(tenant)}`;
}

function devicePath(tenant: string, device: string) {
  return `${tenantPath(tenant)}/devices/${encodeURIComponent(device)}`;
}

function decode(value: string) {
  const base64 = value.replace(/-/g, "+").replace(/_/g, "/");
  const padded = base64 + "=".repeat((4 - (base64.length % 4)) % 4);
  return Uint8Array.from(atob(padded), (c) => c.charCodeAt(0));
}

function encode(buffer: ArrayBuffer | null) {
  if (!buffer) return null;
  let text = "";
  for (const byte of new Uint8Array(buffer)) text += String.fromCharCode(byte);
  return btoa(text).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

async function getCredential(options: Record<string, unknown>) {
  if (!window.PublicKeyCredential || !navigator.credentials) {
    throw new ApiError(0, "passkey_unsupported");
  }
  const allow = Array.isArray(options.allowCredentials)
    ? (options.allowCredentials as { id: string; type: string }[])
    : [];
  let credential: Credential | null;
  try {
    credential = await navigator.credentials.get({
      publicKey: {
        ...(options as unknown as PublicKeyCredentialRequestOptions),
        challenge: decode(String(options.challenge)),
        allowCredentials: allow.map((item) => ({
          type: "public-key",
          id: decode(item.id),
        })),
      },
    });
  } catch {
    throw new ApiError(0, "passkey_cancelled");
  }
  if (!(credential instanceof PublicKeyCredential)) {
    throw new ApiError(0, "passkey_cancelled");
  }
  const response = credential.response as AuthenticatorAssertionResponse;
  return {
    id: credential.id,
    rawId: encode(credential.rawId),
    type: credential.type,
    extensions: {},
    response: {
      authenticatorData: encode(response.authenticatorData),
      clientDataJSON: encode(response.clientDataJSON),
      signature: encode(response.signature),
      userHandle: encode(response.userHandle),
    },
  };
}

async function createCredential(options: Record<string, unknown>) {
  if (!window.PublicKeyCredential || !navigator.credentials) {
    throw new ApiError(0, "passkey_unsupported");
  }
  const user = options.user as {
    id: string;
    name: string;
    displayName: string;
  };
  const exclude = Array.isArray(options.excludeCredentials)
    ? (options.excludeCredentials as { id: string; type: string }[])
    : [];
  let credential: Credential | null;
  try {
    credential = await navigator.credentials.create({
      publicKey: {
        ...(options as unknown as PublicKeyCredentialCreationOptions),
        challenge: decode(String(options.challenge)),
        user: { ...user, id: decode(user.id) },
        excludeCredentials: exclude.map((item) => ({
          type: "public-key",
          id: decode(item.id),
        })),
      },
    });
  } catch {
    throw new ApiError(0, "passkey_cancelled");
  }
  if (!(credential instanceof PublicKeyCredential)) {
    throw new ApiError(0, "passkey_cancelled");
  }
  const response = credential.response as AuthenticatorAttestationResponse;
  return {
    id: credential.id,
    rawId: encode(credential.rawId),
    type: credential.type,
    extensions: {},
    response: {
      attestationObject: encode(response.attestationObject),
      clientDataJSON: encode(response.clientDataJSON),
    },
  };
}

/**
 * Opens the session status channel; `onEnd` runs when the server closes it.
 * While a tenant is watched, `onChange` runs when its devices change; the notice carries no details.
 */
export function watchSession(
  csrf: string,
  onEnd: () => void,
  tenant: string | null = null,
  onChange: () => void = () => {},
) {
  const scheme = location.protocol === "https:" ? "wss:" : "ws:";
  const socket = new WebSocket(
    `${scheme}//${location.host}/api/session/socket`,
  );
  let closed = false;
  socket.onopen = () => {
    socket.send(JSON.stringify({ csrf_token: csrf }));
    if (tenant) socket.send(JSON.stringify({ type: "watch", tenant }));
  };
  socket.onmessage = (event) => {
    try {
      const message: unknown = JSON.parse(String(event.data));
      if (
        typeof message === "object" &&
        message !== null &&
        "type" in message &&
        message.type === "devices_changed" &&
        "tenant" in message &&
        message.tenant === tenant
      )
        onChange();
    } catch {}
  };
  socket.onclose = () => {
    if (!closed) onEnd();
  };
  return () => {
    closed = true;
    socket.close();
  };
}
