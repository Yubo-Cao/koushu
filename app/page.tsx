"use client";

import {
  Boxes,
  Copy,
  Download,
  Languages,
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
      <main className="grid h-dvh min-h-0 grid-cols-[238px_minmax(0,1fr)] lg:grid-cols-[266px_minmax(0,1fr)]">
      <aside className="hairline-r relative min-h-0">
        <div className="scrollbar-thin absolute inset-0 flex flex-col overflow-y-auto">
          <div className="glass-chrome sticky top-0 z-20 px-3 pt-3 pb-2.5">
            {/* The version was a second line under the title, which cost 18px of
                sticky header for a string nobody reads twice. Beside the name it
                costs nothing and still answers "which build is this". */}
            <div className="mb-2.5 flex items-center justify-between gap-2 pl-1">
              <div className="flex min-w-0 items-baseline gap-2">
                <h1 className="t-title text-head font-semibold">Fun ASR</h1>
                <span className="tnum t-micro text-meta text-faint">0.0.2</span>
              </div>
              <Button
                variant="ghost"
                size="sm"
                className="w-[26px] px-0"
                title="Settings"
                onClick={showSettingsWindow}
              >
                <Settings size={16} />
              </Button>
            </div>
            <Button
              variant="primary"
              className="w-full"
              icon={<Plus size={15} />}
              disabled={busy === "session"}
              onClick={newSession}
            >
              New Session
            </Button>
          </div>

          {/* One line per session, not two. The language was a full second line
              at 11px/1.6 under every title, which made a row 57px tall and put
              eight sessions on a screen that comfortably holds fifteen. It is
              secondary information, so it sits where secondary information goes
              — trailing, quiet, on the same line. */}
          <div className="flex-1 px-2 pt-2.5 pb-4">
            {groupedSessions.map(([date, dateSessions]) => (
              <section key={date} className="mb-3.5">
                <p className="t-micro mb-1 px-2.5 text-meta font-semibold tracking-wider uppercase text-faint">
                  {date}
                </p>
                <div className="space-y-px">
                  {dateSessions.map((session) => (
                    <button
                      key={session.id}
                      className={[
                        "press flex w-full items-center gap-2 rounded-md px-2.5 py-1.5 text-left",
                        activeSession?.id === session.id
                          ? "glass rim font-medium text-ink"
                          : "text-ink-2 hover:bg-fill",
                      ].join(" ")}
                      title={session.title}
                      onClick={() => setActiveSession(session)}
                    >
                      <span className="min-w-0 flex-1 truncate text-ctl">{session.title}</span>
                      <span className="t-micro shrink-0 text-meta text-faint">
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
          {/*
            The microphone picker used to live here, three pills from the meter
            that tells you whether it is working and a whole window away from
            the Talk button the footer text told you to press next. It now sits
            in the footer beside both. That is the grouping rule — put a control
            next to what it affects — and it is also what fixed the truncation:
            the mic name is the one value here with no upper bound, and taking
            it out of the toolbar gave the two that remain room to show their
            values in full.

            The Download button renders only when there is something to
            download. A permanently disabled "Installed" button is 110px of
            toolbar spent on a state the status line already reports.
          */}
          <header className="glass-chrome sticky top-0 z-20 px-4 py-2.5">
            <div className="flex items-center gap-4">
              <div className="min-w-0 flex-1">
                <h2 className="t-head truncate text-head font-semibold">
                  {activeSession?.title || "No session"}
                </h2>
                <p className="t-micro truncate text-meta text-smoke">{status}</p>
              </div>

              <div className="flex flex-wrap items-center justify-end gap-1.5">
                <SelectField
                  icon={<Boxes size={13} />}
                  title="Transcription model"
                  className="w-[176px]"
                  value={modelId}
                  onChange={setModelId}
                >
                  {bootstrap.models.map((model) => (
                    <option key={model.id} value={model.id}>
                      {model.name.replace(/\s*\(.*\)$/, "")}
                    </option>
                  ))}
                </SelectField>
                <SelectField
                  icon={<Languages size={13} />}
                  title="Transcription language"
                  className="w-[122px]"
                  value={language}
                  onChange={setLanguage}
                >
                  {languages.map((item) => (
                    <option key={item} value={item}>
                      {item}
                    </option>
                  ))}
                </SelectField>
                {activeModel?.status !== "installed" ? (
                  <Button
                    icon={<Download size={15} />}
                    disabled={busy !== null}
                    onClick={installModel}
                  >
                    {activeModel?.status === "paused" || download?.paused
                      ? "Resume"
                      : "Download"}
                  </Button>
                ) : null}
                <span className="ctl-sep" aria-hidden />
                <Button icon={<PanelTop size={15} />} onClick={showVoiceBar}>
                  Voice Bar
                </Button>
              </div>
            </div>

            {download?.modelId === modelId ? (
              <div className="mt-2.5 max-w-[560px]">
                <DownloadProgress
                  download={download}
                  onPause={download.active ? pauseDownload : undefined}
                />
              </div>
            ) : null}
          </header>

          <div className="flex-1 px-5 py-5">
            {transcripts.length === 0 && !partial ? (
              <div className="flex h-full items-center justify-center text-center">
                <div className="max-w-sm">
                  <div className="glass rim mx-auto mb-4 flex h-14 w-14 items-center justify-center rounded-[19px]">
                    <Mic className="text-accent" size={24} />
                  </div>
                  <p className="t-title text-title font-semibold">
                    Start a local transcription session
                  </p>
                  <p className="t-body mt-1.5 text-ui text-smoke">
                    Saved transcripts appear here by date. Pick a model and language above,
                    a microphone below, then press Talk.
                  </p>
                </div>
              </div>
            ) : (
              <div className="mx-auto max-w-3xl space-y-3">
                {transcripts.map((transcript) => (
                  <article key={transcript.id} className="glass rim rounded-lg p-3.5">
                    <div className="mb-2.5 flex items-center justify-between gap-3">
                      <div className="tnum t-micro text-meta text-smoke">
                        {new Date(transcript.created_at).toLocaleTimeString([], {
                          hour: "2-digit",
                          minute: "2-digit",
                        })}{" "}
                        · {transcript.language}
                      </div>
                      <div className="flex items-center gap-1">
                        <Button
                          variant="ghost"
                          size="sm"
                          title={
                            llm?.baseUrl ? "Format as Markdown" : "Configure an LLM in Settings first"
                          }
                          disabled={!llm?.baseUrl || transcript.id in formatting}
                          onClick={() => void runFormat(transcript.id, transcript.text)}
                        >
                          <Wand2 size={13} />
                          <span className="-ml-0.5">
                            {transcript.id in formatting
                              ? "Formatting"
                              : transcript.formatted_text
                                ? "Redo"
                                : "Format"}
                          </span>
                        </Button>
                        <Button
                          variant="ghost"
                          size="sm"
                          className="w-[26px] px-0"
                          title="Copy"
                          onClick={() => copyTranscript(transcript.formatted_text || transcript.text)}
                        >
                          <Copy size={14} />
                        </Button>
                      </div>
                    </div>
                    <p className="t-body whitespace-pre-wrap text-read">{transcript.text}</p>
                    {transcript.id in formatting || transcript.formatted_text ? (
                      <div className="mt-3 border-t border-line-soft pt-3">
                        <div className="t-micro mb-1.5 flex items-center gap-1.5 text-meta font-medium text-moss">
                          <Wand2 size={12} />
                          {transcript.id in formatting
                            ? "Formatting"
                            : `Formatted · ${transcript.formatted_preset || "typeset"}`}
                        </div>
                        <p className="t-body whitespace-pre-wrap text-read">
                          {formatting[transcript.id] ?? transcript.formatted_text}
                        </p>
                      </div>
                    ) : null}
                    {formatError[transcript.id] ? (
                      <p className="mt-2 text-ui text-rust">{formatError[transcript.id]}</p>
                    ) : null}
                  </article>
                ))}
                {partial ? (
                  <article className="glass rim rounded-lg p-3.5 ring-1 ring-rust/25">
                    <div className="t-micro mb-1.5 flex items-center gap-1.5 text-meta font-medium text-rust">
                      <span className="flex h-1.5 w-1.5 rounded-pill bg-rust" />
                      Live partial
                    </div>
                    <p className="t-body whitespace-pre-wrap text-read">{partial}</p>
                  </article>
                ) : null}
              </div>
            )}
          </div>

          {/* Talk, the microphone it will record from, and the meter that proves
              that microphone is live — one row, one height ladder, reading left
              to right in the order the user acts. */}
          <footer className="glass-chrome sticky bottom-0 z-20 mt-auto px-4 py-2.5">
            <div className="mx-auto flex w-full max-w-4xl items-center gap-2">
              <Button
                size="lg"
                variant={recording ? "danger" : "primary"}
                icon={recording ? <Square size={14} /> : <Mic size={15} />}
                disabled={busy !== null && busy !== "transcribe"}
                onClick={recording ? stopRecording : startRecording}
              >
                {recording ? "Stop" : "Talk"}
              </Button>
              <SelectField
                icon={<Mic size={13} />}
                title={
                  defaultInput
                    ? `Microphone · default is ${defaultInput.name}`
                    : "Microphone"
                }
                className="w-[196px] shrink-0"
                value={audioInputId}
                onChange={setAudioInputId}
                disabled={recording}
              >
                {/* The resolved device name lives in the tooltip, not in the
                    closed control: PipeWire names run to fifty characters and no
                    pill that fits in a footer will ever show one. */}
                <option value="">Default microphone</option>
                {audioInputs.map((input) => (
                  <option key={input.id} value={input.id}>
                    {input.name}
                  </option>
                ))}
              </SelectField>
              <InputLevel level={inputLevel} active={recording} />
              <p className="t-body min-w-0 flex-1 truncate text-ui text-smoke">
                {recording
                  ? "Speak normally. The meter should move while you talk."
                  : audioInputs.length
                    ? "Press Talk, or hold the hotkey on the voice bar."
                    : "No microphone input detected by the native audio backend."}
              </p>
            </div>
          </footer>
        </div>
      </section>
      </main>
    </>
  );
}

/*
  A meter, not a card. Stacking the label above the bar made this the tallest
  thing in the footer and forced the whole strip to the meter's height; laid out
  in one line it drops onto `--ctl-h` with the button and the select beside it,
  and the reading stays exactly as legible.
*/
function InputLevel({ level, active }: { level: AudioLevelInfo; active: boolean }) {
  return (
    <div className="ctl rim flex w-[184px] shrink-0 items-center gap-2.5">
      <span className="t-micro shrink-0 text-meta text-smoke">Input</span>
      <span className="h-[3px] min-w-0 flex-1 overflow-hidden rounded-pill bg-track">
        <span
          className="block h-full rounded-pill bg-moss transition-all duration-100 ease-glass"
          style={{ width: `${active ? level.percent : 0}%` }}
        />
      </span>
      <span className="tnum t-micro w-[44px] shrink-0 text-right text-meta text-smoke">
        {active ? formatDb(level.db) : "idle"}
      </span>
    </div>
  );
}

function formatDb(value: number) {
  if (!Number.isFinite(value)) return "-90 dB";
  return `${Math.round(value)} dB`;
}

/**
 * A select on chrome: capsule, control-ladder height, and a leading glyph.
 *
 * The glyph is what a bare value cannot supply — "中文" alone does not say
 * "language" — and it costs about 18px where a text label costs 48px, which
 * decided whether the toolbar fitted at this window width. The wrapper is a
 * `<label>` so the glyph is part of the hit target rather than a decoration
 * sitting on top of it.
 */
function SelectField({
  icon,
  value,
  onChange,
  children,
  disabled,
  title,
  className = "",
}: {
  icon: React.ReactNode;
  value: string;
  onChange: (value: string) => void;
  children: React.ReactNode;
  disabled?: boolean;
  title?: string;
  className?: string;
}) {
  return (
    <label className={`select-lead ${className}`} title={title}>
      {icon}
      <select
        className="field field-pill"
        value={value}
        disabled={disabled}
        onChange={(event) => onChange(event.target.value)}
      >
        {children}
      </select>
    </label>
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
