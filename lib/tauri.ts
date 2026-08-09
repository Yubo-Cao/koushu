"use client";

import type {
  AudioInputInfo,
  AudioLevelInfo,
  AsrResult,
  Bootstrap,
  ModelInfo,
  ModelDownloadEvent,
  NativeAudioCaptureResult,
  FormatEvent,
  LlmSettings,
  InjectReport,
  InjectTarget,
  PasteResult,
  PushToTalkEvent,
  HotkeyStatus,
  StreamingEvent,
  TrialStatus,
  SessionInfo,
  SessionFilter,
  SearchResponse,
  FilterOptions,
  TranscriptInfo,
} from "@/lib/types";

type InvokeArgs = Record<string, unknown>;

async function invokeCommand<T>(command: string, args?: InvokeArgs): Promise<T> {
  const mod = await import("@tauri-apps/api/core");
  return mod.invoke<T>(command, args);
}

export async function getBootstrap(): Promise<Bootstrap> {
  return invokeCommand<Bootstrap>("get_bootstrap");
}

export async function completeOnboarding(): Promise<void> {
  await invokeCommand("complete_onboarding");
}

export async function resetOnboarding(): Promise<void> {
  await invokeCommand("reset_onboarding");
}

export async function listModels(): Promise<ModelInfo[]> {
  return invokeCommand<ModelInfo[]>("list_models");
}

export async function listSessions(
  limit = 60,
  filter?: SessionFilter,
): Promise<SessionInfo[]> {
  return invokeCommand<SessionInfo[]>("list_sessions", { limit, filter });
}

/**
 * Full-text search across every transcript, newest match first.
 *
 * Local SQLite over a trigram index, so this is a few milliseconds even on a
 * large history — fast enough to run on every keystroke.
 */
export async function searchTranscripts(
  query: string,
  filter?: SessionFilter,
  limit = 80,
): Promise<SearchResponse> {
  return invokeCommand<SearchResponse>("search_transcripts", {
    request: { query, filter: filter ?? {}, limit },
  });
}

/** Puts a session away, or brings it back. Nothing is deleted either way. */
export async function setSessionArchived(
  sessionId: string,
  archived: boolean,
): Promise<SessionInfo | null> {
  return invokeCommand<SessionInfo | null>("set_session_archived", {
    sessionId,
    archived,
  });
}

/** The languages, models and dates present, for populating the filters. */
export async function sessionFilterOptions(): Promise<FilterOptions> {
  return invokeCommand<FilterOptions>("session_filter_options");
}

export async function listTranscripts(sessionId: string): Promise<TranscriptInfo[]> {
  return invokeCommand<TranscriptInfo[]>("list_transcripts", { sessionId });
}

export async function createSession(input: {
  title?: string;
  model: string;
  language: string;
  runtime: string;
}): Promise<SessionInfo> {
  return invokeCommand<SessionInfo>("create_session", { request: input });
}

export async function setSetting(key: string, value: string): Promise<void> {
  await invokeCommand("set_setting", { key, value });
}

/** Whether the compositor blurs behind the bar; drives the glass material. */
export async function desktopBlurActive(): Promise<boolean> {
  return invokeCommand<boolean>("desktop_blur_active");
}

export async function getTrialStatus(): Promise<TrialStatus> {
  return invokeCommand<TrialStatus>("get_trial_status");
}

export async function getLlmSettings(): Promise<LlmSettings> {
  return invokeCommand<LlmSettings>("get_llm_settings");
}

export async function setCloudAsrApiKey(key: string | null): Promise<void> {
  await invokeCommand("set_cloud_asr_api_key", { key });
}

export async function setLlmApiKey(key: string | null): Promise<void> {
  await invokeCommand("set_llm_api_key", { key });
}

/** Stream a formatting pass. Resolves with the complete Markdown. */
export async function formatTranscript(
  input: { transcriptId?: string; text: string; preset?: string },
  onEvent: (event: FormatEvent) => void,
): Promise<string> {
  const mod = await import("@tauri-apps/api/core");
  const channel = new mod.Channel<FormatEvent>();
  channel.onmessage = onEvent;
  return mod.invoke<string>("format_transcript", {
    transcriptId: input.transcriptId,
    text: input.text,
    preset: input.preset,
    onEvent: channel,
  });
}

