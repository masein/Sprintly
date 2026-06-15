// QA F4/F8 fix: subtask hierarchy.
//
// Create a task, add a subtask under it, then assert the subtask does NOT
// appear as an independent top-level card on the board, and that the subtask's
// detail page links back to its parent.
//
// Pre-reqs: dev stack up (`just up`), SPRINTLY_OPEN_SIGNUP=true.

import { test, expect, type Page } from "@playwright/test";

function rand(): string {
  return Math.random().toString(36).slice(2, 8);
}

test.describe("QA F4/F8 — subtask hierarchy", () => {
  test("a subtask is not a top-level card and links its parent", async ({ page }) => {
    const handle = `e2e${rand()}`;
    const key = `ST${rand().slice(0, 3).toUpperCase()}`;

    await test.step("register", async () => {
      await page.goto("/register");
      await fill(page, "Display name", "Subtask Tester");
      await fill(page, "Handle", handle);
      await fill(page, "Email", `${handle}@sprintly.test`);
      await fill(page, "Password", "correct-horse-battery-staple");
      await page.getByRole("button", { name: /\$ git init account/ }).click();
      await expect(page).toHaveURL("/");
    });

    await test.step("create a project", async () => {
      await page.goto("/projects");
      await page.getByRole("button", { name: /new project/i }).first().click();
      const dialog = page.getByRole("dialog");
      await dialog.getByLabel("Name").fill("Subtasks");
      await dialog.getByLabel(/^Key/).fill(key);
      await dialog.getByRole("button", { name: /\$ git init project/ }).click();
      await expect(page).toHaveURL(new RegExp(`/projects/${key}$`));
    });

    await test.step("add a parent card and open it", async () => {
      await page.locator("[data-add-card-button]").first().click();
      await page.getByPlaceholder("card title").fill("Parent work");
      await page.getByRole("button", { name: /^add$/ }).click();
      await expect(page.getByText("Parent work")).toBeVisible();
      // Parent is the first task → KEY-1.
      await page.getByRole("link", { name: `${key}-1` }).first().click();
      await expect(page).toHaveURL(new RegExp(`/tasks/${key}-1`));
    });

    await test.step("add a subtask under it", async () => {
      await page.getByRole("button", { name: /add subtask/i }).click();
      await page.getByPlaceholder("subtask title").fill("Child work");
      await page.getByRole("button", { name: /^add$/ }).click();
      // It appears in the SUBTASKS panel (title text + its own key link).
      await expect(page.getByText("Child work")).toBeVisible();
      await expect(page.getByRole("link", { name: `${key}-2` })).toBeVisible();
    });

    await test.step("the subtask is NOT a top-level board card", async () => {
      await page.goto(`/projects/${key}`);
      await expect(page.getByText("Parent work")).toBeVisible();
      await expect(page.getByText("Child work")).toHaveCount(0);
      // The To do column still counts one card, not two.
      await expect(page.getByText(`${key}-2`)).toHaveCount(0);
    });

    await test.step("the subtask page links its parent", async () => {
      await page.goto(`/tasks/${key}-2`);
      await expect(page).toHaveURL(new RegExp(`/tasks/${key}-2`));
      // The parent key is linked (breadcrumb + the DETAILS "parent" row).
      await expect(page.getByRole("link", { name: `${key}-1` }).first()).toBeVisible();
    });
  });
});

async function fill(page: Page, label: string, value: string) {
  await page.getByLabel(label, { exact: false }).fill(value);
}
