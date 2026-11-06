import { useCallback, useEffect, useRef, useState } from "react";
import type { Language } from "../i18n";
import type { Preferences } from "../preferences";
import { PreferenceControls } from "../SignIn";
import {
  type Event,
  type Mode,
  newRequest,
  reduce,
  type State,
  secondsLeft,
} from "./model";

const text = {
  hu: {
    title: "Használhatósági prototípus",
    note: "Mintaadatok, valódi távkapcsolat nélkül. A két nézetet két külön ablakban nyisd meg.",
    operator: "Kezelő (adminpanel)",
    consent: "Helyi felhasználó (jóváhagyó ablak)",
    openOther: "Másik nézet megnyitása új ablakban",
    customer: "Ügyfél",
    device: "Gép",
    sample: "Mintaadat",
    requestNormal: "Távoli vezérlés kérése",
    requestAdmin: "Rendszergazdai távvezérlés kérése",
    dialogTitle: "Kérés küldése",
    session: "Windows-munkamenet",
    sessionHint:
      "Több felhasználó van bejelentkezve; válaszd ki, kinek az asztalához kapcsolódsz.",
    modeNormal: "Normál távvezérlés",
    modeAdmin: "Rendszergazdai távvezérlés",
    willSee:
      "A felhasználó a nevedet, a szervezetet és a kért műveleteket látja, és ő dönt.",
    send: "Kérés küldése",
    cancel: "Mégse",
    idle: "Nincs aktív kérés.",
    waiting: (who: string, s: number) =>
      `Várakozás ${who} jóváhagyására. A kérés ${s} másodperc múlva lejár.`,
    declined: (who: string) =>
      `${who} elutasította a kérést. Nem jött létre kapcsolat.`,
    expired: (who: string) =>
      `${who} nem válaszolt, a kérés lejárt. Nem jött létre kapcsolat.`,
    connected: (who: string, mode: string, t: string) =>
      `Kapcsolódva: ${who} asztala · ${mode} · ${t}`,
    endedUser: (who: string) => `${who} befejezte a kapcsolatot.`,
    endedOperator: "Bontottad a kapcsolatot.",
    newRequest: "Új kérés",
    disconnect: "Kapcsolat bontása",
    activeMode: "Aktív mód",
    noRequest:
      "Nincs kapcsolódási kérés. Ha egy technikus kapcsolódni szeretne, itt kérjük az engedélyedet.",
    wants: (who: string) => `${who} kapcsolódni szeretne a gépedhez`,
    organisation: "Norda IT-szolgáltatás",
    asks: "Ha engedélyezed, a technikus:",
    normalItems: [
      "látja a képernyődet",
      "használhatja az egeret és a billentyűzetet",
      "a programokat a te jogosultságoddal indítja",
    ],
    adminItems: [
      "kezelheti a Windows rendszergazdai engedélykéréseit (UAC)",
      "a cége által engedélyezett rendszergazdai műveleteket futtathat",
    ],
    expires: (s: number) =>
      `A kérés ${s} másodperc múlva lejár. Lejáratkor nem jön létre kapcsolat.`,
    allow: "Engedélyezés",
    decline: "Elutasítás",
    activeBanner: (who: string, mode: string) =>
      `${who} most látja és vezérli a képernyődet (${mode}).`,
    anytime: "Bármikor befejezheted a kapcsolatot.",
    end: "Befejezés",
    youDeclined: "Elutasítottad a kérést. Nem jött létre kapcsolat.",
    youExpired: "A kérés lejárt. Nem jött létre kapcsolat.",
    youEnded: "A kapcsolat véget ért.",
    operatorEnded: "A technikus bontotta a kapcsolatot.",
    log: "Megfigyelői napló",
    reset: "Alaphelyzet (mindkét nézet)",
    download: "Napló letöltése",
  },
  en: {
    title: "Usability prototype",
    note: "Sample data, no real remote session. Open the two views in separate windows.",
    operator: "Operator (admin panel)",
    consent: "Local user (consent window)",
    openOther: "Open the other view in a new window",
    customer: "Customer",
    device: "Device",
    sample: "Sample data",
    requestNormal: "Request remote control",
    requestAdmin: "Request administrative remote control",
    dialogTitle: "Send request",
    session: "Windows session",
    sessionHint:
      "Several users are signed in; choose whose desktop you connect to.",
    modeNormal: "Normal remote control",
    modeAdmin: "Administrative remote control",
    willSee:
      "The user sees your name, organisation and the requested actions, and decides.",
    send: "Send request",
    cancel: "Cancel",
    idle: "No active request.",
    waiting: (who: string, s: number) =>
      `Waiting for ${who} to answer. The request expires in ${s} seconds.`,
    declined: (who: string) =>
      `${who} declined the request. No connection was made.`,
    expired: (who: string) =>
      `${who} did not answer and the request expired. No connection was made.`,
    connected: (who: string, mode: string, t: string) =>
      `Connected: ${who}'s desktop · ${mode} · ${t}`,
    endedUser: (who: string) => `${who} ended the connection.`,
    endedOperator: "You ended the connection.",
    newRequest: "New request",
    disconnect: "End connection",
    activeMode: "Active mode",
    noRequest:
      "No connection request. When a technician wants to connect, you will be asked here.",
    wants: (who: string) => `${who} wants to connect to your computer`,
    organisation: "Norda IT Services",
    asks: "If you allow it, the technician can:",
    normalItems: [
      "see your screen",
      "use the mouse and keyboard",
      "start programs with your permissions",
    ],
    adminItems: [
      "handle Windows administrator permission prompts (UAC)",
      "run administrative actions approved by their company",
    ],
    expires: (s: number) =>
      `The request expires in ${s} seconds. No connection is made when it expires.`,
    allow: "Allow",
    decline: "Decline",
    activeBanner: (who: string, mode: string) =>
      `${who} can now see and control your screen (${mode}).`,
    anytime: "You can end the connection at any time.",
    end: "End",
    youDeclined: "You declined the request. No connection was made.",
    youExpired: "The request expired. No connection was made.",
    youEnded: "The connection has ended.",
    operatorEnded: "The technician ended the connection.",
    log: "Facilitator log",
    reset: "Reset (both views)",
    download: "Download log",
  },
};

