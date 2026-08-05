// feat/jql-search: /search runs a query language over tasks and lets you save
// the queries you keep retyping. QA report 3: "Add JQL search and can save the
// templates."
//
// Pre-reqs: dev stack up (`just up`), SPRINTLY_OPEN_SIGNUP=true.

import { test, expect, type Page } from "@playwright/test";

function rand(): string {
  return Math.random().toString(36).slice(2, 8);
}

async function fill(page: Page, label: string, value: string) {
  await page.getByLabel(label, { exact: false }).fill(value);
}

async function runQuery(page: Page, jql: string) {
  await page.getByLabel("query", { exact: true }).fill(jql);
  await page.getByRole("button", { name: /\$ run/ }).click();
}

test.describe("query search", () => {
  test("run a query, save it, reload, re-run it, delete it", async ({ page }) => {
    const handle = `e2e${rand()}`;
    const key = `JQ${rand().slice(0, 3).toUpperCase()}`;
    const needle = `needle${rand()}`;

    await page.goto("/register");
    await fill(page, "Display name", "Querier");
    await fill(page, "Handle", handle);
    await fill(page, "Email", `${handle}@sprintly.test`);
    await fill(page, "Password", "correct-horse-battery-staple");
    await page.getByRole("button", { name: /\$ git init account/ }).click();
    await expect(page).toHaveURL(/\/(me\/day)?$/);

    await page.goto("/projects");
    await page.getByRole("button", { name: /new project/i }).first().click();
    const dialog = page.getByRole("dialog");
    await dialog.getByLabel("Name").fill("Querying");
    await dialog.getByLabel(/^Key/).fill(key);
    await dialog.getByRole("button", { name: /\$ git init project/ }).click();
    await expect(page).toHaveURL(new RegExp(`/projects/${key}$`));

    // Two tasks: one the query should find, one it shouldn't.
    await page.goto(`/projects/${key}/backlog`);
    await page.locator("[data-backlog-quick-add]").click();
    const input = page.getByLabel("new task title");
    await input.fill(`find ${needle} please`);
    await input.press("Enter");
    await expect(page.getByText(`find ${needle} please`)).toBeVisible();
    await input.fill("something else entirely");
    await input.press("Enter");
    await expect(page.getByText("something else entirely")).toBeVisible();

    await page.goto("/search");

    // An empty query is everything you can see — both tasks.
    await expect(page.getByTestId("jql-results").getByRole("row")).toHaveCount(2);

    // A real filter narrows it to one.
    await runQuery(page, `title ~ ${needle}`);
    const rows = page.getByTestId("jql-results").getByRole("row");
    await expect(rows).toHaveCount(1);
    await expect(rows.first()).toContainText(`${key}-1`);
    // The query is in the URL, so a result set is a link.
    await expect(page).toHaveURL(new RegExp(`/search\\?jql=.*${needle}`));

    // A broken query says what it didn't understand, and where.
    await runQuery(page, "banana = 3");
    await expect(page.getByTestId("jql-error")).toContainText("banana");
    await expect(page.getByTestId("jql-error")).toContainText("character");

    // currentUser() resolves server-side: nothing is assigned, so this is empty
    // — and it must not be a parse error.
    await runQuery(page, "assignee = currentUser()");
    await expect(page.getByTestId("jql-error")).toHaveCount(0);
    await expect(page.getByTestId("jql-count")).toContainText("no matches");

    // Save the useful one as a template.
    await runQuery(page, `title ~ ${needle}`);
    await page.getByRole("button", { name: /save this query/ }).click();
    const form = page.getByRole("form", { name: "save query" });
    await form.getByLabel("query name").fill("Needles");
    await form.getByRole("button", { name: /^save$/ }).click();
    await expect(page.getByRole("button", { name: "Needles", exact: true })).toBeVisible();

    // It survives a reload, and clicking it runs it again.
    await page.goto("/search");
    await expect(page.getByTestId("jql-results").getByRole("row")).toHaveCount(2);
    await page.getByRole("button", { name: "Needles", exact: true }).click();
    await expect(page.getByTestId("jql-results").getByRole("row")).toHaveCount(1);
    await expect(page.getByLabel("query", { exact: true })).toHaveValue(
      `title ~ ${needle}`,
    );

    // And it can be thrown away again.
    await page.getByRole("button", { name: "delete Needles" }).click();
    await expect(page.getByRole("button", { name: "Needles", exact: true })).toHaveCount(0);
  });
});
