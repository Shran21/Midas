import {
  type KeyboardEvent,
  type MouseEvent,
  useCallback,
  useMemo,
  useState,
} from "react";
import { api, type DeviceSummary, type TenantContext } from "../api";
import { Icon } from "../Icon";
import type { Copy, Language } from "../i18n";
import { remember, stored } from "../preferences";
import { errorText } from "../SignIn";
import {
  DeviceMenu,
  type MenuState,
  type Tab,
  useDeviceActions,
} from "./DeviceMenu";
import { since } from "./labels";
import {
  type Condition,
  condition,
  deviceActions,
  type Filters,
  filtered,
  noFilters,
  percent,
  scopes,
} from "./model";
import { useLive } from "./useLive";

export function ConditionBadge({
  device,
  t,
}: {
  device: DeviceSummary;
  t: Copy;
}) {
  const value = condition(device);
  return (
    <span className={["status-badge", value].join(" ")}>
      <span className={["dot", value].join(" ")} />
      {conditionLabel(value, t)}
    </span>
  );
}

export function conditionLabel(value: Condition, t: Copy) {
  return {
    online: t.online,
    attention: t.attention,
    offline: t.offline,
    revoked: t.revokedState,
  }[value];
}

function Meter({
  value,
  label,
  visible = false,
}: {
  value: number | null;
  label: string;
  visible?: boolean;
}) {
  const text = value === null ? "—" : `${value}%`;
  return (
    <span className="mini-meter" role="img" aria-label={`${label}: ${text}`}>
      {visible && (
        <span className="meter-label" aria-hidden="true">
          {label}
        </span>
      )}
      <span className="mini-track" aria-hidden="true">
        <span
          className={value !== null && value >= 90 ? "high" : undefined}
          style={{ width: `${value ?? 0}%` }}
        />
      </span>
      <span>{text}</span>
    </span>
  );
}

export function usage(device: DeviceSummary) {
  const metrics = device.online ? device.metrics : null;
  return {
    cpu: metrics ? Math.round(metrics.cpu_permille / 10) : null,
    memory: metrics ? percent(metrics.memory_used, metrics.memory_total) : null,
    disk: metrics ? percent(metrics.disk_used, metrics.disk_total) : null,
  };
}

