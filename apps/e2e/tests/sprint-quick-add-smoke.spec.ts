// QA F1 fix: create a task from inside a sprint by typing a new title + Enter.
//
// Register → create project → create a sprint (lands on its detail) → open the
// quick-add, type a brand-new title, press Enter → a card appears in the sprint
// and the count increments. (Previously: Enter was a silent no-op.)
//
// Pre-reqs: dev stack up (`just up`), SPRINTLY_OPEN_SIGNUP=true.

import { test, expect, type Page } from "@playwright/test";

function rand(): string {
  return Math.random().toString(36).slice(2, 8);
}

test.describe("QA F1 — sprint inline create", () => {
  test("type a new title in a sprint and press Enter to create+add", async ({ page }) => {
    const handle = `e2e${rand()}`;
    const key = `SQ${rand().slice(0, 3).toUpperCase()}`;

    await test.step("register", async () => {
      await page.goto("/register");
      await fill(page, "Display name", "Sprint Planner");
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
      await dialog.getByLabel("Name").fill("Sprint Quick");
      await dialog.getByLabel(/^Key/).fill(key);
      await dialog.getByRole("button", { name: /\$ git init project/ }).click();
      await expect(page).toHaveURL(new RegExp(`/projects/${key}$`));
    });

    await test.step("create a sprint (lands on the detail page)", async () => {
      await page.goto(`/projects/${key}/sprints`);
      await page.getByRole("button", { name: /new sprint/i }).click();
      await page.getByPlaceholder(/Sprint 23/i).fill("Sprint 1");
      await page.getByRole("button", { name: /\$ git init sprint/ }).click();
      await expect(page).toHaveURL(/\/sprints\/[0-9a-f-]+$/);
      await expect(page.getByText(/tasks \(0\)/)).toBeVisible();
    });

    await test.step("type a brand-new title and press Enter → it's created in the sprint", async () => {
      await page.getByRole("button", { name: /add tasks/i }).click();
      const input = page.getByLabel("add a task to this sprint");
      await input.fill("Write API tests");
      await input.press("Enter");

      // A card with the new title shows up in the sprint, and the count ticks up.
      await expect(page.getByText("Write API tests")).toBeVisible();
      await expect(page.getByText(/tasks \(1\)/)).toBeVisible();
      // The field cleared and kept focus for the next one (rapid entry).
      await expect(input).toHaveValue("");
      await expect(input).toBeFocused();
    });
  });
});

async function fill(page: Page, label: string, value: string) {
  await page.getByLabel(label, { exact: false }).fill(value);
}
