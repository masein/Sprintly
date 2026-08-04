// feat/sprint-board-nav: the sprint page links straight back to the board,
// matching the backlog page. QA report: "Add a '← Board' button in the
// Sprint section, similar to the Backlog section."
//
// feat/breadcrumbs-nav moved that job into the breadcrumb trail — the project
// key crumb is the link now, and the duplicate right-aligned `← board` is
// gone. Same capability, one affordance instead of two.
//
// Pre-reqs: dev stack up (`just up`), SPRINTLY_OPEN_SIGNUP=true.

import { test, expect, type Page } from "@playwright/test";

function rand(): string {
  return Math.random().toString(36).slice(2, 8);
}

async function fill(page: Page, label: string, value: string) {
  await page.getByLabel(label, { exact: false }).fill(value);
}

test.describe("sprint → board navigation", () => {
  test("the sprint page links back to the board", async ({ page }) => {
    const handle = `e2e${rand()}`;
    const key = `SB${rand().slice(0, 3).toUpperCase()}`;

    await page.goto("/register");
    await fill(page, "Display name", "Nav Tester");
    await fill(page, "Handle", handle);
    await fill(page, "Email", `${handle}@sprintly.test`);
    await fill(page, "Password", "correct-horse-battery-staple");
    await page.getByRole("button", { name: /\$ git init account/ }).click();
    await expect(page).toHaveURL(/\/(me\/day)?$/);

    await page.goto("/projects");
    await page.getByRole("button", { name: /new project/i }).first().click();
    const dialog = page.getByRole("dialog");
    await dialog.getByLabel("Name").fill("Sprint Nav");
    await dialog.getByLabel(/^Key/).fill(key);
    await dialog.getByRole("button", { name: /\$ git init project/ }).click();
    await expect(page).toHaveURL(new RegExp(`/projects/${key}$`));

    await page.goto(`/projects/${key}/sprints`);
    await page.getByRole("button", { name: /new sprint/i }).click();
    await page.getByPlaceholder(/Sprint 23/i).fill("Sprint 1");
    await page.getByRole("button", { name: /\$ git init sprint/ }).click();
    await expect(page).toHaveURL(/\/sprints\/[0-9a-f-]+$/);

    const crumbs = page.getByRole("navigation", { name: "breadcrumb" });
    const boardLink = crumbs.getByRole("link", { name: key, exact: true });
    await expect(boardLink).toBeVisible();
    await boardLink.click();
    await expect(page).toHaveURL(new RegExp(`/projects/${key}$`));
  });
});
