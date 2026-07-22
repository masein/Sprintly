// feat/project-members-ui: the project page's "members" panel. A lead can add
// an existing user (found by email or handle), change their role, and remove
// them; a non-lead member sees the same list read-only.
//
// Two users: A (the lead who creates the project) and B (the invitee). B's
// email deliberately doesn't share B's handle prefix, so finding B by an
// "invitee…" query proves the email-prefix search, not handle matching.
//
// Pre-reqs: dev stack up (`just up`), SPRINTLY_OPEN_SIGNUP=true.

import { test, expect, type Page } from "@playwright/test";

function rand(): string {
  return Math.random().toString(36).slice(2, 8);
}

async function fill(page: Page, label: string, value: string) {
  await page.getByLabel(label, { exact: false }).fill(value);
}

async function register(page: Page, u: { display: string; handle: string; email: string; password: string }) {
  await page.goto("/register");
  await fill(page, "Display name", u.display);
  await fill(page, "Handle", u.handle);
  await fill(page, "Email", u.email);
  await fill(page, "Password", u.password);
  await page.getByRole("button", { name: /\$ git init account/ }).click();
  await expect(page).toHaveURL("/");
}

async function logout(page: Page) {
  await page.goto("/");
  await page.getByRole("button", { name: /logout/i }).click();
  await expect(page.getByText("sign in").first()).toBeVisible();
}

async function login(page: Page, email: string, password: string) {
  await page.goto("/login");
  await fill(page, "Email", email);
  await fill(page, "Password", password);
  await page.getByRole("button", { name: /\$ ssh sprintly/ }).click();
  await expect(page).toHaveURL("/");
}

test.describe("project members UI", () => {
  test("lead adds by email, changes role, removes; member sees read-only", async ({ page }) => {
    const suffix = rand();
    const password = "correct-horse-battery-staple";
    const bHandle = `e2e${suffix}`;
    const bEmail = `invitee${suffix}@sprintly.test`; // prefix differs from handle
    const aHandle = `lead${suffix}`;
    const key = `MB${rand().slice(0, 3).toUpperCase()}`;

    await test.step("register B (the invitee), then log out", async () => {
      await register(page, { display: "Bianca Invitee", handle: bHandle, email: bEmail, password });
      await logout(page);
    });

    await test.step("register A (lead) and create a project", async () => {
      await register(page, { display: "Aled Lead", handle: aHandle, email: `${aHandle}@sprintly.test`, password });
      await page.goto("/projects");
      await page.getByRole("button", { name: /new project/i }).first().click();
      const dialog = page.getByRole("dialog");
      await dialog.getByLabel("Name").fill("Members");
      await dialog.getByLabel(/^Key/).fill(key);
      await dialog.getByRole("button", { name: /\$ git init project/ }).click();
      await expect(page).toHaveURL(new RegExp(`/projects/${key}$`));
    });

    await test.step("lead adds B by email-prefix typeahead", async () => {
      await page.getByRole("button", { name: /^members$/i }).click();
      const dialog = page.getByRole("dialog");
      await expect(dialog.getByText(/Who's on this project/i)).toBeVisible();
      // Type part of B's EMAIL (not handle) — exercises email search.
      await dialog.getByLabel("find a user to add").fill(`invitee${suffix}`);
      await dialog.getByRole("button", { name: new RegExp(`@${bHandle}`) }).click();
      await expect(dialog.getByText(`@${bHandle}`, { exact: true })).toBeVisible();
    });

    await test.step("lead changes B's role to watcher", async () => {
      const dialog = page.getByRole("dialog");
      await dialog.getByLabel(`role for @${bHandle}`).selectOption("watcher");
      await expect(dialog.getByLabel(`role for @${bHandle}`)).toHaveValue("watcher");
    });

    await test.step("lead removes B, then adds them back (remove works, keeps B a member)", async () => {
      const dialog = page.getByRole("dialog");
      page.once("dialog", (d) => d.accept());
      await dialog.getByRole("button", { name: `remove @${bHandle}` }).click();
      await expect(dialog.getByText(`@${bHandle}`, { exact: true })).toHaveCount(0);
      // Re-add by handle this time.
      await dialog.getByLabel("find a user to add").fill(bHandle);
      await dialog.getByRole("button", { name: new RegExp(`@${bHandle}`) }).click();
      await expect(dialog.getByText(`@${bHandle}`, { exact: true })).toBeVisible();
    });

    await test.step("B logs in and sees the members list read-only", async () => {
      await logout(page);
      await login(page, bEmail, password);
      await page.goto(`/projects/${key}`);
      await page.getByRole("button", { name: /^members$/i }).click();
      const dialog = page.getByRole("dialog");
      // B's own row reads "@handle · you", so anchor instead of exact-match.
      await expect(dialog.getByText(new RegExp(`^@${bHandle}`))).toBeVisible();
      await expect(dialog.getByText(`@${aHandle}`, { exact: true })).toBeVisible();
      // Read-only: no add box, no role dropdowns, no remove buttons.
      await expect(dialog.getByLabel("find a user to add")).toHaveCount(0);
      await expect(dialog.getByLabel(`role for @${bHandle}`)).toHaveCount(0);
      await expect(dialog.getByRole("button", { name: `remove @${bHandle}` })).toHaveCount(0);
    });
  });
});
