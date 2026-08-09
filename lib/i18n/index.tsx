"use client";

import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { en, type MessageKey, type Messages } from "./en";
import { zh } from "./zh";
import { getBootstrap, setSetting } from "@/lib/tauri";

/*
  Why this is hand-rolled rather than next-intl or react-i18next.

  `next.config.ts` sets `output: "export"` and `tauri.conf.json` points its two
  windows at fixed paths — `url: "/"` and `url: "/bar"`. Every routing-based
  i18n scheme (next-intl's middleware, an App Router `[locale]` segment,
  next-i18next) works by putting the locale in the URL, which would turn those
  into `/zh` and `/zh/bar` and leave Tauri loading a page that no longer exists.
  Middleware additionally requires a Node server that a static export does not
  have. So the locale cannot live in the route; it has to live in React state.

  Once that is settled, the remaining candidates only differ in weight.
  react-i18next would add ~40 kB and an async resource lifecycle for a catalogue
  of ~200 strings that is already in the bundle; next-intl used client-only is
  really just a provider plus ICU parsing, and no message here needs plurals
  (Chinese has none, and the two English counts are minutes and bytes rendered
  by `Intl`/`formatBytes` anyway). What is left — a context, a lookup and
  `{name}` substitution — is this file.

  The gain is not only size. Because `MessageKey` is derived from the English
  catalogue, a typo or a key missing from `zh.ts` is a `tsc --noEmit` failure
  rather than a string that silently renders as its own key at runtime, which is
  the usual failure mode of the library route.
*/

export type Locale = "zh" | "en";

/** Menu order: the primary audience first. Labels are endonyms, never translated. */
export const LOCALES: readonly { id: Locale; label: string }[] = [
  { id: "zh", label: "中文" },
  { id: "en", label: "English" },
];

const CATALOGUES: Record<Locale, Messages> = { en, zh };

/** Row in the Rust-side `settings` table. Written only on an explicit choice. */
export const LOCALE_SETTING_KEY = "ui.locale";

/**
 * Local mirror of that row.
 *
 * The settings table is the source of truth, but reading it costs an IPC round
 * trip, and a provider that waits for one paints the whole window in the wrong
 * language first. `localStorage` is synchronous and shared across all three
 * webviews (same origin), so it serves the locale on the very first frame and
 * the IPC read reconciles behind it.
 */
const STORAGE_KEY = "ui.locale";

/** Broadcast so the other windows follow immediately, not on next launch. */
const LOCALE_EVENT = "fun-asr://locale-changed";

export type Translate = (key: MessageKey, vars?: Record<string, string | number>) => string;

type I18nValue = {
  locale: Locale;
  setLocale: (locale: Locale) => void;
  t: Translate;
};

const I18nContext = createContext<I18nValue | null>(null);

function isLocale(value: unknown): value is Locale {
  return value === "zh" || value === "en";
}

/**
 * System language, in the sense the user means it: any Chinese tag maps to
 * `zh`, everything else falls back to English. `navigator.languages` is
 * ordered by preference, so the first hit wins.
 */
export function detectSystemLocale(): Locale {
  if (typeof navigator === "undefined") return "en";
  const tags = navigator.languages?.length ? navigator.languages : [navigator.language];
  for (const tag of tags) {
    if (!tag) continue;
    if (tag.toLowerCase().startsWith("zh")) return "zh";
    if (tag.toLowerCase().startsWith("en")) return "en";
  }
  return "en";
}

/** Explicit past choice if there is one, otherwise the system language. */
function readInitialLocale(): Locale {
  try {
    const stored = window.localStorage.getItem(STORAGE_KEY);
    if (isLocale(stored)) return stored;
  } catch {
    // Private mode or a storage-less webview. Detection still works.
  }
  return detectSystemLocale();
}

function interpolate(template: string, vars?: Record<string, string | number>): string {
  if (!vars) return template;
  return template.replace(/\{(\w+)\}/g, (match, name: string) =>
    name in vars ? String(vars[name]) : match,
  );
}

