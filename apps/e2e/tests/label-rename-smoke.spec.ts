// feat/label-rename: label names are editable inline in the label palette, and
// the new name follows onto every task that wore the old one (QA report 6:
// "implement inline editing support for task label names").
//
// Pre-reqs: dev stack up (`just up`), SPRINTLY_OPEN_SIGNUP=true.

import { test, expect, type Page } from "@playwright/test";

function rand(): string {
  return Math.random().toString(36).slice(2, 8);
}

async function fill(page: Page, label: string, value: string) {
  await page.getByLabel(label, { exact: false }).fill(value);
}

test.describe("label rename", () => {
  test("rename a label inline; tagged tasks follow, Esc cancels", async ({ page }) => {
    const handle = `e2e${rand()}`;
    const key = `LR${rand().slice(0, 3).toUpperCase()}`;

    await page.goto("/register");
    await fill(page, "Display name", "Renamer");
    await fill(page, "Handle", handle);
    await fill(page, "Email", `${handle}@sprintly.test`);
    await fill(page, "Password", "correct-horse-battery-staple");
    await page.getByRole("button", { name: /\$ git init account/ }).click();
    await expect(page).toHaveURL(/\/(me\/day)?$/);

    await page.goto("/projects");
    await page.getByRole("button", { name: /new project/i }).first().click();
    const create = page.getByRole("dialog");
    await create.getByLabel("Name").fill("Relabel");
    await create.getByLabel(/^Key/).fill(key);
    await create.getByRole("button", { name: /\$ git init project/ }).click();
    await expect(page).toHaveURL(new RegExp(`/projects/${key}$`));

    // A label, and a task wearing it.
    await page.getByRole("button", { name: /^labels$/i }).first().click();
    let palette = page.getByRole("dialog");
    await palette.getByPlaceholder("new label").fill("front");
    await palette.getByRole("button", { name: /^add$/ }).click();
    await expect(palette.getByText("front", { exact: true })).toBeVisible();
    await palette.getByRole("button", { name: /close/i }).click();

    await page.locator("[data-add-card-button]").first().click();
    await page.getByPlaceholder("card title").fill("paint the header");
    await page.getByRole("button", { name: /^add$/ }).click();
    await expect(page.locator(`[data-task-card="${key}-1"]`)).toBeVisible();
    await page.goto(`/tasks/${key}-1`);
    await page.getByLabel("add label").selectOption({ label: "front" });
    await expect(page.getByLabel(/remove front/i)).toBeVisible();

    await page.goto(`/projects/${key}`);
    await page.getByRole("button", { name: /^labels$/i }).first().click();
    palette = page.getByRole("dialog");

    await test.step("Esc throws the edit away", async () => {
      await palette.getByRole("button", { name: "edit name of front", exact: true }).click();
      const input = palette.getByLabel("rename front", { exact: true });
      await input.fill("nope");
      await input.press("Escape");
      await expect(palette.getByLabel("rename front", { exact: true })).toHaveCount(0);
      // The palette itself is still open, and the name unchanged.
      await expect(palette.getByRole("button", { name: "edit name of front", exact: true })).toBeVisible();
    });

    await test.step("Enter saves", async () => {
      await palette.getByRole("button", { name: "edit name of front", exact: true }).click();
      const input = palette.getByLabel("rename front", { exact: true });
      await input.fill("frontend");
      await input.press("Enter");
      await expect(palette.getByRole("button", { name: "edit name of frontend", exact: true })).toBeVisible();
      await expect(palette.getByRole("button", { name: "edit name of front", exact: true })).toHaveCount(0);
    });

    await test.step("the tagged task carries the new name", async () => {
      await palette.getByRole("button", { name: /close/i }).click();
      const card = page.locator(`[data-task-card="${key}-1"]`);
      await expect(card.getByText("frontend")).toBeVisible();
      await page.goto(`/tasks/${key}-1`);
      await expect(page.getByLabel(/remove frontend/i)).toBeVisible();
      await expect(page.getByLabel(/remove front$/i)).toHaveCount(0);
    });
  });
});
