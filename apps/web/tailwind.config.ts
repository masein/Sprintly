import type { Config } from "tailwindcss";

// Tokens map to CSS variables defined in globals.css.
// Theme switching = `<html data-theme="...">`. Components stay theme-agnostic.

const config: Config = {
  darkMode: ["class", '[data-theme="midnight"]'],
  content: [
    "./app/**/*.{ts,tsx}",
    "./components/**/*.{ts,tsx}",
    "./lib/**/*.{ts,tsx}",
  ],
  theme: {
    extend: {
      // Tailwind's 2xl is 1536px, which leaves 1440-class laptops and
      // ultrawides in the same narrow column as a 1280px screen. `wide`
      // is the breakpoint QA asked for: above it, the shell's container and
      // the multi-column layouts get room instead of gutter.
      screens: { wide: "1440px" },
      fontFamily: {
        sans: ["Inter", "ui-sans-serif", "system-ui", "sans-serif"],
        mono: [
          "JetBrains Mono",
          "ui-monospace",
          "SFMono-Regular",
          "Menlo",
          "monospace",
        ],
      },
      colors: {
        ink: {
          DEFAULT: "var(--ink)",
          subtle: "var(--ink-subtle)",
          muted: "var(--ink-muted)",
        },
        chrome: {
          DEFAULT: "var(--chrome)",
          dim: "var(--chrome-dim)",
        },
        accent: {
          DEFAULT: "var(--accent)",
          fg: "var(--accent-fg)",
        },
      },
    },
  },
  plugins: [],
};

export default config;
