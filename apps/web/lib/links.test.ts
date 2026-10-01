import { describe, expect, it } from "vitest";
import { taskUrl } from "./links";

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
