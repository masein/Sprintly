// feat/sprint-report: the sprint page offers the sprint as a Word or PDF
// report — every task across all statuses, with subtasks and descriptions
// (QA report 6: "report generation in Word (.docx), PDF formats summarizing
// Sprint tasks across all statuses … along with their subtasks and
// descriptions, commits, attached files").
//
// Pre-reqs: dev stack up (`just up`), SPRINTLY_OPEN_SIGNUP=true.

import { readFile } from "node:fs/promises";
import { test, expect, type Page } from "@playwright/test";

function rand(): string {
  return Math.random().toString(36).slice(2, 8);
}

async function fill(page: Page, label: string, value: string) {
  await page.getByLabel(label, { exact: false }).fill(value);
}

async function csrf(page: Page): Promise<string> {
  const cookies = await page.context().cookies();
  return cookies.find((c) => c.name === "sprintly_csrf")?.value ?? "";
}

async function api(page: Page, method: "POST" | "PUT", path: string, body?: unknown) {
  const res = await page.request.fetch(path, {
    method,
    data: body ?? {},
    headers: { "X-CSRF-Token": await csrf(page) },
  });
  expect(res.ok(), `${method} ${path} → ${res.status()} ${await res.text()}`).toBe(true);
  return res.status() === 204 ? null : res.json();
}

test.describe("sprint report", () => {
  test("download the sprint as .docx and .pdf from its page", async ({ page }) => {
    const handle = `e2e${rand()}`;
    const key = `SP${rand().slice(0, 3).toUpperCase()}`;

    await page.goto("/register");
    await fill(page, "Display name", "Reporter");
    await fill(page, "Handle", handle);
    await fill(page, "Email", `${handle}@sprintly.test`);
    await fill(page, "Password", "correct-horse-battery-staple");
    await page.getByRole("button", { name: /\$ git init account/ }).click();
    await expect(page).toHaveURL(/\/(me\/day)?$/);

    await page.goto("/projects");
    await page.getByRole("button", { name: /new project/i }).first().click();
    const dialog = page.getByRole("dialog");
    await dialog.getByLabel("Name").fill("Reports");
    await dialog.getByLabel(/^Key/).fill(key);
    await dialog.getByRole("button", { name: /\$ git init project/ }).click();
    await expect(page).toHaveURL(new RegExp(`/projects/${key}$`));

    const sprint = await api(page, "POST", `/api/v1/projects/${key}/sprints`, {
      name: "Sprint 61",
      goal: "Improve system stability",
      starts_at: new Date().toISOString(),
      ends_at: new Date(Date.now() + 7 * 86_400_000).toISOString(),
    });
    const parent = await api(page, "POST", `/api/v1/projects/${key}/tasks`, {
      title: "delay in live feed",
      description: "Frames arrive two seconds late on the Parsian site.",
      sprint_id: sprint.id,
    });
    const child = await api(page, "POST", `/api/v1/projects/${key}/tasks`, {
      title: "profile the decoder",
    });
    await api(page, "PUT", `/api/v1/tasks/${child.key}/parent`, { parent_key: parent.key });

    await page.goto(`/sprints/${sprint.id}`);
    const report = page.locator("[data-sprint-report]");

    const [docx] = await Promise.all([
      page.waitForEvent("download"),
      report.getByRole("link", { name: "download the sprint report as .docx" }).click(),
    ]);
    expect(docx.suggestedFilename()).toBe(`${key}-sprint-61-report.docx`);
    const docBytes = await readFile((await docx.path())!);
    expect(docBytes.subarray(0, 4).toString("latin1")).toBe("PK\u0003\u0004");
    const xml = docBytes.toString("utf8");
    expect(xml).toContain("Sprint 61 — sprint report");
    expect(xml).toContain("Goal: Improve system stability");
    expect(xml).toContain("delay in live feed");
    expect(xml).toContain("Frames arrive two seconds late");
    expect(xml).toContain("profile the decoder");

    const [pdf] = await Promise.all([
      page.waitForEvent("download"),
      report.getByRole("link", { name: "download the sprint report as .pdf" }).click(),
    ]);
    expect(pdf.suggestedFilename()).toBe(`${key}-sprint-61-report.pdf`);
    const pdfText = (await readFile((await pdf.path())!)).toString("latin1");
    expect(pdfText.startsWith("%PDF-1.4")).toBe(true);
    expect(pdfText).toContain("delay in live feed");
  });
});
