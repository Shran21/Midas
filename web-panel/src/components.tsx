import { type ReactNode, useEffect, useRef, useState } from "react";
import { type alerts, type Device, devices } from "./data";
import { Icon, type IconName } from "./Icon";
import type { Copy, Language } from "./i18n";
import type { Health } from "./useConnection";

export function Stat({
  title,
  value,
  icon,
  tone = "",
  footer,
}: {
  title: string;
  value: number;
  icon: IconName;
  tone?: string;
  footer: ReactNode;
}) {
  return (
    <article className={["stat-card", tone].join(" ")}>
      <div className="stat-top">
        <h2>{title}</h2>
        <span className="stat-icon">
          <Icon name={icon} />
        </span>
      </div>
      <strong className="stat-value">{value}</strong>
      <div className="stat-footer">{footer}</div>
    </article>
  );
}

export function Status({ status, t }: { status: Device["status"]; t: Copy }) {
  return (
    <span className={["status-badge", status].join(" ")}>
      <span className={["dot", status].join(" ")} />
      {t[status]}
    </span>
  );
}

function MiniMeter({ value, label }: { value: number | null; label: string }) {
  return (
    <span
      className="mini-meter"
      role="img"
      aria-label={[
        label,
        ": ",
        value === null ? "—" : [value, "%"].join(""),
      ].join("")}
    >
      <span className="mini-track" aria-hidden="true">
        <span style={{ width: [value ?? 0, "%"].join("") }} />
      </span>
      <span>{value === null ? "—" : <>{value}%</>}</span>
    </span>
  );
}

