// fix/project-order-grip: the project wall could always be rearranged, but the
// drag grip was opacity-0 until a `group-hover` that never fired (the `group`
// class sat on a sibling link, not an ancestor) — so the handle was invisible
// and QA report 6 filed "allow users to customise their project card order"
// against a feature that already existed. This spec pins what was missing: the
// grip is visible without hovering, the page says what it's for, and a custom
// order can be put back to the default.
//
// Pre-reqs: dev stack up (`just up`), SPRINTLY_OPEN_SIGNUP=true.

import { test, expect, type Locator, type Page } from "@playwright/test";

function rand(): string {
  return Math.random().toString(36).slice(2, 8);
}

async function fill(page: Page, label: string, value: string) {
  await page.getByLabel(label, { exact: false }).fill(value);
}

async function makeProject(page: Page, name: string, key: string) {
  await page.goto("/projects");
  await page.getByRole("button", { name: /new project/i }).first().click();
  const dialog = page.getByRole("dialog");
  await dialog.getByLabel("Name").fill(name);
  await dialog.getByLabel(/^Key/).fill(key);
  await dialog.getByRole("button", { name: /\$ git init project/ }).click();
  await expect(page).toHaveURL(new RegExp(`/projects/${key}$`));
}

async function dragOnto(page: Page, handle: Locator, target: Locator) {
  const sb = (await handle.boundingBox())!;
  const tb = (await target.boundingBox())!;
  await page.mouse.move(sb.x + sb.width / 2, sb.y + sb.height / 2);
  await page.mouse.down();
  await page.waitForTimeout(60);
  await page.mouse.move(sb.x + sb.width / 2 + 12, sb.y + sb.height / 2 + 12, { steps: 5 });
  await page.waitForTimeout(60);
  await page.mouse.move(tb.x + tb.width / 2, tb.y + tb.height / 2, { steps: 20 });
  await page.waitForTimeout(80);
  await page.mouse.up();
  await page.waitForTimeout(250);
}

test.describe("project card order", () => {
  test("the grip is visible, explained, and the order can be reset", async ({ page }) => {
    const handle = `e2e${rand()}`;
    const keyA = `OA${rand().slice(0, 3).toUpperCase()}`;
    const keyB = `OB${rand().slice(0, 3).toUpperCase()}`;

    await page.goto("/register");
    await fill(page, "Display name", "Arranger");
    await fill(page, "Handle", handle);
    await fill(page, "Email", `${handle}@sprintly.test`);
    await fill(page, "Password", "correct-horse-battery-staple");
    await page.getByRole("button", { name: /\$ git init account/ }).click();
    await expect(page).toHaveURL(/\/(me\/day)?$/);
    await makeProject(page, "Alpha", keyA);
    await makeProject(page, "Beta", keyB);

    await page.goto("/projects");
    // Park the mouse well away from every card: the grip must not depend on hover.
    await page.mouse.move(0, 0);
    const grip = page.getByRole("button", { name: `reorder ${keyA}` });
    await expect(grip).toBeVisible();
    const opacity = await grip.evaluate((el) => Number(getComputedStyle(el).opacity));
    expect(opacity, "the grip should be visible without hovering").toBeGreaterThan(0.3);

    const hint = page.locator("[data-project-order-hint]");
    await expect(hint).toContainText(/order is yours alone/);
    // No custom order yet, so nothing to reset.
    await expect(hint.getByRole("button", { name: "reset order" })).toHaveCount(0);

    const cards = page.locator("ul > li");
    await expect(cards.first()).toContainText("Beta");
    await dragOnto(page, grip, cards.first());
    await expect(cards.first()).toContainText("Alpha");
    await page.reload();
    await expect(cards.first()).toContainText("Alpha");

    // Reset goes back to the server's order (newest first) and sticks.
    await page.getByRole("button", { name: "reset order" }).click();
    await expect(cards.first()).toContainText("Beta");
    await page.reload();
    await expect(cards.first()).toContainText("Beta");
    await expect(page.getByRole("button", { name: "reset order" })).toHaveCount(0);
  });
});
