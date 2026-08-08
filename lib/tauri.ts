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
  PasteResult,
  PushToTalkEvent,
  HotkeyStatus,
  StreamingEvent,
  TrialStatus,
  SessionInfo,
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

export async function listSessions(limit = 60): Promise<SessionInfo[]> {
  return invokeCommand<SessionInfo[]>("list_sessions", { limit });
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

export async function showVoiceBar(): Promise<void> {
  await invokeCommand("show_voice_bar");
}

export async function hideVoiceBar(): Promise<void> {
  await invokeCommand("hide_voice_bar");
}

export async function showSettingsWindow(): Promise<void> {
  await invokeCommand("show_settings_window");
}
