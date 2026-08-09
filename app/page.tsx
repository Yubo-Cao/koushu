"use client";

import {
  Archive,
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
import {
  SearchResults,
  SessionSearch,
  useSessionBrowser,
} from "@/components/SessionSearch";
import { SetupView } from "@/components/SetupView";
import { TitleBar } from "@/components/TitleBar";
import { WindowFrame } from "@/components/WindowFrame";
import { DEFAULT_BACKEND } from "@/lib/backends";
import { useT } from "@/lib/i18n";
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
  setSessionArchived,
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
  const t = useT();
  const [bootstrap, setBootstrap] = useState<Bootstrap | null>(null);
  const [sessions, setSessions] = useState<SessionInfo[]>([]);
  const [activeSession, setActiveSession] = useState<SessionInfo | null>(null);
  const [transcripts, setTranscripts] = useState<TranscriptInfo[]>([]);
  const [modelId, setModelId] = useState("fun-asr-nano-2512");
  const [language, setLanguage] = useState("中文");
  const [audioInputs, setAudioInputs] = useState<AudioInputInfo[]>([]);
  const [audioInputId, setAudioInputId] = useState("");
  const [inputLevel, setInputLevel] = useState<AudioLevelInfo>(idleLevel);
  // Empty means "nothing has happened yet", which renders as the idle label in
  // whatever locale is current — storing the translated string here instead
  // would freeze it at the locale it was set in.
  const [status, setStatus] = useState("");
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
  // Search, filters and the archive scope. Owns its own state so the sidebar
  // and the results pane cannot disagree about what is being looked at.
  const browser = useSessionBrowser();
  const [pendingScroll, setPendingScroll] = useState<string | null>(null);

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
    // The filters narrow the list in SQL rather than here: a date range or the
    // archive scope has to be applied across the whole history, not across
    // whichever page of it happens to be loaded.
    const next = await listSessions(200, browser.filter);
    setSessions(next);
    if (selectId) {
      setActiveSession(next.find((session) => session.id === selectId) || next[0] || null);
    }
  }

  // Re-list whenever the filters change. The active session is kept if it
  // survives the new filter, so tightening a filter never yanks the transcript
  // pane out from under whatever is being read.
  useEffect(() => {
    listSessions(200, browser.filter)
      .then((next) => {
        setSessions(next);
        setActiveSession((current) =>
          current && next.some((session) => session.id === current.id)
            ? current
            : next[0] || null,
        );
      })
      .catch((error) => setStatus(String(error)));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [browser.filter]);

  /**
   * Opens the session a search hit belongs to and scrolls to the transcript.
   *
   * The scroll cannot happen here: the transcripts for that session have not
   * been fetched yet, so the element does not exist. It is deferred to an
   * effect that runs once they land.
   */
  function openHit(sessionId: string, transcriptId: string) {
    const session = sessions.find((item) => item.id === sessionId);
    if (session) {
      setActiveSession(session);
    } else {
      // The hit is in a session the sidebar filter is hiding — an archived one,
      // most often. Fetch it so it can still be opened.
      listSessions(200, { ...browser.filter, archived: "all" })
        .then((all) => {
          const found = all.find((item) => item.id === sessionId);
          if (found) setActiveSession(found);
        })
        .catch((error) => setStatus(String(error)));
    }
    browser.clear();
    setPendingScroll(transcriptId);
  }

  useEffect(() => {
    if (!pendingScroll) return;
    // This effect first runs while `transcripts` still holds the *previous*
    // session's rows, because selecting a session only starts the fetch. Giving
    // up then would mean the jump silently never happens; waiting until the row
    // is actually in the list is what makes it land.
    if (!transcripts.some((transcript) => transcript.id === pendingScroll)) return;
    const target = document.getElementById(`transcript-${pendingScroll}`);
    setPendingScroll(null);
    target?.scrollIntoView({ block: "center" });
  }, [pendingScroll, transcripts]);

  async function toggleArchive(session: SessionInfo, archived: boolean) {
    try {
      await setSessionArchived(session.id, archived);
      await refreshSessions();
      browser.refreshOptions();
      setStatus(
        archived
          ? `Archived “${session.title}”. It is still searchable and can be restored.`
          : `Restored “${session.title}” to the session list.`,
      );
    } catch (error) {
      setStatus(String(error));
    }
  }

  async function ensureSession(): Promise<SessionInfo> {
    if (activeSession) return activeSession;
    const session = await createSession({
      title: t("home.sessionUntitled"),
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
        title: t("home.sessionTitle", {
          time: new Date().toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" }),
        }),
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
        message: t("download.model"),
      });
      setStatus(t("download.model"));
    } else if (event.event === "paused") {
      setDownload({
        modelId: event.data.modelId,
        active: false,
        paused: true,
        downloadedBytes: event.data.downloadedBytes,
        totalBytes: event.data.totalBytes,
        message: t("download.paused"),
      });
      setStatus(t("download.paused"));
    } else if (event.event === "finished") {
      setDownload({
        modelId: event.data.modelId,
        active: false,
        paused: false,
        downloadedBytes: event.data.downloadedBytes,
        totalBytes: event.data.totalBytes,
        message: t("download.installed"),
      });
      setStatus(t("download.installed"));
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
    setStatus(t("download.model"));
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
      setStatus(model.status === "installed" ? t("download.installed") : t("download.paused"));
    } catch (error) {
      setStatus(String(error));
    } finally {
      setBusy(null);
    }
  }

  async function pauseDownload() {
    setDownload((current) => (current ? { ...current, message: t("download.pausing") } : current));
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
      setStatus(t("home.status.listening"));
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
    setStatus(t("home.status.transcribing"));
    try {
      const capture = await stopAudioCapture();
      setInputLevel(idleLevel);
      if (!capture.speechLike) {
        setPartial("");
        setStatus(t("home.status.noVoice", { db: formatDb(capture.db) }));
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
      setStatus(result.error || t("home.status.saved"));
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

  // Both early returns still need the title bar: the window has no system
  // frame, so without it there is nothing to drag and no way to close.
  if (!bootstrap) {
    return (
      <WindowFrame>
        <TitleBar brand={<span className="t-title text-ctl font-semibold">{t("common.appName")}</span>} />
        <main className="flex flex-1 items-center justify-center text-ctl text-smoke">
          {t("common.loading")}
        </main>
      </WindowFrame>
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
      Three fixed bands and one scroller: title bar, sidebar + transcripts, and
      the control deck. Only the deck is sticky inside the scroll container, and
      it is the only surface in the window carrying a backdrop-filter — it is
      the only one that ever has page content passing beneath it. Chrome that
      cannot be scrolled under gets an honest fill instead of a blur that
      resolves to the pixels it started with.
    */
    <WindowFrame>
      <Confetti fire={celebrate} onDone={() => setCelebrate(false)} />

      {/*
        One bar across the top of the window, not two stacked ones. The system
        frame is gone on both platforms, so this is the title bar; it is also
        the place the session title and status already wanted to be. Its left
        zone is exactly the sidebar's width, so the app identity sits over the
        sidebar and the session sits over the transcripts — the split the
        window already has, carried up into the chrome instead of fought.
      */}
      <TitleBar
        leadClassName="app-lead pr-2"
        brand={
          <>
            <h1 className="t-title text-ctl font-semibold">{t("common.appName")}</h1>
            <span className="tnum t-micro text-meta text-faint">0.0.2</span>
          </>
        }
        actions={
          <>
            <Button
              variant="ghost"
              size="sm"
              className="w-[26px] px-0"
              title={t("home.showVoiceBar")}
              onClick={showVoiceBar}
            >
              <PanelTop size={15} />
            </Button>
            <Button
              variant="ghost"
              size="sm"
              className="w-[26px] px-0"
              title={t("common.settings")}
              onClick={showSettingsWindow}
            >
              <Settings size={16} />
            </Button>
          </>
        }
      >
        <span data-tauri-drag-region className="truncate text-ctl font-semibold">
          {activeSession?.title || t("home.noSession")}
        </span>
        <span data-tauri-drag-region className="t-micro shrink-0 truncate text-meta text-smoke">
          {status || t("home.status.ready")}
        </span>
      </TitleBar>

      <main className="app-grid min-h-0 flex-1">
      {/* The New Session button is deliberately *outside* the scroll container.
          As a sticky glass strip it had session titles sliding under it and
          showing through the material, which reads as a rendering fault rather
          than as depth. Content scrolling under chrome only works where the
          chrome has real weight; a 44px strip does not. */}
      <aside className="hairline-r flex min-h-0 flex-col">
        <div className="shrink-0 px-3 pt-2.5 pb-2.5">
          <Button
            variant="primary"
            className="w-full"
            icon={<Plus size={15} />}
            disabled={busy === "session"}
            onClick={newSession}
          >
            {t("home.newSession")}
          </Button>
        </div>
        <SessionSearch
          browser={browser}
          activeSession={activeSession}
          onArchive={toggleArchive}
        />
        <div className="scrollbar-thin min-h-0 flex-1 overflow-y-auto">

          {/* One line per session, not two. The language was a full second line
              at 11px/1.6 under every title, which made a row 57px tall and put
              eight sessions on a screen that comfortably holds fifteen. It is
              secondary information, so it sits where secondary information goes
              — trailing, quiet, on the same line. */}
          <div className="px-2 pb-4">
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
                        // A 28px row does not need its own pane of glass with a
                        // specular rim around it. A fill and a weight change say
                        // "selected" with less to look at.
                        activeSession?.id === session.id
                          ? "bg-fill-strong font-medium text-ink"
                          : "text-ink-2 hover:bg-fill",
                      ].join(" ")}
                      title={session.title}
                      onClick={() => setActiveSession(session)}
                    >
                      <span className="min-w-0 flex-1 truncate text-ctl">{session.title}</span>
                      <span className="t-micro flex shrink-0 items-center gap-1 text-meta text-faint">
                        {/* Without this, the "Everything" archive scope shows
                            archived and active sessions in one indistinguishable
                            list. */}
                        {session.archived_at ? (
                          <Archive size={10} aria-label={t("search.scopeArchived")} />
                        ) : null}
                        {session.language}
                      </span>
                    </button>
                  ))}
                </div>
              </section>
            ))}

            {/* An empty sidebar under an active filter looks identical to an
                empty sidebar in a brand-new install. */}
            {groupedSessions.length === 0 ? (
              <p className="t-micro px-2.5 py-2 text-meta text-smoke">
                {browser.activeFilters > 0
                  ? t("home.sidebar.noMatches")
                  : browser.filter.archived === "archived"
                    ? t("home.sidebar.noArchived")
                    : t("home.sidebar.noSessions")}
              </p>
            ) : null}
          </div>
        </div>
      </aside>

      <section className="relative min-h-0">
        <div className="scrollbar-thin absolute inset-0 flex flex-col overflow-y-auto">
          {/* The title bar reports; the deck at the bottom acts. Nothing is left
              for a second header except a download in progress, which only
              exists while it is happening. */}
          {download?.modelId === modelId ? (
            <div className="glass-chrome sticky top-0 z-20 px-4 py-2">
              <div className="max-w-[560px]">
                <DownloadProgress
                  download={download}
                  onPause={download.active ? pauseDownload : undefined}
                />
              </div>
            </div>
          ) : null}

          {/* Search takes over the pane rather than opening beside it: a hit is
              a transcript, and this is where transcripts are read. Clearing the
              box puts the session straight back, untouched. */}
          {browser.searching ? (
            <div className="flex-1">
              <SearchResults browser={browser} onOpen={openHit} />
            </div>
          ) : (
          <div className="flex-1 px-4 py-4 min-[900px]:px-5 min-[900px]:py-5">
            {transcripts.length === 0 && !partial ? (
              <div className="flex h-full items-center justify-center text-center">
                <div className="max-w-sm">
                  <div className="glass rim mx-auto mb-4 flex h-14 w-14 items-center justify-center rounded-[19px]">
                    <Mic className="text-accent" size={24} />
                  </div>
                  <p className="t-title text-title font-semibold">
                    {t("home.empty.title")}
                  </p>
                  <p className="t-body mt-1.5 text-ui text-smoke">
                    {t("home.empty.body")}
                  </p>
                </div>
              </div>
            ) : (
              <div className="mx-auto max-w-3xl space-y-3">
                {transcripts.map((transcript) => (
                  <article
                    key={transcript.id}
                    // Scroll target for a search hit, which opens the session
                    // and then has to find the one transcript in it.
                    id={`transcript-${transcript.id}`}
                    className="glass rim rounded-lg p-3.5"
                  >
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
                            llm?.baseUrl
                              ? t("home.transcript.formatTitle")
                              : t("home.transcript.formatDisabled")
                          }
                          disabled={!llm?.baseUrl || transcript.id in formatting}
                          onClick={() => void runFormat(transcript.id, transcript.text)}
                        >
                          <Wand2 size={13} />
                          <span className="-ml-0.5">
                            {transcript.id in formatting
                              ? t("home.transcript.formatting")
                              : transcript.formatted_text
                                ? t("home.transcript.redo")
                                : t("home.transcript.format")}
                          </span>
                        </Button>
                        <Button
                          variant="ghost"
                          size="sm"
                          className="w-[26px] px-0"
                          title={t("common.copy")}
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
                            ? t("home.transcript.formatting")
                            : t("home.transcript.formatted", {
                                preset: transcript.formatted_preset || "typeset",
                              })}
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
                      {t("home.transcript.livePartial")}
                    </div>
                    <p className="t-body whitespace-pre-wrap text-read">{partial}</p>
                  </article>
                ) : null}
              </div>
            )}
          </div>
          )}

          {/* The control deck. Talk, what it will transcribe with, what it will
              listen to, and the meter that proves it is listening — one row,
              one height ladder, reading left to right in the order the user
              acts on them.

              The three selects are flexible rather than fixed. A native select
              cannot shrink its own text, so a fixed width either truncates the
              longest value ("Cloud transcription", a PipeWire device name) or
              reserves dead space for it; giving each a basis, a floor and a
              ceiling lets the row spend whatever the window happens to have.

              The row wraps rather than overflows. Adding the floors up, the
              controls need ~690px and the deck has ~900 at the default window
              — but the same window on a 125% display is ~550, and the Download
              button appears out of nowhere the first time a model is missing.
              A row that cannot wrap answers that by pushing the meter off the
              edge of the window; wrapping drops it onto a second line, which is
              the honest outcome. */}
          <footer className="deck sticky bottom-0 z-20 mt-auto px-4 py-2.5">
            <div className="flex w-full flex-wrap items-center gap-x-2 gap-y-2">
              <Button
                size="lg"
                className="shrink-0"
                variant={recording ? "danger" : "primary"}
                icon={recording ? <Square size={14} /> : <Mic size={15} />}
                disabled={busy !== null && busy !== "transcribe"}
                onClick={recording ? stopRecording : startRecording}
              >
                {recording ? t("home.deck.stop") : t("home.deck.talk")}
              </Button>

              <span className="ctl-sep" aria-hidden />

              <SelectField
                icon={<Boxes size={13} />}
                title={t("home.deck.model")}
                className="min-w-[132px] max-w-[228px] flex-[1.15_1_176px]"
                value={modelId}
                onChange={setModelId}
              >
                {bootstrap.models.map((model) => (
                  <option key={model.id} value={model.id}>
                    {model.name.replace(/\s*\(.*\)$/, "")}
                  </option>
                ))}
              </SelectField>
              {activeModel?.status !== "installed" ? (
                <Button
                  className="shrink-0"
                  icon={<Download size={15} />}
                  disabled={busy !== null}
                  onClick={installModel}
                >
                  {activeModel?.status === "paused" || download?.paused
                    ? t("common.resume")
                    : t("common.download")}
                </Button>
              ) : null}
              <SelectField
                icon={<Languages size={13} />}
                title={t("home.deck.language")}
                className="min-w-[96px] max-w-[150px] flex-[0.7_1_120px]"
                value={language}
                onChange={setLanguage}
              >
                {languages.map((item) => (
                  <option key={item} value={item}>
                    {item}
                  </option>
                ))}
              </SelectField>
              <SelectField
                icon={<Mic size={13} />}
                title={
                  defaultInput
                    ? t("home.deck.microphoneDefault", { name: defaultInput.name })
                    : t("home.deck.microphone")
                }
                className="min-w-[120px] max-w-[240px] flex-[1.15_1_180px]"
                value={audioInputId}
                onChange={setAudioInputId}
                disabled={recording}
              >
                {/* The resolved device name lives in the tooltip, not in the
                    closed control: PipeWire names run to fifty characters and no
                    control that fits in a bar will ever show one. */}
                <option value="">{t("home.deck.microphoneNone")}</option>
                {audioInputs.map((input) => (
                  <option key={input.id} value={input.id}>
                    {input.name}
                  </option>
                ))}
              </SelectField>

              {/* The separator only exists while the row is genuinely one row.
                  Once the meter wraps, a hairline stranded at the start of the
                  second line reads as a stray mark rather than as grouping. */}
              <span className="ctl-sep hidden min-[1150px]:block" aria-hidden />

              {audioInputs.length ? (
                <InputLevel level={inputLevel} active={recording} />
              ) : (
                <p className="t-body min-w-0 flex-1 truncate text-ui text-rust">
                  {t("home.deck.noMicrophone")}
                </p>
              )}
            </div>
          </footer>
        </div>
      </section>
      </main>
    </WindowFrame>
  );
}

/*
  A meter, not a card. Stacking the label above the bar made this the tallest
  thing in the footer and forced the whole strip to the meter's height; laid out
  in one line it drops onto `--ctl-h` with the button and the select beside it,
  and the reading stays exactly as legible.
*/
function InputLevel({ level, active }: { level: AudioLevelInfo; active: boolean }) {
  const t = useT();
  return (
    <div className="ctl rim flex min-w-[150px] max-w-[210px] flex-[0.9_1_170px] items-center gap-2.5">
      <span className="t-micro shrink-0 text-meta text-smoke">{t("home.deck.input")}</span>
      <span className="h-[3px] min-w-0 flex-1 overflow-hidden rounded-pill bg-track">
        <span
          className="block h-full rounded-pill bg-moss transition-all duration-100 ease-glass"
          style={{ width: `${active ? level.percent : 0}%` }}
        />
      </span>
      <span className="tnum t-micro w-[44px] shrink-0 text-right text-meta text-smoke">
        {active ? formatDb(level.db) : t("home.deck.idle")}
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
