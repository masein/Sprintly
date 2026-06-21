// Native Jira import (extends F16): upload a small Jira "all fields" CSV through
// the import modal and assert the card lands on the board with its assignee
// (matched by email) and label.
//
// Pre-reqs: dev stack up (`just up`), SPRINTLY_OPEN_SIGNUP=true.

import { test, expect } from "@playwright/test";

function rand(): string {
  return Math.random().toString(36).slice(2, 8);
}

test.describe("Jira import", () => {
  test("a Jira CSV imports a card with its assignee and label", async ({ page }) => {
    const handle = `e2e${rand()}`;
    const email = `${handle}@sprintly.test`;
    const key = `JR${rand().slice(0, 3).toUpperCase()}`;

    await test.step("register (this user will be the matched assignee)", async () => {
      await page.goto("/register");
      await page.getByLabel("Display name", { exact: false }).fill("Jira Tester");
      await page.getByLabel("Handle", { exact: false }).fill(handle);
      await page.getByLabel("Email", { exact: false }).fill(email);
      await page.getByLabel("Password", { exact: false }).fill("correct-horse-battery-staple");
      await page.getByRole("button", { name: /\$ git init account/ }).click();
      await expect(page).toHaveURL("/");
    });

    await test.step("create a project", async () => {
      await page.goto("/projects");
      await page.getByRole("button", { name: /new project/i }).first().click();
      const dialog = page.getByRole("dialog");
      await dialog.getByLabel("Name").fill("Jira Migration");
      await dialog.getByLabel(/^Key/).fill(key);
      await dialog.getByRole("button", { name: /\$ git init project/ }).click();
      await expect(page).toHaveURL(new RegExp(`/projects/${key}$`));
    });

    // A Jira "all fields" CSV: Assignee is the importer's email → should match.
    const csv =
      "Issue key,Issue Type,Summary,Status,Priority,Assignee,Labels\n" +
      `JIRA-1,Story,Imported from Jira,In Progress,High,${email},backend\n`;

    await test.step("open import/export and upload the Jira CSV", async () => {
      await page.getByRole("button", { name: /import \/ export/i }).click();
      const dialog = page.getByRole("dialog");
      await dialog.locator('input[type="file"]').setInputFiles({
        name: "jira.csv",
        mimeType: "text/csv",
        buffer: Buffer.from(csv),
      });
      // Preview (dry-run) first, then apply.
      await dialog.getByRole("button", { name: /preview \(dry-run\)/i }).click();
      await expect(dialog.getByText(/would be created/i)).toBeVisible();
      await dialog.getByRole("button", { name: /apply import/i }).click();
      await expect(dialog.getByText(/Imported 1 task/i)).toBeVisible();
    });

    await test.step("the card is on the board with assignee + label", async () => {
      await page.goto(`/projects/${key}`);
      await expect(page.getByText("Imported from Jira")).toBeVisible();
      // Assignee matched by email → the importer's handle shows on the card.
      await expect(page.getByText(`@${handle}`).first()).toBeVisible();
      // The label chip rode along.
      await expect(page.getByText("backend").first()).toBeVisible();
    });
  });
});
