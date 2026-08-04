// feat/epic-color-editing: an epic's color can be changed after creation.
// QA report: "Allow changing an Epic's color after creation."
//
// Pre-reqs: dev stack up (`just up`), SPRINTLY_OPEN_SIGNUP=true.

import { test, expect, type Page } from "@playwright/test";

function rand(): string {
  return Math.random().toString(36).slice(2, 8);
}

async function fill(page: Page, label: string, value: string) {
  await page.getByLabel(label, { exact: false }).fill(value);
}

test.describe("epic recolor", () => {
  test("pick a new color on an existing epic; the bar follows", async ({ page }) => {
    const handle = `e2e${rand()}`;
    const key = `EC${rand().slice(0, 3).toUpperCase()}`;
    const epicName = "Rainbow";

    await test.step("register + project + an epic", async () => {
      await page.goto("/register");
      await fill(page, "Display name", "Epic Recolorist");
      await fill(page, "Handle", handle);
      await fill(page, "Email", `${handle}@sprintly.test`);
      await fill(page, "Password", "correct-horse-battery-staple");
      await page.getByRole("button", { name: /\$ git init account/ }).click();
      await expect(page).toHaveURL(/\/(me\/day)?$/);

      await page.goto("/projects");
      await page.getByRole("button", { name: /new project/i }).first().click();
      const dialog = page.getByRole("dialog");
      await dialog.getByLabel("Name").fill("Epic Color");
      await dialog.getByLabel(/^Key/).fill(key);
      await dialog.getByRole("button", { name: /\$ git init project/ }).click();
      await expect(page).toHaveURL(new RegExp(`/projects/${key}$`));

      await page.goto(`/projects/${key}/timeline`);
      await page.getByLabel("epic name").fill(epicName);
      await page.getByLabel("epic start").fill("2026-07-01");
      await page.getByLabel("epic end").fill("2026-07-31");
      await page.getByRole("button", { name: /^add$/ }).first().click();
      await expect(page.getByTestId("epic-bar")).toBeVisible();
    });

    await test.step("recolor from the epic row", async () => {
      // Default swatch is #7c5cff; pick #22d3ee.
      const swatch = page.getByRole("button", { name: `${epicName} color` });
      await expect(swatch).toHaveCSS("background-color", "rgb(124, 92, 255)");
      await swatch.click();
      await page.getByRole("button", { name: "recolor #22d3ee" }).click();
      await expect(swatch).toHaveCSS("background-color", "rgb(34, 211, 238)");
    });

    await test.step("the timeline bar and a reload agree", async () => {
      await expect(page.getByTestId("epic-bar")).toHaveCSS(
        "border-color",
        "rgb(34, 211, 238)",
      );
      await page.reload();
      await expect(
        page.getByRole("button", { name: `${epicName} color` }),
      ).toHaveCSS("background-color", "rgb(34, 211, 238)");
    });
  });
});
