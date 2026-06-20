// QA UX polish bundle: clickable cards (F7), status dropdown (F6), required-field
// feedback (F5), Esc-to-cancel inline inputs (F9).
//
// Pre-reqs: dev stack up (`just up`), SPRINTLY_OPEN_SIGNUP=true.

import { test, expect, type Page, type Locator } from "@playwright/test";

function rand(): string {
  return Math.random().toString(36).slice(2, 8);
}

// Register a fresh user and create a project; returns the project key.
async function setup(page: Page): Promise<string> {
  const handle = `e2e${rand()}`;
  const key = `UX${rand().slice(0, 3).toUpperCase()}`;
  await page.goto("/register");
  await page.getByLabel("Display name", { exact: false }).fill("Polish Tester");
  await page.getByLabel("Handle", { exact: false }).fill(handle);
  await page.getByLabel("Email", { exact: false }).fill(`${handle}@sprintly.test`);
  await page.getByLabel("Password", { exact: false }).fill("correct-horse-battery-staple");
  await page.getByRole("button", { name: /\$ git init account/ }).click();
  await expect(page).toHaveURL("/");

  await page.goto("/projects");
  await page.getByRole("button", { name: /new project/i }).first().click();
  const dialog = page.getByRole("dialog");
  await dialog.getByLabel("Name").fill("Polish");
  await dialog.getByLabel(/^Key/).fill(key);
  await dialog.getByRole("button", { name: /\$ git init project/ }).click();
  await expect(page).toHaveURL(new RegExp(`/projects/${key}$`));
  return key;
}

// The card with `title` inside the board column named `columnName`.
function columnCard(page: Page, columnName: string, title: string): Locator {
  return page
    .locator("div.flex-shrink-0")
    .filter({ has: page.getByRole("button", { name: columnName, exact: true }) })
    .getByText(title);
}

async function addCard(page: Page, title: string) {
  await page.locator("[data-add-card-button]").first().click();
  await page.getByPlaceholder("card title").fill(title);
  await page.getByRole("button", { name: /^add$/ }).click();
  await expect(page.getByText(title)).toBeVisible();
}

test.describe("QA polish — board & forms", () => {
  test("F5: empty New Project submit shows an inline required error", async ({ page }) => {
    // Register only (no project) so we can drive the create modal directly.
    const handle = `e2e${rand()}`;
    await page.goto("/register");
    await page.getByLabel("Display name", { exact: false }).fill("Form Tester");
    await page.getByLabel("Handle", { exact: false }).fill(handle);
    await page.getByLabel("Email", { exact: false }).fill(`${handle}@sprintly.test`);
    await page.getByLabel("Password", { exact: false }).fill("correct-horse-battery-staple");
    await page.getByRole("button", { name: /\$ git init account/ }).click();
    await expect(page).toHaveURL("/");

    await page.goto("/projects");
    await page.getByRole("button", { name: /new project/i }).first().click();
    const dialog = page.getByRole("dialog");
    await dialog.getByRole("button", { name: /\$ git init project/ }).click();
    // Inline error, and the modal stays open (no silent no-op, no navigation).
    await expect(dialog.getByText("Name is required.")).toBeVisible();
    await expect(dialog).toBeVisible();
  });

  test("F7: clicking a card opens the task; a column drag still moves it", async ({ page }) => {
    const key = await setup(page);
    await addCard(page, "Click me");

    // A plain click (no travel) opens the task.
    await columnCard(page, "To do", "Click me").click();
    await expect(page).toHaveURL(new RegExp(`/tasks/${key}-1`));

    // Back to the board, drag the card from "To do" to "In progress".
    await page.goto(`/projects/${key}`);
    const card = columnCard(page, "To do", "Click me");
    const target = page
      .locator("div.flex-shrink-0")
      .filter({ has: page.getByRole("button", { name: "In progress", exact: true }) });
    await dragCard(page, card, target);

    // The card now lives under "In progress" (a drag moved it, didn't open it).
    await expect(page).toHaveURL(new RegExp(`/projects/${key}$`));
    await expect(columnCard(page, "In progress", "Click me")).toBeVisible();
  });

  test("F6: changing status on the detail reflects on the board", async ({ page }) => {
    const key = await setup(page);
    await addCard(page, "Ship it");

    await page.goto(`/tasks/${key}-1`);
    // Status is now a dropdown of the board's real columns.
    await page.getByLabel("status").selectOption({ label: "Done" });

    // The board shows the card under "Done".
    await page.goto(`/projects/${key}`);
    await expect(columnCard(page, "Done", "Ship it")).toBeVisible();
  });

  test("F9: Esc dismisses the inline add-card input", async ({ page }) => {
    await setup(page);
    await page.locator("[data-add-card-button]").first().click();
    const input = page.getByPlaceholder("card title");
    await expect(input).toBeVisible();
    await input.press("Escape");
    await expect(input).toHaveCount(0);
  });
});

// A dnd-kit-friendly drag: press, nudge past the 6px activation distance, then
// travel to the target column's body in steps with brief settles so the
// PointerSensor activates and collision detection registers the drop zone.
async function dragCard(page: Page, source: Locator, target: Locator) {
  const sb = await source.boundingBox();
  const tb = await target.boundingBox();
  if (!sb || !tb) throw new Error("missing bounding box for drag");
  const startX = sb.x + sb.width / 2;
  const startY = sb.y + sb.height / 2;
  // Aim below the column header (~40px) so we land in the droppable body.
  const endX = tb.x + tb.width / 2;
  const endY = tb.y + 72;

  await page.mouse.move(startX, startY);
  await page.mouse.down();
  await page.waitForTimeout(60);
  await page.mouse.move(startX + 10, startY + 10, { steps: 5 }); // past activation distance
  await page.waitForTimeout(60);
  await page.mouse.move(endX, endY, { steps: 20 });
  await page.waitForTimeout(60);
  await page.mouse.move(endX, endY + 2, { steps: 3 }); // settle over the drop zone
  await page.waitForTimeout(60);
  await page.mouse.up();
  await page.waitForTimeout(120);
}
