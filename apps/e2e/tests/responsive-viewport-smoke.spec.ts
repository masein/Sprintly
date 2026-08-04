// Responsive regression suite (fix/responsive-pass): at 390x844 and 768x1024,
// no top-level page should scroll horizontally, the header's session menu
// (settings/logout) must stay reachable once it collapses below `lg`, and the
// project rename input must show the exact full name and cancel cleanly on
// Esc — the three classes of bug the manual audit found.
//
// Pre-reqs: dev stack up (`just up`), SPRINTLY_OPEN_SIGNUP=true.

import { test, expect, type Page } from "@playwright/test";

function rand(): string {
  return Math.random().toString(36).slice(2, 8);
}

// The exact string from the bug report: spaces + parens, the case that made
// the old starved rename input show only its tail ("...test)").
const PROJECT_NAME = "CCTV (Jira test)";

async function fill(page: Page, label: string, value: string) {
  await page.getByLabel(label, { exact: false }).fill(value);
}

// Register a fresh user, create a project named PROJECT_NAME, add one card
// (lands in the backlog — no sprint), and return { key, taskUrl }.
async function setup(page: Page): Promise<{ key: string; taskUrl: string }> {
  const handle = `e2e${rand()}`;
  const key = `RV${rand().slice(0, 3).toUpperCase()}`;

  await page.goto("/register");
  await fill(page, "Display name", "Responsive Tester");
  await fill(page, "Handle", handle);
  await fill(page, "Email", `${handle}@sprintly.test`);
  await fill(page, "Password", "correct-horse-battery-staple");
  await page.getByRole("button", { name: /\$ git init account/ }).click();
  await expect(page).toHaveURL(/\/(me\/day)?$/);

  await page.goto("/projects");
  await page.getByRole("button", { name: /new project/i }).first().click();
  const dialog = page.getByRole("dialog");
  await dialog.getByLabel("Name").fill(PROJECT_NAME);
  await dialog.getByLabel(/^Key/).fill(key);
  await dialog.getByRole("button", { name: /\$ git init project/ }).click();
  await expect(page).toHaveURL(new RegExp(`/projects/${key}$`));

  await page.locator("[data-add-card-button]").first().click();
  await page.getByPlaceholder("card title").fill("a reasonably normal task title");
  await page.getByRole("button", { name: /^add$/ }).click();
  await expect(page.getByText("a reasonably normal task title")).toBeVisible();

  return { key, taskUrl: `/tasks/${key}-1` };
}

async function assertNoHorizontalOverflow(page: Page, label: string) {
  // Wait for the shell to mount before measuring — a pre-hydration read would
  // pass trivially and mask real overflow once content settles.
  // The wordmark, not the breadcrumb root — both link to "/" and both contain
  // "sprintly", so an inexact name matches two elements.
  await expect(page.getByRole("link", { name: "sprintly/", exact: true })).toBeVisible();
  const { scrollWidth, innerWidth } = await page.evaluate(() => ({
    scrollWidth: document.scrollingElement!.scrollWidth,
    innerWidth: window.innerWidth,
  }));
  expect(scrollWidth, `${label}: document.scrollingElement.scrollWidth (${scrollWidth}) > window.innerWidth (${innerWidth})`).toBeLessThanOrEqual(innerWidth);
}

const VIEWPORTS = [
  { name: "390x844", width: 390, height: 844 },
  { name: "768x1024", width: 768, height: 1024 },
];

for (const vp of VIEWPORTS) {
  test.describe(`responsive pass @ ${vp.name}`, () => {
    test.use({ viewport: { width: vp.width, height: vp.height } });

    test(`no page-level horizontal overflow on core pages`, async ({ page }) => {
      const { key, taskUrl } = await setup(page);

      await assertNoHorizontalOverflow(page, "board");

      await page.goto("/projects");
      await assertNoHorizontalOverflow(page, "projects list");

      await page.goto(`/projects/${key}/backlog`);
      await assertNoHorizontalOverflow(page, "backlog");

      await page.goto(`/projects/${key}/sprints`);
      await assertNoHorizontalOverflow(page, "sprints");

      await page.goto(`/projects/${key}/dashboard`);
      await assertNoHorizontalOverflow(page, "dashboard");

      await page.goto(taskUrl);
      await assertNoHorizontalOverflow(page, "task detail");

      await page.goto("/me/day");
      await assertNoHorizontalOverflow(page, "my day");
    });

    test("settings and logout stay reachable from the collapsed header menu", async ({ page }) => {
      await setup(page);

      const menuButton = page.getByRole("button", { name: /open menu/i });
      await expect(menuButton).toBeVisible();
      await menuButton.click();

      // Scope to the open dropdown — the always-mounted desktop SessionBadge
      // (CSS-hidden below `lg`, not unmounted) would otherwise double-match.
      const menu = page.getByRole("menu");
      const settingsLink = menu.getByRole("link", { name: /^settings$/i });
      const logoutButton = menu.getByRole("button", { name: /^logout$/i });
      await expect(settingsLink).toBeVisible();
      await expect(logoutButton).toBeVisible();

      await settingsLink.click();
      await expect(page).toHaveURL("/settings");
    });

    test("rename input pre-fills the exact full name; Esc cancels without saving", async ({ page }) => {
      await setup(page);

      await page.getByRole("button", { name: /rename project/i }).click();
      // The InlineName form is the only textbox on the page while editing.
      const renameInput = page.getByRole("textbox").first();
      await expect(renameInput).toHaveValue(PROJECT_NAME);

      await renameInput.fill("SHOULD NOT SAVE");
      await renameInput.press("Escape");

      // Reverted locally — no save round-trip, original name still showing.
      await expect(page.getByRole("heading", { name: PROJECT_NAME })).toBeVisible();
      await expect(page.getByText("SHOULD NOT SAVE")).toHaveCount(0);

      // Survives a reload too (proves it was never persisted).
      await page.reload();
      await expect(page.getByRole("heading", { name: PROJECT_NAME })).toBeVisible();
    });
  });
}
