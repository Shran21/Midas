/** File bytes in one data message, as the agent accepts them. */
export const CHUNK = 16 * 1024;
/** Bytes the sender may have unacknowledged. */
export const WINDOW = 1024 * 1024;
/** The browser acknowledges a download after this many new bytes. */
export const ACK_STEP = 256 * 1024;
/** Clipboard text, in UTF-8 bytes. */
export const CLIPBOARD_LIMIT = 32 * 1024;
export const FILE_LIMIT = 4 * 1024 * 1024 * 1024;

const TO_DEVICE = 1;
const FROM_DEVICE = 2;
const HEADER = 1 + 16 + 8;
const HEX_ID = /^[0-9a-f]{32}$/;
const HEX_DIGEST = /^[0-9a-f]{64}$/;

export type Entry = {
  name: string;
  dir: boolean;
  size: number;
  /** Unix seconds. */
  modified: number;
};

export type Problem =
  | "unavailable"
  | "denied"
  | "not_found"
  | "too_large"
  | "changed"
  | "no_space"
  | "empty"
  | "invalid";

export type Reply =
  | { type: "clipboard"; text: string }
  | { type: "clipboard_set" }
  | { type: "upload_ready"; id: string; offset: number }
  | { type: "upload_ack"; id: string; offset: number }
  | {
      type: "upload_done";
      id: string;
      name: string;
      size: number;
      sha256: string;
    }
  | {
      type: "listing";
      path: string;
      parent: string | null;
      entries: Entry[];
      truncated: boolean;
    }
  | { type: "download_done"; id: string; size: number; sha256: string }
  | {
      type: "failed";
      id: string | null;
      operation: "clipboard_set" | "clipboard_get" | "list" | null;
      problem: Problem;
    };

const PROBLEMS: readonly Problem[] = [
  "unavailable",
  "denied",
  "not_found",
  "too_large",
  "changed",
  "no_space",
  "empty",
  "invalid",
];

function hex(bytes: Uint8Array) {
  return [...bytes].map((byte) => byte.toString(16).padStart(2, "0")).join("");
}

function idBytes(id: string) {
  const bytes = new Uint8Array(16);
  for (let index = 0; index < 16; index++)
    bytes[index] = Number.parseInt(id.slice(2 * index, 2 * index + 2), 16);
  return bytes;
}

/** The same file gets the same id in a later session, so the transfer resumes. */
export async function transferId(...parts: (string | number)[]) {
  const digest = await crypto.subtle.digest(
    "SHA-256",
    new TextEncoder().encode(parts.join("\u0000")),
  );
  return hex(new Uint8Array(digest).slice(0, 16));
}

/** File bytes to the device: `[1][id][offset LE][bytes]`. */
export function dataMessage(id: string, offset: number, bytes: Uint8Array) {
  const message = new Uint8Array(HEADER + bytes.length);
  message[0] = TO_DEVICE;
  message.set(idBytes(id), 1);
  new DataView(message.buffer).setBigUint64(17, BigInt(offset), true);
  message.set(bytes, HEADER);
  return message;
}

/** File bytes from the device, or `null` for anything else. */
export function readData(buffer: ArrayBuffer) {
  const message = new Uint8Array(buffer);
  if (
    message.length <= HEADER ||
    message.length > HEADER + CHUNK ||
    message[0] !== FROM_DEVICE
  )
    return null;
  const offset = Number(
    new DataView(buffer).getBigUint64(17, true) & 0xffff_ffff_ffffn,
  );
  const bytes = message.slice(HEADER);
  if (offset + bytes.length > FILE_LIMIT) return null;
  return { id: hex(message.slice(1, 17)), offset, bytes };
}

const record = (value: unknown): value is Record<string, unknown> =>
  typeof value === "object" && value !== null && !Array.isArray(value);
const count = (value: unknown): value is number =>
  typeof value === "number" &&
  Number.isSafeInteger(value) &&
  value >= 0 &&
  value <= FILE_LIMIT;
const name = (value: unknown): value is string =>
  typeof value === "string" &&
  value.length > 0 &&
  value.length <= 255 &&
  !/[\\/\p{Cc}]/u.test(value);
const path = (value: unknown): value is string =>
  typeof value === "string" &&
  value.length > 0 &&
  value.length <= 1024 &&
  !/\p{Cc}/u.test(value);
const id = (value: unknown): value is string =>
  typeof value === "string" && HEX_ID.test(value);

function readEntry(value: unknown): Entry | null {
  if (
    !record(value) ||
    !name(value.name) ||
    typeof value.dir !== "boolean" ||
    !count(value.size) ||
    typeof value.modified !== "number" ||
    !Number.isSafeInteger(value.modified)
  )
    return null;
  return {
    name: value.name,
    dir: value.dir,
    size: value.size,
    modified: value.modified,
  };
}

/** An answer from the device, read strictly; `null` for anything malformed. */
export function readReply(text: string): Reply | null {
  let value: unknown;
  try {
    value = JSON.parse(text);
  } catch {
    return null;
  }
  if (!record(value)) return null;
  switch (value.type) {
    case "clipboard":
      return typeof value.text === "string"
        ? { type: "clipboard", text: value.text }
        : null;
    case "clipboard_set":
      return { type: "clipboard_set" };
    case "upload_ready":
    case "upload_ack":
      return id(value.id) && count(value.offset)
        ? { type: value.type, id: value.id, offset: value.offset }
        : null;
    case "upload_done":
      return id(value.id) &&
        name(value.name) &&
        count(value.size) &&
        typeof value.sha256 === "string" &&
        HEX_DIGEST.test(value.sha256)
        ? {
            type: "upload_done",
            id: value.id,
            name: value.name,
            size: value.size,
            sha256: value.sha256,
          }
        : null;
    case "download_done":
      return id(value.id) &&
        count(value.size) &&
        typeof value.sha256 === "string" &&
        HEX_DIGEST.test(value.sha256)
        ? {
            type: "download_done",
            id: value.id,
            size: value.size,
            sha256: value.sha256,
          }
        : null;
    case "listing": {
      if (
        !path(value.path) ||
        (value.parent != null && !path(value.parent)) ||
        !Array.isArray(value.entries) ||
        typeof value.truncated !== "boolean"
      )
        return null;
      const entries = value.entries.map(readEntry);
      if (entries.some((entry) => entry === null)) return null;
      return {
        type: "listing",
        path: value.path,
        parent: (value.parent as string | null | undefined) ?? null,
        entries: entries as Entry[],
        truncated: value.truncated,
      };
    }
    case "failed": {
      const operation = value.operation ?? null;
      const failedId = value.id ?? null;
      if (
        !PROBLEMS.includes(value.problem as Problem) ||
        (failedId !== null && !id(failedId)) ||
        (operation !== null &&
          operation !== "clipboard_set" &&
          operation !== "clipboard_get" &&
          operation !== "list")
      )
        return null;
      return {
        type: "failed",
        id: failedId,
        operation,
        problem: value.problem as Problem,
      };
    }
    default:
      return null;
  }
}

/** UTF-8 length, which the agent's limit counts. */
export function utf8Length(text: string) {
  return new TextEncoder().encode(text).length;
}

/** "1,5 MB" style sizes. */
export function size(bytes: number, locale: string) {
  const units = ["B", "KB", "MB", "GB"];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit++;
  }
  return `${value.toLocaleString(locale, { maximumFractionDigits: unit === 0 ? 0 : 1 })} ${units[unit]}`;
}
