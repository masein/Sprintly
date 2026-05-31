// Single import surface for components: pulls together the auth calls and
// the ApiError type so call sites don't import from two files.

export { login, register, logout, me, type Me, type AuthResponse } from "./auth";
export type { ApiError } from "./api";
