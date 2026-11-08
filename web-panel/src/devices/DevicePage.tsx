import {
  type KeyboardEvent,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import {
  type AlertView,
  api,
  type DeviceSummary,
  type HealthCheck,
  type MetricsView,
  type TenantContext,
} from "../api";
import { Icon } from "../Icon";
import type { Copy, Language } from "../i18n";
import { errorText } from "../SignIn";
import {
  actionIcons,
  actionLabel,
  blockedText,
  DeviceMenu,
  type MenuState,
  type Tab,
  useDeviceActions,
} from "./DeviceMenu";
import { ConditionBadge, platformName, usage } from "./DevicesView";
import {
  alertTitle,
  bytes,
  checkNames,
  inventoryKeys,
  inventoryKinds,
  moment,
  reasons,
  settingNames,
  since,
} from "./labels";
import { type Action, deviceActions } from "./model";
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
  deviceId: string;
  tab: Tab;
  onTab: (tab: Tab) => void;
  onOpen: (device: string, tab: Tab) => void;
  onIdentity?: (name: string) => void;
};

const tabs: Tab[] = ["overview", "monitoring", "inventory", "policy"];

export function DevicePage(props: Props) {
  const { tenant, tenantName, access, t, language, fail, changes, deviceId } =
    props;
  const load = useCallback(
    () => api.device(tenant, deviceId),
    [tenant, deviceId],
  );
  const live = useLive(load, changes, fail, 30_000);
  const [menu, setMenu] = useState<MenuState | null>(null);
  const actions = useDeviceActions({
    tenant,
    tenantName,
    csrf: props.csrf,
    t,
    fail,
    onOpen: props.onOpen,
    onChanged: () => void live.reload(),
  });
  const tabList = useRef<HTMLDivElement>(null);
  const closeMenu = useCallback(() => setMenu(null), []);
  const device = live.value;
  const name = device?.name;
  const { onIdentity } = props;
  useEffect(() => {
    if (name) onIdentity?.(name);
  }, [name, onIdentity]);

  if (!device) {
    if (live.error)
      return (
        <div className="empty-state" role="alert">
          <span className="empty-icon">
            <Icon name="info" size={25} />
          </span>
          <h3>{t.deviceUnavailable}</h3>
          <p>{errorText(live.error, t)}</p>
          <button
            type="button"
            className="button secondary"
            onClick={() => void live.reload()}
          >
            {t.retry}
          </button>
        </div>
      );
    return <p className="loading-line">{t.loading}</p>;
  }

  const offered = deviceActions(device, access);
  const allowedTabs = tabs.filter(
    (tab) => tab === "overview" || offered.some((action) => action.id === tab),
  );
  const tab = allowedTabs.includes(props.tab) ? props.tab : "overview";
  const common = offered.filter((action) =>
    ["refresh", "remote", "remoteAdmin", "revoke"].includes(action.id),
  );
  const tabKeys = (event: KeyboardEvent) => {
    const index = allowedTabs.indexOf(tab);
    const next =
      event.key === "ArrowRight"
        ? index + 1
        : event.key === "ArrowLeft"
          ? index - 1
          : null;
    if (next === null) return;
    event.preventDefault();
    const target =
      allowedTabs[(next + allowedTabs.length) % allowedTabs.length];
    props.onTab(target);
    tabList.current
      ?.querySelector<HTMLButtonElement>(`[data-tab="${target}"]`)
      ?.focus();
  };

  return (
    <>
      <section className="target-bar" aria-label={t.target}>
        <span className="device-icon">
          <Icon name={device.kind === "server" ? "server" : "monitor"} />
        </span>
        <div className="target-identity">
          <span className="target-path">
            {t.customer}: <strong className="target-name">{tenantName}</strong>
            <span className="separator">›</span>
            {t.device}: <strong className="target-name">{device.name}</strong>
          </span>
          <span className="target-meta">
            {device.os ?? platformName(device)} ·{" "}
            {device.kind === "server" ? t.server : t.workstation} ·{" "}
            {device.site?.name ?? t.noSite}
          </span>
        </div>
        <ConditionBadge device={device} t={t} />
        <button
          type="button"
          className="icon-button menu-button"
          aria-haspopup="menu"
          aria-expanded={menu !== null}
          aria-label={`${t.actions}: ${device.name}`}
          title={t.actions}
          onClick={(event) => {
            const box = event.currentTarget.getBoundingClientRect();
            setMenu({
              device,
              actions: offered,
              x: box.right - 240,
              y: box.bottom + 4,
              opener: event.currentTarget,
            });
          }}
        >
          <Icon name="more" />
        </button>
      </section>
      <AvailabilityNote device={device} t={t} language={language} />
      {actions.element}
      {common.length > 0 && (
        <section className="quick-actions" aria-label={t.commonActions}>
          {common.map((action) => (
            <QuickAction
              key={action.id}
              action={action}
              t={t}
              onRun={() => void actions.run(device, action)}
            />
          ))}
        </section>
      )}
      <div
        className="device-tabs"
        role="tablist"
        aria-label={t.deviceSections}
        ref={tabList}
        onKeyDown={tabKeys}
      >
        {allowedTabs.map((name) => (
          <button
            key={name}
            type="button"
            role="tab"
            data-tab={name}
            id={`tab-${name}`}
            aria-selected={tab === name}
            aria-controls="device-panel"
            tabIndex={tab === name ? 0 : -1}
            onClick={() => props.onTab(name)}
          >
            {tabLabel(name, t)}
          </button>
        ))}
      </div>
      <div
        id="device-panel"
        role="tabpanel"
        aria-labelledby={`tab-${tab}`}
        className="device-panel"
      >
        {tab === "overview" && (
          <Overview
            {...props}
            device={device}
            onTab={(next) => props.onTab(next)}
          />
        )}
        {tab === "monitoring" && <Monitoring {...props} device={device} />}
        {tab === "inventory" && <Inventory {...props} device={device} />}
        {tab === "policy" && <Policy {...props} device={device} />}
      </div>
      {menu && (
        <DeviceMenu
          menu={menu}
          tenantName={tenantName}
          t={t}
          onAction={(target, action) => void actions.run(target, action)}
          onClose={closeMenu}
        />
      )}
    </>
  );
}

