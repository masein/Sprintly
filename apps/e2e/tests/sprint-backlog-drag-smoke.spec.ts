// feat/sprint-backlog-lane: the sprint page shows the project backlog in a
// side panel — drag a backlog row into the task list to commit it to the
// sprint, drag a sprint row onto the panel to send it back. QA report:
// "Display the Backlog within the Sprint view … and drag tasks to there."
//
// Pre-reqs: dev stack up (`just up`), SPRINTLY_OPEN_SIGNUP=true.

import { test, expect, type Locator, type Page } from "@playwright/test";

function rand(): string {
  return Math.random().toString(36).slice(2, 8);
}

async function fill(page: Page, label: string, value: string) {
  await page.getByLabel(label, { exact: false }).fill(value);
}

// dnd-kit-friendly drag from a grip handle onto a drop zone.
async function dragTo(page: Page, handle: Locator, target: Locator) {
  const sb = await handle.boundingBox();
  const tb = await target.boundingBox();
  if (!sb || !tb) throw new Error("missing bounding box for drag");
  const startX = sb.x + sb.width / 2;
  const startY = sb.y + sb.height / 2;
  const endX = tb.x + tb.width / 2;
  const endY = tb.y + Math.min(tb.height / 2, 80);

  await page.mouse.move(startX, startY);
  await page.mouse.down();
  await page.waitForTimeout(60);
  await page.mouse.move(startX + 10, startY + 10, { steps: 5 });
  await page.waitForTimeout(60);
  await page.mouse.move(endX, endY, { steps: 20 });
  await page.waitForTimeout(60);
  await page.mouse.move(endX, endY + 2, { steps: 3 });
  await page.waitForTimeout(60);
  await page.mouse.up();
  await page.waitForTimeout(200);
}

test.describe("sprint ↔ backlog drag", () => {
  test("drag a backlog task into the sprint and a sprint task back out", async ({ page }) => {
    const handle = `e2e${rand()}`;
    const key = `SL${rand().slice(0, 3).toUpperCase()}`;

    await test.step("register + project + two backlog tasks + a sprint", async () => {
      await page.goto("/register");
      await fill(page, "Display name", "Lane Dragger");
      await fill(page, "Handle", handle);
      await fill(page, "Email", `${handle}@sprintly.test`);
      await fill(page, "Password", "correct-horse-battery-staple");
      await page.getByRole("button", { name: /\$ git init account/ }).click();
      await expect(page).toHaveURL(/\/(me\/day)?$/);

      await page.goto("/projects");
      await page.getByRole("button", { name: /new project/i }).first().click();
      const dialog = page.getByRole("dialog");
      await dialog.getByLabel("Name").fill("Sprint Lane");
      await dialog.getByLabel(/^Key/).fill(key);
      await dialog.getByRole("button", { name: /\$ git init project/ }).click();
      await expect(page).toHaveURL(new RegExp(`/projects/${key}$`));

      await page.goto(`/projects/${key}/backlog`);
      // The quick-add row stays open (and refocused) after each add, so the
      // collapsed "+ add a task" button only exists for the first one.
      await page.locator("[data-backlog-quick-add]").click();
      for (const title of ["pull me in", "leave me here"]) {
        const input = page.getByLabel("new task title");
        await input.fill(title);
        await input.press("Enter");
        await expect(page.getByText(title)).toBeVisible();
      }

      await page.goto(`/projects/${key}/sprints`);
      await page.getByRole("button", { name: /new sprint/i }).click();
      await page.getByPlaceholder(/Sprint 23/i).fill("Sprint 1");
      await page.getByRole("button", { name: /\$ git init sprint/ }).click();
      await expect(page).toHaveURL(/\/sprints\/[0-9a-f-]+$/);
    });

    await test.step("the backlog panel lists both tasks", async () => {
      const panel = page.getByTestId("backlog-drop");
      await expect(panel.getByText("pull me in")).toBeVisible();
      await expect(panel.getByText("leave me here")).toBeVisible();
    });

    await test.step("drag 'pull me in' into the sprint", async () => {
      const panel = page.getByTestId("backlog-drop");
      const row = panel.locator("li", { hasText: "pull me in" });
      await dragTo(
        page,
        row.getByRole("button", { name: "drag to move" }),
        page.getByTestId("sprint-drop"),
      );
      const sprintZone = page.getByTestId("sprint-drop");
      await expect(sprintZone.getByText("pull me in")).toBeVisible();
      await expect(panel.getByText("pull me in")).toHaveCount(0);
      await expect(page.getByText("tasks (1)")).toBeVisible();
    });

    await test.step("drag it back to the backlog", async () => {
      const sprintZone = page.getByTestId("sprint-drop");
      const row = sprintZone.locator("li", { hasText: "pull me in" });
      await dragTo(
        page,
        row.getByRole("button", { name: "drag to move" }),
        page.getByTestId("backlog-drop"),
      );
      await expect(page.getByTestId("backlog-drop").getByText("pull me in")).toBeVisible();
      await expect(page.getByText("tasks (0)")).toBeVisible();
    });

    await test.step("moves persisted (reload agrees)", async () => {
      await page.reload();
      await expect(page.getByTestId("backlog-drop").getByText("pull me in")).toBeVisible();
      await expect(page.getByText("tasks (0)")).toBeVisible();
    });
  });
});
