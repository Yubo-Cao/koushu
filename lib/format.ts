import type { MessageKey, Translate } from "@/lib/i18n";

export function formatBytes(value?: number | null) {
  if (!value || value <= 0) return "-";
  if (value >= 1024 * 1024 * 1024) return `${(value / 1024 / 1024 / 1024).toFixed(1)} GB`;
  if (value >= 1024 * 1024) return `${(value / 1024 / 1024).toFixed(0)} MB`;
  return `${(value / 1024).toFixed(0)} KB`;
}

/**
 * `t` is passed in rather than imported: this module is not a component, and a
 * hook here would drag the whole file behind the React boundary for two words.
 */
export function formatDownloadProgress(
  downloaded: number | null | undefined,
  total: number | null | undefined,
  t: Translate,
) {
  if (downloaded && total) return `${formatBytes(downloaded)} / ${formatBytes(total)}`;
  if (downloaded) return t("download.downloaded", { size: formatBytes(downloaded) });
  return t("download.preparing");
}

/** Rust's `models.status` column. Anything unmapped is shown raw. */
const MODEL_STATUS_KEYS: Record<string, MessageKey> = {
  available: "model.status.available",
  downloading: "model.status.downloading",
  installed: "model.status.installed",
  paused: "model.status.paused",
  error: "model.status.error",
};

export function modelStatusLabel(status: string, t: Translate): string {
  const key = MODEL_STATUS_KEYS[status];
  return key ? t(key) : status;
}
