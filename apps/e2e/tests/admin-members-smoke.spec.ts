// fix/admin-manage-members: a global admin can add project members and change
// their roles from the members panel — even on a project they're not a member
// of. The API always allowed this (admin short-circuits permission checks);
// the UI used to show admins the read-only view.
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

async function register(page: Page, handle: string, name: string) {
  await page.goto("/register");
  await fill(page, "Display name", name);
  await fill(page, "Handle", handle);
  await fill(page, "Email", `${handle}@sprintly.test`);
  await fill(page, "Password", "correct-horse-battery-staple");
  await page.getByRole("button", { name: /\$ git init account/ }).click();
  await expect(page).toHaveURL("/");
}

test.describe("admin manages project members", () => {
  test("the seeded admin adds a member and changes their role on someone else's project", async ({ page, browser }) => {
    const lead = `e2e${rand()}`;
    const target = `e2e${rand()}`;
    const key = `AM${rand().slice(0, 3).toUpperCase()}`;
    page.on("dialog", (d) => d.accept());

    await test.step("a lead creates a project; a target user merely exists", async () => {
      await register(page, lead, "Project Lead");
      await page.goto("/projects");
      await page.getByRole("button", { name: /new project/i }).first().click();
      const dialog = page.getByRole("dialog");
      await dialog.getByLabel("Name").fill("Admin Managed");
      await dialog.getByLabel(/^Key/).fill(key);
      await dialog.getByRole("button", { name: /\$ git init project/ }).click();
      await expect(page).toHaveURL(new RegExp(`/projects/${key}$`));

      const ctxB = await browser.newContext();
      const pageB = await ctxB.newPage();
      await register(pageB, target, "Future Member");
      await ctxB.close();
    });

    const ctxAdmin = await browser.newContext();
    const admin = await ctxAdmin.newPage();
    admin.on("dialog", (d) => d.accept());

    await test.step("the global admin opens the project's members panel", async () => {
      await admin.goto("/login");
      await fill(admin, "Email", "demo@sprintly.local");
      await fill(admin, "Password", "sprintly");
      await admin.getByRole("button", { name: /\$ ssh sprintly/ }).click();
      await expect(admin).toHaveURL("/");

      await admin.goto(`/projects/${key}`);
      await admin.getByRole("button", { name: /members/i }).click();
      // The manage controls are there now — not the old read-only view.
      await expect(admin.getByLabel("find a user to add")).toBeVisible();
    });

    await test.step("admin adds the target user", async () => {
      await admin.getByLabel("find a user to add").fill(target);
      await admin.getByRole("button", { name: new RegExp(`@${target}`) }).click();
      await expect(
        admin.getByText(`@${target}`, { exact: false }).first(),
      ).toBeVisible();
    });

    await test.step("admin changes the new member's role to lead", async () => {
      const roleSelect = admin.getByLabel(`role for @${target}`);
      await expect(roleSelect).toHaveValue("contributor");
      await roleSelect.selectOption("lead");
      await expect(roleSelect).toHaveValue("lead");
    });

    await test.step("the change is real — the lead sees the new member", async () => {
      await page.goto(`/projects/${key}`);
      await page.getByRole("button", { name: /members/i }).click();
      await expect(page.getByText(`@${target}`).first()).toBeVisible();
      await expect(page.getByLabel(`role for @${target}`)).toHaveValue("lead");
    });

    await ctxAdmin.close();
  });
});
