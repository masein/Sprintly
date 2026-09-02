// QA report 5: subtasks belong in the main column, not squeezed into the
// 280px sidebar; and truncated names (subtasks, attachments) show in full on
// hover.
//
// Pre-reqs: dev stack up (`just up`), SPRINTLY_OPEN_SIGNUP=true.

import { test, expect, type Page } from "@playwright/test";

function rand(): string {
  return Math.random().toString(36).slice(2, 8);
}
async function fill(page: Page, label: string, value: string) {
  await page.getByLabel(label, { exact: false }).fill(value);
}

test.describe("task detail layout", () => {
  test("subtasks render in the main column and carry their full title on hover", async ({ page }) => {
    const handle = `e2e${rand()}`;
    const key = `SL${rand().slice(0, 3).toUpperCase()}`;
    const longTitle =
      "a subtask title long enough that the sidebar used to swallow most of it in an ellipsis";

    await page.goto("/register");
    await fill(page, "Display name", "Layout");
    await fill(page, "Handle", handle);
    await fill(page, "Email", `${handle}@sprintly.test`);
    await fill(page, "Password", "correct-horse-battery-staple");
    await page.getByRole("button", { name: /\$ git init account/ }).click();
    await expect(page).toHaveURL(/\/(me\/day)?$/);

    await page.goto("/projects");
    await page.getByRole("button", { name: /new project/i }).first().click();
    const dialog = page.getByRole("dialog");
    await dialog.getByLabel("Name").fill("Layout");
    await dialog.getByLabel(/^Key/).fill(key);
    await dialog.getByRole("button", { name: /\$ git init project/ }).click();
    await expect(page).toHaveURL(new RegExp(`/projects/${key}$`));

    await page.goto(`/projects/${key}/backlog`);
    await page.locator("[data-backlog-quick-add]").click();
    const input = page.getByLabel("new task title");
    await input.fill("parent of things");
    await input.press("Enter");
    await expect(page.getByText("parent of things")).toBeVisible();

    await page.goto(`/tasks/${key}-1`);
    // Add a subtask through the panel.
    await page.getByRole("button", { name: /\+ add subtask|add subtask/i }).click();
    const subInput = page.getByPlaceholder(/subtask title/i);
    await subInput.fill(longTitle);
    await subInput.press("Enter");
    const row = page.locator("li", { hasText: `${key}-2` });
    await expect(row).toBeVisible();

    // Full title on hover — the whole point of moving it out of the sidebar
    // is that it *also* still says everything when it does truncate.
    await expect(row.locator(`[title="${longTitle}"]`)).toHaveCount(1);

    // Layout: the subtasks heading sits in the main column (left of the
    // sidebar), not inside the <aside>.
    const heading = page.getByRole("heading", { name: /subtasks/i });
    await expect(heading).toBeVisible();
    const inAside = await heading.evaluate((el) => !!el.closest("aside"));
    expect(inAside, "subtasks should have left the sidebar").toBe(false);
    const asideBox = await page.locator("aside").first().boundingBox();
    const headBox = await heading.boundingBox();
    expect(headBox && asideBox && headBox.x < asideBox.x, "main column is left of the sidebar").toBe(true);
  });
});
