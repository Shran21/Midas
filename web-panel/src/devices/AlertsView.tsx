import { useCallback, useState } from "react";
import { api, type TenantContext } from "../api";
import { Icon } from "../Icon";
import type { Copy, Language } from "../i18n";
import { errorText } from "../SignIn";
import type { Tab } from "./DeviceMenu";
import { AlertTable } from "./DevicePage";
import { ConditionBadge } from "./DevicesView";
import { useLive } from "./useLive";

/**
 * The tenant alert list needs tenant-wide monitoring; an operator with narrower scope sees
 * the devices with open alerts instead and opens each device's own alert list.
 */
export function AlertsView({
  tenant,
  access,
  t,
  language,
  csrf,
  fail,
  changes,
  onOpen,
}: {
  tenant: string;
  access: TenantContext;
  t: Copy;
  language: Language;
  csrf: string;
  fail: (error: unknown) => boolean;
  changes: number;
  onOpen: (device: string, tab: Tab) => void;
}) {
  const tenantWide = access.tenant_wide.includes("monitor.read");
  const [openOnly, setOpenOnly] = useState(true);
  const loadAlerts = useCallback(
    () => (tenantWide ? api.alerts(tenant, openOnly) : Promise.resolve([])),
    [tenant, tenantWide, openOnly],
  );
  const loadDevices = useCallback(() => api.devices(tenant), [tenant]);
  const alerts = useLive(loadAlerts, changes, fail);
  const devices = useLive(loadDevices, changes, fail);
  const permitted = (device: string) =>
    devices.value
      ?.find((item) => item.id === device)
      ?.permissions.includes("alert.acknowledge") ?? false;

  if (!tenantWide) {
    const affected = (devices.value ?? []).filter((d) => d.open_alerts > 0);
    return (
      <section className="panel full-alerts">
        <div className="panel-heading">
          <div className="panel-title">
            <Icon name="alert" />
            <h2>{t.withAlerts}</h2>
            <span className="count-badge">{affected.length}</span>
          </div>
        </div>
        <p className="panel-description">{t.scopedAlertsHint}</p>
        {devices.value === undefined ? (
          <p className="loading-line">{t.loading}</p>
        ) : affected.length === 0 ? (
          <div className="empty-alerts">
            <Icon name="check" />
            {t.noAlerts}
          </div>
        ) : (
          <ul className="alert-rows">
            {affected.map((device) => (
              <li key={device.id}>
                <span
                  className={[
                    "alert-symbol",
                    device.critical_alerts > 0 ? "critical" : "warning",
                  ].join(" ")}
                >
                  <Icon name="alert" size={17} />
                </span>
                <span className="alert-content">
                  <span className="alert-title">{device.name}</span>
                  <span className="alert-detail">
                    {device.open_alerts} {t.alertCount}
                    {device.critical_alerts > 0 &&
                      ` · ${device.critical_alerts} ${t.critical.toLocaleLowerCase(language)}`}
                  </span>
                </span>
                <ConditionBadge device={device} t={t} />
                <button
                  type="button"
                  className="button secondary"
                  onClick={() => onOpen(device.id, "monitoring")}
                >
                  {t.actionMonitoring}
                </button>
              </li>
            ))}
          </ul>
        )}
      </section>
    );
  }

  return (
    <section className="panel full-alerts">
      <div className="panel-heading">
        <div className="panel-title">
          <Icon name="alert" />
          <h2>{openOnly ? t.activeAlerts : t.alertHistory}</h2>
          {alerts.value && (
            <span className="count-badge">{alerts.value.length}</span>
          )}
        </div>
        <fieldset className="kind-switch" aria-label={t.alerts}>
          <button
            type="button"
            aria-pressed={openOnly}
            onClick={() => setOpenOnly(true)}
          >
            {t.openAlerts}
          </button>
          <button
            type="button"
            aria-pressed={!openOnly}
            onClick={() => setOpenOnly(false)}
          >
            {t.alertHistory}
          </button>
        </fieldset>
      </div>
      {alerts.value ? (
        <AlertTable
          items={alerts.value}
          canAcknowledge={(alert) => permitted(alert.device_id)}
          tenant={tenant}
          csrf={csrf}
          t={t}
          language={language}
          fail={fail}
          onChanged={() => void alerts.reload()}
          onOpen={(device) => onOpen(device, "monitoring")}
        />
      ) : alerts.error ? (
        <p className="form-error" role="alert">
          {errorText(alerts.error, t)}
        </p>
      ) : (
        <p className="loading-line">{t.loading}</p>
      )}
    </section>
  );
}
