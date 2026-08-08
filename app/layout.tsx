import type { Metadata } from "next";
import "./globals.css";

export const metadata: Metadata = {
  title: "Fun ASR Desktop",
  description: "Local desktop voice transcription for Fun-ASR.",
};

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode;
}>) {
  return (
    <html lang="en">
      <body>{children}</body>
    </html>
  );
}
