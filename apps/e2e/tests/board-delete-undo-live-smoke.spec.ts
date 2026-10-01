// fix/delete-undo-board-sync: deleting a task from its page and landing back
// on the board must not show the deleted card — and undo must bring it back —
// without a reload and without the realtime socket.
//
// QA report 6: "deleted tasks remain visible on the board after the Undo
// action disappears, requiring a manual page refresh". The delete and restore
// calls bypassed TanStack mutations, so nothing invalidated the board's cached
// list; only a WebSocket event could fix the screen, and behind a CDN that
// doesn't carry the socket none ever arrived. This spec kills the socket on
// purpose and navigates client-side (a full reload would hide the bug by
// throwing the cache away).
//
// Pre-reqs: dev stack up (`just up`), SPRINTLY_OPEN_SIGNUP=true.

import { test, expect, type Page } from "@playwright/test";

function rand(): string {
  return Math.random().toString(36).slice(2, 8);
}

async function fill(page: Page, label: string, value: string) {
  await page.getByLabel(label, { exact: false }).fill(value);
}

test.describe("board after delete + undo", () => {
  test("the board drops the card on delete and shows it again on undo — no socket, no reload", async ({
    page,
  }) => {
    // A dead socket, like a proxy that doesn't upgrade WebSockets.
    await page.routeWebSocket(/\/ws/, (ws) => ws.close());

    const handle = `e2e${rand()}`;
    const key = `DU${rand().slice(0, 3).toUpperCase()}`;

    await page.goto("/register");
    await fill(page, "Display name", "Ghostbuster");
    await fill(page, "Handle", handle);
    await fill(page, "Email", `${handle}@sprintly.test`);
    await fill(page, "Password", "correct-horse-battery-staple");
    await page.getByRole("button", { name: /\$ git init account/ }).click();
    await expect(page).toHaveURL(/\/(me\/day)?$/);

    await page.goto("/projects");
    await page.getByRole("button", { name: /new project/i }).first().click();
    const dialog = page.getByRole("dialog");
    await dialog.getByLabel("Name").fill("Ghosts");
    await dialog.getByLabel(/^Key/).fill(key);
    await dialog.getByRole("button", { name: /\$ git init project/ }).click();
    await expect(page).toHaveURL(new RegExp(`/projects/${key}$`));

    // The add-card form stays open for rapid entry: open it once, add both.
    await page.locator("[data-add-card-button]").first().click();
    for (const [i, title] of ["keep me", "delete me"].entries()) {
      await page.getByPlaceholder("card title").fill(title);
      await page.getByRole("button", { name: /^add$/ }).click();
      await expect(page.locator(`[data-task-card="${key}-${i + 1}"]`)).toContainText(title);
    }
    const doomed = page.locator(`[data-task-card="${key}-2"]`);
    await expect(doomed).toBeVisible();

    // Client-side navigation into the task: the board's list stays cached.
    await doomed.click();
    await expect(page).toHaveURL(new RegExp(`/tasks/${key}-2$`));
    await page.getByRole("button", { name: /delete/i }).click();

    // Back on the board — the card is gone straight away.
    await expect(page).toHaveURL(new RegExp(`/projects/${key}$`));
    await expect(page.locator(`[data-task-card="${key}-1"]`)).toBeVisible();
    await expect(doomed).toHaveCount(0);

    // Undo from the toast puts it back on this same screen.
    const toast = page.getByRole("status").filter({ hasText: `Deleted ${key}-2` });
    await toast.getByRole("button", { name: "undo" }).click();
    await expect(page.getByRole("status").filter({ hasText: `${key}-2 is back` })).toBeVisible();
    await expect(doomed).toBeVisible();

    // And it stays consistent once the toast has expired.
    await page.waitForTimeout(6500);
    await expect(doomed).toBeVisible();
  });
});
