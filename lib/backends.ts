/**
 * ASR backend identifiers. These must stay in sync with the `BACKEND_*`
 * constants in `src-tauri/src/lib.rs`, which are also the values stored in the
 * `models.backend` column and the `defaults.runtime` setting.
 *
 * Both backends run on the official QwenAudio/Fun-ASR llama.cpp CPU runtime.
 */

/** Fun-ASR-Nano: SAN-M encoder + Qwen3-0.6B decoder. Slower, more accurate. */
export const BACKEND_NANO = "funasr-nano-gguf-cpu";

/** SenseVoiceSmall: encoder + CTC, one forward pass. Faster, weaker English. */
export const BACKEND_SENSEVOICE = "funasr-sensevoice-gguf-cpu";

/** Fallback when no model is selected yet. */
export const DEFAULT_BACKEND = BACKEND_NANO;

const BACKEND_LABELS: Record<string, string> = {
  [BACKEND_NANO]: "Fun-ASR-Nano (accurate)",
  [BACKEND_SENSEVOICE]: "SenseVoiceSmall (fast)",
};

export function backendLabel(backend: string): string {
  return BACKEND_LABELS[backend] ?? backend;
}
