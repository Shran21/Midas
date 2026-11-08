import {
  type KeyboardEvent,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { api, type DeviceSummary, type TenantContext } from "../api";
import { Icon } from "../Icon";
import type { Copy, Language } from "../i18n";
import { actionIcons, actionLabel, type Tab } from "./DeviceMenu";
import { type Action, type PaletteEntry, palette } from "./model";

/** Ctrl+K search over the sections, devices and actions the operator may use. */
export function CommandPalette({
  tenant,
  tenantName,
  access,
  t,
  language,
  fail,
  sections,
  onSection,
  onOpen,
  onAction,
  onClose,
}: {
  tenant: string;
  tenantName: string;
  access: TenantContext;
  t: Copy;
  language: Language;
  fail: (error: unknown) => boolean;
  sections: { id: string; label: string }[];
  onSection: (id: string) => void;
  onOpen: (device: string, tab: Tab) => void;
  onAction: (device: DeviceSummary, action: Action) => void;
  onClose: () => void;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  const opener = useRef<Element | null>(document.activeElement);
  const [query, setQuery] = useState("");
  const [devices, setDevices] = useState<DeviceSummary[] | null>(null);
  const [active, setActive] = useState(0);

  useEffect(() => {
    if (!dialog.current?.open) dialog.current?.showModal();
    let current = true;
    api.devices(tenant).then(
      (value) => current && setDevices(value),
      (error) => {
        if (current && !fail(error)) setDevices([]);
      },
    );
    return () => {
      current = false;
    };
  }, [tenant, fail]);

  const results = useMemo(
    () =>
      palette(
        query,
        sections,
        devices ?? [],
        access,
        (id) => actionLabel(id, t),
        language,
      ),
    [query, sections, devices, access, t, language],
  );

  const close = () => dialog.current?.close();
  const choose = (entry: PaletteEntry) => {
    close();
    if (entry.kind === "section") onSection(entry.id);
    else if (entry.kind === "device") onOpen(entry.device.id, "overview");
    else onAction(entry.device, entry.action);
  };
  const keys = (event: KeyboardEvent) => {
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      const step = event.key === "ArrowDown" ? 1 : -1;
      setActive(
        (index) =>
          (index + step + results.length) % Math.max(results.length, 1),
      );
    } else if (event.key === "Enter" && results[active]) {
      event.preventDefault();
      choose(results[active]);
    }
  };
  const optionId = (index: number) => `palette-option-${index}`;

  return (
    <dialog
      ref={dialog}
      className="palette"
      aria-label={t.paletteTitle}
      onClose={() => {
        const target = opener.current;
        onClose();
        if (target instanceof HTMLElement && target.isConnected) target.focus();
      }}
    >
      <div className="palette-input">
        <Icon name="search" />
        <input
          role="combobox"
          aria-expanded="true"
          aria-controls="palette-results"
          aria-activedescendant={results[active] ? optionId(active) : undefined}
          aria-label={t.paletteTitle}
          placeholder={t.palettePlaceholder}
          value={query}
          onChange={(event) => {
            setQuery(event.target.value);
            setActive(0);
          }}
          onKeyDown={keys}
        />
        <kbd>Esc</kbd>
      </div>
      <p className="palette-target">
        {t.customer}: <strong>{tenantName}</strong>
      </p>
      <div id="palette-results" role="listbox" className="palette-results">
        {results.map((entry, index) => (
          <div
            key={
              entry.kind === "section"
                ? `s-${entry.id}`
                : entry.kind === "device"
                  ? `d-${entry.device.id}`
                  : `a-${entry.device.id}-${entry.action.id}`
            }
            id={optionId(index)}
            role="option"
            tabIndex={-1}
            aria-selected={index === active}
            className={index === active ? "active" : undefined}
            onMouseEnter={() => setActive(index)}
            onMouseDown={(event) => {
              event.preventDefault();
              choose(entry);
            }}
          >
            <Icon
              name={
                entry.kind === "section"
                  ? "arrow"
                  : entry.kind === "device"
                    ? entry.device.kind === "server"
                      ? "server"
                      : "monitor"
                    : actionIcons[entry.action.id]
              }
              size={16}
            />
            <span>{entry.label}</span>
            <small>
              {entry.kind === "section"
                ? t.paletteSection
                : entry.kind === "device"
                  ? t.device
                  : t.actions}
            </small>
          </div>
        ))}
      </div>
      {devices === null && <p className="loading-line">{t.loading}</p>}
      {devices !== null && results.length === 0 && (
        <p className="muted palette-empty">{t.paletteEmpty}</p>
      )}
      <p className="palette-hint">{t.paletteHint}</p>
    </dialog>
  );
}
