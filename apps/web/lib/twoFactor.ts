// Two-factor auth (F11) client calls — enrolment/management from settings.
// The login step-up call lives in lib/auth.ts next to login().

import { api } from "./api";

export type TwoFactorStatus = {
  enabled: boolean;
  has_secret: boolean;
  required: boolean;
};

export type EnrollResponse = {
  /** base32 secret for manual entry. */
  secret: string;
  /** otpauth:// URI to render as a QR. */
  otpauth_uri: string;
};

export const getTwoFactorStatus = () => api<TwoFactorStatus>("/me/2fa");

export const enrollTwoFactor = () =>
  api<EnrollResponse>("/me/2fa/enroll", { method: "POST" });

export const activateTwoFactor = (code: string) =>
  api<{ recovery_codes: string[] }>("/me/2fa/activate", {
    method: "POST",
    body: { code },
  });

export const disableTwoFactor = (code: string) =>
  api<{ disabled: boolean }>("/me/2fa/disable", {
    method: "POST",
    body: { code },
  });
