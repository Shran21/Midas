import { useEffect, useState } from "react";
import type { Language } from "./i18n";

export type Theme = "light" | "dark";
export type Preferences = {
  language: Language;
  setLanguage: (language: Language) => void;
  theme: Theme;
  setTheme: (theme: Theme) => void;
};

export function stored(key: string, allowed: string[], fallback: string) {
  try {
    const value = localStorage.getItem(key);
    return value && allowed.includes(value) ? value : fallback;
  } catch {
    return fallback;
  }
}

export function remember(key: string, value: string) {
  try {
    localStorage.setItem(key, value);
  } catch {}
}

export function usePreferences(): Preferences {
  const [language, setLanguage] = useState<Language>(
    () => stored("midas.language", ["hu", "en"], "hu") as Language,
  );
  const [theme, setTheme] = useState<Theme>(
    () =>
      stored(
        "midas.theme",
        ["light", "dark"],
        window.matchMedia("(prefers-color-scheme: dark)").matches
          ? "dark"
          : "light",
      ) as Theme,
  );
  useEffect(() => {
    document.documentElement.lang = language;
    document.documentElement.dataset.theme = theme;
    remember("midas.language", language);
    remember("midas.theme", theme);
  }, [language, theme]);
  return { language, setLanguage, theme, setTheme };
}
