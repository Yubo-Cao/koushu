"use client";

import type {
  AudioInputInfo,
  AudioLevelInfo,
  AsrResult,
  Bootstrap,
  GpuRuntimeInfo,
  GpuRuntimeInstallEvent,
  ModelInfo,
  ModelDownloadEvent,
  NativeAudioCaptureResult,
  PasteResult,
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

export async function probePython(): Promise<{ ok: boolean; python: string; message: string }> {
  return invokeCommand("probe_python");
}

export async function probeGpuRuntime(): Promise<GpuRuntimeInfo> {
  return invokeCommand<GpuRuntimeInfo>("probe_gpu_runtime");
}

export async function installGpuRuntimeWithProgress(
  onEvent: (event: GpuRuntimeInstallEvent) => void,
): Promise<GpuRuntimeInfo> {
  const mod = await import("@tauri-apps/api/core");
  const channel = new mod.Channel<GpuRuntimeInstallEvent>();
  channel.onmessage = onEvent;
  return mod.invoke<GpuRuntimeInfo>("install_gpu_runtime_with_progress", { onEvent: channel });
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
  hotwords?: string[];
}): Promise<AsrResult> {
  return invokeCommand<AsrResult>("transcribe_audio", {
    request: {
      session_id: input.sessionId,
      audio_base64: input.audioBase64,
      model_id: input.modelId,
      language: input.language,
      hotwords: input.hotwords,
    },
  });
}

export async function previewAudio(input: {
  sessionId?: string;
  audioBase64: string;
  modelId: string;
  language: string;
  hotwords?: string[];
}): Promise<AsrResult> {
  return invokeCommand<AsrResult>("preview_audio", {
    request: {
      session_id: input.sessionId,
      audio_base64: input.audioBase64,
      model_id: input.modelId,
      language: input.language,
      hotwords: input.hotwords,
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
