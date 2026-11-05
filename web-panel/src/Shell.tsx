import {
  type ReactNode,
  useCallback,
  useEffect,
  useRef,
  useState,
} from "react";
import {
  ApiError,
  api,
  type Session,
  type SignedIn,
  type Tenant,
  type TenantContext,
  watchSession,
} from "./api";
import { Icon, type IconName } from "./Icon";
import { type Copy, type Language, translations } from "./i18n";
import { type Preferences, remember, stored } from "./preferences";
import { errorText, PreferenceControls } from "./SignIn";

export type SignOutReason = "ended" | "signed-out";

type Section =
  | "overview"
  | "devices"
  | "alerts"
  | "automation"
  | "access"
  | "audit"
  | "sessions";

const requirements: Record<Section, string[]> = {
  overview: [],
  devices: ["device.read"],
  alerts: ["monitor.read"],
  automation: ["script.execute", "updates.manage"],
  access: ["access.manage"],
  audit: ["audit.read"],
  sessions: [],
};
const tenantSections: [Section, IconName][] = [
  ["overview", "overview"],
  ["devices", "monitor"],
  ["alerts", "alert"],
  ["automation", "bolt"],
  ["access", "shield"],
  ["audit", "list"],
];

const permissionNames: Record<Language, Record<string, string>> = {
  hu: {
    "device.read": "Eszközadatok megtekintése",
    "monitor.read": "Monitorozás megtekintése",
    "remote.view": "Képernyő megtekintése",
    "remote.control": "Normál távvezérlés",
    "remote.admin": "Rendszergazdai távvezérlés",
    "remote.uac.interact": "UAC kezelése",
    "remote.unattended": "Felügyelet nélküli szerverelérés",
    "terminal.execute": "Háttérterminál",
    "files.read": "Fájlok olvasása",
    "files.write": "Fájlok módosítása",
    "script.execute": "Szkriptfuttatás",
    "updates.manage": "Frissítések kezelése",
    "domain.join": "Tartományba léptetés",
    "access.manage": "Hozzáférés kezelése",
    "audit.read": "Audit olvasása",
  },
  en: {
    "device.read": "View device data",
    "monitor.read": "View monitoring",
    "remote.view": "View screen",
    "remote.control": "Normal remote control",
    "remote.admin": "Administrative remote control",
    "remote.uac.interact": "Interact with UAC",
    "remote.unattended": "Unattended server access",
    "terminal.execute": "Background terminal",
    "files.read": "Read files",
    "files.write": "Modify files",
    "script.execute": "Run scripts",
    "updates.manage": "Manage updates",
    "domain.join": "Join domain",
    "access.manage": "Manage access",
    "audit.read": "Read audit",
  },
};

type Load<T> =
  | { state: "loading" }
  | { state: "ready"; value: T }
  | { state: "missing" }
  | { state: "error"; error: unknown };

function initials(name: string) {
  return (
    name
      .split(/\s+/)
      .filter(Boolean)
      .slice(0, 2)
      .map((part) => part[0]?.toLocaleUpperCase())
      .join("") || "?"
  );
}

function Empty({
  icon,
  title,
  hint,
  action,
}: {
  icon: IconName;
  title: string;
  hint: string;
  action?: ReactNode;
}) {
  return (
    <div className="empty-state" role="status">
      <span className="empty-icon">
        <Icon name={icon} size={25} />
      </span>
      <h3>{title}</h3>
      <p>{hint}</p>
      {action}
    </div>
  );
}

