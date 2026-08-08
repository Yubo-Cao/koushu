"use client";

import { GripVertical, Loader2, Mic, Square } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { DEFAULT_BACKEND } from "@/lib/backends";
import {
  autoPasteText,
  createSession,
  getAudioLevel,
  hideVoiceBar,
  listAudioInputs,
  resizeVoiceBar,
  beginVoiceBarDrag,
  endVoiceBarDrag,
  trackVoiceBarDrag,
  showVoiceBarPassive,
  startAudioCapture,
  startPushToTalk,
  startStreamingTranscription,
  stopAudioCapture,
  stopPushToTalk,
  stopStreamingTranscription,
  transcribeAudio,
} from "@/lib/tauri";
import type {
  AudioLevelInfo,
  HotkeyStatus,
  PushToTalkEvent,
  StreamingEvent,
} from "@/lib/types";

const idleLevel: AudioLevelInfo = { rms: 0, peak: 0, db: -90, percent: 0 };

/** Committed preview segments joined with the live partial. */
function joinPreview(segments: string[], partial: string): string {
  return [...segments, partial].filter((part) => part.trim()).join(" ");
}

type Phase = "idle" | "listening" | "transcribing" | "done";

export default function VoiceBar() {
  const [phase, setPhase] = useState<Phase>("idle");
  const [partial, setPartial] = useState("");
  const [status, setStatus] = useState("");
  const [inputLevel, setInputLevel] = useState<AudioLevelInfo>(idleLevel);
  const [hotkey, setHotkey] = useState<HotkeyStatus | null>(null);
  const [anchor, setAnchor] = useState("bottom-center");
  const [dragging, setDragging] = useState(false);

  const sessionIdRef = useRef<string | null>(null);
  const segmentsRef = useRef<string[]>([]);
  const levelTimerRef = useRef<number | null>(null);
  const recordingRef = useRef(false);
  const pttBusyRef = useRef(false);
  const collapseTimerRef = useRef<number | null>(null);
  const shellRef = useRef<HTMLDivElement | null>(null);


  const recording = phase === "listening";
  const expanded = phase !== "idle";

  useEffect(() => {
    recordingRef.current = recording;
  }, [recording]);

  // Keep the window exactly as large as the pill. Measuring the DOM beats
  // guessing a size per state: the window never clips the text, and never
  // leaves an invisible margin that still eats clicks meant for what is behind
  // it.
  useEffect(() => {
    const el = shellRef.current;
    if (!el) return;
    const sync = () => {
      const rect = el.getBoundingClientRect();
      if (rect.width < 1) return;
      void resizeVoiceBar(Math.ceil(rect.width) + 12, Math.ceil(rect.height) + 12).catch(
        () => {},
      );
    };
    sync();
    const observer = new ResizeObserver(sync);
    observer.observe(el);
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    const el = shellRef.current;
    if (!el) return;
    const rect = el.getBoundingClientRect();
    if (rect.width < 1) return;
    void resizeVoiceBar(Math.ceil(rect.width) + 12, Math.ceil(rect.height) + 12).catch(
      () => {},
    );
  }, [phase, partial, status, hotkey]);

  useEffect(() => {
    let cancelled = false;
    startPushToTalk(undefined, handlePushToTalk)
      .then((value) => {
        if (cancelled) return;
        setHotkey(value);
        if (value.backend === "unavailable") setStatus(value.detail);
      })
      .catch((error) => setStatus(String(error)));
    return () => {
      cancelled = true;
      void stopPushToTalk().catch(() => {});
    };
  }, []);

  function scheduleCollapse(delay: number) {
    if (collapseTimerRef.current) window.clearTimeout(collapseTimerRef.current);
    collapseTimerRef.current = window.setTimeout(() => {
      setPhase("idle");
      setPartial("");
      setStatus("");
    }, delay);
  }

  async function handlePushToTalk(event: PushToTalkEvent) {
    if (pttBusyRef.current) return;
    if (event.event === "pressed") {
      if (recordingRef.current) return;
      pttBusyRef.current = true;
      try {
        await showVoiceBarPassive();
        await start();
      } finally {
        pttBusyRef.current = false;
      }
    } else if (event.event === "released") {
      if (!recordingRef.current) return;
      pttBusyRef.current = true;
      try {
        await stop();
      } finally {
        pttBusyRef.current = false;
      }
    }
  }

  function handleStreamingEvent(event: StreamingEvent) {
    if (event.event === "partial") {
      setPartial(joinPreview(segmentsRef.current, event.data.text));
    } else if (event.event === "segment") {
      segmentsRef.current = [...segmentsRef.current, event.data.text];
      setPartial(joinPreview(segmentsRef.current, ""));
    } else if (event.event === "error") {
      setStatus(event.data.error);
    }
  }

  async function start() {
    if (collapseTimerRef.current) window.clearTimeout(collapseTimerRef.current);
    try {
      const session = await createSession({
        title: `Voice ${new Date().toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}`,
        model: "fun-asr-nano-2512",
        language: "中文",
        runtime: DEFAULT_BACKEND,
      });
      sessionIdRef.current = session.id;
      await startAudioCapture(undefined);
      segmentsRef.current = [];
      setPartial("");
      setStatus("");
      setPhase("listening");
      levelTimerRef.current = window.setInterval(() => {
        void getAudioLevel().then(setInputLevel).catch(() => {});
      }, 100);
      await startStreamingTranscription("fun-asr-nano-2512", handleStreamingEvent);
    } catch (error) {
      setPhase("idle");
      sessionIdRef.current = null;
      setStatus(String(error));
    }
  }

  function clearTimers() {
    void stopStreamingTranscription().catch(() => {});
    if (levelTimerRef.current) window.clearInterval(levelTimerRef.current);
    levelTimerRef.current = null;
  }

  async function stop() {
    clearTimers();
    setPhase("transcribing");
    setInputLevel(idleLevel);
    try {
      const capture = await stopAudioCapture();
      if (!capture.speechLike) {
        setPartial("");
        setStatus("No speech");
        setPhase("done");
        scheduleCollapse(1600);
        return;
      }
      const result = await transcribeAudio({
        sessionId: sessionIdRef.current || undefined,
        audioBase64: capture.audioBase64,
        modelId: "fun-asr-nano-2512",
        language: "中文",
      });
      if (result.text) {
        setPartial(result.text);
        const paste = await autoPasteText(result.text);
        setStatus(paste.message);
      } else {
        setStatus(result.error || "No transcript");
      }
      setPhase("done");
      scheduleCollapse(2800);
    } catch (error) {
      setStatus(String(error));
      setPhase("done");
      scheduleCollapse(2800);
    } finally {
      sessionIdRef.current = null;
    }
  }

  /**
   * Drag the bar by following the real cursor.
   *
   * Two earlier attempts failed for the same underlying reason. Deriving
   * position from the webview's own pointer events — screenX, and later
   * movementX under Pointer Lock — cannot work, because Wayland gives a client
   * no global pointer position and moving the window changes the very
   * coordinates the next delta is measured from. That is a feedback loop, and
   * no amount of smoothing fixes it.
   *
   * The compositor does know where the cursor is, so Rust asks it (~3 ms) and
   * derives the position from cursor-now minus cursor-at-drag-start. Nothing
   * accumulates, and the window is never read back, so the loop is gone. The
   * frontend only has to keep a timer running.
   */
  function startDrag(event: React.MouseEvent) {
    if (event.button !== 0) return;
    event.preventDefault();

    let active = true;
    let timer = 0;

    const finish = () => {
      if (!active) return;
      active = false;
      window.clearInterval(timer);
      document.removeEventListener("mouseup", finish);
      window.removeEventListener("blur", finish);
      setDragging(false);
      void endVoiceBarDrag().then(setAnchor).catch(() => {});
    };

    void beginVoiceBarDrag()
      .then(() => {
        setDragging(true);
        // ~60 Hz. The cursor query is the cost, and it is a few milliseconds.
        timer = window.setInterval(() => {
          void trackVoiceBarDrag().catch(() => {});
        }, 16);
        document.addEventListener("mouseup", finish);
        window.addEventListener("blur", finish);
      })
      .catch(() => {});
  }

  const level = Math.max(0, Math.min(100, inputLevel.percent));

  return (
    <main
      data-transparent="true"
      className="flex h-screen w-screen items-center justify-center bg-transparent"
    >
      {/*
        Clear glass, not frosted glass. This window is transparent and cut to
        the pill's bounds, so backdrop-filter has nothing in the page to sample
        — the desktop belongs to the compositor. What sells the material is
        therefore actually being see-through, plus a specular rim; every knob
        lives in the --pill-* variables in globals.css, and the frosted profile
        switches on with data-desktop-blur once a compositor blurs behind us.

        No shadow: the window hugs the pill and would slice it off. Width must
        stay intrinsic (inline-flex, no w-full) so the ResizeObserver above
        measures the pill and not the viewport.
      */}
      <div
        ref={shellRef}
        className={[
          "glass-pill rim inline-flex select-none items-center gap-2 rounded-pill",
          "py-1.5 pl-1.5 pr-2",
        ].join(" ")}
        title={hotkey?.trigger ?? ""}
      >
        <button
          className={[
            "press flex h-7 w-7 shrink-0 items-center justify-center rounded-pill",
            recording
              ? "bg-rust text-white shadow-[inset_0_1px_0_0_oklch(1_0_0/0.4)]"
              : "bg-fill text-ink hover:bg-fill-strong shadow-[inset_0_1px_0_0_var(--spec-top)]",
          ].join(" ")}
          onClick={(event) => {
            event.stopPropagation();
            void (recording ? stop() : start());
          }}
          title={recording ? "Stop" : "Talk"}
        >
          {phase === "transcribing" ? (
            <Loader2 size={14} className="animate-spin" />
          ) : recording ? (
            <Square size={12} />
          ) : (
            <Mic size={14} />
          )}
        </button>

        {/* Idle is just the button and the binding, nothing more. */}
        {!expanded ? (
          <span className="t-micro rounded-md bg-fill px-1.5 py-[3px] text-[10.5px] font-medium whitespace-nowrap text-smoke">
            {hotkey?.backend === "unavailable" ? "no hotkey" : hotkey?.trigger || "…"}
          </span>
        ) : null}

        {phase === "listening" ? (
          <span className="flex h-5 items-center gap-[2.5px]" aria-label="input level">
            {[0.45, 0.75, 1, 0.75, 0.45].map((weight, i) => (
              <span
                key={i}
                className="w-[3px] rounded-pill bg-rust transition-all duration-100 ease-glass"
                style={{ height: `${Math.max(3, (level / 100) * 20 * weight)}px` }}
              />
            ))}
          </span>
        ) : null}

        {/* leading-none clips CJK ascenders; 1.35 keeps 中文 and Latin aligned
            on the same baseline without growing the pill. */}
        {expanded && (partial || status) ? (
          <span className="vibrant max-w-[380px] truncate text-[12.5px] leading-[1.35] whitespace-nowrap">
            {partial || status}
          </span>
        ) : null}

        <button
          className={[
            "shrink-0 rounded-md px-0.5 py-1 transition-colors duration-150",
            dragging ? "cursor-grabbing text-ink" : "cursor-grab text-faint hover:text-ink",
          ].join(" ")}
          title={`Drag to move · ${anchor}`}
          onMouseDown={startDrag}
        >
          <GripVertical size={13} />
        </button>

        {expanded ? (
          <button
            className="press flex h-5 w-5 shrink-0 items-center justify-center rounded-pill text-[13px] leading-none text-faint hover:bg-fill hover:text-ink"
            title="Hide"
            onClick={(event) => {
              event.stopPropagation();
              void hideVoiceBar();
            }}
          >
            ×
          </button>
        ) : null}
      </div>
    </main>
  );
}
