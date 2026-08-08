"use client";

import { Cpu, Download, HardDrive, RefreshCw, RotateCcw, Save } from "lucide-react";
import { useEffect, useState } from "react";
import { Button } from "@/components/Button";
import { DownloadProgress } from "@/components/DownloadProgress";
import { DEFAULT_BACKEND, backendLabel } from "@/lib/backends";
import { formatBytes } from "@/lib/format";
import {
  downloadModelWithProgress,
  getBootstrap,
  getLlmSettings,
  getTrialStatus,
  setCloudAsrApiKey,
  setLlmApiKey,
  pauseModelDownload,
  resetOnboarding,
  setSetting,
} from "@/lib/tauri";
import type {
  Bootstrap,
  LlmSettings,
  ModelDownloadEvent,
  ModelDownloadState,
  ModelInfo,
  TrialStatus,
} from "@/lib/types";
import { languages } from "@/lib/types";

/**
 * Known-good endpoint/model pairs.
 *
 * Groq first: whisper-large-v3-turbo is the cheapest fast option and the one
 * measured here at ~800 ms for 30 s of audio. Ollama needs no key at all,
 * which makes the fully-offline cloud path one click away.
 */
const ASR_PRESETS = [
  {
    label: "Groq",
    baseUrl: "https://api.groq.com/openai/v1",
    model: "whisper-large-v3-turbo",
  },
  {
    label: "OpenAI",
    baseUrl: "https://api.openai.com/v1",
    model: "gpt-4o-transcribe",
  },
  {
    label: "OpenRouter",
    baseUrl: "https://openrouter.ai/api/v1",
    model: "whisper-large-v3-turbo",
  },
  {
    label: "Local (Ollama)",
    baseUrl: "http://localhost:11434/v1",
    model: "whisper",
  },
];

