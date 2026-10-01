// feat/manage-watchers: on a task, the team can add other project members as
// watchers and take them off again — not just watch themselves (QA report 6:
// "enable users to add and manage additional team members as watchers").
// The added person sees themselves on the list and can step off it.
//
// Pre-reqs: dev stack up (`just up`), SPRINTLY_OPEN_SIGNUP=true.

import { test, expect, type Page } from "@playwright/test";

function rand(): string {
  return Math.random().toString(36).slice(2, 8);
}

async function fill(page: Page, label: string, value: string) {
  await page.getByLabel(label, { exact: false }).fill(value);
}

async function register(page: Page, display: string, handle: string) {
  await page.goto("/register");
  await fill(page, "Display name", display);
  await fill(page, "Handle", handle);
  await fill(page, "Email", `${handle}@sprintly.test`);
  await fill(page, "Password", "correct-horse-battery-staple");
  await page.getByRole("button", { name: /\$ git init account/ }).click();
  await expect(page).toHaveURL(/\/(me\/day)?$/);
}

async function csrf(page: Page): Promise<string> {
  const cookies = await page.context().cookies();
  return cookies.find((c) => c.name === "sprintly_csrf")?.value ?? "";
}

test.describe("task watchers", () => {
  test("a lead adds a teammate as a watcher; the teammate can step off", async ({ page, browser }) => {
    const suffix = rand();
    const leadHandle = `lead${suffix}`;
    const mateHandle = `mate${suffix}`;
    const key = `WT${rand().slice(0, 3).toUpperCase()}`;

    // The teammate, in their own browser.
    const mateCtx = await browser.newContext();
    const mate = await mateCtx.newPage();
    await register(mate, "Mate Watcher", mateHandle);
    const mateMe = await (await mate.request.get("/api/v1/users/me")).json();

    await register(page, "Lead Person", leadHandle);
    await page.goto("/projects");
    await page.getByRole("button", { name: /new project/i }).first().click();
    const dialog = page.getByRole("dialog");
    await dialog.getByLabel("Name").fill("Watching");
    await dialog.getByLabel(/^Key/).fill(key);
    await dialog.getByRole("button", { name: /\$ git init project/ }).click();
    await expect(page).toHaveURL(new RegExp(`/projects/${key}$`));
    const added = await page.request.post(`/api/v1/projects/${key}/members`, {
      data: { user_id: mateMe.id, role: "contributor" },
      headers: { "X-CSRF-Token": await csrf(page) },
    });
    expect(added.ok(), await added.text()).toBe(true);

    await page.goto(`/projects/${key}/backlog`);
    await page.locator("[data-backlog-quick-add]").click();
    await page.getByLabel("new task title").fill("keep an eye on this");
    await page.getByLabel("new task title").press("Enter");
    await expect(page.getByText("keep an eye on this")).toBeVisible();

    const watchers = page.getByRole("region", { name: "watchers" });

    await test.step("lead adds the teammate from the picker", async () => {
      await page.goto(`/tasks/${key}-1`);
      await watchers.getByRole("button", { name: /add a watcher/i }).click();
      await watchers.getByLabel("find a teammate to add as a watcher").fill(mateHandle.slice(0, 6));
      await watchers.getByRole("option", { name: new RegExp(`@${mateHandle}`) }).click();
      await expect(watchers.getByText(`@${mateHandle}`)).toBeVisible();
      // The lead filed it, so the lead was watching already.
      await expect(watchers.getByText(/watchers \(2\)/)).toBeVisible();
      // Already watching → no longer offered.
      await watchers.getByLabel("find a teammate to add as a watcher").fill(mateHandle);
      await expect(watchers.getByRole("option", { name: new RegExp(`@${mateHandle}`) })).toHaveCount(0);
    });

    await test.step("it's stored: the teammate sees themselves watching", async () => {
      await mate.goto(`/tasks/${key}-1`);
      const theirs = mate.getByRole("region", { name: "watchers" });
      await expect(theirs.getByText(`@${mateHandle}`)).toBeVisible();
      await expect(theirs.getByRole("button", { name: /stop watching/i })).toBeVisible();
    });

    await test.step("lead takes them off again", async () => {
      await page.reload();
      await watchers.getByRole("button", { name: `remove @${mateHandle} from watchers` }).click();
      await expect(watchers.getByText(`@${mateHandle}`)).toHaveCount(0);
      await expect(watchers.getByText(`@${leadHandle}`)).toBeVisible();
      await expect(watchers.getByText(/watchers \(1\)/)).toBeVisible();
    });

    await mateCtx.close();
  });
});
