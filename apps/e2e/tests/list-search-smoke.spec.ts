// feat/list-search: search boxes on the board, the sprint task list (which
// also filters the backlog panel beside it), and the backlog page — plus a
// searchable assignee picker instead of a native select.
//
// QA report 3, high priority: "Ability to search a task in sprint and backlog
// tasks", "Ability to search in board tasks", "Can search while assigning the
// new task to a person".
//
// Pre-reqs: dev stack up (`just up`), SPRINTLY_OPEN_SIGNUP=true.

import { test, expect, type Page } from "@playwright/test";

function rand(): string {
  return Math.random().toString(36).slice(2, 8);
}

async function fill(page: Page, label: string, value: string) {
  await page.getByLabel(label, { exact: false }).fill(value);
}

test.describe("list search", () => {
  test("board, sprint, and backlog filter in place; assignee is searchable", async ({ page }) => {
    const handle = `e2e${rand()}`;
    const key = `LS${rand().slice(0, 3).toUpperCase()}`;

    await test.step("register + project + three distinctly-named tasks", async () => {
      await page.goto("/register");
      await fill(page, "Display name", "Search Tester");
      await fill(page, "Handle", handle);
      await fill(page, "Email", `${handle}@sprintly.test`);
      await fill(page, "Password", "correct-horse-battery-staple");
      await page.getByRole("button", { name: /\$ git init account/ }).click();
      // Wait for the session to land before navigating — "/" today, /me/day
      // once the landing redirect merges.
      await expect(page).toHaveURL(/\/(me\/day)?$/);

      await page.goto("/projects");
      await page.getByRole("button", { name: /new project/i }).first().click();
      const dialog = page.getByRole("dialog");
      await dialog.getByLabel("Name").fill("List Search");
      await dialog.getByLabel(/^Key/).fill(key);
      await dialog.getByRole("button", { name: /\$ git init project/ }).click();
      await expect(page).toHaveURL(new RegExp(`/projects/${key}$`));

      await page.goto(`/projects/${key}/backlog`);
      await page.locator("[data-backlog-quick-add]").click();
      for (const title of ["needle in the haystack", "unrelated hay", "more hay"]) {
        const input = page.getByLabel("new task title");
        await input.fill(title);
        await input.press("Enter");
        await expect(page.getByText(title)).toBeVisible();
      }
    });

    await test.step("backlog search narrows the pile", async () => {
      await page.goto(`/projects/${key}/backlog`);
      await expect(page.getByText("unrelated hay")).toBeVisible();
      await page.getByLabel("search backlog tasks", { exact: true }).fill("needle");
      await expect(page.getByText("needle in the haystack")).toBeVisible();
      await expect(page.getByText("unrelated hay")).toHaveCount(0);
      await expect(page.getByText("1 of 3 shown")).toBeVisible();

      // Searching by key works too, and clearing restores everything.
      await page.getByLabel("search backlog tasks", { exact: true }).fill(`${key}-2`);
      await expect(page.getByText("unrelated hay")).toBeVisible();
      await page.getByRole("button", { name: "clear search backlog tasks" }).click();
      await expect(page.getByText("more hay")).toBeVisible();
    });

    await test.step("board search narrows the cards", async () => {
      await page.goto(`/projects/${key}`);
      await expect(page.getByText("unrelated hay")).toBeVisible();
      await page.getByLabel("search board tasks", { exact: true }).fill("needle");
      await expect(page.getByText("needle in the haystack")).toBeVisible();
      await expect(page.getByText("unrelated hay")).toHaveCount(0);
      await expect(page.getByText("1 of 3")).toBeVisible();
    });

    await test.step("sprint search covers the sprint list and the backlog panel", async () => {
      await page.goto(`/projects/${key}/sprints`);
      await page.getByRole("button", { name: /new sprint/i }).click();
      await page.getByPlaceholder(/Sprint 23/i).fill("Sprint 1");
      await page.getByRole("button", { name: /\$ git init sprint/ }).click();
      await expect(page).toHaveURL(/\/sprints\/[0-9a-f-]+$/);

      // Commit one task so both lists have something to filter.
      await page.getByRole("button", { name: /add tasks/i }).click();
      const adder = page.getByLabel("add a task to this sprint");
      await adder.fill("needle in the haystack");
      await expect(page.getByRole("button", { name: /needle in the haystack/ }).first()).toBeVisible();
      await page.getByRole("button", { name: /needle in the haystack/ }).first().click();
      await expect(page.getByTestId("sprint-drop").getByText("needle in the haystack")).toBeVisible();

      const backlogPanel = page.getByTestId("backlog-drop");
      await expect(backlogPanel.getByText("unrelated hay")).toBeVisible();

      await page.getByLabel("search sprint tasks", { exact: true }).fill("needle");
      await expect(page.getByTestId("sprint-drop").getByText("needle in the haystack")).toBeVisible();
      // The backlog panel beside it filters with the same box.
      await expect(backlogPanel.getByText("unrelated hay")).toHaveCount(0);
    });

    await test.step("assignee picker searches people", async () => {
      await page.goto(`/tasks/${key}-1`);
      await page.getByLabel("assignee", { exact: true }).click();
      await page.getByLabel("search assignee", { exact: true }).fill("nobody-by-that-name");
      await expect(page.getByText(/nobody matches/)).toBeVisible();

      await page.getByLabel("search assignee", { exact: true }).fill(handle.slice(0, 5));
      await page.getByRole("option", { name: new RegExp(`@${handle}`) }).click();
      await expect(page.getByLabel("assignee", { exact: true })).toContainText(handle);

      // It stuck.
      await page.reload();
      await expect(page.getByLabel("assignee", { exact: true })).toContainText(handle);
    });
  });
});