function tabLabel(tab: Tab, t: Copy) {
  return {
    overview: t.overview,
    monitoring: t.monitoringAndAlerts,
    inventory: t.actionInventory,
    policy: t.actionPolicy,
  }[tab];
}

function QuickAction({
  action,
  t,
  onRun,
}: {
  action: Action;
  t: Copy;
  onRun: () => void;
}) {
  const note = blockedText(action, t);
  const id = `quick-${action.id}`;
  return (
    <div className={action.blocked ? "quick-action blocked" : "quick-action"}>
      <button
        type="button"
        className={
          action.destructive ? "button danger-outline" : "button secondary"
        }
        aria-disabled={action.blocked !== null || undefined}
        aria-describedby={note ? id : undefined}
        onClick={() => {
          if (!action.blocked) onRun();
        }}
      >
        <Icon name={actionIcons[action.id]} size={16} />
        {actionLabel(action.id, t)}
      </button>
      {note && (
        <small id={id} className="quick-note">
          {action.fresh && !action.blocked && <Icon name="key" size={13} />}
          {note}
        </small>
      )}
    </div>
  );
}

function AvailabilityNote({
  device,
  t,
  language,
}: {
  device: DeviceSummary;
  t: Copy;
  language: Language;
}) {
  if (device.enrollment === "revoked")
    return (
      <div className="availability revoked" role="note">
        <span className="availability-icon">
          <Icon name="ban" size={18} />
        </span>
        <div>
          <strong className="availability-title">{t.revokedTitle}</strong>
          <p>{t.revokedNote}</p>
        </div>
      </div>
    );
  if (device.online) return null;
  return (
    <div className="availability offline" role="note">
      <span className="availability-icon">
        <Icon name="info" size={18} />
      </span>
      <div>
        <strong className="availability-title">
          {t.offlineTitle}
          {device.last_seen_at !== null &&
            ` · ${t.lastSeen}: ${moment(device.last_seen_at, language)}`}
        </strong>
        <p>{t.offlineWorks}</p>
        <p>{t.offlineBlocked}</p>
      </div>
    </div>
  );
}

