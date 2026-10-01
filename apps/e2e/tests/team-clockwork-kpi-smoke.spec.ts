// feat/team-clockwork: the project dashboard's team panel — a clockwork table
// of hours per member per day (this week / last week / this month / last month
// / a custom range), and each member's sprint KPIs: completion, on-time and
// estimation rates (KPI request alongside QA report 6: "there is no way to see
// others' KPIs, clockwork in the dashboard of the project").
//
// Pre-reqs: dev stack up (`just up`), SPRINTLY_OPEN_SIGNUP=true.

import { test, expect, type Page } from "@playwright/test";

function rand(): string {
  return Math.random().toString(36).slice(2, 8);
}

async function fill(page: Page, label: string, value: string) {
  await page.getByLabel(label, { exact: false }).fill(value);
}

async function csrf(page: Page): Promise<string> {
  const cookies = await page.context().cookies();
  return cookies.find((c) => c.name === "sprintly_csrf")?.value ?? "";
}

async function api(page: Page, method: "POST", path: string, body?: unknown) {
  const res = await page.request.fetch(path, {
    method,
    data: body ?? {},
    headers: { "X-CSRF-Token": await csrf(page) },
  });
  expect(res.ok(), `${method} ${path} → ${res.status()} ${await res.text()}`).toBe(true);
  return res.status() === 204 ? null : res.json();
}

const isoDay = (d: Date) => d.toISOString().slice(0, 10);

test.describe("team clockwork + KPIs", () => {
  test("a lead sees hours by member and day, and each member's sprint rates", async ({ page }) => {
    const handle = `e2e${rand()}`;
    const key = `TK${rand().slice(0, 3).toUpperCase()}`;

    await page.goto("/register");
    await fill(page, "Display name", "Team Lead");
    await fill(page, "Handle", handle);
    await fill(page, "Email", `${handle}@sprintly.test`);
    await fill(page, "Password", "correct-horse-battery-staple");
    await page.getByRole("button", { name: /\$ git init account/ }).click();
    await expect(page).toHaveURL(/\/(me\/day)?$/);
    const me = await (await page.request.get("/api/v1/users/me")).json();

    await page.goto("/projects");
    await page.getByRole("button", { name: /new project/i }).first().click();
    const dialog = page.getByRole("dialog");
    await dialog.getByLabel("Name").fill("Team");
    await dialog.getByLabel(/^Key/).fill(key);
    await dialog.getByRole("button", { name: /\$ git init project/ }).click();
    await expect(page).toHaveURL(new RegExp(`/projects/${key}$`));

    // A running sprint with two of my tasks: one estimated and done on time.
    const day = 86_400_000;
    const sprint = await api(page, "POST", `/api/v1/projects/${key}/sprints`, {
      name: "Sprint 7",
      starts_at: new Date(Date.now() - day).toISOString(),
      ends_at: new Date(Date.now() + 6 * day).toISOString(),
    });
    const boards = await (await page.request.get(`/api/v1/projects/${key}/boards`)).json();
    const done = boards.items[0].columns.find((c: { category: string }) => c.category === "done");
    const a = await api(page, "POST", `/api/v1/projects/${key}/tasks`, {
      title: "estimated and on time",
      assignee_id: me.id,
      sprint_id: sprint.id,
      story_points: 3,
      due_date: isoDay(new Date(Date.now() + 2 * day)),
    });
    await api(page, "POST", `/api/v1/projects/${key}/tasks`, {
      title: "neither",
      assignee_id: me.id,
      sprint_id: sprint.id,
    });
    await api(page, "POST", `/api/v1/tasks/${a.key}/move`, { column_id: done.id });
    await api(page, "POST", `/api/v1/sprints/${sprint.id}/start`);

    // 90 minutes logged today (noon UTC, so the UTC day is unambiguous).
    const today = isoDay(new Date());
    await api(page, "POST", `/api/v1/tasks/${a.key}/time-logs`, {
      started_at: `${today}T12:00:00Z`,
      duration_minutes: 90,
    });

    await page.goto(`/projects/${key}/dashboard`);
    const panel = page.getByRole("region", { name: "team" });

    await test.step("clockwork: my row has today's 1.5h", async () => {
      const row = panel.locator(`[data-clockwork-row="${handle}"]`);
      await expect(row).toBeVisible();
      await expect(row.locator(`td[title*="${today}"]`)).toContainText("1.5h");
      await expect(row.locator("td").last()).toContainText("1h 30m");
      // Presets reshape the columns: a month is ~30 days, a week 7.
      await expect(panel.locator("[data-clockwork] thead th")).toHaveCount(7 + 2);
      await panel.getByRole("button", { name: "this month" }).click();
      await expect
        .poll(() => panel.locator("[data-clockwork] thead th").count())
        .toBeGreaterThan(28 + 1);
      await panel.getByRole("button", { name: "last week" }).click();
      await expect(row.locator("td").last()).not.toContainText("1h 30m");
      // A custom range from the calendar.
      await panel.getByLabel("from").fill(today);
      await panel.getByLabel("to").fill(today);
      await expect(panel.locator("[data-clockwork] thead th")).toHaveCount(1 + 2);
      await expect(row).toContainText("1.5h");
    });

    await test.step("KPIs: completion, on time, estimated", async () => {
      await panel.getByRole("tab", { name: /KPIs/ }).click();
      await expect(panel).toContainText("Sprint 7");
      const row = panel.locator(`[data-kpi-row="${handle}"]`);
      await expect(row).toContainText("50%"); // 1 of 2 done; 1 of 2 estimated
      await expect(row).toContainText("100%"); // the done one beat its due date
      await expect(row).toContainText("1/1 with a due date");
    });
  });
});
