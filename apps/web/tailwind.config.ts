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
