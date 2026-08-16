// feat/sprint-complete-carryover: completing a sprint asks where the
// unfinished work goes (backlog / another sprint / a new one you name), and
// the sprints list gains inline edit, delete, and a filter box.
// QA report 3: "In make a sprint complete, ask to move todo and in progress
// tasks to the new sprint (get the new sprint name) or backlog" +
// "Ability to delete and edit the created sprint (change time), also add
// search in sprints".
//
// Pre-reqs: dev stack up (`just up`), SPRINTLY_OPEN_SIGNUP=true.

import { test, expect, type Page } from "@playwright/test";

function rand(): string {
  return Math.random().toString(36).slice(2, 8);
}

async function fill(page: Page, label: string, value: string) {
  await page.getByLabel(label, { exact: false }).fill(value);
}

test.describe("sprint completion carry-over", () => {
  test("unfinished work moves to a new sprint; list edit/delete/search work", async ({ page }) => {
    const handle = `e2e${rand()}`;
    const key = `CO${rand().slice(0, 3).toUpperCase()}`;
    page.on("dialog", (d) => d.accept());

    await test.step("register + project + two backlog tasks", async () => {
      await page.goto("/register");
      await fill(page, "Display name", "Carry Tester");
      await fill(page, "Handle", handle);
      await fill(page, "Email", `${handle}@sprintly.test`);
      await fill(page, "Password", "correct-horse-battery-staple");
      await page.getByRole("button", { name: /\$ git init account/ }).click();
      await expect(page).toHaveURL(/\/(me\/day)?$/);

      await page.goto("/projects");
      await page.getByRole("button", { name: /new project/i }).first().click();
      const dialog = page.getByRole("dialog");
      await dialog.getByLabel("Name").fill("Carry Over");
      await dialog.getByLabel(/^Key/).fill(key);
      await dialog.getByRole("button", { name: /\$ git init project/ }).click();
      await expect(page).toHaveURL(new RegExp(`/projects/${key}$`));
    });

    await test.step("a sprint with one unfinished task, running", async () => {
      await page.goto(`/projects/${key}/sprints`);
      await page.getByRole("button", { name: /new sprint/i }).click();
      await page.getByPlaceholder(/Sprint 23/i).fill("Sprint 7");
      await page.getByRole("button", { name: /\$ git init sprint/ }).click();
      await expect(page).toHaveURL(/\/sprints\/[0-9a-f-]+$/);

      await page.getByRole("button", { name: /add tasks/i }).click();
      const adder = page.getByLabel("add a task to this sprint");
      await adder.fill("unfinished business");
      await adder.press("Enter");
      await expect(page.getByText("unfinished business")).toBeVisible();

      await page.getByRole("button", { name: /start sprint/ }).click();
      await expect(page.getByText("active", { exact: true })).toBeVisible();
    });

    await test.step("completing asks where the leftovers go", async () => {
      await page.getByRole("button", { name: /complete \+ open retro/ }).click();
      const modal = page.getByTestId("complete-sprint-modal");
      await expect(modal).toBeVisible();
      // It counted the unfinished task.
      await expect(modal.getByText(/1.*task isn't done yet/)).toBeVisible();

      // Carry into a brand-new sprint, name prefilled by incrementing.
      await modal.getByRole("radio", { name: /move to a new sprint/ }).check();
      const nameBox = modal.getByLabel("new sprint name");
      await expect(nameBox).toHaveValue("Sprint 8");
      await modal.getByRole("button", { name: /complete sprint/ }).click();
      await expect(page).toHaveURL(/\/retro$/, { timeout: 15_000 });
    });

    await test.step("the task now lives in Sprint 8", async () => {
      await page.goto(`/projects/${key}/sprints`);
      await expect(page.getByText("Sprint 8")).toBeVisible();
      await page.getByText("Sprint 8").click();
      await expect(page.getByText("unfinished business")).toBeVisible();
      await expect(page.getByText("planned", { exact: true })).toBeVisible();
    });

    await test.step("the list filters, renames, and deletes", async () => {
      await page.goto(`/projects/${key}/sprints`);
      const search = page.getByLabel("search sprints");
      await search.fill("Sprint 8");
      await expect(page.getByText("Sprint 7")).toHaveCount(0);
      await expect(page.getByText("Sprint 8")).toBeVisible();
      await search.fill("nothing matches this");
      await expect(page.getByText(/no sprint matches/)).toBeVisible();
      await page.getByRole("button", { name: "clear sprint filter" }).click();

      // Rename + move the dates inline.
      await page.getByRole("button", { name: "edit Sprint 8" }).click();
      await page.getByLabel("Sprint 8 name").fill("Sprint 8 renamed");
      // Far enough out that it can't collide with a default. The new-sprint
      // form ends at today+14, so a nearby hardcoded date eventually *is* that
      // default — this line asserted "2026-08-30" and duly broke on 2026-08-16,
      // when two rows started showing it.
      await page.getByLabel("Sprint 8 end").fill("2031-03-03");
      await page.getByRole("button", { name: "save", exact: true }).click();
      await expect(page.getByText("Sprint 8 renamed")).toBeVisible();
      // Scoped to the row as well, so a second sprint sharing a date can't
      // reintroduce the ambiguity.
      await expect(
        page.locator("li", { hasText: "Sprint 8 renamed" }).getByText("2031-03-03"),
      ).toBeVisible();

      // Delete it — the task falls back to the backlog rather than vanishing.
      await page.getByRole("button", { name: "delete Sprint 8 renamed" }).click();
      await expect(page.getByText("Sprint 8 renamed")).toHaveCount(0);
      await page.goto(`/projects/${key}/backlog`);
      await expect(page.getByText("unfinished business")).toBeVisible();
    });
  });
});
