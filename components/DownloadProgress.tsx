"use client";

import { Pause } from "lucide-react";
import { Button } from "@/components/Button";
import { formatDownloadProgress } from "@/lib/format";
import type { ModelDownloadState } from "@/lib/types";

type DownloadProgressProps = {
  download: ModelDownloadState;
  onPause?: () => void;
};

export function DownloadProgress({ download, onPause }: DownloadProgressProps) {
  const percent =
    download.totalBytes && download.totalBytes > 0
      ? Math.min(100, Math.max(0, (download.downloadedBytes / download.totalBytes) * 100))
      : null;

  return (
    <div className="rounded-md border border-line bg-[#eef3ea] p-3">
      <div className="mb-2 flex items-center justify-between gap-3">
        <div className="min-w-0">
          <p className="truncate text-sm font-medium text-ink">{download.message}</p>
          <p className="mt-1 text-xs text-smoke">
            {formatDownloadProgress(download.downloadedBytes, download.totalBytes)}
            {percent !== null ? ` · ${percent.toFixed(0)}%` : ""}
          </p>
        </div>
        {download.active && onPause ? (
          <Button className="h-8 shrink-0 px-2" variant="secondary" icon={<Pause size={14} />} onClick={onPause}>
            Pause
          </Button>
        ) : null}
      </div>
      <div className="h-2 overflow-hidden rounded-full bg-[#d5dccf]">
        <div
          className={[
            "h-full rounded-full bg-moss transition-all",
            percent === null ? "w-1/3 animate-pulse" : "",
          ].join(" ")}
          style={percent !== null ? { width: `${percent}%` } : undefined}
        />
      </div>
    </div>
  );
}
