import { defineConfig } from "vitest/config";

// Unit tests for pure helpers in lib/. Kept node-only and scoped to *.test.ts
// so it never tries to bootstrap the Next runtime.
export default defineConfig({
  test: {
    environment: "node",
    include: ["lib/**/*.test.ts"],
  },
});
