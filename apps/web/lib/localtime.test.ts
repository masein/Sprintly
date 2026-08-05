import { describe, expect, it } from "vitest";
import { fmtLocal, localDateISO, localTimeHHMM, localToUtcISO } from "./localtime";

describe("localtime", () => {
  it("formats a local date/time without shifting through UTC", () => {
    // Constructed in local time on purpose: the point is that we never round
    // trip through toISOString(), which is what showed 09:00 as "05:30".
    const d = new Date(2026, 7, 4, 9, 5); // 2026-08-04 09:05 local
    expect(localDateISO(d)).toBe("2026-08-04");
    expect(localTimeHHMM(d)).toBe("09:05");
  });

  it("round-trips a typed date+time back to the same local wall clock", () => {
    const iso = localToUtcISO("2026-08-04", "09:05");
    expect(fmtLocal(iso)).toBe("2026-08-04 09:05");
  });

  it("falls back to 09:00 when the time input is empty or malformed", () => {
    expect(fmtLocal(localToUtcISO("2026-08-04", ""))).toBe("2026-08-04 09:00");
    expect(fmtLocal(localToUtcISO("2026-08-04", "nope"))).toBe("2026-08-04 09:00");
  });

  it("leaves an unparseable instant alone instead of printing Invalid Date", () => {
    expect(fmtLocal("not-a-date")).toBe("not-a-date");
  });
});