const TECHNICIAN = "Kovács Béla";
const SESSIONS = [
  {
    id: "console",
    user: "Kiss Anna",
    hu: "Kiss Anna — konzol",
    en: "Kiss Anna — console",
  },
  {
    id: "rdp",
    user: "Nagy Péter",
    hu: "Nagy Péter — távoli asztal (RDP)",
    en: "Nagy Péter — remote desktop (RDP)",
  },
];

type Message =
  | { type: "event"; event: Event }
  | { type: "hello" }
  | { type: "snapshot"; state: State };
type LogEntry = { at: string; role: string; event: string };

function useShared(role: string) {
  const [state, setState] = useState<State>({ kind: "idle" });
  const [log, setLog] = useState<LogEntry[]>([]);
  const channel = useRef<BroadcastChannel | null>(null);
  const current = useRef(state);
  current.current = state;
  const record = useCallback(
    (source: string, event: Event) =>
      setLog((entries) => [
        ...entries,
        {
          at: new Date().toISOString(),
          role: source,
          event: JSON.stringify(event),
        },
      ]),
    [],
  );
  useEffect(() => {
    const bus = new BroadcastChannel("midas-usability-v1");
    channel.current = bus;
    bus.onmessage = ({ data }: MessageEvent<Message>) => {
      if (data.type === "event") {
        setState((s) => reduce(s, data.event));
        record("remote", data.event);
      } else if (data.type === "hello") {
        bus.postMessage({
          type: "snapshot",
          state: current.current,
        } satisfies Message);
      } else if (data.type === "snapshot" && current.current.kind === "idle") {
        setState(data.state);
      }
    };
    bus.postMessage({ type: "hello" } satisfies Message);
    return () => bus.close();
  }, [record]);
  const dispatch = useCallback(
    (event: Event) => {
      setState((s) => reduce(s, event));
      record(role, event);
      channel.current?.postMessage({ type: "event", event } satisfies Message);
    },
    [record, role],
  );
  return { state, dispatch, log };
}

function useNow() {
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), 500);
    return () => clearInterval(timer);
  }, []);
  return now;
}

const clock = (ms: number) => {
  const s = Math.max(0, Math.floor(ms / 1000));
  return `${String(Math.floor(s / 60)).padStart(2, "0")}:${String(s % 60).padStart(2, "0")}`;
};