export function Shell({
  preferences,
  signedIn,
  onSignedOut,
}: {
  preferences: Preferences;
  signedIn: SignedIn;
  onSignedOut: (reason: SignOutReason) => void;
}) {
  const { language } = preferences;
  const t = translations[language];
  const operator = signedIn.session.operator_id;
  const tenantKey = `midas.tenant.${operator}`;
  const [tenants, setTenants] = useState<Load<Tenant[]>>({ state: "loading" });
  const [tenantId, setTenantId] = useState<string | null>(null);
  const [context, setContext] = useState<Load<TenantContext>>({
    state: "loading",
  });
  const [section, setSection] = useState<Section>("overview");
  const [reload, setReload] = useState(0);
  const heading = useRef<HTMLHeadingElement>(null);
  const firstRender = useRef(true);

  const fail = useCallback(
    (error: unknown) => {
      if (error instanceof ApiError && error.status === 401) {
        onSignedOut("ended");
        return true;
      }
      return false;
    },
    [onSignedOut],
  );

  // biome-ignore lint/correctness/useExhaustiveDependencies: the reload counter re-requests the data
  useEffect(() => {
    let active = true;
    setTenants({ state: "loading" });
    api.tenants().then(
      (items) => {
        if (!active) return;
        setTenants({ state: "ready", value: items });
        const saved = stored(
          tenantKey,
          items.map((item) => item.id),
          "",
        );
        setTenantId(saved || items[0]?.id || null);
      },
      (error) => {
        if (active && !fail(error)) setTenants({ state: "error", error });
      },
    );
    return () => {
      active = false;
    };
  }, [tenantKey, reload, fail]);

  // biome-ignore lint/correctness/useExhaustiveDependencies: the reload counter re-requests the data
  useEffect(() => {
    if (!tenantId) return;
    let active = true;
    remember(tenantKey, tenantId);
    setContext({ state: "loading" });
    api.tenant(tenantId).then(
      (value) => active && setContext({ state: "ready", value }),
      (error) => {
        if (!active || fail(error)) return;
        setContext(
          error instanceof ApiError && error.status === 404
            ? { state: "missing" }
            : { state: "error", error },
        );
      },
    );
    return () => {
      active = false;
    };
  }, [tenantId, tenantKey, reload, fail]);

  useEffect(() => {
    let stop = () => {};
    let active = true;
    const connect = () => {
      stop = watchSession(signedIn.csrf_token, () => {
        api.session().then(
          () => {
            if (active) setTimeout(connect, 5000);
          },
          (error) => {
            if (active && !fail(error)) setTimeout(connect, 5000);
          },
        );
      });
    };
    connect();
    return () => {
      active = false;
      stop();
    };
  }, [signedIn.csrf_token, fail]);

  const capabilities =
    context.state === "ready" ? context.value.capabilities : [];
  const allowed = (target: Section) =>
    requirements[target].length === 0 ||
    requirements[target].some((permission) =>
      capabilities.includes(permission),
    );
  const tenantName =
    tenants.state === "ready"
      ? tenants.value.find((item) => item.id === tenantId)?.name
      : undefined;
  const title = t[section];

  useEffect(() => {
    document.title = ["MIDAS", tenantName, title].filter(Boolean).join(" · ");
  }, [tenantName, title]);
  // biome-ignore lint/correctness/useExhaustiveDependencies: focus moves to the heading after navigation
  useEffect(() => {
    if (firstRender.current) {
      firstRender.current = false;
      return;
    }
    heading.current?.focus();
  }, [section]);

  const signOut = async () => {
    try {
      await api.logout(signedIn.csrf_token);
    } catch {}
    onSignedOut("signed-out");
  };

  const navItem = (target: Section, icon: IconName) => {
    if (!allowed(target) && target !== section) return null;
    return (
      <button
        key={target}
        type="button"
        className={section === target ? "nav-item active" : "nav-item"}
        onClick={() => setSection(target)}
        aria-current={section === target ? "page" : undefined}
      >
        <Icon name={icon} />
        <span>{t[target]}</span>
      </button>
    );
  };

  const description: Record<Section, string> = {
    overview: t.signedOverviewDescription,
    devices: t.devicesDescription,
    alerts: t.alertsDescription,
    automation: t.automationDescription,
    access: t.accessDescription,
    audit: t.auditDescription,
    sessions: t.sessionsDescription,
  };

  let content: ReactNode;
  if (section === "sessions") {
    content = (
      <Sessions
        t={t}
        language={language}
        current={signedIn}
        fail={fail}
        onSignedOut={onSignedOut}
      />
    );
  } else if (tenants.state === "loading") {
    content = <p className="loading-line">{t.loading}</p>;
  } else if (tenants.state === "error") {
    content = (
      <Empty
        icon="info"
        title={t.unavailableTitle}
        hint={errorText(tenants.error, t)}
        action={
          <button
            type="button"
            className="button secondary"
            onClick={() => setReload((n) => n + 1)}
          >
            {t.retry}
          </button>
        }
      />
    );
  } else if (tenants.state !== "ready" || tenants.value.length === 0) {
    content = (
      <Empty icon="building" title={t.noTenantsTitle} hint={t.noTenantsHint} />
    );
  } else if (context.state === "loading") {
    content = <p className="loading-line">{t.loading}</p>;
  } else if (context.state === "missing" || context.state === "error") {
    content = (
      <Empty
        icon="info"
        title={t.tenantErrorTitle}
        hint={
          context.state === "missing"
            ? t.tenantMissing
            : errorText(context.error, t)
        }
        action={
          <button
            type="button"
            className="button secondary"
            onClick={() => setReload((n) => n + 1)}
          >
            {t.retry}
          </button>
        }
      />
    );
  } else if (!allowed(section)) {
    content = (
      <Empty
        icon="shield"
        title={t.noPermissionTitle}
        hint={t.noPermissionHint}
        action={
          <button
            type="button"
            className="button secondary"
            onClick={() => setSection("overview")}
          >
            {t.overview}
          </button>
        }
      />
    );
  } else if (section === "overview") {
    content = (
      <section className="panel permissions-panel">
        <div className="panel-heading">
          <div className="panel-title">
            <Icon name="shield" />
            <h2>{t.permissionsTitle}</h2>
            <span className="count-badge">{capabilities.length}</span>
          </div>
        </div>
        <p className="panel-description">{t.permissionsHint}</p>
        {capabilities.length === 0 ? (
          <p className="muted permissions-empty">{t.noPermissions}</p>
        ) : (
          <ul className="permission-list">
            {capabilities.map((permission) => (
              <li key={permission}>
                <Icon name="check" size={15} />
                {permissionNames[language][permission] ?? permission}
              </li>
            ))}
          </ul>
        )}
      </section>
    );
  } else if (section === "devices") {
    content = (
      <Empty icon="monitor" title={t.noDevicesTitle} hint={t.noDevicesHint} />
    );
  } else if (section === "alerts") {
    content = (
      <Empty icon="alert" title={t.noAlertsTitle} hint={t.noAlertsHint} />
    );
  } else {
    content = (
      <Empty
        icon="clock"
        title={t.sectionPendingTitle}
        hint={t.sectionPendingHint}
      />
    );
  }

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
            <p className="nav-label">{tenantName ?? t.tenant}</p>
            {tenantSections.map(([target, icon]) => navItem(target, icon))}
          </div>
          <div className="nav-group">
            <p className="nav-label">{t.account}</p>
            {navItem("sessions", "clock")}
          </div>
        </nav>
        <div className="sidebar-bottom">
          <div className="operator">
            <span className="avatar" aria-hidden="true">
              {initials(signedIn.operator.display_name)}
            </span>
            <div>
              <strong>{signedIn.operator.display_name}</strong>
              <small>{signedIn.operator.login}</small>
            </div>
          </div>
        </div>
      </aside>
      <div className="workspace">
        <header className="topbar">
          <div className="breadcrumbs">
            <span>{tenantName ?? t.account}</span>
            <Icon name="chevron" size={13} />
            <strong>{title}</strong>
          </div>
          {tenants.state === "ready" && tenants.value.length > 0 && (
            <label className="customer-select tenant-select">
              <Icon name="building" />
              <span className="sr-only">{t.chooseTenant}</span>
              <select
                value={tenantId ?? ""}
                onChange={(event) => setTenantId(event.target.value)}
              >
                {tenants.value.map((item) => (
                  <option key={item.id} value={item.id}>
                    {item.name}
                  </option>
                ))}
              </select>
            </label>
          )}
          <div className="topbar-tools">
            <PreferenceControls {...preferences} />
            <span className="topbar-divider" />
            <span className="sr-only">
              {t.signedInAs}: {signedIn.operator.display_name}
            </span>
            <button
              type="button"
              className="button secondary signout-button"
              onClick={signOut}
            >
              <Icon name="arrow" size={15} />
              {t.signOut}
            </button>
          </div>
        </header>
        <main id="main" tabIndex={-1}>
          <div className="page-heading">
            <div>
              {section !== "sessions" && tenantName && (
                <p className="eyebrow">
                  {t.tenant}: {tenantName}
                </p>
              )}
              <h1 ref={heading} tabIndex={-1}>
                {title}
              </h1>
              <p className="page-description">{description[section]}</p>
            </div>
          </div>
          {content}
        </main>
      </div>
    </div>
  );
}

