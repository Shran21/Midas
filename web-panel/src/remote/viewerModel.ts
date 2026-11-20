/** The video's point under the pointer, 0..1 in both directions, or null outside the picture. */
export function videoPoint(
  clientX: number,
  clientY: number,
  box: { left: number; top: number; width: number; height: number },
  video: { width: number; height: number },
) {
  if (
    ![clientX, clientY, box.width, box.height, video.width, video.height].every(
      Number.isFinite,
    ) ||
    box.width <= 0 ||
    box.height <= 0 ||
    video.width <= 0 ||
    video.height <= 0
  )
    return null;
  // The element letterboxes the picture ("object-fit: contain").
  const scale = Math.min(box.width / video.width, box.height / video.height);
  const width = video.width * scale;
  const height = video.height * scale;
  const left = box.left + (box.width - width) / 2;
  const top = box.top + (box.height - height) / 2;
  const x = (clientX - left) / width;
  const y = (clientY - top) / height;
  if (x < 0 || x > 1 || y < 0 || y > 1) return null;
  return { x, y };
}

/** Physical key names the agent maps to scan codes; anything else stays in the browser. */
export function keyCode(code: string) {
  return /^[A-Za-z0-9]{1,32}$/.test(code) ? code : null;
}

export type Stats = {
  width: number;
  height: number;
  targetBitrate: number;
  targetFps: number;
  sentBitrate: number;
  sentFps: number;
  profile: "text" | "motion";
  auto: boolean;
  /** What the remote computer shows: the user's own desktop, an elevated window in front, or
   * the secure desktop of a UAC prompt. */
  desktop: "user" | "elevated" | "secure";
  /** Smoothed round-trip time and lost fraction; `null` while not measured. */
  rttMs: number | null;
  loss: number | null;
  /** Loss, delay or jitter above their limits right now. */
  degraded: boolean;
  /** Whether packets went directly or through the relay; `null` before any went. */
  path: "direct" | "relay" | null;
};

/** Key combinations the browser cannot capture itself, sent as physical keys in order. */
export const SYSTEM_KEYS = {
  start: ["MetaLeft"],
  switchWindow: ["AltLeft", "Tab"],
  taskManager: ["ControlLeft", "ShiftLeft", "Escape"],
  close: ["AltLeft", "F4"],
  run: ["MetaLeft", "KeyR"],
  desktop: ["MetaLeft", "KeyD"],
} as const;

/** The modifier keys held with a pointer event: Ctrl 1, Shift 2, Alt 4, Windows 8. */
export function modifiers(event: {
  ctrlKey: boolean;
  shiftKey: boolean;
  altKey: boolean;
  metaKey: boolean;
}) {
  return (
    (event.ctrlKey ? 1 : 0) |
    (event.shiftKey ? 2 : 0) |
    (event.altKey ? 4 : 0) |
    (event.metaKey ? 8 : 0)
  );
}

/** Characters per text message, as the agent accepts them. */
const TEXT_PIECE = 64;
/** The most text one send types. */
export const TEXT_LIMIT = 2000;

/** Text messages for `text`: whole characters, at most TEXT_PIECE each; other control
 * characters than line breaks and tabs are dropped. */
export function textMessages(text: string) {
  const characters = [...text.replace(/\r\n?/g, "\n")]
    .filter((c) => c === "\n" || c === "\t" || !/\p{Cc}/u.test(c))
    .slice(0, TEXT_LIMIT);
  const messages: { type: "text"; text: string }[] = [];
  for (let start = 0; start < characters.length; start += TEXT_PIECE)
    messages.push({
      type: "text",
      text: characters.slice(start, start + TEXT_PIECE).join(""),
    });
  return messages;
}

/** The agent's answer to Ctrl+Alt+Del, or `null` for any other message. */
export function readSasResult(text: string): "sent" | "unavailable" | null {
  try {
    const value = JSON.parse(text) as { type?: unknown; result?: unknown };
    if (value?.type !== "sas") return null;
    return value.result === "sent" || value.result === "unavailable"
      ? value.result
      : null;
  } catch {
    return null;
  }
}

