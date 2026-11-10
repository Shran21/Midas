import type {
  DeviceSummary,
  RemoteSession,
  RemoteSessions,
  SessionSelection,
} from "../api";

export const OBSERVATION_LIFETIME = 30_000;

export function unavailableReason(device: DeviceSummary, failed: boolean) {
  if (
    !device.permissions.includes("device.read") ||
    !device.permissions.includes("remote.view")
  )
    return "denied";
  if (device.enrollment !== "active") return "revoked";
  if (failed) return "unverified";
  if (!device.online) return "offline";
  if (!device.capabilities.includes("remote.sessions")) return "unsupported";
  return null;
}

export function fresh(started: number, now: number) {
  return (
    Number.isFinite(started) &&
    Number.isFinite(now) &&
    now >= started &&
    now - started < OBSERVATION_LIFETIME
  );
}

export function selectionKey(selection: SessionSelection) {
  return `${selection.connection_id}/${selection.os_session_id}/${selection.generation}`;
}

function record(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function uuid(value: unknown): value is string {
  return (
    typeof value === "string" &&
    /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(
      value,
    ) &&
    value !== "00000000-0000-0000-0000-000000000000"
  );
}

export function readSessions(value: unknown): RemoteSessions | null {
  if (
    !record(value) ||
    !uuid(value.connection_id) ||
    !Array.isArray(value.sessions) ||
    value.sessions.length > 128
  )
    return null;
  const sessions: RemoteSession[] = [];
  const ids = new Set<string>();
  for (const entry of value.sessions) {
    if (!record(entry) || !record(entry.selection)) return null;
    const selection = entry.selection;
    if (
      selection.connection_id !== value.connection_id ||
      !uuid(selection.generation) ||
      typeof selection.os_session_id !== "string" ||
      !/^[1-9]\d{0,9}$/.test(selection.os_session_id) ||
      Number(selection.os_session_id) >= 0xffffffff ||
      ids.has(selection.os_session_id)
    )
      return null;
    if (
      typeof entry.account !== "string" ||
      typeof entry.station !== "string" ||
      typeof entry.console !== "boolean" ||
      (entry.locked !== null && typeof entry.locked !== "boolean")
    )
      return null;
    if (
      entry.presence !== "occupied" &&
      entry.presence !== "vacant" &&
      entry.presence !== "unknown"
    )
      return null;
    if (entry.presence === "vacant" && entry.account !== "") return null;
    ids.add(selection.os_session_id);
    sessions.push({
      selection: {
        connection_id: value.connection_id,
        os_session_id: selection.os_session_id,
        generation: selection.generation,
      },
      presence: entry.presence,
      account: entry.account,
      station: entry.station,
      console: entry.console,
      locked: entry.locked,
    });
  }
  return { connection_id: value.connection_id, sessions };
}
