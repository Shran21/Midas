import { useEffect, useId, useRef, useState } from "react";
import type { Language } from "../i18n";
import type { Transfer } from "./model";
import { transferCopy } from "./transferCopy";
import {
  ACK_STEP,
  CHUNK,
  CLIPBOARD_LIMIT,
  dataMessage,
  type Entry,
  type Reply,
  readData,
  readReply,
  size,
  transferId,
  utf8Length,
  WINDOW,
} from "./transferModel";

/** Without a place to keep partial downloads, a file is held in memory up to this size. */
const MEMORY_LIMIT = 512 * 1024 * 1024;

type Props = {
  channel: RTCDataChannel;
  rights: Transfer;
  language: Language;
};

type Upload = {
  id: string;
  file: File;
  next: number;
  acked: number;
  sending: boolean;
  ended: boolean;
};

type Download = {
  id: string;
  entry: Entry;
  received: number;
  unacked: number;
  /** Writes go one after another, in order. */
  writing: Promise<void>;
  writable: FileSystemWritableFileStream | null;
  partial: {
    folder: FileSystemDirectoryHandle;
    handle: FileSystemFileHandle;
  } | null;
  parts: Uint8Array[];
};

type Listing = Extract<Reply, { type: "listing" }>;

function joined(folder: string, name: string) {
  const separator = folder.includes("\\") ? "\\" : "/";
  return folder.endsWith(separator)
    ? `${folder}${name}`
    : `${folder}${separator}${name}`;
}

async function partialFile(id: string) {
  if (
    !navigator.storage?.getDirectory ||
    typeof FileSystemFileHandle === "undefined" ||
    !("createWritable" in FileSystemFileHandle.prototype)
  )
    return null;
  try {
    const folder = await navigator.storage.getDirectory();
    const handle = await folder.getFileHandle(`${id}.part`, { create: true });
    return { folder, handle };
  } catch {
    return null;
  }
}

function save(blob: Blob, name: string) {
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  link.download = name;
  link.click();
  setTimeout(() => URL.revokeObjectURL(url), 60_000);
}

