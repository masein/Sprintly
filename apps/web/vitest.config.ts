import { defineConfig } from "vitest/config";

// Unit tests for pure helpers in lib/. Kept node-only and scoped to *.test.ts
// so it never tries to bootstrap the Next runtime.
export default defineConfig({
  test: {
    environment: "node",
    include: ["lib/**/*.test.ts"],
    coverage: {
      // The UI (components/app) is exercised by the Playwright e2e suite, not by
      // unit tests — coverage here measures the pure logic layer in lib/.
      provider: "v8",
      reporter: ["text-summary", "lcov"],
      include: ["lib/**/*.ts"],
      exclude: ["lib/**/*.test.ts"],
    },
  },
});
