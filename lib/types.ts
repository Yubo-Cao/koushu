export type PlatformInfo = {
  os: string;
  arch: string;
  session_type?: string | null;
  wayland_display: boolean;
  x11_display: boolean;
  paste_tools: string[];
  bundled_asr: boolean;
};

export type ModelInfo = {
  id: string;
  name: string;
  backend: string;
  source: string;
  repo_id: string;
  local_path: string;
  status: "available" | "downloading" | "installed" | "error" | string;
  size_bytes?: number | null;
  installed_at?: string | null;
  last_error?: string | null;
};

export type SessionInfo = {
  id: string;
  title: string;
  started_at: string;
  ended_at?: string | null;
  date_key: string;
  model: string;
  language: string;
  runtime: string;
};

export type TranscriptInfo = {
  id: string;
  session_id: string;
  text: string;
  status: string;
  source: string;
  created_at: string;
  duration_ms?: number | null;
  model: string;
  language: string;
  /** LLM-typeset Markdown. The raw `text` above is never overwritten. */
  formatted_text?: string | null;
  formatted_preset?: string | null;
  formatted_at?: string | null;
};

export type Bootstrap = {
  setup_complete: boolean;
  settings: Record<string, string>;
  platform: PlatformInfo;
  models: ModelInfo[];
  sessions: SessionInfo[];
};

export type AsrResult = {
  session_id: string;
  transcript?: TranscriptInfo | null;
  text: string;
  runtime: string;
  error?: string | null;
};

export type PasteResult = {
  copied: boolean;
  pasted: boolean;
  method?: string | null;
  message: string;
  session_type?: string | null;
};

export type ModelDownloadEvent =
  | {
      event: "started";
      data: {
        modelId: string;
        downloadedBytes: number;
        totalBytes?: number | null;
      };
    }
  | {
      event: "progress";
      data: {
        modelId: string;
        chunkBytes: number;
        downloadedBytes: number;
        totalBytes?: number | null;
      };
    }
  | {
      event: "paused";
      data: {
        modelId: string;
        downloadedBytes: number;
        totalBytes?: number | null;
      };
    }
  | {
      event: "finished";
      data: {
        modelId: string;
        downloadedBytes: number;
        totalBytes?: number | null;
        model: ModelInfo;
      };
    }
  | {
      event: "error";
      data: {
        modelId: string;
        error: string;
      };
    };

export type ModelDownloadState = {
  modelId: string;
  active: boolean;
  paused: boolean;
  downloadedBytes: number;
  totalBytes?: number | null;
  message: string;
};

export type AudioInputInfo = {
  id: string;
  name: string;
  isDefault: boolean;
};

export type AudioLevelInfo = {
  rms: number;
  peak: number;
  db: number;
  percent: number;
};

export type NativeAudioCaptureResult = AudioLevelInfo & {
  audioBase64: string;
  durationMs: number;
  speechLike: boolean;
  sampleRate: number;
};

export const languages = [
  "中文",
  "English",
  "日本語",
  "粤语",
  "한국어",
  "Français",
  "Deutsch",
  "Español",
] as const;

/**
 * Events from the native streaming transcriber.
 *
 * Both variants are previews. Segment boundaries can cut mid-sentence and short
 * spans decode worse, so the authoritative transcript is the one produced by
 * re-transcribing the whole recording when capture stops.
 */
export type StreamingEvent =
  | {
      event: "partial";
      data: { segmentIndex: number; text: string };
    }
  | {
      event: "segment";
      data: { segmentIndex: number; text: string; startMs: number; endMs: number };
    }
  | {
      event: "error";
      data: { error: string };
    };

/** Which mechanism is serving the global hotkey. */
export type HotkeyBackend = "portal" | "evdev" | "ns-event" | "unavailable";

/**
 * What the platform managed to set up. `detail` explains *why* a preferred
 * backend was skipped and is meant to be shown, not logged: a push-to-talk key
 * that silently does nothing is the worst failure mode.
 */
export type HotkeyStatus = {
  backend: HotkeyBackend;
  trigger: string;
  detail: string;
};

export type PushToTalkEvent = { event: "pressed" } | { event: "released" };

export type FormatPreset = {
  id: string;
  label: string;
  description: string;
  prompt: string;
};

export type LlmSettings = {
  baseUrl: string;
  model: string;
  /** Whether a key is stored. The key itself never crosses this boundary. */
  hasApiKey: boolean;
  preset: string;
  autoFormat: boolean;
  presets: FormatPreset[];
};

export type FormatEvent =
  | { event: "delta"; data: { text: string } }
  | { event: "done"; data: { text: string } }
  | { event: "error"; data: { error: string } };
