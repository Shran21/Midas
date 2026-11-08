import {
  type KeyboardEvent,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import { api, type DeviceSummary } from "../api";
import { Icon, type IconName } from "../Icon";
import type { Copy } from "../i18n";
import { errorText } from "../SignIn";
import type { Action, ActionId } from "./model";

export const actionIcons: Record<ActionId, IconName> = {
  open: "arrow",
  refresh: "refresh",
  monitoring: "activity",
  inventory: "box",
  policy: "settings",
  remote: "remote",
  remoteAdmin: "shield",
  terminal: "terminal",
  files: "folder",
  scripts: "script",
  updates: "refresh",
  revoke: "ban",
};

export function actionLabel(id: ActionId, t: Copy) {
  return {
    open: t.actionOpen,
    refresh: t.actionRefresh,
    monitoring: t.actionMonitoring,
    inventory: t.actionInventory,
    policy: t.actionPolicy,
    remote: t.normalSupport,
    remoteAdmin: t.adminSupport,
    terminal: t.actionTerminal,
    files: t.actionFiles,
    scripts: t.actionScripts,
    updates: t.actionUpdates,
    revoke: t.actionRevoke,
  }[id];
}

export function blockedText(action: Action, t: Copy) {
  switch (action.blocked) {
    case "offline":
      return t.blockedOffline;
    case "revoked":
      return t.blockedRevoked;
    case "planned":
      return t.blockedPlanned;
    case "headless":
      return t.blockedHeadless;
    default:
      return action.fresh ? t.needsFreshPasskey : null;
  }
}

function menuItems(panel: HTMLElement | null) {
  return Array.from(
    panel?.querySelectorAll<HTMLButtonElement>('[role="menuitem"]') ?? [],
  );
}

export type MenuState = {
  device: DeviceSummary;
  actions: Action[];
  x: number;
  y: number;
  /** Receives focus again when the menu closes, if it is still on the page. */
  opener: HTMLElement | null;
};

export function DeviceMenu({
  menu,
  tenantName,
  t,
  onAction,
  onClose,
}: {
  menu: MenuState;
  tenantName: string;
  t: Copy;
  onAction: (device: DeviceSummary, action: Action) => void;
  onClose: () => void;
}) {
  const panel = useRef<HTMLDivElement>(null);
  const [position, setPosition] = useState({ left: menu.x, top: menu.y });
  useLayoutEffect(() => {
    const element = panel.current;
    if (!element) return;
    const { width, height } = element.getBoundingClientRect();
    setPosition({
      left: Math.max(8, Math.min(menu.x, window.innerWidth - width - 8)),
      top: Math.max(8, Math.min(menu.y, window.innerHeight - height - 8)),
    });
    menuItems(element)[0]?.focus();
  }, [menu.x, menu.y]);

  useEffect(() => {
    const opener = menu.opener;
    const outside = (event: PointerEvent) => {
      if (!panel.current?.contains(event.target as Node)) onClose();
    };
    const away = () => onClose();
    document.addEventListener("pointerdown", outside, true);
    window.addEventListener("resize", away);
    window.addEventListener("scroll", away);
    return () => {
      document.removeEventListener("pointerdown", outside, true);
      window.removeEventListener("resize", away);
      window.removeEventListener("scroll", away);
      if (opener?.isConnected) opener.focus();
    };
  }, [menu.opener, onClose]);

  const keys = (event: KeyboardEvent) => {
    const list = menuItems(panel.current);
    const index = list.indexOf(document.activeElement as HTMLButtonElement);
    const move = (next: number) => {
      event.preventDefault();
      list[(next + list.length) % list.length]?.focus();
    };
    if (event.key === "ArrowDown") move(index + 1);
    else if (event.key === "ArrowUp") move(index - 1);
    else if (event.key === "Home") move(0);
    else if (event.key === "End") move(list.length - 1);
    else if (event.key === "Escape" || event.key === "Tab") {
      event.preventDefault();
      onClose();
    }
  };

  const titleId = `menu-${menu.device.id}`;
  return (
    <div
      ref={panel}
      className="device-menu"
      role="menu"
      aria-labelledby={titleId}
      style={position}
      onKeyDown={keys}
    >
      <div className="device-menu-target" id={titleId}>
        <strong className="menu-device">{menu.device.name}</strong>
        <span>
          {tenantName}
          {menu.device.site && ` · ${menu.device.site.name}`}
        </span>
      </div>
      {menu.actions.map((action) => {
        const note = blockedText(action, t);
        const disabled = action.blocked !== null;
        return (
          <button
            key={action.id}
            type="button"
            role="menuitem"
            className={action.destructive ? "danger" : undefined}
            aria-disabled={disabled || undefined}
            onClick={() => {
              if (disabled) return;
              onClose();
              onAction(menu.device, action);
            }}
          >
            <span className="menu-icon">
              <Icon name={actionIcons[action.id]} size={16} />
            </span>
            <span>
              {actionLabel(action.id, t)}
              {note && <small>{note}</small>}
            </span>
          </button>
        );
      })}
    </div>
  );
}

export type Feedback = { kind: "ok" | "error"; text: string } | null;

/** Runs a device action the same way from the list, the cards and the device page. */
export function useDeviceActions({
  tenant,
  tenantName,
  csrf,
  t,
  fail,
  onOpen,
  onChanged,
}: {
  tenant: string;
  tenantName: string;
  csrf: string;
  t: Copy;
  fail: (error: unknown) => boolean;
  onOpen: (device: string, tab: Tab) => void;
  onChanged: () => void;
}) {
  const [feedback, setFeedback] = useState<Feedback>(null);
  const [confirming, setConfirming] = useState<DeviceSummary | null>(null);

  const run = async (device: DeviceSummary, action: Action) => {
    if (action.blocked) return;
    setFeedback(null);
    switch (action.id) {
      case "open":
        return onOpen(device.id, "overview");
      case "monitoring":
      case "inventory":
      case "policy":
        return onOpen(device.id, action.id);
      case "revoke":
        return setConfirming(device);
      case "refresh":
        try {
          await api.refresh(tenant, device.id, csrf);
          setFeedback({
            kind: "ok",
            text: `${device.name}: ${t.refreshRequested}`,
          });
        } catch (error) {
          if (!fail(error))
            setFeedback({
              kind: "error",
              text: `${device.name}: ${errorText(error, t)}`,
            });
        }
        return;
      default:
        return;
    }
  };

  const element = (
    <>
      {feedback && (
        <p
          className={feedback.kind === "ok" ? "notice" : "form-error"}
          role={feedback.kind === "ok" ? "status" : "alert"}
        >
          {feedback.text}
        </p>
      )}
      {confirming && (
        <RevokeDialog
          device={confirming}
          tenantName={tenantName}
          t={t}
          onCancel={() => setConfirming(null)}
          onConfirm={async () => {
            try {
              await api.revokeDevice(tenant, confirming.id, csrf);
              setFeedback({
                kind: "ok",
                text: `${confirming.name}: ${t.revoked}`,
              });
              onChanged();
            } catch (error) {
              if (!fail(error))
                setFeedback({
                  kind: "error",
                  text: `${confirming.name}: ${errorText(error, t)}`,
                });
            }
          }}
        />
      )}
    </>
  );
  return { run, element };
}

export type Tab = "overview" | "monitoring" | "inventory" | "policy";

function RevokeDialog({
  device,
  tenantName,
  t,
  onCancel,
  onConfirm,
}: {
  device: DeviceSummary;
  tenantName: string;
  t: Copy;
  onCancel: () => void;
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
      aria-labelledby="revoke-title"
      aria-describedby="revoke-hint"
      onClose={() => {
        const target = opener.current;
        onCancel();
        if (target instanceof HTMLElement && target.isConnected) target.focus();
      }}
    >
      <h2 id="revoke-title">{t.revokeTitle}</h2>
      <dl className="target-facts">
        <div className="fact">
          <dt>{t.customer}</dt>
          <dd>{tenantName}</dd>
        </div>
        <div className="fact">
          <dt>{t.device}</dt>
          <dd>{device.name}</dd>
        </div>
        {device.site && (
          <div className="fact">
            <dt>{t.site}</dt>
            <dd>{device.site.name}</dd>
          </div>
        )}
      </dl>
      <p id="revoke-hint">{t.revokeHint}</p>
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
          className="button danger"
          disabled={busy}
          onClick={async () => {
            setBusy(true);
            await onConfirm();
            setBusy(false);
            dialog.current?.close();
          }}
        >
          {t.actionRevoke}
        </button>
      </div>
    </dialog>
  );
}
