// feat/subtask-reorder: subtasks can be dragged into order by a grip handle,
// like the sprint page's task list, and the order is stored — it survives a
// reload (QA report 6: "drag-and-drop subtask reordering with intuitive grab
// handles").
//
// Pre-reqs: dev stack up (`just up`), SPRINTLY_OPEN_SIGNUP=true.

import { test, expect, type Locator, type Page } from "@playwright/test";

function rand(): string {
  return Math.random().toString(36).slice(2, 8);
}

async function fill(page: Page, label: string, value: string) {
  await page.getByLabel(label, { exact: false }).fill(value);
}

// dnd-kit-friendly drag: press, wiggle past the activation distance, travel.
async function dragOnto(page: Page, handle: Locator, target: Locator, below = false) {
  const sb = (await handle.boundingBox())!;
  const tb = (await target.boundingBox())!;
  await page.mouse.move(sb.x + sb.width / 2, sb.y + sb.height / 2);
  await page.mouse.down();
  await page.waitForTimeout(60);
  await page.mouse.move(sb.x + sb.width / 2, sb.y + sb.height / 2 + 8, { steps: 4 });
  await page.waitForTimeout(60);
  const y = below ? tb.y + tb.height * 0.8 : tb.y + tb.height * 0.2;
  await page.mouse.move(sb.x + sb.width / 2, y, { steps: 20 });
  await page.waitForTimeout(100);
  await page.mouse.up();
  await page.waitForTimeout(300);
}

async function order(page: Page): Promise<string[]> {
  return page
    .locator("[data-subtask-row]")
    .evaluateAll((els) => els.map((e) => e.getAttribute("data-subtask-row") ?? ""));
}

test.describe("subtask reordering", () => {
  test("drag a subtask by its grip; the order sticks", async ({ page }) => {
    const handle = `e2e${rand()}`;
    const key = `SR${rand().slice(0, 3).toUpperCase()}`;

    await page.goto("/register");
    await fill(page, "Display name", "Orderer");
    await fill(page, "Handle", handle);
    await fill(page, "Email", `${handle}@sprintly.test`);
    await fill(page, "Password", "correct-horse-battery-staple");
    await page.getByRole("button", { name: /\$ git init account/ }).click();
    await expect(page).toHaveURL(/\/(me\/day)?$/);

    await page.goto("/projects");
    await page.getByRole("button", { name: /new project/i }).first().click();
    const dialog = page.getByRole("dialog");
    await dialog.getByLabel("Name").fill("Ordering");
    await dialog.getByLabel(/^Key/).fill(key);
    await dialog.getByRole("button", { name: /\$ git init project/ }).click();
    await expect(page).toHaveURL(new RegExp(`/projects/${key}$`));

    await page.goto(`/projects/${key}/backlog`);
    await page.locator("[data-backlog-quick-add]").click();
    const input = page.getByLabel("new task title");
    await input.fill("the parent");
    await input.press("Enter");
    await expect(page.getByText("the parent")).toBeVisible();

    await page.goto(`/tasks/${key}-1`);
    for (const t of ["first", "second", "third"]) {
      await page.getByRole("button", { name: /add subtask/i }).click();
      await page.getByPlaceholder("subtask title").fill(t);
      await page.getByPlaceholder("subtask title").press("Enter");
      await expect(page.locator("[data-subtask-list]").getByText(t, { exact: true })).toBeVisible();
    }
    const [a, b, c] = [`${key}-2`, `${key}-3`, `${key}-4`];
    await expect.poll(() => order(page)).toEqual([a, b, c]);

    // Each row has a grip.
    for (const k of [a, b, c]) {
      await expect(page.getByRole("button", { name: `reorder ${k}` })).toBeVisible();
    }

    // Drag the third above the first.
    await dragOnto(
      page,
      page.getByRole("button", { name: `reorder ${c}` }),
      page.locator(`[data-subtask-row="${a}"]`),
    );
    await expect.poll(() => order(page)).toEqual([c, a, b]);

    // Stored, not just local.
    await page.reload();
    await expect.poll(() => order(page)).toEqual([c, a, b]);

    // A new subtask goes to the end of the custom order.
    await page.getByRole("button", { name: /add subtask/i }).click();
    await page.getByPlaceholder("subtask title").fill("fourth");
    await page.getByPlaceholder("subtask title").press("Enter");
    await expect.poll(() => order(page)).toEqual([c, a, b, `${key}-5`]);
  });
});
