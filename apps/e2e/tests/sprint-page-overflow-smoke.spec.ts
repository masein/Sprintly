// fix/sprint-page-overflow: one very long task title used to widen the
// sprint page's task column and shove the backlog/burndown sidebar off the
// viewport. QA report 2: "the second column appears to be partially out of
// the viewport when the task title is very long."
//
// Pre-reqs: dev stack up (`just up`), SPRINTLY_OPEN_SIGNUP=true.

import { test, expect, type Page } from "@playwright/test";

function rand(): string {
  return Math.random().toString(36).slice(2, 8);
}

async function fill(page: Page, label: string, value: string) {
  await page.getByLabel(label, { exact: false }).fill(value);
}

const LONG_TITLE =
  "show the backlog list in the sprint view or show the sprint tasks in backlog view because it is much easier to move tasks between them that way";

test.describe("sprint page overflow", () => {
  test("a very long task title doesn't push the sidebar off-viewport", async ({ page }) => {
    const handle = `e2e${rand()}`;
    const key = `OV${rand().slice(0, 3).toUpperCase()}`;

    await page.goto("/register");
    await fill(page, "Display name", "Overflow Tester");
    await fill(page, "Handle", handle);
    await fill(page, "Email", `${handle}@sprintly.test`);
    await fill(page, "Password", "correct-horse-battery-staple");
    await page.getByRole("button", { name: /\$ git init account/ }).click();
    await expect(page).toHaveURL(/\/(me\/day)?$/);

    await page.goto("/projects");
    await page.getByRole("button", { name: /new project/i }).first().click();
    const dialog = page.getByRole("dialog");
    await dialog.getByLabel("Name").fill("Overflow");
    await dialog.getByLabel(/^Key/).fill(key);
    await dialog.getByRole("button", { name: /\$ git init project/ }).click();
    await expect(page).toHaveURL(new RegExp(`/projects/${key}$`));

    await page.goto(`/projects/${key}/sprints`);
    await page.getByRole("button", { name: /new sprint/i }).click();
    await page.getByPlaceholder(/Sprint 23/i).fill("Sprint 1");
    await page.getByRole("button", { name: /\$ git init sprint/ }).click();
    await expect(page).toHaveURL(/\/sprints\/[0-9a-f-]+$/);

    await page.getByRole("button", { name: /add tasks/i }).click();
    const adder = page.getByLabel("add a task to this sprint");
    await adder.fill(LONG_TITLE);
    await adder.press("Enter");
    await expect(page.getByText(LONG_TITLE.slice(0, 40) + "", { exact: false }).first()).toBeVisible();

    // No page-level horizontal scroll, and the sidebar stays in view.
    const overflow = await page.evaluate(
      () => document.documentElement.scrollWidth - document.documentElement.clientWidth,
    );
    expect(overflow).toBeLessThanOrEqual(0);
    const panel = page.getByTestId("backlog-drop");
    const box = await panel.boundingBox();
    const viewport = page.viewportSize()!;
    expect(box).not.toBeNull();
    expect(box!.x + box!.width).toBeLessThanOrEqual(viewport.width + 1);
  });
});
