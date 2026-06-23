import { describe, it, expect } from "vitest";
import {
  avatarSvg,
  asAvatarStyle,
  AVATAR_STYLES,
  DEFAULT_AVATAR_STYLE,
} from "./avatar";

describe("avatarSvg", () => {
  it("is deterministic: same seed + style → byte-identical markup", () => {
    for (const style of AVATAR_STYLES) {
      const a = avatarSvg("user-123", style);
      const b = avatarSvg("user-123", style);
      expect(a).toBe(b);
    }
  });

  it("varies by seed", () => {
    expect(avatarSvg("alice", "robot")).not.toBe(avatarSvg("bob", "robot"));
  });

  it("varies by style for the same seed", () => {
    const seed = "same-seed";
    const rendered = AVATAR_STYLES.map((s) => avatarSvg(seed, s));
    expect(new Set(rendered).size).toBe(AVATAR_STYLES.length);
  });

  it("always returns a well-formed svg element", () => {
    for (const style of AVATAR_STYLES) {
      const svg = avatarSvg("x", style);
      expect(svg.startsWith("<svg")).toBe(true);
      expect(svg.trimEnd().endsWith("</svg>")).toBe(true);
      expect(svg).toContain('viewBox="0 0 100 100"');
    }
  });

  it("falls back to the default style for an unknown style", () => {
    // @ts-expect-error — exercising the runtime fallback path on bad input.
    expect(avatarSvg("x", "nope")).toBe(avatarSvg("x", DEFAULT_AVATAR_STYLE));
  });

  it("treats an empty seed as a stable 'anon'", () => {
    expect(avatarSvg("", "beaver")).toBe(avatarSvg("anon", "beaver"));
  });
});

describe("asAvatarStyle", () => {
  it("passes through known styles and defaults unknown/empty ones", () => {
    expect(asAvatarStyle("identicon")).toBe("identicon");
    expect(asAvatarStyle(null)).toBe(DEFAULT_AVATAR_STYLE);
    expect(asAvatarStyle("bogus")).toBe(DEFAULT_AVATAR_STYLE);
  });
});
