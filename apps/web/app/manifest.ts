import type { MetadataRoute } from "next";

// PWA manifest (F17). Served at /manifest.webmanifest; Next auto-injects the
// <link rel="manifest">. Installable shell; data still comes from the API.
export default function manifest(): MetadataRoute.Manifest {
  return {
    name: "Sprintly",
    short_name: "Sprintly",
    description: "Self-hosted project management for software teams.",
    start_url: "/",
    scope: "/",
    display: "standalone",
    background_color: "#0a0a0b",
    theme_color: "#7c5cff",
    icons: [
      { src: "/icon.svg", sizes: "any", type: "image/svg+xml", purpose: "any" },
      { src: "/icon.svg", sizes: "any", type: "image/svg+xml", purpose: "maskable" },
    ],
  };
}
