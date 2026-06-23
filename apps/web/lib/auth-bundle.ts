// Single import surface for components: pulls together the auth calls and
// the ApiError type so call sites don't import from two files.

export {
  login,
  register,
  logout,
  me,
  setMyAvatar,
  twoFactorLogin,
  isTwoFactorChallenge,
  isMustChangePassword,
  changePasswordForced,
  type Me,
  type AvatarPayload,
  type AuthResponse,
  type LoginResult,
} from "./auth";
export type { ApiError } from "./api";
