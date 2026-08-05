// feat/board-sprint-swimlanes: the "sprint" swimlane grouping splits the board
// into an active-sprint lane and a backlog / no-sprint lane (plus other sprints
// if present), so under "all tasks" scope you can tell committed work apart
// from the rest.
//
// Pre-reqs: dev stack up (`just up`), SPRINTLY_OPEN_SIGNUP=true.

import { test, expect, type Page, type Locator } from "@playwright/test";

function rand(): string {
  return Math.random().toString(36).slice(2, 8);
}

async function fill(page: Page, label: string, value: string) {
  await page.getByLabel(label, { exact: false }).fill(value);
}

async function quickAdd(page: Page, title: string) {
  await page.locator("[data-add-card-button]").first().click();
  await page.getByPlaceholder("card title").fill(title);
  await page.getByRole("button", { name: /^add$/ }).click();
  await expect(page.getByText(title)).toBeVisible();
}

// The lane <section> whose header contains `headerText`.
function lane(page: Page, headerText: RegExp): Locator {
  return page.locator("section").filter({
    has: page.locator('[data-testid="lane-header"]', { hasText: headerText }),
  });
}

test.describe("board sprint swimlanes", () => {
  test("swimlanes=sprint renders an active-sprint lane and a backlog lane with correct membership", async ({ page }) => {
    const handle = `e2e${rand()}`;
    const key = `SW${rand().slice(0, 3).toUpperCase()}`;

    await test.step("register + project", async () => {
      await page.goto("/register");
      await fill(page, "Display name", "Swimlane Tester");
      await fill(page, "Handle", handle);
      await fill(page, "Email", `${handle}@sprintly.test`);
      await fill(page, "Password", "correct-horse-battery-staple");
      await page.getByRole("button", { name: /\$ git init account/ }).click();
      await expect(page).toHaveURL(/\/(me\/day)?$/);

      await page.goto("/projects");
      await page.getByRole("button", { name: /new project/i }).first().click();
      const dialog = page.getByRole("dialog");
      await dialog.getByLabel("Name").fill("Swimlanes");
      await dialog.getByLabel(/^Key/).fill(key);
      await dialog.getByRole("button", { name: /\$ git init project/ }).click();
      await expect(page).toHaveURL(new RegExp(`/projects/${key}$`));
    });

    await test.step("add a backlog card (no sprint yet)", async () => {
      await quickAdd(page, "Loose backlog card");
    });

    await test.step("create a sprint with a card and start it", async () => {
      await page.goto(`/projects/${key}/sprints`);
      await page.getByRole("button", { name: /new sprint/i }).click();
      await page.getByPlaceholder(/Sprint 23/i).fill("Sprint 1");
      await page.getByRole("button", { name: /\$ git init sprint/ }).click();
      await expect(page).toHaveURL(/\/sprints\/[0-9a-f-]+$/);
      await page.getByRole("button", { name: /add tasks/i }).click();
      const adder = page.getByLabel("add a task to this sprint");
      await adder.fill("Committed work");
      await adder.press("Enter");
      await expect(page.getByText("Committed work")).toBeVisible();
      await page.getByRole("button", { name: /start sprint/i }).click();
      await expect(page.getByRole("button", { name: /start sprint/i })).toHaveCount(0);
    });

    await test.step("on the board, scope to all tasks and group by sprint", async () => {
      await page.goto(`/projects/${key}`);
      // See everything, not just the active sprint, so both lanes have cards.
      await page.getByLabel("board scope").selectOption("all");
      await page.getByLabel("swimlane grouping").selectOption("sprint");
    });

    await test.step("both lanes render with the right cards", async () => {
      const activeLane = lane(page, /active/i);
      const backlogLane = lane(page, /backlog · no sprint/i);
      await expect(activeLane).toBeVisible();
      await expect(backlogLane).toBeVisible();

      // Committed work sits in the active-sprint lane, not the backlog lane.
      await expect(activeLane.getByText("Committed work")).toBeVisible();
      await expect(activeLane.getByText("Loose backlog card")).toHaveCount(0);

      // The loose card sits in the backlog lane, not the active-sprint lane.
      await expect(backlogLane.getByText("Loose backlog card")).toBeVisible();
      await expect(backlogLane.getByText("Committed work")).toHaveCount(0);
    });
  });
});