function stateLabel(state: string, t: Copy) {
  return (
    {
      ok: t.stateOk,
      warning: t.warning,
      critical: t.critical,
      unavailable: t.stateUnavailable,
    }[state] ?? state
  );
}

function CheckList({
  checks,
  language,
  t,
  detailed,
}: {
  checks: HealthCheck[];
  language: Language;
  t: Copy;
  detailed: boolean;
}) {
  if (checks.length === 0)
    return <p className="muted panel-description">{t.noHealthYet}</p>;
  return (
    <ul className="check-list">
      {checks.map((check) => (
        <li key={check.id} className={check.state}>
          <span className={["check-state", check.state].join(" ")}>
            {stateLabel(check.state, t)}
          </span>
          <span className="check-body">
            <strong>{checkNames[language][check.id] ?? check.id}</strong>
            <span>{reasons[language][check.code] ?? check.summary}</span>
            {detailed && check.subjects.length > 0 && (
              <span className="check-subjects">
                {check.subjects.join(", ")}
              </span>
            )}
            {detailed && (
              <small className="muted">
                {t.changedAt}: {moment(check.changed_at, language)}
              </small>
            )}
          </span>
        </li>
      ))}
    </ul>
  );
}

type Section = Props & { device: DeviceSummary };

function Overview({
  device,
  t,
  language,
  tenant,
  fail,
  changes,
  onTab,
}: Section) {
  const canMonitor = device.permissions.includes("monitor.read");
  const load = useCallback(
    () => api.health(tenant, device.id),
    [tenant, device.id],
  );
  const health = useLive(load, changes, fail);
  const used = usage(device);
  const facts: [string, string][] = [
    [t.site, device.site?.name ?? t.noSite],
    [t.groups, device.groups.map((g) => g.name).join(", ") || "—"],
    [t.operatingSystem, device.os ?? platformName(device)],
    [t.type, device.kind === "server" ? t.server : t.workstation],
    [
      t.lastSeen,
      device.online
        ? t.now
        : (moment(device.last_seen_at, language) ?? t.never),
    ],
    [t.enrolledAt, moment(device.decided_at, language) ?? "—"],
    [
      t.inventoryReconciled,
      moment(device.inventory_reconciled_at, language) ?? "—",
    ],
    [t.agentCapabilities, device.capabilities.join(", ") || "—"],
  ];
  return (
    <div className="device-overview">
      <section className="panel">
        <div className="panel-heading">
          <div className="panel-title">
            <Icon name="info" />
            <h2>{t.details}</h2>
          </div>
        </div>
        <dl className="device-facts">
          {facts.map(([label, value]) => (
            <div key={label}>
              <dt>{label}</dt>
              <dd>{value}</dd>
            </div>
          ))}
        </dl>
      </section>
      <section className="panel">
        <div className="panel-heading">
          <div className="panel-title">
            <Icon name="activity" />
            <h2>{t.healthTitle}</h2>
          </div>
          {device.open_alerts > 0 && canMonitor && (
            <button
              type="button"
              className="heading-link"
              onClick={() => onTab("monitoring")}
            >
              {device.open_alerts} {t.alertCount}
              <Icon name="arrow" size={15} />
            </button>
          )}
        </div>
        <p className="panel-description">
          {device.health_reported_at === null
            ? t.noHealthYet
            : `${t.reported}: ${since(device.health_reported_at, language)}`}
        </p>
        {health.value ? (
          <CheckList
            checks={health.value.checks}
            language={language}
            t={t}
            detailed={false}
          />
        ) : health.error ? (
          <p className="form-error">{errorText(health.error, t)}</p>
        ) : (
          <p className="loading-line">{t.loading}</p>
        )}
        <div className="metric-list">
          {(
            [
              ["CPU", used.cpu],
              [t.memory, used.memory],
              [t.disk, used.disk],
            ] as const
          ).map(([label, value]) => (
            <div className="metric-row" key={label}>
              <div>
                <span>{label}</span>
                <strong>{value === null ? t.unavailable : `${value}%`}</strong>
              </div>
              <div className="metric-track" aria-hidden="true">
                <span
                  className={Number(value) >= 90 ? "metric-warning" : ""}
                  style={{ width: `${value ?? 0}%` }}
                />
              </div>
            </div>
          ))}
          {!device.online && <p className="muted">{t.metricsOffline}</p>}
        </div>
      </section>
    </div>
  );
}

