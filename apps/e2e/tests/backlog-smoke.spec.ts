// F9 smoke: backlog multi-select + bulk assign.
//
// Register a fresh user → create a project (they become its lead) → add two
// cards on the board (both land in the backlog, no sprint) → open the backlog
// → select all → bulk "assign to me" → the rows show as assigned.
//
// Pre-reqs: dev stack up (`just up`), SPRINTLY_OPEN_SIGNUP=true.

import { test, expect, type Page } from "@playwright/test";

function rand(): string {
  return Math.random().toString(36).slice(2, 8);
}

test.describe("F9 backlog bulk smoke", () => {
  test("multi-select the backlog and bulk-assign", async ({ page }) => {
    const handle = `e2e${rand()}`;
    const key = `BK${rand().slice(0, 3).toUpperCase()}`;

    await test.step("register", async () => {
      await page.goto("/register");
      await fill(page, "Display name", "Backlog Tester");
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
      await dialog.getByLabel("Name").fill("Backlog");
      await dialog.getByLabel(/^Key/).fill(key);
      await dialog.getByRole("button", { name: /\$ git init project/ }).click();
      await expect(page).toHaveURL(new RegExp(`/projects/${key}$`));
    });

    await test.step("add two cards (they start in the backlog)", async () => {
      await page.locator("[data-add-card-button]").first().click();
      const titleInput = page.getByPlaceholder("card title");
      for (const title of ["triage me", "and me too"]) {
        await titleInput.fill(title);
        await page.getByRole("button", { name: /^add$/ }).click();
        await expect(page.getByText(title)).toBeVisible();
      }
    });

    await test.step("open the backlog and bulk-assign to me", async () => {
      await page.getByRole("link", { name: /backlog/i }).click();
      await expect(page).toHaveURL(new RegExp(`/projects/${key}/backlog$`));
      // Both cards are here (no sprint).
      await expect(page.getByText("triage me")).toBeVisible();
      await expect(page.getByText("and me too")).toBeVisible();
      // Select all, then assign to me.
      await page.getByRole("button", { name: /select all/i }).click();
      await expect(page.getByText("2 selected")).toBeVisible();
      await page.getByRole("button", { name: /assign to me/i }).click();
      // Both rows now show the "assigned" marker.
      await expect(page.getByText("assigned")).toHaveCount(2);
    });
  });
});

async function fill(page: Page, label: string, value: string) {
  await page.getByLabel(label, { exact: false }).fill(value);
}
