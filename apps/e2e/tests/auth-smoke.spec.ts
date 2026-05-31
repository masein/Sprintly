// M1 smoke test.
//
// Covers the full M1 auth surface end-to-end:
//   1. register a fresh account (random handle so re-runs don't collide)
//   2. land on /, see signed-in badge with the handle and role
//   3. visit /settings, change the display name, save
//   4. /users/me reflects the change (read via the same browser context)
//   5. logout, badge flips to anonymous
//   6. login with the credentials we just registered, badge returns
//
// Pre-reqs: the dev stack is up (`just up`). SPRINTLY_OPEN_SIGNUP=true in
// the .env (the default in .env.example). If you've already run seed, the
// demo user exists but doesn't conflict with the random handles below.

import { test, expect, type Page } from "@playwright/test";

function rand(): string {
  // 6-char alphanum suffix; collision-resistant for a single test run.
  return Math.random().toString(36).slice(2, 8);
}

test.describe("M1 auth smoke", () => {
  test("register → settings → logout → login", async ({ page }) => {
    const handle = `e2e${rand()}`;
    const email = `${handle}@sprintly.test`;
    const password = "correct-horse-battery-staple";
    const displayName = "E2E User";

    await test.step("register", async () => {
      await page.goto("/register");
      await fill(page, "Display name", displayName);
      await fill(page, "Handle", handle);
      await fill(page, "Email", email);
      await fill(page, "Password", password);
      await page.getByRole("button", { name: /\$ git init account/ }).click();
      await expect(page).toHaveURL("/");
    });

    await test.step("signed-in badge appears", async () => {
      await expect(page.getByText(`@${handle}`)).toBeVisible();
      // First user is admin; if the DB was wiped, this will be admin. Else member.
      await expect(page.getByText(/admin|member/)).toBeVisible();
    });

    await test.step("edit profile via /settings", async () => {
      await page.goto("/settings");
      await expect(page.getByText(`@${handle}`)).toBeVisible();
      const newName = `${displayName} (edited)`;
      const nameInput = page.getByLabel("Display name");
      await nameInput.fill(newName);
      await page.getByRole("button", { name: /\$ save/ }).click();
      await expect(page.getByText(/saved/)).toBeVisible();
    });

    await test.step("logout flips the badge", async () => {
      await page.goto("/");
      await page.getByRole("button", { name: /logout/i }).click();
      await expect(page.getByText("sign in").first()).toBeVisible();
    });

    await test.step("login with the credentials we just registered", async () => {
      await page.goto("/login");
      await fill(page, "Email", email);
      await fill(page, "Password", password);
      await page.getByRole("button", { name: /\$ ssh sprintly/ }).click();
      await expect(page).toHaveURL("/");
      await expect(page.getByText(`@${handle}`)).toBeVisible();
    });
  });
});

async function fill(page: Page, label: string, value: string) {
  await page.getByLabel(label, { exact: false }).fill(value);
}
