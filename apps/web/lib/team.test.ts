import { describe, expect, it } from "vitest";
import { cellHours, presetRange, rate } from "./team";

// Thursday 1 October 2026.
const thu = new Date(Date.UTC(2026, 9, 1, 15, 30));

describe("presetRange", () => {
  it("weeks run Monday to Sunday", () => {
    expect(presetRange("this-week", thu)).toEqual({ from: "2026-09-28", to: "2026-10-04" });
    expect(presetRange("last-week", thu)).toEqual({ from: "2026-09-21", to: "2026-09-27" });
  });

  it("a Sunday still belongs to the week that started on Monday", () => {
    const sun = new Date(Date.UTC(2026, 9, 4));
    expect(presetRange("this-week", sun)).toEqual({ from: "2026-09-28", to: "2026-10-04" });
  });

  it("months are whole calendar months", () => {
    expect(presetRange("this-month", thu)).toEqual({ from: "2026-10-01", to: "2026-10-31" });
    expect(presetRange("last-month", thu)).toEqual({ from: "2026-09-01", to: "2026-09-30" });
    const jan = new Date(Date.UTC(2027, 0, 15));
    expect(presetRange("last-month", jan)).toEqual({ from: "2026-12-01", to: "2026-12-31" });
  });
});

describe("rate / cellHours", () => {
  it("has no rate without a denominator", () => {
    expect(rate(1, 2)).toBe(50);
    expect(rate(0, 0)).toBeNull();
  });

  it("prints compact hours and blanks zeros", () => {
    expect(cellHours(0)).toBe("");
    expect(cellHours(45)).toBe("45m");
    expect(cellHours(90)).toBe("1.5h");
    expect(cellHours(120)).toBe("2h");
  });
});
