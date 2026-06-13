// F8 smoke: saved board views + swimlanes.
//
// Register a fresh user → create a project (they become its lead) → add two
// cards → group the board into swimlanes by priority and assert a lane
// appears → save the grouping as a named view → reload → reselect the view
// and assert the grouping is restored.
//
// Pre-reqs: dev stack up (`just up`), SPRINTLY_OPEN_SIGNUP=true.

import { test, expect, type Page } from "@playwright/test";

function rand(): string {
  return Math.random().toString(36).slice(2, 8);
}

test.describe("F8 board views smoke", () => {
  test("group into swimlanes, save a view, restore it", async ({ page }) => {
    const handle = `e2e${rand()}`;
    const key = `BV${rand().slice(0, 3).toUpperCase()}`;
    const viewName = `focus-${rand()}`;

    await test.step("register", async () => {
      await page.goto("/register");
      await fill(page, "Display name", "Views Tester");
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
      await dialog.getByLabel("Name").fill("Board Views");
      // The key field auto-derives; override it with our unique key.
      await dialog.getByLabel(/^Key/).fill(key);
      await dialog.getByRole("button", { name: /\$ git init project/ }).click();
      await expect(page).toHaveURL(new RegExp(`/projects/${key}$`));
      // Default board with columns is present.
      await expect(page.getByLabel("swimlane grouping")).toBeVisible();
    });

    await test.step("add two cards", async () => {
      // Open the first column's add-card form once; it stays open after each
      // submit (title clears), so both cards land in the same column.
      await page.locator("[data-add-card-button]").first().click();
      const titleInput = page.getByPlaceholder("card title");
      for (const title of ["first card", "second card"]) {
        await titleInput.fill(title);
        await page.getByRole("button", { name: /^add$/ }).click();
        await expect(page.getByText(title)).toBeVisible();
      }
    });

    await test.step("swimlane by priority shows a lane", async () => {
      await page.getByLabel("swimlane grouping").selectOption("priority");
      // Both cards default to p2 → one lane labelled p2 with both cards.
      const lane = page.getByTestId("lane-header").filter({ hasText: "p2" });
      await expect(lane).toBeVisible();
      await expect(lane).toContainText("· 2");
    });

    await test.step("save the grouping as a view", async () => {
      await page.getByRole("button", { name: /save view/i }).click();
      await page.getByPlaceholder("view name").fill(viewName);
      await page.getByRole("button", { name: /^save$/ }).click();
      // The new view becomes the active selection.
      await expect(page.getByLabel("saved view")).toContainText(viewName);
    });

    await test.step("reload resets grouping; reselecting the view restores it", async () => {
      await page.reload();
      // Fresh load is ungrouped — no lane headers.
      await expect(page.getByTestId("lane-header")).toHaveCount(0);
      await page.getByLabel("saved view").selectOption({ label: viewName });
      const lane = page.getByTestId("lane-header").filter({ hasText: "p2" });
      await expect(lane).toBeVisible();
    });
  });
});

async function fill(page: Page, label: string, value: string) {
  await page.getByLabel(label, { exact: false }).fill(value);
}
