// fix/my-day-live: My Day updates itself — no manual refresh. A change made
// in another tab (assign a task to me) lands on an already-open My Day via
// the WebSocket invalidation. QA report: "Changes require a manual refresh
// before appearing. The view should update automatically."
//
// Pre-reqs: dev stack up (`just up`), SPRINTLY_OPEN_SIGNUP=true.

import { test, expect, type Page } from "@playwright/test";

function rand(): string {
  return Math.random().toString(36).slice(2, 8);
}

async function fill(page: Page, label: string, value: string) {
  await page.getByLabel(label, { exact: false }).fill(value);
}

test.describe("my day live updates", () => {
  test("assigning a task in another tab shows up on an open My Day", async ({ page, context }) => {
    const handle = `e2e${rand()}`;
    const key = `MD${rand().slice(0, 3).toUpperCase()}`;

    await test.step("register + project + a task", async () => {
      await page.goto("/register");
      await fill(page, "Display name", "My Day Tester");
      await fill(page, "Handle", handle);
      await fill(page, "Email", `${handle}@sprintly.test`);
      await fill(page, "Password", "correct-horse-battery-staple");
      await page.getByRole("button", { name: /\$ git init account/ }).click();
      await expect(page).toHaveURL(/\/(me\/day)?$/);

      await page.goto("/projects");
      await page.getByRole("button", { name: /new project/i }).first().click();
      const dialog = page.getByRole("dialog");
      await dialog.getByLabel("Name").fill("My Day Live");
      await dialog.getByLabel(/^Key/).fill(key);
      await dialog.getByRole("button", { name: /\$ git init project/ }).click();
      await expect(page).toHaveURL(new RegExp(`/projects/${key}$`));

      await page.goto(`/projects/${key}/backlog`);
      await page.locator("[data-backlog-quick-add]").click();
      const input = page.getByLabel("new task title");
      await input.fill("hot potato");
      await input.press("Enter");
      await expect(page.getByText("hot potato")).toBeVisible();
    });

    await test.step("park My Day in this tab — nothing assigned yet", async () => {
      await page.goto("/me/day");
      await expect(page.getByText("inbox zero. touch grass.")).toBeVisible();
    });

    await test.step("assign the task to me from a second tab", async () => {
      const other = await context.newPage();
      await other.goto(`/tasks/${key}-1`);
      const assignee = other.getByLabel("assignee", { exact: true });
      // Pick myself (the only member).
      const myValue = await assignee
        .locator("option", { hasText: `@${handle}` })
        .getAttribute("value");
      await assignee.selectOption(myValue!);
      await expect(assignee).not.toHaveValue("");
      await other.close();
    });

    await test.step("My Day catches up on its own — no reload", async () => {
      // The WS invalidation should repaint the still-open first tab. Poll-free
      // assertion: Playwright retries until the row appears (well under the
      // 60s interval fallback, so a pass proves the push path).
      await expect(page.getByText("hot potato")).toBeVisible({ timeout: 15_000 });
      await expect(page.getByText("inbox zero. touch grass.")).toHaveCount(0);
    });
  });
});
