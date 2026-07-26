// feat/subtask-conversion: convert a task into a subtask and back, from the
// task detail's parent field. QA report: "Allow converting a task into a
// subtask, a subtask into a task, and moving subtasks between parent tasks."
//
// Pre-reqs: dev stack up (`just up`), SPRINTLY_OPEN_SIGNUP=true.

import { test, expect, type Page } from "@playwright/test";

function rand(): string {
  return Math.random().toString(36).slice(2, 8);
}

async function fill(page: Page, label: string, value: string) {
  await page.getByLabel(label, { exact: false }).fill(value);
}

test.describe("task ↔ subtask conversion", () => {
  test("demote a task under a parent, then promote it back", async ({ page }) => {
    const handle = `e2e${rand()}`;
    const key = `CV${rand().slice(0, 3).toUpperCase()}`;

    await test.step("register + project + two top-level tasks", async () => {
      await page.goto("/register");
      await fill(page, "Display name", "Converter");
      await fill(page, "Handle", handle);
      await fill(page, "Email", `${handle}@sprintly.test`);
      await fill(page, "Password", "correct-horse-battery-staple");
      await page.getByRole("button", { name: /\$ git init account/ }).click();
      await expect(page).toHaveURL("/");

      await page.goto("/projects");
      await page.getByRole("button", { name: /new project/i }).first().click();
      const dialog = page.getByRole("dialog");
      await dialog.getByLabel("Name").fill("Convert");
      await dialog.getByLabel(/^Key/).fill(key);
      await dialog.getByRole("button", { name: /\$ git init project/ }).click();
      await expect(page).toHaveURL(new RegExp(`/projects/${key}$`));

      await page.goto(`/projects/${key}/backlog`);
      await page.locator("[data-backlog-quick-add]").click();
      for (const title of ["the epic saga", "a mere chapter"]) {
        const input = page.getByLabel("new task title");
        await input.fill(title);
        await input.press("Enter");
        await expect(page.getByText(title)).toBeVisible();
      }
    });

    await test.step("make KEY-2 a subtask of KEY-1", async () => {
      await page.goto(`/tasks/${key}-2`);
      await page.getByRole("button", { name: "make subtask of…" }).click();
      await page.getByLabel("parent task search").fill("epic saga");
      await page.getByRole("button", { name: new RegExp(`${key}-1`) }).click();
      await expect(page.getByRole("link", { name: `↳ ${key}-1` })).toBeVisible();
    });

    await test.step("it left the board and joined the parent's panel", async () => {
      await page.goto(`/projects/${key}`);
      await expect(page.getByText("the epic saga")).toBeVisible();
      await expect(page.getByText("a mere chapter")).toHaveCount(0);

      await page.goto(`/tasks/${key}-1`);
      await expect(page.getByText(`subtasks (1)`)).toBeVisible();
      await expect(page.getByRole("link", { name: `${key}-2` })).toBeVisible();
    });

    await test.step("promote it back to a top-level task", async () => {
      await page.goto(`/tasks/${key}-2`);
      await page.getByRole("button", { name: "↑ promote" }).click();
      await expect(page.getByRole("button", { name: "make subtask of…" })).toBeVisible();
      await expect(page.getByRole("link", { name: `↳ ${key}-1` })).toHaveCount(0);

      await page.goto(`/projects/${key}`);
      await expect(page.getByText("a mere chapter")).toBeVisible();
    });
  });
});
