"use client";

import {
  Copy,
  Download,
  Mic,
  PanelTop,
  Plus,
  Settings,
  Square,
  Wand2,
} from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { Button } from "@/components/Button";
import { DownloadProgress } from "@/components/DownloadProgress";
import { SetupView } from "@/components/SetupView";
import {
  copyText,
  createSession,
  downloadModelWithProgress,
  getAudioLevel,
  getBootstrap,
  listAudioInputs,
  listSessions,
  listTranscripts,
  pauseModelDownload,
  previewAudio,
  showSettingsWindow,
  showVoiceBar,
  snapshotAudioCapture,
  startAudioCapture,
  stopAudioCapture,
  transcribeAudio,
} from "@/lib/tauri";
import type {
  AudioInputInfo,
  AudioLevelInfo,
  Bootstrap,
  ModelDownloadEvent,
  ModelDownloadState,
  ModelInfo,
  SessionInfo,
  TranscriptInfo,
} from "@/lib/types";
import { languages } from "@/lib/types";

const idleLevel: AudioLevelInfo = { rms: 0, peak: 0, db: -90, percent: 0 };
const previewIntervalMs = 6500;
const previewWindowMs = 5500;

export default function Home() {
  const [bootstrap, setBootstrap] = useState<Bootstrap | null>(null);
  const [sessions, setSessions] = useState<SessionInfo[]>([]);
  const [activeSession, setActiveSession] = useState<SessionInfo | null>(null);
  const [transcripts, setTranscripts] = useState<TranscriptInfo[]>([]);
  const [modelId, setModelId] = useState("fun-asr-nano-2512");
  const [language, setLanguage] = useState("中文");
  const [audioInputs, setAudioInputs] = useState<AudioInputInfo[]>([]);
  const [audioInputId, setAudioInputId] = useState("");
  const [inputLevel, setInputLevel] = useState<AudioLevelInfo>(idleLevel);
  const [status, setStatus] = useState("Ready");
  const [recording, setRecording] = useState(false);
  const [partial, setPartial] = useState("");
  const [busy, setBusy] = useState<string | null>(null);
  const [download, setDownload] = useState<ModelDownloadState | null>(null);
  const recordingSessionIdRef = useRef<string | null>(null);
  const previewTimerRef = useRef<number | null>(null);
  const levelTimerRef = useRef<number | null>(null);
  const previewBusyRef = useRef(false);

  useEffect(() => {
    getBootstrap()
      .then((data) => {
        setBootstrap(data);
        setSessions(data.sessions);
        setModelId(String(data.settings["defaults.model"] || "fun-asr-nano-2512"));
        setLanguage(String(data.settings["defaults.language"] || "中文"));
        if (data.sessions[0]) setActiveSession(data.sessions[0]);
      })
      .catch((error) => setStatus(String(error)));
    void refreshAudioInputs();
  }, []);

  useEffect(() => {
    if (!activeSession) {
      setTranscripts([]);
      return;
    }
    listTranscripts(activeSession.id)
      .then(setTranscripts)
      .catch((error) => setStatus(String(error)));
  }, [activeSession]);

  const groupedSessions = useMemo(() => groupByDate(sessions), [sessions]);
  const activeModel = bootstrap?.models.find((model) => model.id === modelId);
  const defaultInput = audioInputs.find((input) => input.isDefault);

  function selectedRuntime() {
    return activeModel?.backend || "crispasr-gguf-cpu";
  }

  async function refreshAudioInputs() {
    try {
      const devices = await listAudioInputs();
      setAudioInputs(devices);
    } catch {
      setAudioInputs([]);
    }
  }

  async function refreshSessions(selectId?: string) {
    const next = await listSessions();
    setSessions(next);
    if (selectId) {
      setActiveSession(next.find((session) => session.id === selectId) || next[0] || null);
    }
  }

  async function ensureSession(): Promise<SessionInfo> {
    if (activeSession) return activeSession;
    const session = await createSession({
      title: "Session",
      model: modelId,
      language,
      runtime: selectedRuntime(),
    });
    await refreshSessions(session.id);
    return session;
  }

  async function newSession() {
    setBusy("session");
    try {
      const session = await createSession({
        title: `Session ${new Date().toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}`,
        model: modelId,
        language,
        runtime: selectedRuntime(),
      });
      setActiveSession(session);
      await refreshSessions(session.id);
      setTranscripts([]);
    } catch (error) {
      setStatus(String(error));
    } finally {
      setBusy(null);
    }
  }

  function handleDownloadEvent(event: ModelDownloadEvent) {
    if (event.data.modelId !== modelId) return;
    if (event.event === "started" || event.event === "progress") {
      setDownload({
        modelId: event.data.modelId,
        active: true,
        paused: false,
        downloadedBytes: event.data.downloadedBytes,
        totalBytes: event.data.totalBytes,
        message: "Downloading model from Hugging Face",
      });
      setStatus("Downloading model from Hugging Face.");
    } else if (event.event === "paused") {
      setDownload({
        modelId: event.data.modelId,
        active: false,
        paused: true,
        downloadedBytes: event.data.downloadedBytes,
        totalBytes: event.data.totalBytes,
        message: "Download paused",
      });
      setStatus("Download paused.");
    } else if (event.event === "finished") {
      setDownload({
        modelId: event.data.modelId,
        active: false,
        paused: false,
        downloadedBytes: event.data.downloadedBytes,
        totalBytes: event.data.totalBytes,
        message: "Model installed",
      });
      setStatus("Model installed.");
    } else if (event.event === "error") {
      setDownload((current) =>
        current
          ? { ...current, active: false, message: event.data.error }
          : {
              modelId: event.data.modelId,
              active: false,
              paused: false,
              downloadedBytes: 0,
              totalBytes: null,
              message: event.data.error,
            },
      );
      setStatus(event.data.error);
    }
  }

  async function installModel() {
    setBusy("download");
    setStatus("Downloading model from Hugging Face.");
    try {
      const model = await downloadModelWithProgress(modelId, handleDownloadEvent);
      setBootstrap((current) =>
        current
          ? {
              ...current,
              models: current.models.map((item) => (item.id === model.id ? model : item)),
            }
          : current,
      );
      setStatus(model.status === "installed" ? "Model installed." : "Download paused.");
    } catch (error) {
      setStatus(String(error));
    } finally {
      setBusy(null);
    }
  }

  async function pauseDownload() {
    setDownload((current) => (current ? { ...current, message: "Pausing download..." } : current));
    await pauseModelDownload(modelId);
  }

  async function startRecording() {
    setBusy("capture");
    try {
      const session = await ensureSession();
      await startAudioCapture(audioInputId || undefined);
      recordingSessionIdRef.current = session.id;
      setRecording(true);
      setPartial("");
      setStatus("Listening");
      void refreshAudioInputs();
      levelTimerRef.current = window.setInterval(() => {
        void getAudioLevel()
          .then(setInputLevel)
          .catch((error) => setStatus(String(error)));
      }, 120);
      previewTimerRef.current = window.setInterval(() => {
        void runPreview(session.id);
      }, previewIntervalMs);
    } catch (error) {
      setRecording(false);
      recordingSessionIdRef.current = null;
      setStatus(String(error));
    } finally {
      setBusy(null);
    }
  }

  async function runPreview(sessionId: string) {
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
        sessionId,
        audioBase64: capture.audioBase64,
        modelId,
        language,
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

  async function stopRecording() {
    const sessionId = recordingSessionIdRef.current || activeSession?.id;
    if (!sessionId) {
      setRecording(false);
      return;
    }
    clearRecordingTimers();
    setBusy("transcribe");
    setRecording(false);
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
        sessionId,
        audioBase64: capture.audioBase64,
        modelId,
        language,
      });
      if (result.transcript) {
        setTranscripts((current) => [...current, result.transcript as TranscriptInfo]);
        setPartial("");
      }
      setStatus(result.error || "Saved");
    } catch (error) {
      setStatus(String(error));
    } finally {
      recordingSessionIdRef.current = null;
      setBusy(null);
    }
  }

  async function copyTranscript(text: string) {
    const result = await copyText(text);
    setStatus(result.message);
  }

  if (!bootstrap) {
    return (
      <main className="flex min-h-screen items-center justify-center bg-panel text-sm text-smoke">
        Loading Fun ASR Desktop
      </main>
    );
  }

  if (!bootstrap.setup_complete) {
    return (
      <SetupView
        bootstrap={bootstrap}
        onDone={() => setBootstrap({ ...bootstrap, setup_complete: true })}
        onModelsChanged={(models: ModelInfo[]) => setBootstrap({ ...bootstrap, models })}
      />
    );
  }

  return (
    <main className="grid h-dvh min-h-0 grid-cols-[260px_minmax(0,1fr)] bg-panel lg:grid-cols-[280px_minmax(0,1fr)]">
      <aside className="flex min-h-0 flex-col border-r border-line bg-[#e9ece5]">
        <div className="border-b border-line p-4">
          <div className="mb-4 flex items-center justify-between">
            <div>
              <h1 className="text-lg font-semibold">Fun ASR</h1>
              <p className="text-xs text-smoke">0.0.2 Linux</p>
            </div>
            <Button variant="ghost" className="h-9 w-9 px-0" title="Settings" onClick={showSettingsWindow}>
              <Settings size={17} />
            </Button>
          </div>
          <Button className="w-full" icon={<Plus size={16} />} disabled={busy === "session"} onClick={newSession}>
            New Session
          </Button>
        </div>

        <div className="scrollbar-thin min-h-0 flex-1 overflow-y-auto p-3">
          {groupedSessions.map(([date, dateSessions]) => (
            <section key={date} className="mb-4">
              <p className="mb-2 px-2 text-xs font-semibold uppercase text-smoke">{date}</p>
              <div className="space-y-1">
                {dateSessions.map((session) => (
                  <button
                    key={session.id}
                    className={[
                      "block w-full rounded-md px-3 py-2 text-left text-sm transition",
                      activeSession?.id === session.id
                        ? "bg-paper font-medium text-ink shadow-sm"
                        : "text-smoke hover:bg-paper/60 hover:text-ink",
                    ].join(" ")}
                    onClick={() => setActiveSession(session)}
                  >
                    <span className="block truncate">{session.title}</span>
                    <span className="mt-1 block text-xs text-smoke">{session.language}</span>
                  </button>
                ))}
              </div>
            </section>
          ))}
        </div>
      </aside>

      <section className="grid min-h-0 grid-rows-[auto_1fr_auto]">
        <header className="flex flex-wrap items-start justify-between gap-3 border-b border-line bg-paper px-5 py-3">
          <div className="flex min-w-0 items-center gap-3">
            <div className="min-w-0">
              <h2 className="truncate text-base font-semibold">{activeSession?.title || "No session"}</h2>
              <p className="text-xs text-smoke">{status}</p>
              {download?.modelId === modelId ? (
                <div className="mt-2 w-[min(520px,calc(100vw-340px))] min-w-[280px]">
                  <DownloadProgress download={download} onPause={download.active ? pauseDownload : undefined} />
                </div>
              ) : null}
            </div>
          </div>

          <div className="flex max-w-full flex-wrap items-center justify-end gap-2">
            <Select value={modelId} onChange={setModelId}>
              {bootstrap.models.map((model) => (
                <option key={model.id} value={model.id}>
                  {model.name}
                </option>
              ))}
            </Select>
            <Select value={language} onChange={setLanguage}>
              {languages.map((item) => (
                <option key={item} value={item}>
                  {item}
                </option>
              ))}
            </Select>
            <Select value={audioInputId} onChange={setAudioInputId} disabled={recording}>
              <option value="">{defaultInput ? `Default mic - ${defaultInput.name}` : "Default mic"}</option>
              {audioInputs.map((input) => (
                <option key={input.id} value={input.id}>
                  {input.name}
                </option>
              ))}
            </Select>
            <Button
              icon={<Download size={16} />}
              disabled={busy !== null || activeModel?.status === "installed"}
              onClick={installModel}
            >
              {activeModel?.status === "installed" ? "Installed" : activeModel?.status === "paused" || download?.paused ? "Resume" : "Download"}
            </Button>
            <Button icon={<PanelTop size={16} />} onClick={showVoiceBar}>
              Voice Bar
            </Button>
          </div>
        </header>

        <div className="scrollbar-thin min-h-0 overflow-y-auto px-6 py-5">
          {transcripts.length === 0 && !partial ? (
            <div className="flex h-full items-center justify-center text-center">
              <div>
                <Mic className="mx-auto mb-4 text-rust" size={34} />
                <p className="text-lg font-semibold">Start a local transcription session</p>
                <p className="mt-2 max-w-md text-sm leading-6 text-smoke">
                  Saved transcripts appear here by date. Use the model and language controls before recording.
                </p>
              </div>
            </div>
          ) : (
            <div className="mx-auto max-w-3xl space-y-4">
              {transcripts.map((transcript) => (
                <article key={transcript.id} className="rounded-lg border border-line bg-paper p-4 shadow-sm">
                  <div className="mb-3 flex items-center justify-between gap-3">
                    <div className="text-xs text-smoke">
                      {new Date(transcript.created_at).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })} -{" "}
                      {transcript.language}
                    </div>
                    <Button
                      variant="ghost"
                      className="h-8 w-8 px-0"
                      title="Copy"
                      onClick={() => copyTranscript(transcript.text)}
                    >
                      <Copy size={15} />
                    </Button>
                  </div>
                  <p className="whitespace-pre-wrap text-[15px] leading-7">{transcript.text}</p>
                </article>
              ))}
              {partial ? (
                <article className="rounded-lg border border-dashed border-rust/50 bg-[#f8f3e9] p-4">
                  <div className="mb-2 flex items-center gap-2 text-xs font-medium text-rust">
                    <Wand2 size={14} />
                    Live partial
                  </div>
                  <p className="whitespace-pre-wrap text-[15px] leading-7">{partial}</p>
                </article>
              ) : null}
            </div>
          )}
        </div>

        <footer className="border-t border-line bg-paper p-4">
          <div className="mx-auto flex max-w-4xl flex-wrap items-center gap-3">
            <Button
              variant={recording ? "danger" : "primary"}
              icon={recording ? <Square size={16} /> : <Mic size={16} />}
              disabled={busy !== null && busy !== "transcribe"}
              onClick={recording ? stopRecording : startRecording}
            >
              {recording ? "Stop" : "Talk"}
            </Button>
            <InputLevel level={inputLevel} active={recording} />
            <div className="min-w-[220px] flex-1 text-sm text-smoke">
              {recording
                ? "Speak normally. The meter should move while you talk."
                : audioInputs.length
                  ? "Select a microphone, then press Talk."
                  : "No microphone input detected by the native audio backend."}
            </div>
          </div>
        </footer>
      </section>
    </main>
  );
}

