// Regression: "apply import" must reliably fire for a large Jira export — not
// silently no-op. Builds a few-hundred-row "all fields" CSV (~1 MB), previews,
// applies, and asserts the apply actually ran (a card lands on the board).
//
// Pre-reqs: dev stack up (`just up`), SPRINTLY_OPEN_SIGNUP=true.

import { test, expect, type Page } from "@playwright/test";

function rand(): string {
  return Math.random().toString(36).slice(2, 8);
}

// A Jira "Export Excel CSV (all fields)"-shaped export with `rows` issues. Each
// row carries a chunky description so the payload is ~1 MB — the size that made
// apply silently no-op in the field.
function bigJiraCsv(rows: number, marker: string): string {
  const filler = (
    "lorem ipsum dolor sit amet consectetur adipiscing elit sed do eiusmod " +
    "tempor incididunt ut labore et dolore magna aliqua "
  ).repeat(12);
  const header =
    "Issue id,Issue key,Issue Type,Summary,Description,Status,Priority\n";
  let body = "";
  for (let i = 1; i <= rows; i++) {
    const summary = i === 1 ? marker : `Imported issue ${i}`;
    body += `${1000 + i},BIG-${i},Task,${summary},"${filler}",To Do,Medium\n`;
  }
  return header + body;
}

test.describe("import — apply a large Jira file", () => {
  test("apply fires and actually imports (no silent no-op)", async ({ page }) => {
    const handle = `e2e${rand()}`;
    const key = `BIG${rand().slice(0, 3).toUpperCase()}`;
    const marker = `Cam rollout ${rand()}`;
    const csv = bigJiraCsv(600, marker);

    await test.step("register + create a project", async () => {
      await page.goto("/register");
      await fill(page, "Display name", "Import Tester");
      await fill(page, "Handle", handle);
      await fill(page, "Email", `${handle}@sprintly.test`);
      await fill(page, "Password", "correct-horse-battery-staple");
      await page.getByRole("button", { name: /\$ git init account/ }).click();
      await expect(page).toHaveURL("/");

      await page.goto("/projects");
      await page.getByRole("button", { name: /new project/i }).first().click();
      const dialog = page.getByRole("dialog");
      await dialog.getByLabel("Name").fill("Big Import");
      await dialog.getByLabel(/^Key/).fill(key);
      await dialog.getByRole("button", { name: /\$ git init project/ }).click();
      await expect(page).toHaveURL(new RegExp(`/projects/${key}$`));
    });

    await test.step("open import, upload the big file, preview", async () => {
      await page.getByRole("button", { name: /import \/ export/i }).click();
      const dialog = page.getByRole("dialog");
      await dialog.locator('input[type="file"]').setInputFiles({
        name: "jira-big.csv",
        mimeType: "text/csv",
        buffer: Buffer.from(csv),
      });
      await dialog.getByRole("button", { name: /preview \(dry-run\)/i }).click();
      await expect(dialog.getByText(/would be created/i)).toBeVisible({ timeout: 30_000 });
    });

    await test.step("apply fires a real request and reports success", async () => {
      const dialog = page.getByRole("dialog");
      const applied = page.waitForResponse(
        (r) => r.url().includes(`/projects/${key}/import`) && r.request().method() === "POST"
          && (r.request().postData() ?? "").includes('"dry_run":false'),
        { timeout: 30_000 },
      );
      await dialog.getByRole("button", { name: /apply import/i }).click();
      const res = await applied;
      expect(res.status()).toBe(200);
      await expect(dialog.getByText(/Imported \d+ task/i)).toBeVisible({ timeout: 30_000 });
    });

    await test.step("the imported card is on the board", async () => {
      await page.goto(`/projects/${key}`);
      await expect(page.getByText(marker)).toBeVisible({ timeout: 15_000 });
    });
  });
});

async function fill(page: Page, label: string, value: string) {
  await page.getByLabel(label, { exact: false }).fill(value);
}
