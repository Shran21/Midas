import { type FormEvent, useCallback, useEffect, useState } from "react";
import {
  api,
  type CreatedOperator,
  type Member,
  type Role,
  type Scope,
  type Site,
} from "./api";
import type { Copy, Language } from "./i18n";
import { permissionNames } from "./permissions";
import { errorText } from "./SignIn";

type Feedback = { kind: "ok" | "error"; text: string } | null;

function useAction(
  t: Copy,
  fail: (error: unknown) => boolean,
  after: () => void,
) {
  const [busy, setBusy] = useState(false);
  const [feedback, setFeedback] = useState<Feedback>(null);
  const run = async (action: () => Promise<unknown>, success = t.saved) => {
    setBusy(true);
    setFeedback(null);
    try {
      await action();
      setFeedback({ kind: "ok", text: success });
      after();
      return true;
    } catch (error) {
      if (!fail(error))
        setFeedback({ kind: "error", text: errorText(error, t) });
      return false;
    } finally {
      setBusy(false);
    }
  };
  return { busy, feedback, run };
}

function Message({ feedback }: { feedback: Feedback }) {
  if (!feedback) return null;
  return (
    <p
      className={feedback.kind === "ok" ? "notice" : "form-error"}
      role={feedback.kind === "ok" ? "status" : "alert"}
    >
      {feedback.text}
    </p>
  );
}

function scopeLabel(scope: Scope, sites: Site[], t: Copy) {
  if (scope.kind === "tenant") return t.wholeTenant;
  if (scope.kind === "site")
    return `${t.siteScope}: ${sites.find((s) => s.id === scope.id)?.name ?? scope.id.slice(0, 8)}`;
  return `${scope.kind === "group" ? t.groupScope : t.deviceScope}: ${scope.id.slice(0, 8)}`;
}

function ScopeSelect({
  value,
  onChange,
  sites,
  t,
}: {
  value: string;
  onChange: (value: string) => void;
  sites: Site[];
  t: Copy;
}) {
  return (
    <label className="field">
      <span>{t.scopeField}</span>
      <select value={value} onChange={(event) => onChange(event.target.value)}>
        <option value="">{t.wholeTenant}</option>
        {sites.map((site) => (
          <option key={site.id} value={site.id}>
            {t.siteScope}: {site.name}
          </option>
        ))}
      </select>
    </label>
  );
}

const toScope = (site: string): Scope =>
  site ? { kind: "site", id: site } : { kind: "tenant" };

