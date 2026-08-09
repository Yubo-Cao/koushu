"use client";

import { useEffect, useState, type ReactNode } from "react";
import { getWindowChrome } from "@/lib/tauri";

/**
 * The window as the app draws it.
 *
 * On macOS the system still owns the frame — the traffic lights, the rounded
 * corners and the shadow all come from AppKit, and this component is a plain
 * wrapper that costs nothing.
 *
 * On Linux the window is created with `set_decorations(false)`, which hands the
 * app three jobs the compositor used to do:
 *
 *   1. **The shadow.** A Wayland compositor does not shadow a client-side-
 *      decorated surface; KWin expects the client to paint its own. A
 *      `box-shadow` can only fall outside the element it is on, so the window
 *      has to be bigger than the shell — hence `--gutter`, a ring of
 *      transparent window around the visible surface. This only works if the
 *      window was created with `transparent: true`; without it the gutter is an
 *      opaque band of page background and the shadow lands on nothing.
 *   2. **The rounded corners.** Same reason: a radius on a shell that reaches
 *      the window edge is cut square by the window rectangle.
 *   3. **Resizing.** Dropping decorations drops the compositor's resize border
 *      with them, so a bare CSD window cannot be resized by the pointer at all.
 *      The eight grips below give it back. They also solve the gutter's own
 *      problem: a Wayland surface receives pointer events across its whole
 *      rectangle regardless of `pointer-events`, so a transparent margin is
 *      going to eat clicks whatever we do — better that it eats them for a
 *      reason.
 *
 * Which case we are in is decided by one backend answer, `window_chrome`, and
 * by nothing this side can infer.
 *
 * The tempting inference — "the window is undecorated, therefore we draw the
 * frame, therefore open the gutter" — is wrong, and it shipped a visible bug
 * before this comment existed. Decorations were dropped long before
 * transparency was added, so for a while both were true of the same window:
 * undecorated *and* opaque. A gutter on an opaque window cannot be transparent;
 * it renders as a band of page background, and the user sees a hard pale border
 * around the window — two frames where there should be one.
 *
 * "Capability A is off" does not imply "capability B is on", however obviously
 * the two belong to the same idea, because they can land in separate commits
 * and the gap between them is what the user sees. So the frontend asks about
 * the fact it actually depends on: is this window transparent. The default
 * until that is answered is *no gutter*, because the cost of being wrong that
 * way is a missing shadow rather than a broken window.
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
  // Starts closed. An extra frame without the shadow is invisible; an extra
  // frame *with* a gutter on an opaque window is the bug described above.
  const [csd, setCsd] = useState(false);
  // Separate question, separate answer. Whether the pointer can resize this
  // window depends on whether the compositor still draws a resize border, which
  // is what `isDecorated()` reports — and on nothing about transparency.
  const [grips, setGrips] = useState(false);

  useEffect(() => {
    let cancelled = false;
    void getWindowChrome()
      .then((chrome) => {
        if (cancelled) return;
        setCsd(chrome.csdGutter);
        // The backend adds this ring to the window size, so reading the number
        // back from it is what stops the two definitions of "18px" drifting.
        if (chrome.csdGutter) {
          document.documentElement.style.setProperty("--gutter", `${chrome.gutter}px`);
        } else {
          document.documentElement.style.removeProperty("--gutter");
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

  // A maximised window has no outside, so the gutter, the radius and the
  // shadow all have to go — otherwise maximising leaves a transparent border
  // and four notches of desktop at the corners.
  useEffect(() => {
    if (!csd) {
      document.documentElement.removeAttribute("data-window-state");
      return;
    }
    let cancelled = false;
    let unlisten: (() => void) | undefined;

    const sync = async () => {
      try {
        const window = await currentWindow();
        const maximized = await window.isMaximized();
        if (!cancelled) {
          document.documentElement.dataset.windowState = maximized ? "maximized" : "floating";
        }
      } catch {
        // Not running under Tauri.
      }
    };

    void sync();
    void currentWindow()
      .then((window) => window.onResized(() => void sync()))
      .then((stop) => {
        if (cancelled) stop();
        else unlisten = stop;
      })
      .catch(() => {});

    return () => {
      cancelled = true;
      unlisten?.();
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
