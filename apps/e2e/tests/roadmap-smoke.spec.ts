// F6 smoke: roadmap / timeline.
//
// Register a fresh user → create a project (they become its lead) → open the
// timeline → create an epic with a start + end date → assert an epic bar
// renders on the timeline.
//
// Pre-reqs: dev stack up (`just up`), SPRINTLY_OPEN_SIGNUP=true.

import { test, expect, type Page } from "@playwright/test";

function rand(): string {
  return Math.random().toString(36).slice(2, 8);
}

test.describe("F6 roadmap smoke", () => {
  test("create an epic with dates → it renders as a bar", async ({ page }) => {
    const handle = `e2e${rand()}`;
    const key = `RM${rand().slice(0, 3).toUpperCase()}`;
    const epicName = `epic-${rand()}`;

    await test.step("register", async () => {
      await page.goto("/register");
      await fill(page, "Display name", "Roadmap Tester");
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
      await dialog.getByLabel("Name").fill("Roadmap");
      await dialog.getByLabel(/^Key/).fill(key);
      await dialog.getByRole("button", { name: /\$ git init project/ }).click();
      await expect(page).toHaveURL(new RegExp(`/projects/${key}$`));
    });

    await test.step("open the timeline", async () => {
      await page.getByRole("link", { name: /timeline/i }).click();
      await expect(page).toHaveURL(new RegExp(`/projects/${key}/timeline$`));
      // Empty state before anything is scheduled.
      await expect(page.getByText(/Nothing scheduled yet/i)).toBeVisible();
    });

    await test.step("create an epic with dates → it renders as a bar", async () => {
      await page.getByLabel("epic name").fill(epicName);
      await page.getByLabel("epic start").fill("2026-07-01");
      await page.getByLabel("epic end").fill("2026-07-31");
      // The add button inside the epics manager (first of the two managers).
      await page.getByRole("button", { name: /^add$/ }).first().click();
      // The empty state is gone and a bar with the epic's name + progress shows.
      const bar = page.getByTestId("epic-bar").filter({ hasText: epicName });
      await expect(bar).toBeVisible();
      await expect(bar).toContainText("0/0");
    });
  });
});

async function fill(page: Page, label: string, value: string) {
  await page.getByLabel(label, { exact: false }).fill(value);
}
