// User avatars: a generated avatar renders for an assigned user on both the
// board card and the task detail (and the @handle is always present too — the
// avatar is never the only signal).
//
// Pre-reqs: dev stack up (`just up`), SPRINTLY_OPEN_SIGNUP=true.

import { test, expect, type Page } from "@playwright/test";

function rand(): string {
  return Math.random().toString(36).slice(2, 8);
}

test.describe("user avatars", () => {
  test("a generated avatar shows on the card and on task detail", async ({ page }) => {
    const handle = `e2e${rand()}`;
    const key = `AV${rand().slice(0, 3).toUpperCase()}`;
    const name = "Avatar Tester";

    await test.step("register", async () => {
      await page.goto("/register");
      await fill(page, "Display name", name);
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
      await dialog.getByLabel("Name").fill("Avatars");
      await dialog.getByLabel(/^Key/).fill(key);
      await dialog.getByRole("button", { name: /\$ git init project/ }).click();
      await expect(page).toHaveURL(new RegExp(`/projects/${key}$`));
    });

    await test.step("add a card and assign it to the member", async () => {
      await page.locator("[data-add-card-button]").first().click();
      await page.getByPlaceholder("card title").fill("Pixel me");
      await page.getByRole("button", { name: /^add$/ }).click();
      await expect(page.getByText("Pixel me")).toBeVisible();
      await page.getByRole("link", { name: new RegExp(`${key}-\\d+`) }).first().click();
      await expect(page).toHaveURL(new RegExp(`/tasks/${key}-\\d+`));
      await page.getByLabel("assignee", { exact: true }).selectOption({ label: `@${handle}` });
    });

    await test.step("task detail shows the assignee's avatar + handle", async () => {
      const assigneeRow = page.getByLabel("assignee").locator("..");
      await expect(assigneeRow.getByRole("img", { name })).toBeVisible();
    });

    await test.step("the board card shows the avatar next to the @handle", async () => {
      await page.goto(`/projects/${key}`);
      const card = page.locator("[data-task-card]", { hasText: "Pixel me" });
      await expect(card).toBeVisible();
      // Avatar (accessible name = display name) and the @handle are both present.
      await expect(card.getByRole("img", { name })).toBeVisible();
      await expect(card.getByText(`@${handle}`)).toBeVisible();
    });
  });
});

async function fill(page: Page, label: string, value: string) {
  await page.getByLabel(label, { exact: false }).fill(value);
}
