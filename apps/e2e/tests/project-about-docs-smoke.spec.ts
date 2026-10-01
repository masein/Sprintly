// feat/project-about: the project dashboard opens with what the project is —
// a description the lead writes (markdown) — and its documents: roadmaps,
// requirement briefs (QA report 6: "enable leaders to add and edit project
// descriptions as well as upload essential project documents directly within
// the dashboard").
//
// Pre-reqs: dev stack up (`just up`), SPRINTLY_OPEN_SIGNUP=true.

import { test, expect, type Page } from "@playwright/test";

function rand(): string {
  return Math.random().toString(36).slice(2, 8);
}

async function fill(page: Page, label: string, value: string) {
  await page.getByLabel(label, { exact: false }).fill(value);
}

test.describe("project description + documents", () => {
  test("the lead writes a description and uploads a roadmap; it downloads", async ({ page }) => {
    const handle = `e2e${rand()}`;
    const key = `PA${rand().slice(0, 3).toUpperCase()}`;

    await page.goto("/register");
    await fill(page, "Display name", "Describer");
    await fill(page, "Handle", handle);
    await fill(page, "Email", `${handle}@sprintly.test`);
    await fill(page, "Password", "correct-horse-battery-staple");
    await page.getByRole("button", { name: /\$ git init account/ }).click();
    await expect(page).toHaveURL(/\/(me\/day)?$/);

    await page.goto("/projects");
    await page.getByRole("button", { name: /new project/i }).first().click();
    const dialog = page.getByRole("dialog");
    await dialog.getByLabel("Name").fill("About");
    await dialog.getByLabel(/^Key/).fill(key);
    await dialog.getByRole("button", { name: /\$ git init project/ }).click();
    await expect(page).toHaveURL(new RegExp(`/projects/${key}$`));

    await page.goto(`/projects/${key}/dashboard`);
    const about = page.getByRole("region", { name: "about this project" });
    await expect(about.getByText(/No description yet/)).toBeVisible();

    await test.step("write a markdown description", async () => {
      await about.getByRole("button", { name: "edit the project description" }).click();
      await about
        .getByLabel("project description")
        .fill("Detection and live feed for **three** sites.\n\n- Parsian\n- Roshan");
      await about.getByRole("button", { name: "save" }).click();
      await expect(about.locator("strong", { hasText: "three" })).toBeVisible();
      await expect(about.getByText("Roshan")).toBeVisible();
      await page.reload();
      await expect(about.getByText("Parsian")).toBeVisible();
    });

    await test.step("upload a document, then download it", async () => {
      const docs = about.getByRole("region", { name: "project documents" });
      await docs.getByLabel("upload project documents").setInputFiles({
        name: "roadmap.txt",
        mimeType: "text/plain",
        buffer: Buffer.from("Q4: ship detection v2\n"),
      });
      const row = docs.locator('[data-project-document="roadmap.txt"]');
      await expect(row).toBeVisible({ timeout: 15_000 });
      await expect(docs).toContainText("documents (1)");
      const href = await row.getByRole("link", { name: "Download roadmap.txt" }).getAttribute("href");
      expect(href).toMatch(/^\/api\/v1\/project-documents\/[0-9a-f-]+\/download$/);
      const res = await page.request.get(href!);
      expect(res.status()).toBe(200);
      expect(await res.text()).toContain("ship detection v2");
    });

    await test.step("remove it", async () => {
      const docs = about.getByRole("region", { name: "project documents" });
      await docs.getByRole("button", { name: "remove roadmap.txt" }).click();
      await expect(docs.getByText("no documents yet")).toBeVisible();
    });
  });
});