export function DeviceList({
  items,
  view,
  compact,
  t,
  onSelect,
}: {
  items: Device[];
  view: "list" | "grid";
  compact: boolean;
  t: Copy;
  onSelect: (device: Device) => void;
}) {
  if (view === "grid")
    return (
      <div className="device-grid">
        {items.map((device) => (
          <button
            type="button"
            className="device-card"
            key={device.id}
            onClick={() => onSelect(device)}
          >
            <span className="device-card-heading">
              <span className="device-icon">
                <Icon name={device.kind === "server" ? "server" : "monitor"} />
              </span>
              <Status status={device.status} t={t} />
            </span>
            <strong>{device.name}</strong>
            <span className="muted">{device.os}</span>
            <span className="device-card-location">
              <Icon name="building" size={14} />
              {device.customer}
              <span>· {device.site}</span>
            </span>
            <span className="card-footer">
              <code>{device.ip}</code>
              <Icon name="arrow" size={16} />
            </span>
          </button>
        ))}
      </div>
    );
  return (
    <div className="table-scroll">
      <table
        className={compact ? "device-table compact-table" : "device-table"}
      >
        <thead>
          <tr>
            <th>{t.device}</th>
            <th>
              {t.customer} / {t.site}
            </th>
            <th>{t.status}</th>
            <th className="resource-column">{t.resources}</th>
            <th className="seen-column">{t.lastSeen}</th>
            <th>
              <span className="sr-only">{t.openDevice}</span>
            </th>
          </tr>
        </thead>
        <tbody>
          {items.map((device) => (
            <tr key={device.id}>
              <td>
                <button
                  type="button"
                  className="device-link"
                  onClick={() => onSelect(device)}
                >
                  <span className="device-icon">
                    <Icon
                      name={device.kind === "server" ? "server" : "monitor"}
                    />
                  </span>
                  <span>
                    <strong>{device.name}</strong>
                    <small>{device.os}</small>
                  </span>
                </button>
              </td>
              <td>
                <span className="cell-primary">{device.customer}</span>
                <small className="cell-secondary">{device.site}</small>
              </td>
              <td>
                <Status status={device.status} t={t} />
              </td>
              <td className="resource-column">
                <div className="resource-pair">
                  <MiniMeter label="CPU" value={device.cpu} />
                  <MiniMeter label="RAM" value={device.memory} />
                </div>
              </td>
              <td className="seen-column">
                <span className="seen">
                  <Icon name="clock" size={13} />
                  {device.seenMinutes === 0 ? (
                    t.now
                  ) : (
                    <>
                      {device.seenMinutes} {t.minutesAgo}
                    </>
                  )}
                </span>
              </td>
              <td>
                <button
                  type="button"
                  className="icon-button row-action"
                  aria-label={[t.openDevice, device.name].join(": ")}
                  onClick={() => onSelect(device)}
                >
                  <Icon name="chevron" size={16} />
                </button>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

export function ConnectionStatus({
  health,
  t,
  language,
  onCheck,
}: {
  health: Health;
  t: Copy;
  language: Language;
  onCheck: () => void;
}) {
  return (
    <div className="connection-card">
      <div className="connection-heading">
        <span
          className={[
            "dot",
            health.status === "connected"
              ? "online"
              : health.status === "checking"
                ? "checking"
                : "critical",
          ].join(" ")}
        />
        <strong>{t.connection}</strong>
        <button
          type="button"
          className="icon-button"
          onClick={onCheck}
          disabled={health.status === "checking"}
          aria-label={t.checkConnection}
          title={t.checkConnection}
        >
          <Icon name="refresh" size={15} />
        </button>
      </div>
      <p role="status">{t[health.status]}</p>
      {health.checkedAt && (
        <small>
          {t.checked}{" "}
          {health.checkedAt.toLocaleTimeString(language, {
            hour: "2-digit",
            minute: "2-digit",
          })}
        </small>
      )}
    </div>
  );
}

export function AlertList({
  items,
  language,
  t,
  onSelect,
}: {
  items: (typeof alerts)[number][];
  language: Language;
  t: Copy;
  onSelect: (device: Device) => void;
}) {
  return (
    <div className="alert-list">
      {items.length ? (
        items.map((alert) => {
          const device = devices.find((item) => item.id === alert.deviceId);
          return (
            <button
              key={alert.id}
              type="button"
              className="alert-item"
              onClick={() => device && onSelect(device)}
            >
              <span className={["alert-symbol", alert.severity].join(" ")}>
                <Icon
                  name={alert.id === "restart" ? "refresh" : "alert"}
                  size={17}
                />
              </span>
              <span className="alert-content">
                <span className="alert-title">{alert.title[language]}</span>
                <span className="alert-host">{device?.name}</span>
                <span className="alert-detail">{alert.detail[language]}</span>
                <span className={["severity", alert.severity].join(" ")}>
                  {t[alert.severity]}
                </span>
              </span>
              <Icon name="chevron" size={15} />
            </button>
          );
        })
      ) : (
        <div className="empty-alerts">
          <Icon name="check" />
          {t.noAlerts}
        </div>
      )}
    </div>
  );
}

export function DeviceDialog({
  device,
  t,
  onClose,
}: {
  device: Device;
  t: Copy;
  onClose: () => void;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  const opener = useRef<Element | null>(document.activeElement);
  const [section, setSection] = useState<"details" | "monitoring" | "support">(
    "details",
  );
  const metrics: [string, number | null][] = [
    ["CPU", device.cpu],
    [t.memory, device.memory],
    [t.disk, device.disk],
  ];
  useEffect(() => {
    if (!dialog.current?.open) dialog.current?.showModal();
  }, []);
  return (
    <dialog
      ref={dialog}
      className="device-dialog"
      aria-labelledby="device-title"
      aria-describedby="device-sample"
      onClose={() => {
        const target = opener.current;
        onClose();
        if (target instanceof HTMLElement && target.isConnected)
          requestAnimationFrame(() => target.focus());
      }}
    >
      <div className="dialog-heading">
        <div className="dialog-identity">
          <span className="device-icon">
            <Icon
              name={device.kind === "server" ? "server" : "monitor"}
              size={23}
            />
          </span>
          <div>
            <p id="device-sample" className="sample-label">
              {t.sampleDevice}
            </p>
            <h2 id="device-title">{device.name}</h2>
            <p>
              {device.customer} · {device.site}
            </p>
          </div>
        </div>
        <button
          type="button"
          className="icon-button"
          aria-label={t.close}
          onClick={() => dialog.current?.close()}
        >
          <Icon name="close" />
        </button>
      </div>
      <div className="dialog-status">
        <Status status={device.status} t={t} />
        <span>{device.os}</span>
      </div>
      <div className="dialog-tabs">
        {(["details", "monitoring", "support"] as const).map((name) => (
          <button
            key={name}
            type="button"
            aria-pressed={section === name}
            onClick={() => setSection(name)}
          >
            {t[name]}
          </button>
        ))}
      </div>
      <div className="dialog-body">
        {section === "details" && (
          <>
            <dl className="device-facts">
              {[
                [t.customer, device.customer],
                [t.site, device.site],
                [t.operatingSystem, device.os],
                [t.ip, device.ip],
                [t.type, t[device.kind]],
                [t.user, device.user],
                [t.updates, String(device.updates)],
                [t.restart, device.restart ? t.required : t.notRequired],
              ].map(([label, value]) => (
                <div key={label}>
                  <dt>{label}</dt>
                  <dd>{value}</dd>
                </div>
              ))}
            </dl>
            <div className="inline-note">
              <Icon name="info" size={16} />
              {t.previewNote}
            </div>
          </>
        )}
        {section === "monitoring" && (
          <>
            <p className="section-caption">{t.sampleMetrics}</p>
            <div className="metric-list">
              {metrics.map(([label, value]) => (
                <div className="metric-row" key={label}>
                  <div>
                    <span>{label}</span>
                    <strong>
                      {value === null ? t.unavailable : <>{value}%</>}
                    </strong>
                  </div>
                  <div className="metric-track" aria-hidden="true">
                    <span
                      className={Number(value) > 85 ? "metric-warning" : ""}
                      style={{ width: [value ?? 0, "%"].join("") }}
                    />
                  </div>
                </div>
              ))}
            </div>
          </>
        )}
        {section === "support" && (
          <>
            <div className="inline-note">
              <Icon name="info" size={16} />
              {t.supportHint}
            </div>
            <div className="support-options">
              <div>
                <Icon name="remote" size={24} />
                <h3>{t.normalSupport}</h3>
                <p>{t.normalSupportHint}</p>
                <button type="button" className="button primary" disabled>
                  {t.normalSupport}
                </button>
              </div>
              <div>
                <Icon name="shield" size={24} />
                <h3>{t.adminSupport}</h3>
                <p>{t.adminSupportHint}</p>
                <button type="button" className="button secondary" disabled>
                  {t.adminSupport}
                </button>
              </div>
            </div>
          </>
        )}
      </div>
      <div className="dialog-footer">
        <span className="sample-label">
          <span className="dot sample-dot" />
          {t.sample}
        </span>
        <button
          type="button"
          className="button secondary"
          onClick={() => dialog.current?.close()}
        >
          {t.close}
        </button>
      </div>
    </dialog>
  );
}