function Sparkline({ values, label }: { values: number[]; label: string }) {
  if (values.length < 2) return null;
  const width = 240;
  const height = 48;
  const points = values
    .map(
      (value, index) =>
        `${((index / (values.length - 1)) * width).toFixed(1)},${(height - (Math.min(100, value) / 100) * height).toFixed(1)}`,
    )
    .join(" ");
  return (
    <svg
      className="sparkline"
      viewBox={`0 0 ${width} ${height}`}
      preserveAspectRatio="none"
      role="img"
      aria-label={label}
    >
      <polyline points={points} fill="none" />
    </svg>
  );
}

function Monitoring({
  device,
  t,
  language,
  tenant,
  fail,
  changes,
  csrf,
}: Section) {
  const loadHealth = useCallback(
    () => api.health(tenant, device.id),
    [tenant, device.id],
  );
  const loadMetrics = useCallback(
    () => api.metrics(tenant, device.id),
    [tenant, device.id],
  );
  const loadAlerts = useCallback(
    () => api.deviceAlerts(tenant, device.id),
    [tenant, device.id],
  );
  const health = useLive(loadHealth, changes, fail);
  const metrics = useLive(loadMetrics, changes, fail);
  const alerts = useLive(loadAlerts, changes, fail);
  return (
    <div className="device-monitoring">
      <section className="panel">
        <div className="panel-heading">
          <div className="panel-title">
            <Icon name="activity" />
            <h2>{t.healthTitle}</h2>
          </div>
          {health.value?.reported_at && (
            <span className="muted">
              {t.reported}: {since(health.value.reported_at, language)}
            </span>
          )}
        </div>
        {health.value ? (
          <CheckList
            checks={health.value.checks}
            language={language}
            t={t}
            detailed
          />
        ) : (
          <p className="loading-line">{t.loading}</p>
        )}
      </section>
      <section className="panel">
        <div className="panel-heading">
          <div className="panel-title">
            <Icon name="monitor" />
            <h2>{t.lastHour}</h2>
          </div>
        </div>
        {metrics.value ? (
          <MetricCharts view={metrics.value} t={t} language={language} />
        ) : (
          <p className="loading-line">{t.loading}</p>
        )}
      </section>
      <section className="panel device-alerts">
        <div className="panel-heading">
          <div className="panel-title">
            <Icon name="alert" />
            <h2>{t.alerts}</h2>
          </div>
        </div>
        {alerts.value ? (
          <AlertTable
            items={alerts.value}
            canAcknowledge={device.permissions.includes("alert.acknowledge")}
            tenant={tenant}
            csrf={csrf}
            t={t}
            language={language}
            fail={fail}
            onChanged={() => void alerts.reload()}
          />
        ) : (
          <p className="loading-line">{t.loading}</p>
        )}
      </section>
    </div>
  );
}

