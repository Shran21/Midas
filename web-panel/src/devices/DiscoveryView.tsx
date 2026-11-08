import {
  type FormEvent,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import {
  ApiError,
  api,
  type CreatedToken,
  type DiscoveredComputer,
  type PendingDevice,
  type Site,
  type TenantContext,
} from "../api";
import { Icon } from "../Icon";
import type { Copy, Language } from "../i18n";
import { errorText } from "../SignIn";
import type { Tab } from "./DeviceMenu";
import { moment, since } from "./labels";
import { useLive } from "./useLive";

type Props = {
  tenant: string;
  tenantName: string;
  access: TenantContext;
  t: Copy;
  language: Language;
  csrf: string;
  fail: (error: unknown) => boolean;
  changes: number;
  onOpen: (device: string, tab: Tab) => void;
};

/** Enrollment decisions and directory discovery; discovery never installs an agent. */
export function DiscoveryView(props: Props) {
  const { access, t } = props;
  const enroll = access.tenant_wide.includes("device.enroll");
  const discover = access.tenant_wide.includes("directory.discover");
  return (
    <div className="discovery">
      <div className="inline-note discovery-note">
        <Icon name="info" size={16} />
        {t.discoveryNote}
      </div>
      {enroll && <Pending {...props} />}
      {enroll && <TokenPanel {...props} />}
      {discover && <Directory {...props} />}
      {enroll && <Decisions {...props} />}
      {!enroll && !discover && <p className="muted">{t.noPermissionHint}</p>}
    </div>
  );
}

type Outcome = { id: string; name: string; ok: boolean; text: string };

function Pending({
  tenant,
  tenantName,
  t,
  language,
  csrf,
  fail,
  changes,
}: Props) {
  const load = useCallback(() => api.pendingDevices(tenant), [tenant]);
  const pending = useLive(load, changes, fail);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [confirm, setConfirm] = useState<{
    decision: "approve" | "reject";
    devices: PendingDevice[];
  } | null>(null);
  const [outcomes, setOutcomes] = useState<Outcome[]>([]);
  const items = pending.value ?? [];
  const toggle = (id: string) =>
    setSelected((current) => {
      const next = new Set(current);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  const run = async (
    decision: "approve" | "reject",
    devices: PendingDevice[],
  ) => {
    const results: Outcome[] = [];
    for (const device of devices) {
      try {
        await api.decide(tenant, device.id, decision, csrf);
        results.push({
          id: device.id,
          name: device.name,
          ok: true,
          text: decision === "approve" ? t.approved : t.rejectedResult,
        });
      } catch (error) {
        if (fail(error)) return;
        results.push({
          id: device.id,
          name: device.name,
          ok: false,
          text: errorText(error, t),
        });
      }
    }
    setOutcomes(results);
    setSelected(new Set());
    void pending.reload();
  };
  const chosen = items.filter((item) => selected.has(item.id));
  return (
    <section className="panel">
      <div className="panel-heading">
        <div className="panel-title">
          <Icon name="clock" />
          <h2>{t.pendingRequests}</h2>
          <span className="count-badge">{items.length}</span>
        </div>
        <div className="bulk-actions">
          <button
            type="button"
            className="button secondary"
            disabled={chosen.length === 0}
            onClick={() => setConfirm({ decision: "approve", devices: chosen })}
          >
            {t.approveSelected} ({chosen.length})
          </button>
          <button
            type="button"
            className="button danger-outline"
            disabled={chosen.length === 0}
            onClick={() => setConfirm({ decision: "reject", devices: chosen })}
          >
            {t.rejectSelected}
          </button>
        </div>
      </div>
      <p className="panel-description">{t.pendingHint}</p>
      {outcomes.length > 0 && (
        <ul className="outcome-list" aria-live="polite">
          {outcomes.map((outcome) => (
            <li key={outcome.id} className={outcome.ok ? "ok" : "failed"}>
              <Icon name={outcome.ok ? "check" : "alert"} size={14} />
              <strong>{outcome.name}</strong> {outcome.text}
            </li>
          ))}
        </ul>
      )}
      {pending.value === undefined ? (
        <p className="loading-line">{t.loading}</p>
      ) : items.length === 0 ? (
        <p className="muted panel-description">{t.noPending}</p>
      ) : (
        <div className="table-scroll">
          <table className="device-table compact-table">
            <thead>
              <tr>
                <th>
                  <input
                    type="checkbox"
                    aria-label={t.selectAll}
                    checked={chosen.length === items.length}
                    onChange={(event) =>
                      setSelected(
                        new Set(
                          event.target.checked
                            ? items.map((item) => item.id)
                            : [],
                        ),
                      )
                    }
                  />
                </th>
                <th>{t.device}</th>
                <th>{t.type}</th>
                <th>{t.requestedAt}</th>
                <th>
                  <span className="sr-only">{t.actions}</span>
                </th>
              </tr>
            </thead>
            <tbody>
              {items.map((item) => (
                <tr key={item.id}>
                  <td>
                    <input
                      type="checkbox"
                      aria-label={`${t.select}: ${item.name}`}
                      checked={selected.has(item.id)}
                      onChange={() => toggle(item.id)}
                    />
                  </td>
                  <td>
                    <strong>{item.name}</strong>
                    <small className="cell-secondary">
                      {item.platform === "windows" ? "Windows" : "Linux"}
                    </small>
                  </td>
                  <td>{item.kind === "server" ? t.server : t.workstation}</td>
                  <td>{since(item.requested_at, language)}</td>
                  <td className="row-buttons">
                    <button
                      type="button"
                      className="button secondary"
                      onClick={() =>
                        setConfirm({ decision: "approve", devices: [item] })
                      }
                    >
                      {t.approve}
                    </button>
                    <button
                      type="button"
                      className="button danger-outline"
                      onClick={() =>
                        setConfirm({ decision: "reject", devices: [item] })
                      }
                    >
                      {t.reject}
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
      {confirm && (
        <DecisionDialog
          t={t}
          tenantName={tenantName}
          decision={confirm.decision}
          devices={confirm.devices}
          onClose={() => setConfirm(null)}
          onConfirm={() => run(confirm.decision, confirm.devices)}
        />
      )}
    </section>
  );
}

function DecisionDialog({
  t,
  tenantName,
  decision,
  devices,
  onClose,
  onConfirm,
}: {
  t: Copy;
  tenantName: string;
  decision: "approve" | "reject";
  devices: PendingDevice[];
  onClose: () => void;
  onConfirm: () => Promise<void>;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  const opener = useRef<Element | null>(document.activeElement);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    if (!dialog.current?.open) dialog.current?.showModal();
  }, []);
  return (
    <dialog
      ref={dialog}
      className="confirm-dialog"
      aria-labelledby="decision-title"
      onClose={() => {
        const target = opener.current;
        onClose();
        if (target instanceof HTMLElement && target.isConnected) target.focus();
      }}
    >
      <h2 id="decision-title">
        {decision === "approve" ? t.approveTitle : t.rejectTitle}
      </h2>
      <p>
        {t.customer}: <strong>{tenantName}</strong> · {devices.length}{" "}
        {t.devicesAffected}
      </p>
      <ul className="affected-list">
        {devices.map((device) => (
          <li key={device.id}>
            <strong>{device.name}</strong>{" "}
            <span className="muted">
              {device.platform === "windows" ? "Windows" : "Linux"} ·{" "}
              {device.kind === "server" ? t.server : t.workstation}
            </span>
          </li>
        ))}
      </ul>
      <p>{decision === "approve" ? t.approveHint : t.rejectHint}</p>
      <p className="quick-note">
        <Icon name="key" size={13} />
        {t.needsFreshPasskey}
      </p>
      <div className="dialog-actions">
        <button
          type="button"
          className="button secondary"
          onClick={() => dialog.current?.close()}
        >
          {t.cancel}
        </button>
        <button
          type="button"
          className={
            decision === "approve" ? "button primary" : "button danger"
          }
          disabled={busy}
          onClick={async () => {
            setBusy(true);
            await onConfirm();
            setBusy(false);
            dialog.current?.close();
          }}
        >
          {decision === "approve" ? t.approve : t.reject}
        </button>
      </div>
    </dialog>
  );
}

function TokenPanel({ tenant, access, t, language, csrf, fail }: Props) {
  const canListSites = access.tenant_wide.includes("access.manage");
  const load = useCallback(
    () => (canListSites ? api.sites(tenant) : Promise.resolve([] as Site[])),
    [tenant, canListSites],
  );
  const sites = useLive(load, 0, fail, 600_000);
  const [site, setSite] = useState("");
  const [platform, setPlatform] = useState<"windows" | "linux">("windows");
  const [kind, setKind] = useState<"workstation" | "server">("workstation");
  const [preapproved, setPreapproved] = useState(false);
  const [hours, setHours] = useState(24);
  const [created, setCreated] = useState<CreatedToken | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const submit = async (event: FormEvent) => {
    event.preventDefault();
    setError(null);
    setCopied(false);
    try {
      setCreated(
        await api.createToken(
          tenant,
          {
            site_id: site || null,
            platform,
            kind,
            preapproved,
            lifetime_hours: hours,
          },
          csrf,
        ),
      );
    } catch (failure) {
      if (!fail(failure)) setError(errorText(failure, t));
    }
  };
  return (
    <section className="panel">
      <div className="panel-heading">
        <div className="panel-title">
          <Icon name="key" />
          <h2>{t.tokenTitle}</h2>
        </div>
      </div>
      <p className="panel-description">{t.tokenHint}</p>
      <form className="token-form" onSubmit={submit}>
        {canListSites && (
          <label className="field">
            <span>{t.site}</span>
            <select
              value={site}
              onChange={(event) => setSite(event.target.value)}
            >
              <option value="">{t.noSite}</option>
              {(sites.value ?? []).map((item) => (
                <option key={item.id} value={item.id}>
                  {item.name}
                </option>
              ))}
            </select>
          </label>
        )}
        <label className="field">
          <span>{t.operatingSystem}</span>
          <select
            value={platform}
            onChange={(event) => setPlatform(event.target.value as "windows")}
          >
            <option value="windows">Windows</option>
            <option value="linux">Linux</option>
          </select>
        </label>
        <label className="field">
          <span>{t.type}</span>
          <select
            value={kind}
            onChange={(event) => setKind(event.target.value as "workstation")}
          >
            <option value="workstation">{t.workstation}</option>
            <option value="server">{t.server}</option>
          </select>
        </label>
        <label className="field">
          <span>{t.tokenHours}</span>
          <input
            type="number"
            min={1}
            max={720}
            value={hours}
            onChange={(event) => setHours(Number(event.target.value))}
          />
        </label>
        <label className="check-field">
          <input
            type="checkbox"
            checked={preapproved}
            onChange={(event) => setPreapproved(event.target.checked)}
          />
          <span>
            {t.preapproved}
            <small>{t.preapprovedHint}</small>
          </span>
        </label>
        <button type="submit" className="button primary">
          {t.createToken}
        </button>
      </form>
      {error && (
        <p className="form-error" role="alert">
          {error}
        </p>
      )}
      {created && (
        <div className="token-result" role="status">
          <p>
            <strong>{t.tokenOnce}</strong> {t.tokenExpires}:{" "}
            {moment(created.expires_at, language)}
          </p>
          <code>{created.token}</code>
          <div className="row-buttons">
            <button
              type="button"
              className="button secondary"
              onClick={async () => {
                try {
                  await navigator.clipboard.writeText(created.token);
                  setCopied(true);
                } catch {
                  setCopied(false);
                }
              }}
            >
              {copied ? t.copied : t.copy}
            </button>
            <button
              type="button"
              className="button secondary"
              onClick={() => setCreated(null)}
            >
              {t.hideToken}
            </button>
          </div>
          <p className="muted">{t.tokenHandling}</p>
        </div>
      )}
    </section>
  );
}

function Directory({ tenant, t, language, fail, changes, onOpen }: Props) {
  const loadConnector = useCallback(
    () =>
      api.connector(tenant).catch((error: unknown) => {
        if (error instanceof ApiError && error.status === 404) return null;
        throw error;
      }),
    [tenant],
  );
  const loadComputers = useCallback(() => api.discovered(tenant), [tenant]);
  const connector = useLive(loadConnector, changes, fail);
  const computers = useLive(loadComputers, changes, fail);
  const [status, setStatus] = useState<
    "all" | DiscoveredComputer["status"] | "removed"
  >("all");
  const [query, setQuery] = useState("");
  const list = computers.value ?? [];
  const shown = useMemo(() => {
    const normalized = query.trim().toLocaleLowerCase(language);
    return list.filter(
      (item) =>
        (status === "all"
          ? true
          : status === "removed"
            ? item.removed_at !== null
            : item.status === status && item.removed_at === null) &&
        (normalized === "" ||
          [
            item.name,
            item.dns_host_name,
            item.distinguished_name,
            item.operating_system,
          ].some((value) =>
            value.toLocaleLowerCase(language).includes(normalized),
          )),
    );
  }, [list, status, query, language]);
  const count = (value: DiscoveredComputer["status"]) =>
    list.filter((item) => item.status === value && item.removed_at === null)
      .length;
  const check = connector.value?.last_check;
  return (
    <section className="panel">
      <div className="panel-heading">
        <div className="panel-title">
          <Icon name="search" />
          <h2>{t.discoveredTitle}</h2>
          <span className="count-badge">{list.length}</span>
        </div>
      </div>
      <p className="panel-description">
        {connector.value === null
          ? t.noConnector
          : connector.value
            ? `${t.connectorLabel}: ${connector.value.domain} · ${
                check
                  ? check.ok
                    ? `${t.lastScan} ${since(check.at, language)}, ${check.objects} ${t.computers}`
                    : `${t.scanFailed} (${check.error ?? "?"}), ${since(check.at, language)}`
                  : t.noScanYet
              }`
            : t.loading}
      </p>
      <div className="inventory-tools">
        <label className="device-search">
          <Icon name="search" size={16} />
          <span className="sr-only">{t.searchLabel}</span>
          <input
            type="search"
            placeholder={t.computerSearch}
            value={query}
            onChange={(event) => setQuery(event.target.value)}
          />
        </label>
        <div className="kind-switch">
          {(
            [
              ["all", t.all, list.length],
              ["candidate", t.candidate, count("candidate")],
              ["managed", t.managedComputer, count("managed")],
              ["conflict", t.conflictStatus, count("conflict")],
              [
                "removed",
                t.removedComputer,
                list.filter((item) => item.removed_at !== null).length,
              ],
            ] as const
          ).map(([value, label, total]) => (
            <button
              key={value}
              type="button"
              aria-pressed={status === value}
              onClick={() => setStatus(value)}
            >
              {label}
              <span>{total}</span>
            </button>
          ))}
        </div>
      </div>
      {computers.value === undefined ? (
        computers.error ? (
          <p className="form-error">{errorText(computers.error, t)}</p>
        ) : (
          <p className="loading-line">{t.loading}</p>
        )
      ) : shown.length === 0 ? (
        <p className="muted panel-description">{t.noComputers}</p>
      ) : (
        <div className="table-scroll">
          <table className="device-table compact-table">
            <thead>
              <tr>
                <th>{t.computer}</th>
                <th>{t.status}</th>
                <th>{t.operatingSystem}</th>
                <th>{t.lastLogon}</th>
              </tr>
            </thead>
            <tbody>
              {shown.map((item) => (
                <tr
                  key={`${item.domain}/${item.object_guid}`}
                  className={item.removed_at ? "removed-row" : undefined}
                >
                  <td>
                    <strong>{item.name}</strong>
                    <small className="cell-secondary">
                      {item.dns_host_name || item.domain}
                    </small>
                    <small className="cell-secondary dn">
                      {item.distinguished_name}
                    </small>
                  </td>
                  <td>
                    <span
                      className={["discovery-status", item.status].join(" ")}
                    >
                      {item.removed_at
                        ? t.removedComputer
                        : {
                            candidate: t.candidate,
                            managed: t.managedComputer,
                            conflict: t.conflictStatus,
                          }[item.status]}
                    </span>
                    {item.disabled && (
                      <small className="cell-secondary">
                        {t.accountDisabled}
                      </small>
                    )}
                    {item.devices.map((device) => (
                      <button
                        key={device.id}
                        type="button"
                        className="inline-link"
                        onClick={() => onOpen(device.id, "overview")}
                      >
                        {device.name}
                      </button>
                    ))}
                    {item.name_matches.length > 0 && (
                      <small className="cell-secondary hint">
                        {t.nameHint}:{" "}
                        {item.name_matches
                          .map((device) => device.name)
                          .join(", ")}
                      </small>
                    )}
                  </td>
                  <td>
                    {item.operating_system || "—"}
                    <small className="cell-secondary">
                      {item.operating_system_version}
                    </small>
                  </td>
                  <td>{moment(item.last_logon_at, language) ?? "—"}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </section>
  );
}

function Decisions({ tenant, t, language, fail, changes, onOpen }: Props) {
  const load = useCallback(() => api.decisions(tenant), [tenant]);
  const decisions = useLive(load, changes, fail);
  const items = decisions.value ?? [];
  return (
    <section className="panel">
      <div className="panel-heading">
        <div className="panel-title">
          <Icon name="list" />
          <h2>{t.decisionsTitle}</h2>
          <span className="count-badge">{items.length}</span>
        </div>
      </div>
      {decisions.value === undefined ? (
        <p className="loading-line">{t.loading}</p>
      ) : items.length === 0 ? (
        <p className="muted panel-description">{t.noDecisions}</p>
      ) : (
        <div className="table-scroll">
          <table className="device-table compact-table">
            <thead>
              <tr>
                <th>{t.device}</th>
                <th>{t.decision}</th>
                <th>{t.decidedBy}</th>
                <th>{t.decidedAt}</th>
              </tr>
            </thead>
            <tbody>
              {items.map((item) => (
                <tr key={item.id}>
                  <td>
                    {item.enrollment === "rejected" ? (
                      <strong>{item.name}</strong>
                    ) : (
                      <button
                        type="button"
                        className="inline-link"
                        onClick={() => onOpen(item.id, "overview")}
                      >
                        {item.name}
                      </button>
                    )}
                  </td>
                  <td>
                    {
                      {
                        active: t.decisionActive,
                        rejected: t.decisionRejected,
                        revoked: t.revokedState,
                      }[item.enrollment]
                    }
                  </td>
                  <td>{item.decided_by ?? t.preapprovedToken}</td>
                  <td>{moment(item.decided_at, language)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </section>
  );
}
