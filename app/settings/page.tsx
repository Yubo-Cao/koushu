"use client";

import { Cpu, Download, HardDrive, RefreshCw, RotateCcw, Save } from "lucide-react";
import { useEffect, useState } from "react";
import { Button } from "@/components/Button";
import { DownloadProgress } from "@/components/DownloadProgress";
import { formatBytes } from "@/lib/format";
import {
  downloadModelWithProgress,
  getBootstrap,
  installGpuRuntimeWithProgress,
  pauseModelDownload,
  probeGpuRuntime,
  resetOnboarding,
  setSetting,
} from "@/lib/tauri";
import type {
  Bootstrap,
  GpuRuntimeInfo,
  GpuRuntimeInstallEvent,
  ModelDownloadEvent,
  ModelDownloadState,
  ModelInfo,
} from "@/lib/types";
import { languages } from "@/lib/types";

export default function SettingsPage() {
  const [bootstrap, setBootstrap] = useState<Bootstrap | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [message, setMessage] = useState("");
  const [defaultModel, setDefaultModel] = useState("fun-asr-nano-2512");
  const [defaultLanguage, setDefaultLanguage] = useState("中文");
  const [retainAudio, setRetainAudio] = useState(false);
  const [autoPaste, setAutoPaste] = useState(true);
  const [download, setDownload] = useState<ModelDownloadState | null>(null);
  const [gpuRuntime, setGpuRuntime] = useState<GpuRuntimeInfo | null>(null);
  const [gpuInstalling, setGpuInstalling] = useState(false);
  const [gpuLog, setGpuLog] = useState<string[]>([]);

  useEffect(() => {
    refresh();
  }, []);

  function refresh() {
    getBootstrap()
      .then((data) => {
        setBootstrap(data);
        setDefaultModel(String(data.settings["defaults.model"] || "fun-asr-nano-2512"));
        setDefaultLanguage(String(data.settings["defaults.language"] || "中文"));
        setRetainAudio(data.settings["audio.retain"] === "true");
        setAutoPaste(data.settings["floating.autoPaste"] !== "false");
        void refreshGpuRuntime();
      })
      .catch((error) => setMessage(String(error)));
  }

  async function refreshGpuRuntime() {
    try {
      setGpuRuntime(await probeGpuRuntime());
    } catch (error) {
      setGpuRuntime(null);
      setMessage(String(error));
    }
  }

  function handleDownloadEvent(event: ModelDownloadEvent) {
    if (event.event === "started" || event.event === "progress") {
      setDownload({
        modelId: event.data.modelId,
        active: true,
        paused: false,
        downloadedBytes: event.data.downloadedBytes,
        totalBytes: event.data.totalBytes,
        message: "Downloading model from Hugging Face",
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

  async function install(model: ModelInfo) {
    setBusy(model.id);
    setMessage("Downloading model from Hugging Face.");
    try {
      const updated = await downloadModelWithProgress(model.id, handleDownloadEvent);
      setBootstrap((current) =>
        current
          ? {
              ...current,
              models: current.models.map((item) => (item.id === updated.id ? updated : item)),
            }
          : current,
      );
      setMessage(updated.status === "installed" ? "Model installed." : "Download paused.");
    } catch (error) {
      setMessage(String(error));
    } finally {
      setBusy(null);
    }
  }

  async function pauseDownload(modelId: string) {
    setDownload((current) => (current ? { ...current, message: "Pausing download..." } : current));
    await pauseModelDownload(modelId);
  }

  function handleGpuRuntimeEvent(event: GpuRuntimeInstallEvent) {
    if (event.event === "finished") {
      setGpuRuntime(event.data.runtime);
      setGpuLog((current) => [...current.slice(-5), event.data.runtime.message]);
      return;
    }
    const message = event.event === "error" ? event.data.error : event.data.message;
    setGpuLog((current) => [...current.slice(-5), message]);
  }

  async function installGpuRuntime() {
    setGpuInstalling(true);
    setGpuLog(["Preparing GPU runtime installer."]);
    setMessage("Installing app-managed GPU runtime.");
    try {
      const runtime = await installGpuRuntimeWithProgress(handleGpuRuntimeEvent);
      setGpuRuntime(runtime);
      setMessage(runtime.message);
    } catch (error) {
      const text = String(error);
      setGpuLog((current) => [...current.slice(-5), text]);
      setMessage(text);
    } finally {
      setGpuInstalling(false);
    }
  }

  async function saveSettings() {
    setBusy("settings");
    try {
      const selectedModel = bootstrap?.models.find((model) => model.id === defaultModel);
      await setSetting("defaults.model", defaultModel);
      await setSetting("defaults.language", defaultLanguage);
      await setSetting("defaults.runtime", selectedModel?.backend || "crispasr-gguf-cpu");
      await setSetting("audio.retain", retainAudio ? "true" : "false");
      await setSetting("floating.autoPaste", autoPaste ? "true" : "false");
      setMessage("Settings saved.");
    } catch (error) {
      setMessage(String(error));
    } finally {
      setBusy(null);
    }
  }

  async function showSetupAgain() {
    await resetOnboarding();
    setMessage("Setup will show on next main-window load.");
  }

  if (!bootstrap) {
    return <main className="flex min-h-screen items-center justify-center bg-panel text-sm text-smoke">Loading settings</main>;
  }

  return (
    <main className="min-h-dvh overflow-y-auto bg-panel px-5 py-5 md:px-6">
      <header className="mb-5 flex flex-wrap items-center justify-between gap-3">
        <div className="min-w-0">
          <h1 className="text-2xl font-semibold">Settings</h1>
          <p className="mt-1 text-sm text-smoke">Models, defaults, transcript storage, and Linux paste diagnostics.</p>
        </div>
        <Button icon={<RefreshCw size={16} />} onClick={refresh}>
          Refresh
        </Button>
      </header>

      <div className="grid grid-cols-1 gap-5 xl:grid-cols-[minmax(0,1fr)_330px]">
        <section className="space-y-4">
          <Panel title="Models">
            <div className="divide-y divide-line">
              {bootstrap.models.map((model) => (
                <div key={model.id} className="grid gap-3 py-4 first:pt-0 last:pb-0 md:grid-cols-[minmax(0,1fr)_auto] md:items-center">
                  <div className="min-w-0">
                    <div className="flex items-center gap-2">
                      <HardDrive size={16} className="text-moss" />
                      <h2 className="font-semibold">{model.name}</h2>
                    </div>
                    <p className="mt-1 text-sm text-smoke">{model.repo_id}</p>
                    <p className="mt-1 truncate text-xs text-smoke">{model.local_path}</p>
                    {model.backend === "funasr-vllm-gpu" && !gpuRuntime?.ok ? (
                      <p className="mt-2 text-sm text-[#a43b2e]">
                        Install the GPU runtime first. The AppImage will keep Python, CUDA wheels, PyTorch, and vLLM in app data.
                      </p>
                    ) : null}
                    {model.last_error ? <p className="mt-2 text-sm text-[#a43b2e]">{model.last_error}</p> : null}
                    {download?.modelId === model.id ? (
                      <div className="mt-3 max-w-xl">
                        <DownloadProgress
                          download={download}
                          onPause={download.active ? () => pauseDownload(model.id) : undefined}
                        />
                      </div>
                    ) : null}
                  </div>
                  <div className="flex shrink-0 items-center justify-end gap-3">
                    <span className="text-sm text-smoke">{formatBytes(model.size_bytes)}</span>
                    <Button
                      icon={<Download size={16} />}
                      disabled={busy !== null || model.status === "installed"}
                      onClick={() => install(model)}
                    >
                      {model.status === "installed" ? "Installed" : model.status === "paused" ? "Resume" : model.status}
                    </Button>
                  </div>
                </div>
              ))}
            </div>
          </Panel>

          <Panel title="Defaults">
            <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
              <label className="text-sm">
                <span className="mb-2 block font-medium">Model</span>
                <select
                  className="h-10 w-full rounded-md border border-line bg-paper px-3 outline-none focus:border-cobalt"
                  value={defaultModel}
                  onChange={(event) => setDefaultModel(event.target.value)}
                >
                  {bootstrap.models.map((model) => (
                    <option key={model.id} value={model.id}>
                      {model.name}
                    </option>
                  ))}
                </select>
              </label>
              <label className="text-sm">
                <span className="mb-2 block font-medium">Language</span>
                <select
                  className="h-10 w-full rounded-md border border-line bg-paper px-3 outline-none focus:border-cobalt"
                  value={defaultLanguage}
                  onChange={(event) => setDefaultLanguage(event.target.value)}
                >
                  {languages.map((language) => (
                    <option key={language} value={language}>
                      {language}
                    </option>
                  ))}
                </select>
              </label>
              <Info
                label="Runtime"
                value={bootstrap.models.find((model) => model.id === defaultModel)?.backend || "crispasr-gguf-cpu"}
              />
            </div>
          </Panel>
        </section>

        <aside className="space-y-4">
          <Panel title="GPU Runtime">
            <div className="mb-4 flex items-center gap-2 text-sm">
              <Cpu size={16} className={gpuRuntime?.ok ? "text-moss" : "text-rust"} />
              <span className="font-medium">{gpuRuntime?.ok ? "Ready" : gpuRuntime?.installed ? "Needs attention" : "Not installed"}</span>
            </div>
            <div className="space-y-3">
              <Info label="Driver" value={gpuRuntime?.driver || "unchecked"} />
              <Info label="Python" value={gpuRuntime?.pythonVersion || "managed Python 3.12"} />
              <Info label="CUDA" value={gpuRuntime?.torchCuda ? `torch CUDA ${gpuRuntime.torchCuda}` : gpuRuntime?.driverOk ? "driver detected" : "not detected"} />
              <Info label="Device" value={gpuRuntime?.device || "none"} />
              <Info label="vLLM" value={gpuRuntime?.vllm || "not installed"} />
              <Info label="FunASR" value={gpuRuntime?.funasr || "not installed"} />
            </div>
            {gpuRuntime?.message ? <p className="mt-3 text-sm leading-5 text-smoke">{gpuRuntime.message}</p> : null}
            {gpuLog.length ? (
              <div className="mt-3 rounded-md border border-line bg-panel p-3 text-xs leading-5 text-smoke">
                {gpuLog.map((line, index) => (
                  <p key={`${line}-${index}`}>{line}</p>
                ))}
              </div>
            ) : null}
            <div className="mt-4 grid grid-cols-1 gap-2">
              <Button className="w-full" icon={<RefreshCw size={16} />} disabled={gpuInstalling} onClick={refreshGpuRuntime}>
                Check GPU Runtime
              </Button>
              <Button className="w-full" variant="primary" icon={<Download size={16} />} disabled={gpuInstalling} onClick={installGpuRuntime}>
                {gpuInstalling ? "Installing" : gpuRuntime?.installed ? "Repair GPU Runtime" : "Install GPU Runtime"}
              </Button>
            </div>
          </Panel>

          <Panel title="Storage">
            <Toggle label="Retain audio files" checked={retainAudio} onChange={setRetainAudio} />
            <Toggle label="Floating bar auto-paste" checked={autoPaste} onChange={setAutoPaste} />
            <Button className="mt-4 w-full" variant="primary" icon={<Save size={16} />} disabled={busy !== null} onClick={saveSettings}>
              Save Settings
            </Button>
          </Panel>

          <Panel title="Linux Paste">
            <Info label="Session" value={bootstrap.platform.session_type || "unknown"} />
            <Info label="Wayland" value={bootstrap.platform.wayland_display ? "yes" : "no"} />
            <Info label="X11" value={bootstrap.platform.x11_display ? "yes" : "no"} />
            <Info label="Tools" value={bootstrap.platform.paste_tools.join(", ") || "none"} />
          </Panel>

          <Panel title="Setup">
            <Button className="w-full" icon={<RotateCcw size={16} />} onClick={showSetupAgain}>
              Show Setup Again
            </Button>
          </Panel>

          {message ? <p className="rounded-md border border-line bg-paper p-3 text-sm text-smoke">{message}</p> : null}
        </aside>
      </div>
    </main>
  );
}

function Panel({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="rounded-lg border border-line bg-[#fbfcf8] p-4 shadow-sm">
      <h2 className="mb-4 text-base font-semibold">{title}</h2>
      {children}
    </section>
  );
}

function Toggle({
  label,
  checked,
  onChange,
}: {
  label: string;
  checked: boolean;
  onChange: (value: boolean) => void;
}) {
  return (
    <label className="mb-3 flex items-center justify-between gap-4 text-sm">
      <span>{label}</span>
      <input
        className="h-4 w-4 accent-[#50715f]"
        type="checkbox"
        checked={checked}
        onChange={(event) => onChange(event.target.checked)}
      />
    </label>
  );
}

function Info({ label, value }: { label: string; value: string }) {
  return (
    <div className="mb-3 flex items-start justify-between gap-4 text-sm">
      <span className="text-smoke">{label}</span>
      <span className="max-w-[190px] text-right font-medium">{value}</span>
    </div>
  );
}
