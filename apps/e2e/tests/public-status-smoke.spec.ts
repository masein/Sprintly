// F18 smoke: a project lead enables the public status page, and the tokenised
// URL renders for a visitor with NO session.
//
// Pre-reqs: dev stack up (`just up`), SPRINTLY_OPEN_SIGNUP=true.

import { test, expect, type Page } from "@playwright/test";

function rand(): string {
  return Math.random().toString(36).slice(2, 8);
}

test.describe("F18 public status smoke", () => {
  test("enable a public status page, then view it logged out", async ({ page }) => {
    const handle = `e2e${rand()}`;
    const key = `PS${rand().slice(0, 3).toUpperCase()}`;

    await test.step("register", async () => {
      await page.goto("/register");
      await fill(page, "Display name", "Status Tester");
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
      await dialog.getByLabel("Name").fill("Public Demo");
      await dialog.getByLabel(/^Key/).fill(key);
      await dialog.getByRole("button", { name: /\$ git init project/ }).click();
      await expect(page).toHaveURL(new RegExp(`/projects/${key}$`));
    });

    let url = "";
    await test.step("enable the public status page", async () => {
      await page.getByRole("button", { name: /public status/i }).click();
      await page.getByRole("button", { name: /enable public status/i }).click();
      const code = page.getByTestId("public-url");
      await expect(code).toBeVisible();
      url = ((await code.textContent()) ?? "").trim();
      expect(url).toContain("/status/");
    });

    await test.step("a logged-out visitor sees the whitelisted summary", async () => {
      // Drop the session entirely.
      await page.context().clearCookies();
      await page.goto(url);
      await expect(page.getByRole("heading", { name: "Public Demo" })).toBeVisible();
      await expect(page.getByText(/live status/i)).toBeVisible();
      // Board column counts render (default board has columns).
      await expect(page.getByText(/^board$/i)).toBeVisible();
    });
  });
});

async function fill(page: Page, label: string, value: string) {
  await page.getByLabel(label, { exact: false }).fill(value);
}
