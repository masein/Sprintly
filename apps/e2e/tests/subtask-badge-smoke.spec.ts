// QA report 5: "Have an indicator on tasks showing whether they have subtasks
// and display the number as a badge (everywhere — backlog, board, sprint)."
//
// Pre-reqs: dev stack up (`just up`), SPRINTLY_OPEN_SIGNUP=true.

import { test, expect, type Page } from "@playwright/test";

function rand(): string {
  return Math.random().toString(36).slice(2, 8);
}
async function fill(page: Page, label: string, value: string) {
  await page.getByLabel(label, { exact: false }).fill(value);
}

test.describe("subtask badge", () => {
  test("a parent shows its live subtask count on the backlog, the board, and the sprint page", async ({ page }) => {
    const handle = `e2e${rand()}`;
    const key = `SB${rand().slice(0, 3).toUpperCase()}`;

    await page.goto("/register");
    await fill(page, "Display name", "Badger");
    await fill(page, "Handle", handle);
    await fill(page, "Email", `${handle}@sprintly.test`);
    await fill(page, "Password", "correct-horse-battery-staple");
    await page.getByRole("button", { name: /\$ git init account/ }).click();
    await expect(page).toHaveURL(/\/(me\/day)?$/);

    await page.goto("/projects");
    await page.getByRole("button", { name: /new project/i }).first().click();
    const dialog = page.getByRole("dialog");
    await dialog.getByLabel("Name").fill("Badges");
    await dialog.getByLabel(/^Key/).fill(key);
    await dialog.getByRole("button", { name: /\$ git init project/ }).click();
    await expect(page).toHaveURL(new RegExp(`/projects/${key}$`));

    await page.goto(`/projects/${key}/backlog`);
    await page.locator("[data-backlog-quick-add]").click();
    const input = page.getByLabel("new task title");
    await input.fill("the parent");
    await input.press("Enter");
    await expect(page.getByText("the parent")).toBeVisible();
    await input.fill("childless");
    await input.press("Enter");
    await expect(page.getByText("childless")).toBeVisible();

    // Two subtasks on KEY-1 via the task detail panel.
    await page.goto(`/tasks/${key}-1`);
    for (const t of ["first child", "second child"]) {
      await page.getByRole("button", { name: /add subtask/i }).click();
      const sub = page.getByPlaceholder("subtask title");
      await sub.fill(t);
      await sub.press("Enter");
      await expect(page.getByText(t)).toBeVisible();
    }

    // Backlog: badge on the parent, none on the childless one.
    await page.goto(`/projects/${key}/backlog`);
    const parentRow = page.locator("li", { hasText: `${key}-1` });
    await expect(parentRow.getByLabel("2 subtasks")).toBeVisible();
    await expect(page.locator("li", { hasText: "childless" }).locator("[data-subtask-badge]")).toHaveCount(0);

    // Board card.
    await page.goto(`/projects/${key}`);
    const card = page.locator(`[data-task-card="${key}-1"]`);
    await expect(card.getByLabel("2 subtasks")).toBeVisible();

    // Sprint page backlog panel (same count, different list).
    await page.goto(`/projects/${key}/sprints`);
    await page.getByRole("button", { name: /new sprint/i }).click();
    await page.getByPlaceholder(/Sprint 23/i).fill("Sprint 1");
    await page.getByRole("button", { name: /\$ git init sprint/ }).click();
    await expect(page).toHaveURL(/\/sprints\/[0-9a-f-]+$/);
    await expect(
      page.locator("li", { hasText: `${key}-1` }).getByLabel("2 subtasks").first(),
    ).toBeVisible();
  });
});
