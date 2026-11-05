import { type FormEvent, useEffect, useRef, useState } from "react";
import { ApiError, api, type SignedIn } from "./api";
import { Icon } from "./Icon";
import { type Copy, type Language, translations } from "./i18n";
import type { Preferences } from "./preferences";

type Mode = "passkey" | "password" | "recovery" | "directory";

export function errorText(error: unknown, t: Copy) {
  const code = error instanceof ApiError ? error.code : "unexpected";
  switch (code) {
    case "authentication_failed":
      return t.errorAuth;
    case "try_later":
      return t.errorRate;
    case "invalid_request":
      return t.errorInvalid;
    case "passkey_cancelled":
      return t.errorPasskeyCancelled;
    case "passkey_unsupported":
      return t.errorPasskeyUnsupported;
    case "network":
      return t.errorNetwork;
    case "fresh_passkey_required":
      return t.freshPasskeyRequired;
    case "escalation_denied":
      return t.escalationDenied;
    case "not_found":
      return t.notFound;
    case "conflict":
      return t.conflict;
    case "recent_sign_in_required":
      return t.recentSignInRequired;
    default:
      return t.errorUnavailable;
  }
}

export function PreferenceControls({
  language,
  setLanguage,
  theme,
  setTheme,
}: Preferences) {
  const t = translations[language];
  return (
    <div className="preference-controls">
      <label className="language-control">
        <Icon name="globe" />
        <span className="sr-only">{t.language}</span>
        <select
          value={language}
          onChange={(event) => setLanguage(event.target.value as Language)}
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
    </div>
  );
}

export function SignIn({
  preferences,
  notice,
  onSignedIn,
}: {
  preferences: Preferences;
  notice: string | null;
  onSignedIn: (session: SignedIn) => void;
}) {
  const t = translations[preferences.language];
  const [mode, setMode] = useState<Mode>("passkey");
  const [login, setLogin] = useState("");
  const [password, setPassword] = useState("");
  const [code, setCode] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const heading = useRef<HTMLHeadingElement>(null);

  useEffect(() => {
    document.title = ["MIDAS", t.signInTitle].join(" · ");
  }, [t]);
  useEffect(() => {
    heading.current?.focus();
  }, []);

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    setBusy(true);
    setError(null);
    try {
      const session =
        mode === "passkey"
          ? await api.passkey(login)
          : mode === "password"
            ? await api.password(login, password, code)
            : mode === "directory"
              ? await api.directory(login.trim(), password)
              : await api.recovery(login, code.trim());
      setPassword("");
      setCode("");
      onSignedIn(session);
    } catch (failure) {
      setError(errorText(failure, t));
      setBusy(false);
    }
  };

  const modes: [Mode, string][] = [
    ["passkey", t.methodPasskey],
    ["password", t.methodPassword],
    ["recovery", t.methodRecovery],
    ["directory", t.methodDirectory],
  ];
  return (
    <div className="signin-page">
      <header className="signin-top">
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
        <PreferenceControls {...preferences} />
      </header>
      <main id="main" className="signin-main">
        <form className="panel signin-card" onSubmit={submit} noValidate>
          <h1 ref={heading} tabIndex={-1}>
            {t.signInTitle}
          </h1>
          <p className="muted">{t.signInIntro}</p>
          {notice && (
            <p className="notice" role="status">
              <Icon name="info" size={16} />
              {notice}
            </p>
          )}
          <fieldset className="method-switch">
            <legend className="sr-only">{t.signInMethod}</legend>
            {modes.map(([value, label]) => (
              <button
                key={value}
                type="button"
                aria-pressed={mode === value}
                onClick={() => {
                  setMode(value);
                  setError(null);
                  setCode("");
                }}
              >
                {label}
              </button>
            ))}
          </fieldset>
          <label className="field">
            <span>{t.login}</span>
            <input
              name="username"
              autoComplete={
                mode === "passkey" ? "username webauthn" : "username"
              }
              value={login}
              onChange={(event) => setLogin(event.target.value)}
              required
              maxLength={254}
            />
          </label>
          {(mode === "password" || mode === "directory") && (
            <label className="field">
              <span>{t.password}</span>
              <input
                type="password"
                name="password"
                autoComplete="current-password"
                value={password}
                onChange={(event) => setPassword(event.target.value)}
                required
              />
            </label>
          )}
          {(mode === "password" || mode === "recovery") && (
            <label className="field">
              <span>{mode === "password" ? t.totp : t.recoveryCode}</span>
              <input
                name="code"
                autoComplete="one-time-code"
                inputMode={mode === "password" ? "numeric" : "text"}
                spellCheck={false}
                value={code}
                onChange={(event) => setCode(event.target.value)}
                required
                maxLength={mode === "password" ? 6 : 64}
              />
            </label>
          )}
          <p className="field-hint">
            {mode === "passkey"
              ? t.passkeyHint
              : mode === "password"
                ? t.passwordHint
                : mode === "directory"
                  ? t.directoryHint
                  : t.recoveryHint}
          </p>
          {error && (
            <p className="form-error" role="alert">
              {error}
            </p>
          )}
          <button
            type="submit"
            className="button primary signin-submit"
            disabled={busy || login.trim() === ""}
          >
            {busy
              ? t.signingIn
              : mode === "passkey"
                ? t.passkeyAction
                : t.signInAction}
          </button>
        </form>
      </main>
    </div>
  );
}
