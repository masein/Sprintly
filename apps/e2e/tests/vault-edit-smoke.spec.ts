// feat/vault-editing: vault items are editable (name / username / description
// / rotate the secret), password entries carry a username, and file-shaped
// kinds accept a dropped file. QA report 3: "Ability to edit vaults", "for
// password type … add username", "for env type, ability to upload (drag and
// drop) the file".
//
// Pre-reqs: dev stack up (`just up`), SPRINTLY_OPEN_SIGNUP=true.

import { test, expect, type Page } from "@playwright/test";

function rand(): string {
  return Math.random().toString(36).slice(2, 8);
}

async function fill(page: Page, label: string, value: string) {
  await page.getByLabel(label, { exact: false }).fill(value);
}

test.describe("vault editing", () => {
  test("edit metadata, keep the secret, and load an env file from disk", async ({ page }) => {
    const handle = `e2e${rand()}`;
    const key = `VE${rand().slice(0, 3).toUpperCase()}`;
    page.on("dialog", (d) => d.accept());

    await test.step("register + project", async () => {
      await page.goto("/register");
      await fill(page, "Display name", "Vault Editor");
      await fill(page, "Handle", handle);
      await fill(page, "Email", `${handle}@sprintly.test`);
      await fill(page, "Password", "correct-horse-battery-staple");
      await page.getByRole("button", { name: /\$ git init account/ }).click();
      await expect(page).toHaveURL(/\/(me\/day)?$/);

      await page.goto("/projects");
      await page.getByRole("button", { name: /new project/i }).first().click();
      const dialog = page.getByRole("dialog");
      await dialog.getByLabel("Name").fill("Vault Edit");
      await dialog.getByLabel(/^Key/).fill(key);
      await dialog.getByRole("button", { name: /\$ git init project/ }).click();
      await expect(page).toHaveURL(new RegExp(`/projects/${key}$`));
    });

    await test.step("create a password entry with a username", async () => {
      await page.goto(`/projects/${key}/vault`);
      await page.getByRole("button", { name: /^add$/i }).first().click();
      await page.getByLabel("vault item name").fill("db.prod.internal");
      await page.getByLabel("vault item username").fill("svc_reader");
      await page.getByLabel("the actual secret value").fill("hunter2-but-longer");
      await page.getByRole("button", { name: /git add secret/ }).click();

      await expect(page.getByText("db.prod.internal")).toBeVisible();
      await expect(page.getByText("svc_reader")).toBeVisible();
    });

    await test.step("edit the username and description; the secret survives", async () => {
      await page.getByRole("button", { name: "edit db.prod.internal" }).click();
      await page.getByLabel("db.prod.internal username").fill("svc_writer");
      await page.getByLabel("db.prod.internal description").fill("rotate quarterly");
      await page.getByRole("button", { name: "save", exact: true }).click();

      await expect(page.getByText("svc_writer")).toBeVisible();
      await expect(page.getByText("rotate quarterly")).toBeVisible();

      // The stored secret is untouched — reveal shows the original.
      await page.getByRole("button", { name: /reveal/ }).first().click();
      await expect(page.getByText("hunter2-but-longer")).toBeVisible();
    });

    await test.step("an env_file entry accepts a file from disk", async () => {
      await page.reload();
      await page.getByRole("button", { name: /^add$/i }).first().click();
      await page.getByLabel("vault item name").fill("staging .env");
      await page.getByRole("combobox").first().selectOption("env_file");
      await page.setInputFiles('input[aria-label="secret file"]', {
        name: ".env.staging",
        mimeType: "text/plain",
        buffer: Buffer.from("DATABASE_URL=postgres://x\nREDIS_URL=redis://y\n"),
      });
      // The textarea filled itself from the file.
      await expect(page.getByLabel("the actual secret value")).toHaveValue(
        /DATABASE_URL=postgres:\/\/x/,
      );
      await expect(page.getByText(/loaded \.env\.staging/)).toBeVisible();
      await page.getByRole("button", { name: /git add secret/ }).click();
      await expect(page.getByText("staging .env")).toBeVisible();
    });
  });
});
