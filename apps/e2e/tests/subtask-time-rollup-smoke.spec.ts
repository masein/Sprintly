// feat/subtask-time-rollup: a parent task's tracked total includes time
// logged on its direct subtasks. Log 1h on the parent + 30m on a subtask →
// the parent's timer panel reads "tracked 1h 30m · 30m in subtasks".
//
// Pre-reqs: dev stack up (`just up`), SPRINTLY_OPEN_SIGNUP=true.

import { test, expect, type Page } from "@playwright/test";

function rand(): string {
  return Math.random().toString(36).slice(2, 8);
}

async function fill(page: Page, label: string, value: string) {
  await page.getByLabel(label, { exact: false }).fill(value);
}

async function logManual(page: Page, hours: string, minutes: string) {
  await page.getByRole("button", { name: /manual entry/i }).click();
  // Scope to the timer card — the details panel's "estimate hours" input
  // would otherwise collide with the bare "hours" label.
  const timer = page.locator("section", { hasText: "manual entry" });
  await timer.getByLabel("hours", { exact: true }).fill(hours);
  await timer.getByLabel("minutes").fill(minutes);
  await timer.getByRole("button", { name: "add log" }).click();
}

test.describe("subtask time rollup", () => {
  test("parent tracked total includes subtask logs", async ({ page }) => {
    const handle = `e2e${rand()}`;
    const key = `TR${rand().slice(0, 3).toUpperCase()}`;

    await test.step("register + project + a task with a subtask", async () => {
      await page.goto("/register");
      await fill(page, "Display name", "Rollup Tester");
      await fill(page, "Handle", handle);
      await fill(page, "Email", `${handle}@sprintly.test`);
      await fill(page, "Password", "correct-horse-battery-staple");
      await page.getByRole("button", { name: /\$ git init account/ }).click();
      await expect(page).toHaveURL(/\/(me\/day)?$/);

      await page.goto("/projects");
      await page.getByRole("button", { name: /new project/i }).first().click();
      const dialog = page.getByRole("dialog");
      await dialog.getByLabel("Name").fill("Rollup");
      await dialog.getByLabel(/^Key/).fill(key);
      await dialog.getByRole("button", { name: /\$ git init project/ }).click();
      await expect(page).toHaveURL(new RegExp(`/projects/${key}$`));

      await page.goto(`/projects/${key}/backlog`);
      await page.locator("[data-backlog-quick-add]").click();
      const input = page.getByLabel("new task title");
      await input.fill("the parent");
      await input.press("Enter");
      await expect(page.getByText("the parent")).toBeVisible();

      await page.goto(`/tasks/${key}-1`);
      await page.getByRole("button", { name: /add subtask/i }).click();
      await page.getByPlaceholder("subtask title").fill("the child");
      await page.getByPlaceholder("subtask title").press("Enter");
      await expect(page.getByRole("link", { name: `${key}-2` })).toBeVisible();
    });

    await test.step("1h on the parent → tracked 1h, no subtask note", async () => {
      await logManual(page, "1", "0");
      const tracked = page.getByTestId("tracked-total");
      await expect(tracked).toContainText("tracked");
      await expect(tracked).toContainText("1h");
      await expect(tracked).not.toContainText("subtasks");
    });

    await test.step("30m on the subtask → parent total rolls it up", async () => {
      await page.goto(`/tasks/${key}-2`);
      await logManual(page, "0", "30");
      await expect(page.getByTestId("tracked-total")).toContainText("30m");

      await page.goto(`/tasks/${key}-1`);
      const tracked = page.getByTestId("tracked-total");
      await expect(tracked).toContainText("1h 30m");
      await expect(tracked).toContainText("30m in subtasks");
    });
  });
});
