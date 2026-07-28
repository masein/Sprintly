import { afterEach, describe, expect, it, vi } from "vitest";
import { copyText } from "./clipboard";

// The whole point of this helper is the environment WITHOUT
// navigator.clipboard (plain-http deployments) — simulate both worlds.

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("copyText", () => {
  it("uses navigator.clipboard when it exists", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal("navigator", { clipboard: { writeText } });
    expect(await copyText("hello")).toBe(true);
    expect(writeText).toHaveBeenCalledWith("hello");
  });

  it("falls back to execCommand when navigator.clipboard is missing (http)", async () => {
    vi.stubGlobal("navigator", {}); // insecure context: no .clipboard at all
    const appended: unknown[] = [];
    const ta = {
      value: "",
      setAttribute: vi.fn(),
      select: vi.fn(),
      style: {} as Record<string, string>,
    };
    vi.stubGlobal("document", {
      createElement: vi.fn(() => ta),
      execCommand: vi.fn(() => true),
      body: {
        appendChild: (n: unknown) => appended.push(n),
        removeChild: vi.fn(),
      },
    });
    expect(await copyText("fallback me")).toBe(true);
    expect(ta.value).toBe("fallback me");
    expect(appended).toHaveLength(1);
  });

  it("reports failure honestly instead of pretending", async () => {
    vi.stubGlobal("navigator", {});
    vi.stubGlobal("document", {
      createElement: () => {
        throw new Error("no DOM here");
      },
    });
    expect(await copyText("doomed")).toBe(false);
  });
});
