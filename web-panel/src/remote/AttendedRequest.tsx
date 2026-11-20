import { useEffect, useId, useRef, useState } from "react";
import {
  ApiError,
  api,
  type DeviceSummary,
  type SessionSelection,
} from "../api";
import type { Language } from "../i18n";
import {
  CONSENT_LIFETIME,
  finished,
  type RemoteRequest,
  readRequest,
} from "./model";
import { requestCopy } from "./requestCopy";
import { Viewer } from "./Viewer";

/** A request the panel follows; the server keeps its state, the panel only reads it. */
export type Started = {
  request: RemoteRequest;
  account: string;
  /** Local clock (performance.now) when the window or the grant ends. */
  ends: number | null;
};

type Problem = "fresh" | "busy" | "offline" | "denied" | "error";

function endsAt(request: RemoteRequest, received: number) {
  if (request.until === null) return null;
  const left = Math.min(
    Math.max(0, request.until * 1000 - Date.now()),
    CONSENT_LIFETIME,
  );
  return received + left;
}

function problem(error: unknown): Problem {
  if (!(error instanceof ApiError)) return "error";
  if (error.code === "fresh_passkey_required") return "fresh";
  if (error.code === "device_offline") return "offline";
  if (error.status === 409) return "busy";
  if ([403, 404].includes(error.status)) return "denied";
  return "error";
}

type ButtonsProps = {
  tenant: string;
  device: DeviceSummary;
  session: SessionSelection;
  account: string;
  csrf: string;
  language: Language;
  blocked: boolean;
  fail: (error: unknown) => boolean;
  confirmIdentity: (force?: boolean) => Promise<string>;
  onStarted: (started: Started) => void;
};

/** The two request buttons of UI-04, for a fresh selection of a signed-in session. */
export function RequestButtons({
  tenant,
  device,
  session,
  account,
  csrf,
  language,
  blocked,
  fail,
  confirmIdentity,
  onStarted,
}: ButtonsProps) {
  const c = requestCopy[language];
  const id = useId();
  const [uac, setUac] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<Problem | null>(null);
  const request = useRef<AbortController | null>(null);
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      request.current?.abort();
    };
  }, []);
  if (!device.capabilities.includes("remote.grants"))
    return (
      <p className="notice" role="status">
        {c.unsupported}
      </p>
    );
  const send = async (mode: "normal" | "administrative") => {
    if (request.current || busy || blocked) return;
    const controller = new AbortController();
    request.current = controller;
    setBusy(true);
    setError(null);
    const timeout = setTimeout(() => controller.abort(), 12_000);
    try {
      const response = await api.requestRemote(
        tenant,
        device.id,
        { mode, uac, session },
        csrf,
        controller.signal,
      );
      const received = performance.now();
      const value = readRequest(response);
      if (
        !value ||
        value.os_session_id !== session.os_session_id ||
        value.mode !== mode ||
        value.uac !== uac
      )
        throw new Error("invalid request");
      if (mounted.current)
        onStarted({ request: value, account, ends: endsAt(value, received) });
    } catch (caught) {
      if (mounted.current && !fail(caught)) setError(problem(caught));
    } finally {
      clearTimeout(timeout);
      request.current = null;
      if (mounted.current) setBusy(false);
    }
  };
  const authenticate = async () => {
    setBusy(true);
    try {
      await confirmIdentity(true);
      if (mounted.current) setError(null);
    } catch (caught) {
      if (mounted.current && !fail(caught)) setError("fresh");
    } finally {
      if (mounted.current) setBusy(false);
    }
  };
  const disabled = busy || blocked;
  return (
    <section className="remote-request" aria-labelledby={`${id}-title`}>
      <h3 id={`${id}-title`}>{c.title}</h3>
      <p className="muted">{c.intro}</p>
      {device.permissions.includes("remote.uac.interact") && (
        <label className="remote-check-uac">
          <input
            type="checkbox"
            checked={uac}
            disabled={disabled}
            onChange={(event) => {
              setUac(event.target.checked);
              setError(null);
            }}
          />
          {c.uac}
        </label>
      )}
      <div className="remote-request-actions">
        <button
          type="button"
          className="button primary"
          disabled={disabled}
          onClick={() => void send("normal")}
        >
          {busy ? c.sending : c.normal}
        </button>
        {device.permissions.includes("remote.admin") && (
          <button
            type="button"
            className="button"
            disabled={disabled}
            onClick={() => void send("administrative")}
          >
            {c.administrative}
          </button>
        )}
      </div>
      {error && (
        <p role="alert" className="form-error">
          {c[error]}
        </p>
      )}
      {error === "fresh" && (
        <button
          type="button"
          className="button"
          disabled={busy}
          onClick={() => void authenticate()}
        >
          {c.authenticate}
        </button>
      )}
    </section>
  );
}

