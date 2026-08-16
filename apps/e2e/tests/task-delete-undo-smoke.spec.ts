// fix/qa4-feedback-pass: deleting a task no longer asks "are you sure?" — it
// deletes, says so, and offers undo. QA report 4 asked for a way back from an
// accidental delete; the undo toast is that way, and it's faster than reading a
// warning nobody reads.
//
// This is the only spec that exercises POST /tasks/:key/restore through the UI.
//
// Pre-reqs: dev stack up (`just up`), SPRINTLY_OPEN_SIGNUP=true.

import { test, expect, type Page } from "@playwright/test";

function rand(): string {
  return Math.random().toString(36).slice(2, 8);
}

async function fill(page: Page, label: string, value: string) {
  await page.getByLabel(label, { exact: false }).fill(value);
}

async function signUpWithProject(page: Page, key: string) {
  const handle = `e2e${rand()}`;
  await page.goto("/register");
  await fill(page, "Display name", "Undoer");
  await fill(page, "Handle", handle);
  await fill(page, "Email", `${handle}@sprintly.test`);
  await fill(page, "Password", "correct-horse-battery-staple");
  await page.getByRole("button", { name: /\$ git init account/ }).click();
  await expect(page).toHaveURL(/\/(me\/day)?$/);

  await page.goto("/projects");
  await page.getByRole("button", { name: /new project/i }).first().click();
  const dialog = page.getByRole("dialog");
  await dialog.getByLabel("Name").fill("Undoing");
  await dialog.getByLabel(/^Key/).fill(key);
  await dialog.getByRole("button", { name: /\$ git init project/ }).click();
  await expect(page).toHaveURL(new RegExp(`/projects/${key}$`));
}

async function addBacklogTask(page: Page, key: string, title: string) {
  await page.goto(`/projects/${key}/backlog`);
  await page.locator("[data-backlog-quick-add]").click();
  const input = page.getByLabel("new task title");
  await input.fill(title);
  await input.press("Enter");
  await expect(page.getByText(title)).toBeVisible();
}

test.describe("task delete", () => {
  test("deletes without a confirm dialog, and undo brings it back", async ({ page }) => {
    const key = `UN${rand().slice(0, 3).toUpperCase()}`;
    await signUpWithProject(page, key);
    await addBacklogTask(page, key, "delete me then dont");

    // If a confirm dialog ever comes back, this fails the test rather than
    // hanging the run: Playwright auto-dismisses dialogs, which would silently
    // cancel the delete.
    const dialogs: string[] = [];
    page.on("dialog", (d) => {
      dialogs.push(d.message());
      void d.dismiss();
    });

    await page.goto(`/tasks/${key}-1`);
    await expect(page.getByRole("heading", { name: /delete me then dont/ })).toBeVisible();
    await page.getByRole("button", { name: /delete/i }).click();

    // Straight to the project, no interstitial question.
    await expect(page).toHaveURL(new RegExp(`/projects/${key}$`));
    expect(dialogs, "delete should not ask for confirmation any more").toEqual([]);

    // The toast says what happened and offers the way back.
    const toast = page.getByRole("status").filter({ hasText: `Deleted ${key}-1` });
    await expect(toast).toBeVisible();

    // It really is gone in the meantime.
    await page.goto(`/projects/${key}/backlog`);
    await expect(page.getByText("delete me then dont")).toHaveCount(0);

    // Undo has to be taken from the toast, so redo the delete to get one back.
    await page.goto(`/tasks/${key}-1`);
    // A deleted task is a 404 for reads — the undo path is the only door.
    await page.goto(`/projects/${key}/backlog`);

    await addBacklogTask(page, key, "second chance");
    await page.goto(`/tasks/${key}-2`);
    await page.getByRole("button", { name: /delete/i }).click();
    await expect(page).toHaveURL(new RegExp(`/projects/${key}$`));

    const undoToast = page.getByRole("status").filter({ hasText: `Deleted ${key}-2` });
    await expect(undoToast).toBeVisible();
    await undoToast.getByRole("button", { name: "undo" }).click();
    await expect(page.getByRole("status").filter({ hasText: `${key}-2 is back` })).toBeVisible();

    // Restored, and readable again.
    await page.goto(`/projects/${key}/backlog`);
    await expect(page.getByText("second chance")).toBeVisible();
    await page.goto(`/tasks/${key}-2`);
    await expect(page.getByRole("heading", { name: /second chance/ })).toBeVisible();
  });
});
