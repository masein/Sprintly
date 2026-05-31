import type { Metadata } from "next";
import "../styles/globals.css";
import { Providers } from "@/components/Providers";

export const metadata: Metadata = {
  title: "Sprintly",
  description: "Self-hosted project management for software teams.",
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
        <Providers>{children}</Providers>
      </body>
    </html>
  );
}
