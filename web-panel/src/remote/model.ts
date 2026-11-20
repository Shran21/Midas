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

export function validReason(value: string) {
  return (
    value.trim().length > 0 &&
    new TextEncoder().encode(value).length <= 512 &&
    !Array.from(value).some((char) => {
      const code = char.codePointAt(0) ?? 0;
      return code < 32 || (code >= 127 && code <= 159);
    })
  );
}

export function readDecision(
  value: unknown,
  selection: SessionSelection,
  now: number,
) {
  if (!record(value) || !Number.isFinite(now)) return null;
  if (value.decision === "local_consent_required")
    return { kind: "consent" as const };
  if (
    value.decision !== "eligible" ||
    !record(value.session) ||
    value.session.connection_id !== selection.connection_id ||
    value.session.os_session_id !== selection.os_session_id ||
    value.session.generation !== selection.generation ||
    typeof value.expires_at !== "number" ||
    !Number.isSafeInteger(value.expires_at) ||
    value.expires_at * 1000 <= now ||
    value.expires_at * 1000 > now + OBSERVATION_LIFETIME
  )
    return null;
  return { kind: "eligible" as const, expires: value.expires_at * 1000 };
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

export type RequestState =
  | "waiting"
  | "active"
  | "declined"
  | "expired"
  | "unavailable"
  | "cancelled"
  | "ended"
  | "failed";

/** What a session may move besides the picture and input; each needs its own permission. */
export type Transfer = {
  clipboard: boolean;
  /** Files to the device. */
  upload: boolean;
  /** Files from the device. */
  download: boolean;
};

export const NO_TRANSFER: Transfer = {
  clipboard: false,
  upload: false,
  download: false,
};

function readTransfer(value: unknown): Transfer | null {
  if (
    !record(value) ||
    typeof value.clipboard !== "boolean" ||
    typeof value.upload !== "boolean" ||
    typeof value.download !== "boolean"
  )
    return null;
  return {
    clipboard: value.clipboard,
    upload: value.upload,
    download: value.download,
  };
}

export function sameTransfer(a: Transfer, b: Transfer) {
  return (
    a.clipboard === b.clipboard &&
    a.upload === b.upload &&
    a.download === b.download
  );
}

export type RemoteRequest = {
  id: string;
  mode: "normal" | "administrative";
  uac: boolean;
  transfer: Transfer;
  os_session_id: string;
  /** The requester's name as the consent window showed it. */
  requester: string;
  state: RequestState;
  /** Unix seconds: the local window closes (waiting) or the grant ends (active). */
  until: number | null;
  reason: string | null;
};

/** A local answer longer than this is never shown, whatever the server says. */
export const CONSENT_LIFETIME = 60_000;

export function finished(state: RequestState) {
  return state !== "waiting" && state !== "active";
}

export function readRequest(
  value: unknown,
  expected?: { id: string; os_session_id: string },
): RemoteRequest | null {
  if (
    !record(value) ||
    !uuid(value.id) ||
    (value.mode !== "normal" && value.mode !== "administrative") ||
    typeof value.uac !== "boolean" ||
    typeof value.os_session_id !== "string" ||
    !/^[1-9]\d{0,9}$/.test(value.os_session_id) ||
    typeof value.requester !== "string" ||
    value.requester.length > 200 ||
    (expected !== undefined &&
      (value.id !== expected.id ||
        value.os_session_id !== expected.os_session_id))
  )
    return null;
  const transfer = readTransfer(value.transfer);
  if (!transfer) return null;
  const time = (field: unknown) =>
    typeof field === "number" && Number.isSafeInteger(field) && field > 0
      ? field
      : null;
  let until: number | null = null;
  let reason: string | null = null;
  switch (value.state) {
    case "waiting":
      until = time(value.answer_by);
      if (until === null) return null;
      break;
    case "active":
      until = time(value.until);
      if (until === null) return null;
      break;
    case "failed":
      if (
        typeof value.reason !== "string" ||
        !/^[a-z_]{1,64}$/.test(value.reason)
      )
        return null;
      reason = value.reason;
      break;
    case "ended":
      // Present when the session did not end by the operator's own action.
      if (value.reason !== undefined) {
        if (
          typeof value.reason !== "string" ||
          !/^[a-z_]{1,64}$/.test(value.reason)
        )
          return null;
        reason = value.reason;
      }
      break;
    case "declined":
    case "expired":
    case "unavailable":
    case "cancelled":
      break;
    default:
      return null;
  }
  return {
    id: value.id,
    mode: value.mode,
    uac: value.uac,
    transfer,
    os_session_id: value.os_session_id,
    requester: value.requester,
    state: value.state,
    until,
    reason,
  };
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
