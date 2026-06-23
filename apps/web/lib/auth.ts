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
  avatar_style: string | null;
  avatar_seed: string | null;
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
/** Returned by /auth/login for a provisioned account that must set its own
 *  password before it gets a session (e.g. Jira-imported users). */
export type MustChangePasswordChallenge = {
  must_change_password_required: true;
  challenge: string;
};
export type LoginResult =
  | AuthResponse
  | TwoFactorChallenge
  | MustChangePasswordChallenge;

export const isTwoFactorChallenge = (r: LoginResult): r is TwoFactorChallenge =>
  "two_factor_required" in r && r.two_factor_required === true;

export const isMustChangePassword = (
  r: LoginResult,
): r is MustChangePasswordChallenge =>
  "must_change_password_required" in r && r.must_change_password_required === true;

export const login = (p: LoginPayload) =>
  api<LoginResult>("/auth/login", { method: "POST", body: p });

/** Complete a 2FA login with a TOTP or recovery code. */
export const twoFactorLogin = (challenge: string, code: string) =>
  api<AuthResponse>("/auth/2fa", { method: "POST", body: { challenge, code } });

/** Spend a force-reset challenge: set a new password and get a session. */
export const changePasswordForced = (challenge: string, new_password: string) =>
  api<AuthResponse>("/auth/password/change", {
    method: "POST",
    body: { challenge, new_password },
  });

export const register = (p: RegisterPayload) =>
  api<AuthResponse>("/auth/register", { method: "POST", body: p });

export const logout = () => api<void>("/auth/logout", { method: "POST" });

export const me = () => api<Me>("/users/me");

// Replace the whole avatar in one shot. `url` is an uploaded/linked image
// (data: or https:), `style`/`seed` describe the generated avatar. Send all
// null to revert to the deterministic default.
export type AvatarPayload = {
  url?: string | null;
  style?: string | null;
  seed?: string | null;
};
export const setMyAvatar = (p: AvatarPayload) =>
  api<Me>("/users/me/avatar", { method: "PUT", body: p });

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
