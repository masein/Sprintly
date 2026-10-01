// fix/git-activity-overflow: a long commit or PR title in the task page's git
// activity panel truncates (full title on hover) instead of running out of the
// sidebar and giving the whole page a horizontal scrollbar — spotted while
// checking the QA report 6 task-page layout.
//
// Pre-reqs: dev stack up (`just up`), SPRINTLY_OPEN_SIGNUP=true.

import { test, expect, type Page } from "@playwright/test";

function rand(): string {
  return Math.random().toString(36).slice(2, 8);
}

async function fill(page: Page, label: string, value: string) {
  await page.getByLabel(label, { exact: false }).fill(value);
}

const LONG =
  "gateway: shrink the websocket send buffer so frames stop queueing behind the slowest viewer on the Parsian site";

test.describe("git activity panel", () => {
  test("a long commit title truncates instead of widening the page", async ({ page }) => {
    const handle = `e2e${rand()}`;
    const key = `GA${rand().slice(0, 3).toUpperCase()}`;

    await page.goto("/register");
    await fill(page, "Display name", "Committer");
    await fill(page, "Handle", handle);
    await fill(page, "Email", `${handle}@sprintly.test`);
    await fill(page, "Password", "correct-horse-battery-staple");
    await page.getByRole("button", { name: /\$ git init account/ }).click();
    await expect(page).toHaveURL(/\/(me\/day)?$/);

    await page.goto("/projects");
    await page.getByRole("button", { name: /new project/i }).first().click();
    const dialog = page.getByRole("dialog");
    await dialog.getByLabel("Name").fill("Git");
    await dialog.getByLabel(/^Key/).fill(key);
    await dialog.getByRole("button", { name: /\$ git init project/ }).click();
    await expect(page).toHaveURL(new RegExp(`/projects/${key}$`));

    await page.goto(`/projects/${key}/backlog`);
    await page.locator("[data-backlog-quick-add]").click();
    await page.getByLabel("new task title").fill("needs a commit");
    await page.getByLabel("new task title").press("Enter");
    await expect(page.locator("li", { hasText: "needs a commit" }).first()).toBeVisible();

    // Git links arrive from provider webhooks; stub the read so the panel has
    // a long title to lay out.
    await page.route(`**/api/v1/tasks/${key}-1/git-links`, (route) =>
      route.fulfill({
        json: [
          {
            id: "00000000-0000-7000-8000-000000000001",
            kind: "commit",
            provider: "github",
            external_ref: "9f3c2ab71d",
            url: null,
            title: LONG,
            state: null,
            check_state: null,
            created_at: "2026-10-01T09:00:00Z",
          },
        ],
      }),
    );
    await page.goto(`/tasks/${key}-1`);
    const title = page.getByText(LONG);
    await expect(title).toBeVisible();
    await expect(title).toHaveAttribute("title", LONG);

    // No horizontal page scroll, and the title stays inside the sidebar.
    const overflow = await page.evaluate(
      () => document.documentElement.scrollWidth - document.documentElement.clientWidth,
    );
    expect(overflow).toBeLessThanOrEqual(0);
    const aside = (await page.locator("aside").first().boundingBox())!;
    const box = (await title.boundingBox())!;
    expect(box.x + box.width).toBeLessThanOrEqual(aside.x + aside.width + 1);
  });
});
