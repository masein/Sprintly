// fix/member-role-live: membership and role changes reach open sessions over
// the WebSocket — no manual refresh. QA report 2: "Changing a member role
// requires a page refresh before taking effect on the project."
//
// Pre-reqs: dev stack up (`just up`), SPRINTLY_OPEN_SIGNUP=true.

import { test, expect, type Page } from "@playwright/test";

function rand(): string {
  return Math.random().toString(36).slice(2, 8);
}

async function fill(page: Page, label: string, value: string) {
  await page.getByLabel(label, { exact: false }).fill(value);
}

async function register(page: Page, handle: string, name: string) {
  await page.goto("/register");
  await fill(page, "Display name", name);
  await fill(page, "Handle", handle);
  await fill(page, "Email", `${handle}@sprintly.test`);
  await fill(page, "Password", "correct-horse-battery-staple");
  await page.getByRole("button", { name: /\$ git init account/ }).click();
  await expect(page).toHaveURL("/");
}

test.describe("member role changes are live", () => {
  test("a promotion shows up on the member's open project page, unprompted", async ({ page, browser }) => {
    const lead = `e2e${rand()}`;
    const member = `e2e${rand()}`;
    const key = `RL${rand().slice(0, 3).toUpperCase()}`;

    const ctxB = await browser.newContext();
    const pageB = await ctxB.newPage();

    await test.step("two users; the lead makes a project and adds the member", async () => {
      await register(pageB, member, "Future Lead");
      await register(page, lead, "Original Lead");

      await page.goto("/projects");
      await page.getByRole("button", { name: /new project/i }).first().click();
      const dialog = page.getByRole("dialog");
      await dialog.getByLabel("Name").fill("Role Live");
      await dialog.getByLabel(/^Key/).fill(key);
      await dialog.getByRole("button", { name: /\$ git init project/ }).click();
      await expect(page).toHaveURL(new RegExp(`/projects/${key}$`));

      await page.getByRole("button", { name: /members/i }).click();
      await page.getByLabel("find a user to add").fill(member);
      await page.getByRole("button", { name: new RegExp(`@${member}`) }).click();
      await expect(page.getByLabel(`role for @${member}`)).toHaveValue("contributor");
    });

    await test.step("the member parks on the project page as contributor", async () => {
      await pageB.goto(`/projects/${key}`);
      await expect(pageB.getByText("you are contributor")).toBeVisible();
    });

    await test.step("lead promotes them — the parked page catches up on its own", async () => {
      await page.getByLabel(`role for @${member}`).selectOption("lead");
      // No reload on pageB: the member_changed event refreshes it.
      await expect(pageB.getByText("you are lead")).toBeVisible({ timeout: 15_000 });
      // And the manage controls appear (rename pencil is lead-only).
      await expect(pageB.getByRole("button", { name: "Rename project" })).toBeVisible();
    });

    await ctxB.close();
  });
});
