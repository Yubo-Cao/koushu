import type { Metadata } from "next";
import { I18nProvider } from "@/lib/i18n";
import "./globals.css";
// After globals.css, and deliberately: the macOS sheet is unlayered, so it
// wins over Tailwind's layered rules on source order alone.
import "./glass-macos.css";

export const metadata: Metadata = {
  title: "Koushu 口述",
  description: "Local desktop voice transcription for Fun-ASR.",
};

/**
 * Stamp the platform onto `<html>` before anything is painted.
 *
 * The material rules differ per platform in ways CSS cannot ask about: macOS
 * has a real system material behind the webview and the page must get out of
 * its way, while the Linux build paints its own because the compositor gives
 * it nothing to sample. There is no media query for "which OS", so the answer
 * has to come from script.
 *
 * It runs inline, as the first thing in `<body>`, rather than from an effect
 * or a Tauri command. Both of those land at least one frame late, and one
 * frame of the wrong material is a visible flash of the painted capsule before
 * the real glass appears. `navigator.userAgent` is enough here — this only has
 * to separate three desktop targets, and the WebKit build on macOS says
 * "Macintosh" in every version this app supports.
 */
const PLATFORM_SCRIPT = `(function(){var u=navigator.userAgent||"";var p=/Mac|iPhone|iPad/.test(u)?"macos":/Windows|Win32/.test(u)?"windows":"linux";document.documentElement.dataset.platform=p;})();`;

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
        <script dangerouslySetInnerHTML={{ __html: PLATFORM_SCRIPT }} />
        <I18nProvider>{children}</I18nProvider>
      </body>
    </html>
  );
}
