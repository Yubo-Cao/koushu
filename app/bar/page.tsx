"use client";

import { ClipboardCheck, GripHorizontal, Mic, Square, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { Button } from "@/components/Button";
import { DEFAULT_BACKEND } from "@/lib/backends";
import {
  autoPasteText,
  createSession,
  getAudioLevel,
  hideVoiceBar,
  listAudioInputs,
  previewAudio,
  snapshotAudioCapture,
  startAudioCapture,
  stopAudioCapture,
  transcribeAudio,
} from "@/lib/tauri";
import type { AudioInputInfo, AudioLevelInfo } from "@/lib/types";

const idleLevel: AudioLevelInfo = { rms: 0, peak: 0, db: -90, percent: 0 };
const previewIntervalMs = 6500;
const previewWindowMs = 5500;

export default function VoiceBar() {
  const [recording, setRecording] = useState(false);
  const [partial, setPartial] = useState("");
  const [status, setStatus] = useState("Ready");
  const [busy, setBusy] = useState(false);
  const [audioInputs, setAudioInputs] = useState<AudioInputInfo[]>([]);
  const [audioInputId, setAudioInputId] = useState("");
  const [inputLevel, setInputLevel] = useState<AudioLevelInfo>(idleLevel);
  const sessionIdRef = useRef<string | null>(null);
  const previewTimerRef = useRef<number | null>(null);
  const levelTimerRef = useRef<number | null>(null);
  const previewBusyRef = useRef(false);

  useEffect(() => {
    void refreshAudioInputs();
  }, []);

  async function refreshAudioInputs() {
    try {
      const devices = await listAudioInputs();
      setAudioInputs(devices);
    } catch {
      setAudioInputs([]);
    }
  }

  async function drag() {
    const { getCurrentWindow } = await import("@tauri-apps/api/window");
    await getCurrentWindow().startDragging();
  }

  async function start() {
    setBusy(true);
    try {
      const session = await createSession({
        title: `Voice Bar ${new Date().toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}`,
        model: "fun-asr-nano-2512",
        language: "中文",
        runtime: DEFAULT_BACKEND,
      });
      await startAudioCapture(audioInputId || undefined);
      sessionIdRef.current = session.id;
      void refreshAudioInputs();
      setPartial("");
      setRecording(true);
      setStatus("Listening");
      levelTimerRef.current = window.setInterval(() => {
        void getAudioLevel()
          .then(setInputLevel)
          .catch((error) => setStatus(String(error)));
      }, 120);
      previewTimerRef.current = window.setInterval(runPreview, previewIntervalMs);
    } catch (error) {
      setRecording(false);
      sessionIdRef.current = null;
      setStatus(String(error));
    } finally {
      setBusy(false);
    }
  }

  async function runPreview() {
    if (previewBusyRef.current) return;
    previewBusyRef.current = true;
    try {
      const capture = await snapshotAudioCapture(previewWindowMs);
      setInputLevel(capture);
      if (capture.durationMs < 2500 || !capture.speechLike) {
        setStatus(`Listening · input ${formatDb(capture.db)}`);
        return;
      }
      setStatus(`Updating preview · input ${formatDb(capture.db)}`);
      const result = await previewAudio({
        sessionId: sessionIdRef.current || undefined,
        audioBase64: capture.audioBase64,
        modelId: "fun-asr-nano-2512",
        language: "中文",
      });
      if (result.text) setPartial(result.text);
      if (result.error) setStatus(result.error);
    } catch (error) {
      setStatus(String(error));
    } finally {
      previewBusyRef.current = false;
    }
  }

  function clearRecordingTimers() {
    if (previewTimerRef.current) window.clearInterval(previewTimerRef.current);
    previewTimerRef.current = null;
    if (levelTimerRef.current) window.clearInterval(levelTimerRef.current);
    levelTimerRef.current = null;
  }

  async function stop() {
    setBusy(true);
    setRecording(false);
    clearRecordingTimers();
    setStatus("Transcribing");
    try {
      const capture = await stopAudioCapture();
      setInputLevel(idleLevel);
      if (!capture.speechLike) {
        setPartial("");
        setStatus(`No voice detected · input ${formatDb(capture.db)}`);
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
        setStatus(result.error || "No transcript returned.");
      }
    } catch (error) {
      setStatus(String(error));
    } finally {
      sessionIdRef.current = null;
      setBusy(false);
    }
  }

  const defaultInput = audioInputs.find((input) => input.isDefault);

  return (
    <main className="h-screen bg-transparent p-3">
      <div className="flex h-full min-w-0 items-center gap-3 rounded-lg border border-line bg-paper px-3 shadow-lg">
        <button
          className="flex h-12 w-8 items-center justify-center rounded-md text-smoke hover:bg-black/5"
          title="Drag"
          onMouseDown={(event) => {
            if (event.buttons === 1) void drag();
          }}
        >
          <GripHorizontal size={18} />
        </button>

        <Button
          variant={recording ? "danger" : "primary"}
          icon={recording ? <Square size={16} /> : <Mic size={16} />}
          disabled={busy && !recording}
          onClick={recording ? stop : start}
        >
          {recording ? "Stop" : "Talk"}
        </Button>

        <select
          className="h-9 max-w-[170px] rounded-md border border-line bg-paper px-2 text-sm outline-none focus:border-cobalt"
          value={audioInputId}
          disabled={recording}
          onChange={(event) => setAudioInputId(event.target.value)}
        >
          <option value="">{defaultInput ? `Default - ${defaultInput.name}` : "Default mic"}</option>
          {audioInputs.map((input) => (
            <option key={input.id} value={input.id}>
              {input.name}
            </option>
          ))}
        </select>

        <InputLevel level={inputLevel} active={recording} />

        <div className="min-w-0 flex-1">
          <div className="mb-1 flex items-center gap-2 text-xs font-medium text-smoke">
            <ClipboardCheck size={13} />
            {status}
          </div>
          <p className="truncate text-sm text-ink">{partial || "Waiting for speech"}</p>
        </div>

        <Button variant="ghost" className="h-9 w-9 px-0" title="Close" onClick={hideVoiceBar}>
          <X size={17} />
        </Button>
      </div>
    </main>
  );
}

function InputLevel({ level, active }: { level: AudioLevelInfo; active: boolean }) {
  return (
    <div className="w-28">
      <div className="mb-1 flex items-center justify-between text-xs text-smoke">
        <span>Input</span>
        <span>{active ? formatDb(level.db) : "idle"}</span>
      </div>
      <div className="h-2 overflow-hidden rounded-full bg-[#d5dccf]">
        <div className="h-full rounded-full bg-moss transition-all" style={{ width: `${active ? level.percent : 0}%` }} />
      </div>
    </div>
  );
}

function formatDb(value: number) {
  if (!Number.isFinite(value)) return "-90 dB";
  return `${Math.round(value)} dB`;
}
