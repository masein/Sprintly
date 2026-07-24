// feat/task-sprint-field: the task detail's details panel gains a `sprint`
// select — the discoverable way to move a task OUT of a sprint (to the
// backlog) or into another sprint, from anywhere. Reported from real usage:
// "move a sprint task to backlog — not found".
//
// Pre-reqs: dev stack up (`just up`), SPRINTLY_OPEN_SIGNUP=true.

import { test, expect, type Page } from "@playwright/test";

function rand(): string {
  return Math.random().toString(36).slice(2, 8);
}

async function fill(page: Page, label: string, value: string) {
  await page.getByLabel(label, { exact: false }).fill(value);
}

test.describe("task sprint field", () => {
  test("move a sprint task to the backlog and back, from the task detail", async ({ page }) => {
    const handle = `e2e${rand()}`;
    const key = `SF${rand().slice(0, 3).toUpperCase()}`;

    await test.step("register + project + a sprint with one task", async () => {
      await page.goto("/register");
      await fill(page, "Display name", "Sprint Field Tester");
      await fill(page, "Handle", handle);
      await fill(page, "Email", `${handle}@sprintly.test`);
      await fill(page, "Password", "correct-horse-battery-staple");
      await page.getByRole("button", { name: /\$ git init account/ }).click();
      await expect(page).toHaveURL("/");

      await page.goto("/projects");
      await page.getByRole("button", { name: /new project/i }).first().click();
      const dialog = page.getByRole("dialog");
      await dialog.getByLabel("Name").fill("Sprint Field");
      await dialog.getByLabel(/^Key/).fill(key);
      await dialog.getByRole("button", { name: /\$ git init project/ }).click();
      await expect(page).toHaveURL(new RegExp(`/projects/${key}$`));

      await page.goto(`/projects/${key}/sprints`);
      await page.getByRole("button", { name: /new sprint/i }).click();
      await page.getByPlaceholder(/Sprint 23/i).fill("Sprint 1");
      await page.getByRole("button", { name: /\$ git init sprint/ }).click();
      await expect(page).toHaveURL(/\/sprints\/[0-9a-f-]+$/);
      await page.getByRole("button", { name: /add tasks/i }).click();
      const adder = page.getByLabel("add a task to this sprint");
      await adder.fill("Escape artist");
      await adder.press("Enter");
      await expect(page.getByText("Escape artist")).toBeVisible();
    });

    await test.step("the task detail shows the sprint in its details panel", async () => {
      await page.goto(`/tasks/${key}-1`);
      await expect(page.getByLabel("sprint", { exact: true })).toHaveValue(/./); // a sprint id
      await expect(page.getByLabel("sprint", { exact: true }).locator("option:checked")).toHaveText(/Sprint 1/);
    });

    await test.step("select backlog → the task drops out of the sprint", async () => {
      await page.getByLabel("sprint", { exact: true }).selectOption("");
      await expect(page.getByLabel("sprint", { exact: true })).toHaveValue("");
      // Really in the backlog now.
      await page.goto(`/projects/${key}/backlog`);
      await expect(page.getByText("Escape artist")).toBeVisible();
    });

    await test.step("select the sprint again → it leaves the backlog", async () => {
      await page.goto(`/tasks/${key}-1`);
      // Option labels carry a state suffix ("Sprint 1 · planned"), so pick by value.
      const sprintValue = await page
        .getByLabel("sprint", { exact: true })
        .locator("option", { hasText: "Sprint 1" })
        .getAttribute("value");
      await page.getByLabel("sprint", { exact: true }).selectOption(sprintValue!);
      await expect(page.getByLabel("sprint", { exact: true })).not.toHaveValue("");
      await page.goto(`/projects/${key}/backlog`);
      await expect(page.getByText("Escape artist")).toHaveCount(0);
    });
  });
});
