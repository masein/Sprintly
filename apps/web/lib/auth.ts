// Auth helpers used across pages.
//
// Sprintly never stores the access token in JS — it lives in an HttpOnly
// cookie. We keep the *user object* in memory after login, and re-fetch from
// /users/me on a fresh page load. That's the source of truth.

import { api } from "./api";

export type Me = {
  id: string;
  email: string;
  handle: string;
  display_name: string;
  avatar_url: string | null;
  role: "admin" | "member" | "viewer";
  status: "active" | "invited" | "suspended";
  timezone: string;
  currency: string;
  settings: Record<string, unknown>;
  created_at: string;
  last_seen_at: string | null;
};

export type LoginPayload = { email: string; password: string };
export type RegisterPayload = {
  email: string;
  handle: string;
  display_name: string;
  password: string;
  invite_token?: string;
};

export type AuthResponse = {
  access_token: string;
  user: {
    id: string;
    email: string;
    handle: string;
    display_name: string;
    role: string;
  };
};

/** Returned by /auth/login when the account has 2FA enabled — the password
 *  step passed, but a second factor is needed to finish. */
export type TwoFactorChallenge = { two_factor_required: true; challenge: string };
export type LoginResult = AuthResponse | TwoFactorChallenge;

export const isTwoFactorChallenge = (r: LoginResult): r is TwoFactorChallenge =>
  "two_factor_required" in r && r.two_factor_required === true;

export const login = (p: LoginPayload) =>
  api<LoginResult>("/auth/login", { method: "POST", body: p });

/** Complete a 2FA login with a TOTP or recovery code. */
export const twoFactorLogin = (challenge: string, code: string) =>
  api<AuthResponse>("/auth/2fa", { method: "POST", body: { challenge, code } });

export const register = (p: RegisterPayload) =>
  api<AuthResponse>("/auth/register", { method: "POST", body: p });

export const logout = () => api<void>("/auth/logout", { method: "POST" });

export const me = () => api<Me>("/users/me");

export const requestPasswordReset = (email: string) =>
  api<{ message: string; dev_token?: string }>(
    "/auth/password/reset/request",
    { method: "POST", body: { email } },
  );

export const confirmPasswordReset = (token: string, new_password: string) =>
  api<void>("/auth/password/reset/confirm", {
    method: "POST",
    body: { token, new_password },
  });
