import { useEffect, useRef, useState } from "react";
import {
  AlertList,
  ConnectionStatus,
  DeviceDialog,
  DeviceList,
  Stat,
} from "./components";
import { alerts, type Device, devices } from "./data";
import { Icon, type IconName } from "./Icon";
import { type Language, translations } from "./i18n";
import type { Preferences } from "./preferences";
import { useConnection } from "./useConnection";

type Page = "overview" | "devices" | "alerts" | "customers" | "settings";
const customers = [...new Set(devices.map((device) => device.customer))];

export function Preview({
  language,
  setLanguage,
  theme,
  setTheme,
}: Preferences) {
  const [page, setPage] = useState<Page>("overview");
  const [customer, setCustomer] = useState("all");
  const [query, setQuery] = useState("");
  const [kind, setKind] = useState("all");
  const [status, setStatus] = useState("all");
  const [system, setSystem] = useState("all");
  const [view, setView] = useState<"list" | "grid">("list");
  const [selected, setSelected] = useState<Device | null>(null);
  const search = useRef<HTMLInputElement>(null);
  const { health, check } = useConnection();
  const t = translations[language];

  useEffect(() => {
    document.title = ["MIDAS", translations[language][page]].join(" · ");
  }, [language, page]);

  useEffect(() => {
    const shortcut = (event: KeyboardEvent) => {
      if (
        (event.ctrlKey || event.metaKey) &&
        event.key.toLowerCase() === "k" &&
        !document.querySelector("dialog[open]")
      ) {
        event.preventDefault();
        search.current?.focus();
      }
    };
    document.addEventListener("keydown", shortcut);
    return () => document.removeEventListener("keydown", shortcut);
  }, []);

  const scoped = devices.filter(
    (device) => customer === "all" || device.customer === customer,
  );
  const normalized = query.trim().toLocaleLowerCase(language);
  const filtered = scoped.filter(
    (device) =>
      (kind === "all" || device.kind === kind) &&
      (status === "all" || device.status === status) &&
      (system === "all" || device.family === system) &&
      [device.name, device.customer, device.site, device.ip, device.os].some(
        (value) => value.toLocaleLowerCase(language).includes(normalized),
      ),
  );
  const scopedAlerts = alerts.filter((alert) =>
    scoped.some((device) => device.id === alert.deviceId),
  );
  const online = scoped.filter((device) => device.status !== "offline").length;
  const serverCount = scoped.filter(
    (device) => device.kind === "server",
  ).length;
  const description = {
    overview: t.overviewDescription,
    devices: t.devicesDescription,
    alerts: t.alertsDescription,
    customers: t.customersDescription,
    settings: t.settingsDescription,
  }[page];
  const clearFilters = () => {
    setQuery("");
    setKind("all");
    setStatus("all");
    setSystem("all");
  };
  const navigate = (next: Page) => {
    setPage(next);
    clearFilters();
  };
  const navigation = (target: Page, icon: IconName, count?: number) => (
    <button
      type="button"
      className={page === target ? "nav-item active" : "nav-item"}
      onClick={() => navigate(target)}
      aria-current={page === target ? "page" : undefined}
      title={t[target]}
    >
      <Icon name={icon} />
      <span>{t[target]}</span>
      {count !== undefined && <span className="nav-count">{count}</span>}
    </button>
  );

  return (
    <div className="app-shell">
      <a className="skip-link" href="#main">
        {t.skip}
      </a>
      <aside className="sidebar">
        <div className="brand">
          <span className="brand-symbol" aria-hidden="true">
            <i />
            <i />
            <i />
          </span>
          <div>
            <strong>MIDAS</strong>
            <span>{t.console}</span>
          </div>
        </div>
        <nav aria-label={t.console}>
          <div className="nav-group">
            <p className="nav-label">{t.operations}</p>
            {navigation("overview", "overview")}
            {navigation("devices", "monitor", devices.length)}
            {navigation("alerts", "alert", alerts.length)}
            <button type="button" className="nav-item planned" disabled>
              <Icon name="remote" />
              <span>{t.remote}</span>
              <small>{t.later}</small>
            </button>
            <button type="button" className="nav-item planned" disabled>
              <Icon name="bolt" />
              <span>{t.automation}</span>
              <small>{t.later}</small>
            </button>
          </div>
          <div className="nav-group">
            <p className="nav-label">{t.management}</p>
            {navigation("customers", "building")}
            {navigation("settings", "settings")}
          </div>
        </nav>
        <div className="sidebar-bottom">
          <ConnectionStatus
            health={health}
            t={t}
            language={language}
            onCheck={check}
          />
          <div className="operator">
            <span className="avatar">OP</span>
            <div>
              <strong>{t.operator}</strong>
              <small>{t.previewAccount}</small>
            </div>
            <Icon name="shield" />
          </div>
        </div>
      </aside>
      <div className="workspace">
        <header className="topbar">
          <div className="breadcrumbs">
            <span>{t.operations}</span>
            <Icon name="chevron" size={13} />
            <strong>{t[page]}</strong>
          </div>
          <div className="global-search">
            <Icon name="search" />
            <input
              ref={search}
              aria-label={t.searchLabel}
              placeholder={t.search}
              value={query}
              onChange={(event) => {
                setQuery(event.target.value);
                setPage("devices");
              }}
            />
            <kbd>Ctrl K</kbd>
          </div>
          <div className="topbar-tools">
            <label className="language-control">
              <Icon name="globe" />
              <span className="sr-only">{t.language}</span>
              <select
                value={language}
                onChange={(event) =>
                  setLanguage(event.target.value as Language)
                }
              >
                <option value="hu">HU</option>
                <option value="en">EN</option>
              </select>
            </label>
            <button
              type="button"
              className="icon-button theme-button"
              onClick={() => setTheme(theme === "light" ? "dark" : "light")}
              aria-label={theme === "light" ? t.dark : t.light}
              title={theme === "light" ? t.dark : t.light}
            >
              <Icon name={theme === "light" ? "moon" : "sun"} />
            </button>
            <span className="topbar-divider" />
            <span
              className="avatar small-avatar"
              role="img"
              aria-label={t.previewAccount}
            >
              OP
            </span>
          </div>
        </header>
        <main id="main" tabIndex={-1}>
          <div className="preview-banner">
            <span className="preview-tag">
              <Icon name="info" size={14} />
              {t.preview}
            </span>
            <span>{t.previewNote}</span>
          </div>
          <div className="page-heading">
            <div>
              <h1>{t[page]}</h1>
              <p className="page-description">{description}</p>
            </div>
            {page !== "settings" && page !== "customers" && (
              <label className="customer-select">
                <Icon name="building" />
                <span className="sr-only">{t.customerScope}</span>
                <select
                  value={customer}
                  onChange={(event) => setCustomer(event.target.value)}
                >
                  <option value="all">{t.allCustomers}</option>
                  {customers.map((name) => (
                    <option key={name}>{name}</option>
                  ))}
                </select>
              </label>
            )}
          </div>
          {(page === "overview" || page === "devices") && (
            <>
              <section className="stats-grid" aria-label={t.overview}>
                <Stat
                  title={t.managed}
                  value={scoped.length}
                  icon="monitor"
                  footer={
                    <>
                      {serverCount} {t.servers}
                      <span className="separator">·</span>
                      {scoped.length - serverCount} {t.workstations}
                    </>
                  }
                />
                <Stat
                  title={t.available}
                  value={online}
                  icon="check"
                  tone="positive"
                  footer={
                    <>
                      <span className="dot offline" />
                      {scoped.length - online} {t.offlineCount}
                    </>
                  }
                />
                <Stat
                  title={t.activeAlerts}
                  value={scopedAlerts.length}
                  icon="alert"
                  tone="warning"
                  footer={
                    <>
                      <span className="dot critical" />
                      {
                        scopedAlerts.filter(
                          (alert) => alert.severity === "critical",
                        ).length
                      }{" "}
                      {t.critical.toLocaleLowerCase(language)}
                    </>
                  }
                />
                <Stat
                  title={t.pendingUpdates}
                  value={scoped.reduce(
                    (sum, device) => sum + device.updates,
                    0,
                  )}
                  icon="refresh"
                  footer={
                    <>
                      {scoped.filter((device) => device.updates > 0).length}{" "}
                      {t.affectedDevices}
                    </>
                  }
                />
              </section>
              <div
                className={
                  page === "overview" ? "overview-grid" : "devices-section"
                }
              >
                <section className="panel inventory-panel">
                  <div className="panel-heading">
                    <div className="panel-title">
                      <Icon name="monitor" />
                      <h2>{t.inventory}</h2>
                      <span className="count-badge">{scoped.length}</span>
                    </div>
                    <fieldset className="view-switch" aria-label={t.inventory}>
                      <button
                        type="button"
                        aria-label={t.list}
                        aria-pressed={view === "list"}
                        onClick={() => setView("list")}
                      >
                        <Icon name="list" />
                      </button>
                      <button
                        type="button"
                        aria-label={t.grid}
                        aria-pressed={view === "grid"}
                        onClick={() => setView("grid")}
                      >
                        <Icon name="grid" />
                      </button>
                    </fieldset>
                  </div>
                  <div className="inventory-tools">
                    <div className="kind-switch">
                      {(["all", "workstation", "server"] as const).map(
                        (value) => (
                          <button
                            key={value}
                            type="button"
                            aria-pressed={kind === value}
                            onClick={() => setKind(value)}
                          >
                            {value === "all"
                              ? t.all
                              : value === "server"
                                ? t.serverFilter
                                : t.workstationFilter}
                            <span>
                              {value === "all"
                                ? scoped.length
                                : scoped.filter(
                                    (device) => device.kind === value,
                                  ).length}
                            </span>
                          </button>
                        ),
                      )}
                    </div>
                    <fieldset className="filters" aria-label={t.filters}>
                      <Icon name="filter" size={15} />
                      <select
                        aria-label={t.status}
                        value={status}
                        onChange={(event) => setStatus(event.target.value)}
                      >
                        <option value="all">{t.allStatuses}</option>
                        <option value="online">{t.online}</option>
                        <option value="attention">{t.attention}</option>
                        <option value="offline">{t.offline}</option>
                      </select>
                      <select
                        aria-label={t.operatingSystem}
                        value={system}
                        onChange={(event) => setSystem(event.target.value)}
                      >
                        <option value="all">{t.allSystems}</option>
                        <option value="windows">Windows</option>
                        <option value="linux">Linux</option>
                      </select>
                    </fieldset>
                  </div>
                  {filtered.length === 0 ? (
                    <div className="empty-state">
                      <span className="empty-icon">
                        <Icon name="search" size={25} />
                      </span>
                      <h3>{t.noResults}</h3>
                      <p>{t.noResultsHint}</p>
                      <button
                        type="button"
                        className="button secondary"
                        onClick={clearFilters}
                      >
                        {t.reset}
                      </button>
                    </div>
                  ) : (
                    <DeviceList
                      items={filtered}
                      view={view}
                      compact={page === "overview"}
                      t={t}
                      onSelect={setSelected}
                    />
                  )}
                  <div className="table-footer">
                    <span>
                      {filtered.length} / {scoped.length} {t.result}
                    </span>
                    <span className="sample-label">
                      <span className="dot sample-dot" />
                      {t.sample}
                    </span>
                  </div>
                </section>
                {page === "overview" && (
                  <aside className="overview-rail">
                    <section className="panel alerts-panel">
                      <div className="panel-heading">
                        <div className="panel-title">
                          <Icon name="alert" />
                          <h2>{t.attentionTitle}</h2>
                        </div>
                        <span className="count-badge warning-count">
                          {scopedAlerts.length}
                        </span>
                      </div>
                      <p className="panel-description">{t.attentionHint}</p>
                      <AlertList
                        items={scopedAlerts}
                        language={language}
                        t={t}
                        onSelect={setSelected}
                      />
                      <button
                        type="button"
                        className="panel-link"
                        onClick={() => navigate("alerts")}
                      >
                        {t.allAlerts}
                        <Icon name="arrow" size={15} />
                      </button>
                    </section>
                    <section className="panel distribution-panel">
                      <div className="panel-heading">
                        <h2>{t.distribution}</h2>
                        <Icon name="server" />
                      </div>
                      <div className="distribution-bar" aria-hidden="true">
                        <span style={{ flex: scoped.length - serverCount }} />
                        <span style={{ flex: serverCount }} />
                      </div>
                      <div className="distribution-legend">
                        <span>
                          <i className="legend-workstation" />
                          {t.workstationFilter}
                        </span>
                        <strong>{scoped.length - serverCount}</strong>
                        <span>
                          <i className="legend-server" />
                          {t.serverFilter}
                        </span>
                        <strong>{serverCount}</strong>
                      </div>
                    </section>
                  </aside>
                )}
              </div>
            </>
          )}
          {page === "alerts" && (
            <section className="panel full-alerts">
              <div className="panel-heading">
                <div className="panel-title">
                  <Icon name="alert" />
                  <h2>{t.activeAlerts}</h2>
                  <span className="count-badge">{scopedAlerts.length}</span>
                </div>
                <span className="sample-label">{t.sample}</span>
              </div>
              <AlertList
                items={scopedAlerts}
                language={language}
                t={t}
                onSelect={setSelected}
              />
            </section>
          )}
          {page === "customers" && (
            <div className="customer-grid">
              {customers.map((name) => {
                const fleet = devices.filter(
                  (device) => device.customer === name,
                );
                return (
                  <section className="panel customer-card" key={name}>
                    <span className="customer-icon">
                      <Icon name="building" size={24} />
                    </span>
                    <h2>{name}</h2>
                    <p>
                      {[...new Set(fleet.map((device) => device.site))].join(
                        " · ",
                      )}
                    </p>
                    <div className="customer-counts">
                      <div>
                        <strong>{fleet.length}</strong>
                        <span>{t.devices}</span>
                      </div>
                      <div>
                        <strong>
                          {
                            fleet.filter(
                              (device) => device.status !== "offline",
                            ).length
                          }
                        </strong>
                        <span>{t.available}</span>
                      </div>
                    </div>
                    <button
                      type="button"
                      className="panel-link"
                      onClick={() => {
                        setCustomer(name);
                        navigate("devices");
                      }}
                    >
                      {t.fleet}
                      <Icon name="arrow" size={16} />
                    </button>
                  </section>
                );
              })}
            </div>
          )}
          {page === "settings" && (
            <div className="settings-grid">
              <section className="panel settings-panel">
                <h2>{t.appearance}</h2>
                <p className="muted">{t.appearanceHint}</p>
                <h3>{t.theme}</h3>
                <fieldset className="theme-options" aria-label={t.chooseTheme}>
                  {(["light", "dark"] as const).map((value) => (
                    <button
                      key={value}
                      type="button"
                      className="theme-option"
                      aria-pressed={theme === value}
                      onClick={() => setTheme(value)}
                    >
                      <span className={["theme-swatch", value].join(" ")}>
                        <i />
                        <i />
                        <i />
                      </span>
                      <span>
                        <Icon name={value === "light" ? "sun" : "moon"} />
                        {t[value]}
                        {theme === value && <Icon name="check" size={16} />}
                      </span>
                    </button>
                  ))}
                </fieldset>
                <label className="setting-language">
                  {t.language}
                  <select
                    value={language}
                    onChange={(event) =>
                      setLanguage(event.target.value as Language)
                    }
                  >
                    <option value="hu">Magyar</option>
                    <option value="en">English</option>
                  </select>
                </label>
              </section>
              <section className="panel settings-panel">
                <h2>{t.connection}</h2>
                <p className="muted">{t.connectionHint}</p>
                <ConnectionStatus
                  health={health}
                  t={t}
                  language={language}
                  onCheck={check}
                />
              </section>
            </div>
          )}
          <footer className="page-footer">
            <span>
              MIDAS <span className="separator">/</span> {t.console}
            </span>
            <span>{t.previewAccount}</span>
          </footer>
        </main>
      </div>
      {selected && (
        <DeviceDialog
          device={selected}
          t={t}
          onClose={() => setSelected(null)}
        />
      )}
    </div>
  );
}
