// feat/forgot-password: /login can start a password reset. The endpoints and
// the /reset page shipped in M1, but nothing in the UI ever called them — a
// forgotten password meant asking an admin to mint a link.
//
// Pre-reqs: dev stack up (`just up`), SPRINTLY_OPEN_SIGNUP=true.

import { test, expect, type Page } from "@playwright/test";

function rand(): string {
  return Math.random().toString(36).slice(2, 8);
}

async function fill(page: Page, label: string, value: string) {
  await page.getByLabel(label, { exact: false }).fill(value);
}

test.describe("forgot password", () => {
  test("ask for a link, then use it to set a new password", async ({ page }) => {
    const handle = `e2e${rand()}`;
    const email = `${handle}@sprintly.test`;

    // An account to forget the password of.
    await page.goto("/register");
    await fill(page, "Display name", "Forgetful");
    await fill(page, "Handle", handle);
    await fill(page, "Email", email);
    await fill(page, "Password", "correct-horse-battery-staple");
    await page.getByRole("button", { name: /\$ git init account/ }).click();
    await expect(page).toHaveURL(/\/(me\/day)?$/);

    // Sign out so we're a stranger at the door.
    await page.goto("/login");

    // The request returns the token in dev (no SMTP configured), which is what
    // lets this spec walk the whole flow instead of stopping at "sent".
    const requested = page.waitForResponse(
      (r) => r.url().includes("/auth/password/reset/request") && r.request().method() === "POST",
    );
    await page.getByRole("button", { name: /forgot your password\?/ }).click();
    await fill(page, "Email", email);
    await page.getByRole("button", { name: /\$ mail me a link/ }).click();

    const res = await requested;
    expect(res.status()).toBe(200);
    const body = (await res.json()) as { message: string; dev_token?: string };
    // Never leak whether the address exists.
    expect(body.message).toMatch(/if that account exists/i);
    await expect(page.getByTestId("reset-sent")).toBeVisible();

    const token = body.dev_token;
    expect(token, "dev builds return the token so this is testable").toBeTruthy();

    // Walk the link the email would have contained.
    await page.goto(`/reset?token=${token}`);
    await fill(page, "New password", "a-brand-new-passphrase-99");
    await fill(page, "Confirm password", "a-brand-new-passphrase-99");
    await page.getByRole("button", { name: /\$ reset password/ }).click();
    await expect(page).toHaveURL(/\/login/, { timeout: 15_000 });

    // The new password works.
    await fill(page, "Email", email);
    await fill(page, "Password", "a-brand-new-passphrase-99");
    await page.getByRole("button", { name: /\$ ssh sprintly/ }).click();
    await expect(page).toHaveURL(/\/(me\/day)?$/);
  });

  test("an unknown address is answered exactly like a known one", async ({ page }) => {
    await page.goto("/login");
    const requested = page.waitForResponse((r) =>
      r.url().includes("/auth/password/reset/request"),
    );
    await page.getByRole("button", { name: /forgot your password\?/ }).click();
    await fill(page, "Email", `nobody-${rand()}@sprintly.test`);
    await page.getByRole("button", { name: /\$ mail me a link/ }).click();

    const res = await requested;
    expect(res.status()).toBe(200);
    const body = (await res.json()) as { message: string; dev_token?: string };
    expect(body.message).toMatch(/if that account exists/i);
    // No token for an address with no account — that's the tell we must not give.
    expect(body.dev_token).toBeUndefined();
    await expect(page.getByTestId("reset-sent")).toBeVisible();
  });
});