export function AccessView({
  tenant,
  t,
  language,
  csrf,
  fail,
}: {
  tenant: string;
  t: Copy;
  language: Language;
  csrf: string;
  fail: (error: unknown) => boolean;
}) {
  const [members, setMembers] = useState<Member[] | null>(null);
  const [roles, setRoles] = useState<Role[]>([]);
  const [sites, setSites] = useState<Site[]>([]);
  const [error, setError] = useState<unknown>(null);
  const [login, setLogin] = useState("");
  const [siteName, setSiteName] = useState("");
  const [assignment, setAssignment] = useState({
    operator: "",
    role: "",
    site: "",
  });
  const [rule, setRule] = useState({
    operator: "",
    permission: "device.read",
    effect: "deny" as "allow" | "deny",
    site: "",
  });

  const load = useCallback(() => {
    Promise.all([api.members(tenant), api.roles(), api.sites(tenant)]).then(
      ([m, r, s]) => {
        setMembers(m);
        setRoles(r);
        setSites(s);
        setError(null);
      },
      (failure) => {
        if (!fail(failure)) setError(failure);
      },
    );
  }, [tenant, fail]);
  useEffect(load, [load]);
  const { busy, feedback, run } = useAction(t, fail, load);

  if (error) return <p className="form-error">{errorText(error, t)}</p>;
  if (!members) return <p className="loading-line">{t.loading}</p>;
  const names = permissionNames[language];
  const submit = (action: () => Promise<unknown>) => (event: FormEvent) => {
    event.preventDefault();
    void run(action);
  };

  return (
    <div className="admin-grid">
      <p className="muted admin-intro">{t.accessIntro}</p>
      <Message feedback={feedback} />
      <section className="panel admin-panel">
        <h2>{t.members}</h2>
        <ul className="member-list">
          {members.map((member) => (
            <li key={member.operator_id}>
              <div className="member-head">
                <div>
                  <strong>{member.display_name}</strong>
                  <small className="muted">{member.login}</small>
                </div>
                {!member.enabled && (
                  <span className="count-badge">{t.disabled}</span>
                )}
                <button
                  type="button"
                  className="button secondary"
                  disabled={busy}
                  onClick={() =>
                    run(() =>
                      api.setMember(
                        tenant,
                        member.operator_id,
                        !member.enabled,
                        csrf,
                      ),
                    )
                  }
                >
                  {member.enabled ? t.disable : t.enable}
                </button>
              </div>
              <dl className="member-rules">
                <dt>{t.roles}</dt>
                <dd>
                  {member.assignments.length === 0 && t.none}
                  {member.assignments.map((item) => (
                    <span key={item.id} className="rule-chip">
                      {item.role_name} · {scopeLabel(item.scope, sites, t)}
                      <button
                        type="button"
                        aria-label={`${t.remove}: ${item.role_name}`}
                        disabled={busy}
                        onClick={() =>
                          run(() => api.unassign(tenant, item.id, csrf))
                        }
                      >
                        ×
                      </button>
                    </span>
                  ))}
                </dd>
                <dt>{t.overrides}</dt>
                <dd>
                  {member.overrides.length === 0 && t.none}
                  {member.overrides.map((item) => (
                    <span
                      key={item.id}
                      className={
                        item.effect === "deny" ? "rule-chip deny" : "rule-chip"
                      }
                    >
                      {item.effect === "deny" ? t.denyEffect : t.allowEffect}:{" "}
                      {names[item.permission] ?? item.permission} ·{" "}
                      {scopeLabel(item.scope, sites, t)}
                      <button
                        type="button"
                        aria-label={`${t.remove}: ${names[item.permission] ?? item.permission}`}
                        disabled={busy}
                        onClick={() =>
                          run(() => api.removeOverride(tenant, item.id, csrf))
                        }
                      >
                        ×
                      </button>
                    </span>
                  ))}
                </dd>
              </dl>
            </li>
          ))}
        </ul>
      </section>
      <form
        className="panel admin-panel"
        onSubmit={submit(() => api.addMember(tenant, login.trim(), csrf))}
      >
        <h2>{t.addMember}</h2>
        <label className="field">
          <span>{t.memberLogin}</span>
          <input
            value={login}
            onChange={(event) => setLogin(event.target.value)}
            required
          />
        </label>
        <button
          type="submit"
          className="button primary"
          disabled={busy || !login.trim()}
        >
          {t.add}
        </button>
      </form>
      <form
        className="panel admin-panel"
        onSubmit={submit(() =>
          api.assign(
            tenant,
            {
              operator: assignment.operator,
              role: assignment.role,
              scope: toScope(assignment.site),
            },
            csrf,
          ),
        )}
      >
        <h2>{t.assignRole}</h2>
        <label className="field">
          <span>{t.memberField}</span>
          <select
            value={assignment.operator}
            onChange={(event) =>
              setAssignment({ ...assignment, operator: event.target.value })
            }
            required
          >
            <option value="" disabled>
              —
            </option>
            {members.map((m) => (
              <option key={m.operator_id} value={m.operator_id}>
                {m.display_name}
              </option>
            ))}
          </select>
        </label>
        <label className="field">
          <span>{t.roleField}</span>
          <select
            value={assignment.role}
            onChange={(event) =>
              setAssignment({ ...assignment, role: event.target.value })
            }
            required
          >
            <option value="" disabled>
              —
            </option>
            {roles.map((r) => (
              <option key={r.id} value={r.id}>
                {r.name}
              </option>
            ))}
          </select>
        </label>
        <ScopeSelect
          value={assignment.site}
          onChange={(site) => setAssignment({ ...assignment, site })}
          sites={sites}
          t={t}
        />
        <button
          type="submit"
          className="button primary"
          disabled={busy || !assignment.operator || !assignment.role}
        >
          {t.assignRole}
        </button>
      </form>
      <form
        className="panel admin-panel"
        onSubmit={submit(() =>
          api.override(
            tenant,
            {
              operator: rule.operator,
              permission: rule.permission,
              effect: rule.effect,
              scope: toScope(rule.site),
            },
            csrf,
          ),
        )}
      >
        <h2>{t.addRule}</h2>
        <label className="field">
          <span>{t.memberField}</span>
          <select
            value={rule.operator}
            onChange={(event) =>
              setRule({ ...rule, operator: event.target.value })
            }
            required
          >
            <option value="" disabled>
              —
            </option>
            {members.map((m) => (
              <option key={m.operator_id} value={m.operator_id}>
                {m.display_name}
              </option>
            ))}
          </select>
        </label>
        <label className="field">
          <span>{t.permissionField}</span>
          <select
            value={rule.permission}
            onChange={(event) =>
              setRule({ ...rule, permission: event.target.value })
            }
          >
            {Object.entries(names).map(([key, label]) => (
              <option key={key} value={key}>
                {label}
              </option>
            ))}
          </select>
        </label>
        <label className="field">
          <span>{t.effectField}</span>
          <select
            value={rule.effect}
            onChange={(event) =>
              setRule({
                ...rule,
                effect: event.target.value as "allow" | "deny",
              })
            }
          >
            <option value="deny">{t.denyEffect}</option>
            <option value="allow">{t.allowEffect}</option>
          </select>
        </label>
        <ScopeSelect
          value={rule.site}
          onChange={(site) => setRule({ ...rule, site })}
          sites={sites}
          t={t}
        />
        <button
          type="submit"
          className="button primary"
          disabled={busy || !rule.operator}
        >
          {t.addRule}
        </button>
      </form>
      <section className="panel admin-panel">
        <h2>{t.sites}</h2>
        {sites.length === 0 ? (
          <p className="muted">{t.noSites}</p>
        ) : (
          <ul className="plain-list">
            {sites.map((site) => (
              <li key={site.id}>{site.name}</li>
            ))}
          </ul>
        )}
        <form
          onSubmit={submit(() => api.createSite(tenant, siteName.trim(), csrf))}
          className="inline-form"
        >
          <label className="field">
            <span>{t.siteName}</span>
            <input
              value={siteName}
              onChange={(event) => setSiteName(event.target.value)}
              required
            />
          </label>
          <button
            type="submit"
            className="button secondary"
            disabled={busy || !siteName.trim()}
          >
            {t.newSite}
          </button>
        </form>
      </section>
    </div>
  );
}

