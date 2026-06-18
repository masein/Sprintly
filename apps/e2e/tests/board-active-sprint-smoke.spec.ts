// QA F10 fix: the board reflects the active sprint.
//
// Create one backlog card and one card inside a sprint, start the sprint, then
// assert the board defaults to the active-sprint scope (showing only the sprint
// card) and that switching to "all tasks" brings the backlog card back.
//
// Pre-reqs: dev stack up (`just up`), SPRINTLY_OPEN_SIGNUP=true.

import { test, expect, type Page } from "@playwright/test";

function rand(): string {
  return Math.random().toString(36).slice(2, 8);
}

test.describe("QA F10 — board active-sprint scope", () => {
  test("board defaults to the active sprint and can switch to all tasks", async ({ page }) => {
    const handle = `e2e${rand()}`;
    const key = `SC${rand().slice(0, 3).toUpperCase()}`;

    await test.step("register", async () => {
      await page.goto("/register");
      await fill(page, "Display name", "Scope Tester");
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
      await dialog.getByLabel("Name").fill("Scoping");
      await dialog.getByLabel(/^Key/).fill(key);
      await dialog.getByRole("button", { name: /\$ git init project/ }).click();
      await expect(page).toHaveURL(new RegExp(`/projects/${key}$`));
    });

    await test.step("add a backlog card (not in any sprint)", async () => {
      await page.locator("[data-add-card-button]").first().click();
      await page.getByPlaceholder("card title").fill("Backlog work");
      await page.getByRole("button", { name: /^add$/ }).click();
      await expect(page.getByText("Backlog work")).toBeVisible();
    });

    await test.step("create a sprint with one card, then start it", async () => {
      await page.goto(`/projects/${key}/sprints`);
      await page.getByRole("button", { name: /new sprint/i }).click();
      await page.getByPlaceholder(/sprint name/i).fill("Sprint 1");
      await page.getByRole("button", { name: /\$ git init sprint/ }).click();
      await expect(page).toHaveURL(/\/sprints\/[0-9a-f-]+$/);

      // Quick-add a brand-new task straight into the sprint.
      await page.getByRole("button", { name: /add tasks/i }).click();
      const adder = page.getByLabel("add a task to this sprint");
      await adder.fill("Sprint work");
      await adder.press("Enter");
      await expect(page.getByText("Sprint work")).toBeVisible();

      await page.getByRole("button", { name: /start sprint/i }).click();
      // The start action flips state; the start button goes away.
      await expect(page.getByRole("button", { name: /start sprint/i })).toHaveCount(0);
    });

    await test.step("the board defaults to the active sprint", async () => {
      await page.goto(`/projects/${key}`);
      const scope = page.getByLabel("board scope");
      await expect(scope).toHaveValue("active");
      // Only the sprint card shows; the backlog card is out of scope.
      await expect(page.getByText("Sprint work")).toBeVisible();
      await expect(page.getByText("Backlog work")).toHaveCount(0);
    });

    await test.step("switching to all tasks brings the backlog card back", async () => {
      await page.getByLabel("board scope").selectOption("all");
      await expect(page.getByText("Sprint work")).toBeVisible();
      await expect(page.getByText("Backlog work")).toBeVisible();
    });
  });
});

async function fill(page: Page, label: string, value: string) {
  await page.getByLabel(label, { exact: false }).fill(value);
}
