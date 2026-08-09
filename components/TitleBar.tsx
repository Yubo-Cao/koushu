"use client";

import { Minus, Square, X } from "lucide-react";
import { useEffect, useState, type ReactNode } from "react";
import { useT } from "@/lib/i18n";

/**
 * The app's own title bar.
 *
 * Both platforms drop the system bar, but not to the same degree, and the
 * difference decides what this component renders.
 *
 * - macOS keeps its traffic lights (`titleBarStyle: "Overlay"`): content runs
 *   under them, but close/minimise/zoom stay native. Hand-drawn replacements
 *   never match the real ones on hover behaviour, on window-menu integration or
 *   on VoiceOver, so the only thing owed here is the ~72px of clearance they
 *   occupy.
 * - Linux has `set_decorations(false)` and therefore no buttons at all, so this
 *   draws them. Right-hand side, minimise/maximise/close in that order, which
 *   is what both KDE and GNOME default to.
 *
 * Everything that is not a control carries `data-tauri-drag-region`. Tauri
 * matches that attribute on the exact event target rather than on an ancestor,
 * so it has to be repeated on the inert children — and must never appear on a
 * button, or the click is swallowed by the drag.
 */

async function currentWindow() {
  const { getCurrentWindow } = await import("@tauri-apps/api/window");
  return getCurrentWindow();
}

/** Platform sniffing, not feature detection: the question is genuinely "which
 *  OS drew the window frame", and nothing in the DOM answers that. */
function detectMac() {
  if (typeof navigator === "undefined") return false;
  return /mac/i.test(navigator.userAgent);
}

type TitleBarProps = {
  /** Left zone. Sized by `leadClassName` so it can line up with a sidebar. */
  brand?: ReactNode;
  /** Width for the left zone. Supplying it also draws the divider that lines
      the zone up with a sidebar; without one there is no column to line up
      with and a stray hairline two words in just looks like a mistake. */
  leadClassName?: string;
  /** Centre zone: whatever this window is currently showing. */
  children?: ReactNode;
  /** Right zone, ahead of the window buttons. */
  actions?: ReactNode;
};

export function TitleBar({ brand, leadClassName = "", children, actions }: TitleBarProps) {
  // Resolved after mount: the server render has no navigator, and guessing
  // wrong would either hide the Linux buttons or double up on macOS.
  const [mac, setMac] = useState<boolean | null>(null);

  useEffect(() => setMac(detectMac()), []);

  return (
    <header
      data-tauri-drag-region
      className="titlebar relative z-30 flex h-[38px] shrink-0 items-center gap-2 pr-1.5 pl-2 select-none"
    >
      {mac ? <span data-tauri-drag-region className="w-[64px] shrink-0" /> : null}

      {brand ? (
        <div
          data-tauri-drag-region
          className={[
            "flex h-full shrink-0 items-center gap-2",
            leadClassName ? `hairline-r ${leadClassName}` : "pr-1",
          ].join(" ")}
        >
          {brand}
        </div>
      ) : null}

      <div data-tauri-drag-region className="flex min-w-0 flex-1 items-baseline gap-2">
        {children}
      </div>

      <div className="flex shrink-0 items-center gap-0.5">
        {actions}
        {mac === false ? <WindowButtons /> : null}
      </div>
    </header>
  );
}

function WindowButtons() {
  const t = useT();
  return (
    <div className="ml-1 flex items-center gap-0.5">
      <WindowButton label={t("titlebar.minimise")} onClick={(w) => w.minimize()}>
        <Minus size={14} />
      </WindowButton>
      <WindowButton label={t("titlebar.maximise")} onClick={(w) => w.toggleMaximize()}>
        <Square size={11} />
      </WindowButton>
      <WindowButton label={t("titlebar.close")} danger onClick={(w) => w.close()}>
        <X size={14} />
      </WindowButton>
    </div>
  );
}

function WindowButton({
  label,
  danger,
  onClick,
  children,
}: {
  label: string;
  danger?: boolean;
  onClick: (window: Awaited<ReturnType<typeof currentWindow>>) => Promise<void>;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      className={[
        "press flex h-[26px] w-[26px] items-center justify-center rounded-pill text-smoke",
        danger
          ? "hover:bg-rust hover:text-white"
          : "hover:bg-fill-strong hover:text-ink",
      ].join(" ")}
      onClick={() => void currentWindow().then(onClick).catch(() => {})}
    >
      {children}
    </button>
  );
}
