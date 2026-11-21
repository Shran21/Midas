/** Characters of one chat message, as both the server and the agent accept them. */
export const CHAT_LIMIT = 1000;

export type ChatMessage = {
  seq: number;
  from: "operator" | "user";
  text: string;
  /** Unix seconds. */
  at: number;
};

/** Not blank, within the limit, and no control character other than a line break. */
export function chatValid(text: string) {
  return (
    text.trim() !== "" &&
    [...text].length <= CHAT_LIMIT &&
    !/[\p{Cc}]/u.test(text.replace(/\n/g, ""))
  );
}

function readMessage(value: unknown): ChatMessage | null {
  if (typeof value !== "object" || value === null) return null;
  const message = value as Record<string, unknown>;
  if (
    typeof message.seq !== "number" ||
    !Number.isSafeInteger(message.seq) ||
    message.seq < 1 ||
    (message.from !== "operator" && message.from !== "user") ||
    typeof message.text !== "string" ||
    !chatValid(message.text) ||
    typeof message.at !== "number" ||
    !Number.isSafeInteger(message.at)
  )
    return null;
  return {
    seq: message.seq,
    from: message.from,
    text: message.text,
    at: message.at,
  };
}

/** The server's chat page; `null` when anything in it is malformed. */
export function readChat(value: unknown): ChatMessage[] | null {
  if (typeof value !== "object" || value === null) return null;
  const { messages } = value as { messages?: unknown };
  if (!Array.isArray(messages)) return null;
  const read = messages.map(readMessage);
  return read.every((message) => message !== null)
    ? (read as ChatMessage[])
    : null;
}

/** Adds new messages in order, each once. */
export function merge(known: ChatMessage[], arrived: ChatMessage[]) {
  const last = known.at(-1)?.seq ?? 0;
  const fresh = arrived
    .filter((message) => message.seq > last)
    .sort((a, b) => a.seq - b.seq);
  return fresh.length ? [...known, ...fresh] : known;
}

export function readSent(value: unknown) {
  return readMessage(value);
}
