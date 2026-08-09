import type { Metadata } from "next";
import { I18nProvider } from "@/lib/i18n";
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
    // `lang` is a placeholder: a static export has no request to read a locale
    // from, so the prerender ships the fallback and the provider corrects the
    // attribute on mount.
    <html lang="en">
      <body>
        <I18nProvider>{children}</I18nProvider>
      </body>
    </html>
  );
}