type StatusProps = {
  tenant: string;
  device: DeviceSummary;
  csrf: string;
  language: Language;
  started: Started;
  fail: (error: unknown) => boolean;
  onChange: (started: Started) => void;
  onDismiss: () => void;
};

/** Follows one request until it ends; withdrawing or ending it is always one button away. */
export function RequestStatus({
  tenant,
  device,
  csrf,
  language,
  started,
  fail,
  onChange,
  onDismiss,
}: StatusProps) {
  const c = requestCopy[language];
  const { request, account } = started;
  const who = account || c.someone;
  const message = useRef<HTMLDivElement>(null);
  const [now, setNow] = useState(() => performance.now());
  const [stopping, setStopping] = useState(false);
  const state = request.state;
  const done = finished(state);
  const latest = useRef({ started, fail, onChange, csrf });
  latest.current = { started, fail, onChange, csrf };

  // Each new state is announced and takes the focus, so a vanished button never strands it.
  useEffect(() => {
    if (state) message.current?.focus();
  }, [state]);

  // Leaving the page withdraws a waiting request and ends an active one.
  useEffect(
    () => () => {
      const { started, csrf } = latest.current;
      if (!finished(started.request.state))
        void api
          .cancelRemoteRequest(tenant, device.id, started.request.id, csrf)
          .catch(() => undefined);
    },
    [tenant, device.id],
  );

  useEffect(() => {
    if (done) return;
    const controller = new AbortController();
    let timer: ReturnType<typeof setTimeout> | null = null;
    const poll = async () => {
      const { started } = latest.current;
      try {
        const response = await api.remoteRequest(
          tenant,
          device.id,
          started.request.id,
          controller.signal,
        );
        const received = performance.now();
        const value = readRequest(response, started.request);
        if (value && value.state !== state) {
          latest.current.onChange({
            ...started,
            request: value,
            ends: endsAt(value, received),
          });
          return;
        }
      } catch (caught) {
        if (controller.signal.aborted || latest.current.fail(caught)) return;
      }
      if (!controller.signal.aborted)
        timer = setTimeout(() => void poll(), 1000);
    };
    timer = setTimeout(() => void poll(), 1000);
    return () => {
      controller.abort();
      if (timer !== null) clearTimeout(timer);
    };
  }, [tenant, device.id, done, state]);

  useEffect(() => {
    if (done) return;
    const tick = setInterval(() => setNow(performance.now()), 1000);
    return () => clearInterval(tick);
  }, [done]);

  const stop = async () => {
    if (stopping) return;
    setStopping(true);
    try {
      await api.cancelRemoteRequest(tenant, device.id, request.id, csrf);
    } catch (caught) {
      fail(caught);
    } finally {
      setStopping(false);
    }
  };

  const text = (() => {
    switch (request.state) {
      case "waiting":
        return c.waiting(who);
      case "active":
        return c.active;
      case "declined":
        return c.declined(who);
      case "expired":
        return c.expired(who);
      case "unavailable":
        return c.unavailable;
      case "cancelled":
        return c.cancelled;
      case "ended":
        return c.ended;
      case "failed":
        return request.reason === "fresh_passkey_required"
          ? c.failedFresh
          : c.failed;
    }
  })();
  const seconds =
    started.ends === null
      ? null
      : Math.max(0, Math.ceil((started.ends - now) / 1000));
  return (
    <section className="remote-request-status notice">
      <p className="muted">
        {c.target(device.name, request.os_session_id)} ·{" "}
        {c.mode(request.mode === "administrative", request.uac)}
      </p>
      <div ref={message} tabIndex={-1} role="status" aria-live="polite">
        <strong>{text}</strong>
      </div>
      {!done && seconds !== null && (
        <p className="muted">
          {request.state === "waiting"
            ? c.remaining(seconds)
            : c.activeRemaining(seconds)}
        </p>
      )}
      {request.state === "active" && (
        <Viewer
          tenant={tenant}
          device={device}
          request={request}
          csrf={csrf}
          language={language}
          fail={fail}
        />
      )}
      <div className="remote-request-actions">
        {!done && (
          <button
            type="button"
            className="button"
            disabled={stopping}
            onClick={() => void stop()}
          >
            {request.state === "waiting" ? c.cancel : c.end}
          </button>
        )}
        {done && (
          <button type="button" className="button" onClick={onDismiss}>
            {c.again}
          </button>
        )}
      </div>
    </section>
  );
}
