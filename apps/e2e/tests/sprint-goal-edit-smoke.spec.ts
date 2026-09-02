// QA report 5: "Enable editing sprint goals for created one."
//
// Pre-reqs: dev stack up (`just up`), SPRINTLY_OPEN_SIGNUP=true.

import { test, expect, type Page } from "@playwright/test";

function rand(): string {
  return Math.random().toString(36).slice(2, 8);
}
async function fill(page: Page, label: string, value: string) {
  await page.getByLabel(label, { exact: false }).fill(value);
}

test.describe("sprint goal", () => {
  test("a lead rewrites the goal inline and it sticks", async ({ page }) => {
    const handle = `e2e${rand()}`;
    const key = `SG${rand().slice(0, 3).toUpperCase()}`;

    await page.goto("/register");
    await fill(page, "Display name", "Goalie");
    await fill(page, "Handle", handle);
    await fill(page, "Email", `${handle}@sprintly.test`);
    await fill(page, "Password", "correct-horse-battery-staple");
    await page.getByRole("button", { name: /\$ git init account/ }).click();
    await expect(page).toHaveURL(/\/(me\/day)?$/);

    await page.goto("/projects");
    await page.getByRole("button", { name: /new project/i }).first().click();
    const dialog = page.getByRole("dialog");
    await dialog.getByLabel("Name").fill("Goals");
    await dialog.getByLabel(/^Key/).fill(key);
    await dialog.getByRole("button", { name: /\$ git init project/ }).click();
    await expect(page).toHaveURL(new RegExp(`/projects/${key}$`));

    await page.goto(`/projects/${key}/sprints`);
    await page.getByRole("button", { name: /new sprint/i }).click();
    await page.getByPlaceholder(/Sprint 23/i).fill("Sprint 1");
    await page.getByRole("button", { name: /\$ git init sprint/ }).click();
    await expect(page).toHaveURL(/\/sprints\/[0-9a-f-]+$/);

    await page.goto(`/projects/${key}/sprints`);
    await page.getByRole("button", { name: "edit Sprint 1" }).click();
    await page.getByLabel("Sprint 1 goal").fill("ship the login page, no regressions");
    await page.getByRole("button", { name: "save", exact: true }).click();
    await expect(page.getByText("ship the login page, no regressions")).toBeVisible();

    // Persisted, not just rendered.
    await page.reload();
    await expect(page.getByText("ship the login page, no regressions")).toBeVisible();

    // And it can be rewritten again — this isn't a one-shot field.
    await page.getByRole("button", { name: "edit Sprint 1" }).click();
    await page.getByLabel("Sprint 1 goal").fill("actually: fix the three flaky specs");
    await page.getByRole("button", { name: "save", exact: true }).click();
    await expect(page.getByText("actually: fix the three flaky specs")).toBeVisible();
    await expect(page.getByText("ship the login page, no regressions")).toHaveCount(0);
  });
});