function Sessions({
  t,
  language,
  current,
  fail,
  onSignedOut,
}: {
  t: Copy;
  language: Language;
  current: SignedIn;
  fail: (error: unknown) => boolean;
  onSignedOut: (reason: SignOutReason) => void;
}) {
  const [items, setItems] = useState<Load<Session[]>>({ state: "loading" });
  const [error, setError] = useState<string | null>(null);
  const [reload, setReload] = useState(0);
  const format = new Intl.DateTimeFormat(language, {
    dateStyle: "medium",
    timeStyle: "short",
  });
  const time = (seconds: number) => format.format(new Date(seconds * 1000));
  const methods = {
    passkey: t.sessionMethodPasskey,
    password_totp: t.sessionMethodPassword,
    recovery_code: t.sessionMethodRecovery,
  };

  // biome-ignore lint/correctness/useExhaustiveDependencies: the reload counter re-requests the data
  useEffect(() => {
    let active = true;
    setItems({ state: "loading" });
    api.sessions().then(
      (value) => active && setItems({ state: "ready", value }),
      (failure) => {
        if (active && !fail(failure))
          setItems({ state: "error", error: failure });
      },
    );
    return () => {
      active = false;
    };
  }, [reload, fail]);

  const revoke = async (id: string) => {
    setError(null);
    try {
      await api.revoke(id, current.csrf_token);
      if (id === current.session.id) onSignedOut("signed-out");
      else setReload((n) => n + 1);
    } catch (failure) {
      if (!fail(failure)) setError(t.revokeFailed);
    }
  };

  if (items.state === "loading")
    return <p className="loading-line">{t.loading}</p>;
  if (items.state !== "ready")
    return (
      <Empty
        icon="info"
        title={t.unavailableTitle}
        hint={
          items.state === "error"
            ? errorText(items.error, t)
            : t.unavailableHint
        }
        action={
          <button
            type="button"
            className="button secondary"
            onClick={() => setReload((n) => n + 1)}
          >
            {t.retry}
          </button>
        }
      />
    );
  return (
    <section className="panel sessions-panel">
      {error && (
        <p className="form-error" role="alert">
          {error}
        </p>
      )}
      <ul className="session-list">
        {items.value.map((item) => (
          <li key={item.id}>
            <div>
              <strong>
                {methods[item.authentication_method]}
                {item.id === current.session.id && (
                  <span className="count-badge current-badge">
                    {t.currentSession}
                  </span>
                )}
              </strong>
              <dl>
                <div>
                  <dt>{t.signedInAt}</dt>
                  <dd>{time(item.created_at)}</dd>
                </div>
                <div>
                  <dt>{t.lastActivity}</dt>
                  <dd>{time(item.last_seen_at)}</dd>
                </div>
                <div>
                  <dt>{t.expires}</dt>
                  <dd>{time(item.expires_at)}</dd>
                </div>
              </dl>
            </div>
            <button
              type="button"
              className="button secondary"
              onClick={() => revoke(item.id)}
            >
              {item.id === current.session.id ? t.signOut : t.revoke}
            </button>
          </li>
        ))}
      </ul>
      {items.value.length <= 1 && (
        <p className="muted permissions-empty">{t.noSessions}</p>
      )}
    </section>
  );
}
