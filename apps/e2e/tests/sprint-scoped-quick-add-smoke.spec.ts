// fix/sprint-scoped-quick-add: when the board is scoped to a sprint, a column
// quick-add must inherit that sprint so the new card stays in the filtered
// view (it used to be created sprint-less and vanish immediately). Under "all
// tasks" scope the card stays sprint-less, as before.
//
// Pre-reqs: dev stack up (`just up`), SPRINTLY_OPEN_SIGNUP=true.

import { test, expect, type Page } from "@playwright/test";

function rand(): string {
  return Math.random().toString(36).slice(2, 8);
}

async function fill(page: Page, label: string, value: string) {
  await page.getByLabel(label, { exact: false }).fill(value);
}

async function quickAdd(page: Page, title: string) {
  await page.locator("[data-add-card-button]").first().click();
  await page.getByPlaceholder("card title").fill(title);
  await page.getByRole("button", { name: /^add$/ }).click();
}

test.describe("sprint-scoped quick-add", () => {
  test("a card quick-added under an active-sprint scope joins the sprint and stays visible", async ({ page }) => {
    const handle = `e2e${rand()}`;
    const key = `SQ${rand().slice(0, 3).toUpperCase()}`;

    await test.step("register + project", async () => {
      await page.goto("/register");
      await fill(page, "Display name", "Sprint Scoped Tester");
      await fill(page, "Handle", handle);
      await fill(page, "Email", `${handle}@sprintly.test`);
      await fill(page, "Password", "correct-horse-battery-staple");
      await page.getByRole("button", { name: /\$ git init account/ }).click();
      await expect(page).toHaveURL(/\/(me\/day)?$/);

      await page.goto("/projects");
      await page.getByRole("button", { name: /new project/i }).first().click();
      const dialog = page.getByRole("dialog");
      await dialog.getByLabel("Name").fill("Sprint Scoped");
      await dialog.getByLabel(/^Key/).fill(key);
      await dialog.getByRole("button", { name: /\$ git init project/ }).click();
      await expect(page).toHaveURL(new RegExp(`/projects/${key}$`));
    });

    await test.step("create a sprint with one card, then start it", async () => {
      await page.goto(`/projects/${key}/sprints`);
      await page.getByRole("button", { name: /new sprint/i }).click();
      await page.getByPlaceholder(/Sprint 23/i).fill("Sprint 1");
      await page.getByRole("button", { name: /\$ git init sprint/ }).click();
      await expect(page).toHaveURL(/\/sprints\/[0-9a-f-]+$/);
      await page.getByRole("button", { name: /add tasks/i }).click();
      const adder = page.getByLabel("add a task to this sprint");
      await adder.fill("Committed work");
      await adder.press("Enter");
      await expect(page.getByText("Committed work")).toBeVisible();
      await page.getByRole("button", { name: /start sprint/i }).click();
      await expect(page.getByRole("button", { name: /start sprint/i })).toHaveCount(0);
    });

    await test.step("board is scoped to the active sprint; quick-add a card", async () => {
      await page.goto(`/projects/${key}`);
      await expect(page.getByLabel("board scope")).toHaveValue("active");
      await quickAdd(page, "Added while scoped");
      // The fix: it stays visible under the active-sprint scope instead of
      // vanishing (before, it was created sprint-less and fell out of scope).
      await expect(page.getByText("Added while scoped")).toBeVisible();
    });

    await test.step("it survives a reload under the active scope", async () => {
      await page.reload();
      await expect(page.getByLabel("board scope")).toHaveValue("active");
      await expect(page.getByText("Added while scoped")).toBeVisible();
    });

    await test.step("it's really in the sprint — not in the backlog", async () => {
      await page.goto(`/projects/${key}/backlog`);
      await expect(page.getByText("Added while scoped")).toHaveCount(0);
    });

    await test.step("under all-tasks scope, quick-add stays sprint-less (backlog)", async () => {
      await page.goto(`/projects/${key}`);
      await page.getByLabel("board scope").selectOption("all");
      await quickAdd(page, "Loose backlog card");
      await expect(page.getByText("Loose backlog card")).toBeVisible();
      // No sprint → it shows up in the backlog.
      await page.goto(`/projects/${key}/backlog`);
      await expect(page.getByText("Loose backlog card")).toBeVisible();
    });
  });
});