export function CustomersView({
  t,
  csrf,
  fail,
  onTenantCreated,
}: {
  t: Copy;
  csrf: string;
  fail: (error: unknown) => boolean;
  onTenantCreated: () => void;
}) {
  const [name, setName] = useState("");
  const [login, setLogin] = useState("");
  const [display, setDisplay] = useState("");
  const [created, setCreated] = useState<CreatedOperator | null>(null);
  const { busy, feedback, run } = useAction(t, fail, () => {});

  const createTenant = async (event: FormEvent) => {
    event.preventDefault();
    if (await run(() => api.createTenant(name.trim(), csrf), t.tenantCreated)) {
      setName("");
      onTenantCreated();
    }
  };
  const createOperator = async (event: FormEvent) => {
    event.preventDefault();
    setCreated(null);
    await run(async () => {
      const result = await api.createOperator(
        login.trim(),
        display.trim(),
        csrf,
      );
      setCreated(result);
      setLogin("");
      setDisplay("");
    }, t.operatorCreated);
  };
  return (
    <div className="admin-grid">
      <Message feedback={feedback} />
      {created && (
        <section className="panel admin-panel codes-panel" aria-live="polite">
          <h2>
            {created.display_name} · {created.login}
          </h2>
          <ol className="code-list">
            {created.recovery_codes.map((code) => (
              <li key={code}>
                <code>{code}</code>
              </li>
            ))}
          </ol>
          <button
            type="button"
            className="button secondary"
            onClick={() => setCreated(null)}
          >
            {t.hideCodes}
          </button>
        </section>
      )}
      <form className="panel admin-panel" onSubmit={createTenant}>
        <h2>{t.newTenant}</h2>
        <label className="field">
          <span>{t.tenantName}</span>
          <input
            value={name}
            onChange={(event) => setName(event.target.value)}
            required
            maxLength={200}
          />
        </label>
        <button
          type="submit"
          className="button primary"
          disabled={busy || !name.trim()}
        >
          {t.create}
        </button>
      </form>
      <form className="panel admin-panel" onSubmit={createOperator}>
        <h2>{t.newOperator}</h2>
        <label className="field">
          <span>{t.login}</span>
          <input
            value={login}
            onChange={(event) => setLogin(event.target.value)}
            required
            maxLength={254}
          />
        </label>
        <label className="field">
          <span>{t.displayName}</span>
          <input
            value={display}
            onChange={(event) => setDisplay(event.target.value)}
            required
            maxLength={200}
          />
        </label>
        <button
          type="submit"
          className="button primary"
          disabled={busy || !login.trim() || !display.trim()}
        >
          {t.create}
        </button>
      </form>
    </div>
  );
}
