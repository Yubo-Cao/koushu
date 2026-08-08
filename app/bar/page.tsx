"use client";

import { Loader2, Mic, Square } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { DEFAULT_BACKEND } from "@/lib/backends";
import {
  anchorVoiceBar,
  autoPasteText,
  createSession,
  getAudioLevel,
  hideVoiceBar,
  listAudioInputs,
  resizeVoiceBar,
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

/**
 * Pick the screen edge a drag ended nearest to.
 *
 * A layer-shell surface has no free-floating position — the compositor places
 * it from its anchor — so "drag it anywhere" is something this window can
 * never honour. Snapping to the nearest edge is the honest version of that
 * gesture, and is why dragging previously appeared to do nothing.
 */
function nearestAnchor(x: number, y: number, w: number, h: number): string {
  const vertical = y < h / 2 ? "top" : "bottom";
  const horizontal = x < w / 3 ? "left" : x > (w * 2) / 3 ? "right" : "center";
  return `${vertical}-${horizontal}`;
}

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

  const onDragEnd = useCallback((event: MouseEvent) => {
    setDragging(false);
    const next = nearestAnchor(
      event.screenX,
      event.screenY,
      window.screen.width,
      window.screen.height,
    );
    setAnchor(next);
    void anchorVoiceBar(next, 18).catch(() => {});
  }, []);

  useEffect(() => {
    if (!dragging) return;
    window.addEventListener("mouseup", onDragEnd);
    return () => window.removeEventListener("mouseup", onDragEnd);
  }, [dragging, onDragEnd]);

  const level = Math.max(0, Math.min(100, inputLevel.percent));

  return (
    <main className="flex h-screen w-screen items-center justify-center bg-transparent">
      <div
        ref={shellRef}
        onMouseDown={(event) => {
          if (event.button === 0) setDragging(true);
        }}
        className={[
          "inline-flex select-none items-center gap-2 rounded-full border border-black/10",
          "bg-paper/95 py-1.5 pl-1.5 pr-3 shadow-lg",
          dragging ? "cursor-grabbing" : "cursor-grab",
        ].join(" ")}
        title={`${hotkey?.trigger ?? ""} · drag to an edge to dock · ${anchor}`}
      >
        <button
          className={[
            "flex h-7 w-7 shrink-0 items-center justify-center rounded-full transition-colors",
            recording ? "bg-rust text-white" : "bg-black/5 text-ink hover:bg-black/10",
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
          <span className="whitespace-nowrap text-[11px] font-medium text-smoke">
            {hotkey?.backend === "unavailable" ? "no hotkey" : hotkey?.trigger || "…"}
          </span>
        ) : null}

        {phase === "listening" ? (
          <span className="flex h-5 items-center gap-[2px]" aria-label="input level">
            {[0.45, 0.75, 1, 0.75, 0.45].map((weight, i) => (
              <span
                key={i}
                className="w-[3px] rounded-full bg-rust/85 transition-all duration-100"
                style={{ height: `${Math.max(3, (level / 100) * 20 * weight)}px` }}
              />
            ))}
          </span>
        ) : null}

        {expanded && (partial || status) ? (
          <span className="max-w-[380px] truncate whitespace-nowrap text-[12px] leading-none text-ink">
            {partial || status}
          </span>
        ) : null}

        {expanded ? (
          <button
            className="shrink-0 text-[13px] leading-none text-smoke/60 hover:text-ink"
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
