// Time report: seed a closed time log (and a sprint) via the API, then open the
// project metrics page, switch to the Time tab, and assert totals show for a
// custom range and for a picked sprint.
//
// Pre-reqs: dev stack up (`just up`), SPRINTLY_OPEN_SIGNUP=true.

import { test, expect, type Page } from "@playwright/test";

function rand(): string {
  return Math.random().toString(36).slice(2, 8);
}

async function csrf(page: Page): Promise<string> {
  const cookies = await page.context().cookies();
  return cookies.find((c) => c.name === "sprintly_csrf")?.value ?? "";
}
async function apiPost(page: Page, path: string, body: unknown) {
  const res = await page.request.post(path, {
    data: body ?? {},
    headers: { "X-CSRF-Token": await csrf(page) },
  });
  return res;
}

test.describe("time report", () => {
  test("logged time shows on the metrics Time tab for a range and a sprint", async ({ page }) => {
    const handle = `e2e${rand()}`;
    const key = `TR${rand().slice(0, 3).toUpperCase()}`;

    await test.step("register + create a project", async () => {
      await page.goto("/register");
      await fill(page, "Display name", "Time Tester");
      await fill(page, "Handle", handle);
      await fill(page, "Email", `${handle}@sprintly.test`);
      await fill(page, "Password", "correct-horse-battery-staple");
      await page.getByRole("button", { name: /\$ git init account/ }).click();
      await expect(page).toHaveURL("/");

      await page.goto("/projects");
      await page.getByRole("button", { name: /new project/i }).first().click();
      const dialog = page.getByRole("dialog");
      await dialog.getByLabel("Name").fill("Time Report");
      await dialog.getByLabel(/^Key/).fill(key);
      await dialog.getByRole("button", { name: /\$ git init project/ }).click();
      await expect(page).toHaveURL(new RegExp(`/projects/${key}$`));
    });

    let taskKey = "";
    await test.step("add a card", async () => {
      await page.locator("[data-add-card-button]").first().click();
      await page.getByPlaceholder("card title").fill("Timed work");
      await page.getByRole("button", { name: /^add$/ }).click();
      const link = page.getByRole("link", { name: new RegExp(`${key}-\\d+`) }).first();
      await expect(link).toBeVisible();
      taskKey = (await link.innerText()).trim();
    });

    await test.step("seed a 90-minute log yesterday + a sprint containing the task", async () => {
      const yesterday = new Date(Date.now() - 24 * 3600 * 1000).toISOString();
      const logRes = await apiPost(page, `/api/v1/tasks/${taskKey}/time-logs`, {
        started_at: yesterday,
        duration_minutes: 90,
        billable: true,
      });
      expect(logRes.ok()).toBeTruthy();

      const starts = new Date(Date.now() - 10 * 24 * 3600 * 1000).toISOString();
      const ends = new Date(Date.now() + 10 * 24 * 3600 * 1000).toISOString();
      const sprintRes = await apiPost(page, `/api/v1/projects/${key}/sprints`, {
        name: "E2E Sprint",
        starts_at: starts,
        ends_at: ends,
      });
      expect(sprintRes.ok()).toBeTruthy();
      const sprint = await sprintRes.json();
      const assignRes = await apiPost(page, `/api/v1/sprints/${sprint.id}/tasks/${taskKey}`, {});
      expect(assignRes.ok()).toBeTruthy();
    });

    await test.step("Time tab shows the total for the default range", async () => {
      await page.goto(`/projects/${key}/metrics`);
      await page.getByRole("tab", { name: "time" }).click();
      // 90 minutes → "1h 30m", shown in the total tile and the by-person row.
      await expect(page.getByText("1h 30m").first()).toBeVisible({ timeout: 15_000 });
      await expect(page.getByText(`@${handle}`).first()).toBeVisible();
    });

    await test.step("pick the sprint → totals still show", async () => {
      await page.getByLabel("sprint").selectOption({ label: "E2E Sprint" });
      await expect(page.getByText("1h 30m").first()).toBeVisible({ timeout: 15_000 });
      await expect(page.getByText(/sprint: E2E Sprint/i)).toBeVisible();
    });
  });
});

async function fill(page: Page, label: string, value: string) {
  await page.getByLabel(label, { exact: false }).fill(value);
}
