// feat/commit-style-task-entry: the "new task" fields take a git-style commit
// message — first line the title, the rest the description (QA report 6,
// with its exact example). Pasting and Shift+Enter both work; Enter on a
// single line still files a plain task.
//
// Pre-reqs: dev stack up (`just up`), SPRINTLY_OPEN_SIGNUP=true.

import { test, expect, type Page } from "@playwright/test";

function rand(): string {
  return Math.random().toString(36).slice(2, 8);
}

async function fill(page: Page, label: string, value: string) {
  await page.getByLabel(label, { exact: false }).fill(value);
}

const SUBJECT = "deploy the application on the server";
const BODY =
  "The application is ready for deployment, and the server is up and ready to host it.";

test.describe("commit-style task entry", () => {
  test("first line → title, the rest → description", async ({ page, context }) => {
    await context.grantPermissions(["clipboard-read", "clipboard-write"]);
    const handle = `e2e${rand()}`;
    const key = `CM${rand().slice(0, 3).toUpperCase()}`;

    await page.goto("/register");
    await fill(page, "Display name", "Committer");
    await fill(page, "Handle", handle);
    await fill(page, "Email", `${handle}@sprintly.test`);
    await fill(page, "Password", "correct-horse-battery-staple");
    await page.getByRole("button", { name: /\$ git init account/ }).click();
    await expect(page).toHaveURL(/\/(me\/day)?$/);

    await page.goto("/projects");
    await page.getByRole("button", { name: /new project/i }).first().click();
    const dialog = page.getByRole("dialog");
    await dialog.getByLabel("Name").fill("Commits");
    await dialog.getByLabel(/^Key/).fill(key);
    await dialog.getByRole("button", { name: /\$ git init project/ }).click();
    await expect(page).toHaveURL(new RegExp(`/projects/${key}$`));

    await test.step("paste the QA report's message into the board's add-card", async () => {
      await page.locator("[data-add-card-button]").first().click();
      const field = page.getByPlaceholder("card title");
      await field.focus();
      await page.evaluate((t) => navigator.clipboard.writeText(t), `${SUBJECT}\n\n${BODY}`);
      await page.keyboard.press("ControlOrMeta+V");
      // The split is previewed before anything is filed.
      await expect(page.locator("[data-commit-split]")).toContainText(`title: ${SUBJECT}`);
      await field.press("Enter");
      await expect(page.locator(`[data-task-card="${key}-1"]`)).toContainText(SUBJECT);
    });

    await test.step("the task has that title and that description", async () => {
      await page.goto(`/tasks/${key}-1`);
      await expect(page.getByRole("heading", { name: SUBJECT })).toBeVisible();
      await expect(page.getByText(BODY)).toBeVisible();
    });

    await test.step("Shift+Enter types a body in the backlog quick-add", async () => {
      await page.goto(`/projects/${key}/backlog`);
      await page.locator("[data-backlog-quick-add]").click();
      const input = page.getByLabel("new task title");
      await input.type("rotate the TLS certificate");
      await input.press("Shift+Enter");
      await input.press("Shift+Enter");
      await input.type("expires on Friday");
      await input.press("Enter");
      await expect(page.getByText("rotate the TLS certificate")).toBeVisible();
      await page.goto(`/tasks/${key}-2`);
      await expect(page.getByRole("heading", { name: "rotate the TLS certificate" })).toBeVisible();
      await expect(page.getByText("expires on Friday")).toBeVisible();
    });

    await test.step("a single line is still just a title", async () => {
      await page.goto(`/projects/${key}/backlog`);
      await page.locator("[data-backlog-quick-add]").click();
      const input = page.getByLabel("new task title");
      await input.fill("just a title");
      await expect(page.locator("[data-commit-split]")).toHaveCount(0);
      await input.press("Enter");
      await expect(page.getByText("just a title")).toBeVisible();
      await page.goto(`/tasks/${key}-3`);
      await expect(page.getByText("no description yet")).toBeVisible();
    });
  });
});
