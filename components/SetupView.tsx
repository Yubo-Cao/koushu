"use client";

import { ArrowRight, Check, Cpu, Database, Download, Shield } from "lucide-react";
import { useState } from "react";
import { Button } from "@/components/Button";
import { DownloadProgress } from "@/components/DownloadProgress";
import { completeOnboarding, downloadModelWithProgress, pauseModelDownload } from "@/lib/tauri";
import type { Bootstrap, ModelDownloadEvent, ModelDownloadState, ModelInfo } from "@/lib/types";

type SetupViewProps = {
  bootstrap: Bootstrap;
  onDone: () => void;
  onModelsChanged: (models: ModelInfo[]) => void;
};

export function SetupView({ bootstrap, onDone, onModelsChanged }: SetupViewProps) {
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState("");
  const [download, setDownload] = useState<ModelDownloadState | null>(null);
  const model = bootstrap.models.find((item) => item.id === "fun-asr-nano-2512") || bootstrap.models[0];
  const installed = model?.status === "installed";

  function handleDownloadEvent(event: ModelDownloadEvent) {
    if (!model || event.data.modelId !== model.id) return;
    if (event.event === "started" || event.event === "progress") {
      setDownload({
        modelId: event.data.modelId,
        active: true,
        paused: false,
        downloadedBytes: event.data.downloadedBytes,
        totalBytes: event.data.totalBytes,
        message: "Downloading Fun-ASR-Nano from Hugging Face",
      });
    } else if (event.event === "paused") {
      setDownload({
        modelId: event.data.modelId,
        active: false,
        paused: true,
        downloadedBytes: event.data.downloadedBytes,
        totalBytes: event.data.totalBytes,
        message: "Download paused",
      });
    } else if (event.event === "finished") {
      setDownload({
        modelId: event.data.modelId,
        active: false,
        paused: false,
        downloadedBytes: event.data.downloadedBytes,
        totalBytes: event.data.totalBytes,
        message: "Model installed",
      });
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
    }
  }

  async function downloadAndContinue() {
    if (!model) return;
    setBusy(true);
    setMessage("Downloading the Fun-ASR-Nano Q4_K GGUF model from Hugging Face.");
    try {
      const updated = installed ? model : await downloadModelWithProgress(model.id, handleDownloadEvent);
      onModelsChanged(bootstrap.models.map((item) => (item.id === updated.id ? updated : item)));
      if (updated.status !== "installed") {
        setMessage("Download paused. Resume when you are ready.");
        return;
      }
      await completeOnboarding();
      onDone();
    } catch (error) {
      setMessage(String(error));
    } finally {
      setBusy(false);
    }
  }

  async function pauseDownload() {
    if (!model) return;
    setDownload((current) => (current ? { ...current, message: "Pausing download..." } : current));
    await pauseModelDownload(model.id);
  }

  async function continueWithoutModel() {
    await completeOnboarding();
    onDone();
  }

  return (
    <main className="min-h-dvh overflow-y-auto">
      <section className="mx-auto flex min-h-dvh max-w-6xl flex-col px-5 py-6 md:px-6 md:py-8">
        <header className="flex flex-wrap items-start justify-between gap-4">
          <div>
            <p className="t-micro text-ctl font-semibold text-accent">Fun ASR Desktop 0.0.2</p>
            <h1 className="t-display mt-2.5 max-w-3xl text-[28px] leading-[1.15] font-semibold text-ink md:text-[34px]">
              Welcome to local voice transcription.
            </h1>
          </div>
          <div className="ctl rim inline-flex items-center text-smoke">
            {bootstrap.platform.os} {bootstrap.platform.arch}
          </div>
        </header>

        <div className="grid flex-1 grid-cols-1 gap-6 py-6 lg:grid-cols-[minmax(0,1fr)_minmax(340px,400px)] lg:gap-8 lg:py-8">
          <section className="flex flex-col justify-center">
            <p className="t-body max-w-2xl text-[15px] leading-[1.65] text-smoke">
              Talk into your microphone, get fast local transcripts, keep history on this machine, and use the floating bar to paste text into any app.
            </p>

            <div className="mt-8 grid max-w-3xl grid-cols-1 gap-3 sm:grid-cols-2">
              <Feature icon={<Cpu size={17} />} title="CPU-only ASR" text="Bundles the official Fun-ASR llama.cpp runtime. No GPU, no Python, no CUDA." />
              <Feature icon={<Download size={17} />} title="Hugging Face models" text="Downloads Fun-ASR-Nano Q4_K on first setup." />
              <Feature icon={<Database size={17} />} title="Local transcripts" text="Saves sessions by date in local SQLite." />
              <Feature icon={<Shield size={17} />} title="Private by default" text="No cloud service is needed for transcription." />
            </div>
          </section>

          <aside className="glass rim flex min-w-0 flex-col rounded-lg p-4">
            <div className="flex items-start justify-between gap-4 border-b border-line-soft pb-3">
              <div>
                <h2 className="t-title text-title font-semibold">Install Default Model</h2>
                <p className="t-body mt-0.5 text-ui text-smoke">Fun-ASR-Nano GGUF Q4_K from Hugging Face</p>
              </div>
              {installed ? <Check className="shrink-0 text-moss" size={22} /> : <Download className="shrink-0 text-accent" size={22} />}
            </div>

            <div className="space-y-2 py-4 text-ctl">
              <Info label="Runtime" value={bootstrap.platform.bundled_asr ? "Bundled CPU runtime" : "Runtime missing"} />
              <Info label="Model" value={model?.name || "Fun-ASR-Nano GGUF Q4_K"} />
              <Info label="Download" value="about 897 MB" />
              <Info label="Status" value={installed ? "ready" : model?.status || "available"} />
              <Info label="Clipboard" value={bootstrap.platform.paste_tools.length ? "copy and paste available" : "copy only fallback"} />
            </div>

            {download ? (
              <div className="mt-auto">
                <DownloadProgress download={download} onPause={download.active ? pauseDownload : undefined} />
              </div>
            ) : message ? (
              <p className="t-body mt-auto rounded-md bg-fill p-2.5 text-ui text-smoke">{message}</p>
            ) : (
              <div className="t-body mt-auto rounded-md bg-fill p-2.5 text-ui text-smoke">
                The runtime is bundled with the app. The model is downloaded once and stored in app data.
              </div>
            )}

            <div className="mt-4 space-y-2">
              <Button
                className="w-full"
                variant="primary"
                icon={installed ? <ArrowRight size={15} /> : <Download size={15} />}
                disabled={busy || !bootstrap.platform.bundled_asr}
                onClick={downloadAndContinue}
              >
                {installed ? "Continue" : model?.status === "paused" || download?.paused ? "Resume Download" : "Download Model and Continue"}
              </Button>
              <Button className="w-full" variant="secondary" disabled={busy} onClick={continueWithoutModel}>
                Continue Without Model
              </Button>
            </div>
          </aside>
        </div>
      </section>
    </main>
  );
}

function Feature({ icon, title, text }: { icon: React.ReactNode; title: string; text: string }) {
  return (
    <div className="glass rim rounded-lg p-3.5">
      <div className="mb-2.5 text-rust">{icon}</div>
      <h3 className="t-head text-ctl font-semibold text-ink">{title}</h3>
      <p className="t-body mt-1 text-ui text-smoke">{text}</p>
    </div>
  );
}

function Info({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex items-baseline justify-between gap-4">
      <span className="text-smoke">{label}</span>
      <span className="min-w-0 truncate text-right font-medium text-ink" title={value}>{value}</span>
    </div>
  );
}
