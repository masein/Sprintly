// OIDC SSO (F10) — the login page asks whether SSO is on and where to send the
// browser to start the flow.

import { api } from "./api";

const API_BASE =
  process.env.NEXT_PUBLIC_API_BASE_URL ?? "http://localhost:8080/api/v1";

export type OidcStatus = {
  enabled: boolean;
  local_login_disabled: boolean;
};

export const getOidcStatus = () => api<OidcStatus>("/auth/oidc/status");

/** Full-page navigation target that kicks off the redirect dance. */
export const oidcStartUrl = () => `${API_BASE}/auth/oidc/start`;
