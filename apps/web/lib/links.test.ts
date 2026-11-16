import { describe, expect, it } from "vitest";
import { safeExternalUrl, taskUrl } from "./links";

describe("taskUrl", () => {
  it("joins the origin and the key", () => {
    expect(taskUrl("https://sprintly.example", "SPD-146")).toBe(
      "https://sprintly.example/tasks/SPD-146",
    );
  });

  it("doesn't double the slash when the origin ends with one", () => {
    expect(taskUrl("http://212.33.206.34:8083/", "PP-1")).toBe(
      "http://212.33.206.34:8083/tasks/PP-1",
    );
  });

  it("encodes anything that isn't URL-safe", () => {
    expect(taskUrl("http://x", "A B/1")).toBe("http://x/tasks/A%20B%2F1");
  });
});

describe("safeExternalUrl", () => {
  it("keeps http(s) links", () => {
    expect(safeExternalUrl("https://github.com/acme/app/pull/5")).toBe(
      "https://github.com/acme/app/pull/5",
    );
    expect(safeExternalUrl(" http://gitlab.internal/x ")).toBe("http://gitlab.internal/x");
  });

  it("drops script and other non-web schemes", () => {
    for (const bad of [
      "javascript:alert(1)",
      "JavaScript:alert(document.cookie)",
      " javascript:alert(1)",
      "data:text/html,<script>alert(1)</script>",
      "vbscript:msgbox(1)",
      "file:///etc/passwd",
    ]) {
      expect(safeExternalUrl(bad)).toBeNull();
    }
  });

  it("drops relative, empty and unparseable values", () => {
    for (const bad of ["/tasks/X-1", "//evil.example", "", null, undefined, "not a url"]) {
      expect(safeExternalUrl(bad)).toBeNull();
    }
  });
});