export async function startPushToTalk(
  trigger: string | undefined,
  onEvent: (event: PushToTalkEvent) => void,
): Promise<HotkeyStatus> {
  const mod = await import("@tauri-apps/api/core");
  const channel = new mod.Channel<PushToTalkEvent>();
  channel.onmessage = onEvent;
  return mod.invoke<HotkeyStatus>("start_push_to_talk", { trigger, onEvent: channel });
}

export async function stopPushToTalk(): Promise<void> {
  await invokeCommand("stop_push_to_talk");
}

/**
 * Store a chord and put it into effect. `null` restores the default.
 *
 * The returned status is the answer to "does the new key work?", and it is not
 * the same question as whether this call threw. Callers must read `ok`.
 */
export async function setPushToTalkTrigger(trigger: string | null): Promise<HotkeyStatus> {
  return invokeCommand<HotkeyStatus>("set_push_to_talk_trigger", { trigger });
}

/** What push-to-talk is doing, for a window that did not start it. */
export async function pushToTalkStatus(): Promise<HotkeyStatus | null> {
  return invokeCommand<HotkeyStatus | null>("push_to_talk_status");
}

/**
 * Let go of the binding while a new chord is being recorded, so pressing the
 * current one records it instead of starting a recording.
 */
export async function suspendPushToTalk(): Promise<void> {
  await invokeCommand("suspend_push_to_talk");
}

export async function resizeVoiceBar(width: number, height: number): Promise<void> {
  await invokeCommand("resize_voice_bar", { width, height });
}

export async function beginVoiceBarDrag(): Promise<void> {
  await invokeCommand("begin_voice_bar_drag");
}

/** Follow the cursor for one tick. Rust reads the real cursor position. */
export async function trackVoiceBarDrag(): Promise<void> {
  await invokeCommand("track_voice_bar_drag");
}

/** Finish the drag and snap to the nearest edge. Returns the new dock. */
export async function endVoiceBarDrag(): Promise<string> {
  return invokeCommand<string>("end_voice_bar_drag");
}

/** Snap to the nearest edge of the monitor the bar is actually on. */
export async function snapVoiceBar(margin?: number): Promise<string> {
  return invokeCommand<string>("snap_voice_bar", { margin });
}

export async function anchorVoiceBar(
  anchor: string,
  margin?: number,
): Promise<{ anchored: boolean; layerShell: boolean; detail: string }> {
  return invokeCommand("anchor_voice_bar", { anchor, margin });
}

export async function showVoiceBarPassive(): Promise<void> {
  await invokeCommand("show_voice_bar_passive");
}

export async function startStreamingTranscription(
  modelId: string,
  onEvent: (event: StreamingEvent) => void,
): Promise<void> {
  const mod = await import("@tauri-apps/api/core");
  const channel = new mod.Channel<StreamingEvent>();
  channel.onmessage = onEvent;
  await mod.invoke("start_streaming_transcription", { modelId, onEvent: channel });
}

export async function stopStreamingTranscription(): Promise<void> {
  await invokeCommand("stop_streaming_transcription");
}

export async function downloadModelWithProgress(
  modelId: string,
  onEvent: (event: ModelDownloadEvent) => void,
): Promise<ModelInfo> {
  const mod = await import("@tauri-apps/api/core");
  const channel = new mod.Channel<ModelDownloadEvent>();
  channel.onmessage = onEvent;
  return mod.invoke<ModelInfo>("download_model_with_progress", { modelId, onEvent: channel });
}

export async function pauseModelDownload(modelId: string): Promise<void> {
  await invokeCommand("pause_model_download", { modelId });
}

export async function listAudioInputs(): Promise<AudioInputInfo[]> {
  return invokeCommand<AudioInputInfo[]>("list_audio_inputs");
}

export async function startAudioCapture(deviceId?: string): Promise<void> {
  await invokeCommand("start_audio_capture", {
    deviceId: deviceId || null,
  });
}

export async function getAudioLevel(): Promise<AudioLevelInfo> {
  return invokeCommand<AudioLevelInfo>("get_audio_level");
}

