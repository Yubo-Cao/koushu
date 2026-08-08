"use client";

import { useEffect, useMemo, useState } from "react";

/**
 * A short burst of confetti, drawn with plain DOM and CSS.
 *
 * No dependency: a confetti library is a canvas engine and a physics loop for
 * something that runs for one second, twice in the app's life. This is ~40
 * absolutely-positioned divs on CSS animations, which the compositor handles
 * on the GPU without a rAF loop of our own.
 *
 * Respects `prefers-reduced-motion` by rendering nothing at all. Celebration
 * is the one thing that should never override that preference.
 */
export function Confetti({ fire, onDone }: { fire: boolean; onDone?: () => void }) {
  const [active, setActive] = useState(false);

  const reduced = useMemo(
    () =>
      typeof window !== "undefined" &&
      window.matchMedia?.("(prefers-reduced-motion: reduce)").matches,
    [],
  );

  // Positions are fixed per mount so a re-render does not restart the motion.
  const pieces = useMemo(
    () =>
      Array.from({ length: 42 }, (_, i) => ({
        left: Math.random() * 100,
        delay: Math.random() * 0.25,
        duration: 1.1 + Math.random() * 0.9,
        drift: (Math.random() - 0.5) * 160,
        spin: (Math.random() - 0.5) * 900,
        size: 5 + Math.random() * 6,
        hue: [12, 32, 145, 205, 268][i % 5],
      })),
    [],
  );

  useEffect(() => {
    if (!fire || reduced) {
      if (fire && reduced) onDone?.();
      return;
    }
    setActive(true);
    const timer = window.setTimeout(() => {
      setActive(false);
      onDone?.();
    }, 2400);
    return () => window.clearTimeout(timer);
  }, [fire, reduced, onDone]);

  if (!active || reduced) return null;

  return (
    <div className="pointer-events-none fixed inset-0 z-50 overflow-hidden" aria-hidden="true">
      {pieces.map((piece, i) => (
        <span
          key={i}
          className="absolute block rounded-[1px]"
          style={{
            left: `${piece.left}%`,
            top: "-6%",
            width: `${piece.size}px`,
            height: `${piece.size * 1.6}px`,
            background: `hsl(${piece.hue} 72% 58%)`,
            animation: `confetti-fall ${piece.duration}s cubic-bezier(.25,.6,.4,1) ${piece.delay}s forwards`,
            // Custom properties keep the per-piece values out of the keyframes.
            ["--drift" as string]: `${piece.drift}px`,
            ["--spin" as string]: `${piece.spin}deg`,
          }}
        />
      ))}
    </div>
  );
}
