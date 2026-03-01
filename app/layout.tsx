import type { Metadata } from "next";
import "./globals.css";

export const metadata: Metadata = {
  title: "RAW Processor — Web RAW Image Editor",
  description:
    "Browser-based RAW image processor powered by WebAssembly and WebGPU. Edit CR2, NEF, ARW, DNG and more directly in your browser with real-time adjustments.",
};

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode;
}>) {
  return (
    <html lang="ja">
      <body>{children}</body>
    </html>
  );
}