export async function snapshotAudioCapture(maxMs?: number): Promise<NativeAudioCaptureResult> {
  return invokeCommand<NativeAudioCaptureResult>("snapshot_audio_capture", {
    maxMs: maxMs ?? null,
  });
}

export async function stopAudioCapture(): Promise<NativeAudioCaptureResult> {
  return invokeCommand<NativeAudioCaptureResult>("stop_audio_capture");
}

export async function transcribeAudio(input: {
  sessionId?: string;
  audioBase64: string;
  modelId: string;
  language: string;
}): Promise<AsrResult> {
  return invokeCommand<AsrResult>("transcribe_audio", {
    request: {
      session_id: input.sessionId,
      audio_base64: input.audioBase64,
      model_id: input.modelId,
      language: input.language,
    },
  });
}

export async function previewAudio(input: {
  sessionId?: string;
  audioBase64: string;
  modelId: string;
  language: string;
}): Promise<AsrResult> {
  return invokeCommand<AsrResult>("preview_audio", {
    request: {
      session_id: input.sessionId,
      audio_base64: input.audioBase64,
      model_id: input.modelId,
      language: input.language,
    },
  });
}

export async function saveTextTranscript(input: {
  sessionId: string;
  text: string;
  model: string;
  language: string;
}): Promise<TranscriptInfo> {
  return invokeCommand<TranscriptInfo>("save_text_transcript", {
    sessionId: input.sessionId,
    text: input.text,
    model: input.model,
    language: input.language,
  });
}

export async function copyText(text: string): Promise<PasteResult> {
  return invokeCommand<PasteResult>("copy_text", { text });
}

export async function autoPasteText(text: string): Promise<PasteResult> {
  return invokeCommand<PasteResult>("auto_paste_text", { text });
}

/** Resolve the focused application. Call at the start of an utterance. */
export async function captureInjectTarget(): Promise<InjectTarget> {
  return invokeCommand<InjectTarget>("capture_inject_target");
}

/**
 * Insert text into a previously captured target.
 *
 * Set `keepClipboard` for live, mid-utterance delivery: overwriting the
 * clipboard once per spoken phrase would wipe whatever the user had copied,
 * many times a minute. Leave it unset for the final delivery so the finished
 * transcript is also on the clipboard, where it can be pasted again.
 */
export async function injectText(
  text: string,
  target?: InjectTarget | null,
  keepClipboard = false,
): Promise<InjectReport> {
  return invokeCommand<InjectReport>("inject_text", {
    text,
    target: target ?? null,
    keepClipboard,
  });
}

export async function showVoiceBar(): Promise<void> {
  await invokeCommand("show_voice_bar");
}

export async function hideVoiceBar(): Promise<void> {
  await invokeCommand("hide_voice_bar");
}

export async function showSettingsWindow(): Promise<void> {
  await invokeCommand("show_settings_window");
}

/**
 * Whether this window wants an app-drawn frame — a transparent ring around a
 * rounded shell, with the shadow painted into it.
 *
 * This has to be asked directly, and it deliberately does *not* mean "the
 * window is undecorated". Those came apart in practice: dropping decorations
 * landed long before transparency did, and in between, a gutter opened on an
 * opaque window rendered as a hard band of page background — a fake second
 * frame around the real one. So the backend answers the question the frontend
 * actually has ("is this window transparent, so a gutter is invisible?"), and
 * returns false when transparency was tried and abandoned as well as when it
 * was never attempted.
 *
 * The safe answer is false: no gutter costs a shadow, whereas a wrong true
 * costs a visibly broken window.
 */
export type WindowChrome = {
  csdGutter: boolean;
  /** Width of the ring, in CSS pixels. The backend also adds it to the window
   *  size, so taking the number from here is what keeps the two from drifting. */
  gutter: number;
};

export async function getWindowChrome(): Promise<WindowChrome> {
  const raw = await invokeCommand<Record<string, unknown>>("window_chrome");
  // Accept either casing: whether the Rust struct carries a serde rename is not
  // something this side should be able to be broken by.
  const enabled = (raw?.["csd_gutter"] ?? raw?.["csdGutter"]) === true;
  const size = Number(raw?.["gutter"]);
  return { csdGutter: enabled, gutter: Number.isFinite(size) && size >= 0 ? size : 18 };
}
