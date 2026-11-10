import type {
  DeviceUnattendedPolicy,
  MaintenanceWindow,
  Session,
} from "../api";

export type Policy = { enabled: boolean } | DeviceUnattendedPolicy;
export type Draft = {
  enabled: boolean;
  start: string;
  end: string;
  base: string;
};

export function policyKey(policy: Policy): string {
  const window =
    "maintenance_window" in policy ? policy.maintenance_window : null;
  return JSON.stringify([
    policy.enabled,
    window?.starts_at ?? null,
    window?.ends_at ?? null,
  ]);
}

export function localTime(seconds: number): string {
  const date = new Date(seconds * 1000);
  const pad = (value: number) => String(value).padStart(2, "0");
  return `${String(date.getFullYear()).padStart(4, "0")}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}T${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`;
}

export function parseLocalTime(value: string): number | null {
  if (!/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}(:\d{2})?$/.test(value)) return null;
  const normalized = value.length === 16 ? `${value}:00` : value;
  const seconds = new Date(normalized).getTime() / 1000;
  if (
    !Number.isSafeInteger(seconds) ||
    seconds < 0 ||
    seconds > 253402300799 ||
    localTime(seconds) !== normalized
  )
    return null;
  // Reject both occurrences of an ambiguous local time at a clock change.
  for (let minutes = -180; minutes <= 180; minutes += 15) {
    if (minutes !== 0 && localTime(seconds + minutes * 60) === normalized)
      return null;
  }
  return seconds;
}

export function draftFor(policy: Policy): Draft {
  const window =
    "maintenance_window" in policy ? policy.maintenance_window : null;
  return {
    enabled: policy.enabled,
    start: window ? localTime(window.starts_at) : "",
    end: window ? localTime(window.ends_at) : "",
    base: policyKey(policy),
  };
}

export function windowFor(
  draft: Draft,
  now: number,
): MaintenanceWindow | "invalidTime" | "invalidOrder" | "expired" | null {
  if (!draft.enabled) return null;
  const starts_at = parseLocalTime(draft.start);
  const ends_at = parseLocalTime(draft.end);
  if (starts_at === null || ends_at === null) return "invalidTime";
  if (ends_at <= starts_at) return "invalidOrder";
  if (ends_at <= now) return "expired";
  return { starts_at, ends_at };
}

export function freshPasskey(session: Session, now: number): boolean {
  return (
    session.authentication_method === "passkey" &&
    session.authenticated_at <= now &&
    now - session.authenticated_at < 300 &&
    session.expires_at > now
  );
}
