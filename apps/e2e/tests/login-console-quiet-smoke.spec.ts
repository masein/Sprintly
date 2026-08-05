// fix/lighthouse-pass: /login used to open a WebSocket it could never
// authenticate (three failed handshakes per view, and back/forward-cache
// blocked) and to request a favicon we don't ship. QA report 3: "Check
// lighthouse standards."
//
// What's left on a signed-out page is the session check itself — one 401 from
// /users/me and the single refresh attempt behind it. Those are correct, so
// this spec asserts on the *kinds* of noise we fixed rather than on silence.
//
// Pre-reqs: dev stack up (`just up`).

import { test, expect } from "@playwright/test";

test.describe("signed-out console", () => {
  test("/login opens no WebSocket and asks for no missing favicon", async ({ page }) => {
    const errors: string[] = [];
    const failed: string[] = [];
    page.on("console", (m) => {
      if (m.type() === "error") errors.push(m.text());
    });
    page.on("response", (r) => {
      if (r.status() >= 400) failed.push(`${r.status()} ${r.url()}`);
    });
    const sockets: string[] = [];
    page.on("websocket", (ws) => sockets.push(ws.url()));

    await page.goto("/login");
    await expect(page.getByRole("button", { name: /ssh sprintly/i })).toBeVisible();
    // Realtime came up on a timer, so give it longer than it would need.
    await page.waitForTimeout(2500);

    expect(sockets, "a signed-out page has no session to subscribe with").toEqual([]);
    expect(
      errors.filter((e) => /websocket/i.test(e)),
      "failed WebSocket handshakes are back",
    ).toEqual([]);
    expect(
      failed.filter((f) => /favicon\.ico/.test(f)),
      "browsers are guessing /favicon.ico again — is <link rel=icon> gone?",
    ).toEqual([]);
    expect(
      failed.filter((f) => /achievements/.test(f)),
      "the achievement poller is asking for a session that isn't there",
    ).toEqual([]);
  });
});
