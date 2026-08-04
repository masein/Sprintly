// feat/project-key-rename: a lead can change the project key; every task key
// is rewritten with it and the page lands on the new URL. QA report 2:
// "Project key cannot be modified."
//
// Pre-reqs: dev stack up (`just up`), SPRINTLY_OPEN_SIGNUP=true.

import { test, expect, type Page } from "@playwright/test";

function rand(): string {
  return Math.random().toString(36).slice(2, 8);
}

async function fill(page: Page, label: string, value: string) {
  await page.getByLabel(label, { exact: false }).fill(value);
}

test.describe("project key rename", () => {
  test("rename cascades to task keys and navigates to the new URL", async ({ page }) => {
    const handle = `e2e${rand()}`;
    const oldKey = `KA${rand().slice(0, 3).toUpperCase()}`;
    const newKey = `KB${rand().slice(0, 3).toUpperCase()}`;
    page.on("dialog", (d) => d.accept());

    await test.step("register + project + a task", async () => {
      await page.goto("/register");
      await fill(page, "Display name", "Key Renamer");
      await fill(page, "Handle", handle);
      await fill(page, "Email", `${handle}@sprintly.test`);
      await fill(page, "Password", "correct-horse-battery-staple");
      await page.getByRole("button", { name: /\$ git init account/ }).click();
      await expect(page).toHaveURL(/\/(me\/day)?$/);

      await page.goto("/projects");
      await page.getByRole("button", { name: /new project/i }).first().click();
      const dialog = page.getByRole("dialog");
      await dialog.getByLabel("Name").fill("Rename Me");
      await dialog.getByLabel(/^Key/).fill(oldKey);
      await dialog.getByRole("button", { name: /\$ git init project/ }).click();
      await expect(page).toHaveURL(new RegExp(`/projects/${oldKey}$`));

      await page.goto(`/projects/${oldKey}/backlog`);
      await page.locator("[data-backlog-quick-add]").click();
      const input = page.getByLabel("new task title");
      await input.fill("identity crisis");
      await input.press("Enter");
      await expect(page.getByText("identity crisis")).toBeVisible();
    });

    await test.step("rename the key from the project header", async () => {
      await page.goto(`/projects/${oldKey}`);
      await page.getByRole("button", { name: "Change project key" }).click();
      await page.getByLabel("new project key").fill(newKey);
      await page.getByRole("button", { name: "rename", exact: true }).click();
      await expect(page).toHaveURL(new RegExp(`/projects/${newKey}$`), { timeout: 10_000 });
    });

    await test.step("the task carries the new key; the old one is gone", async () => {
      await page.goto(`/tasks/${newKey}-1`);
      await expect(page.getByText("identity crisis").first()).toBeVisible();

      await page.goto(`/tasks/${oldKey}-1`);
      await expect(page.getByText(/not found/i).first()).toBeVisible();
    });
  });
});
