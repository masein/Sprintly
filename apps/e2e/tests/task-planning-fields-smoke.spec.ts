// feat/task-planning-fields: story points, due date, and estimate get
// editors on the task detail. QA report 2: "Estimate and Due Date fields
// are missing." (The API supported them since day one — no UI set them.)
//
// Pre-reqs: dev stack up (`just up`), SPRINTLY_OPEN_SIGNUP=true.

import { test, expect, type Page } from "@playwright/test";

function rand(): string {
  return Math.random().toString(36).slice(2, 8);
}

async function fill(page: Page, label: string, value: string) {
  await page.getByLabel(label, { exact: false }).fill(value);
}

test.describe("task planning fields", () => {
  test("set points, due date, and estimate; they persist", async ({ page }) => {
    const handle = `e2e${rand()}`;
    const key = `PL${rand().slice(0, 3).toUpperCase()}`;

    await page.goto("/register");
    await fill(page, "Display name", "Planner");
    await fill(page, "Handle", handle);
    await fill(page, "Email", `${handle}@sprintly.test`);
    await fill(page, "Password", "correct-horse-battery-staple");
    await page.getByRole("button", { name: /\$ git init account/ }).click();
    await expect(page).toHaveURL(/\/(me\/day)?$/);

    await page.goto("/projects");
    await page.getByRole("button", { name: /new project/i }).first().click();
    const dialog = page.getByRole("dialog");
    await dialog.getByLabel("Name").fill("Planning");
    await dialog.getByLabel(/^Key/).fill(key);
    await dialog.getByRole("button", { name: /\$ git init project/ }).click();
    await expect(page).toHaveURL(new RegExp(`/projects/${key}$`));

    await page.goto(`/projects/${key}/backlog`);
    await page.locator("[data-backlog-quick-add]").click();
    const input = page.getByLabel("new task title");
    await input.fill("plan me");
    await input.press("Enter");
    await expect(page.getByText("plan me")).toBeVisible();

    await page.goto(`/tasks/${key}-1`);
    // Each editor fires its PATCH on blur/change — wait for the response so
    // the reload can't race an in-flight save.
    const patched = () =>
      page.waitForResponse(
        (r) => r.url().includes(`/tasks/${key}-1`) && r.request().method() === "PATCH" && r.ok(),
      );

    let wait = patched();
    await page.getByLabel("story points").fill("5");
    await page.getByLabel("story points").blur();
    await wait;

    wait = patched();
    await page.getByLabel("due date").fill("2026-08-15");
    await wait;

    wait = patched();
    await page.getByLabel("estimate hours").fill("2.5");
    await page.getByLabel("estimate hours").blur();
    await wait;

    // Persisted: a reload shows the same values.
    await page.reload();
    await expect(page.getByLabel("story points")).toHaveValue("5");
    await expect(page.getByLabel("due date")).toHaveValue("2026-08-15");
    await expect(page.getByLabel("estimate hours")).toHaveValue("2.5");

    // Clearing works too (explicit-null round-trip, not a COALESCE no-op).
    wait = patched();
    await page.getByLabel("story points").fill("");
    await page.getByLabel("story points").blur();
    await wait;
    await page.reload();
    await expect(page.getByLabel("story points")).toHaveValue("");
    await expect(page.getByLabel("due date")).toHaveValue("2026-08-15");
  });
});