function Operator({
  language,
  state,
  dispatch,
  now,
}: {
  language: Language;
  state: State;
  dispatch: (e: Event) => void;
  now: number;
}) {
  const t = text[language];
  const dialog = useRef<HTMLDialogElement>(null);
  const opener = useRef<HTMLElement | null>(null);
  const [mode, setMode] = useState<Mode>("normal");
  const [session, setSession] = useState("");
  const who =
    SESSIONS.find(
      (s) => s.id === (state.kind === "idle" ? session : state.request.session),
    )?.user ?? "";
  const modeName = (m: Mode) => (m === "admin" ? t.modeAdmin : t.modeNormal);
  const open = (next: Mode, button: HTMLElement) => {
    opener.current = button;
    setMode(next);
    setSession("");
    dialog.current?.showModal();
  };
  const close = () => {
    dialog.current?.close();
  };
  const busy = state.kind === "waiting" || state.kind === "connected";
  let status: string;
  switch (state.kind) {
    case "idle":
      status = t.idle;
      break;
    case "waiting":
      status = t.waiting(who, secondsLeft(state, now));
      break;
    case "declined":
      status = t.declined(who);
      break;
    case "expired":
      status = t.expired(who);
      break;
    case "connected":
      status = t.connected(
        who,
        modeName(state.request.mode),
        clock(now - state.since),
      );
      break;
    case "ended":
      status = state.by === "user" ? t.endedUser(who) : t.endedOperator;
      break;
  }
  return (
    <section className="panel usability-panel" aria-labelledby="operator-title">
      <h2 id="operator-title">{t.operator}</h2>
      <dl className="usability-target">
        <dt>{t.customer}</dt>
        <dd>Norda Kft.</dd>
        <dt>{t.device}</dt>
        <dd>WS-BUD-014 · Windows 11 Pro</dd>
      </dl>
      <p className="sample-label">{t.sample}</p>
      <div className="usability-actions">
        <button
          type="button"
          className="button primary"
          disabled={busy}
          onClick={(e) => open("normal", e.currentTarget)}
        >
          {t.requestNormal}
        </button>
        <button
          type="button"
          className="button secondary"
          disabled={busy}
          onClick={(e) => open("admin", e.currentTarget)}
        >
          {t.requestAdmin}
        </button>
      </div>
      <p className="usability-status" role="status" aria-live="polite">
        {status}
      </p>
      {state.kind === "connected" && (
        <div className="usability-actions">
          <span className="count-badge">
            {t.activeMode}: {modeName(state.request.mode)}
          </span>
          <button
            type="button"
            className="button secondary"
            onClick={() =>
              dispatch({
                type: "end",
                id: state.request.id,
                by: "operator",
                at: Date.now(),
              })
            }
          >
            {t.disconnect}
          </button>
        </div>
      )}
      <dialog
        ref={dialog}
        className="device-dialog usability-dialog"
        aria-labelledby="request-title"
        onClose={() => {
          const target = opener.current;
          if (target?.isConnected) requestAnimationFrame(() => target.focus());
        }}
      >
        <form
          method="dialog"
          className="usability-form"
          onSubmit={(event) => {
            event.preventDefault();
            if (!session) return;
            dispatch({
              type: "request",
              request: newRequest(
                mode,
                session,
                Date.now(),
                crypto.randomUUID(),
              ),
            });
            close();
          }}
        >
          <h2 id="request-title">
            {t.dialogTitle}: {modeName(mode)}
          </h2>
          <fieldset className="usability-sessions">
            <legend>{t.session}</legend>
            <p className="field-hint">{t.sessionHint}</p>
            {SESSIONS.map((s) => (
              <label key={s.id}>
                <input
                  type="radio"
                  name="session"
                  value={s.id}
                  checked={session === s.id}
                  onChange={() => setSession(s.id)}
                  required
                />
                {s[language]}
              </label>
            ))}
          </fieldset>
          <p className="field-hint">{t.willSee}</p>
          <div className="usability-actions">
            <button
              type="submit"
              className="button primary"
              disabled={!session}
            >
              {t.send}
            </button>
            <button type="button" className="button secondary" onClick={close}>
              {t.cancel}
            </button>
          </div>
        </form>
      </dialog>
    </section>
  );
}

