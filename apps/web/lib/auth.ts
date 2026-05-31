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

export const login = (p: LoginPayload) =>
  api<AuthResponse>("/auth/login", { method: "POST", body: p });

export const register = (p: RegisterPayload) =>
  api<AuthResponse>("/auth/register", { method: "POST", body: p });

export const logout = () => api<void>("/auth/logout", { method: "POST" });

export const me = () => api<Me>("/users/me");
