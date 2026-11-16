import { describe, expect, it } from "vitest";
import { buildCsp, generateNonce } from "./csp";

function directive(csp: string, name: string): string[] {
  const d = csp.split("; ").find((x) => x.startsWith(`${name} `));
  return d ? d.split(" ").slice(1) : [];
}

describe("generateNonce", () => {
  it("is base64 of 16 random bytes, different every time", () => {
    const a = generateNonce();
    const b = generateNonce();
    expect(a).toMatch(/^[A-Za-z0-9+/]{22}==$/);
    expect(a).not.toBe(b);
  });
});

describe("buildCsp", () => {
  it("gates scripts on the nonce with strict-dynamic", () => {
    const csp = buildCsp("abc123");
    const script = directive(csp, "script-src");
    expect(script.slice(0, 2)).toEqual(["'nonce-abc123'", "'strict-dynamic'"]);
    expect(script).not.toContain("'unsafe-eval'");
  });

  it("only allows eval in dev", () => {
    expect(directive(buildCsp("n", { dev: true }), "script-src")).toContain("'unsafe-eval'");
  });

  it("keeps the hardening directives", () => {
    const csp = buildCsp("n");
    expect(directive(csp, "object-src")).toEqual(["'none'"]);
    expect(directive(csp, "frame-ancestors")).toEqual(["'none'"]);
    expect(directive(csp, "base-uri")).toEqual(["'self'"]);
    expect(directive(csp, "form-action")).toEqual(["'self'"]);
    expect(directive(csp, "connect-src")).toEqual(["'self'", "ws:", "wss:"]);
  });
});
