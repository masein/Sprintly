// fix/contributor-task-permissions: a contributor can do the task work the UI
// offers — add and move cards, rename a task, comment, plan it into a sprint —
// instead of meeting a 403 behind every control (the API used to allow only
// leads). A watcher-role member sees the work but none of the controls.
//
// Pre-reqs: dev stack up (`just up`), SPRINTLY_OPEN_SIGNUP=true.

import { test, expect, type Browser, type Page } from "@playwright/test";

function rand(): string {
  return Math.random().toString(36).slice(2, 8);
}

async function fill(page: Page, label: string, value: string) {
  await page.getByLabel(label, { exact: false }).fill(value);
}

async function register(browser: Browser, display: string, handle: string) {
  const ctx = await browser.newContext();
  const page = await ctx.newPage();
  await page.goto("/register");
  await fill(page, "Display name", display);
  await fill(page, "Handle", handle);
  await fill(page, "Email", `${handle}@sprintly.test`);
  await fill(page, "Password", "correct-horse-battery-staple");
  await page.getByRole("button", { name: /\$ git init account/ }).click();
  await expect(page).toHaveURL(/\/(me\/day)?$/);
  const me = await (await page.request.get("/api/v1/users/me")).json();
  return { page, ctx, me };
}

async function csrf(page: Page): Promise<string> {
  const cookies = await page.context().cookies();
  return cookies.find((c) => c.name === "sprintly_csrf")?.value ?? "";
}

async function api(page: Page, method: "POST", path: string, body?: unknown) {
  const res = await page.request.fetch(path, {
    method,
    data: body ?? {},
    headers: { "X-CSRF-Token": await csrf(page) },
  });
  expect(res.ok(), `${method} ${path} → ${res.status()} ${await res.text()}`).toBe(true);
  return res.status() === 204 ? null : res.json();
}

test.describe("contributors work on tasks", () => {
  test("a contributor adds, moves, edits and plans; a watcher only reads", async ({ browser }) => {
    const s = rand();
    const key = `CW${rand().slice(0, 3).toUpperCase()}`;
    const lead = await register(browser, "Lead", `lead${s}`);
    const contrib = await register(browser, "Contributor", `contrib${s}`);
    const reader = await register(browser, "Reader", `reader${s}`);

    // The lead sets up: project, team, a sprint.
    await api(lead.page, "POST", "/api/v1/projects", { key, name: "Team work" });
    await api(lead.page, "POST", `/api/v1/projects/${key}/members`, { user_id: contrib.me.id, role: "contributor" });
    await api(lead.page, "POST", `/api/v1/projects/${key}/members`, { user_id: reader.me.id, role: "watcher" });
    const sprint = await api(lead.page, "POST", `/api/v1/projects/${key}/sprints`, {
      name: "Sprint 1",
      starts_at: new Date().toISOString(),
      ends_at: new Date(Date.now() + 7 * 86_400_000).toISOString(),
    });

    const page = contrib.page;
    await test.step("contributor: add a card on the board and drag it to In progress", async () => {
      await page.goto(`/projects/${key}`);
      await page.locator("[data-add-card-button]").first().click();
      await page.getByPlaceholder("card title").fill("contributor's card");
      await page.getByRole("button", { name: /^add$/ }).click();
      const card = page.locator(`[data-task-card="${key}-1"]`);
      await expect(card).toContainText("contributor's card");

      // Drag it by the card itself into the "In progress" column.
      const column = page
        .locator("div.flex-shrink-0")
        .filter({ has: page.getByRole("button", { name: "In progress", exact: true }) });
      const cb = (await card.boundingBox())!;
      const tb = (await column.boundingBox())!;
      await page.mouse.move(cb.x + cb.width / 2, cb.y + cb.height / 2);
      await page.mouse.down();
      await page.waitForTimeout(60);
      await page.mouse.move(cb.x + cb.width / 2 + 10, cb.y + cb.height / 2 + 10, { steps: 5 });
      await page.waitForTimeout(60);
      await page.mouse.move(tb.x + tb.width / 2, tb.y + 72, { steps: 20 });
      await page.waitForTimeout(60);
      await page.mouse.move(tb.x + tb.width / 2, tb.y + 74, { steps: 3 });
      await page.waitForTimeout(60);
      await page.mouse.up();
      await expect
        .poll(async () => (await (await page.request.get(`/api/v1/tasks/${key}-1`)).json()).status)
        .toBe("in_progress");
    });

    await test.step("contributor: rename it and comment", async () => {
      await page.goto(`/tasks/${key}-1`);
      await page.getByRole("button", { name: "Rename" }).click();
      const title = page.locator("input:focus");
      await title.fill("renamed by a contributor");
      await title.press("Enter");
      await expect(page.getByRole("heading", { name: "renamed by a contributor" })).toBeVisible();
      await page.getByPlaceholder(/leave a comment/i).fill("picking this up");
      await page.getByRole("button", { name: /\$ commit/ }).click();
      await expect(page.getByText("picking this up")).toBeVisible();
    });

    await test.step("contributor: add work to the sprint, but can't start it", async () => {
      await page.goto(`/sprints/${sprint.id}`);
      const adder = page.getByRole("button", { name: /add tasks/i });
      await adder.click();
      await page.getByLabel("add a task to this sprint").fill("planned by a contributor");
      await page.getByLabel("add a task to this sprint").press("Enter");
      await expect(page.getByTestId("sprint-drop").getByText("planned by a contributor")).toBeVisible();
      await expect(page.getByRole("button", { name: /start sprint/i })).toHaveCount(0);
    });

    await test.step("watcher-role member: reads everything, gets no controls", async () => {
      const r = reader.page;
      await r.goto(`/projects/${key}`);
      await expect(r.locator(`[data-task-card="${key}-1"]`)).toBeVisible();
      await expect(r.locator("[data-add-card-button]")).toHaveCount(0);
      await r.goto(`/tasks/${key}-1`);
      await expect(r.getByRole("heading", { name: "renamed by a contributor" })).toBeVisible();
      await expect(r.getByRole("button", { name: "Rename" })).toHaveCount(0);
      await r.goto(`/sprints/${sprint.id}`);
      await expect(r.getByTestId("sprint-drop").getByText("planned by a contributor")).toBeVisible();
      await expect(r.getByRole("button", { name: /add tasks/i })).toHaveCount(0);
      await expect(r.getByRole("button", { name: "Remove from sprint" })).toHaveCount(0);
    });

    for (const u of [lead, contrib, reader]) await u.ctx.close();
  });
});