function MetricCharts({
  view,
  t,
  language,
}: {
  view: MetricsView;
  t: Copy;
  language: Language;
}) {
  if (view.recent.length === 0)
    return <p className="muted panel-description">{t.noMetrics}</p>;
  const latest = view.recent[view.recent.length - 1];
  const series = [
    {
      label: "CPU",
      values: view.recent.map((s) => s.cpu_permille / 10),
      now: `${Math.round(latest.cpu_permille / 10)}%`,
    },
    {
      label: t.memory,
      values: view.recent.map((s) =>
        s.memory_total ? (s.memory_used / s.memory_total) * 100 : 0,
      ),
      now: `${bytes(latest.memory_used, language)} / ${bytes(latest.memory_total, language)}`,
    },
    {
      label: t.disk,
      values: view.recent.map((s) =>
        s.disk_total ? (s.disk_used / s.disk_total) * 100 : 0,
      ),
      now: `${bytes(latest.disk_used, language)} / ${bytes(latest.disk_total, language)}`,
    },
  ];
  return (
    <div className="chart-list">
      {series.map((item) => (
        <div className="chart-row" key={item.label}>
          <div className="chart-head">
            <span>{item.label}</span>
            <strong>{item.now}</strong>
          </div>
          <Sparkline
            values={item.values}
            label={`${item.label}, ${t.lastHour}: ${item.now}`}
          />
        </div>
      ))}
      <small className="muted">
        {t.samples}: {view.recent.length} · {moment(latest.at, language)}
      </small>
    </div>
  );
}

export function AlertTable({
  items,
  canAcknowledge,
  tenant,
  csrf,
  t,
  language,
  fail,
  onChanged,
  onOpen,
}: {
  items: AlertView[];
  canAcknowledge: boolean | ((alert: AlertView) => boolean);
  tenant: string;
  csrf: string;
  t: Copy;
  language: Language;
  fail: (error: unknown) => boolean;
  onChanged: () => void;
  onOpen?: (device: string) => void;
}) {
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  if (items.length === 0)
    return (
      <div className="empty-alerts">
        <Icon name="check" />
        {t.noAlerts}
      </div>
    );
  const acknowledge = async (alert: AlertView) => {
    setBusy(alert.id);
    setError(null);
    try {
      await api.acknowledge(tenant, alert.id, csrf);
      onChanged();
    } catch (failure) {
      if (!fail(failure)) setError(errorText(failure, t));
    } finally {
      setBusy(null);
    }
  };
  return (
    <>
      {error && (
        <p className="form-error" role="alert">
          {error}
        </p>
      )}
      <ul className="alert-rows">
        {items.map((alert) => {
          const allowed =
            typeof canAcknowledge === "function"
              ? canAcknowledge(alert)
              : canAcknowledge;
          return (
            <li
              key={alert.id}
              className={alert.resolved_at ? "resolved" : undefined}
            >
              <span className={["alert-symbol", alert.severity].join(" ")}>
                <Icon name="alert" size={17} />
              </span>
              <span className="alert-content">
                <span className="alert-title">
                  {alertTitle(alert.kind, language)}
                  {onOpen && (
                    <button
                      type="button"
                      className="inline-link"
                      onClick={() => onOpen(alert.device_id)}
                    >
                      {alert.device_name}
                    </button>
                  )}
                </span>
                <span className="alert-detail">{alert.summary}</span>
                <span className="alert-meta">
                  <span className={["severity", alert.severity].join(" ")}>
                    {alert.severity === "critical" ? t.critical : t.warning}
                  </span>
                  {alert.resolved_at
                    ? `${t.resolvedAt}: ${moment(alert.resolved_at, language)}`
                    : `${t.openedAt}: ${moment(alert.opened_at, language)}`}
                  {alert.occurrences > 1 &&
                    ` · ${alert.occurrences} ${t.occurrences}`}
                  {alert.acknowledged_at &&
                    ` · ${t.acknowledgedAt}: ${moment(alert.acknowledged_at, language)}`}
                </span>
              </span>
              {!alert.resolved_at && !alert.acknowledged_at && allowed && (
                <button
                  type="button"
                  className="button secondary"
                  disabled={busy === alert.id}
                  onClick={() => void acknowledge(alert)}
                >
                  {t.acknowledge}
                </button>
              )}
            </li>
          );
        })}
      </ul>
    </>
  );
}

