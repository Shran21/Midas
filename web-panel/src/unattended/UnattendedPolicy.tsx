import {
  type FormEvent,
  useCallback,
  useEffect,
  useId,
  useRef,
  useState,
} from "react";
import { ApiError, api, type DeviceSummary } from "../api";
import { moment } from "../devices/labels";
import { useLive } from "../devices/useLive";
import { Icon } from "../Icon";
import { type Language, translations } from "../i18n";
import { errorText } from "../SignIn";
import { policyCopy } from "./copy";
import {
  type Draft,
  draftFor,
  type Policy,
  parseLocalTime,
  policyKey,
  windowFor,
} from "./model";

type Props = {
  tenant: string;
  tenantName: string;
  device?: DeviceSummary;
  manage: boolean;
  language: Language;
  changes: number;
  confirmIdentity: (force?: boolean) => Promise<string>;
  fail: (error: unknown) => boolean;
};

export function UnattendedPolicy(props: Props) {
  if (props.device && props.device.kind !== "server") return null;
  return (
    <PolicyForm
      key={`${props.tenant}-${props.device?.id ?? "tenant"}`}
      {...props}
    />
  );
}

function PolicyForm({
  tenant,
  tenantName,
  device,
  manage,
  language,
  changes,
  confirmIdentity,
  fail,
}: Props) {
  const c = policyCopy[language];
  const t = translations[language];
  const id = useId();
  const deviceId = device?.id;
  const load = useCallback(
    (): Promise<Policy> =>
      deviceId
        ? api.deviceUnattended(tenant, deviceId)
        : api.tenantUnattended(tenant),
    [tenant, deviceId],
  );
  const live = useLive(load, changes, fail);
  const [draft, setDraft] = useState<Draft | null>(null);
  const [busy, setBusy] = useState(false);
  const [feedback, setFeedback] = useState<{
    error: boolean;
    text: string;
  } | null>(null);
  const mounted = useRef(false);
  const saving = useRef(false);
  const forceConfirmation = useRef(false);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  const saved = live.value;
  const form = draft ?? (saved ? draftFor(saved) : null);
  const conflict = !!(saved && draft && draft.base !== policyKey(saved));
  const edit = (update: Partial<Draft>) => {
    if (!form) return;
    setDraft({ ...form, ...update });
    setFeedback(null);
  };
  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (!manage || !form || !draft || conflict || live.error || saving.current)
      return;
    const window = deviceId ? windowFor(form, Date.now() / 1000) : null;
    if (typeof window === "string") {
      setFeedback({ error: true, text: c[window] });
      return;
    }
    saving.current = true;
    setBusy(true);
    setFeedback(null);
    let written = false;
    try {
      const csrf = await confirmIdentity(forceConfirmation.current);
      forceConfirmation.current = false;
      if (!mounted.current) return;
      const current = await load();
      if (!mounted.current) return;
      if (policyKey(current) !== draft.base) {
        setFeedback({ error: true, text: c.conflict });
        await live.reload();
        return;
      }
      const spec = { enabled: form.enabled, maintenance_window: window };
      if (deviceId) await api.setDeviceUnattended(tenant, deviceId, spec, csrf);
      else await api.setTenantUnattended(tenant, form.enabled, csrf);
      written = true;
      if (!mounted.current) return;
      setDraft(null);
      const verified = await load();
      const expected = deviceId ? spec : { enabled: form.enabled };
      if (policyKey(verified) !== policyKey(expected))
        throw new Error("readback");
      if (mounted.current) setFeedback({ error: false, text: c.saved });
    } catch (error) {
      const needsConfirmation =
        error instanceof ApiError && error.code === "fresh_passkey_required";
      if (needsConfirmation) forceConfirmation.current = true;
      if (mounted.current && !fail(error))
        setFeedback({
          error: true,
          text: written
            ? c.readbackFailed
            : needsConfirmation
              ? c.confirmRetry
              : errorText(error, t),
        });
    } finally {
      saving.current = false;
      if (mounted.current) {
        setBusy(false);
        if (written) void live.reload();
      }
    }
  };
  const window =
    saved && "maintenance_window" in saved ? saved.maintenance_window : null;
  const tenantEnabled =
    saved && "tenant_enabled" in saved ? saved.tenant_enabled : saved?.enabled;
  const utc = (value: string) => {
    const seconds = parseLocalTime(value);
    return seconds === null
      ? "—"
      : new Date(seconds * 1000)
          .toISOString()
          .replace("T", " ")
          .replace(".000Z", " UTC");
  };

  return (
    <section className="panel unattended-panel" aria-labelledby={`${id}-title`}>
      <div className="panel-heading">
        <div className="panel-title">
          <Icon name="shield" />
          <h2 id={`${id}-title`}>{c.title}</h2>
        </div>
        <span className="count-badge">
          {deviceId ? c.deviceTitle : c.tenantTitle}
        </span>
      </div>
      <div className="unattended-content">
        <p className="muted">{deviceId ? c.deviceHint : c.tenantHint}</p>
        <p className="unattended-target">
          <strong>{tenantName}</strong>
          {device && (
            <>
              {" "}
              › <strong>{device.name}</strong>
            </>
          )}
        </p>
        {live.error ? (
          <p className="form-error" role="alert">
            {errorText(live.error, t)}
          </p>
        ) : null}
        {!saved && !live.error && <p role="status">{t.loading}</p>}
        {saved && (
          <>
            <dl className="unattended-status">
              <div>
                <dt>{c.customer}</dt>
                <dd>{tenantEnabled ? c.enabled : c.disabled}</dd>
              </div>
              {deviceId && (
                <div>
                  <dt>{c.server}</dt>
                  <dd>{saved.enabled ? c.enabled : c.disabled}</dd>
                </div>
              )}
              {deviceId && (
                <div>
                  <dt>{c.savedWindow}</dt>
                  <dd>
                    {window ? (
                      <>
                        {moment(window.starts_at, language)} –{" "}
                        {moment(window.ends_at, language)}
                        <small>
                          {"within_maintenance_window" in saved &&
                          saved.within_maintenance_window
                            ? c.windowOpen
                            : c.windowClosed}
                        </small>
                      </>
                    ) : (
                      c.noWindow
                    )}
                  </dd>
                </div>
              )}
            </dl>
            {deviceId && !tenantEnabled && (
              <p className="notice">{c.tenantOff}</p>
            )}
            {manage && form ? (
              <form onSubmit={(event) => void submit(event)}>
                <fieldset
                  disabled={busy || !!live.error}
                  className="unattended-fields"
                >
                  <legend className="sr-only">
                    {deviceId ? c.deviceTitle : c.tenantTitle}
                  </legend>
                  <label className="check-field">
                    <input
                      type="checkbox"
                      checked={form.enabled}
                      onChange={(event) =>
                        edit({ enabled: event.target.checked })
                      }
                    />
                    <span>
                      {deviceId ? c.deviceToggle : c.tenantToggle}
                      {deviceId && <small>{c.offHint}</small>}
                    </span>
                  </label>
                  {deviceId && form.enabled && (
                    <>
                      <p className="muted">
                        {c.timezone}:{" "}
                        <strong>
                          {Intl.DateTimeFormat().resolvedOptions().timeZone}
                        </strong>
                      </p>
                      <div className="unattended-window">
                        {(["start", "end"] as const).map((field) => (
                          <label className="field" key={field}>
                            <span id={`${id}-${field}-label`}>{c[field]}</span>
                            <input
                              type="datetime-local"
                              aria-labelledby={`${id}-${field}-label`}
                              step="1"
                              value={form[field]}
                              onChange={(event) =>
                                edit({ [field]: event.target.value })
                              }
                              required
                              aria-describedby={`${id}-${field}-utc`}
                            />
                            <small className="muted" id={`${id}-${field}-utc`}>
                              {c.utc}: {utc(form[field])}
                            </small>
                          </label>
                        ))}
                      </div>
                    </>
                  )}
                  <p className="muted">
                    <Icon name="key" size={14} /> {c.authHint}
                  </p>
                  {conflict && (
                    <p className="form-error" role="alert">
                      {c.conflict}
                    </p>
                  )}
                  <div className="unattended-actions">
                    <button
                      type="submit"
                      className="button primary"
                      disabled={!draft || conflict}
                    >
                      {busy ? c.saving : c.save}
                    </button>
                    {draft && (
                      <button
                        type="button"
                        className="button secondary"
                        onClick={() => {
                          setDraft(null);
                          setFeedback(null);
                        }}
                      >
                        {c.discard}
                      </button>
                    )}
                  </div>
                </fieldset>
              </form>
            ) : (
              <p className="muted">{c.readOnly}</p>
            )}
            <p className="muted">{c.conditions}</p>
            {deviceId && <p className="muted">{c.reboot}</p>}
          </>
        )}
        {feedback && (
          <p
            className={feedback.error ? "form-error" : "notice"}
            role={feedback.error ? "alert" : "status"}
          >
            {feedback.text}
          </p>
        )}
        <p className="unattended-availability">
          <Icon name="info" size={16} />
          {c.unavailable}
        </p>
        <div className="unattended-actions">
          <button
            type="button"
            className="button secondary"
            disabled={busy || live.loading}
            onClick={() => void live.reload()}
          >
            {c.refresh}
          </button>
          {live.loadedAt && (
            <small className="muted">
              {c.checked}: {moment(live.loadedAt, language)}
            </small>
          )}
        </div>
      </div>
    </section>
  );
}
