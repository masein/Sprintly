// feat/clockwork-history: My Day and the project dashboard can step back
// through past weeks of time logs instead of being stuck on the current one.
// Register → project → task → log 1h dated last Monday → both surfaces show
// nothing for this week and the hour one "previous week" click away.
//
// Pre-reqs: dev stack up (`just up`), SPRINTLY_OPEN_SIGNUP=true.

import { test, expect, type Page } from "@playwright/test";

function rand(): string {
  return Math.random().toString(36).slice(2, 8);
}

async function fill(page: Page, label: string, value: string) {
  await page.getByLabel(label, { exact: false }).fill(value);
}

/** Monday of last week, UTC, as YYYY-MM-DD — one "previous week" click away. */
function lastMondayISO(): string {
  const d = new Date();
  const day = d.getUTCDay();
  const offset = day === 0 ? 6 : day - 1;
  d.setUTCDate(d.getUTCDate() - offset - 7);
  return d.toISOString().slice(0, 10);
}

test.describe("clockwork history", () => {
  test("last week's logs are reachable from My Day and the dashboard", async ({ page }) => {
    const handle = `e2e${rand()}`;
    const key = `CW${rand().slice(0, 3).toUpperCase()}`;

    await test.step("register + project + a task", async () => {
      await page.goto("/register");
      await fill(page, "Display name", "Clockwork Tester");
      await fill(page, "Handle", handle);
      await fill(page, "Email", `${handle}@sprintly.test`);
      await fill(page, "Password", "correct-horse-battery-staple");
      await page.getByRole("button", { name: /\$ git init account/ }).click();
      await expect(page).toHaveURL(/\/(me\/day)?$/);

      await page.goto("/projects");
      await page.getByRole("button", { name: /new project/i }).first().click();
      const dialog = page.getByRole("dialog");
      await dialog.getByLabel("Name").fill("Clockwork");
      await dialog.getByLabel(/^Key/).fill(key);
      await dialog.getByRole("button", { name: /\$ git init project/ }).click();
      await expect(page).toHaveURL(new RegExp(`/projects/${key}$`));

      await page.goto(`/projects/${key}/backlog`);
      await page.locator("[data-backlog-quick-add]").click();
      const input = page.getByLabel("new task title");
      await input.fill("time travel");
      await input.press("Enter");
      await expect(page.getByText("time travel")).toBeVisible();
    });

    await test.step("log 1h against the task, dated last Monday", async () => {
      await page.goto(`/tasks/${key}-1`);
      await page.getByRole("button", { name: /manual entry/i }).click();
      // Scope to the timer card — the details panel has its own date ("due
      // date") and hours ("estimate hours") inputs now.
      const timer = page.locator("section", { hasText: "manual entry" });
      await timer.locator('input[type="date"]').fill(lastMondayISO());
      await timer.getByLabel("hours", { exact: true }).fill("1");
      await timer.getByLabel("minutes").fill("0");
      await timer.getByRole("button", { name: "add log" }).click();
      // The log list shows the entry once it lands.
      await expect(page.getByText("1h", { exact: true }).first()).toBeVisible();
    });

    await test.step("My Day: this week is empty, last week has the hour", async () => {
      await page.goto("/me/day");
      const panel = page.locator('section[aria-label="clockwork"]');
      await expect(panel.getByText("nothing logged this week")).toBeVisible();

      await panel.getByRole("button", { name: "previous week" }).click();
      await expect(panel.getByText(`week of ${lastMondayISO()}`)).toBeVisible();
      await expect(panel.getByRole("link", { name: `${key}-1` })).toBeVisible();
      await expect(panel.getByText("1h", { exact: true }).first()).toBeVisible();

      // "this week" resets the window.
      await panel.getByRole("button", { name: "this week" }).click();
      await expect(panel.getByText("nothing logged this week")).toBeVisible();
    });

    await test.step("dashboard: contributors are week-navigable too", async () => {
      await page.goto(`/projects/${key}/dashboard`);
      const panel = page.locator('section[aria-label="top contributors"]');
      await expect(panel.getByText("no time logged")).toBeVisible();

      await panel.getByRole("button", { name: "previous week" }).click();
      await expect(panel.getByText(`@${handle}`)).toBeVisible();
      await expect(panel.getByText("1h", { exact: true })).toBeVisible();
    });
  });
});
