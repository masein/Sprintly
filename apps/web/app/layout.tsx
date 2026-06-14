import type { Metadata, Viewport } from "next";
import "../styles/globals.css";
import { Providers } from "@/components/Providers";
import { PwaRegister } from "@/components/PwaRegister";

export const metadata: Metadata = {
  title: "Sprintly",
  description: "Self-hosted project management for software teams.",
  appleWebApp: { capable: true, title: "Sprintly", statusBarStyle: "black-translucent" },
};

export const viewport: Viewport = {
  themeColor: "#7c5cff",
  width: "device-width",
  initialScale: 1,
};

export default function RootLayout({
  children,
}: {
  children: React.ReactNode;
}) {
  return (
    <html lang="en" className="dark">
      <head>
        {/* Self-hosted fonts come later. For now: system stack with web font hints. */}
        <link
          rel="preconnect"
          href="https://rsms.me"
          crossOrigin="anonymous"
        />
        <link rel="stylesheet" href="https://rsms.me/inter/inter.css" />
      </head>
      <body className="min-h-screen bg-ink text-chrome font-sans antialiased">
        <PwaRegister />
        <Providers>{children}</Providers>
      </body>
    </html>
  );
}
