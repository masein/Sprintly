import { describe, expect, it } from "vitest";
import { fmtHours } from "./metrics";

describe("fmtHours", () => {
  it("renders an em dash for zero / missing / negative", () => {
    expect(fmtHours(0)).toBe("—");
    expect(fmtHours(-3)).toBe("—");
    expect(fmtHours(NaN)).toBe("—");
  });

  it("renders hours below a day (1 decimal under 10h, whole at/over 10h)", () => {
    expect(fmtHours(6)).toBe("6.0h");
    expect(fmtHours(3.5)).toBe("3.5h");
    expect(fmtHours(12)).toBe("12h");
  });

  it("renders days at or beyond 24h, to one decimal", () => {
    expect(fmtHours(24)).toBe("1.0d");
    expect(fmtHours(84)).toBe("3.5d");
  });
});
