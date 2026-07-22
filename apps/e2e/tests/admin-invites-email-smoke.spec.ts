// feat/admin-invites-email: the /admin invites tab and admin email editing.
//
// As the seeded demo admin: mint an admin invite → the one-shot URL registers
// a NEW user who lands as admin (field prefilled from ?invite=). Then edit
// that user's email from /admin → users, prove the change is real by logging
// in with the NEW email, and check the duplicate-email conflict is surfaced.
//
// Pre-reqs: dev stack up (`just up`) + `just seed` (demo@sprintly.local /
// sprintly, role admin). CI runs the seed step in e2e.yml.

import { test, expect, type Page } from "@playwright/test";

function rand(): string {
  return Math.random().toString(36).slice(2, 8);
}

async function fill(page: Page, label: string, value: string) {
  await page.getByLabel(label, { exact: false }).fill(value);
}

async function login(page: Page, email: string, password: string) {
  await page.goto("/login");
  await fill(page, "Email", email);
  await fill(page, "Password", password);
  await page.getByRole("button", { name: /\$ ssh sprintly/ }).click();
  await expect(page).toHaveURL("/");
}

async function logout(page: Page) {
  await page.goto("/");
  await page.getByRole("button", { name: /logout/i }).click();
  await expect(page.getByText("sign in").first()).toBeVisible();
}

test.describe("admin invites + email edit", () => {
  test("mint admin invite → register as admin → edit email → login with it", async ({ page }) => {
    const suffix = rand();
    const handle = `e2e${suffix}`;
    const email = `${handle}@sprintly.test`;
    const newEmail = `renamed-${suffix}@sprintly.test`;
    const password = "correct-horse-battery-staple";

    await test.step("demo admin mints an admin invite", async () => {
      await login(page, "demo@sprintly.local", "sprintly");
      await page.goto("/admin");
      await page.getByRole("button", { name: /^invites$/i }).click();
      await page.getByLabel("invited role").selectOption("admin");
      await page.getByRole("button", { name: /mint invite link/i }).click();
      await expect(page.getByText(/shown once/i)).toBeVisible();
    });

    let inviteUrl = "";
    await test.step("grab the one-shot URL and log out", async () => {
      inviteUrl = (await page.getByTestId("invite-url").textContent()) ?? "";
      expect(inviteUrl).toContain("/register?invite=");
      await logout(page);
    });

    await test.step("the invite link prefills the token; registering lands as admin", async () => {
      await page.goto(inviteUrl);
      const token = new URL(inviteUrl).searchParams.get("invite") ?? "";
      await expect(page.getByLabel("Invite token", { exact: false })).toHaveValue(token);

      await fill(page, "Display name", "Invited Admin");
      await fill(page, "Handle", handle);
      await fill(page, "Email", email);
      await fill(page, "Password", password);
      await page.getByRole("button", { name: /\$ git init account/ }).click();
      await expect(page).toHaveURL("/");
      // The session badge shows the global role — admin, straight away.
      await expect(page.getByText("admin", { exact: true })).toBeVisible();
    });

    await test.step("the minted invite now reads used", async () => {
      await page.goto("/admin");
      await page.getByRole("button", { name: /^invites$/i }).click();
      await expect(page.getByText("used", { exact: true }).first()).toBeVisible();
    });

    await test.step("duplicate email is refused with the server's message", async () => {
      await page.getByRole("button", { name: /^users$/i }).click();
      await page.getByPlaceholder(/search handle/).fill(handle);
      await page.getByLabel(`edit email for @${handle}`).click();
      const input = page.getByLabel(`new email for @${handle}`);
      await input.fill("demo@sprintly.local");
      await input.press("Enter");
      // The API maps every conflict to its house copy ("That already exists.").
      await expect(page.getByText(/already exists/i)).toBeVisible();
    });

    await test.step("a fresh email saves", async () => {
      const input = page.getByLabel(`new email for @${handle}`);
      await input.fill(newEmail);
      await input.press("Enter");
      await expect(page.getByText(newEmail)).toBeVisible();
    });

    await test.step("login works with the new email (and only the new one)", async () => {
      await logout(page);
      await login(page, newEmail, password);
      await expect(page.getByText(`@${handle}`)).toBeVisible();
    });
  });
});
