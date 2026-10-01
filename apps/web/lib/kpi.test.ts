import { describe, expect, it } from "vitest";
import { percent, signedPercent, summarizeVelocity, velocityValue } from "./kpi";
import type { VelocitySprint } from "./sprints";

const s = (name: string, points: number, extra: Partial<VelocitySprint> = {}): VelocitySprint => ({
  id: name,
  name,
  completed_at: "2026-09-01T00:00:00Z",
  points,
  tasks: 0,
  estimate_minutes: 0,
  logged_minutes: 0,
  ...extra,
});

describe("velocity", () => {
  it("reads every unit, hours to one decimal", () => {
    const x = s("S1", 8, { tasks: 3, estimate_minutes: 150, logged_minutes: 61 });
    expect(velocityValue(x, "points")).toBe(8);
    expect(velocityValue(x, "tasks")).toBe(3);
    expect(velocityValue(x, "hours")).toBe(2.5);
    expect(velocityValue(x, "logged")).toBe(1);
  });

  it("averages completed sprints and compares the last two", () => {
    const sum = summarizeVelocity([s("S8", 24), s("S9", 31), s("S10", 27)], null, "points");
    expect(sum.average).toBe(27.3);
    expect(sum.vsPrevious).toBe(-13); // 27 vs 31
    expect(sum.current).toBe(27);
    expect(sum.currentLabel).toBe("S10");
  });

  it("prefers the running sprint as 'current'", () => {
    const sum = summarizeVelocity([s("S12", 30)], s("S13", 12), "points");
    expect(sum.current).toBe(12);
    expect(sum.currentLabel).toBe("S13 (so far)");
    expect(sum.average).toBe(30); // the running sprint isn't in the average
    expect(sum.vsPrevious).toBeNull();
  });

  it("has nothing to say before the first sprint", () => {
    expect(summarizeVelocity([], null, "points")).toEqual({
      current: null,
      currentLabel: null,
      average: null,
      vsPrevious: null,
    });
  });

  it("won't divide by a zero previous sprint", () => {
    expect(summarizeVelocity([s("A", 0), s("B", 5)], null, "points").vsPrevious).toBeNull();
  });
});

describe("percent", () => {
  it("rounds and survives an empty sprint", () => {
    expect(percent(59, 72)).toBe(82);
    expect(percent(0, 0)).toBe(0);
    expect(signedPercent(12)).toBe("+12%");
    expect(signedPercent(-5)).toBe("−5%");
    expect(signedPercent(0)).toBe("±0%");
  });
});
