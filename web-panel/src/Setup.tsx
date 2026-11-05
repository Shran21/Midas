import { type FormEvent, useEffect, useRef, useState } from "react";
import { ApiError, api, type SignedIn } from "./api";
import { translations } from "./i18n";
import type { Preferences } from "./preferences";
import { errorText, PreferenceControls } from "./SignIn";

export function Setup({
  token,
  preferences,
  onSignedIn,
}: {
  token: string;
  preferences: Preferences;
  onSignedIn: (session: SignedIn) => void;
}) {
  const t = translations[preferences.language];
  const [login, setLogin] = useState("");
  const [name, setName] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const heading = useRef<HTMLHeadingElement>(null);
  useEffect(() => {
    document.title = ["MIDAS", t.setupTitle].join(" · ");
  }, [t]);
  useEffect(() => {
    heading.current?.focus();
  }, []);
  const submit = async (event: FormEvent) => {
    event.preventDefault();
    setBusy(true);
    setError(null);
    try {
      onSignedIn(await api.setup(token, login.trim(), name.trim()));
    } catch (failure) {
      setError(
        failure instanceof ApiError && failure.status === 401
          ? t.setupInvalid
          : errorText(failure, t),
      );
      setBusy(false);
    }
  };
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
            {t.setupTitle}
          </h1>
          <p className="muted">{t.setupIntro}</p>
          <label className="field">
            <span>{t.login}</span>
            <input
              autoComplete="username webauthn"
              value={login}
              onChange={(event) => setLogin(event.target.value)}
              required
              maxLength={254}
            />
          </label>
          <label className="field">
            <span>{t.displayName}</span>
            <input
              autoComplete="name"
              value={name}
              onChange={(event) => setName(event.target.value)}
              required
              maxLength={200}
            />
          </label>
          <p className="field-hint">{t.passkeyHint}</p>
          {error && (
            <p className="form-error" role="alert">
              {error}
            </p>
          )}
          <button
            type="submit"
            className="button primary signin-submit"
            disabled={busy || !login.trim() || !name.trim()}
          >
            {busy ? t.signingIn : t.setupAction}
          </button>
        </form>
      </main>
    </div>
  );
}
