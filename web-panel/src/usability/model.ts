export type Mode = "normal" | "admin";

export type Request = {
  id: string;
  mode: Mode;
  session: string;
  requestedAt: number;
  expiresAt: number;
};

export type State =
  | { kind: "idle" }
  | { kind: "waiting"; request: Request }
  | { kind: "declined"; request: Request }
  | { kind: "expired"; request: Request }
  | { kind: "connected"; request: Request; since: number }
  | { kind: "ended"; request: Request; by: "user" | "operator"; at: number };

export type Event =
  | { type: "request"; request: Request }
  | { type: "allow"; id: string; at: number }
  | { type: "decline"; id: string; at: number }
  | { type: "expire"; id: string; at: number }
  | { type: "end"; id: string; by: "user" | "operator"; at: number }
  | { type: "reset" };

export const REQUEST_SECONDS = 60;

export function newRequest(
  mode: Mode,
  session: string,
  now: number,
  id: string,
): Request {
  return {
    id,
    mode,
    session,
    requestedAt: now,
    expiresAt: now + REQUEST_SECONDS * 1000,
  };
}

/** Answers count only for the pending request and only before it expires; nothing activates late. */
export function reduce(state: State, event: Event): State {
  switch (event.type) {
    case "request":
      return state.kind === "waiting" || state.kind === "connected"
        ? state
        : { kind: "waiting", request: event.request };
    case "allow":
      if (state.kind !== "waiting" || state.request.id !== event.id)
        return state;
      if (event.at >= state.request.expiresAt)
        return { kind: "expired", request: state.request };
      return { kind: "connected", request: state.request, since: event.at };
    case "decline":
      return state.kind === "waiting" && state.request.id === event.id
        ? { kind: "declined", request: state.request }
        : state;
    case "expire":
      return state.kind === "waiting" &&
        state.request.id === event.id &&
        event.at >= state.request.expiresAt
        ? { kind: "expired", request: state.request }
        : state;
    case "end":
      return state.kind === "connected" && state.request.id === event.id
        ? { kind: "ended", request: state.request, by: event.by, at: event.at }
        : state;
    case "reset":
      return { kind: "idle" };
  }
}

export function secondsLeft(state: State, now: number) {
  return state.kind === "waiting"
    ? Math.min(
        REQUEST_SECONDS,
        Math.max(0, Math.ceil((state.request.expiresAt - now) / 1000)),
      )
    : 0;
}
