// Tiny fetch wrapper used by every client component that talks to the API.
//
// Responsibilities:
//   1. Send credentials (cookies) by default.
//   2. Add X-CSRF-Token header on writes by mirroring the sprintly_csrf cookie.
//   3. On 401, try POST /auth/refresh exactly once; if that succeeds, replay
//      the original request.
//   4. Normalize error shape into ApiError so call sites just `try/catch`.

export type ApiError = {
  status: number;
  code: string;
  message: string;
};

const BASE =
  process.env.NEXT_PUBLIC_API_BASE_URL ?? "http://localhost:8080/api/v1";

type RequestOpts = Omit<RequestInit, "body"> & {
  body?: unknown;
  /** Internal: prevents recursive refresh attempts. */
  _retry?: boolean;
};

const WRITE_METHODS = new Set(["POST", "PATCH", "PUT", "DELETE"]);

export async function api<T = unknown>(
  path: string,
  opts: RequestOpts = {},
): Promise<T> {
  const { body, _retry, headers, method = "GET", ...rest } = opts;

  const finalHeaders: Record<string, string> = {
    "Content-Type": "application/json",
    Accept: "application/json",
    ...(headers as Record<string, string> | undefined ?? {}),
  };

  // Double-submit CSRF: echo the cookie as a header on writes. The backend
  // exempts /auth/login, /auth/register, /auth/refresh, /auth/password/reset/*
  // because those run before the cookie is set.
  if (WRITE_METHODS.has(method.toUpperCase())) {
    const csrf = readCookie("sprintly_csrf");
    if (csrf) finalHeaders["X-CSRF-Token"] = csrf;
  }

  const init: RequestInit = {
    ...rest,
    method,
    credentials: "include",
    headers: finalHeaders,
    body: body === undefined ? undefined : JSON.stringify(body),
  };

  const res = await fetch(`${BASE}${path}`, init);

  if (res.status === 401 && !_retry && path !== "/auth/refresh" && path !== "/auth/login") {
    const refreshed = await tryRefresh();
    if (refreshed) {
      return api<T>(path, { ...opts, _retry: true });
    }
  }

  // An expired CSRF cookie 403s before auth is even consulted. A successful
  // refresh re-issues it, so treat this exactly like the 401 path: refresh
  // once, then replay the write with the fresh cookie.
  if (res.status === 403 && !_retry && WRITE_METHODS.has(method.toUpperCase())) {
    const isJson403 = res.headers.get("content-type")?.includes("application/json");
    const payload403 = isJson403 ? await res.clone().json().catch(() => null) : null;
    if (payload403?.error?.code === "csrf") {
      const refreshed = await tryRefresh();
      if (refreshed) {
        return api<T>(path, { ...opts, _retry: true });
      }
    }
  }

  if (res.status === 204) return undefined as T;

  const isJson = res.headers
    .get("content-type")
    ?.includes("application/json");
  const payload = isJson ? await res.json() : null;

  if (!res.ok) {
    const err: ApiError = {
      status: res.status,
      code: payload?.error?.code ?? "unknown",
      message:
        payload?.error?.message ??
        `Request failed (${res.status}). Try again, or check the logs.`,
    };
    throw err;
  }

  return payload as T;
}

// Single-flight refresh. Without this, every request that 401s at the same
// moment (page load fires a dozen) races its own POST /auth/refresh; the
// server rotates the refresh token on first use and treats the losers as
// token reuse — and burns the whole session. One shared promise means one
// refresh per expiry, everyone else awaits it.
let refreshInFlight: Promise<boolean> | null = null;

export function tryRefresh(): Promise<boolean> {
  if (!refreshInFlight) {
    refreshInFlight = (async () => {
      try {
        await api("/auth/refresh", { method: "POST", _retry: true });
        return true;
      } catch {
        return false;
      } finally {
        // Cleared on the next microtask so late joiners in this tick still
        // share the result, while the next expiry gets a fresh attempt.
        setTimeout(() => {
          refreshInFlight = null;
        }, 0);
      }
    })();
  }
  return refreshInFlight;
}

function readCookie(name: string): string | null {
  if (typeof document === "undefined") return null;
  const target = `${name}=`;
  for (const raw of document.cookie.split(";")) {
    const trimmed = raw.trim();
    if (trimmed.startsWith(target)) {
      return decodeURIComponent(trimmed.slice(target.length));
    }
  }
  return null;
}
