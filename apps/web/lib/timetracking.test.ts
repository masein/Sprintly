import { describe, expect, it } from "vitest";
import { fmtMinutes, fmtMoneyCents } from "./timetracking";

describe("fmtMinutes", () => {
  it("shows bare minutes under an hour", () => {
    expect(fmtMinutes(0)).toBe("0m");
    expect(fmtMinutes(45)).toBe("45m");
    expect(fmtMinutes(59)).toBe("59m");
  });

  it("shows whole hours without a minutes suffix", () => {
    expect(fmtMinutes(60)).toBe("1h");
    expect(fmtMinutes(120)).toBe("2h");
  });

  it("shows hours and minutes together", () => {
    expect(fmtMinutes(90)).toBe("1h 30m");
    expect(fmtMinutes(125)).toBe("2h 5m");
  });
});

describe("fmtMoneyCents", () => {
  it("renders cents as a 2-decimal amount with the currency", () => {
    expect(fmtMoneyCents(0, "USD")).toBe("USD 0.00");
    expect(fmtMoneyCents(10_500, "USD")).toBe("USD 105.00");
    expect(fmtMoneyCents(199, "EUR")).toBe("EUR 1.99");
  });
});
