// fix/admin-password-reset: the admin's "reset pw" now mints a URL that
// actually lands on the reset page (/reset?token=…, not the ignored
// /login?reset=…) and shows it in a copyable field instead of relying on
// navigator.clipboard (absent on plain-http deployments).
//
// Pre-reqs: dev stack up (`just up`) + `just seed` (demo@sprintly.local /
// sprintly is the seeded global admin), SPRINTLY_OPEN_SIGNUP=true.

import { test, expect, type Page } from "@playwright/test";

function rand(): string {
  return Math.random().toString(36).slice(2, 8);
}

async function fill(page: Page, label: string, value: string) {
  await page.getByLabel(label, { exact: false }).fill(value);
}

test.describe("admin password reset", () => {
  test("mint a reset link, use it, sign in with the new password", async ({ page, browser }) => {
    const victim = `e2e${rand()}`;

    await test.step("a user exists", async () => {
      await page.goto("/register");
      await fill(page, "Display name", "Locked Out");
      await fill(page, "Handle", victim);
      await fill(page, "Email", `${victim}@sprintly.test`);
      await fill(page, "Password", "old-password-forgotten");
      await page.getByRole("button", { name: /\$ git init account/ }).click();
      await expect(page).toHaveURL(/\/(me\/day)?$/);
    });

    let resetUrl = "";
    await test.step("admin mints a reset link and can see it", async () => {
      const ctx = await browser.newContext();
      const admin = await ctx.newPage();
      await admin.goto("/login");
      await fill(admin, "Email", "demo@sprintly.local");
      await fill(admin, "Password", "sprintly");
      await admin.getByRole("button", { name: /\$ ssh sprintly/ }).click();
      await expect(admin).toHaveURL(/\/(me\/day)?$/);

      await admin.goto("/admin");
      await admin.getByPlaceholder("search handle / email / name").fill(victim);
      const row = admin.locator("li", { hasText: `@${victim}` });
      await row.getByRole("button", { name: /reset pw/i }).click();

      const banner = admin.getByTestId("reset-url-banner");
      await expect(banner).toBeVisible();
      await expect(banner.getByText(`@${victim}`)).toBeVisible();
      resetUrl = await banner.getByLabel("password reset url").inputValue();
      expect(resetUrl).toContain("/reset?token=");
      await ctx.close();
    });

    await test.step("the link works — set a new password", async () => {
      const ctx = await browser.newContext();
      const fresh = await ctx.newPage();
      // The minted URL carries the configured public host; keep the token,
      // use the test runner's base URL.
      const path = new URL(resetUrl).pathname + new URL(resetUrl).search;
      await fresh.goto(path);
      await fill(fresh, "New password", "brand-new-password-1");
      await fill(fresh, "Confirm password", "brand-new-password-1");
      await fresh.getByRole("button", { name: /\$ reset password/ }).click();
      await expect(fresh.getByText(/Password updated/i)).toBeVisible();

      await fresh.goto("/login");
      await fill(fresh, "Email", `${victim}@sprintly.test`);
      await fill(fresh, "Password", "brand-new-password-1");
      await fresh.getByRole("button", { name: /\$ ssh sprintly/ }).click();
      await expect(fresh).toHaveURL(/\/(me\/day)?$/);
      await ctx.close();
    });
  });
});
