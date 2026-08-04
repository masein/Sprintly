// feat/retro-note-editing: retro entries can be edited after creation, by
// their author, while the retro is open. QA report: "Allow editing
// retrospective entries after creation."
//
// Pre-reqs: dev stack up (`just up`), SPRINTLY_OPEN_SIGNUP=true.

import { test, expect, type Page } from "@playwright/test";

function rand(): string {
  return Math.random().toString(36).slice(2, 8);
}

async function fill(page: Page, label: string, value: string) {
  await page.getByLabel(label, { exact: false }).fill(value);
}

test.describe("retro note editing", () => {
  test("author edits their note; the edit marker shows", async ({ page }) => {
    const handle = `e2e${rand()}`;
    const key = `RE${rand().slice(0, 3).toUpperCase()}`;
    page.on("dialog", (d) => d.accept());

    await test.step("register + project + a completed sprint (opens the retro)", async () => {
      await page.goto("/register");
      await fill(page, "Display name", "Retro Editor");
      await fill(page, "Handle", handle);
      await fill(page, "Email", `${handle}@sprintly.test`);
      await fill(page, "Password", "correct-horse-battery-staple");
      await page.getByRole("button", { name: /\$ git init account/ }).click();
      await expect(page).toHaveURL(/\/(me\/day)?$/);

      await page.goto("/projects");
      await page.getByRole("button", { name: /new project/i }).first().click();
      const dialog = page.getByRole("dialog");
      await dialog.getByLabel("Name").fill("Retro Edit");
      await dialog.getByLabel(/^Key/).fill(key);
      await dialog.getByRole("button", { name: /\$ git init project/ }).click();
      await expect(page).toHaveURL(new RegExp(`/projects/${key}$`));

      await page.goto(`/projects/${key}/sprints`);
      await page.getByRole("button", { name: /new sprint/i }).click();
      await page.getByPlaceholder(/Sprint 23/i).fill("Sprint 1");
      await page.getByRole("button", { name: /\$ git init sprint/ }).click();
      await expect(page).toHaveURL(/\/sprints\/[0-9a-f-]+$/);

      await page.getByRole("button", { name: /start sprint/ }).click();
      await page.getByRole("button", { name: /complete \+ open retro/ }).click();
      await expect(page).toHaveURL(/\/retro$/);
    });

    await test.step("drop a note", async () => {
      await page.getByPlaceholder("add to went well…").fill("shipped the thing");
      await page.getByRole("button", { name: "add", exact: true }).first().click();
      await expect(page.getByText("shipped the thing")).toBeVisible();
      await expect(page.getByText("· edited")).toHaveCount(0);
    });

    await test.step("edit it in place", async () => {
      await page.getByRole("button", { name: "Edit note" }).click();
      const editor = page.getByLabel("edit note");
      await editor.fill("shipped the thing, twice");
      await page.getByRole("button", { name: "save", exact: true }).click();
      await expect(page.getByText("shipped the thing, twice")).toBeVisible();
      await expect(page.getByText("edited")).toBeVisible();
    });

    await test.step("closing the retro hides the edit affordance", async () => {
      await page.getByRole("button", { name: /close \+ write summary/ }).click();
      await expect(page.getByText("Locked. The markdown summary below is what gets shared.")).toBeVisible();
      await expect(page.getByRole("button", { name: "Edit note" })).toHaveCount(0);
      // The summary snapshotted the edited text.
      await expect(page.getByText(/shipped the thing, twice/).first()).toBeVisible();
    });
  });
});
