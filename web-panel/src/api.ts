export type Method = "passkey" | "password_totp" | "recovery_code";
export type Session = {
  id: string;
  operator_id: string;
  authentication_method: Method;
  authenticated_at: number;
  created_at: number;
  last_seen_at: number;
  expires_at: number;
};
export type SignedIn = {
  session: Session;
  csrf_token: string;
  operator: { login: string; display_name: string };
};
export type Tenant = { id: string; name: string };
export type TenantContext = Tenant & { capabilities: string[] };

export class ApiError extends Error {
  constructor(
    readonly status: number,
    readonly code: string,
  ) {
    super(code);
  }
}

async function request<T>(
  path: string,
  init: { method?: string; body?: unknown; csrf?: string } = {},
): Promise<T> {
  const headers: Record<string, string> = {};
  if (init.body !== undefined) headers["content-type"] = "application/json";
  if (init.csrf) headers["x-csrf-token"] = init.csrf;
  let response: Response;
  try {
    response = await fetch(path, {
      method: init.method ?? "GET",
      headers,
      body: init.body === undefined ? undefined : JSON.stringify(init.body),
      credentials: "same-origin",
      cache: "no-store",
    });
  } catch {
    throw new ApiError(0, "network");
  }
  if (response.status === 204) return undefined as T;
  const body: unknown = await response.json().catch(() => null);
  if (response.ok && (typeof body !== "object" || body === null))
    throw new ApiError(502, "unexpected");
  if (!response.ok) {
    const code =
      typeof body === "object" &&
      body !== null &&
      "error" in body &&
      typeof body.error === "string"
        ? body.error
        : "unexpected";
    throw new ApiError(response.status, code);
  }
  return body as T;
}

export const api = {
  session: () => request<SignedIn>("/api/session"),
  password: (login: string, password: string, code: string) =>
    request<SignedIn>("/api/auth/password", {
      method: "POST",
      body: { login, password, code },
    }),
  recovery: (login: string, code: string) =>
    request<SignedIn>("/api/auth/recovery", {
      method: "POST",
      body: { login, code },
    }),
  passkey: async (login: string) => {
    const issued = await request<{
      id: string;
      options: { publicKey: Record<string, unknown> };
    }>("/api/auth/passkey/start", { method: "POST", body: { login } });
    const credential = await getCredential(issued.options.publicKey);
    return request<SignedIn>("/api/auth/passkey/finish", {
      method: "POST",
      body: { id: issued.id, response: credential },
    });
  },
  tenants: () => request<Tenant[]>("/api/tenants"),
  tenant: (id: string) =>
    request<TenantContext>(`/api/tenants/${encodeURIComponent(id)}`),
  sessions: () => request<Session[]>("/api/sessions"),
  revoke: (id: string, csrf: string) =>
    request<void>(`/api/sessions/${encodeURIComponent(id)}`, {
      method: "DELETE",
      csrf,
    }),
  logout: (csrf: string) =>
    request<void>("/api/session/logout", { method: "POST", csrf }),
};

function decode(value: string) {
  const base64 = value.replace(/-/g, "+").replace(/_/g, "/");
  const padded = base64 + "=".repeat((4 - (base64.length % 4)) % 4);
  return Uint8Array.from(atob(padded), (c) => c.charCodeAt(0));
}

function encode(buffer: ArrayBuffer | null) {
  if (!buffer) return null;
  let text = "";
  for (const byte of new Uint8Array(buffer)) text += String.fromCharCode(byte);
  return btoa(text).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

async function getCredential(options: Record<string, unknown>) {
  if (!window.PublicKeyCredential || !navigator.credentials) {
    throw new ApiError(0, "passkey_unsupported");
  }
  const allow = Array.isArray(options.allowCredentials)
    ? (options.allowCredentials as { id: string; type: string }[])
    : [];
  let credential: Credential | null;
  try {
    credential = await navigator.credentials.get({
      publicKey: {
        ...(options as unknown as PublicKeyCredentialRequestOptions),
        challenge: decode(String(options.challenge)),
        allowCredentials: allow.map((item) => ({
          type: "public-key",
          id: decode(item.id),
        })),
      },
    });
  } catch {
    throw new ApiError(0, "passkey_cancelled");
  }
  if (!(credential instanceof PublicKeyCredential)) {
    throw new ApiError(0, "passkey_cancelled");
  }
  const response = credential.response as AuthenticatorAssertionResponse;
  return {
    id: credential.id,
    rawId: encode(credential.rawId),
    type: credential.type,
    extensions: {},
    response: {
      authenticatorData: encode(response.authenticatorData),
      clientDataJSON: encode(response.clientDataJSON),
      signature: encode(response.signature),
      userHandle: encode(response.userHandle),
    },
  };
}

/** Opens the session status channel; `onEnd` runs when the server closes it. */
export function watchSession(csrf: string, onEnd: () => void) {
  const scheme = location.protocol === "https:" ? "wss:" : "ws:";
  const socket = new WebSocket(
    `${scheme}//${location.host}/api/session/socket`,
  );
  let closed = false;
  socket.onopen = () => socket.send(JSON.stringify({ csrf_token: csrf }));
  socket.onclose = () => {
    if (!closed) onEnd();
  };
  return () => {
    closed = true;
    socket.close();
  };
}