function InputLevel({ level, active }: { level: AudioLevelInfo; active: boolean }) {
  return (
    <div className="flex min-w-[220px] items-center gap-3 rounded-md border border-line bg-panel px-3 py-2">
      <div className="flex-1">
        <div className="mb-1 flex items-center justify-between text-xs text-smoke">
          <span>Input</span>
          <span>{active ? formatDb(level.db) : "idle"}</span>
        </div>
        <div className="h-2 overflow-hidden rounded-full bg-[#d5dccf]">
          <div className="h-full rounded-full bg-moss transition-all" style={{ width: `${active ? level.percent : 0}%` }} />
        </div>
      </div>
    </div>
  );
}

function formatDb(value: number) {
  if (!Number.isFinite(value)) return "-90 dB";
  return `${Math.round(value)} dB`;
}

function Select({
  value,
  onChange,
  children,
  disabled,
}: {
  value: string;
  onChange: (value: string) => void;
  children: React.ReactNode;
  disabled?: boolean;
}) {
  return (
    <select
      className="h-9 max-w-[190px] rounded-md border border-line bg-paper px-2 text-sm outline-none focus:border-cobalt disabled:opacity-60"
      value={value}
      disabled={disabled}
      onChange={(event) => onChange(event.target.value)}
    >
      {children}
    </select>
  );
}

function groupByDate(sessions: SessionInfo[]) {
  const map = new Map<string, SessionInfo[]>();
  for (const session of sessions) {
    const group = map.get(session.date_key) || [];
    group.push(session);
    map.set(session.date_key, group);
  }
  return Array.from(map.entries());
}
