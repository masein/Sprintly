// feat/retro-summary-editing: the generated retro summary is a draft, not
// scripture — leads can rework it after closing. QA report: "Allow editing
// the generated retrospective summary so it can be refined before finalizing."
//
// Pre-reqs: dev stack up (`just up`), SPRINTLY_OPEN_SIGNUP=true.

import { test, expect, type Page } from "@playwright/test";

function rand(): string {
  return Math.random().toString(36).slice(2, 8);
}

async function fill(page: Page, label: string, value: string) {
  await page.getByLabel(label, { exact: false }).fill(value);
}

test.describe("retro summary editing", () => {
  test("lead refines the generated summary after closing", async ({ page }) => {
    const handle = `e2e${rand()}`;
    const key = `RS${rand().slice(0, 3).toUpperCase()}`;
    page.on("dialog", (d) => d.accept());

    await test.step("register + project + a closed retro", async () => {
      await page.goto("/register");
      await fill(page, "Display name", "Summary Editor");
      await fill(page, "Handle", handle);
      await fill(page, "Email", `${handle}@sprintly.test`);
      await fill(page, "Password", "correct-horse-battery-staple");
      await page.getByRole("button", { name: /\$ git init account/ }).click();
      await expect(page).toHaveURL("/");

      await page.goto("/projects");
      await page.getByRole("button", { name: /new project/i }).first().click();
      const dialog = page.getByRole("dialog");
      await dialog.getByLabel("Name").fill("Retro Summary");
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
      // Completing now asks where unfinished work goes before it commits — with
      // nothing left over the default (leave it here) is fine, but the modal
      // still has to be confirmed.
      const completeModal = page.getByTestId("complete-sprint-modal");
      await expect(completeModal).toBeVisible();
      await completeModal.getByRole("button", { name: /complete sprint/ }).click();
      await expect(page).toHaveURL(/\/retro$/, { timeout: 15_000 });

      await page.getByPlaceholder("add to went well…").fill("we shipped");
      await page.getByRole("button", { name: "add", exact: true }).first().click();
      await expect(page.getByText("we shipped")).toBeVisible();
      await page.getByRole("button", { name: /close \+ write summary/ }).click();
      await expect(page.getByText("summary · markdown")).toBeVisible();
    });

    await test.step("edit the summary in place", async () => {
      await page.getByRole("button", { name: "edit", exact: true }).click();
      const editor = page.getByLabel("edit summary");
      // The generated draft is prefilled — replace it wholesale.
      await expect(editor).toHaveValue(/we shipped/);
      await editor.fill("# Sprint 1 — the director's cut\n\nWe shipped, twice.");
      await page.getByRole("button", { name: "save", exact: true }).click();
      await expect(
        page.getByRole("heading", { name: "Sprint 1 — the director's cut" }),
      ).toBeVisible();
      await expect(page.getByText("We shipped, twice.")).toBeVisible();
    });

    await test.step("the rework survives a reload", async () => {
      await page.reload();
      await expect(
        page.getByRole("heading", { name: "Sprint 1 — the director's cut" }),
      ).toBeVisible();
    });
  });
});
