import { describe, expect, it } from "vitest";
import { labelColorMap, type Label } from "./labels";

function label(name: string, color: string): Label {
  return {
    id: name,
    project_id: "p",
    name,
    color,
    created_at: "2026-01-01T00:00:00Z",
  };
}

describe("labelColorMap", () => {
  it("maps lowercased names to colours", () => {
    const m = labelColorMap([label("Backend", "#ff0000"), label("UI", "#00ff00")]);
    expect(m).toEqual({ backend: "#ff0000", ui: "#00ff00" });
  });

  it("is empty for no labels", () => {
    expect(labelColorMap([])).toEqual({});
  });

  it("last write wins on a case-insensitive name clash", () => {
    const m = labelColorMap([label("bug", "#111111"), label("BUG", "#222222")]);
    expect(m.bug).toBe("#222222");
  });
});
