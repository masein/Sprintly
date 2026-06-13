// F11 smoke: TOTP two-factor, end to end.
//
//   1. register a fresh account → /settings
//   2. start 2FA enrolment → read the base32 setup key from the QR step
//   3. compute the live TOTP code (Node crypto) → confirm → 2FA is on
//   4. save-recovery-codes step appears; dismiss it
//   5. logout, then log back in: password alone now triggers a code prompt
//   6. a wrong code is rejected; the correct computed code completes the login
//
// This proves the core AC — enrol → subsequent logins require a valid TOTP —
// against the running stack. The recovery-code single-use path is covered by
// the backend integration tests.
//
// Pre-reqs: dev stack up (`just up`), SPRINTLY_OPEN_SIGNUP=true.

import { test, expect, type Page } from "@playwright/test";
import crypto from "node:crypto";

function rand(): string {
  return Math.random().toString(36).slice(2, 8);
}

// RFC 4648 base32 decode (no padding) — matches the API's encoder.
function base32Decode(s: string): Buffer {
  const alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
  let bits = 0;
  let value = 0;
  const out: number[] = [];
  for (const ch of s.replace(/=+$/, "").toUpperCase()) {
    const idx = alphabet.indexOf(ch);
    if (idx === -1) continue;
    value = (value << 5) | idx;
    bits += 5;
    if (bits >= 8) {
      out.push((value >>> (bits - 8)) & 0xff);
      bits -= 8;
    }
  }
  return Buffer.from(out);
}

// Current 6-digit TOTP for a base32 secret (RFC 6238, SHA1/6/30).
function totp(secretB32: string, atMs = Date.now()): string {
  const key = base32Decode(secretB32);
  let counter = Math.floor(atMs / 1000 / 30);
  const buf = Buffer.alloc(8);
  for (let i = 7; i >= 0; i--) {
    buf[i] = counter & 0xff;
    counter = Math.floor(counter / 256);
  }
  const hmac = crypto.createHmac("sha1", key).update(buf).digest();
  const offset = hmac[19] & 0x0f;
  const bin =
    ((hmac[offset] & 0x7f) << 24) |
    (hmac[offset + 1] << 16) |
    (hmac[offset + 2] << 8) |
    hmac[offset + 3];
  return (bin % 1_000_000).toString().padStart(6, "0");
}

test.describe("F11 two-factor smoke", () => {
  test("enrol with TOTP, then logins require a code", async ({ page }) => {
    const handle = `e2e${rand()}`;
    const email = `${handle}@sprintly.test`;
    const password = "correct-horse-battery-staple";

    await test.step("register", async () => {
      await page.goto("/register");
      await fill(page, "Display name", "TwoFactor Tester");
      await fill(page, "Handle", handle);
      await fill(page, "Email", email);
      await fill(page, "Password", password);
      await page.getByRole("button", { name: /\$ git init account/ }).click();
      await expect(page).toHaveURL("/");
    });

    let secret = "";
    await test.step("start enrolment and read the setup key", async () => {
      await page.goto("/settings");
      await page.getByRole("button", { name: /set up two-factor/i }).click();
      const code = page.getByTestId("totp-secret");
      await expect(code).toBeVisible();
      secret = ((await code.textContent()) ?? "").trim();
      expect(secret.length).toBeGreaterThan(10);
    });

    await test.step("confirm with a live code → 2FA turns on", async () => {
      await page.getByPlaceholder("123456").fill(totp(secret));
      await page.getByRole("button", { name: /verify & turn on/i }).click();
      // Recovery codes appear exactly once.
      await expect(page.getByText(/save these recovery codes/i)).toBeVisible();
      await page.getByRole("button", { name: /i've saved them/i }).click();
      // The disable control only renders once 2FA is on.
      await expect(page.getByRole("button", { name: /disable/i })).toBeVisible();
    });

    await test.step("logout", async () => {
      await page.goto("/");
      await page.getByRole("button", { name: /logout/i }).click();
      await expect(page.getByText("sign in").first()).toBeVisible();
    });

    await test.step("login now demands a second factor", async () => {
      await page.goto("/login");
      await fill(page, "Email", email);
      await fill(page, "Password", password);
      await page.getByRole("button", { name: /\$ ssh sprintly/ }).click();
      // Step-up screen, not the app.
      await expect(page.getByText(/authenticator app/i)).toBeVisible();
      await expect(page).toHaveURL(/\/login$/);
    });

    await test.step("a wrong code is rejected, the right one gets in", async () => {
      await page.getByLabel(/authentication code/i).fill("000000");
      await page.getByRole("button", { name: /\$ verify/ }).click();
      await expect(page.getByText(/didn't work/i)).toBeVisible();

      await page.getByLabel(/authentication code/i).fill(totp(secret));
      await page.getByRole("button", { name: /\$ verify/ }).click();
      await expect(page).toHaveURL("/");
      await expect(page.getByText(`@${handle}`)).toBeVisible();
    });
  });
});

async function fill(page: Page, label: string, value: string) {
  await page.getByLabel(label, { exact: false }).fill(value);
}
