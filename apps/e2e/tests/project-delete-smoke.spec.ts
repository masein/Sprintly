// feat/project-delete: a lead can delete a project, behind a type-the-key
// confirmation, and a global admin can restore it (QA report 6: "a safe
// project deletion workflow with appropriate confirmation safeguards").
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

test.describe("project deletion", () => {
  test("type the key to delete; an admin can bring it back", async ({ page, browser }) => {
    const handle = `e2e${rand()}`;
    const key = `PD${rand().slice(0, 3).toUpperCase()}`;

    await page.goto("/register");
    await fill(page, "Display name", "Deleter");
    await fill(page, "Handle", handle);
    await fill(page, "Email", `${handle}@sprintly.test`);
    await fill(page, "Password", "correct-horse-battery-staple");
    await page.getByRole("button", { name: /\$ git init account/ }).click();
    await expect(page).toHaveURL(/\/(me\/day)?$/);

    await page.goto("/projects");
    await page.getByRole("button", { name: /new project/i }).first().click();
    const create = page.getByRole("dialog");
    await create.getByLabel("Name").fill("Doomed");
    await create.getByLabel(/^Key/).fill(key);
    await create.getByRole("button", { name: /\$ git init project/ }).click();
    await expect(page).toHaveURL(new RegExp(`/projects/${key}$`));
    await page.locator("[data-add-card-button]").first().click();
    await page.getByPlaceholder("card title").fill("survive the purge");
    await page.getByRole("button", { name: /^add$/ }).click();
    await expect(page.getByText("survive the purge")).toBeVisible();

    await test.step("the delete button asks for the key", async () => {
      await page.getByRole("button", { name: /^delete$/ }).click();
      const dialog = page.getByRole("dialog", { name: `delete ${key}` });
      await expect(dialog.getByText(/every task and subtask/)).toBeVisible();
      const confirm = dialog.getByRole("button", { name: `delete ${key}` });
      await expect(confirm).toBeDisabled();
      await dialog.getByLabel("type the project key to confirm").fill(key.toLowerCase());
      await expect(confirm).toBeDisabled();
      // Backing out leaves everything alone.
      await dialog.getByRole("button", { name: /keep it/ }).click();
      await expect(dialog).toHaveCount(0);
      await expect(page.getByText("survive the purge")).toBeVisible();
    });

    await test.step("typing the key exactly deletes it", async () => {
      await page.getByRole("button", { name: /^delete$/ }).click();
      const dialog = page.getByRole("dialog", { name: `delete ${key}` });
      await dialog.getByLabel("type the project key to confirm").fill(key);
      await dialog.getByRole("button", { name: `delete ${key}` }).click();
      await expect(page).toHaveURL(/\/projects$/);
      await expect(page.getByRole("status").filter({ hasText: `Deleted ${key}` })).toBeVisible();
      await expect(page.getByRole("link", { name: new RegExp(key) })).toHaveCount(0);
      // Its tasks are gone with it.
      const res = await page.request.get(`/api/v1/tasks/${key}-1`);
      expect(res.status()).toBe(404);
    });

    await test.step("an admin restores it", async () => {
      const ctx = await browser.newContext();
      const admin = await ctx.newPage();
      await admin.goto("/login");
      await fill(admin, "Email", "demo@sprintly.local");
      await fill(admin, "Password", "sprintly");
      await admin.getByRole("button", { name: /\$ ssh sprintly/ }).click();
      await expect(admin).toHaveURL(/\/(me\/day)?$/);
      await admin.goto("/admin");
      await admin.getByRole("button", { name: "projects", exact: true }).click();
      const tab = admin.getByRole("region", { name: "deleted projects" });
      await expect(tab.getByText(key, { exact: true })).toBeVisible();
      await tab.getByRole("button", { name: `restore ${key}` }).click();
      await expect(tab.getByText(`Restored ${key} with 1 task.`)).toBeVisible();
      await ctx.close();
    });

    await test.step("…and it's back for the lead, task and all", async () => {
      await page.goto(`/projects/${key}`);
      await expect(page.getByText("survive the purge")).toBeVisible();
    });
  });
});
