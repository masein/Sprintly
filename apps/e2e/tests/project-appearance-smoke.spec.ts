// feat/project-appearance: a project's icon and colour are editable after
// creation, and each person can drag their project cards into their own order.
//
// QA report 3, low priority: "ability to change the project icon and its
// color", "can edit project cards order for each person".
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

// dnd-kit-friendly drag: press, wiggle past the activation distance, travel.
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

test.describe("project appearance + ordering", () => {
  test("repaint a project, then arrange the cards your way", async ({ page }) => {
    const handle = `e2e${rand()}`;
    const keyA = `PA${rand().slice(0, 3).toUpperCase()}`;
    const keyB = `PB${rand().slice(0, 3).toUpperCase()}`;

    await test.step("register + two projects", async () => {
      await page.goto("/register");
      await fill(page, "Display name", "Decorator");
      await fill(page, "Handle", handle);
      await fill(page, "Email", `${handle}@sprintly.test`);
      await fill(page, "Password", "correct-horse-battery-staple");
      await page.getByRole("button", { name: /\$ git init account/ }).click();
      await expect(page).toHaveURL(/\/(me\/day)?$/);

      await makeProject(page, "Alpha", keyA);
      await makeProject(page, "Beta", keyB);
    });

    await test.step("change the project's icon and colour", async () => {
      await page.goto(`/projects/${keyA}`);
      await page.getByRole("button", { name: "project appearance" }).click();
      await page.getByRole("button", { name: "icon rocket" }).click();
      // The panel stays open on purpose — pick the colour in the same visit.
      await expect(page.getByRole("button", { name: "icon rocket" })).toHaveAttribute(
        "aria-pressed",
        "true",
      );
      await page.getByRole("button", { name: "colour #22d3ee" }).click();
      await expect(page.getByRole("button", { name: "colour #22d3ee" })).toHaveAttribute(
        "aria-pressed",
        "true",
      );

      // Survives a reload — it's stored, not local state.
      await page.reload();
      await page.getByRole("button", { name: "project appearance" }).click();
      await expect(page.getByRole("button", { name: "icon rocket" })).toHaveAttribute(
        "aria-pressed",
        "true",
      );
      await expect(page.getByRole("button", { name: "colour #22d3ee" })).toHaveAttribute(
        "aria-pressed",
        "true",
      );
    });

    await test.step("drag the second card ahead of the first", async () => {
      await page.goto("/projects");
      const cards = page.locator("ul > li");
      // Newest-first from the server: Beta, then Alpha.
      await expect(cards.first()).toContainText("Beta");

      await dragOnto(
        page,
        page.getByRole("button", { name: `reorder ${keyA}` }),
        cards.first(),
      );
      await expect(page.locator("ul > li").first()).toContainText("Alpha");
    });

    await test.step("the order is mine and it sticks", async () => {
      await page.reload();
      await expect(page.locator("ul > li").first()).toContainText("Alpha");
    });
  });
});