/** Clipboard and file transfer over the session's `transfer` channel, as the grant allows. */
export function TransferPanel({ channel, rights, language }: Props) {
  const c = transferCopy[language];
  const locale = language === "hu" ? "hu-HU" : "en-GB";
  const id = useId();
  const [outgoing, setOutgoing] = useState("");
  const [incoming, setIncoming] = useState<string | null>(null);
  const [message, setMessage] = useState("");
  const [open, setOpen] = useState(channel.readyState === "open");
  const [progress, setProgress] = useState<string | null>(null);
  const [listing, setListing] = useState<Listing | null>(null);
  const upload = useRef<Upload | null>(null);
  const download = useRef<Download | null>(null);
  const [busy, setBusy] = useState(false);
  const sender = useRef<(value: object | Uint8Array) => boolean>(() => false);

  useEffect(() => {
    channel.binaryType = "arraybuffer";
    channel.bufferedAmountLowThreshold = CHUNK * 8;
    const send = (value: object | Uint8Array) => {
      if (channel.readyState !== "open") return false;
      if (value instanceof Uint8Array)
        channel.send(value as Uint8Array<ArrayBuffer>);
      else channel.send(JSON.stringify(value));
      return true;
    };
    const drained = () =>
      new Promise<void>((resolve) =>
        channel.addEventListener("bufferedamountlow", () => resolve(), {
          once: true,
        }),
      );
    const pump = async () => {
      const current = upload.current;
      if (!current || current.sending) return;
      current.sending = true;
      try {
        const total = current.file.size;
        while (
          current.next < total &&
          current.next - current.acked < WINDOW &&
          channel.readyState === "open"
        ) {
          if (channel.bufferedAmount > WINDOW) {
            await drained();
            continue;
          }
          const end = Math.min(current.next + CHUNK, total);
          const bytes = new Uint8Array(
            await current.file.slice(current.next, end).arrayBuffer(),
          );
          if (!send(dataMessage(current.id, current.next, bytes))) return;
          current.next = end;
        }
        if (current.acked === total && !current.ended) {
          current.ended = true;
          send({ type: "upload_end", id: current.id });
        }
      } finally {
        current.sending = false;
      }
    };
    const finish = async (done: Download, sha256: string, total: number) => {
      download.current = null;
      await done.writing;
      if (done.received !== total) {
        setMessage(c.problem("invalid"));
        return;
      }
      let blob: Blob;
      const { partial } = done;
      if (done.writable && partial) {
        await done.writable.close();
        blob = await partial.handle.getFile();
        // The browser reads the file while it saves it; the stored part goes afterwards.
        setTimeout(
          () =>
            void partial.folder.removeEntry(`${done.id}.part`).catch(() => {}),
          120_000,
        );
      } else blob = new Blob(done.parts as BlobPart[]);
      save(blob, done.entry.name);
      setProgress(null);
      setMessage(
        `${c.downloaded(done.entry.name)} · SHA-256 ${sha256.slice(0, 12)}…`,
      );
    };
    const reply = (value: Reply) => {
      switch (value.type) {
        case "clipboard":
          setIncoming(value.text);
          setMessage(c.clipboardReceived);
          break;
        case "clipboard_set":
          setMessage(c.clipboardSent);
          break;
        case "upload_ready":
        case "upload_ack": {
          const current = upload.current;
          if (!current || current.id !== value.id) return;
          if (value.type === "upload_ready") current.next = value.offset;
          current.acked = value.offset;
          setProgress(
            c.uploading(
              current.file.name,
              size(value.offset, locale),
              size(current.file.size, locale),
            ),
          );
          void pump();
          break;
        }
        case "upload_done":
          if (upload.current?.id !== value.id) return;
          upload.current = null;
          setBusy(false);
          setProgress(null);
          setMessage(
            `${c.uploaded(value.name)} · SHA-256 ${value.sha256.slice(0, 12)}…`,
          );
          break;
        case "listing":
          setListing(value);
          break;
        case "download_done": {
          const current = download.current;
          if (!current || current.id !== value.id) return;
          void finish(current, value.sha256, value.size).finally(() =>
            setBusy(false),
          );
          break;
        }
        case "failed":
          if (value.id) {
            if (upload.current?.id === value.id) upload.current = null;
            if (download.current?.id === value.id) {
              void download.current.writable?.close().catch(() => {});
              download.current = null;
            }
            setBusy(false);
            setProgress(null);
          }
          setMessage(c.problem(value.problem));
          break;
      }
    };
    const data = (buffer: ArrayBuffer) => {
      const value = readData(buffer);
      const current = download.current;
      if (!value || !current || value.id !== current.id) return;
      if (value.offset !== current.received) {
        send({ type: "cancel", id: current.id });
        reply({
          type: "failed",
          id: current.id,
          operation: null,
          problem: "invalid",
        });
        return;
      }
      const { writable } = current;
      if (writable)
        current.writing = current.writing.then(() =>
          writable.write({
            type: "write",
            position: value.offset,
            data: value.bytes as Uint8Array<ArrayBuffer>,
          }),
        );
      else current.parts.push(value.bytes);
      current.received += value.bytes.length;
      current.unacked += value.bytes.length;
      if (current.unacked >= ACK_STEP) {
        current.unacked = 0;
        const offset = current.received;
        void current.writing.then(() =>
          send({ type: "ack", id: current.id, offset }),
        );
      }
      setProgress(
        c.downloading(
          current.entry.name,
          size(current.received, locale),
          size(current.entry.size, locale),
        ),
      );
    };
    const received = (event: MessageEvent) => {
      if (typeof event.data === "string") {
        const value = readReply(event.data);
        if (value) reply(value);
      } else if (event.data instanceof ArrayBuffer) data(event.data);
    };
    const opened = () => {
      setOpen(true);
      if (rights.download) send({ type: "list" });
    };
    // What arrived is kept, so a later connection resumes from it.
    const closed = () => {
      setOpen(false);
      setBusy(false);
      setProgress(null);
      const current = download.current;
      download.current = null;
      upload.current = null;
      if (current?.writable)
        void current.writing
          .then(() => current.writable?.close())
          .catch(() => {});
      setMessage((shown) => (shown ? `${shown} ${c.closed}` : c.closed));
    };
    channel.addEventListener("message", received);
    channel.addEventListener("open", opened);
    channel.addEventListener("close", closed);
    if (channel.readyState === "open") opened();
    sender.current = send;
    return () => {
      channel.removeEventListener("message", received);
      channel.removeEventListener("open", opened);
      channel.removeEventListener("close", closed);
      closed();
    };
  }, [channel, rights.download, c, locale]);

  const sendClipboard = () => {
    if (utf8Length(outgoing) > CLIPBOARD_LIMIT) {
      setMessage(c.tooLong);
      return;
    }
    if (outgoing && sender.current({ type: "clipboard_set", text: outgoing }))
      setMessage("");
  };

  const choose = async (file: File | undefined) => {
    if (!file || upload.current) return;
    const transfer = await transferId(file.name, file.size, file.lastModified);
    upload.current = {
      id: transfer,
      file,
      next: 0,
      acked: 0,
      sending: false,
      ended: false,
    };
    setBusy(true);
    setMessage("");
    if (
      !sender.current({
        type: "upload",
        id: transfer,
        name: file.name,
        size: file.size,
      })
    ) {
      upload.current = null;
      setBusy(false);
    }
  };

  const fetchFile = async (entry: Entry) => {
    if (!listing || download.current) return;
    const path = joined(listing.path, entry.name);
    const transfer = await transferId(path, entry.size, entry.modified);
    const partial = await partialFile(transfer);
    let offset = 0;
    let writable: FileSystemWritableFileStream | null = null;
    if (partial) {
      const existing = (await partial.handle.getFile()).size;
      writable = await partial.handle.createWritable({
        keepExistingData: existing <= entry.size,
      });
      offset = existing <= entry.size ? existing : 0;
    } else if (entry.size > MEMORY_LIMIT) {
      setMessage(c.tooLargeHere);
      return;
    } else setMessage(c.memoryOnly);
    download.current = {
      id: transfer,
      entry,
      received: offset,
      unacked: 0,
      writing: Promise.resolve(),
      writable,
      partial,
      parts: [],
    };
    setBusy(true);
    sender.current({
      type: "download",
      id: transfer,
      path,
      size: entry.size,
      modified: entry.modified,
      offset,
    });
  };

  const list = (path: string | null) =>
    sender.current(path === null ? { type: "list" } : { type: "list", path });

  return (
    <section className="remote-transfer" aria-labelledby={`${id}-title`}>
      <h4 id={`${id}-title`}>{c.title}</h4>
      <p role="status" aria-live="polite" className="remote-transfer-status">
        {progress ?? message}
      </p>
      {rights.clipboard && (
        <fieldset className="remote-transfer-clipboard">
          <legend>{c.clipboard}</legend>
          <label htmlFor={`${id}-out`}>{c.outgoing}</label>
          <textarea
            id={`${id}-out`}
            rows={3}
            value={outgoing}
            disabled={!open}
            onChange={(event) => setOutgoing(event.target.value)}
          />
          <div className="remote-transfer-actions">
            <button
              type="button"
              className="button"
              disabled={!open || outgoing === ""}
              onClick={sendClipboard}
            >
              {c.sendClipboard}
            </button>
            <button
              type="button"
              className="button"
              disabled={!open}
              onClick={() => sender.current({ type: "clipboard_get" })}
            >
              {c.getClipboard}
            </button>
          </div>
          {incoming !== null && (
            <>
              <label htmlFor={`${id}-in`}>{c.incoming}</label>
              <textarea id={`${id}-in`} rows={3} value={incoming} readOnly />
              <button
                type="button"
                className="button"
                onClick={() =>
                  void navigator.clipboard
                    ?.writeText(incoming)
                    .then(() => setMessage(c.copied))
                    .catch(() => {})
                }
              >
                {c.copy}
              </button>
            </>
          )}
        </fieldset>
      )}
      {rights.upload && (
        <fieldset className="remote-transfer-upload">
          <legend>{c.upload}</legend>
          <label htmlFor={`${id}-file`}>{c.choose}</label>
          <input
            id={`${id}-file`}
            type="file"
            disabled={!open || busy}
            aria-describedby={`${id}-upload-hint`}
            onChange={(event) => {
              void choose(event.target.files?.[0]);
              event.target.value = "";
            }}
          />
          <p id={`${id}-upload-hint`} className="muted">
            {c.uploadHint}
          </p>
        </fieldset>
      )}
      {rights.download && (
        <fieldset className="remote-transfer-download">
          <legend>{c.download}</legend>
          {listing && (
            <>
              <p className="remote-transfer-path">
                <code>{listing.path}</code>{" "}
                {listing.parent && (
                  <button
                    type="button"
                    className="button"
                    disabled={!open}
                    onClick={() => list(listing.parent)}
                  >
                    {c.up}
                  </button>
                )}
              </p>
              <div className="table-scroll">
                <table className="device-table">
                  <thead>
                    <tr>
                      <th scope="col">{c.name}</th>
                      <th scope="col">{c.size}</th>
                      <th scope="col">{c.modified}</th>
                      <th scope="col">
                        <span className="sr-only">{c.fetch}</span>
                      </th>
                    </tr>
                  </thead>
                  <tbody>
                    {listing.entries.map((entry) => (
                      <tr key={entry.name}>
                        <td>
                          {entry.dir ? (
                            <button
                              type="button"
                              className="button-link"
                              disabled={!open}
                              onClick={() =>
                                list(joined(listing.path, entry.name))
                              }
                            >
                              {entry.name}
                            </button>
                          ) : (
                            entry.name
                          )}
                        </td>
                        <td>
                          {entry.dir ? c.folder : size(entry.size, locale)}
                        </td>
                        <td>
                          {new Date(entry.modified * 1000).toLocaleString(
                            locale,
                          )}
                        </td>
                        <td>
                          {!entry.dir && (
                            <button
                              type="button"
                              className="button"
                              disabled={!open || busy}
                              onClick={() => void fetchFile(entry)}
                            >
                              {c.fetch}
                            </button>
                          )}
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
              {listing.truncated && <p className="muted">{c.truncated}</p>}
            </>
          )}
          {!listing && (
            <button
              type="button"
              className="button"
              disabled={!open}
              onClick={() => list(null)}
            >
              {c.browse}
            </button>
          )}
        </fieldset>
      )}
    </section>
  );
}
