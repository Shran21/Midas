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
};

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
  };
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
