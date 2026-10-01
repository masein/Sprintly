// fix/attachments-in-main-column: on the task page, attachments sit in the main
// column between the description and the subtasks — not at the bottom of the
// sidebar under the timer, links and watchers (QA report 6: "put it between
// Description and subtasks section in the task detail page").
//
// Pre-reqs: dev stack up (`just up`), SPRINTLY_OPEN_SIGNUP=true.

import { test, expect, type Page } from "@playwright/test";

function rand(): string {
  return Math.random().toString(36).slice(2, 8);
}

async function fill(page: Page, label: string, value: string) {
  await page.getByLabel(label, { exact: false }).fill(value);
}

test.describe("task page layout", () => {
  test("attachments sit between the description and the subtasks", async ({ page }) => {
    const handle = `e2e${rand()}`;
    const key = `AL${rand().slice(0, 3).toUpperCase()}`;

    await page.goto("/register");
    await fill(page, "Display name", "Layout Checker");
    await fill(page, "Handle", handle);
    await fill(page, "Email", `${handle}@sprintly.test`);
    await fill(page, "Password", "correct-horse-battery-staple");
    await page.getByRole("button", { name: /\$ git init account/ }).click();
    await expect(page).toHaveURL(/\/(me\/day)?$/);

    await page.goto("/projects");
    await page.getByRole("button", { name: /new project/i }).first().click();
    const dialog = page.getByRole("dialog");
    await dialog.getByLabel("Name").fill("Layout");
    await dialog.getByLabel(/^Key/).fill(key);
    await dialog.getByRole("button", { name: /\$ git init project/ }).click();
    await expect(page).toHaveURL(new RegExp(`/projects/${key}$`));

    await page.goto(`/projects/${key}/backlog`);
    await page.locator("[data-backlog-quick-add]").click();
    const input = page.getByLabel("new task title");
    await input.fill("where do the files go");
    await input.press("Enter");
    await expect(page.getByText("where do the files go")).toBeVisible();

    await page.goto(`/tasks/${key}-1`);
    // The description's label is a plain span, not a heading.
    const description = page.getByText("description", { exact: true }).first();
    const attachments = page.getByRole("heading", { name: /^attachments/i });
    const subtasks = page.getByRole("heading", { name: /^subtasks/i });
    await expect(attachments).toBeVisible();

    // Not in the sidebar any more.
    expect(await attachments.evaluate((el) => !!el.closest("aside"))).toBe(false);

    // Stacked in that order in the main column.
    const d = (await description.boundingBox())!;
    const a = (await attachments.boundingBox())!;
    const s = (await subtasks.boundingBox())!;
    expect(d.y, "description above attachments").toBeLessThan(a.y);
    expect(a.y, "attachments above subtasks").toBeLessThan(s.y);
    const aside = (await page.locator("aside").first().boundingBox())!;
    expect(a.x, "attachments are left of the sidebar").toBeLessThan(aside.x);

    // And they still work from there.
    await page.setInputFiles('input[type="file"]', {
      name: "roadmap.txt",
      mimeType: "text/plain",
      buffer: Buffer.from("phase 1, phase 2, phase 3\n"),
    });
    await expect(page.getByRole("link", { name: "Download roadmap.txt" })).toBeVisible({
      timeout: 15_000,
    });
  });
});
