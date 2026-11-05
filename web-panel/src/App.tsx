import { useCallback, useEffect, useState } from "react";
import { ApiError, api, type SignedIn } from "./api";
import { Icon } from "./Icon";
import { translations } from "./i18n";
import { Preview } from "./Preview";
import { usePreferences } from "./preferences";
import { Shell, type SignOutReason } from "./Shell";
import { SignIn } from "./SignIn";

type Mode =
  | { kind: "checking" }
  | { kind: "preview" }
  | { kind: "unavailable" }
  | { kind: "signed-out"; reason: SignOutReason | null }
  | { kind: "signed-in"; session: SignedIn };

export function App() {
  const preferences = usePreferences();
  const t = translations[preferences.language];
  const [mode, setMode] = useState<Mode>({ kind: "checking" });

  const check = useCallback(async () => {
    setMode({ kind: "checking" });
    try {
      setMode({ kind: "signed-in", session: await api.session() });
    } catch (error) {
      if (error instanceof ApiError && error.status === 401)
        setMode({ kind: "signed-out", reason: null });
      // Without browser authentication the server exposes only the sample-data preview.
      else if (error instanceof ApiError && error.status === 404)
        setMode({ kind: "preview" });
      else setMode({ kind: "unavailable" });
    }
  }, []);

  useEffect(() => {
    void check();
  }, [check]);

  const signedOut = useCallback(
    (reason: SignOutReason) => setMode({ kind: "signed-out", reason }),
    [],
  );

  switch (mode.kind) {
    case "preview":
      return <Preview {...preferences} />;
    case "signed-in":
      return (
        <Shell
          key={mode.session.session.id}
          preferences={preferences}
          signedIn={mode.session}
          onSignedOut={signedOut}
        />
      );
    case "signed-out":
      return (
        <SignIn
          preferences={preferences}
          notice={
            mode.reason === "ended"
              ? t.sessionEnded
              : mode.reason === "signed-out"
                ? t.signedOut
                : null
          }
          onSignedIn={(session) => setMode({ kind: "signed-in", session })}
        />
      );
    default:
      return (
        <main id="main" className="status-page">
          {mode.kind === "checking" ? (
            <p className="loading-line" role="status">
              {t.checkingSession}
            </p>
          ) : (
            <div className="empty-state" role="alert">
              <span className="empty-icon">
                <Icon name="info" size={25} />
              </span>
              <h1>{t.unavailableTitle}</h1>
              <p>{t.unavailableHint}</p>
              <button
                type="button"
                className="button secondary"
                onClick={() => void check()}
              >
                {t.retry}
              </button>
            </div>
          )}
        </main>
      );
  }
}