function Consent({
  language,
  state,
  dispatch,
  now,
}: {
  language: Language;
  state: State;
  dispatch: (e: Event) => void;
  now: number;
}) {
  const t = text[language];
  const decline = useRef<HTMLButtonElement>(null);
  const end = useRef<HTMLButtonElement>(null);
  const [seen, setSeen] = useState<{ id: string; at: number } | null>(null);
  const modeName = (m: Mode) => (m === "admin" ? t.modeAdmin : t.modeNormal);
  const pending = state.kind === "waiting" ? state.request.id : null;
  useEffect(() => {
    if (pending) {
      setSeen({ id: pending, at: Date.now() });
      decline.current?.focus();
    }
  }, [pending]);
  useEffect(() => {
    if (state.kind === "connected") end.current?.focus();
  }, [state.kind]);
  // Allowing needs a deliberate action: a key press carried over from other work cannot approve instantly.
  const armed = seen !== null && seen.id === pending && now - seen.at >= 1000;
  if (state.kind === "waiting") {
    const items =
      state.request.mode === "admin"
        ? [...t.normalItems, ...t.adminItems]
        : t.normalItems;
    return (
      <section
        className="panel usability-panel consent-window"
        role="alertdialog"
        aria-modal="false"
        aria-labelledby="consent-title"
        aria-describedby="consent-body"
      >
        <h2 id="consent-title">{t.wants(TECHNICIAN)}</h2>
        <div id="consent-body">
          <p>
            <strong>{t.organisation}</strong> · {modeName(state.request.mode)}
          </p>
          <p>{t.asks}</p>
          <ul>
            {items.map((item) => (
              <li key={item}>{item}</li>
            ))}
          </ul>
          <p className="field-hint">{t.expires(secondsLeft(state, now))}</p>
        </div>
        <div className="usability-actions">
          <button
            ref={decline}
            type="button"
            className="button secondary"
            onClick={() =>
              dispatch({
                type: "decline",
                id: state.request.id,
                at: Date.now(),
              })
            }
          >
            {t.decline}
          </button>
          <button
            type="button"
            className="button primary"
            disabled={!armed}
            onClick={() =>
              dispatch({ type: "allow", id: state.request.id, at: Date.now() })
            }
          >
            {t.allow}
          </button>
        </div>
      </section>
    );
  }
  let message = t.noRequest;
  if (state.kind === "declined") message = t.youDeclined;
  if (state.kind === "expired") message = t.youExpired;
  if (state.kind === "ended")
    message = state.by === "user" ? t.youEnded : t.operatorEnded;
  return (
    <section
      className="panel usability-panel consent-window"
      aria-labelledby="consent-idle-title"
    >
      <h2 id="consent-idle-title">MIDAS</h2>
      {state.kind === "connected" ? (
        <>
          <p className="notice" role="status">
            {t.activeBanner(TECHNICIAN, modeName(state.request.mode))}
          </p>
          <p>{t.anytime}</p>
          <button
            ref={end}
            type="button"
            className="button primary usability-end"
            onClick={() =>
              dispatch({
                type: "end",
                id: state.request.id,
                by: "user",
                at: Date.now(),
              })
            }
          >
            {t.end}
          </button>
        </>
      ) : (
        <p role="status">{message}</p>
      )}
    </section>
  );
}

export function Usability({ preferences }: { preferences: Preferences }) {
  const t = text[preferences.language];
  const [role, setRole] = useState(() =>
    location.hash === "#usability/consent" ? "consent" : "operator",
  );
  const { state, dispatch, log } = useShared(role);
  const now = useNow();
  useEffect(() => {
    const listener = () =>
      setRole(location.hash === "#usability/consent" ? "consent" : "operator");
    window.addEventListener("hashchange", listener);
    return () => window.removeEventListener("hashchange", listener);
  }, []);
  useEffect(() => {
    document.title = [
      "MIDAS",
      t.title,
      role === "consent" ? t.consent : t.operator,
    ].join(" · ");
  }, [t, role]);
  useEffect(() => {
    if (state.kind === "waiting" && now >= state.request.expiresAt)
      dispatch({ type: "expire", id: state.request.id, at: now });
  }, [state, now, dispatch]);
  const other =
    role === "consent" ? "#usability/operator" : "#usability/consent";
  return (
    <div className="signin-page">
      <header className="signin-top">
        <strong>MIDAS · {t.title}</strong>
        <PreferenceControls {...preferences} />
      </header>
      <main id="main" className="usability-main">
        <p className="preview-banner">{t.note}</p>
        <nav className="usability-roles" aria-label={t.title}>
          <a
            href="#usability/operator"
            aria-current={role === "operator" ? "page" : undefined}
          >
            {t.operator}
          </a>
          <a
            href="#usability/consent"
            aria-current={role === "consent" ? "page" : undefined}
          >
            {t.consent}
          </a>
          <button
            type="button"
            className="button secondary"
            onClick={() =>
              window.open(
                `${location.pathname}${other}`,
                "_blank",
                "width=520,height=720",
              )
            }
          >
            {t.openOther}
          </button>
        </nav>
        {role === "consent" ? (
          <Consent
            language={preferences.language}
            state={state}
            dispatch={dispatch}
            now={now}
          />
        ) : (
          <Operator
            language={preferences.language}
            state={state}
            dispatch={dispatch}
            now={now}
          />
        )}
        <details className="panel usability-log">
          <summary>{t.log}</summary>
          <button
            type="button"
            className="button secondary"
            onClick={() => dispatch({ type: "reset" })}
          >
            {t.reset}
          </button>
          <ol>
            {log.map((entry) => (
              <li key={`${entry.at}-${entry.event}`}>
                <code>
                  {entry.at} · {entry.role} · {entry.event}
                </code>
              </li>
            ))}
          </ol>
          <button
            type="button"
            className="button secondary"
            onClick={() => {
              const link = document.createElement("a");
              link.href = URL.createObjectURL(
                new Blob([JSON.stringify(log, null, 1)], {
                  type: "application/json",
                }),
              );
              link.download = `midas-usability-${role}-${Date.now()}.json`;
              link.click();
            }}
          >
            {t.download}
          </button>
        </details>
      </main>
    </div>
  );
}
