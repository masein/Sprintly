// F17 smoke: at a 375px phone viewport the board renders, a card can be added,
// and the task detail opens — plus the PWA manifest is served.
//
// Pre-reqs: dev stack up (`just up`), SPRINTLY_OPEN_SIGNUP=true.

import { test, expect, type Page } from "@playwright/test";

function rand(): string {
  return Math.random().toString(36).slice(2, 8);
}

test.use({ viewport: { width: 375, height: 812 } });

test.describe("F17 mobile smoke", () => {
  test("board + task detail work at 375px; manifest is served", async ({ page }) => {
    const handle = `e2e${rand()}`;
    const key = `MOB${rand().slice(0, 2).toUpperCase()}`;

    await test.step("register", async () => {
      await page.goto("/register");
      await fill(page, "Display name", "Mobile Tester");
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
      await dialog.getByLabel("Name").fill("Mobile");
      await dialog.getByLabel(/^Key/).fill(key);
      await dialog.getByRole("button", { name: /\$ git init project/ }).click();
      await expect(page).toHaveURL(new RegExp(`/projects/${key}$`));
    });

    await test.step("board renders and a card can be added on a phone", async () => {
      // The board's columns render (horizontally scrollable at this width).
      await page.locator("[data-add-card-button]").first().click();
      await page.getByPlaceholder("card title").fill("phone task");
      await page.getByRole("button", { name: /^add$/ }).click();
      await expect(page.getByText("phone task")).toBeVisible();
    });

    await test.step("the task opens at phone width", async () => {
      // The task-key link on the card navigates (the card body is the drag handle).
      await page.getByRole("link", { name: new RegExp(`${key}-\\d+`) }).first().click();
      await expect(page).toHaveURL(new RegExp(`/tasks/${key}-\\d+`));
      await expect(page.getByRole("heading", { name: "phone task" })).toBeVisible();
    });

    await test.step("the PWA manifest is served", async () => {
      const res = await page.request.get("/manifest.webmanifest");
      expect(res.ok()).toBeTruthy();
      const m = await res.json();
      expect(m.name).toBe("Sprintly");
      expect(m.display).toBe("standalone");
      expect(Array.isArray(m.icons) && m.icons.length > 0).toBeTruthy();
    });
  });
});

async function fill(page: Page, label: string, value: string) {
  await page.getByLabel(label, { exact: false }).fill(value);
}