export function DevicesView({
  tenant,
  tenantName,
  access,
  t,
  language,
  csrf,
  fail,
  changes,
  onOpen,
}: {
  tenant: string;
  tenantName: string;
  access: TenantContext;
  t: Copy;
  language: Language;
  csrf: string;
  fail: (error: unknown) => boolean;
  changes: number;
  onOpen: (device: string, tab: Tab) => void;
}) {
  const load = useCallback(() => api.devices(tenant), [tenant]);
  const live = useLive(load, changes, fail);
  const [filters, setFilters] = useState<Filters>(noFilters);
  const [view, setView] = useState<"list" | "grid">(
    () => stored("midas.devices.view", ["list", "grid"], "list") as "list",
  );
  const [menu, setMenu] = useState<MenuState | null>(null);
  const actions = useDeviceActions({
    tenant,
    tenantName,
    csrf,
    t,
    fail,
    onOpen,
    onChanged: () => void live.reload(),
  });
  const devices = live.value ?? [];
  const shown = useMemo(
    () => filtered(devices, filters, language),
    [devices, filters, language],
  );
  const { sites, groups } = useMemo(() => scopes(devices), [devices]);
  const closeMenu = useCallback(() => setMenu(null), []);
  const set = (changes: Partial<Filters>) =>
    setFilters((current) => ({ ...current, ...changes }));
  const choose = (next: "list" | "grid") => {
    setView(next);
    remember("midas.devices.view", next);
  };

  const openMenu = (
    device: DeviceSummary,
    event: MouseEvent<HTMLElement>,
    fromButton: boolean,
  ) => {
    event.preventDefault();
    const element = event.currentTarget;
    const box = element.getBoundingClientRect();
    setMenu({
      device,
      actions: deviceActions(device, access),
      x: fromButton ? box.right - 240 : event.clientX,
      y: fromButton ? box.bottom + 4 : event.clientY,
      opener: fromButton
        ? element
        : element.querySelector<HTMLElement>(".menu-button"),
    });
  };
  const keyMenu = (
    device: DeviceSummary,
    event: KeyboardEvent<HTMLElement>,
  ) => {
    if (
      event.key === "ContextMenu" ||
      (event.shiftKey && event.key === "F10")
    ) {
      event.preventDefault();
      const element = event.currentTarget;
      const box = element.getBoundingClientRect();
      setMenu({
        device,
        actions: deviceActions(device, access),
        x: box.left + 24,
        y: box.top + 24,
        opener: element.querySelector<HTMLElement>(".menu-button"),
      });
    }
  };

  if (live.value === undefined) {
    if (live.error)
      return (
        <div className="empty-state" role="alert">
          <span className="empty-icon">
            <Icon name="info" size={25} />
          </span>
          <h3>{t.unavailableTitle}</h3>
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

  const online = devices.filter((d) => d.online && d.enrollment === "active");
  const alerting = devices.filter((d) => d.open_alerts > 0);
  const restart = devices.filter((d) => d.restart_pending).length;
  const servers = devices.filter((d) => d.kind === "server").length;
  const menuButton = (device: DeviceSummary) => (
    <button
      type="button"
      className="icon-button menu-button"
      aria-haspopup="menu"
      aria-expanded={menu?.device.id === device.id}
      aria-label={`${t.actions}: ${device.name}`}
      title={t.actions}
      onClick={(event) => openMenu(device, event, true)}
    >
      <Icon name="more" size={18} />
    </button>
  );

  return (
    <>
      <section className="stats-grid" aria-label={t.devices}>
        <article className="stat-card">
          <div className="stat-top">
            <h2>{t.managed}</h2>
            <span className="stat-icon">
              <Icon name="monitor" />
            </span>
          </div>
          <strong className="stat-value">{devices.length}</strong>
          <div className="stat-footer">
            {servers} {t.servers}
            <span className="separator">·</span>
            {devices.length - servers} {t.workstations}
          </div>
        </article>
        <article className="stat-card positive">
          <div className="stat-top">
            <h2>{t.available}</h2>
            <span className="stat-icon">
              <Icon name="check" />
            </span>
          </div>
          <strong className="stat-value">{online.length}</strong>
          <div className="stat-footer">
            <span className="dot offline" />
            {devices.length - online.length} {t.offlineCount}
          </div>
        </article>
        <button
          type="button"
          className="stat-card warning stat-filter"
          aria-pressed={filters.alerts}
          onClick={() => set({ alerts: !filters.alerts })}
        >
          <span className="stat-top">
            <span className="stat-label">{t.withAlerts}</span>
            <span className="stat-icon">
              <Icon name="alert" />
            </span>
          </span>
          <strong className="stat-value">{alerting.length}</strong>
          <span className="stat-footer">
            <span className="dot critical" />
            {devices.reduce((sum, d) => sum + d.critical_alerts, 0)}{" "}
            {t.critical.toLocaleLowerCase(language)}
          </span>
        </button>
        <button
          type="button"
          className="stat-card stat-filter"
          aria-pressed={filters.restart}
          onClick={() => set({ restart: !filters.restart })}
        >
          <span className="stat-top">
            <span className="stat-label">{t.restartPending}</span>
            <span className="stat-icon">
              <Icon name="refresh" />
            </span>
          </span>
          <strong className="stat-value">{restart}</strong>
          <span className="stat-footer">{t.restartFilterHint}</span>
        </button>
      </section>
      {actions.element}
      <section className="panel inventory-panel">
        <div className="panel-heading">
          <div className="panel-title">
            <Icon name="monitor" />
            <h2>{t.inventory}</h2>
            <span className="count-badge">{devices.length}</span>
            <span className="live-state" role="status">
              <span className={live.error ? "dot critical" : "dot online"} />
              {live.error
                ? t.liveStale
                : `${t.liveUpdated} ${since(live.loadedAt, language) ?? ""}`}
            </span>
          </div>
          <fieldset className="view-switch" aria-label={t.layout}>
            <button
              type="button"
              aria-label={t.list}
              aria-pressed={view === "list"}
              onClick={() => choose("list")}
            >
              <Icon name="list" />
            </button>
            <button
              type="button"
              aria-label={t.grid}
              aria-pressed={view === "grid"}
              onClick={() => choose("grid")}
            >
              <Icon name="grid" />
            </button>
          </fieldset>
        </div>
        <div className="inventory-tools">
          <label className="device-search">
            <Icon name="search" size={16} />
            <span className="sr-only">{t.searchLabel}</span>
            <input
              type="search"
              placeholder={t.deviceSearch}
              value={filters.query}
              onChange={(event) => set({ query: event.target.value })}
            />
          </label>
          <div className="kind-switch">
            {(["all", "workstation", "server"] as const).map((value) => (
              <button
                key={value}
                type="button"
                aria-pressed={filters.kind === value}
                onClick={() => set({ kind: value })}
              >
                {value === "all"
                  ? t.all
                  : value === "server"
                    ? t.serverFilter
                    : t.workstationFilter}
                <span>
                  {value === "all"
                    ? devices.length
                    : devices.filter((d) => d.kind === value).length}
                </span>
              </button>
            ))}
          </div>
          <fieldset className="filters" aria-label={t.filters}>
            <Icon name="filter" size={15} />
            <select
              aria-label={t.status}
              value={filters.condition}
              onChange={(event) =>
                set({ condition: event.target.value as Filters["condition"] })
              }
            >
              <option value="all">{t.allStatuses}</option>
              <option value="online">{t.online}</option>
              <option value="attention">{t.attention}</option>
              <option value="offline">{t.offline}</option>
              <option value="revoked">{t.revokedState}</option>
            </select>
            <select
              aria-label={t.operatingSystem}
              value={filters.platform}
              onChange={(event) =>
                set({ platform: event.target.value as Filters["platform"] })
              }
            >
              <option value="all">{t.allSystems}</option>
              <option value="windows">Windows</option>
              <option value="linux">Linux</option>
            </select>
            <select
              aria-label={t.site}
              value={filters.site}
              onChange={(event) => set({ site: event.target.value })}
            >
              <option value="all">{t.allSites}</option>
              <option value="none">{t.noSite}</option>
              {sites.map((site) => (
                <option key={site.id} value={site.id}>
                  {site.name}
                </option>
              ))}
            </select>
            {groups.length > 0 && (
              <select
                aria-label={t.group}
                value={filters.group}
                onChange={(event) => set({ group: event.target.value })}
              >
                <option value="all">{t.allGroups}</option>
                {groups.map((group) => (
                  <option key={group.id} value={group.id}>
                    {group.name}
                  </option>
                ))}
              </select>
            )}
          </fieldset>
        </div>
        {devices.length === 0 ? (
          <div className="empty-state">
            <span className="empty-icon">
              <Icon name="monitor" size={25} />
            </span>
            <h3>{t.noDevicesTitle}</h3>
            <p>{t.noDevicesHint}</p>
          </div>
        ) : shown.length === 0 ? (
          <div className="empty-state">
            <span className="empty-icon">
              <Icon name="search" size={25} />
            </span>
            <h3>{t.noResults}</h3>
            <p>{t.noResultsHint}</p>
            <button
              type="button"
              className="button secondary"
              onClick={() => setFilters(noFilters)}
            >
              {t.reset}
            </button>
          </div>
        ) : view === "grid" ? (
          <ul className="device-grid live-grid">
            {shown.map((device) => {
              const used = usage(device);
              return (
                <li
                  key={device.id}
                  className="device-card"
                  onContextMenu={(event) => openMenu(device, event, false)}
                  onKeyDown={(event) => keyMenu(device, event)}
                >
                  <span className="device-card-heading">
                    <span className="device-icon">
                      <Icon
                        name={device.kind === "server" ? "server" : "monitor"}
                      />
                    </span>
                    <ConditionBadge device={device} t={t} />
                    {menuButton(device)}
                  </span>
                  <button
                    type="button"
                    className="device-card-open"
                    onClick={() => onOpen(device.id, "overview")}
                  >
                    <strong className="card-name">{device.name}</strong>
                    <span className="muted">
                      {device.os ?? platformName(device)}
                    </span>
                  </button>
                  <span className="device-card-location">
                    <Icon name="building" size={14} />
                    {device.site?.name ?? t.noSite}
                    {device.groups.length > 0 && (
                      <span>
                        {" "}
                        · {device.groups.map((g) => g.name).join(", ")}
                      </span>
                    )}
                  </span>
                  <span className="card-meters">
                    <Meter label="CPU" value={used.cpu} visible />
                    <Meter label={t.memory} value={used.memory} visible />
                    <Meter label={t.disk} value={used.disk} visible />
                  </span>
                  <span className="card-footer">
                    <span className="seen">
                      <Icon name="clock" size={13} />
                      {device.online
                        ? t.now
                        : (since(device.last_seen_at, language) ?? t.never)}
                    </span>
                    <span className="card-flags">
                      {device.restart_pending && (
                        <span className="flag">{t.restartPending}</span>
                      )}
                      {device.open_alerts > 0 && (
                        <span className="flag alert-flag">
                          {device.open_alerts} {t.alertCount}
                        </span>
                      )}
                    </span>
                  </span>
                </li>
              );
            })}
          </ul>
        ) : (
          <div className="table-scroll">
            <table className="device-table">
              <thead>
                <tr>
                  <th>{t.device}</th>
                  <th>
                    {t.site} / {t.group}
                  </th>
                  <th>{t.status}</th>
                  <th className="resource-column">{t.resources}</th>
                  <th className="seen-column">{t.lastSeen}</th>
                  <th>
                    <span className="sr-only">{t.actions}</span>
                  </th>
                </tr>
              </thead>
              <tbody>
                {shown.map((device) => {
                  const used = usage(device);
                  return (
                    <tr
                      key={device.id}
                      onContextMenu={(event) => openMenu(device, event, false)}
                      onKeyDown={(event) => keyMenu(device, event)}
                    >
                      <td>
                        <button
                          type="button"
                          className="device-link"
                          onClick={() => onOpen(device.id, "overview")}
                        >
                          <span className="device-icon">
                            <Icon
                              name={
                                device.kind === "server" ? "server" : "monitor"
                              }
                            />
                          </span>
                          <span>
                            <strong>{device.name}</strong>
                            <small>{device.os ?? platformName(device)}</small>
                          </span>
                        </button>
                      </td>
                      <td>
                        <span className="cell-primary">
                          {device.site?.name ?? t.noSite}
                        </span>
                        <small className="cell-secondary">
                          {device.groups.map((g) => g.name).join(", ") || "—"}
                        </small>
                      </td>
                      <td>
                        <ConditionBadge device={device} t={t} />
                        {(device.restart_pending || device.open_alerts > 0) && (
                          <small className="cell-secondary">
                            {[
                              device.open_alerts > 0
                                ? `${device.open_alerts} ${t.alertCount}`
                                : null,
                              device.restart_pending ? t.restartPending : null,
                            ]
                              .filter(Boolean)
                              .join(" · ")}
                          </small>
                        )}
                      </td>
                      <td className="resource-column">
                        <div className="resource-pair">
                          <Meter label="CPU" value={used.cpu} />
                          <Meter label={t.memory} value={used.memory} />
                        </div>
                      </td>
                      <td className="seen-column">
                        <span className="seen">
                          <Icon name="clock" size={13} />
                          {device.online
                            ? t.now
                            : (since(device.last_seen_at, language) ?? t.never)}
                        </span>
                      </td>
                      <td>{menuButton(device)}</td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        )}
        <div className="table-footer">
          <span>
            {shown.length} / {devices.length} {t.result}
          </span>
          <span className="muted">{t.contextMenuHint}</span>
        </div>
      </section>
      {menu && (
        <DeviceMenu
          menu={menu}
          tenantName={tenantName}
          t={t}
          onAction={(device, action) => void actions.run(device, action)}
          onClose={closeMenu}
        />
      )}
    </>
  );
}

export function platformName(device: DeviceSummary) {
  return device.platform === "windows" ? "Windows" : "Linux";
}
