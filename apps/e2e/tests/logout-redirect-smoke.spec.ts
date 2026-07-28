// fix/logout-redirect: logging out lands on /login with caches cleared,
// instead of staying on the page showing stale signed-in data. QA report 2:
// "Logout redirection … requires manual page refreshes."
//
// Pre-reqs: dev stack up (`just up`), SPRINTLY_OPEN_SIGNUP=true.

import { test, expect, type Page } from "@playwright/test";

function rand(): string {
  return Math.random().toString(36).slice(2, 8);
}

async function fill(page: Page, label: string, value: string) {
  await page.getByLabel(label, { exact: false }).fill(value);
}

test.describe("logout", () => {
  test("logout lands on /login and the session is really gone", async ({ page }) => {
    const handle = `e2e${rand()}`;

    await page.goto("/register");
    await fill(page, "Display name", "Leaver");
    await fill(page, "Handle", handle);
    await fill(page, "Email", `${handle}@sprintly.test`);
    await fill(page, "Password", "correct-horse-battery-staple");
    await page.getByRole("button", { name: /\$ git init account/ }).click();
    await expect(page).toHaveURL("/");
    await expect(page.getByText(`@${handle}`)).toBeVisible();

    await page.getByRole("button", { name: "logout" }).click();
    await expect(page).toHaveURL(/\/login$/);

    // No lingering session: a protected page bounces back to sign-in.
    await page.goto("/me/day");
    await expect(page).toHaveURL(/\/login$/);
  });
});
