// fix/jql-sprint-state: `sprint is active` (and Jira's `sprint in
// openSprints()`) run instead of failing with "expected `empty` after `is`" —
// QA report 6 pasted exactly this query from the search page.
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

async function api(page: Page, method: "POST" | "PATCH", path: string, body?: unknown) {
  const res = await page.request.fetch(path, {
    method,
    data: body ?? {},
    headers: { "X-CSRF-Token": await csrf(page) },
  });
  expect(res.ok(), `${method} ${path} → ${res.status()} ${await res.text()}`).toBe(true);
  return res.status() === 204 ? null : res.json();
}

async function runQuery(page: Page, jql: string) {
  await page.getByLabel("query", { exact: true }).fill(jql);
  await page.getByRole("button", { name: /\$ run/ }).click();
}

const QA_QUERY =
  "assignee = currentUser() AND status != done AND sprint is active ORDER BY priority ASC";

test.describe("query search — sprint state", () => {
  test("the QA report's query finds my open work in the running sprint", async ({ page }) => {
    const handle = `e2e${rand()}`;
    const key = `SQ${rand().slice(0, 3).toUpperCase()}`;

    await page.goto("/register");
    await fill(page, "Display name", "Sprinter");
    await fill(page, "Handle", handle);
    await fill(page, "Email", `${handle}@sprintly.test`);
    await fill(page, "Password", "correct-horse-battery-staple");
    await page.getByRole("button", { name: /\$ git init account/ }).click();
    await expect(page).toHaveURL(/\/(me\/day)?$/);
    const me = await (await page.request.get("/api/v1/users/me")).json();

    await page.goto("/projects");
    await page.getByRole("button", { name: /new project/i }).first().click();
    const dialog = page.getByRole("dialog");
    await dialog.getByLabel("Name").fill("Sprint queries");
    await dialog.getByLabel(/^Key/).fill(key);
    await dialog.getByRole("button", { name: /\$ git init project/ }).click();
    await expect(page).toHaveURL(new RegExp(`/projects/${key}$`));

    // One task of mine in a running sprint, one in the backlog.
    const inSprint = await api(page, "POST", `/api/v1/projects/${key}/tasks`, {
      title: "ship the release notes",
      assignee_id: me.id,
    });
    await api(page, "POST", `/api/v1/projects/${key}/tasks`, {
      title: "someday, maybe",
      assignee_id: me.id,
    });
    const today = new Date();
    const inTwoWeeks = new Date(today.getTime() + 14 * 86_400_000);
    const sprint = await api(page, "POST", `/api/v1/projects/${key}/sprints`, {
      name: "Running",
      starts_at: today.toISOString(),
      ends_at: inTwoWeeks.toISOString(),
    });
    await api(page, "POST", `/api/v1/sprints/${sprint.id}/tasks/${inSprint.key}`);
    await api(page, "POST", `/api/v1/sprints/${sprint.id}/start`);

    await page.goto("/search");
    await runQuery(page, QA_QUERY);
    await expect(page.getByTestId("jql-error")).toHaveCount(0);
    const rows = page.getByTestId("jql-results");
    await expect(rows.getByText("ship the release notes")).toBeVisible();
    await expect(rows.getByText("someday, maybe")).toHaveCount(0);

    // The ligature glyph people copy from screenshots works too.
    await runQuery(page, QA_QUERY.replace("!=", "≠"));
    await expect(page.getByTestId("jql-error")).toHaveCount(0);
    await expect(rows.getByText("ship the release notes")).toBeVisible();

    // And the cheatsheet says it exists.
    await page.getByText("what can I write?").click();
    await expect(page.getByText(/sprint in openSprints\(\)/)).toBeVisible();
  });
});
