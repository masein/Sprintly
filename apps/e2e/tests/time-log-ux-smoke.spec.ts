// fix/time-log-ux: manual entries carry a real start time in the user's own
// timezone (not a hardcoded 09:00 rendered back as UTC "05:30"), log rows show
// who logged them plus the full note on hover, and the header's coffee meter
// reads today's total instead of the week's.
//
// QA report 3, middle priority: "add a tooltip on hover to display the
// complete note", "the 5:30 is placeholder with no meaning… in manual entry
// there is no start time", "the tracked box does not show who has added the
// time log", "the start time logger auto-fills UTC instead of the configured
// local timezone", "the time-bar 0-100 is for daily but the number is weekly".
//
// Pre-reqs: dev stack up (`just up`), SPRINTLY_OPEN_SIGNUP=true.

import { test, expect, type Page } from "@playwright/test";

function rand(): string {
  return Math.random().toString(36).slice(2, 8);
}

async function fill(page: Page, label: string, value: string) {
  await page.getByLabel(label, { exact: false }).fill(value);
}

// A local wall-clock date/time we can assert verbatim: the whole point is that
// what you type is what you see, with no UTC shift in between.
const LOG_DATE = "2026-08-03";
const LOG_TIME = "14:45";
const LONG_NOTE =
  "reviewed what has to happen before the migration can land, then paired on the retry logic";

test.describe("time log UX", () => {
  test("start time is local, the note is on hover, and the logger is named", async ({ page }) => {
    const handle = `e2e${rand()}`;
    const key = `TL${rand().slice(0, 3).toUpperCase()}`;

    await test.step("register + project + task", async () => {
      await page.goto("/register");
      await fill(page, "Display name", "Time Logger");
      await fill(page, "Handle", handle);
      await fill(page, "Email", `${handle}@sprintly.test`);
      await fill(page, "Password", "correct-horse-battery-staple");
      await page.getByRole("button", { name: /\$ git init account/ }).click();
      await expect(page).toHaveURL(/\/(me\/day)?$/);

      await page.goto("/projects");
      await page.getByRole("button", { name: /new project/i }).first().click();
      const dialog = page.getByRole("dialog");
      await dialog.getByLabel("Name").fill("Time Log UX");
      await dialog.getByLabel(/^Key/).fill(key);
      await dialog.getByRole("button", { name: /\$ git init project/ }).click();
      await expect(page).toHaveURL(new RegExp(`/projects/${key}$`));

      await page.goto(`/projects/${key}/backlog`);
      await page.locator("[data-backlog-quick-add]").click();
      const input = page.getByLabel("new task title");
      await input.fill("log some hours");
      await input.press("Enter");
      await expect(page.getByText("log some hours")).toBeVisible();
    });

    await test.step("manual entry takes a start time, defaulted to now", async () => {
      await page.goto(`/tasks/${key}-1`);
      await page.getByRole("button", { name: /manual entry/i }).click();
      const form = page.getByTestId("manual-entry-form");

      // Both fields exist and are prefilled with the local now, not UTC.
      const startTime = form.getByLabel("start time");
      await expect(startTime).toHaveValue(/^\d{2}:\d{2}$/);
      const localNow = await page.evaluate(() => {
        const d = new Date();
        const p = (n: number) => String(n).padStart(2, "0");
        return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
      });
      await expect(form.getByLabel("log date")).toHaveValue(localNow);

      await form.getByLabel("log date").fill(LOG_DATE);
      await startTime.fill(LOG_TIME);
      await form.getByLabel("hours", { exact: true }).fill("1");
      await form.getByLabel("minutes").fill("30");
      await form.locator('input[placeholder="note (optional)"]').fill(LONG_NOTE);
      await form.getByRole("button", { name: "add log" }).click();
    });

    await test.step("the row shows the local time, the logger, and the note in a tooltip", async () => {
      const row = page.locator("li", { hasText: "1h 30m" }).first();
      await expect(row).toBeVisible();

      // Displayed verbatim in local time — no 09:00→05:30 surprise.
      await expect(row).toContainText(`${LOG_DATE} ${LOG_TIME}`);

      // Who logged it: avatar (alt carries the handle) + the note tooltip.
      await expect(row.getByTitle(new RegExp(`@${handle}: `))).toBeVisible();
      const tip = await row.getByTitle(new RegExp(`@${handle}: `)).getAttribute("title");
      expect(tip).toContain(LONG_NOTE);
    });

    await test.step("the header meter counts today, not the week", async () => {
      // The entry above is dated in the past, so today's total is still 0 —
      // the old code would have shown the weekly sum here.
      const meter = page.locator('div[title*="logged today"]').first();
      await expect(meter).toBeVisible();
      const title = await meter.getAttribute("title");
      expect(title).toContain("0.0h logged today");

      // Log 45m starting now → today's number moves.
      await page.getByRole("button", { name: /manual entry/i }).click();
      const form = page.getByTestId("manual-entry-form");
      await form.getByLabel("hours", { exact: true }).fill("0");
      await form.getByLabel("minutes").fill("45");
      await form.getByRole("button", { name: "add log" }).click();

      await page.reload();
      const meter2 = page.locator('div[title*="logged today"]').first();
      await expect(meter2).toHaveAttribute("title", /0\.8h logged today/);
    });
  });
});
