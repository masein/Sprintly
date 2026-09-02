// QA report 5: board filters survive a refresh, there's a "clear filters"
// reset to the current sprint, and the assignee filter offers every member,
// not just "me".
//
// Pre-reqs: dev stack up (`just up`), SPRINTLY_OPEN_SIGNUP=true.

import { test, expect, type Page } from "@playwright/test";

function rand(): string {
  return Math.random().toString(36).slice(2, 8);
}
async function fill(page: Page, label: string, value: string) {
  await page.getByLabel(label, { exact: false }).fill(value);
}

test.describe("board filters", () => {
  test("persist across refresh, clear in one click, and know every member", async ({ page }) => {
    const handle = `e2e${rand()}`;
    const key = `BF${rand().slice(0, 3).toUpperCase()}`;

    await page.goto("/register");
    await fill(page, "Display name", "Filterer");
    await fill(page, "Handle", handle);
    await fill(page, "Email", `${handle}@sprintly.test`);
    await fill(page, "Password", "correct-horse-battery-staple");
    await page.getByRole("button", { name: /\$ git init account/ }).click();
    await expect(page).toHaveURL(/\/(me\/day)?$/);

    await page.goto("/projects");
    await page.getByRole("button", { name: /new project/i }).first().click();
    const dialog = page.getByRole("dialog");
    await dialog.getByLabel("Name").fill("Filters");
    await dialog.getByLabel(/^Key/).fill(key);
    await dialog.getByRole("button", { name: /\$ git init project/ }).click();
    await expect(page).toHaveURL(new RegExp(`/projects/${key}$`));

    // Two cards: one p0, one p3.
    await page.goto(`/projects/${key}/backlog`);
    await page.locator("[data-backlog-quick-add]").click();
    const input = page.getByLabel("new task title");
    await input.fill("urgent thing");
    await input.press("Enter");
    await expect(page.getByText("urgent thing")).toBeVisible();
    await input.fill("someday thing");
    await input.press("Enter");
    await expect(page.getByText("someday thing")).toBeVisible();
    await page.goto(`/tasks/${key}-1`);
    await page.getByLabel("priority", { exact: true }).selectOption("p0");
    await page.goto(`/tasks/${key}-2`);
    await page.getByLabel("priority", { exact: true }).selectOption("p3");

    // Apply priority:p0 on the board.
    await page.goto(`/projects/${key}`);
    await expect(page.locator(`[data-task-card="${key}-1"]`)).toBeVisible();
    await expect(page.locator(`[data-task-card="${key}-2"]`)).toBeVisible();
    await page.getByRole("button", { name: /^filter$/ }).click();
    await page.getByRole("button", { name: "priority", exact: true }).click();
    await page.getByRole("button", { name: "p0", exact: true }).click();
    await expect(page.locator(`[data-task-card="${key}-1"]`)).toBeVisible();
    await expect(page.locator(`[data-task-card="${key}-2"]`)).toHaveCount(0);
    await expect(page).toHaveURL(/[?&]f=priority(%3A|:)p0/);

    // Refresh: the filter is still applied. This is the reported bug.
    await page.reload();
    await expect(page.locator(`[data-task-card="${key}-1"]`)).toBeVisible();
    await expect(page.locator(`[data-task-card="${key}-2"]`)).toHaveCount(0);
    await expect(page.getByText("priority:")).toBeVisible();

    // Assignee filter offers the member by handle, not just "me".
    await page.getByRole("button", { name: /^filter$/ }).click();
    await page.getByRole("button", { name: "assignee", exact: true }).click();
    await expect(page.getByRole("button", { name: "me", exact: true })).toBeVisible();
    await expect(page.getByRole("button", { name: `@${handle}`, exact: true })).toBeVisible();
    await page.getByRole("button", { name: "Cancel" }).click();

    // One click clears everything and both cards return; URL is clean again.
    await page.getByRole("button", { name: /clear filters/ }).click();
    await expect(page.locator(`[data-task-card="${key}-2"]`)).toBeVisible();
    await expect(page).not.toHaveURL(/[?&]f=/);
    await expect(page.getByRole("button", { name: /clear filters/ })).toHaveCount(0);
  });
});
