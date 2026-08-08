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
import { Confetti } from "@/components/Confetti";
import { SetupView } from "@/components/SetupView";
import { DEFAULT_BACKEND } from "@/lib/backends";
import {
  copyText,
  createSession,
  formatTranscript,
  getLlmSettings,
  downloadModelWithProgress,
  getAudioLevel,
  getBootstrap,
  listAudioInputs,
  listSessions,
  listTranscripts,
  pauseModelDownload,
  showSettingsWindow,
  showVoiceBar,
  startStreamingTranscription,
  stopStreamingTranscription,
  startAudioCapture,
  stopAudioCapture,
  transcribeAudio,
} from "@/lib/tauri";
import type {
  AudioInputInfo,
  LlmSettings,
  StreamingEvent,
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
// Committed preview segments, joined with the live partial for display.
function joinPreview(segments: string[], partial: string): string {
  return [...segments, partial].filter((part) => part.trim()).join(" ");
}

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
  // Fired once, when the very first transcript lands — the moment the app
  // stops being a promise and starts being a tool.
  const [celebrate, setCelebrate] = useState(false);
  const recordingSessionIdRef = useRef<string | null>(null);
  const segmentsRef = useRef<string[]>([]);
  const [llm, setLlm] = useState<LlmSettings | null>(null);
  // Streaming Markdown per transcript while a format pass is in flight.
  const [formatting, setFormatting] = useState<Record<string, string>>({});
  const [formatError, setFormatError] = useState<Record<string, string>>({});
  const levelTimerRef = useRef<number | null>(null);

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
    getLlmSettings().then(setLlm).catch(() => setLlm(null));
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
    return activeModel?.backend || DEFAULT_BACKEND;
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
      segmentsRef.current = [];
      await startStreamingTranscription(modelId, handleStreamingEvent);
    } catch (error) {
      setRecording(false);
      recordingSessionIdRef.current = null;
      setStatus(String(error));
    } finally {
      setBusy(null);
    }
  }

  async function runFormat(transcriptId: string, text: string) {
    setFormatError((current) => ({ ...current, [transcriptId]: "" }));
    setFormatting((current) => ({ ...current, [transcriptId]: "" }));
    try {
      await formatTranscript({ transcriptId, text }, (event) => {
        if (event.event === "delta") {
          setFormatting((current) => ({
            ...current,
            [transcriptId]: (current[transcriptId] || "") + event.data.text,
          }));
        } else if (event.event === "error") {
          setFormatError((current) => ({ ...current, [transcriptId]: event.data.error }));
        }
      });
      // Re-read so the stored formatted text becomes the source of truth.
      if (activeSession) setTranscripts(await listTranscripts(activeSession.id));
    } catch (error) {
      setFormatError((current) => ({ ...current, [transcriptId]: String(error) }));
    } finally {
      setFormatting((current) => {
        const next = { ...current };
        delete next[transcriptId];
        return next;
      });
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

  function clearRecordingTimers() {
    void stopStreamingTranscription().catch(() => {});
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
      if (result.trial?.firstTranscript) setCelebrate(true);
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
      <main className="flex min-h-screen items-center justify-center text-sm text-smoke">
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
    /*
      Chrome floats, content flows under it. The sidebar header, the toolbar and
      the transcript bar are sticky glass inside their own scroll containers, so
      what they blur is the real content sliding beneath them — the one place in
      this app where backdrop-filter has something to work with. An opaque strip
      that merely reserves space would look the same standing still and dead in
      motion.
    */
    <>
      <Confetti fire={celebrate} onDone={() => setCelebrate(false)} />
      <main className="grid h-dvh min-h-0 grid-cols-[254px_minmax(0,1fr)] lg:grid-cols-[282px_minmax(0,1fr)]">
      <aside className="hairline-r relative min-h-0">
        <div className="scrollbar-thin absolute inset-0 flex flex-col overflow-y-auto">
          <div className="glass-chrome sticky top-0 z-20 px-4 pt-4 pb-3">
            <div className="mb-3.5 flex items-center justify-between gap-2">
              <div className="min-w-0">
                <h1 className="t-title text-[17px] font-semibold">Fun ASR</h1>
                <p className="t-micro text-[11px] text-smoke">0.0.2 Linux</p>
              </div>
              <Button
                variant="ghost"
                className="h-9 w-9 px-0"
                title="Settings"
                onClick={showSettingsWindow}
              >
                <Settings size={17} />
              </Button>
            </div>
            <Button
              variant="primary"
              className="w-full"
              icon={<Plus size={16} />}
              disabled={busy === "session"}
              onClick={newSession}
            >
              New Session
            </Button>
          </div>

          <div className="flex-1 px-3 pt-3 pb-4">
            {groupedSessions.map(([date, dateSessions]) => (
              <section key={date} className="mb-5">
                <p className="t-micro mb-2 px-2 text-[10.5px] font-semibold tracking-wider uppercase text-faint">
                  {date}
                </p>
                <div className="space-y-0.5">
                  {dateSessions.map((session) => (
                    <button
                      key={session.id}
                      className={[
                        "press block w-full rounded-md px-3 py-2 text-left",
                        activeSession?.id === session.id
                          ? "glass rim font-medium text-ink"
                          : "text-ink-2 hover:bg-fill",
                      ].join(" ")}
                      onClick={() => setActiveSession(session)}
                    >
                      <span className="block truncate text-[13.5px]">{session.title}</span>
                      <span className="t-micro mt-0.5 block text-[11px] text-smoke">
                        {session.language}
                      </span>
                    </button>
                  ))}
                </div>
              </section>
            ))}
          </div>
        </div>
      </aside>

      <section className="relative min-h-0">
        <div className="scrollbar-thin absolute inset-0 flex flex-col overflow-y-auto">
          <header className="glass-chrome sticky top-0 z-20 flex flex-wrap items-start justify-between gap-3 px-5 py-3">
            <div className="flex min-w-0 items-center gap-3">
              <div className="min-w-0">
                <h2 className="t-head truncate text-[15px] font-semibold">
                  {activeSession?.title || "No session"}
                </h2>
                <p className="t-micro text-[11.5px] text-smoke">{status}</p>
                {download?.modelId === modelId ? (
                  <div className="mt-2 w-[min(520px,calc(100vw-340px))] min-w-[280px]">
                    <DownloadProgress
                      download={download}
                      onPause={download.active ? pauseDownload : undefined}
                    />
                  </div>
                ) : null}
              </div>
            </div>

            <div className="flex max-w-full flex-wrap items-center justify-end gap-2">
              <Select value={modelId} onChange={setModelId}>
                {bootstrap.models.map((model) => (
                  <option key={model.id} value={model.id}>
                    {model.name.replace(/\s*\(.*\)$/, "")}
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
                <option value="">
                  {defaultInput ? `Default mic - ${defaultInput.name}` : "Default mic"}
                </option>
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
                {activeModel?.status === "installed"
                  ? "Installed"
                  : activeModel?.status === "paused" || download?.paused
                    ? "Resume"
                    : "Download"}
              </Button>
              <Button icon={<PanelTop size={16} />} onClick={showVoiceBar}>
                Voice Bar
              </Button>
            </div>
          </header>

          <div className="flex-1 px-6 py-6">
            {transcripts.length === 0 && !partial ? (
              <div className="flex h-full items-center justify-center text-center">
                <div className="max-w-md">
                  <div className="glass rim mx-auto mb-5 flex h-16 w-16 items-center justify-center rounded-[22px]">
                    <Mic className="text-accent" size={28} />
                  </div>
                  <p className="t-title text-[19px] font-semibold">
                    Start a local transcription session
                  </p>
                  <p className="t-body mt-2 text-[13.5px] text-smoke">
                    Saved transcripts appear here by date. Use the model and language controls
                    before recording.
                  </p>
                </div>
              </div>
            ) : (
              <div className="mx-auto max-w-3xl space-y-3.5">
                {transcripts.map((transcript) => (
                  <article key={transcript.id} className="glass rim rounded-lg p-4">
                    <div className="mb-3 flex items-center justify-between gap-3">
                      <div className="tnum t-micro text-[11.5px] text-smoke">
                        {new Date(transcript.created_at).toLocaleTimeString([], {
                          hour: "2-digit",
                          minute: "2-digit",
                        })}{" "}
                        · {transcript.language}
                      </div>
                      <div className="flex items-center gap-1">
                        <Button
                          variant="ghost"
                          className="h-8 px-2 text-xs"
                          title={
                            llm?.baseUrl ? "Format as Markdown" : "Configure an LLM in Settings first"
                          }
                          disabled={!llm?.baseUrl || transcript.id in formatting}
                          onClick={() => void runFormat(transcript.id, transcript.text)}
                        >
                          <Wand2 size={14} />
                          <span className="ml-1">
                            {transcript.id in formatting
                              ? "Formatting"
                              : transcript.formatted_text
                                ? "Redo"
                                : "Format"}
                          </span>
                        </Button>
                        <Button
                          variant="ghost"
                          className="h-8 w-8 px-0"
                          title="Copy"
                          onClick={() => copyTranscript(transcript.formatted_text || transcript.text)}
                        >
                          <Copy size={15} />
                        </Button>
                      </div>
                    </div>
                    <p className="t-body whitespace-pre-wrap text-[14.5px]">{transcript.text}</p>
                    {transcript.id in formatting || transcript.formatted_text ? (
                      <div className="mt-3 border-t border-line-soft pt-3">
                        <div className="t-micro mb-2 flex items-center gap-2 text-[11.5px] font-medium text-moss">
                          <Wand2 size={13} />
                          {transcript.id in formatting
                            ? "Formatting"
                            : `Formatted · ${transcript.formatted_preset || "typeset"}`}
                        </div>
                        <p className="t-body whitespace-pre-wrap text-[14.5px]">
                          {formatting[transcript.id] ?? transcript.formatted_text}
                        </p>
                      </div>
                    ) : null}
                    {formatError[transcript.id] ? (
                      <p className="mt-2 text-[13px] text-rust">{formatError[transcript.id]}</p>
                    ) : null}
                  </article>
                ))}
                {partial ? (
                  <article className="glass rim rounded-lg p-4 ring-1 ring-rust/25">
                    <div className="t-micro mb-2 flex items-center gap-2 text-[11.5px] font-medium text-rust">
                      <span className="flex h-1.5 w-1.5 rounded-pill bg-rust" />
                      Live partial
                    </div>
                    <p className="t-body whitespace-pre-wrap text-[14.5px]">{partial}</p>
                  </article>
                ) : null}
              </div>
            )}
          </div>

          <footer className="glass-chrome sticky bottom-0 z-20 mt-auto px-4 py-3">
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
              <div className="min-w-[200px] flex-1 text-[12.5px] text-smoke">
                {recording
                  ? "Speak normally. The meter should move while you talk."
                  : audioInputs.length
                    ? "Select a microphone, then press Talk."
                    : "No microphone input detected by the native audio backend."}
              </div>
            </div>
          </footer>
        </div>
      </section>
      </main>
    </>
  );
}

function InputLevel({ level, active }: { level: AudioLevelInfo; active: boolean }) {
  return (
    <div className="glass rim flex min-w-[210px] items-center gap-3 rounded-md px-3 py-1.5">
      <div className="flex-1">
        <div className="t-micro mb-1 flex items-center justify-between text-[11px] text-smoke">
          <span>Input</span>
          <span className="tnum">{active ? formatDb(level.db) : "idle"}</span>
        </div>
        <div className="h-1.5 overflow-hidden rounded-pill bg-track">
          <div
            className="h-full rounded-pill bg-moss transition-all duration-100 ease-glass"
            style={{ width: `${active ? level.percent : 0}%` }}
          />
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
      className="field h-9 max-w-[190px] rounded-pill pl-3.5 pr-8 text-[13px]"
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
