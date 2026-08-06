// feat/notification-emails: settings → email. Choose whether Sprintly mails
// you, what about, and when. Asked for as "send proper notifications to email
// of each user as well".
//
// Pre-reqs: dev stack up (`just up`), SPRINTLY_OPEN_SIGNUP=true.

import { test, expect, type Page } from "@playwright/test";

function rand(): string {
  return Math.random().toString(36).slice(2, 8);
}

async function fill(page: Page, label: string, value: string) {
  await page.getByLabel(label, { exact: false }).fill(value);
}

test.describe("email preferences", () => {
  test("pick a mode, toggle a kind, choose a digest hour — and it persists", async ({ page }) => {
    const handle = `e2e${rand()}`;

    await page.goto("/register");
    await fill(page, "Display name", "Mailer");
    await fill(page, "Handle", handle);
    await fill(page, "Email", `${handle}@sprintly.test`);
    await fill(page, "Password", "correct-horse-battery-staple");
    await page.getByRole("button", { name: /\$ git init account/ }).click();
    await expect(page).toHaveURL(/\/(me\/day)?$/);

    await page.goto("/settings");
    const section = page.getByRole("region", { name: "email notifications" });
    await expect(section).toBeVisible();

    // Defaults: mail as it happens, mentions and assignments on, comments off.
    await expect(section.getByRole("button", { name: "as it happens" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    await expect(section.getByLabel(/@mentions me/)).toBeChecked();
    await expect(section.getByLabel(/comments on a task/)).not.toBeChecked();

    // Comments are opt-in.
    const saved = () =>
      page.waitForResponse(
        (r) =>
          r.url().includes("/users/me/email-prefs") &&
          r.request().method() === "PATCH" &&
          r.ok(),
      );
    let wait = saved();
    await section.getByLabel(/comments on a task/).check();
    await wait;

    // Digest mode reveals the hour picker; immediate doesn't need one.
    await expect(section.getByLabel("digest hour")).toHaveCount(0);
    wait = saved();
    await section.getByRole("button", { name: "once a day" }).click();
    await wait;
    await expect(section.getByLabel("digest hour")).toBeVisible();

    wait = saved();
    await section.getByLabel("digest hour").selectOption("17");
    await wait;

    // Survives a reload — this is stored server-side, not in the tab.
    await page.reload();
    const after = page.getByRole("region", { name: "email notifications" });
    await expect(after.getByRole("button", { name: "once a day" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    await expect(after.getByLabel("digest hour")).toHaveValue("17");
    await expect(after.getByLabel(/comments on a task/)).toBeChecked();

    // "never" hides the rest — there's nothing to configure about no email.
    wait = saved();
    await after.getByRole("button", { name: "never" }).click();
    await wait;
    await expect(after.getByLabel(/@mentions me/)).toHaveCount(0);

    // With no SMTP configured in dev, the UI says so rather than pretending.
    await expect(after.getByText(/no mail server is configured/i)).toBeVisible();
  });
});
