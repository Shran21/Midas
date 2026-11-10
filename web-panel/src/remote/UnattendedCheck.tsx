import { type FormEvent, useEffect, useId, useRef, useState } from "react";
import {
  ApiError,
  api,
  type DeviceSummary,
  type SessionSelection,
} from "../api";
import type { Language } from "../i18n";
import { decisionCopy } from "./decisionCopy";
import { readDecision, validReason } from "./model";

type Props = {
  tenant: string;
  device: DeviceSummary;
  session: SessionSelection;
  csrf: string;
  language: Language;
  fail: (error: unknown) => boolean;
  confirmIdentity: (force?: boolean) => Promise<string>;
};
type Feedback = {
  kind:
    | "eligible"
    | "consent"
    | "expired"
    | "denied"
    | "unavailable"
    | "failed"
    | "fresh"
    | "invalidReason";
  deadline?: number;
};

export function UnattendedCheck({
  tenant,
  device,
  session,
  csrf,
  language,
  fail,
  confirmIdentity,
}: Props) {
  const c = decisionCopy[language];
  const id = useId();
  const [mode, setMode] = useState<"normal" | "administrative">("normal");
  const [uac, setUac] = useState(false);
  const [reason, setReason] = useState("");
  const [busy, setBusy] = useState(false);
  const [feedback, setFeedback] = useState<Feedback | null>(null);
  const request = useRef<AbortController | null>(null);
  const mounted = useRef(false);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      request.current?.abort();
      request.current = null;
      if (timer.current !== null) clearTimeout(timer.current);
    };
  }, []);
  useEffect(() => {
    if (feedback?.deadline === undefined) return;
    const timeout = setTimeout(
      () => setFeedback({ kind: "expired" }),
      Math.max(0, feedback.deadline - performance.now()),
    );
    return () => clearTimeout(timeout);
  }, [feedback]);

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (request.current || busy || document.visibilityState !== "visible")
      return;
    if (!validReason(reason)) {
      setFeedback({ kind: "invalidReason" });
      return;
    }
    const controller = new AbortController();
    request.current = controller;
    setBusy(true);
    setFeedback(null);
    const timeout = setTimeout(() => {
      if (request.current !== controller) return;
      request.current = null;
      controller.abort();
      setBusy(false);
      setFeedback({ kind: "unavailable" });
    }, 12_000);
    timer.current = timeout;
    try {
      const result = await api.checkUnattended(
        tenant,
        device.id,
        { mode, uac, reason, session },
        csrf,
        controller.signal,
      );
      if (request.current !== controller) return;
      const received = performance.now();
      const wallNow = Date.now();
      const decision = readDecision(result, session, wallNow);
      if (!decision) throw new Error("invalid decision");
      setFeedback(
        decision.kind === "eligible"
          ? {
              kind: "eligible",
              deadline: received + Math.max(0, decision.expires - wallNow),
            }
          : { kind: "consent" },
      );
    } catch (error) {
      if (request.current === controller && !fail(error)) {
        setFeedback({
          kind:
            error instanceof ApiError
              ? error.code === "fresh_passkey_required"
                ? "fresh"
                : error.status === 409
                  ? "unavailable"
                  : [403, 404].includes(error.status)
                    ? "denied"
                    : "failed"
              : "failed",
        });
      }
    } finally {
      clearTimeout(timeout);
      if (request.current === controller) {
        request.current = null;
        setBusy(false);
      }
    }
  };
  const authenticate = async () => {
    if (busy) return;
    setBusy(true);
    try {
      await confirmIdentity(true);
    } catch (error) {
      if (mounted.current && !fail(error)) setFeedback({ kind: "fresh" });
    } finally {
      if (mounted.current) setBusy(false);
    }
  };
  const status =
    feedback?.kind === "eligible" &&
    performance.now() >= (feedback.deadline ?? 0)
      ? "expired"
      : feedback?.kind;
  return (
    <form
      className="remote-check"
      onSubmit={(event) => void submit(event)}
      aria-labelledby={`${id}-title`}
    >
      <h3 id={`${id}-title`}>{c.title}</h3>
      <p className="muted">{c.intro}</p>
      <fieldset disabled={busy}>
        <label htmlFor={`${id}-mode`}>{c.mode}</label>
        <select
          id={`${id}-mode`}
          value={mode}
          onChange={(event) => {
            setMode(
              event.target.value === "administrative"
                ? "administrative"
                : "normal",
            );
            setFeedback(null);
          }}
        >
          <option value="normal">{c.normal}</option>
          {device.permissions.includes("remote.admin") && (
            <option value="administrative">{c.admin}</option>
          )}
        </select>
        {device.permissions.includes("remote.uac.interact") && (
          <label className="remote-check-uac">
            <input
              type="checkbox"
              checked={uac}
              onChange={(event) => {
                setUac(event.target.checked);
                setFeedback(null);
              }}
            />
            {c.uac}
          </label>
        )}
        <label htmlFor={`${id}-reason`}>{c.reason}</label>
        <input
          id={`${id}-reason`}
          value={reason}
          required
          maxLength={512}
          aria-describedby={`${id}-hint`}
          onChange={(event) => {
            setReason(event.target.value);
            setFeedback(null);
          }}
        />
        <p id={`${id}-hint`} className="muted">
          {c.reasonHint}
        </p>
        <button type="submit" className="button primary">
          {busy ? c.checking : c.check}
        </button>
      </fieldset>
      {status && (
        <p
          role={
            status === "eligible" ||
            status === "consent" ||
            status === "expired"
              ? "status"
              : "alert"
          }
          className="notice"
        >
          {c[status]}
        </p>
      )}
      {status === "fresh" && (
        <button
          type="button"
          className="button"
          disabled={busy}
          onClick={() => void authenticate()}
        >
          {c.authenticate}
        </button>
      )}
    </form>
  );
}
