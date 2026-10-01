// feat/sprint-charts: the sprint charts card — burndown, burnup, velocity —
// with sprint progress, scope change, days and health beside the chart, on
// the project dashboard and the sprint page. KPI request + QA report 6:
// "charts are empty and need UI improvement", "sprint progress", "scope
// change", "project-level KPIs near the charts".
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

async function api(page: Page, method: "POST" | "DELETE", path: string, body?: unknown) {
  const res = await page.request.fetch(path, {
    method,
    data: body ?? {},
    headers: { "X-CSRF-Token": await csrf(page) },
  });
  expect(res.ok(), `${method} ${path} → ${res.status()} ${await res.text()}`).toBe(true);
  return res.status() === 204 ? null : res.json();
}

test.describe("sprint charts + KPIs", () => {
  test("progress, scope change and a real burndown, on the dashboard and the sprint", async ({ page }) => {
    const handle = `e2e${rand()}`;
    const key = `SC${rand().slice(0, 3).toUpperCase()}`;

    await page.goto("/register");
    await fill(page, "Display name", "Charter");
    await fill(page, "Handle", handle);
    await fill(page, "Email", `${handle}@sprintly.test`);
    await fill(page, "Password", "correct-horse-battery-staple");
    await page.getByRole("button", { name: /\$ git init account/ }).click();
    await expect(page).toHaveURL(/\/(me\/day)?$/);

    await page.goto("/projects");
    await page.getByRole("button", { name: /new project/i }).first().click();
    const dialog = page.getByRole("dialog");
    await dialog.getByLabel("Name").fill("Charts");
    await dialog.getByLabel(/^Key/).fill(key);
    await dialog.getByRole("button", { name: /\$ git init project/ }).click();
    await expect(page).toHaveURL(new RegExp(`/projects/${key}$`));

    const day = 86_400_000;
    const sprint = await api(page, "POST", `/api/v1/projects/${key}/sprints`, {
      name: "Sprint 12",
      starts_at: new Date(Date.now() - 2 * day).toISOString(),
      ends_at: new Date(Date.now() + 5 * day).toISOString(),
    });
    const boards = await (await page.request.get(`/api/v1/projects/${key}/boards`)).json();
    const done = boards.items[0].columns.find((c: { category: string }) => c.category === "done");
    const keys: string[] = [];
    for (const [title, pts] of [["a", 5], ["b", 3], ["c", 2]] as const) {
      const t = await api(page, "POST", `/api/v1/projects/${key}/tasks`, { title, story_points: pts });
      await api(page, "POST", `/api/v1/sprints/${sprint.id}/tasks/${t.key}`);
      keys.push(t.key);
    }
    await api(page, "POST", `/api/v1/sprints/${sprint.id}/start`);
    // After the start: one more task joins (scope change), one is finished.
    const late = await api(page, "POST", `/api/v1/projects/${key}/tasks`, { title: "late", story_points: 5 });
    await api(page, "POST", `/api/v1/sprints/${sprint.id}/tasks/${late.key}`);
    await api(page, "POST", `/api/v1/tasks/${keys[0]}/move`, { column_id: done.id });

    await test.step("dashboard: the card, its burndown and the KPIs beside it", async () => {
      await page.goto(`/projects/${key}/dashboard`);
      const card = page.getByRole("region", { name: "sprint charts" });
      await expect(card.getByRole("heading", { name: "Sprint burn down" })).toBeVisible();
      await expect(card).toContainText("Sprint 12");
      // A drawn line, not an empty-state box.
      await expect(card.locator('[data-chart="burndown"] .recharts-area-curve')).toBeVisible();
      const progress = card.locator('[data-kpi="sprint-progress"]');
      await expect(progress).toContainText("1 / 4");
      await expect(progress.getByRole("img", { name: "25% of sprint tasks done" })).toBeVisible();
      // 5 points added after the start ÷ 10 at the start.
      await expect(card.locator('[data-kpi="scope-change"]')).toHaveText("+50%");
      await expect(card.locator('[data-kpi="key-metrics"]')).toContainText("days elapsed");
      await expect(card.locator('[data-kpi="key-metrics"]')).toContainText("3 / 8");
      await expect(card.locator('[data-kpi="quick-view"]')).toContainText("on track");
    });

    await test.step("burnup and velocity tabs", async () => {
      const card = page.getByRole("region", { name: "sprint charts" });
      await card.getByRole("tab", { name: "burnup" }).click();
      await expect(card.getByRole("heading", { name: "Sprint burn up" })).toBeVisible();
      await expect(card.locator('[data-chart="burnup"] .recharts-line').first()).toBeVisible();
      await card.getByRole("tab", { name: "velocity" }).click();
      await expect(card.getByRole("heading", { name: "Velocity" })).toBeVisible();
      await expect(card.getByText(/No completed sprints yet/)).toBeVisible();
      // The running sprint's tally so far is still shown.
      await expect(card).toContainText("5 pts");
    });

    await test.step("the sprint page carries the same card", async () => {
      await page.goto(`/sprints/${sprint.id}`);
      const card = page.getByRole("region", { name: "sprint charts" });
      await expect(card.locator('[data-chart="burndown"]')).toBeVisible();
      await expect(card.locator('[data-kpi="scope-change"]')).toHaveText("+50%");
    });

    await test.step("complete it; velocity has a bar and an average", async () => {
      await api(page, "POST", `/api/v1/sprints/${sprint.id}/complete`, { carry_over: { to: "backlog" } });
      await page.goto(`/projects/${key}/dashboard`);
      const card = page.getByRole("region", { name: "sprint charts" });
      // Nothing running now → the card opens on velocity.
      await expect(card.getByRole("heading", { name: "Velocity" })).toBeVisible();
      await expect(card.locator(".recharts-bar-rectangle")).toHaveCount(1);
      await expect(card).toContainText("5 pts/sprint");
      await card.getByLabel("velocity unit").selectOption("tasks");
      await expect(card).toContainText("1 tasks/sprint");
    });
  });
});
