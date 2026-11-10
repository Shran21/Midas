import { useEffect, useId, useLayoutEffect, useRef, useState } from "react";
import { ApiError, api, type DeviceSummary, type RemoteSessions } from "../api";
import { Icon } from "../Icon";
import type { Language } from "../i18n";
import { sessionCopy } from "./copy";
import {
  fresh,
  OBSERVATION_LIFETIME,
  readSessions,
  selectionKey,
  unavailableReason,
} from "./model";
import { UnattendedCheck } from "./UnattendedCheck";

type Props = {
  tenant: string;
  device: DeviceSummary;
  csrf: string;
  language: Language;
  unavailable: boolean;
  fail: (error: unknown) => boolean;
  confirmIdentity: (force?: boolean) => Promise<string>;
};

type State =
  | { kind: "idle" | "loading" | "expired" }
  | { kind: "error"; denied: boolean }
  | {
      kind: "ready";
      catalogue: RemoteSessions;
      started: number;
      fetched: number;
      selected: string | null;
    };

export function SessionPicker(props: Props) {
  const id = useId();
  const c = sessionCopy[props.language];
  const reason = unavailableReason(props.device, props.unavailable);
  return (
    <section className="panel remote-sessions" aria-labelledby={`${id}-title`}>
      <div className="panel-heading">
        <div className="panel-title">
          <Icon name="remote" />
          <h2 id={`${id}-title`}>{c.title}</h2>
        </div>
      </div>
      <div className="remote-content">
        <p className="muted">{c.intro}</p>
        {reason ? (
          <p className="notice" role="status">
            {c[reason]}
          </p>
        ) : (
          <Picker
            key={`${props.tenant}/${props.device.id}/${props.csrf}`}
            {...props}
          />
        )}
        <p className="remote-pending">
          <Icon name="info" size={16} />
          {c.connectionPending}
        </p>
      </div>
    </section>
  );
}