export function I18nProvider({ children }: { children: ReactNode }) {
  /*
    `null` means "not resolved yet", and children are withheld until it is.

    A static export ships prerendered HTML, so the first client render has to
    match it byte for byte or React 19 reports a hydration mismatch and throws
    the tree away. The prerender has no `navigator` and no `localStorage`, so it
    cannot know the locale; the client does, immediately. Rendering nothing on
    both sides makes them agree, and the effect below fills it in before the
    first paint the user could notice. The alternative — defaulting to English
    and correcting after hydration — is a visible flash of the wrong language on
    every launch for the users this feature is for.
  */
  const [locale, setLocaleState] = useState<Locale | null>(null);
  /** Set once the user picks a language, to stop the reconcile below racing it. */
  const chosen = useRef(false);

  useEffect(() => {
    setLocaleState(readInitialLocale());
  }, []);

  /*
    Reconcile the mirror against the row it mirrors.

    The database is authoritative in *both* directions, which is the part that
    is easy to get wrong. Adopting the row when it exists is obvious. The case
    that bites is the row being absent while the mirror still holds a value —
    the settings table was reset, or app data was restored from a backup taken
    before the choice. "No row" means the user has never chosen, and a user who
    has never chosen follows the system; if the stale mirror were left to stand
    it would pin the app to a language nothing in the database asks for, with
    no way back short of clearing site data. So the mirror is cleared and
    detection runs again.

    `chosen` guards the one race this opens: a user who changes the language
    before the bootstrap round trip returns must not be overruled by the
    pre-change snapshot it is carrying.
  */
  useEffect(() => {
    let cancelled = false;
    getBootstrap()
      .then((data) => {
        if (cancelled || chosen.current) return;
        const stored = data.settings?.[LOCALE_SETTING_KEY];
        if (isLocale(stored)) {
          setLocaleState(stored);
          try {
            window.localStorage.setItem(STORAGE_KEY, stored);
          } catch {}
          return;
        }
        try {
          window.localStorage.removeItem(STORAGE_KEY);
        } catch {}
        setLocaleState(detectSystemLocale());
      })
      .catch(() => {
        // Browser preview, or the backend is not up yet. The mirror stands:
        // without an answer from the database there is nothing to reconcile
        // against, and guessing would throw away a real choice.
      });
    return () => {
      cancelled = true;
    };
  }, []);

  // Three windows, three React trees. Whichever one changes the setting tells
  // the others, so switching language in Settings repaints the main window and
  // the voice bar at once instead of on their next load.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    void import("@tauri-apps/api/event")
      .then(({ listen }) =>
        listen<Locale>(LOCALE_EVENT, (event) => {
          if (isLocale(event.payload)) setLocaleState(event.payload);
        }),
      )
      .then((stop) => {
        if (cancelled) stop();
        else unlisten = stop;
      })
      .catch(() => {});
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  // Font stack, hyphenation and line breaking all key off this, and WebKit
  // reads it live.
  useEffect(() => {
    if (locale) document.documentElement.lang = locale === "zh" ? "zh-CN" : "en";
  }, [locale]);

  const setLocale = useCallback((next: Locale) => {
    chosen.current = true;
    setLocaleState(next);
    try {
      window.localStorage.setItem(STORAGE_KEY, next);
    } catch {}
    void setSetting(LOCALE_SETTING_KEY, next).catch(() => {});
    void import("@tauri-apps/api/event")
      .then(({ emit }) => emit(LOCALE_EVENT, next))
      .catch(() => {});
  }, []);

  const value = useMemo<I18nValue | null>(() => {
    if (!locale) return null;
    const catalogue = CATALOGUES[locale];
    return {
      locale,
      setLocale,
      // `en[key]` as the fallback is unreachable while the catalogues are
      // typed, and stays as the guard against a hand-edited catalogue.
      t: (key, vars) => interpolate(catalogue[key] ?? en[key] ?? key, vars),
    };
  }, [locale, setLocale]);

  if (!value) return null;
  return <I18nContext.Provider value={value}>{children}</I18nContext.Provider>;
}

export function useI18n(): I18nValue {
  const value = useContext(I18nContext);
  if (!value) throw new Error("useI18n must be used inside <I18nProvider>");
  return value;
}

/** Shorthand for the common case: `const t = useT()`. */
export function useT(): Translate {
  return useI18n().t;
}

export type { MessageKey };
