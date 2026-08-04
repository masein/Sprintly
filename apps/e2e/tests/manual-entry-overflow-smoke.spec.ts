// fix/manual-entry-wrap: the manual time-entry row (date · h · m · billable)
// wraps inside the timer card instead of poking out of it. QA report 2:
// "Add time log appears to be partially out of the box."
//
// Pre-reqs: dev stack up (`just up`), SPRINTLY_OPEN_SIGNUP=true.

import { test, expect, type Page } from "@playwright/test";

function rand(): string {
  return Math.random().toString(36).slice(2, 8);
}

async function fill(page: Page, label: string, value: string) {
  await page.getByLabel(label, { exact: false }).fill(value);
}

test.describe("manual time entry layout", () => {
  test("the entry form stays inside the timer card", async ({ page }) => {
    const handle = `e2e${rand()}`;
    const key = `ME${rand().slice(0, 3).toUpperCase()}`;

    await page.goto("/register");
    await fill(page, "Display name", "Entry Tester");
    await fill(page, "Handle", handle);
    await fill(page, "Email", `${handle}@sprintly.test`);
    await fill(page, "Password", "correct-horse-battery-staple");
    await page.getByRole("button", { name: /\$ git init account/ }).click();
    await expect(page).toHaveURL(/\/(me\/day)?$/);

    await page.goto("/projects");
    await page.getByRole("button", { name: /new project/i }).first().click();
    const dialog = page.getByRole("dialog");
    await dialog.getByLabel("Name").fill("Entry Box");
    await dialog.getByLabel(/^Key/).fill(key);
    await dialog.getByRole("button", { name: /\$ git init project/ }).click();
    await expect(page).toHaveURL(new RegExp(`/projects/${key}$`));

    await page.goto(`/projects/${key}/backlog`);
    await page.locator("[data-backlog-quick-add]").click();
    const input = page.getByLabel("new task title");
    await input.fill("time me");
    await input.press("Enter");
    await expect(page.getByText("time me")).toBeVisible();

    await page.goto(`/tasks/${key}-1`);
    await page.getByRole("button", { name: /manual entry/i }).click();
    const form = page.getByTestId("manual-entry-form");
    await expect(form).toBeVisible();

    const formBox = (await form.boundingBox())!;
    // Every control (incl. the date input and billable checkbox) sits within
    // the form's card bounds — nothing pokes out the right side.
    for (const locator of [
      form.locator('input[type="date"]'),
      form.getByLabel("hours"),
      form.getByLabel("minutes"),
      form.getByText("billable"),
      form.getByRole("button", { name: "add log" }),
    ]) {
      const box = (await locator.boundingBox())!;
      expect(box.x + box.width).toBeLessThanOrEqual(formBox.x + formBox.width + 1);
    }
    // And the card itself stays inside the viewport.
    const viewport = page.viewportSize()!;
    expect(formBox.x + formBox.width).toBeLessThanOrEqual(viewport.width + 1);
  });
});
