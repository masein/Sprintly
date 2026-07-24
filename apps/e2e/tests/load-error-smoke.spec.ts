// fix/honest-load-errors: when a page's data fetch fails (500, network), the
// page says so and offers a retry — instead of an eternal loading placeholder
// or a misleading empty state. Forced via request interception on the
// dashboard endpoint, then lifted to prove "$ retry" actually recovers.
//
// Pre-reqs: dev stack up (`just up`), SPRINTLY_OPEN_SIGNUP=true.

import { test, expect, type Page } from "@playwright/test";

function rand(): string {
  return Math.random().toString(36).slice(2, 8);
}

async function fill(page: Page, label: string, value: string) {
  await page.getByLabel(label, { exact: false }).fill(value);
}

test.describe("honest load errors", () => {
  test("a failing dashboard fetch shows an error with a working retry", async ({ page }) => {
    const handle = `e2e${rand()}`;
    const key = `LE${rand().slice(0, 3).toUpperCase()}`;

    await test.step("register + project", async () => {
      await page.goto("/register");
      await fill(page, "Display name", "Load Error Tester");
      await fill(page, "Handle", handle);
      await fill(page, "Email", `${handle}@sprintly.test`);
      await fill(page, "Password", "correct-horse-battery-staple");
      await page.getByRole("button", { name: /\$ git init account/ }).click();
      await expect(page).toHaveURL("/");

      await page.goto("/projects");
      await page.getByRole("button", { name: /new project/i }).first().click();
      const dialog = page.getByRole("dialog");
      await dialog.getByLabel("Name").fill("Load Errors");
      await dialog.getByLabel(/^Key/).fill(key);
      await dialog.getByRole("button", { name: /\$ git init project/ }).click();
      await expect(page).toHaveURL(new RegExp(`/projects/${key}$`));
    });

    await test.step("break the dashboard endpoint → the page admits it", async () => {
      await page.route("**/api/v1/projects/*/dashboard", (route) =>
        route.fulfill({
          status: 500,
          contentType: "application/json",
          body: JSON.stringify({
            error: { code: "internal", message: "We broke it. Tell an admin and go get coffee." },
          }),
        }),
      );
      await page.goto(`/projects/${key}/dashboard`);
      const box = page.getByTestId("load-error");
      await expect(box).toBeVisible();
      await expect(box).toContainText(/didn't load/);
      // Not stuck on the loading placeholder.
      await expect(page.getByText("compiling vibes…")).toHaveCount(0);
    });

    await test.step("lift the failure → $ retry recovers without a reload", async () => {
      await page.unroute("**/api/v1/projects/*/dashboard");
      await page.getByRole("button", { name: /retry/ }).click();
      await expect(page.getByRole("heading", { name: "At a glance." })).toBeVisible();
      await expect(page.getByTestId("load-error")).toHaveCount(0);
    });
  });
});
