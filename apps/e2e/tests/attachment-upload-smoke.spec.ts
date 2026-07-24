// fix/presign-host-path: attachments upload + download through the path-based
// MinIO proxy (/s3 via Caddy) — the exact prod topology. The presigner used to
// sign the SigV4 host as "host:port/s3" (path not stripped), so MinIO rejected
// every presigned PUT/GET with 403 SignatureDoesNotMatch on path-based
// deployments. Dev/CI now route the same way, so this spec is the regression.
//
// Pre-reqs: dev stack up (`just up`) with MINIO_PUBLIC_ENDPOINT routed through
// /s3 (the .env.example default), SPRINTLY_OPEN_SIGNUP=true.

import { test, expect, type Page } from "@playwright/test";

function rand(): string {
  return Math.random().toString(36).slice(2, 8);
}

async function fill(page: Page, label: string, value: string) {
  await page.getByLabel(label, { exact: false }).fill(value);
}

test.describe("attachment upload via /s3 proxy", () => {
  test("upload lands, lists, and downloads through the path-based presigned URL", async ({ page }) => {
    const handle = `e2e${rand()}`;
    const key = `AT${rand().slice(0, 3).toUpperCase()}`;

    await test.step("register + project + task", async () => {
      await page.goto("/register");
      await fill(page, "Display name", "Attachment Tester");
      await fill(page, "Handle", handle);
      await fill(page, "Email", `${handle}@sprintly.test`);
      await fill(page, "Password", "correct-horse-battery-staple");
      await page.getByRole("button", { name: /\$ git init account/ }).click();
      await expect(page).toHaveURL("/");

      await page.goto("/projects");
      await page.getByRole("button", { name: /new project/i }).first().click();
      const dialog = page.getByRole("dialog");
      await dialog.getByLabel("Name").fill("Attachments");
      await dialog.getByLabel(/^Key/).fill(key);
      await dialog.getByRole("button", { name: /\$ git init project/ }).click();
      await expect(page).toHaveURL(new RegExp(`/projects/${key}$`));

      await page.locator("[data-add-card-button]").first().click();
      await page.getByPlaceholder("card title").fill("attach things here");
      await page.getByRole("button", { name: /^add$/ }).click();
      await expect(page.getByText("attach things here")).toBeVisible();
    });

    await test.step("upload a file from the task detail", async () => {
      await page.goto(`/tasks/${key}-1`);
      await page.setInputFiles('input[type="file"]', {
        name: "notes.txt",
        mimeType: "text/plain",
        buffer: Buffer.from("presigned uploads should survive a path proxy\n"),
      });
      // The row appears once the presigned PUT + complete round-trip lands.
      await expect(page.getByText("notes.txt")).toBeVisible({ timeout: 15_000 });
      // No lingering per-file error state.
      await expect(page.getByText(/upload failed/i)).toHaveCount(0);
    });

    await test.step("the presigned download URL actually serves the bytes", async () => {
      const href = await page
        .getByRole("link", { name: "Download notes.txt" })
        .getAttribute("href");
      expect(href, "attachment row should link to a presigned URL").toBeTruthy();
      const res = await page.request.get(href!);
      expect(res.status(), "presigned GET must not 403").toBe(200);
      expect(await res.text()).toContain("survive a path proxy");
    });
  });
});
