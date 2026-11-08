import { useCallback, useEffect, useRef, useState } from "react";

export type Live<T> = {
  value: T | undefined;
  error: unknown;
  loading: boolean;
  /** Seconds since the epoch of the last successful load. */
  loadedAt: number | null;
};

/**
 * Loads now, again after each change notice and periodically while the page is visible.
 * The previous value stays on screen while a newer one loads.
 */
export function useLive<T>(
  load: () => Promise<T>,
  changes: number,
  fail: (error: unknown) => boolean,
  period = 60_000,
) {
  const [state, setState] = useState<Live<T>>({
    value: undefined,
    error: null,
    loading: true,
    loadedAt: null,
  });
  const generation = useRef(0);
  const reload = useCallback(async () => {
    const current = ++generation.current;
    setState((previous) => ({ ...previous, loading: true }));
    try {
      const value = await load();
      if (current === generation.current)
        setState({
          value,
          error: null,
          loading: false,
          loadedAt: Date.now() / 1000,
        });
    } catch (error) {
      if (current !== generation.current || fail(error)) return;
      setState((previous) => ({ ...previous, error, loading: false }));
    }
  }, [load, fail]);

  useEffect(() => {
    const timer = setTimeout(() => void reload(), changes === 0 ? 0 : 250);
    return () => clearTimeout(timer);
  }, [reload, changes]);

  useEffect(() => {
    const tick = setInterval(() => {
      if (document.visibilityState === "visible") void reload();
    }, period);
    const visible = () => {
      if (document.visibilityState === "visible") void reload();
    };
    document.addEventListener("visibilitychange", visible);
    return () => {
      clearInterval(tick);
      document.removeEventListener("visibilitychange", visible);
      generation.current += 1;
    };
  }, [reload, period]);

  return { ...state, reload };
}
