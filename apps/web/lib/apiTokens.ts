// Personal API tokens — managed in /settings. The secret appears exactly
// once, in the create response.

import { api } from "./api";

export type ApiTokenScope = "read" | "write";

export type ApiToken = {
  id: string;
  name: string;
  scopes: ApiTokenScope[];
  last_used_at: string | null;
  expires_at: string | null;
  created_at: string;
};

export const listApiTokens = () => api<ApiToken[]>("/me/tokens");

export const createApiToken = (body: {
  name: string;
  scopes: ApiTokenScope[];
  expires_at?: string;
}) => api<{ token: ApiToken; secret: string }>("/me/tokens", { method: "POST", body });

export const revokeApiToken = (id: string) =>
  api<void>(`/me/tokens/${id}`, { method: "DELETE" });
