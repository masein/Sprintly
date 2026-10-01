// feat/roadmap-epic-tasks: clicking an epic on the roadmap opens it at the top
// of the epics section with its tasks grouped by status — QA report 6's
// example: "Phase 3 includes four tasks, three of them completed; clicking it
// opens the tasks at the top of the epic section".
//
// Pre-reqs: dev stack up (`just up`), SPRINTLY_OPEN_SIGNUP=true.

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

test.describe("roadmap — open an epic", () => {
  test("clicking Phase 3 shows its four tasks, three of them done", async ({ page }) => {
    const handle = `e2e${rand()}`;
    const key = `RE${rand().slice(0, 3).toUpperCase()}`;
    const epicName = "Phase 3 – AI Chat: Ollama + LangChain";

    await page.goto("/register");
    await fill(page, "Display name", "Roadmapper");
    await fill(page, "Handle", handle);
    await fill(page, "Email", `${handle}@sprintly.test`);
    await fill(page, "Password", "correct-horse-battery-staple");
    await page.getByRole("button", { name: /\$ git init account/ }).click();
    await expect(page).toHaveURL(/\/(me\/day)?$/);

    await page.goto("/projects");
    await page.getByRole("button", { name: /new project/i }).first().click();
    const dialog = page.getByRole("dialog");
    await dialog.getByLabel("Name").fill("Roadmaps");
    await dialog.getByLabel(/^Key/).fill(key);
    await dialog.getByRole("button", { name: /\$ git init project/ }).click();
    await expect(page).toHaveURL(new RegExp(`/projects/${key}$`));

    // An epic with dates, four tasks in it, three finished.
    const epic = await api(page, "POST", `/api/v1/projects/${key}/epics`, {
      name: epicName,
      start_date: "2026-09-01",
      end_date: "2026-10-15",
    });
    const boards = await (await page.request.get(`/api/v1/projects/${key}/boards`)).json();
    const done = boards.items[0].columns.find((c: { category: string }) => c.category === "done");
    const titles = ["pick a model", "wire LangChain", "stream replies", "rate-limit the bot"];
    for (const [i, title] of titles.entries()) {
      const t = await api(page, "POST", `/api/v1/projects/${key}/tasks`, { title });
      await api(page, "PUT", `/api/v1/tasks/${t.key}/epic`, { epic_id: epic.id });
      if (i < 3) await api(page, "POST", `/api/v1/tasks/${t.key}/move`, { column_id: done.id });
    }

    await page.goto(`/projects/${key}/timeline`);
    const bar = page.getByTestId("epic-bar").filter({ hasText: "Phase 3" });
    await expect(bar).toContainText("3/4");

    await test.step("click the bar → the epic opens with its tasks by status", async () => {
      await bar.click();
      const panel = page.getByRole("region", { name: `${epicName} tasks` });
      await expect(panel).toBeVisible();
      await expect(panel).toContainText("3/4 done · 75%");
      await expect(panel.locator('[data-epic-group="done"]')).toContainText("done (3)");
      await expect(panel.locator('[data-epic-group="todo"]')).toContainText("to do (1)");
      await expect(panel.locator('[data-epic-group="todo"]')).toContainText("rate-limit the bot");
      for (const t of titles.slice(0, 3)) {
        await expect(panel.locator('[data-epic-group="done"]')).toContainText(t);
      }
      // It sits above the epics list, in view.
      await expect(panel).toBeInViewport();
      const list = page.getByRole("heading", { name: /^epics$/i });
      expect((await panel.boundingBox())!.y).toBeLessThan((await list.boundingBox())!.y);
      // The tasks link through.
      await panel.getByRole("link", { name: `${key}-4` }).click();
      await expect(page).toHaveURL(new RegExp(`/tasks/${key}-4$`));
    });

    await test.step("the epic row's chevron opens and closes it too", async () => {
      await page.goto(`/projects/${key}/timeline`);
      const toggle = page.getByRole("button", { name: `show tasks in ${epicName}` });
      await toggle.click();
      await expect(page.getByRole("region", { name: `${epicName} tasks` })).toBeVisible();
      await toggle.click();
      await expect(page.getByRole("region", { name: `${epicName} tasks` })).toHaveCount(0);
    });
  });
});
