// fix/lane-drop-zones: with swimlanes on (assignee / label / priority /
// sprint), dragging a card into another column's body must move it. The drop
// zones inside a lane carry a `:body:<laneKey>` id, and the drag handler only
// recognised bare `:body` — so every column-body drop in a grouped view was
// silently ignored (cards only moved if you landed exactly on another card).
//
// Pre-reqs: dev stack up (`just up`), SPRINTLY_OPEN_SIGNUP=true.

import { test, expect, type Page, type Locator } from "@playwright/test";

function rand(): string {
  return Math.random().toString(36).slice(2, 8);
}

async function fill(page: Page, label: string, value: string) {
  await page.getByLabel(label, { exact: false }).fill(value);
}

// The column container (a lane renders its own set) holding `columnName`,
// scoped inside the lane section that contains `laneText` in its header.
function laneColumn(page: Page, laneText: RegExp, columnName: string): Locator {
  return page
    .locator("section")
    .filter({ has: page.locator('[data-testid="lane-header"]', { hasText: laneText }) })
    .locator("div.flex-shrink-0")
    .filter({ has: page.getByRole("button", { name: columnName, exact: true }) });
}

// dnd-kit-friendly drag into a column body (same recipe as ux-polish-smoke).
async function dragTo(page: Page, source: Locator, target: Locator) {
  const sb = await source.boundingBox();
  const tb = await target.boundingBox();
  if (!sb || !tb) throw new Error("missing bounding box for drag");
  const startX = sb.x + sb.width / 2;
  const startY = sb.y + sb.height / 2;
  const endX = tb.x + tb.width / 2;
  const endY = tb.y + 72; // below the column header, in the droppable body

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
  await page.waitForTimeout(150);
}

test.describe("swimlane column drag", () => {
  test("a card dragged into another column's body moves, inside a priority lane", async ({ page }) => {
    const handle = `e2e${rand()}`;
    const key = `LD${rand().slice(0, 3).toUpperCase()}`;

    await test.step("register + project + a card", async () => {
      await page.goto("/register");
      await fill(page, "Display name", "Lane Drag Tester");
      await fill(page, "Handle", handle);
      await fill(page, "Email", `${handle}@sprintly.test`);
      await fill(page, "Password", "correct-horse-battery-staple");
      await page.getByRole("button", { name: /\$ git init account/ }).click();
      await expect(page).toHaveURL("/");

      await page.goto("/projects");
      await page.getByRole("button", { name: /new project/i }).first().click();
      const dialog = page.getByRole("dialog");
      await dialog.getByLabel("Name").fill("Lane Drag");
      await dialog.getByLabel(/^Key/).fill(key);
      await dialog.getByRole("button", { name: /\$ git init project/ }).click();
      await expect(page).toHaveURL(new RegExp(`/projects/${key}$`));

      await page.locator("[data-add-card-button]").first().click();
      await page.getByPlaceholder("card title").fill("Drag me across");
      await page.getByRole("button", { name: /^add$/ }).click();
      await expect(page.getByText("Drag me across")).toBeVisible();
    });

    await test.step("group by priority; the card sits in the p2 lane's To do", async () => {
      await page.getByLabel("swimlane grouping").selectOption("priority");
      await expect(page.locator('[data-testid="lane-header"]', { hasText: "p2" })).toBeVisible();
      await expect(laneColumn(page, /p2/, "To do").getByText("Drag me across")).toBeVisible();
    });

    await test.step("drag it into the (empty) In progress column body", async () => {
      const card = laneColumn(page, /p2/, "To do").getByText("Drag me across");
      const target = laneColumn(page, /p2/, "In progress");
      await dragTo(page, card, target);
      await expect(laneColumn(page, /p2/, "In progress").getByText("Drag me across")).toBeVisible();
      await expect(laneColumn(page, /p2/, "To do").getByText("Drag me across")).toHaveCount(0);
    });

    await test.step("the move persisted (server-side, not just optimistic)", async () => {
      await page.reload();
      await page.getByLabel("swimlane grouping").selectOption("priority");
      await expect(laneColumn(page, /p2/, "In progress").getByText("Drag me across")).toBeVisible();
    });
  });
});
