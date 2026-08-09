"use client";

import { useEffect, useState, type ReactNode } from "react";
import { getWindowChrome } from "@/lib/tauri";

/**
 * The window as the app draws it — which, now, is not much of it.
 *
 * Every platform's own toolkit owns the frame:
 *
 *   * **macOS** keeps the traffic lights, the rounded corners and the shadow in
 *     AppKit, and this component is a plain wrapper that costs nothing.
 *   * **Linux** hands the frame to GTK. The window is decorated in GTK's sense
 *     and given an empty, hidden titlebar widget, which puts the toolkit into
 *     client-side decorations: *GTK* paints the drop shadow and the corners,
 *     outside the webview's allocation, and owns the resize edges. See
 *     `adopt_gtk_csd` in `lib.rs`.
 *
 * # What used to be here, and why it could not work
 *
 * Linux used to run undecorated and transparent, with the app painting its own
 * shadow into an 18px transparent ring — a "gutter" — around the visible shell,
 * plus eight hand-drawn resize grips to replace the border that dropping
 * decorations took away.
 *
 * That could not be made correct, for a reason that has nothing to do with the
 * CSS: **WebKitGTK never clears a transparent window's surface.** Every frame
 * composites `src OVER dst` onto whatever was there before, so any pixel ever
 * painted opaque stays opaque for the life of that backing store — and the
 * window's first frame is painted before any script runs, with the gutter still
 * closed, stamping the page background into the ring permanently. Measured on
 * the shipped build, that ring transmitted 0.5–1.7% of the desktop behind it
 * (98–99% opaque) in the page's own gradient colours. That was the "shadow"
 * that bled and ended on a hard straight line: not a shadow, a frozen copy of
 * the window's background.
 *
 * Moving the shadow into GTK's layer takes it out of the webview's surface
 * entirely, and takes maximising and resizing with it — GTK drops the shadow
 * and the radius when the toplevel is maximised, and handles resize drags in
 * the toolkit rather than across the JS bridge.
 *
 * # What is left
 *
 * `window_chrome` still answers one question — *does the frontend draw the
 * frame* — and it now answers false everywhere, which is what closes the
 * gutter, squares the corners and drops the shadow in `globals.css`.
 *
 * The grips below are kept, and kept gated on `isDecorated()`, because they are
 * the answer to a different question: a window with no decorations at all has
 * no pointer resize affordance whatsoever. While GTK decorates the window this
 * renders nothing. If that ever stops being true, the window is still
 * resizable, which is not a property worth betting on a config flag.
 */

type ResizeDirection =
  | "North"
  | "NorthEast"
  | "East"
  | "SouthEast"
  | "South"
  | "SouthWest"
  | "West"
  | "NorthWest";

const GRIPS: { dir: ResizeDirection; cls: string }[] = [
  { dir: "NorthWest", cls: "win-grip-nw" },
  { dir: "North", cls: "win-grip-n" },
  { dir: "NorthEast", cls: "win-grip-ne" },
  { dir: "West", cls: "win-grip-w" },
  { dir: "East", cls: "win-grip-e" },
  { dir: "SouthWest", cls: "win-grip-sw" },
  { dir: "South", cls: "win-grip-s" },
  { dir: "SouthEast", cls: "win-grip-se" },
];

async function currentWindow() {
  const { getCurrentWindow } = await import("@tauri-apps/api/window");
  return getCurrentWindow();
}

export function WindowFrame({ children }: { children: ReactNode }) {
  // Starts closed, and the backend currently never opens it. Kept as state
  // rather than deleted because it is the one switch that turns the app-drawn
  // frame back on, and the frame it turns on has to stay coherent.
  const [csd, setCsd] = useState(false);
  // Separate question, separate answer. Whether the pointer can resize this
  // window depends on whether the toolkit still gives it a resize border, which
  // is what `isDecorated()` reports — and on nothing about the frame the page
  // draws.
  const [grips, setGrips] = useState(false);

  useEffect(() => {
    let cancelled = false;
    void getWindowChrome()
      .then((chrome) => {
        if (cancelled) return;
        setCsd(chrome.csdGutter);
        // `--gutter-size`, never `--gutter`. An inline custom property outranks
        // every stylesheet rule, so writing `--gutter` here would pin the ring
        // open and the maximised state could not close it — which is exactly
        // how a maximised window ended up keeping its border. The stylesheet
        // reads this one and decides what the gutter *currently* is.
        if (chrome.csdGutter) {
          document.documentElement.style.setProperty("--gutter-size", `${chrome.gutter}px`);
        } else {
          document.documentElement.style.removeProperty("--gutter-size");
        }
      })
      .catch(() => {
        // No backend (browser preview), or an older build without the command.
        // Either way the honest answer is "do not open the gutter".
      });
    void currentWindow()
      .then((window) => window.isDecorated())
      .then((decorated) => {
        if (!cancelled) setGrips(!decorated);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    document.documentElement.dataset.frame = csd ? "csd" : "native";
  }, [csd]);

  useEffect(() => {
    document.documentElement.dataset.windowResize = grips ? "grips" : "system";
  }, [grips]);

  // A window drawing its own frame has to close the gutter, square the corners
  // and drop the shadow when it is maximised, or maximising leaves a border of
  // desktop around a window the user asked to fill the screen. Inert while the
  // toolkit owns the frame — GTK does this itself — and it costs one attribute
  // to keep the app-drawn path correct if it is ever switched back on.
  useEffect(() => {
    if (!csd) {
      document.documentElement.removeAttribute("data-window-state");
      return;
    }
    let cancelled = false;
    const stops: (() => void)[] = [];

    const write = (maximized: boolean) => {
      if (cancelled) return;
      document.documentElement.dataset.windowState = maximized ? "maximized" : "floating";
    };

    const sync = async () => {
      try {
        const window = await currentWindow();
        write(await window.isMaximized());
      } catch {
        // Not running under Tauri.
      }
    };

    void sync();
    void currentWindow()
      .then((window) => window.onResized(() => void sync()))
      .then((stop) => {
        if (cancelled) stop();
        else stops.push(stop);
      })
      .catch(() => {});

    return () => {
      cancelled = true;
      for (const stop of stops) stop();
    };
  }, [csd]);

  return (
    <div className="win-frame">
      <div className="win-shell">{children}</div>
      {grips
        ? GRIPS.map(({ dir, cls }) => (
            <div
              key={dir}
              aria-hidden="true"
              className={`win-grip ${cls}`}
              onMouseDown={(event) => {
                if (event.button !== 0) return;
                // Without this the press also starts a text selection, which
                // the compositor-side resize then drags along with it.
                event.preventDefault();
                void currentWindow()
                  .then((window) => window.startResizeDragging(dir))
                  .catch(() => {});
              }}
            />
          ))
        : null}
    </div>
  );
}
