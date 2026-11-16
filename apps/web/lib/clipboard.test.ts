import { afterEach, describe, expect, it, vi } from "vitest";
import { cancelClipboardWipe, copyText, scheduleClipboardWipe } from "./clipboard";

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

describe("scheduleClipboardWipe", () => {
  afterEach(() => {
    cancelClipboardWipe();
    vi.useRealTimers();
  });

  it("overwrites the clipboard once the delay passes", async () => {
    vi.useFakeTimers();
    const writeText = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal("navigator", { clipboard: { writeText } });
    scheduleClipboardWipe(30_000);
    await vi.advanceTimersByTimeAsync(29_999);
    expect(writeText).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(1);
    expect(writeText).toHaveBeenCalledWith("");
  });

  it("retries when the page regains focus if the browser refused", async () => {
    vi.useFakeTimers();
    const writeText = vi
      .fn()
      .mockRejectedValueOnce(new Error("Document is not focused"))
      .mockResolvedValue(undefined);
    vi.stubGlobal("navigator", { clipboard: { writeText } });
    // No execCommand fallback either (unfocused page).
    vi.stubGlobal("document", {
      createElement: () => {
        throw new Error("no DOM");
      },
    });
    const listeners: Record<string, () => void> = {};
    vi.stubGlobal("window", {
      addEventListener: (ev: string, fn: () => void) => (listeners[ev] = fn),
      removeEventListener: (ev: string) => delete listeners[ev],
    });
    scheduleClipboardWipe(1_000);
    await vi.advanceTimersByTimeAsync(1_000);
    expect(writeText).toHaveBeenCalledTimes(1);
    const onFocus = listeners.focus;
    expect(onFocus).toBeTypeOf("function");
    onFocus?.();
    await vi.advanceTimersByTimeAsync(0);
    expect(writeText).toHaveBeenCalledTimes(2);
    expect(writeText).toHaveBeenLastCalledWith("");
    expect(listeners.focus).toBeUndefined();
  });

  it("a newer copy replaces the pending wipe instead of stacking", async () => {
    vi.useFakeTimers();
    const writeText = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal("navigator", { clipboard: { writeText } });
    scheduleClipboardWipe(30_000);
    await vi.advanceTimersByTimeAsync(20_000);
    scheduleClipboardWipe(30_000); // copied again: the clock restarts
    await vi.advanceTimersByTimeAsync(20_000);
    expect(writeText).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(10_000);
    expect(writeText).toHaveBeenCalledTimes(1);
  });
});
