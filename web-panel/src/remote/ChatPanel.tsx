import { useEffect, useId, useRef, useState } from "react";
import { api } from "../api";
import type { Language } from "../i18n";
import {
  CHAT_LIMIT,
  type ChatMessage,
  chatValid,
  merge,
  readChat,
  readSent,
} from "./chatModel";

/** How often the conversation is read while the session runs. */
const POLL = 1500;

const copy = {
  hu: {
    title: "Üzenetek",
    empty:
      "Még nincs üzenet. A felhasználó a gépén, a távsegítés jelzőablakában válaszolhat.",
    you: "Te",
    user: (who: string) => who || "Felhasználó",
    message: "Üzenet a felhasználónak",
    send: "Küldés",
    tooLong: "Az üzenet legfeljebb 1000 karakter lehet.",
    failed:
      "Az üzenet nem ment el. A munkamenet lezárult, vagy túl gyorsan küldtél.",
  },
  en: {
    title: "Messages",
    empty:
      "No messages yet. The user can answer on their computer, in the remote-support indicator.",
    you: "You",
    user: (who: string) => who || "User",
    message: "Message to the user",
    send: "Send",
    tooLong: "A message can be at most 1000 characters.",
    failed:
      "The message was not sent. The session ended, or messages were sent too fast.",
  },
};

type Props = {
  tenant: string;
  device: string;
  request: string;
  /** The signed-in user's account, as the session list names it. */
  account: string;
  csrf: string;
  language: Language;
  fail: (error: unknown) => boolean;
};

/** The chat of an active session; it ends with the session. */
export function ChatPanel({
  tenant,
  device,
  request,
  account,
  csrf,
  language,
  fail,
}: Props) {
  const c = copy[language];
  const locale = language === "hu" ? "hu-HU" : "en-GB";
  const id = useId();
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [text, setText] = useState("");
  const [error, setError] = useState("");
  const [sending, setSending] = useState(false);
  const last = useRef(0);
  const latest = useRef({ fail });
  latest.current = { fail };

  useEffect(() => {
    let stopped = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const read = async () => {
      try {
        const page = readChat(
          await api.remoteChat(tenant, device, request, last.current),
        );
        if (stopped || !page) return;
        setMessages((known) => {
          const next = merge(known, page);
          last.current = next.at(-1)?.seq ?? last.current;
          return next;
        });
      } catch (caught) {
        if (stopped || latest.current.fail(caught)) return;
      }
      if (!stopped) timer = setTimeout(read, POLL);
    };
    void read();
    return () => {
      stopped = true;
      clearTimeout(timer);
    };
  }, [tenant, device, request]);

  const send = async () => {
    const message = text.trim();
    if (!chatValid(message)) {
      if ([...message].length > CHAT_LIMIT) setError(c.tooLong);
      return;
    }
    setSending(true);
    setError("");
    try {
      const sent = readSent(
        await api.sendRemoteChat(tenant, device, request, message, csrf),
      );
      if (sent) {
        setMessages((known) => {
          const next = merge(known, [sent]);
          last.current = Math.max(last.current, sent.seq);
          return next;
        });
        setText("");
      }
    } catch (caught) {
      if (!fail(caught)) setError(c.failed);
    } finally {
      setSending(false);
    }
  };

  return (
    <section className="remote-chat" aria-labelledby={`${id}-title`}>
      <h4 id={`${id}-title`}>{c.title}</h4>
      <ol className="remote-chat-messages" aria-live="polite">
        {messages.length === 0 && <li className="muted">{c.empty}</li>}
        {messages.map((message) => (
          <li key={message.seq} className={`remote-chat-${message.from}`}>
            <span className="remote-chat-who">
              {message.from === "operator" ? c.you : c.user(account)}
              {" · "}
              {new Date(message.at * 1000).toLocaleTimeString(locale, {
                hour: "2-digit",
                minute: "2-digit",
              })}
            </span>
            <span className="remote-chat-text">{message.text}</span>
          </li>
        ))}
      </ol>
      <form
        className="remote-chat-form"
        onSubmit={(event) => {
          event.preventDefault();
          void send();
        }}
      >
        <label htmlFor={`${id}-text`}>{c.message}</label>
        <textarea
          id={`${id}-text`}
          rows={2}
          value={text}
          maxLength={CHAT_LIMIT}
          onChange={(event) => setText(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter" && !event.shiftKey) {
              event.preventDefault();
              void send();
            }
          }}
        />
        <button
          type="submit"
          className="button"
          disabled={sending || text.trim() === ""}
        >
          {c.send}
        </button>
      </form>
      {error && (
        <p role="alert" className="form-error">
          {error}
        </p>
      )}
    </section>
  );
}
