// feat/breadcrumbs-nav: the trail line above every page title is clickable
// (project key → board, sprints → list, task → parent), the settings page's
// mid-form "← back" is gone, "/" bounces signed-in users to My Day, and the
// shell uses the room a ≥1440px screen has.
//
// QA report 3, high priority: "Inconsistent Back button behaviour … implement
// breadcrumbs to seamlessly trace movement through tasks and subtasks",
// "make the breadcrumbs clickable in the Sprint section", "back button in
// setting is at the middle of the page", "make the breadcrumb clickable to
// click on the project key and move to the board", "add >1440px media query",
// "set the landing page to automatically redirect users to the My Day view".
//
// Pre-reqs: dev stack up (`just up`), SPRINTLY_OPEN_SIGNUP=true.

import { test, expect, type Page } from "@playwright/test";

function rand(): string {
  return Math.random().toString(36).slice(2, 8);
}

async function fill(page: Page, label: string, value: string) {
  await page.getByLabel(label, { exact: false }).fill(value);
}

test.describe("breadcrumb navigation", () => {
  test("trails are clickable across sprint, backlog, and task pages", async ({ page }) => {
    const handle = `e2e${rand()}`;
    const key = `BC${rand().slice(0, 3).toUpperCase()}`;

    await test.step("register (landing redirects to My Day once signed in)", async () => {
      await page.goto("/register");
      await fill(page, "Display name", "Crumb Tester");
      await fill(page, "Handle", handle);
      await fill(page, "Email", `${handle}@sprintly.test`);
      await fill(page, "Password", "correct-horse-battery-staple");
      await page.getByRole("button", { name: /\$ git init account/ }).click();
      await expect(page).toHaveURL(/\/(me\/day)?$/);
      // The landing page hands a signed-in user over to My Day.
      await expect(page).toHaveURL(/\/me\/day$/, { timeout: 10_000 });
      await expect(page.getByRole("navigation", { name: "breadcrumb" })).toContainText("my day");
    });

    await test.step("project + sprint + task to navigate", async () => {
      await page.goto("/projects");
      await page.getByRole("button", { name: /new project/i }).first().click();
      const dialog = page.getByRole("dialog");
      await dialog.getByLabel("Name").fill("Breadcrumbs");
      await dialog.getByLabel(/^Key/).fill(key);
      await dialog.getByRole("button", { name: /\$ git init project/ }).click();
      await expect(page).toHaveURL(new RegExp(`/projects/${key}$`));

      await page.goto(`/projects/${key}/backlog`);
      await page.locator("[data-backlog-quick-add]").click();
      const input = page.getByLabel("new task title");
      await input.fill("trace me");
      await input.press("Enter");
      await expect(page.getByText("trace me")).toBeVisible();
    });

    await test.step("backlog: the project key in the trail goes to the board", async () => {
      await page.goto(`/projects/${key}/backlog`);
      const crumbs = page.getByRole("navigation", { name: "breadcrumb" });
      await expect(crumbs).toContainText("backlog");
      await crumbs.getByRole("link", { name: key }).click();
      await expect(page).toHaveURL(new RegExp(`/projects/${key}$`));
    });

    await test.step("task detail: trail walks back to the project", async () => {
      await page.goto(`/tasks/${key}-1`);
      const crumbs = page.getByRole("navigation", { name: "breadcrumb" });
      await expect(crumbs).toContainText(`${key}-1`);
      await crumbs.getByRole("link", { name: key }).click();
      await expect(page).toHaveURL(new RegExp(`/projects/${key}$`));
    });

    await test.step("sprint page: every crumb is a link", async () => {
      await page.goto(`/projects/${key}/sprints`);
      await page.getByRole("button", { name: /new sprint/i }).click();
      await page.getByPlaceholder(/Sprint 23/i).fill("Sprint 1");
      await page.getByRole("button", { name: /\$ git init sprint/ }).click();
      await expect(page).toHaveURL(/\/sprints\/[0-9a-f-]+$/);

      const crumbs = page.getByRole("navigation", { name: "breadcrumb" });
      await expect(crumbs).toContainText("Sprint 1");
      await crumbs.getByRole("link", { name: "sprints" }).click();
      await expect(page).toHaveURL(new RegExp(`/projects/${key}/sprints$`));
    });

    await test.step("settings: no stray back button mid-form", async () => {
      await page.goto("/settings");
      await expect(page.getByRole("link", { name: "← back" })).toHaveCount(0);
      const crumbs = page.getByRole("navigation", { name: "breadcrumb" });
      await expect(crumbs).toContainText("settings");
      await crumbs.getByRole("link", { name: "sprintly" }).click();
      // Signed in, so "/" hands us to My Day again.
      await expect(page).toHaveURL(/\/me\/day$/, { timeout: 10_000 });
    });

    await test.step("a 1440px+ viewport gets a wider content column", async () => {
      await page.setViewportSize({ width: 1600, height: 900 });
      await page.goto(`/projects/${key}/backlog`);
      const main = page.locator("main").first();
      const box = (await main.boundingBox())!;
      // max-w-7xl would cap this at 1280; the wide: breakpoint lifts it.
      expect(box.width).toBeGreaterThan(1300);
      // Still no horizontal page scroll.
      const overflow = await page.evaluate(
        () => document.documentElement.scrollWidth - document.documentElement.clientWidth,
      );
      expect(overflow).toBeLessThanOrEqual(0);
    });
  });
});
