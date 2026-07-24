// feat/mentions: @mention autocomplete + highlight.
//
// Register → create project + task → in the comment box, type `@` and part
// of a handle → the member dropdown appears → pick → the handle is inserted
// → submit → the rendered comment highlights the mention (and leaves emails
// and code spans alone). Then the same autocomplete in the description
// editor. Cross-user notification fan-out is covered by the API's
// integration/unit tests (you can't be notified about mentioning yourself).
//
// Pre-reqs: dev stack up (`just up`), SPRINTLY_OPEN_SIGNUP=true.

import { test, expect, type Page } from "@playwright/test";

function rand(): string {
  return Math.random().toString(36).slice(2, 8);
}

async function fill(page: Page, label: string, value: string) {
  await page.getByLabel(label, { exact: false }).fill(value);
}

test.describe("@mentions", () => {
  test("autocomplete a member in a comment and see it highlighted", async ({ page }) => {
    const handle = `e2e${rand()}`;
    const key = `MN${rand().slice(0, 3).toUpperCase()}`;

    await test.step("register + project + a task", async () => {
      await page.goto("/register");
      await fill(page, "Display name", "Mention Tester");
      await fill(page, "Handle", handle);
      await fill(page, "Email", `${handle}@sprintly.test`);
      await fill(page, "Password", "correct-horse-battery-staple");
      await page.getByRole("button", { name: /\$ git init account/ }).click();
      await expect(page).toHaveURL("/");

      await page.goto("/projects");
      await page.getByRole("button", { name: /new project/i }).first().click();
      const dialog = page.getByRole("dialog");
      await dialog.getByLabel("Name").fill("Mentions");
      await dialog.getByLabel(/^Key/).fill(key);
      await dialog.getByRole("button", { name: /\$ git init project/ }).click();
      await expect(page).toHaveURL(new RegExp(`/projects/${key}$`));

      await page.locator("[data-add-card-button]").first().click();
      await page.getByPlaceholder("card title").fill("Ping me maybe");
      await page.getByRole("button", { name: /^add$/ }).click();
      await expect(page.getByText("Ping me maybe")).toBeVisible();
      await page.getByRole("link", { name: new RegExp(`${key}-\\d+`) }).first().click();
      await expect(page).toHaveURL(new RegExp(`/tasks/${key}-\\d+`));
    });

    const commentBox = page.getByPlaceholder(/leave a comment/);

    await test.step("typing @ suggests project members", async () => {
      await commentBox.click();
      await commentBox.pressSequentially(`heads up @${handle.slice(0, 4)}`);
      const listbox = page.getByRole("listbox", { name: "mention a member" });
      await expect(listbox).toBeVisible();
      await listbox.getByRole("button").filter({ hasText: `@${handle}` }).click();
      await expect(commentBox).toHaveValue(`heads up @${handle} `);
    });

    await test.step("Enter picks the highlighted suggestion too", async () => {
      await commentBox.pressSequentially(`and mail me@example.com or \`@${handle}\` and @${handle.slice(0, 4)}`);
      await expect(page.getByRole("listbox", { name: "mention a member" })).toBeVisible();
      await commentBox.press("Enter");
      await expect(commentBox).toHaveValue(
        `heads up @${handle} and mail me@example.com or \`@${handle}\` and @${handle} `,
      );
      // Enter consumed by the picker — still a one-line comment box, no submit.
      await expect(page.getByText("no comments yet — be the first")).toBeVisible();
    });

    await test.step("submitted comment highlights mentions, not emails or code", async () => {
      await page.getByRole("button", { name: /\$ commit/ }).click();
      const comment = page.locator("article");
      // Two real mentions highlighted…
      await expect(comment.locator(`[data-mention="${handle}"]`)).toHaveCount(2);
      // …the email's @-part and the code span stay plain.
      await expect(comment.locator('[data-mention="example"]')).toHaveCount(0);
      await expect(comment.locator("code").filter({ hasText: `@${handle}` })).toBeVisible();
    });

    await test.step("description editor autocompletes too", async () => {
      // The description's edit affordance is lowercase "edit"; the title's is
      // "Rename" and a comment's is "Edit", so anchor case-sensitively.
      await page.getByRole("button", { name: /^edit$/ }).click();
      const desc = page.getByPlaceholder(/markdown — backticks/);
      await desc.click();
      await desc.pressSequentially(`cc @${handle.slice(0, 4)}`);
      const listbox = page.getByRole("listbox", { name: "mention a member" });
      await expect(listbox).toBeVisible();
      await listbox.getByRole("button").filter({ hasText: `@${handle}` }).click();
      await expect(desc).toHaveValue(`cc @${handle} `);
      await page.getByRole("button", { name: /:wq/ }).click();
      await expect(page.locator(`section [data-mention="${handle}"]`).first()).toBeVisible();
    });
  });
});