export default function SettingsPage() {
  const [bootstrap, setBootstrap] = useState<Bootstrap | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [message, setMessage] = useState("");
  const [defaultModel, setDefaultModel] = useState("fun-asr-nano-2512");
  const [defaultLanguage, setDefaultLanguage] = useState("中文");
  const [retainAudio, setRetainAudio] = useState(false);
  const [autoPaste, setAutoPaste] = useState(true);
  const [download, setDownload] = useState<ModelDownloadState | null>(null);
  const [llm, setLlm] = useState<LlmSettings | null>(null);
  const [llmBaseUrl, setLlmBaseUrl] = useState("");
  const [llmModel, setLlmModel] = useState("");
  const [llmPreset, setLlmPreset] = useState("typeset");
  // Never populated from the backend; the stored key is write-only from here.
  const [apiKeyDraft, setApiKeyDraft] = useState("");
  const [asrBaseUrl, setAsrBaseUrl] = useState("");
  const [asrModel, setAsrModel] = useState("");
  const [asrLanguage, setAsrLanguage] = useState("");
  const [asrKeyDraft, setAsrKeyDraft] = useState("");
  const [trial, setTrial] = useState<TrialStatus | null>(null);

  useEffect(() => {
    refresh();
  }, []);

  function refresh() {
    getTrialStatus().then(setTrial).catch(() => setTrial(null));
    getLlmSettings()
      .then((settings) => {
        setLlm(settings);
        setLlmBaseUrl(settings.baseUrl);
        setLlmModel(settings.model);
        setLlmPreset(settings.preset);
      })
      .catch(() => setLlm(null));
    getBootstrap()
      .then((data) => {
        setBootstrap(data);
        setDefaultModel(String(data.settings["defaults.model"] || "fun-asr-nano-2512"));
        setDefaultLanguage(String(data.settings["defaults.language"] || "中文"));
        setRetainAudio(data.settings["audio.retain"] === "true");
        setAutoPaste(data.settings["floating.autoPaste"] !== "false");
        setAsrBaseUrl(String(data.settings["asr.cloud.baseUrl"] || ""));
        setAsrModel(String(data.settings["asr.cloud.model"] || ""));
        setAsrLanguage(String(data.settings["asr.cloud.language"] || ""));
      })
      .catch((error) => setMessage(String(error)));
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

  async function saveSettings() {
    setBusy("settings");
    try {
      const selectedModel = bootstrap?.models.find((model) => model.id === defaultModel);
      await setSetting("defaults.model", defaultModel);
      await setSetting("defaults.language", defaultLanguage);
      await setSetting("defaults.runtime", selectedModel?.backend || DEFAULT_BACKEND);
      await setSetting("audio.retain", retainAudio ? "true" : "false");
      await setSetting("floating.autoPaste", autoPaste ? "true" : "false");
      await setSetting("asr.cloud.baseUrl", asrBaseUrl.trim());
      await setSetting("asr.cloud.model", asrModel.trim());
      await setSetting("asr.cloud.language", asrLanguage.trim());
      if (asrKeyDraft.trim()) {
        await setCloudAsrApiKey(asrKeyDraft.trim());
        setAsrKeyDraft("");
      }
      await setSetting("llm.baseUrl", llmBaseUrl.trim());
      await setSetting("llm.model", llmModel.trim());
      await setSetting("llm.preset", llmPreset);
      // Empty draft means "leave the stored key alone"; clearing is explicit.
      if (apiKeyDraft.trim()) {
        await setLlmApiKey(apiKeyDraft.trim());
        setApiKeyDraft("");
      }
      setLlm(await getLlmSettings());
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
    return <main className="flex min-h-screen items-center justify-center text-sm text-smoke">Loading settings</main>;
  }

  return (
    <main className="min-h-dvh overflow-y-auto px-5 py-5 md:px-6">
      <header className="mb-5 flex flex-wrap items-center justify-between gap-3">
        <div className="min-w-0">
          <h1 className="t-display text-[26px] font-semibold">Settings</h1>
          <p className="mt-1.5 text-[13px] text-smoke">Models, defaults, transcript storage, and Linux paste diagnostics.</p>
        </div>
        <Button icon={<RefreshCw size={16} />} onClick={refresh}>
          Refresh
        </Button>
      </header>

      <div className="grid grid-cols-1 gap-5 xl:grid-cols-[minmax(0,1fr)_330px]">
        <section className="space-y-4">
          <Panel title="Models">
            <div className="divide-y divide-line-soft">
              {bootstrap.models.map((model) => (
                <div key={model.id} className="grid gap-3 py-4 first:pt-0 last:pb-0 md:grid-cols-[minmax(0,1fr)_auto] md:items-center">
                  <div className="min-w-0">
                    <div className="flex items-center gap-2">
                      <HardDrive size={16} className="text-moss" />
                      <h2 className="text-[14px] font-semibold">{model.name}</h2>
                    </div>
                    <p className="mt-1 text-[13px] text-smoke">{model.repo_id}</p>
                    <p className="mt-1 truncate font-mono text-[11px] text-faint">{model.local_path}</p>
                    {model.last_error ? <p className="mt-2 text-[13px] text-rust">{model.last_error}</p> : null}
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
                    <span className="tnum text-[12.5px] text-smoke">{formatBytes(model.size_bytes)}</span>
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
              <label className="text-[13px]">
                <span className="t-micro mb-1.5 block text-[11.5px] font-medium text-smoke">Model</span>
                <select
                  className="field h-10 w-full pl-3.5 pr-9 text-[13px]"
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
              <label className="text-[13px]">
                <span className="t-micro mb-1.5 block text-[11.5px] font-medium text-smoke">Language</span>
                <select
                  className="field h-10 w-full pl-3.5 pr-9 text-[13px]"
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
                value={backendLabel(
                  bootstrap.models.find((model) => model.id === defaultModel)?.backend || DEFAULT_BACKEND,
                )}
              />
            </div>
          </Panel>
        </section>

        <aside className="space-y-4">
          <Panel title="Trial">
            {trial?.licensed ? (
              <p className="t-body text-[13px] text-smoke">
                Licensed. Thank you — that genuinely funds this.
              </p>
            ) : trial ? (
              <>
                <div className="mb-2 flex items-baseline justify-between">
                  <span className="t-title text-[15px] font-semibold">
                    {Math.floor(trial.usedSeconds / 60)} min
                  </span>
                  <span className="text-[12px] text-smoke">
                    of {Math.round(trial.limitSeconds / 60)} min
                  </span>
                </div>
                <div className="h-1.5 overflow-hidden rounded-pill bg-fill">
                  <div
                    className="h-full rounded-pill bg-accent transition-all"
                    style={{
                      width: `${Math.min(100, (trial.usedSeconds / trial.limitSeconds) * 100)}%`,
                    }}
                  />
                </div>
                <p className="t-body mt-3 text-[12px] leading-5 text-smoke">
                  Counts speech detected by VAD, not how long you held the key.
                  Local transcription keeps working; this only tracks the free
                  build&rsquo;s budget.
                </p>
              </>
            ) : null}
          </Panel>

          <Panel title="Runtime">
            <div className="mb-4 flex items-center gap-2 text-sm">
              <Cpu size={16} className={bootstrap.platform.bundled_asr ? "text-moss" : "text-rust"} />
              <span className="font-medium">
                {bootstrap.platform.bundled_asr ? "Bundled runtime ready" : "Runtime missing"}
              </span>
            </div>
            <div className="space-y-3">
              <Info label="Engine" value="llama.cpp (official Fun-ASR)" />
              <Info label="Compute" value="CPU only" />
              <Info label="Platform" value={`${bootstrap.platform.os} ${bootstrap.platform.arch}`} />
            </div>
            <p className="t-body mt-3 text-[13px] text-smoke">
              Runs entirely on the CPU. No GPU, no CUDA, and no Python are needed or used.
            </p>
          </Panel>

          <Panel title="Cloud transcription">
            <p className="t-body mb-3 text-[13px] text-smoke">
              Optional. Any OpenAI-compatible <code>/v1/audio/transcriptions</code>{" "}
              endpoint (OpenAI, Groq, or a local whisper server). Select
              &ldquo;Cloud transcription&rdquo; as the model to use it. The local
              model keeps running the live preview either way.
            </p>
            <div className="space-y-3">
              <Field label="Base URL">
                <input
                  className="field h-9 w-full px-3 text-[13px]"
                  placeholder="https://api.groq.com/openai/v1"
                  value={asrBaseUrl}
                  onChange={(event) => setAsrBaseUrl(event.target.value)}
                />
              </Field>
              <Field label="Model">
                <input
                  className="field h-9 w-full px-3 text-[13px]"
                  placeholder="whisper-large-v3-turbo"
                  value={asrModel}
                  onChange={(event) => setAsrModel(event.target.value)}
                />
              </Field>
              <Field label="Language hint (blank = autodetect)">
                <input
                  className="field h-9 w-full px-3 text-[13px]"
                  placeholder="leave blank for code-switched speech"
                  value={asrLanguage}
                  onChange={(event) => setAsrLanguage(event.target.value)}
                />
              </Field>
              <Field label="API key">
                <input
                  type="password"
                  className="field h-9 w-full px-3 text-[13px]"
                  placeholder="Leave blank to keep the stored key"
                  value={asrKeyDraft}
                  onChange={(event) => setAsrKeyDraft(event.target.value)}
                />
              </Field>
            </div>
          </Panel>

          <Panel title="Markdown formatting">
            <p className="t-body mb-3 text-[13px] text-smoke">
              Optional. Any OpenAI-compatible endpoint works, including a local
              server. Transcripts are kept as spoken; the formatted version is
              stored alongside them.
            </p>
            <div className="space-y-3">
              <Field label="Base URL">
                <input
                  className="field h-9 w-full px-3 text-[13px]"
                  placeholder="http://localhost:11434/v1"
                  value={llmBaseUrl}
                  onChange={(event) => setLlmBaseUrl(event.target.value)}
                />
              </Field>
              <Field label="Model">
                <input
                  className="field h-9 w-full px-3 text-[13px]"
                  placeholder="qwen2.5:7b"
                  value={llmModel}
                  onChange={(event) => setLlmModel(event.target.value)}
                />
              </Field>
              <Field label={llm?.hasApiKey ? "API key (stored)" : "API key"}>
                <input
                  type="password"
                  className="field h-9 w-full px-3 text-[13px]"
                  placeholder={llm?.hasApiKey ? "Leave blank to keep" : "Optional for local servers"}
                  value={apiKeyDraft}
                  onChange={(event) => setApiKeyDraft(event.target.value)}
                />
              </Field>
              {llm?.hasApiKey ? (
                <Button
                  className="w-full"
                  onClick={async () => {
                    await setLlmApiKey(null);
                    setLlm(await getLlmSettings());
                    setMessage("API key cleared.");
                  }}
                >
                  Clear stored key
                </Button>
              ) : null}
              <Field label="Preset">
                <select
                  className="field h-9 w-full pl-3.5 pr-9 text-[13px]"
                  value={llmPreset}
                  onChange={(event) => setLlmPreset(event.target.value)}
                >
                  {(llm?.presets || []).map((preset) => (
                    <option key={preset.id} value={preset.id}>
                      {preset.label}
                    </option>
                  ))}
                </select>
              </Field>
              <p className="t-body text-[11.5px] text-smoke">
                {llm?.presets.find((preset) => preset.id === llmPreset)?.description}
              </p>
            </div>
            <p className="t-body mt-3 text-[11.5px] text-smoke">
              The key is stored in the system keychain, not in the app database.
            </p>
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

          {message ? <p className="glass rim rounded-md p-3 text-[13px] text-smoke">{message}</p> : null}
        </aside>
      </div>
    </main>
  );
}

function Panel({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="glass rim rounded-lg p-4">
      <h2 className="t-head mb-4 text-[14.5px] font-semibold">{title}</h2>
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
    <label className="mb-3 flex items-center justify-between gap-4 text-[13px]">
      <span>{label}</span>
      <input
        className="h-4 w-4 accent-accent"
        type="checkbox"
        checked={checked}
        onChange={(event) => onChange(event.target.checked)}
      />
    </label>
  );
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <label className="block">
      <span className="t-micro mb-1.5 block text-[11.5px] font-medium text-smoke">{label}</span>
      {children}
    </label>
  );
}

function Info({ label, value }: { label: string; value: string }) {
  return (
    <div className="mb-2.5 flex items-start justify-between gap-4 text-[13px]">
      <span className="text-smoke">{label}</span>
      <span className="max-w-[190px] text-right font-medium text-ink">{value}</span>
    </div>
  );
}
