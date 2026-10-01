// feat/copy-task-link: a link icon next to task keys copies the task's URL —
// on the task page, board cards, backlog rows and sprint rows (QA report 6:
// "add a quick-copy task link near task identifiers").
//
// Pre-reqs: dev stack up (`just up`), SPRINTLY_OPEN_SIGNUP=true.

import { test, expect, type Page } from "@playwright/test";

function rand(): string {
  return Math.random().toString(36).slice(2, 8);
}

async function fill(page: Page, label: string, value: string) {
  await page.getByLabel(label, { exact: false }).fill(value);
}

async function clipboard(page: Page): Promise<string> {
  return page.evaluate(() => navigator.clipboard.readText());
}

test.describe("copy task link", () => {
  test("the link icon next to a key copies that task's URL", async ({ page, context, baseURL }) => {
    await context.grantPermissions(["clipboard-read", "clipboard-write"]);
    const handle = `e2e${rand()}`;
    const key = `CL${rand().slice(0, 3).toUpperCase()}`;

    await page.goto("/register");
    await fill(page, "Display name", "Linker");
    await fill(page, "Handle", handle);
    await fill(page, "Email", `${handle}@sprintly.test`);
    await fill(page, "Password", "correct-horse-battery-staple");
    await page.getByRole("button", { name: /\$ git init account/ }).click();
    await expect(page).toHaveURL(/\/(me\/day)?$/);

    await page.goto("/projects");
    await page.getByRole("button", { name: /new project/i }).first().click();
    const dialog = page.getByRole("dialog");
    await dialog.getByLabel("Name").fill("Links");
    await dialog.getByLabel(/^Key/).fill(key);
    await dialog.getByRole("button", { name: /\$ git init project/ }).click();
    await expect(page).toHaveURL(new RegExp(`/projects/${key}$`));

    await page.goto(`/projects/${key}/backlog`);
    await page.locator("[data-backlog-quick-add]").click();
    const input = page.getByLabel("new task title");
    await input.fill("share me");
    await input.press("Enter");
    await expect(page.getByText("share me")).toBeVisible();

    const expected = `${new URL(baseURL!).origin}/tasks/${key}-1`;

    await test.step("from a backlog row", async () => {
      await page.getByRole("button", { name: `copy link to ${key}-1` }).click();
      await expect(page.getByRole("status").filter({ hasText: `Copied a link to ${key}-1` })).toBeVisible();
      expect(await clipboard(page)).toBe(expected);
    });

    await test.step("from the task page, next to the key in the breadcrumbs", async () => {
      await page.evaluate(() => navigator.clipboard.writeText(""));
      await page.goto(`/tasks/${key}-1`);
      await page.getByRole("button", { name: `copy link to ${key}-1` }).click();
      expect(await clipboard(page)).toBe(expected);
      // Copying doesn't navigate anywhere.
      await expect(page).toHaveURL(new RegExp(`/tasks/${key}-1$`));
    });

    await test.step("from a board card, without opening the card", async () => {
      await page.evaluate(() => navigator.clipboard.writeText(""));
      await page.goto(`/projects/${key}`);
      const card = page.locator(`[data-task-card="${key}-1"]`);
      await card.hover();
      await card.getByRole("button", { name: `copy link to ${key}-1` }).click();
      expect(await clipboard(page)).toBe(expected);
      await expect(page).toHaveURL(new RegExp(`/projects/${key}$`));
    });
  });
});