function Picker({
  tenant,
  device,
  csrf,
  language,
  fail,
  confirmIdentity,
}: Props) {
  const c = sessionCopy[language];
  const id = useId();
  const [state, setState] = useState<State>({ kind: "idle" });
  const request = useRef<AbortController | null>(null);
  const requestTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const refreshButton = useRef<HTMLButtonElement>(null);
  const selectionArea = useRef<HTMLDivElement>(null);
  const selectionFocused = useRef(false);
  const ready =
    state.kind === "ready" && fresh(state.started, performance.now())
      ? state
      : null;
  const selected = ready?.catalogue.sessions.find(
    (session) => selectionKey(session.selection) === ready.selected,
  );
  const started = state.kind === "ready" ? state.started : null;
  const isReady = ready !== null;

  useLayoutEffect(() => {
    if (!isReady && selectionFocused.current) {
      selectionFocused.current = false;
      refreshButton.current?.focus();
    }
  }, [isReady]);

  useEffect(() => {
    const clear = () => {
      request.current?.abort();
      request.current = null;
      if (requestTimer.current !== null) clearTimeout(requestTimer.current);
    };
    const hide = () => {
      if (document.visibilityState !== "visible") {
        clear();
        setState({ kind: "expired" });
      }
    };
    document.addEventListener("visibilitychange", hide);
    return () => {
      clear();
      document.removeEventListener("visibilitychange", hide);
    };
  }, []);

  useEffect(() => {
    if (started === null) return;
    const timer = setTimeout(
      () => {
        if (selectionArea.current?.contains(document.activeElement))
          refreshButton.current?.focus();
        setState((previous) =>
          previous.kind === "ready" && previous.started === started
            ? { kind: "expired" }
            : previous,
        );
      },
      Math.max(0, OBSERVATION_LIFETIME - (performance.now() - started)),
    );
    return () => clearTimeout(timer);
  }, [started]);

  const load = async () => {
    if (request.current || document.visibilityState !== "visible") return;
    const controller = new AbortController();
    request.current = controller;
    const began = performance.now();
    setState({ kind: "loading" });
    const timeout = setTimeout(() => {
      if (request.current !== controller) return;
      request.current = null;
      controller.abort();
      setState({ kind: "error", denied: false });
    }, 12_000);
    requestTimer.current = timeout;
    try {
      const response = await api.remoteSessions(
        tenant,
        device.id,
        csrf,
        controller.signal,
      );
      if (request.current !== controller) return;
      const catalogue = readSessions(response);
      if (!catalogue) throw new Error("invalid session list");
      if (!fresh(began, performance.now())) setState({ kind: "expired" });
      else
        setState({
          kind: "ready",
          catalogue,
          started: began,
          fetched: Date.now(),
          selected: null,
        });
    } catch (error) {
      if (request.current === controller && !fail(error)) {
        setState({
          kind: "error",
          denied:
            error instanceof ApiError && [403, 404].includes(error.status),
        });
      }
    } finally {
      clearTimeout(timeout);
      if (request.current === controller) request.current = null;
    }
  };

  const choose = (key: string) =>
    setState((previous) => {
      if (
        previous.kind !== "ready" ||
        !fresh(previous.started, performance.now())
      )
        return { kind: "expired" };
      const target = previous.catalogue.sessions.find(
        (session) => selectionKey(session.selection) === key,
      );
      return target && target.presence !== "unknown"
        ? { ...previous, selected: key }
        : previous;
    });

  return (
    <>
      <div className="remote-toolbar">
        <button
          type="button"
          ref={refreshButton}
          className="button primary"
          disabled={state.kind === "loading"}
          onClick={() => void load()}
        >
          <Icon name="refresh" size={16} />
          {state.kind === "loading"
            ? c.loading
            : state.kind === "idle"
              ? c.fetch
              : c.refresh}
        </button>
        {ready && (
          <span className="muted">
            {c.refreshed}:{" "}
            {new Intl.DateTimeFormat(language === "hu" ? "hu-HU" : "en-GB", {
              hour: "2-digit",
              minute: "2-digit",
              second: "2-digit",
            }).format(ready.fetched)}
          </span>
        )}
      </div>
      <div role="status" aria-live="polite">
        {state.kind === "idle" && <p className="muted">{c.idle}</p>}
        {state.kind === "loading" && (
          <p className="loading-line">{c.loading}</p>
        )}
        {(state.kind === "expired" || (state.kind === "ready" && !ready)) && (
          <p className="notice">{c.expired}</p>
        )}
      </div>
      {state.kind === "error" && (
        <p role="alert" className="form-error">
          {state.denied ? c.denied : c.failed}
        </p>
      )}
      {ready && ready.catalogue.sessions.length === 0 && (
        <div className="remote-empty" role="status">
          <strong>{c.noSessions}</strong>
          <p className="muted">{c.noSessionsHint}</p>
        </div>
      )}
      {ready && ready.catalogue.sessions.length > 0 && (
        <div
          ref={selectionArea}
          className="remote-target"
          onFocusCapture={() => {
            selectionFocused.current = true;
          }}
          onBlurCapture={(event) => {
            if (!event.currentTarget.contains(event.relatedTarget))
              selectionFocused.current = false;
          }}
        >
          <fieldset className="remote-list" aria-describedby={`${id}-validity`}>
            <legend>{c.choose}</legend>
            {ready.catalogue.sessions.map((session) => {
              const key = selectionKey(session.selection);
              const unknown = session.presence === "unknown";
              const account =
                session.account ||
                (session.presence === "vacant"
                  ? c.noAccount
                  : c.accountUnknown);
              return (
                <label
                  key={key}
                  className={`remote-session ${ready.selected === key ? "selected" : ""} ${unknown ? "uncertain" : ""}`}
                >
                  <input
                    type="radio"
                    name={`${id}-session`}
                    value={key}
                    checked={ready.selected === key}
                    disabled={unknown}
                    onChange={() => choose(key)}
                  />
                  <span className="remote-session-content">
                    <span className="remote-session-heading">
                      <strong>
                        <bdi>{account}</bdi>
                      </strong>
                      <span className={`remote-presence ${session.presence}`}>
                        {c[session.presence]}
                      </span>
                    </span>
                    <span className="remote-session-meta">
                      <span>
                        {session.console
                          ? c.console
                          : session.station || c.session}
                      </span>
                      <span>
                        {c.session} {session.selection.os_session_id}
                      </span>
                      <span>
                        {session.locked === null
                          ? c.lockUnknown
                          : session.locked
                            ? c.locked
                            : c.unlocked}
                      </span>
                    </span>
                    {unknown && (
                      <span className="remote-uncertain-note">
                        {c.unknownHint}
                      </span>
                    )}
                  </span>
                </label>
              );
            })}
          </fieldset>
          <div className="remote-selection" role="status" aria-live="polite">
            <Icon name={selected ? "check" : "info"} size={20} />
            <div>
              <strong>
                {selected
                  ? `${c.selected}: ${c.session} ${selected.selection.os_session_id}`
                  : c.selectHint}
              </strong>
              {selected && (
                <p>
                  {selected.presence === "occupied"
                    ? c.consent
                    : device.kind === "server"
                      ? c.policy
                      : c.workstation}
                </p>
              )}
            </div>
          </div>
          <p id={`${id}-validity`} className="muted remote-validity">
            {c.validity}
          </p>
          {selected &&
            device.kind === "server" &&
            device.permissions.includes("remote.control") &&
            device.permissions.includes("remote.unattended") && (
              <UnattendedCheck
                key={`${selectionKey(selected.selection)}/${device.permissions.join(",")}`}
                tenant={tenant}
                device={device}
                session={selected.selection}
                csrf={csrf}
                language={language}
                fail={fail}
                confirmIdentity={confirmIdentity}
              />
            )}
        </div>
      )}
    </>
  );
}
