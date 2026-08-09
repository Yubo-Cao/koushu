/**
 * ASR backend identifiers. These must stay in sync with the `BACKEND_*`
 * constants in `src-tauri/src/lib.rs`, which are also the values stored in the
 * `models.backend` column and the `defaults.runtime` setting.
 *
 * Both backends run on the official QwenAudio/Fun-ASR llama.cpp CPU runtime.
 */

import type { MessageKey, Translate } from "@/lib/i18n";

/** Fun-ASR-Nano: SAN-M encoder + Qwen3-0.6B decoder. Slower, more accurate. */
export const BACKEND_NANO = "funasr-nano-gguf-cpu";

/** SenseVoiceSmall: encoder + CTC, one forward pass. Faster, weaker English. */
export const BACKEND_SENSEVOICE = "funasr-sensevoice-gguf-cpu";

/** Fallback when no model is selected yet. */
export const DEFAULT_BACKEND = BACKEND_NANO;

/**
 * The model name is a proper noun and stays as it is in every locale; only the
 * parenthetical — what choosing it costs you — is translated. An unknown
 * backend falls back to its raw identifier, which is what a diagnostic wants.
 */
const BACKEND_LABEL_KEYS: Record<string, MessageKey> = {
  [BACKEND_NANO]: "backend.nano",
  [BACKEND_SENSEVOICE]: "backend.sensevoice",
};

export function backendLabel(backend: string, t: Translate): string {
  const key = BACKEND_LABEL_KEYS[backend];
  return key ? t(key) : backend;
}
