"use client";

import { Check, Keyboard, Loader2, RotateCcw, TriangleAlert } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { Button } from "@/components/Button";
import { captureChord, formatTrigger, DEFAULT_TRIGGER, type ChordProblem } from "@/lib/hotkey";
import { useT, type MessageKey } from "@/lib/i18n";
import {
  pushToTalkStatus,
  setPushToTalkTrigger,
  suspendPushToTalk,
} from "@/lib/tauri";
import type { HotkeyBackend, HotkeyStatus } from "@/lib/types";

const BACKEND_KEYS: Record<HotkeyBackend, MessageKey> = {
  portal: "settings.hotkey.backend.portal",
  evdev: "settings.hotkey.backend.evdev",
  "ns-event": "settings.hotkey.backend.ns-event",
  unavailable: "settings.hotkey.backend.unavailable",
};

const PROBLEM_KEYS: Record<ChordProblem, MessageKey> = {
  needsModifier: "settings.hotkey.needsModifier",
  unsupportedKey: "settings.hotkey.unsupportedKey",
};

/** Two renderings of the same chord, ignoring how each spaces its separators. */
function sameSpelling(a: string, b: string): boolean {
  const strip = (value: string) => value.replace(/\s+/g, "").toLowerCase();
  return strip(a) === strip(b);
}

/**
 * Record a new push-to-talk chord and say, truthfully, whether it took.
 *
 * The control is a keycap that you click and then press the chord into, which
 * is how every desktop does this and the reason none of them make you type
 * `CTRL+ALT+space` — that string is a storage format, not something a user
 * should have to know.
 *
 * The part that matters more than the recording is the line underneath. A
 * global shortcut can be accepted and still not fire: the XDG portal keeps a
 * binding it has already made, answers the rebind with success, and leaves the
 * old chord in place. Reporting that as "saved" would leave someone holding a
 * dead key and blaming their microphone, so the status here is driven by
 * `HotkeyStatus.ok` and never by the call merely having returned.
 */
