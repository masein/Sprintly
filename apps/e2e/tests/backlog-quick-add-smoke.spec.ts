// feat/backlog-quick-add: the backlog page can create a task inline. Register
// a fresh user → create a project → open the backlog → quick-add a task → it
// shows up in the list without a reload. Also checks the QA F5 required-field
// nudge on an empty submit.
//
// Pre-reqs: dev stack up (`just up`), SPRINTLY_OPEN_SIGNUP=true.

import { test, expect, type Page } from "@playwright/test";

function rand(): string {
  return Math.random().toString(36).slice(2, 8);
}

async function fill(page: Page, label: string, value: string) {
  await page.getByLabel(label, { exact: false }).fill(value);
}

test.describe("backlog quick-add", () => {
  test("file a task from the backlog; it appears in the list", async ({ page }) => {
    const handle = `e2e${rand()}`;
    const key = `BQ${rand().slice(0, 3).toUpperCase()}`;

    await test.step("register", async () => {
      await page.goto("/register");
      await fill(page, "Display name", "Backlog Quick Tester");
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
      await dialog.getByLabel("Name").fill("Backlog Quick");
      await dialog.getByLabel(/^Key/).fill(key);
      await dialog.getByRole("button", { name: /\$ git init project/ }).click();
      await expect(page).toHaveURL(new RegExp(`/projects/${key}$`));
    });

    await test.step("open the backlog — it starts empty", async () => {
      await page.goto(`/projects/${key}/backlog`);
      await expect(page.getByText(/Backlog zero/i)).toBeVisible();
    });

    await test.step("empty submit shows the F5 nudge, no silent no-op", async () => {
      await page.locator("[data-backlog-quick-add]").click();
      const input = page.getByLabel("new task title");
      await expect(input).toBeVisible();
      await input.press("Enter");
      await expect(page.getByText("Needs a title.")).toBeVisible();
      // Still open, nothing created.
      await expect(input).toBeVisible();
    });

    await test.step("file a task; it appears without a reload", async () => {
      const input = page.getByLabel("new task title");
      await input.fill("triage this pile item");
      await input.press("Enter");
      await expect(page.getByText("triage this pile item")).toBeVisible();
      // The empty-state copy is gone now that there's a row.
      await expect(page.getByText(/Backlog zero/i)).toHaveCount(0);
    });

    await test.step("it's a real backlog task (survives a reload, no sprint)", async () => {
      await page.reload();
      await expect(page.getByText("triage this pile item")).toBeVisible();
      // It links to a task detail page — it was actually created.
      await expect(
        page.getByRole("link", { name: new RegExp(`${key}-\\d+`) }),
      ).toBeVisible();
    });
  });
});
