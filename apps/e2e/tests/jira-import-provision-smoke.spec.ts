// Jira import with user provisioning: a Jira assignee with no Sprintly account
// is created on import (when "create missing users" is on), added to the
// project, and assigned the card — so an assignee avatar shows on the board.
//
// Pre-reqs: dev stack up (`just up`), SPRINTLY_OPEN_SIGNUP=true.

import { test, expect } from "@playwright/test";

function rand(): string {
  return Math.random().toString(36).slice(2, 8);
}

test.describe("Jira import — user provisioning", () => {
  test("provisions a missing assignee and shows their avatar on the card", async ({ page }) => {
    const handle = `e2e${rand()}`;
    const key = `JP${rand().slice(0, 3).toUpperCase()}`;
    // Unique assignee per run so the dev DB (shared across runs) always has to
    // *provision* them rather than matching a leftover from a prior run.
    const suffix = rand();
    const who = `Dana${suffix}`;
    const whoHandle = who.toLowerCase();

    await test.step("register + create a project", async () => {
      await page.goto("/register");
      await page.getByLabel("Display name", { exact: false }).fill("Import Owner");
      await page.getByLabel("Handle", { exact: false }).fill(handle);
      await page.getByLabel("Email", { exact: false }).fill(`${handle}@sprintly.test`);
      await page.getByLabel("Password", { exact: false }).fill("correct-horse-battery-staple");
      await page.getByRole("button", { name: /\$ git init account/ }).click();
      await expect(page).toHaveURL("/");

      await page.goto("/projects");
      await page.getByRole("button", { name: /new project/i }).first().click();
      const dialog = page.getByRole("dialog");
      await dialog.getByLabel("Name").fill("Jira Provision");
      await dialog.getByLabel(/^Key/).fill(key);
      await dialog.getByRole("button", { name: /\$ git init project/ }).click();
      await expect(page).toHaveURL(new RegExp(`/projects/${key}$`));
    });

    // The assignee is not a Sprintly user → provisioned on import.
    const csv =
      "Issue key,Issue Type,Summary,Status,Assignee\n" +
      `JIRA-1,Story,Imported with a new user,In Progress,${who}\n`;

    await test.step("import with provisioning on", async () => {
      await page.getByRole("button", { name: /import \/ export/i }).click();
      const dialog = page.getByRole("dialog");
      // Turn on "create missing users".
      await dialog.getByRole("checkbox").check();
      await dialog.locator('input[type="file"]').setInputFiles({
        name: "jira.csv",
        mimeType: "text/csv",
        buffer: Buffer.from(csv),
      });
      await dialog.getByRole("button", { name: /preview \(dry-run\)/i }).click();
      await expect(dialog.getByText(/would be created/i)).toBeVisible();
      // The preview reports a user would be created.
      await expect(dialog.getByText(/1 created/i)).toBeVisible();
      await dialog.getByRole("button", { name: /apply import/i }).click();
      await expect(dialog.getByText(/Imported 1 task/i)).toBeVisible();
    });

    await test.step("the card shows the provisioned assignee's avatar", async () => {
      await page.goto(`/projects/${key}`);
      await expect(page.getByText("Imported with a new user")).toBeVisible();
      // The provisioned handle is the slug of the display name.
      await expect(page.getByText(`@${whoHandle}`).first()).toBeVisible();
    });
  });
});
