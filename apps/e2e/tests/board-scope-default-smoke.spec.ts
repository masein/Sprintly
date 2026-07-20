// fix/board-scope-default: with an active sprint, the board opens scoped to it
// on EVERY fresh open. Switching to "all tasks" is a session-only move — a
// reload snaps back to the active sprint (it used to be persisted, so a fresh
// open wrongly stayed on "all tasks").
//
// Pre-reqs: dev stack up (`just up`), SPRINTLY_OPEN_SIGNUP=true.

import { test, expect, type Page } from "@playwright/test";

function rand(): string {
  return Math.random().toString(36).slice(2, 8);
}

async function fill(page: Page, label: string, value: string) {
  await page.getByLabel(label, { exact: false }).fill(value);
}

test.describe("board scope default", () => {
  test("active sprint wins on a fresh open; switching to all tasks doesn't stick across a reload", async ({ page }) => {
    const handle = `e2e${rand()}`;
    const key = `SD${rand().slice(0, 3).toUpperCase()}`;

    await test.step("register + project", async () => {
      await page.goto("/register");
      await fill(page, "Display name", "Scope Default Tester");
      await fill(page, "Handle", handle);
      await fill(page, "Email", `${handle}@sprintly.test`);
      await fill(page, "Password", "correct-horse-battery-staple");
      await page.getByRole("button", { name: /\$ git init account/ }).click();
      await expect(page).toHaveURL("/");

      await page.goto("/projects");
      await page.getByRole("button", { name: /new project/i }).first().click();
      const dialog = page.getByRole("dialog");
      await dialog.getByLabel("Name").fill("Scope Default");
      await dialog.getByLabel(/^Key/).fill(key);
      await dialog.getByRole("button", { name: /\$ git init project/ }).click();
      await expect(page).toHaveURL(new RegExp(`/projects/${key}$`));
    });

    await test.step("one backlog card, one sprint card, then start the sprint", async () => {
      await page.locator("[data-add-card-button]").first().click();
      await page.getByPlaceholder("card title").fill("Backlog work");
      await page.getByRole("button", { name: /^add$/ }).click();
      await expect(page.getByText("Backlog work")).toBeVisible();

      await page.goto(`/projects/${key}/sprints`);
      await page.getByRole("button", { name: /new sprint/i }).click();
      await page.getByPlaceholder(/Sprint 23/i).fill("Sprint 1");
      await page.getByRole("button", { name: /\$ git init sprint/ }).click();
      await expect(page).toHaveURL(/\/sprints\/[0-9a-f-]+$/);

      await page.getByRole("button", { name: /add tasks/i }).click();
      const adder = page.getByLabel("add a task to this sprint");
      await adder.fill("Sprint work");
      await adder.press("Enter");
      await expect(page.getByText("Sprint work")).toBeVisible();

      await page.getByRole("button", { name: /start sprint/i }).click();
      await expect(page.getByRole("button", { name: /start sprint/i })).toHaveCount(0);
    });

    await test.step("fresh open defaults to the active sprint", async () => {
      await page.goto(`/projects/${key}`);
      await expect(page.getByLabel("board scope")).toHaveValue("active");
      await expect(page.getByText("Sprint work")).toBeVisible();
      await expect(page.getByText("Backlog work")).toHaveCount(0);
    });

    await test.step("switch to all tasks for the session", async () => {
      await page.getByLabel("board scope").selectOption("all");
      await expect(page.getByLabel("board scope")).toHaveValue("all");
      await expect(page.getByText("Backlog work")).toBeVisible();
    });

    await test.step("reload → back to the active sprint (the fix)", async () => {
      await page.reload();
      await expect(page.getByLabel("board scope")).toHaveValue("active");
      await expect(page.getByText("Sprint work")).toBeVisible();
      await expect(page.getByText("Backlog work")).toHaveCount(0);
    });

    await test.step("re-navigating away and back also returns to active", async () => {
      await page.getByLabel("board scope").selectOption("all");
      await expect(page.getByLabel("board scope")).toHaveValue("all");
      await page.goto(`/projects/${key}/backlog`);
      await page.goto(`/projects/${key}`);
      await expect(page.getByLabel("board scope")).toHaveValue("active");
    });
  });
});
