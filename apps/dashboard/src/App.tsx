import { useEffect, useState } from "react";

const text = {
  hu: {
    build: "Fejlesztői változat",
    title: "Elindult a MIDAS.",
    description: "A központi üzemeltetőfelület alapjai készülnek.",
    checking: "Kapcsolódás a helyi szerverhez…",
    online: "A helyi szerver elérhető",
    offline: "A helyi szerver nem érhető el",
    hint: "Indítsd el a fejlesztői környezetet a just dev paranccsal.",
    retry: "Újrapróbálás",
  },
  en: {
    build: "Development build",
    title: "MIDAS is running.",
    description: "The foundations of the operations console are taking shape.",
    checking: "Connecting to the local server…",
    online: "Local server connected",
    offline: "Local server unavailable",
    hint: "Start the development environment with just dev.",
    retry: "Retry",
  },
} as const;

export function App() {
  const [language, setLanguage] = useState<"hu" | "en">("hu");
  const [attempt, setAttempt] = useState(0);
  const copy = text[language];

  useEffect(() => {
    document.documentElement.lang = language;
  }, [language]);

  return (
    <div className="page">
      <header>
        <span className="wordmark">MIDAS</span>
        <nav aria-label={language === "hu" ? "Nyelv" : "Language"}>
          <button
            type="button"
            aria-pressed={language === "hu"}
            onClick={() => setLanguage("hu")}
          >
            Magyar
          </button>
          <button
            type="button"
            aria-pressed={language === "en"}
            onClick={() => setLanguage("en")}
          >
            English
          </button>
        </nav>
      </header>
      <main>
        <p className="eyebrow">{copy.build}</p>
        <h1>{copy.title}</h1>
        <p className="description">{copy.description}</p>
        <Connection
          key={attempt}
          copy={copy}
          onRetry={() => setAttempt((value) => value + 1)}
        />
      </main>
    </div>
  );
}

function Connection({
  copy,
  onRetry,
}: {
  copy: (typeof text)["hu" | "en"];
  onRetry: () => void;
}) {
  const [status, setStatus] = useState<"checking" | "online" | "offline">(
    "checking",
  );

  useEffect(() => {
    const controller = new AbortController();
    const timeout = setTimeout(() => controller.abort(), 5000);
    let active = true;
    setStatus("checking");
    fetch("/health/live", { signal: controller.signal, cache: "no-store" })
      .then(async (response) => {
        if (!response.ok) throw new Error("Service unavailable");
        const result: unknown = await response.json();
        if (
          typeof result !== "object" ||
          result === null ||
          !("status" in result) ||
          result.status !== "ok"
        ) {
          throw new Error("Unexpected service response");
        }
        if (active) setStatus("online");
      })
      .catch(() => {
        if (active) setStatus("offline");
      })
      .finally(() => clearTimeout(timeout));
    return () => {
      active = false;
      clearTimeout(timeout);
      controller.abort();
    };
  }, []);

  return (
    <div className="connection">
      <p role="status">
        <span className={`indicator ${status}`} aria-hidden="true" />
        {copy[status]}
      </p>
      {status === "offline" && (
        <>
          <p>{copy.hint}</p>
          <button type="button" onClick={onRetry}>
            {copy.retry}
          </button>
        </>
      )}
    </div>
  );
}
