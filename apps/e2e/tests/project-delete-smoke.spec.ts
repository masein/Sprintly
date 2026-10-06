// Project deletion: the API soft-deletes a project only when the key is typed
// back as confirmation, and a global admin can restore it. The project page
// offers no delete button — review of the QA 6 screenshots: what was wanted
// was undo for deleted *tasks* (board-delete-undo-live), not a red button on
// every board header.
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

async function csrf(page: Page): Promise<string> {
  const cookies = await page.context().cookies();
  return cookies.find((c) => c.name === "sprintly_csrf")?.value ?? "";
}

test.describe("project deletion", () => {
  test("no delete on the board header; the API wants the key; an admin can bring it back", async ({ page, browser }) => {
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

    await test.step("the project header offers archive, not delete", async () => {
      await expect(page.getByRole("button", { name: /^archive$/ })).toBeVisible();
      await expect(page.getByRole("button", { name: /^delete$/ })).toHaveCount(0);
    });

    await test.step("the API deletes only with the key typed back", async () => {
      const token = await csrf(page);
      const del = (confirm: string) =>
        page.request.fetch(`/api/v1/projects/${key}`, {
          method: "DELETE",
          data: { confirm },
          headers: { "X-CSRF-Token": token },
        });
      expect((await del(key.toLowerCase())).status()).toBe(400);
      const ok = await del(key);
      expect(ok.status(), await ok.text()).toBe(204);
      // Its tasks are gone with it.
      const res = await page.request.get(`/api/v1/tasks/${key}-1`);
      expect(res.status()).toBe(404);
      await page.goto("/projects");
      await expect(page.getByRole("link", { name: new RegExp(key) })).toHaveCount(0);
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