/** The key messages for one combination: every key down in order, then up in reverse. */
export function combination(keys: readonly string[]) {
  return [
    ...keys.map((code) => ({ type: "key", code, down: true })),
    ...[...keys].reverse().map((code) => ({ type: "key", code, down: false })),
  ];
}

function count(value: unknown, limit: number) {
  return typeof value === "number" &&
    Number.isFinite(value) &&
    value >= 0 &&
    value <= limit
    ? value
    : null;
}

/** The agent's once-a-second report; anything malformed counts as no measurement. */
export function readStats(text: string): Stats | null {
  let value: unknown;
  try {
    value = JSON.parse(text);
  } catch {
    return null;
  }
  if (typeof value !== "object" || value === null) return null;
  const record = value as Record<string, unknown>;
  if (record.type !== "stats") return null;
  const width = count(record.width, 16384);
  const height = count(record.height, 16384);
  const targetBitrate = count(record.target_bitrate, 1e9);
  const targetFps = count(record.target_fps, 240);
  const sentBitrate = count(record.sent_bitrate, 1e9);
  const sentFps = count(record.sent_fps, 240);
  // Agents without a privileged component never report a desktop.
  const desktop = record.desktop ?? "user";
  if (desktop !== "user" && desktop !== "elevated" && desktop !== "secure")
    return null;
  const optional = (field: unknown, limit: number) =>
    field === undefined || field === null ? null : count(field, limit);
  const rttMs = optional(record.rtt_ms, 60_000);
  const loss = optional(record.loss, 1);
  if (
    (record.rtt_ms != null && rttMs === null) ||
    (record.loss != null && loss === null)
  )
    return null;
  const network = record.network ?? "good";
  if (network !== "good" && network !== "degraded") return null;
  const path = record.path ?? null;
  if (path !== null && path !== "direct" && path !== "relay") return null;
  if (
    width === null ||
    height === null ||
    targetBitrate === null ||
    targetFps === null ||
    sentBitrate === null ||
    sentFps === null ||
    (record.profile !== "text" && record.profile !== "motion") ||
    typeof record.auto !== "boolean"
  )
    return null;
  return {
    width,
    height,
    targetBitrate,
    targetFps,
    sentBitrate,
    sentFps,
    profile: record.profile,
    auto: record.auto,
    desktop,
    rttMs,
    loss,
    degraded: network === "degraded",
    path,
  };
}

/** The relays the server named for this session; anything malformed means none. */
export function readIceServers(value: unknown): RTCIceServer[] {
  if (typeof value !== "object" || value === null) return [];
  const list = (value as { ice_servers?: unknown }).ice_servers;
  if (!Array.isArray(list) || list.length > 4) return [];
  const servers: RTCIceServer[] = [];
  for (const item of list) {
    if (typeof item !== "object" || item === null) return [];
    const { urls, username, credential } = item as Record<string, unknown>;
    if (
      !Array.isArray(urls) ||
      urls.length === 0 ||
      urls.length > 4 ||
      !urls.every(
        (url) =>
          typeof url === "string" &&
          url.length <= 256 &&
          /^turns?:[^\s"<>\\]+$/.test(url),
      ) ||
      typeof username !== "string" ||
      username.length === 0 ||
      username.length > 128 ||
      typeof credential !== "string" ||
      credential.length === 0 ||
      credential.length > 128
    )
      return [];
    servers.push({ urls: urls as string[], username, credential });
  }
  return servers;
}

/** Bits per second in the unit people read. */
export function bitrate(bps: number, language: "hu" | "en") {
  const mega = bps >= 1_000_000;
  const value = mega ? bps / 1_000_000 : bps / 1000;
  const formatted = new Intl.NumberFormat(
    language === "hu" ? "hu-HU" : "en-GB",
    {
      maximumFractionDigits: mega ? 1 : 0,
    },
  ).format(value);
  return `${formatted} ${mega ? "Mbit/s" : "kbit/s"}`;
}
