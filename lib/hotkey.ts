/**
 * Reading a push-to-talk chord off the keyboard, and writing one back out.
 *
 * The stored form is the XDG shortcuts spelling — `CTRL+ALT+space` — because
 * that is what the portal takes as its `preferred_trigger` and what the evdev
 * and macOS listeners parse. It is not a form anyone should have to type, so
 * nothing in the UI asks them to: the chord is recorded by pressing it, and
 * shown with `formatTrigger`.
 *
 * Rust validates the same rules again in `hotkey::normalize_trigger` and is the
 * authority. These checks exist so the recorder can say *why* a chord was
 * refused at the moment it is pressed, in the user's language, instead of
 * accepting it and reporting a failure a round trip later.
 */

/** Why a chord cannot serve as push-to-talk. Keys into the message catalogue. */
export type ChordProblem = "needsModifier" | "unsupportedKey";

export type ChordCapture =
  /** Modifiers are down, the main key has not been pressed yet. */
  | { state: "pending"; parts: string[] }
  | { state: "captured"; trigger: string; parts: string[] }
  | { state: "rejected"; problem: ChordProblem; parts: string[] };

const MODIFIER_CODES = new Set([
  "ControlLeft",
  "ControlRight",
  "AltLeft",
  "AltRight",
  "ShiftLeft",
  "ShiftRight",
  "MetaLeft",
  "MetaRight",
]);

/**
 * The XKB keysym name for a physical key, or null for one we will not bind.
 *
 * Read from `code` rather than `key`: `key` is what the layout and the held
 * modifiers produce, so Ctrl+Alt+2 arrives as `"@"` on some layouts and as a
 * dead key on others, and the recorded chord would then depend on which
 * modifiers happened to be down while recording it.
 *
 * The refusals are the deliberate part. Escape cancels the recorder, and Tab,
 * Enter, Backspace and the arrows are what every dialog and text field on the
 * system is built out of; a global grab on one of those takes it away
 * everywhere, which is not a thing to let someone do by accident.
 */
function keyName(code: string): string | null {
  if (code === "Space") return "space";
  if (/^Key[A-Z]$/.test(code)) return code.slice(3).toLowerCase();
  if (/^Digit[0-9]$/.test(code)) return code.slice(5);
  if (/^F([1-9]|1[0-9]|2[0-4])$/.test(code)) return code;
  return null;
}

/** Held modifiers, in the order a chord is written. */
function modifiersOf(event: {
  ctrlKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
  metaKey: boolean;
}): string[] {
  const parts: string[] = [];
  if (event.ctrlKey) parts.push("CTRL");
  if (event.altKey) parts.push("ALT");
  if (event.shiftKey) parts.push("SHIFT");
  if (event.metaKey) parts.push("LOGO");
  return parts;
}

/**
 * Turn one keydown into either a finished chord or a reason it is not one.
 *
 * Pressing a modifier reports `pending` rather than an error, which is what
 * makes the control feel like a recorder: the modifiers appear as they go
 * down, and the chord completes on the key that finishes it.
 */
export function captureChord(event: KeyboardEvent): ChordCapture {
  const modifiers = modifiersOf(event);
  if (MODIFIER_CODES.has(event.code)) {
    return { state: "pending", parts: modifiers };
  }

  const key = keyName(event.code);
  if (!key) {
    return { state: "rejected", problem: "unsupportedKey", parts: modifiers };
  }
  if (modifiers.length === 0) {
    return { state: "rejected", problem: "needsModifier", parts: [key] };
  }
  return {
    state: "captured",
    trigger: [...modifiers, key].join("+"),
    parts: [...modifiers, key],
  };
}

/** Apple's modifier glyphs, in Apple's order. */
const MAC_SYMBOLS: Record<string, string> = {
  CTRL: "⌃",
  ALT: "⌥",
  SHIFT: "⇧",
  LOGO: "⌘",
};

const MAC_ORDER = ["CTRL", "ALT", "SHIFT", "LOGO"];

/** What each modifier is called where there are no glyphs for them. */
const NAMES: Record<string, string> = {
  CTRL: "Ctrl",
  ALT: "Alt",
  SHIFT: "Shift",
  LOGO: "Super",
};

/**
 * The chord as a person would write it.
 *
 * `spaceLabel` comes from the message catalogue, because the space bar is the
 * one key in the supported set whose name is a word rather than what is printed
 * on it. Letters, digits and function keys read the same in every language.
 */
export function formatTrigger(
  trigger: string,
  options: { mac?: boolean; spaceLabel: string },
): string {
  const parts = trigger.split("+").filter(Boolean);
  const modifiers = parts.filter((part) => part in NAMES);
  const keys = parts.filter((part) => !(part in NAMES));
  const key = keys
    .map((name) => (name === "space" ? options.spaceLabel : name.toUpperCase()))
    .join(" ");

  if (options.mac) {
    const glyphs = MAC_ORDER.filter((name) => modifiers.includes(name))
      .map((name) => MAC_SYMBOLS[name])
      .join("");
    return `${glyphs}${key}`;
  }
  return [...modifiers.map((name) => NAMES[name]), key].filter(Boolean).join(" + ");
}

/** The chord the app ships with, and what "restore default" restores. */
export const DEFAULT_TRIGGER = "CTRL+ALT+space";
