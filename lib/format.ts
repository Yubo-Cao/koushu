export function formatBytes(value?: number | null) {
  if (!value || value <= 0) return "-";
  if (value >= 1024 * 1024 * 1024) return `${(value / 1024 / 1024 / 1024).toFixed(1)} GB`;
  if (value >= 1024 * 1024) return `${(value / 1024 / 1024).toFixed(0)} MB`;
  return `${(value / 1024).toFixed(0)} KB`;
}

export function formatDownloadProgress(downloaded?: number | null, total?: number | null) {
  if (downloaded && total) return `${formatBytes(downloaded)} / ${formatBytes(total)}`;
  if (downloaded) return `${formatBytes(downloaded)} downloaded`;
  return "Preparing download";
}