function Inventory({ device, t, language, tenant, fail, changes }: Section) {
  const load = useCallback(
    () => api.inventory(tenant, device.id),
    [tenant, device.id],
  );
  const inventory = useLive(load, changes, fail, 300_000);
  const [query, setQuery] = useState("");
  const grouped = useMemo(() => {
    const kinds = new Map<string, { key: string; value: string }[]>();
    for (const item of inventory.value?.items ?? []) {
      const list = kinds.get(item.kind) ?? [];
      list.push(item);
      kinds.set(item.kind, list);
    }
    return kinds;
  }, [inventory.value]);
  if (!inventory.value) return <p className="loading-line">{t.loading}</p>;
  if (inventory.value.items.length === 0)
    return (
      <div className="empty-state">
        <span className="empty-icon">
          <Icon name="box" size={25} />
        </span>
        <h3>{t.noInventoryTitle}</h3>
        <p>{t.noInventoryHint}</p>
      </div>
    );
  const value = (kind: string, key: string, text: string) => {
    if (kind === "hardware" && key === "memory_bytes")
      return bytes(Number(text), language);
    if (kind === "disk") {
      const [size, ...system] = text.split(" ");
      return `${bytes(Number(size), language)} ${system.join(" ")}`;
    }
    return text;
  };
  const normalized = query.trim().toLocaleLowerCase(language);
  const software = (grouped.get("software") ?? []).filter(
    (item) =>
      normalized === "" ||
      item.key.toLocaleLowerCase(language).includes(normalized) ||
      item.value.toLocaleLowerCase(language).includes(normalized),
  );
  return (
    <div className="device-inventory">
      <p className="muted">
        {t.inventoryReconciled}:{" "}
        {moment(inventory.value.reconciled_at, language) ?? "—"}
      </p>
      <div className="inventory-columns">
        {["hardware", "os", "disk", "admin"].map((kind) =>
          grouped.has(kind) ? (
            <section className="panel" key={kind}>
              <h2>{inventoryKinds[language][kind]}</h2>
              <dl className="device-facts">
                {(grouped.get(kind) ?? []).map((item) => (
                  <div key={item.key}>
                    <dt>{inventoryKeys[language][item.key] ?? item.key}</dt>
                    <dd>{value(kind, item.key, item.value) || "—"}</dd>
                  </div>
                ))}
              </dl>
            </section>
          ) : null,
        )}
      </div>
      {grouped.has("software") && (
        <section className="panel">
          <div className="panel-heading">
            <div className="panel-title">
              <Icon name="box" />
              <h2>{inventoryKinds[language].software}</h2>
              <span className="count-badge">
                {grouped.get("software")?.length}
              </span>
            </div>
            <label className="device-search">
              <Icon name="search" size={16} />
              <span className="sr-only">{t.softwareSearch}</span>
              <input
                type="search"
                placeholder={t.softwareSearch}
                value={query}
                onChange={(event) => setQuery(event.target.value)}
              />
            </label>
          </div>
          <div className="table-scroll software-scroll">
            <table className="device-table compact-table">
              <thead>
                <tr>
                  <th>{t.name}</th>
                  <th>{t.versionAndPublisher}</th>
                </tr>
              </thead>
              <tbody>
                {software.map((item) => (
                  <tr key={item.key}>
                    <td>{item.key}</td>
                    <td>{item.value}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </section>
      )}
    </div>
  );
}

function Policy({
  device,
  t,
  language,
  tenant,
  fail,
  changes,
  access,
  csrf,
}: Section) {
  const load = useCallback(
    () => api.policy(tenant, device.id),
    [tenant, device.id],
  );
  const settings = useLive(load, changes, fail);
  const manageGroups = access.tenant_wide.includes("access.manage");
  return (
    <div className="device-policy">
      <section className="panel">
        <div className="panel-heading">
          <div className="panel-title">
            <Icon name="settings" />
            <h2>{t.effectivePolicy}</h2>
          </div>
        </div>
        <p className="panel-description">{t.effectivePolicyHint}</p>
        {settings.value ? (
          <div className="table-scroll">
            <table className="device-table compact-table">
              <thead>
                <tr>
                  <th>{t.setting}</th>
                  <th>{t.value}</th>
                  <th>{t.source}</th>
                </tr>
              </thead>
              <tbody>
                {settings.value.map((setting) => (
                  <tr key={setting.key}>
                    <td>
                      {settingNames[language][setting.key] ?? setting.key}
                      <small className="cell-secondary">
                        <code>{setting.key}</code>
                      </small>
                    </td>
                    <td>
                      <strong>{setting.value}</strong>
                      {setting.value !== setting.default && (
                        <small className="cell-secondary">
                          {t.defaultValue}: {setting.default}
                        </small>
                      )}
                    </td>
                    <td>
                      {
                        {
                          default: t.sourceDefault,
                          tenant: t.sourceTenant,
                          site: t.sourceSite,
                          group: t.sourceGroup,
                        }[setting.source.level]
                      }
                      {setting.source.name && (
                        <small className="cell-secondary">
                          {setting.source.name}
                        </small>
                      )}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        ) : (
          <p className="loading-line">{t.loading}</p>
        )}
      </section>
      <Groups
        device={device}
        tenant={tenant}
        csrf={csrf}
        t={t}
        fail={fail}
        manage={manageGroups}
        onChanged={() => void settings.reload()}
      />
    </div>
  );
}

function Groups({
  device,
  tenant,
  csrf,
  t,
  fail,
  manage,
  onChanged,
}: {
  device: DeviceSummary;
  tenant: string;
  csrf: string;
  t: Copy;
  fail: (error: unknown) => boolean;
  manage: boolean;
  onChanged: () => void;
}) {
  const load = useCallback(
    () => (manage ? api.groups(tenant) : Promise.resolve([])),
    [tenant, manage],
  );
  const groups = useLive(load, 0, fail, 600_000);
  const [choice, setChoice] = useState("");
  const [error, setError] = useState<string | null>(null);
  const change = async (group: string, member: boolean) => {
    setError(null);
    try {
      await api.setGroupMember(tenant, group, device.id, member, csrf);
      setChoice("");
      onChanged();
    } catch (failure) {
      if (!fail(failure)) setError(errorText(failure, t));
    }
  };
  const available = (groups.value ?? []).filter(
    (group) => !device.groups.some((member) => member.id === group.id),
  );
  return (
    <section className="panel">
      <div className="panel-heading">
        <div className="panel-title">
          <Icon name="list" />
          <h2>{t.groups}</h2>
        </div>
      </div>
      <p className="panel-description">
        {manage ? t.groupsManageHint : t.groupsReadHint}
      </p>
      {error && (
        <p className="form-error" role="alert">
          {error}
        </p>
      )}
      {device.groups.length === 0 ? (
        <p className="muted">{t.noGroups}</p>
      ) : (
        <ul className="chip-list">
          {device.groups.map((group) => (
            <li key={group.id} className="rule-chip">
              {group.name}
              {manage && (
                <button
                  type="button"
                  aria-label={`${t.removeFromGroup}: ${group.name}`}
                  onClick={() => void change(group.id, false)}
                >
                  <Icon name="close" size={13} />
                </button>
              )}
            </li>
          ))}
        </ul>
      )}
      {manage && available.length > 0 && (
        <form
          className="inline-form"
          onSubmit={(event) => {
            event.preventDefault();
            if (choice) void change(choice, true);
          }}
        >
          <label className="field">
            <span>{t.addToGroup}</span>
            <select
              value={choice}
              onChange={(event) => setChoice(event.target.value)}
            >
              <option value="">—</option>
              {available.map((group) => (
                <option key={group.id} value={group.id}>
                  {group.name}
                </option>
              ))}
            </select>
          </label>
          <button type="submit" className="button secondary" disabled={!choice}>
            {t.add}
          </button>
          <small className="quick-note">
            <Icon name="key" size={13} />
            {t.needsFreshPasskey}
          </small>
        </form>
      )}
    </section>
  );
}
