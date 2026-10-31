import { useCallback, useEffect, useRef, useState } from "react";

export type Health = {
  status: "checking" | "connected" | "disconnected";
  checkedAt: Date | null;
};

export function useConnection() {
  const [health, setHealth] = useState<Health>({
    status: "checking",
    checkedAt: null,
  });
  const request = useRef<AbortController | null>(null);
  const check = useCallback(async () => {
    request.current?.abort();
    const controller = new AbortController();
    request.current = controller;
    setHealth({ status: "checking", checkedAt: null });
    const timeout = setTimeout(() => controller.abort(), 5000);
    try {
      const response = await fetch("/health/live", {
        signal: controller.signal,
        cache: "no-store",
      });
      const body: unknown = await response.json();
      if (
        !response.ok ||
        typeof body !== "object" ||
        body === null ||
        !("status" in body) ||
        body.status !== "ok"
      )
        throw new Error("Unavailable");
      if (request.current === controller)
        setHealth({ status: "connected", checkedAt: new Date() });
    } catch {
      if (request.current === controller)
        setHealth({ status: "disconnected", checkedAt: new Date() });
    } finally {
      clearTimeout(timeout);
    }
  }, []);
  useEffect(() => {
    void check();
    return () => {
      request.current?.abort();
      request.current = null;
    };
  }, [check]);
  return { health, check };
}