export function HotkeyRecorder({ mac, stored }: { mac: boolean; stored?: string }) {
  const t = useT();
  const [status, setStatus] = useState<HotkeyStatus | null>(null);
  // Seeded from the settings row rather than from the listener, because the
  // two can disagree: if nothing has bound the hotkey yet, the listener has
  // nothing to report but the user's choice is still their choice, and showing
  // them the default instead would look like it had been thrown away.
  const [trigger, setTrigger] = useState(stored || DEFAULT_TRIGGER);
  const [recording, setRecording] = useState(false);
  /** Modifiers held so far, so the keycap fills in as the chord is pressed. */
  const [pending, setPending] = useState<string[]>([]);
  const [problem, setProblem] = useState<ChordProblem | null>(null);
  const [busy, setBusy] = useState(false);

  const show = useCallback(
    (value: string) => formatTrigger(value, { mac, spaceLabel: t("settings.hotkey.key.space") }),
    [mac, t],
  );

  useEffect(() => {
    let cancelled = false;
    pushToTalkStatus()
      .then((value) => {
        if (cancelled || !value) return;
        setStatus(value);
        setTrigger(value.trigger);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, []);

  // Closing the window mid-recording would otherwise leave push-to-talk
  // suspended with nothing left alive to put it back, so the hotkey would stay
  // dead until the app was restarted. Refs rather than state because this runs
  // on unmount, when there is no render left to read state for.
  const live = useRef({ recording, trigger });
  live.current = { recording, trigger };
  useEffect(
    () => () => {
      if (live.current.recording) {
        void setPushToTalkTrigger(live.current.trigger).catch(() => {});
      }
    },
    [],
  );

  const apply = useCallback(async (next: string | null) => {
    setBusy(true);
    setProblem(null);
    try {
      const result = await setPushToTalkTrigger(next);
      setStatus(result);
      setTrigger(result.trigger);
    } catch (error) {
      setStatus({
        backend: "unavailable",
        trigger: next ?? DEFAULT_TRIGGER,
        ok: false,
        detail: String(error),
      });
    } finally {
      setBusy(false);
    }
  }, []);

  const stopRecording = useCallback(() => {
    setRecording(false);
    setPending([]);
  }, []);

  async function startRecording() {
    setProblem(null);
    setPending([]);
    // Let go of the current chord first. Otherwise pressing it to re-record it
    // starts a recording instead, and under the portal the compositor holds
    // that chord for itself so the webview would never see the keys at all.
    await suspendPushToTalk().catch(() => {});
    setRecording(true);
  }

  /** Put back whatever was bound before recording started. */
  const cancelRecording = useCallback(() => {
    stopRecording();
    void apply(trigger);
  }, [apply, stopRecording, trigger]);

  useEffect(() => {
    if (!recording) return;

    // Capture phase, and every event swallowed: a chord being recorded must not
    // also reach the page, or Ctrl+Alt+D would run whatever Ctrl+Alt+D does.
    function onKeyDown(event: KeyboardEvent) {
      event.preventDefault();
      event.stopPropagation();
      if (event.code === "Escape") {
        cancelRecording();
        return;
      }
      const capture = captureChord(event);
      if (capture.state === "pending") {
        setPending(capture.parts);
        return;
      }
      if (capture.state === "rejected") {
        setProblem(capture.problem);
        setPending([]);
        return;
      }
      stopRecording();
      void apply(capture.trigger);
    }

    // Modifiers coming back up rewind the preview, so a mis-press can be
    // corrected by lifting the key rather than by starting over.
    function onKeyUp(event: KeyboardEvent) {
      event.preventDefault();
      event.stopPropagation();
      const capture = captureChord(event);
      if (capture.state === "pending") setPending(capture.parts);
    }

    window.addEventListener("keydown", onKeyDown, true);
    window.addEventListener("keyup", onKeyUp, true);
    window.addEventListener("blur", cancelRecording);
    return () => {
      window.removeEventListener("keydown", onKeyDown, true);
      window.removeEventListener("keyup", onKeyUp, true);
      window.removeEventListener("blur", cancelRecording);
    };
  }, [recording, apply, cancelRecording, stopRecording]);

  const bound = status?.boundDescription || "";
  const conflicted = status ? !status.ok && Boolean(bound) : false;
  const label = recording
    ? pending.length
      ? show(pending.join("+"))
      : t("settings.hotkey.recording")
    : show(trigger);

  return (
    <div>
      <p className="t-body mb-2.5 text-ui text-smoke">{t("settings.hotkey.desc")}</p>

      <div className="flex flex-wrap items-center gap-2">
        <button
          type="button"
          // A live region: the label changes under the user's fingers while
          // they hold the chord, and a screen reader has to follow it.
          aria-live="polite"
          disabled={busy}
          onClick={() => (recording ? cancelRecording() : void startRecording())}
          className={[
            "press flex h-9 min-w-[168px] items-center justify-center gap-2 rounded-md px-3",
            "text-ctl font-medium tabular-nums",
            recording
              ? "bg-fill text-ink ring-2 ring-accent"
              : "bg-fill text-ink hover:bg-fill-strong",
          ].join(" ")}
        >
          {recording ? (
            <Keyboard size={15} className="shrink-0 text-accent" />
          ) : null}
          <span className="truncate">{label}</span>
        </button>

        {!recording ? (
          <Button size="sm" disabled={busy} onClick={() => void startRecording()}>
            {t("settings.hotkey.change")}
          </Button>
        ) : null}

        {!recording && trigger !== DEFAULT_TRIGGER ? (
          <Button
            size="sm"
            icon={<RotateCcw size={14} />}
            disabled={busy}
            onClick={() => void apply(null)}
          >
            {t("settings.hotkey.reset")}
          </Button>
        ) : null}
      </div>

      <div className="mt-2 space-y-1">
        {recording ? (
          <p className="t-body text-meta text-smoke">{t("settings.hotkey.recordingHint")}</p>
        ) : null}

        {problem ? (
          <p className="t-body flex items-start gap-1.5 text-ui text-rust">
            <TriangleAlert size={14} className="mt-[2px] shrink-0" />
            <span>{t(PROBLEM_KEYS[problem])}</span>
          </p>
        ) : null}

        {busy ? (
          <p className="t-body flex items-center gap-1.5 text-ui text-smoke">
            <Loader2 size={14} className="shrink-0 animate-spin" />
            {t("settings.hotkey.applying")}
          </p>
        ) : null}

        {!busy && !recording && status ? (
          conflicted ? (
            <p className="t-body flex items-start gap-1.5 text-ui text-rust">
              <TriangleAlert size={14} className="mt-[2px] shrink-0" />
              <span>
                {t("settings.hotkey.conflict", {
                  bound,
                  path: t("settings.hotkey.conflictPath"),
                })}
              </span>
            </p>
          ) : status.ok ? (
            <p className="t-body flex items-start gap-1.5 text-ui text-smoke">
              <Check size={14} className="mt-[2px] shrink-0 text-moss" />
              <span>{t("settings.hotkey.live", { chord: show(status.trigger) })}</span>
            </p>
          ) : (
            <p className="t-body flex items-start gap-1.5 text-ui text-rust">
              <TriangleAlert size={14} className="mt-[2px] shrink-0" />
              <span>{t("settings.hotkey.notBound")}</span>
            </p>
          )
        ) : null}

        {/* The desktop's own words for the binding, whenever they differ from
            the chord shown on the cap. Kept even when everything is fine: it is
            how someone confirms the app and the desktop agree. Spacing is not a
            difference — the desktop writes `Ctrl+Alt+空格` where the cap reads
            `Ctrl + Alt + 空格`, and printing both would be noise. */}
        {!busy && !recording && bound && !conflicted && !sameSpelling(bound, show(trigger)) ? (
          <p className="t-body text-meta text-smoke">
            {t("settings.hotkey.boundAs", { bound })}
          </p>
        ) : null}

        {/* Rust's own words, which are not translated anywhere in this app.
            Shown only when something is wrong, where the extra precision is
            worth more than the language mismatch. */}
        {!busy && !recording && status && !status.ok && status.detail ? (
          <p className="t-body text-meta text-faint">{status.detail}</p>
        ) : null}

        {!busy && !recording && status ? (
          <p className="t-body text-meta text-smoke">
            {t("settings.hotkey.listener")}: {t(BACKEND_KEYS[status.backend])}
          </p>
        ) : null}

        {!recording && status?.backend === "portal" ? (
          <p className="t-body text-meta text-faint">{t("settings.hotkey.portalNote")}</p>
        ) : null}
      </div>
    </div>
  );
}
