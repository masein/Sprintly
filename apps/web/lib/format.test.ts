import { describe, expect, it } from "vitest";
import { pluralize } from "./format";

describe("pluralize", () => {
  it("uses the singular for exactly one (QA F11: no more '1 tasks')", () => {
    expect(pluralize(1, "task")).toBe("1 task");
    expect(pluralize(-1, "task")).toBe("-1 task");
  });

  it("uses the plural for zero and many", () => {
    expect(pluralize(0, "task")).toBe("0 tasks");
    expect(pluralize(2, "task")).toBe("2 tasks");
    expect(pluralize(17, "member")).toBe("17 members");
  });

  it("honours an explicit irregular plural", () => {
    expect(pluralize(1, "entry", "entries")).toBe("1 entry");
    expect(pluralize(3, "entry", "entries")).toBe("3 entries");
  });
});
