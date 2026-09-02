// QA report 5: completed sprints preserve their tasks "in their exact state
// and time-logged metrics during that active cycle" — the Jira model — even
// when leftovers are carried into the next sprint.
//
// Pre-reqs: dev stack up (`just up`), SPRINTLY_OPEN_SIGNUP=true.

import { test, expect, type Page } from "@playwright/test";

function rand(): string {
  return Math.random().toString(36).slice(2, 8);
}
async function fill(page: Page, label: string, value: string) {
  await page.getByLabel(label, { exact: false }).fill(value);
}

test.describe("sprint history", () => {
  test("a completed sprint still shows the task it carried forward, as it stood", async ({ page }) => {
    const handle = `e2e${rand()}`;
    const key = `SS${rand().slice(0, 3).toUpperCase()}`;

    await page.goto("/register");
    await fill(page, "Display name", "Historian");
    await fill(page, "Handle", handle);
    await fill(page, "Email", `${handle}@sprintly.test`);
    await fill(page, "Password", "correct-horse-battery-staple");
    await page.getByRole("button", { name: /\$ git init account/ }).click();
    await expect(page).toHaveURL(/\/(me\/day)?$/);

    await page.goto("/projects");
    await page.getByRole("button", { name: /new project/i }).first().click();
    const dialog = page.getByRole("dialog");
    await dialog.getByLabel("Name").fill("History");
    await dialog.getByLabel(/^Key/).fill(key);
    await dialog.getByRole("button", { name: /\$ git init project/ }).click();
    await expect(page).toHaveURL(new RegExp(`/projects/${key}$`));

    await page.goto(`/projects/${key}/sprints`);
    await page.getByRole("button", { name: /new sprint/i }).click();
    await page.getByPlaceholder(/Sprint 23/i).fill("Sprint 7");
    await page.getByRole("button", { name: /\$ git init sprint/ }).click();
    await expect(page).toHaveURL(/\/sprints\/[0-9a-f-]+$/);
    const sprintUrl = page.url();

    // One task, into the sprint, sprint started.
    await page.getByRole("button", { name: /add tasks/i }).click();
    const add = page.getByLabel("add a task to this sprint");
    await add.fill("unfinished business");
    await add.press("Enter");
    await expect(page.getByText("unfinished business")).toBeVisible();
    await page.getByRole("button", { name: /start sprint/ }).click();
    await expect(page.getByText("active", { exact: true })).toBeVisible();

    // Complete it, carrying the leftover into a brand-new sprint.
    await page.getByRole("button", { name: /complete \+ open retro/ }).click();
    const modal = page.getByTestId("complete-sprint-modal");
    await modal.getByRole("radio", { name: /move to a new sprint/ }).check();
    await modal.getByRole("button", { name: /complete sprint/ }).click();
    await expect(page).toHaveURL(/\/retro$/, { timeout: 15_000 });

    // The completed sprint still lists the task — flagged as history — and in
    // the state it had at completion (todo), even though it now lives in
    // Sprint 8.
    await page.goto(sprintUrl);
    await expect(page.getByTestId("sprint-snapshot-note")).toBeVisible();
    const row = page.locator("li", { hasText: `${key}-1` });
    await expect(row).toBeVisible();
    await expect(row.getByText("todo", { exact: true })).toBeVisible();

    // And the new sprint has it live.
    await page.goto(`/projects/${key}/sprints`);
    await page.getByRole("link", { name: /Sprint 8/ }).first().click();
    await expect(page.locator("li", { hasText: `${key}-1` })).toBeVisible();
    await expect(page.getByTestId("sprint-snapshot-note")).toHaveCount(0);
  });
});
