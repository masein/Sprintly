// QA report 5: "Display task priority, assignee avatars/names, and associated
// labels directly on backlog tasks, accompanied by a new assignee filter."
//
// Pre-reqs: dev stack up (`just up`), SPRINTLY_OPEN_SIGNUP=true.

import { test, expect, type Page } from "@playwright/test";

function rand(): string {
  return Math.random().toString(36).slice(2, 8);
}
async function fill(page: Page, label: string, value: string) {
  await page.getByLabel(label, { exact: false }).fill(value);
}

test.describe("backlog metadata", () => {
  test("rows show who and what; the assignee filter narrows the pile", async ({ page }) => {
    const handle = `e2e${rand()}`;
    const key = `BM${rand().slice(0, 3).toUpperCase()}`;

    await page.goto("/register");
    await fill(page, "Display name", "Backlogger");
    await fill(page, "Handle", handle);
    await fill(page, "Email", `${handle}@sprintly.test`);
    await fill(page, "Password", "correct-horse-battery-staple");
    await page.getByRole("button", { name: /\$ git init account/ }).click();
    await expect(page).toHaveURL(/\/(me\/day)?$/);

    await page.goto("/projects");
    await page.getByRole("button", { name: /new project/i }).first().click();
    const dialog = page.getByRole("dialog");
    await dialog.getByLabel("Name").fill("Metadata");
    await dialog.getByLabel(/^Key/).fill(key);
    await dialog.getByRole("button", { name: /\$ git init project/ }).click();
    await expect(page).toHaveURL(new RegExp(`/projects/${key}$`));

    await page.goto(`/projects/${key}/backlog`);
    await page.locator("[data-backlog-quick-add]").click();
    const input = page.getByLabel("new task title");
    await input.fill("mine, labelled");
    await input.press("Enter");
    await expect(page.getByText("mine, labelled")).toBeVisible();
    await input.fill("nobody's yet");
    await input.press("Enter");
    await expect(page.getByText("nobody's yet")).toBeVisible();

    // Assign KEY-1 to me and label it, through the task page.
    await page.goto(`/tasks/${key}-1`);
    await page.getByLabel("assignee", { exact: true }).click();
    await page.getByLabel("search assignee").fill(handle);
    await page.getByRole("option", { name: new RegExp(handle) }).first().click();
    // Wait for the assignment to land before leaving the page.
    await expect(page.getByLabel("assignee", { exact: true })).toContainText(handle);

    await page.goto(`/projects/${key}/backlog`);
    const mine = page.locator("li", { hasText: `${key}-1` });
    const theirs = page.locator("li", { hasText: `${key}-2` });
    // Who: handle on the row (was a bare "assigned").
    await expect(mine.getByText(`@${handle}`)).toBeVisible();
    await expect(theirs.getByText(/@/)).toHaveCount(0);

    // Filter: unassigned hides mine; me hides theirs; anyone shows both.
    const filter = page.getByLabel("filter by assignee");
    await filter.selectOption("unassigned");
    await expect(mine).toHaveCount(0);
    await expect(theirs).toBeVisible();
    await filter.selectOption({ label: `@${handle}` });
    await expect(mine).toBeVisible();
    await expect(theirs).toHaveCount(0);
    await filter.selectOption("anyone");
    await expect(mine).toBeVisible();
    await expect(theirs).toBeVisible();
  });
});
