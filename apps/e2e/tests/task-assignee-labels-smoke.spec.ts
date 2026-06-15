// QA F2/F3 fix: assignee + labels controls on the task detail.
//
// Register → create project → create a label → add a card → open the task →
// set an assignee and add the label → reload → both persist → and they render
// on the board card.
//
// Pre-reqs: dev stack up (`just up`), SPRINTLY_OPEN_SIGNUP=true.

import { test, expect, type Page } from "@playwright/test";

function rand(): string {
  return Math.random().toString(36).slice(2, 8);
}

test.describe("QA F2/F3 — task assignee + labels", () => {
  test("assign a member and apply a label; both persist and show on the card", async ({ page }) => {
    const handle = `e2e${rand()}`;
    const key = `AL${rand().slice(0, 3).toUpperCase()}`;

    await test.step("register", async () => {
      await page.goto("/register");
      await fill(page, "Display name", "Assignee Tester");
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
      await dialog.getByLabel("Name").fill("AssignLabels");
      await dialog.getByLabel(/^Key/).fill(key);
      await dialog.getByRole("button", { name: /\$ git init project/ }).click();
      await expect(page).toHaveURL(new RegExp(`/projects/${key}$`));
    });

    await test.step("create a project label", async () => {
      await page.getByRole("button", { name: /^labels$/i }).first().click();
      const dialog = page.getByRole("dialog");
      await dialog.getByPlaceholder("new label").fill("backend");
      await dialog.getByRole("button", { name: /^add$/ }).click();
      await expect(dialog.getByText("backend")).toBeVisible();
      await dialog.getByRole("button", { name: /close/i }).click();
    });

    await test.step("add a card and open the task", async () => {
      await page.locator("[data-add-card-button]").first().click();
      await page.getByPlaceholder("card title").fill("Hand me off");
      await page.getByRole("button", { name: /^add$/ }).click();
      await expect(page.getByText("Hand me off")).toBeVisible();
      await page.getByRole("link", { name: new RegExp(`${key}-\\d+`) }).first().click();
      await expect(page).toHaveURL(new RegExp(`/tasks/${key}-\\d+`));
    });

    await test.step("assign to the member and add the label", async () => {
      await page.getByLabel("assignee").selectOption({ label: `@${handle}` });
      await page.getByLabel("add label").selectOption({ label: "backend" });
      // The label chip shows on the task immediately.
      await expect(page.getByLabel(/remove backend/i)).toBeVisible();
    });

    await test.step("reload: both persist", async () => {
      await page.reload();
      // Assignee is no longer "unassigned".
      await expect(page.getByLabel("assignee")).not.toHaveValue("");
      await expect(page.getByLabel(/remove backend/i)).toBeVisible();
    });

    await test.step("the board card reflects assignee + label", async () => {
      await page.goto(`/projects/${key}`);
      await expect(page.getByText("Hand me off")).toBeVisible();
      await expect(page.getByText(`@${handle}`).first()).toBeVisible();
      await expect(page.getByText("backend").first()).toBeVisible();
    });
  });
});

async function fill(page: Page, label: string, value: string) {
  await page.getByLabel(label, { exact: false }).fill(value);
}
